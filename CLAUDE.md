# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`snproxy` is a Rust daemon that impersonates VS Code's `sn-scriptsync` WebSocket server (port 1978) so the SN Utils browser extension's Helper Tab connects to it instead. The Helper Tab carries the user's authenticated browser session; snproxy fronts it with a local HTTP REST API (port 8766) that any tool can call without OAuth/tokens/IP allowlists.

The workspace contains two crates:
- **`snproxy`** — the daemon (`src/`)
- **`sncli`** — a CLI client that calls the HTTP API (`sncli/`)

## Build commands

```bash
cargo build                  # debug build (both crates)
cargo build --release        # release build
cargo build -p snproxy       # single crate
cargo build -p sncli
cargo check                  # fast type check, no binary
nix build                    # reproducible build → ./result/bin/snproxy
nix develop                  # dev shell with cargo, rust-analyzer, websocat, jq
```

There is no test suite. Manual testing uses `curl`, `websocat`, or `sncli` against a running instance.

## Running

```bash
./target/release/snproxy [--host 127.0.0.1] [--ws-port 1978] [--port 8766] [--timeout 30]
```

Log level is controlled by `RUST_LOG` (default: `info`). Set `RUST_LOG=debug` to see per-message WS frame logs.

**VS Code with sn-scriptsync must NOT be running** — it owns the same port 1978.

Before using any API endpoint that talks to ServiceNow, the user must run `/token` from their SN instance tab, which causes the Helper Tab to send `{instance: {url, name, g_ck}}` over WebSocket. snproxy caches this and uses it for all subsequent calls.

## Architecture

### Core data flow

```
Helper Tab (browser, authenticated) ──WS:1978──► snproxy ──HTTP:8766──► curl/sncli/MCP
```

All HTTP handlers are synchronous from the caller's perspective: they send a message to the Helper Tab and block until the correlated response arrives (or timeout).

### State (`src/state.rs`)

`AppState` (cheaply `Clone`d via `Arc`) holds:
- `ws_tx` — mpsc sender to the active Helper Tab connection (last-connect-wins)
- `pending` — `HashMap<agentRequestId, oneshot::Sender<Value>>` for in-flight calls
- `event_tx` — broadcast channel for the SSE stream (`/events`)
- `sn_instance` — cached `{url, name, g_ck}` from the Helper Tab's `/token` response

Two send primitives:
- `state.call(payload)` — injects a unique `agentRequestId`, registers a oneshot in `pending`, sends, awaits reply
- `state.fire(payload)` — send-and-forget, no correlation

### WebSocket layer (`src/ws.rs`)

On each inbound WS message:
1. If `instance.g_ck` is present → cache it in `sn_instance`
2. If `agentRequestId` is present → resolve the matching oneshot in `pending`
3. Broadcast to SSE (with `g_ck` redacted to `"[redacted]"`)

### HTTP API (`src/api/`)

Each file handles one feature area, registered in `src/api/mod.rs`:

| File | Routes | SN Utils action(s) |
|------|--------|--------------------|
| `health.rs` | `GET /health` | — |
| `records.rs` | `GET/POST/PATCH/DELETE /records/:table[/:sys_id]`, `GET /records/:table/schema` | `agentQueryRecords`, `agentRestApi` |
| `scripts.rs` | `POST /scripts/bg`, `POST /scripts/slash` | `agentRunBackgroundScript`, `runSlashCommand` |
| `rest.rs` | `POST /rest` | `agentRestApi` |
| `browser.rs` | `GET/POST /browser/form`, `/browser/form/action`, `/browser/navigate`, `/browser/click`, `/browser/screenshot`, `/browser/tab` | various `agent*` actions |
| `context.rs` | `PUT /context` | context-switching actions |
| `artifacts.rs` | `POST /artifacts` | artifact creation |
| `events.rs` | `GET /events` | SSE stream (no SN call) |
| `raw.rs` | `POST /raw` | raw WS passthrough |

**SN Utils Pro** is required for `agentRestApi` (used by `/rest` and `GET`/`PATCH`/`DELETE /records`). `agentQueryRecords` (record listing) works on the free tier.

### snstate (`snstate/src/main.rs`)

Terraform-like state management for ServiceNow records. Operates on a local directory tree of JSON files.

**File layout per record:**
```
<dir>/<table>/<sys_id>.json        — editable desired state
<dir>/<table>/<sys_id>.orig.json   — baseline (last pulled / last pushed)
```

**Record file format** — `_meta` block (identity) plus flat field values:
```json
{ "_meta": { "instance": "dev12345.service-now.com", "table": "incident", "sys_id": "..." },
  "short_description": "Something broke", "state": "1" }
```

**Commands:**
- `pull <table> -i <instance> [-q query] [-f fields] [-l limit]` — import records; writes both `.json` and `.orig.json`
- `status` — diff local files vs baseline, no SN connection needed
- `plan [table[/sys_id]] [-i instance]` — fetch live SN state and show what `push` would change; uses the list endpoint (free tier)
- `push [target] [-n] [--force]` — PATCH existing records, POST new ones, update baseline; `--dry-run` / `-n` skips the actual requests
- `apply` — alias for push

New records are created by placing a `.json` file in `<dir>/<table>/` with no `_meta.sys_id` (or no `_meta` at all). After a successful POST, the file is renamed to `<new_sys_id>.json` and `_meta` is populated.

The `push` command skips records whose `.json` matches `.orig.json` unless `--force` is passed.

### sncli (`sncli/src/main.rs`)

Single-file CLI. Normalises instance identifiers (short name → `name.service-now.com`, strips `https://`) via `normalize_instance()`. Instance can be set via `--instance/-i` or `SNPROXY_INSTANCE` env var. Server URL via `--server` or `SNPROXY_URL` (default: `http://localhost:8766`).
