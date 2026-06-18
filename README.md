# sn-tools

A suite of local tools for ServiceNow, built on top of the SN Utils browser extension's
authenticated WebSocket channel — no OAuth, no API keys, no IP allowlist changes required.

| Tool | What it is |
|------|-----------|
| **snproxy** | Daemon that holds the browser session and exposes a local HTTP REST API |
| **sncli** | CLI for scripts, record CRUD, and schema queries |
| **sntui** | Terminal UI — browse tables, drill into records, follow references |
| **snstate** | Terraform-like state management: pull records to disk, push changes back |

---

## The problem

ServiceNow instances live behind SSO, session cookies, and IP allowlists. Getting programmatic
access from a local tool usually means fighting through OAuth service accounts, expiring API
keys, or basic auth that SSO-only orgs have disabled. Every "simple" path hits a wall that
requires someone with elevated access to unblock. Meanwhile the browser just works — because
you already have a session.

## How it works

[SN Utils](https://snutils.com) ships a Helper Tab — a browser tab that connects to VS Code's
`sn-scriptsync` on `ws://127.0.0.1:1978` and relays authenticated ServiceNow commands through
your existing session. **snproxy replaces the VS Code WS server**. The Helper Tab connects to
snproxy instead; snproxy holds that socket and fronts it with a local HTTP REST API.

```
┌──────────────────────────────────────────┐
│  Browser — SN Utils Helper Tab           │
│  (your authenticated session, all orgs)  │
└─────────────────┬────────────────────────┘
                  │  ws://127.0.0.1:1978
                  ▼
┌──────────────────────────────────────────┐
│  snproxy                                 │
│  WS server  ←→  HTTP REST :8766         │
└─────────────────┬────────────────────────┘
                  │  HTTP JSON
          ┌───────┴────────┐
         sncli           sntui
         snstate          curl / MCP
```

No tokens. No admin. No IP allowlists.

---

## Prerequisites

1. Chromium browser with [SN Utils](https://snutils.com) installed
2. Helper Tab open (from the extension menu)
3. **VS Code `sn-scriptsync` must NOT be running** — it owns the same port 1978

---

## Build & install

```bash
cargo build --release          # all four binaries → target/release/
nix build .#snproxy            # reproducible Nix build
nix build .#sncli
nix build .#sntui
nix build .#snstate
nix develop                    # dev shell with cargo, rust-analyzer, websocat, jq
```

A NixOS module is included — `services.snproxy.enable = true` installs and starts everything.

---

## snproxy

The daemon. Run it once; everything else talks to it.

```bash
snproxy [--host 127.0.0.1] [--ws-port 1978] [--port 8766] [--timeout 30]
```

Open the Helper Tab in your browser — it connects automatically. Once connected, run `/token`
from any SN instance tab to register that instance.

`RUST_LOG=debug` logs every WS frame. `RUST_LOG=trace` logs full raw payloads.

**API surface** (all endpoints block until the SN response arrives):

| Route | What it does |
|-------|-------------|
| `GET /health` | Connection status |
| `GET /records/:table` | Query records (`?q=`, `?fields=`, `?limit=`, `?display_value=`) |
| `GET /records/:table/:sys_id` | Fetch one record (`?display_value=false\|true\|all`) |
| `POST/PATCH/DELETE /records/:table[/:sys_id]` | Create / update / delete |
| `GET /records/:table/schema` | Table column metadata |
| `POST /scripts/bg` | Run a server-side Glide script, returns parsed output |
| `POST /scripts/slash` | Run an SN Utils slash command |
| `POST /rest` | Proxy any SN REST call through the browser session |
| `GET/POST /browser/form` | Read / set form fields in the active tab |
| `POST /browser/navigate` | Navigate a tab to a URL |
| `POST /browser/screenshot` | Capture a tab as PNG (base64) |
| `GET /events` | SSE stream of all inbound WS messages |

> `agentRestApi` (used by `/rest` and `GET`/`PATCH`/`DELETE /records`) requires **SN Utils Pro**.
> Record listing via `agentQueryRecords` works on the free tier.

---

## sncli

Single-binary CLI. Instance via `--instance`/`-i` or `$SNPROXY_INSTANCE`.

```bash
# Background scripts
sncli scripts bg -i dev12345 -s 'gs.info(gs.getUserName())'
sncli scripts bg -i dev12345 -f myscript.js --json   # structured JSON output

# Records
sncli records list incident -i dev12345 -q 'active=true' -l 10
sncli records get  incident <sys_id> -i dev12345
sncli records schema sys_user -i dev12345             # column metadata

# Slash commands
sncli scripts slash -i dev12345 -c /token
```

---

## sntui

Interactive terminal UI. Keyboard-driven table browser with live data from snproxy.

```bash
sntui [--port 8766]
```

| Key | Action |
|-----|--------|
| `↑`/`↓` or `j`/`k` | Move cursor |
| `Enter` | Open record / follow reference field |
| `Esc` / `q` | Back (unwinds reference chain step by step) |
| `f` | Filter |
| `r` | Refresh |
| `s` | Script runner overlay |
| `g` / `G` | Jump to top / bottom |

The detail view fetches with `display_value=all`: reference fields show their display name
(cyan) instead of a raw sys_id. Cursor a reference field and press Enter to open that record —
Esc walks back up the chain, with `[Esc: back ×N]` in the title showing depth. Schema is
loaded automatically and cached to `~/.cache/snproxy/schema/` (1-hour TTL) so column labels
appear as headers instead of raw field names.

---

## snstate

Manages ServiceNow records as local TOML files — pull them down, edit, push changes back.

```bash
snstate pull incident -i dev12345 -q 'active=true' -d ./state
snstate status                         # diff local vs baseline, no network needed
snstate plan   -i dev12345 -d ./state  # show what push would change (live SN fetch)
snstate push   -i dev12345 -d ./state  # PATCH changed records, POST new ones
snstate push   --dry-run               # preview without sending
```

**File layout** — one TOML file per record:

```
state/
  incident/
    <sys_id>.toml        ← editable desired state
    <sys_id>.state.toml  ← baseline (last pulled / last pushed)
```

Each file has a `[_meta]` block (instance, table, sys_id) followed by flat field values.
`push` skips records whose `.toml` matches `.state.toml` unless `--force` is passed. New
records are created by dropping a `.toml` without a `sys_id` in `_meta`; after a successful
POST the file is renamed to `<new_sys_id>.toml`.

---

## Known limitations

**Port conflict**: snproxy and VS Code's sn-scriptsync both want `:1978`. Run one or the other.

**One active Helper Tab**: the last tab to connect wins. Multiple tabs work but only the most
recent receives commands.
