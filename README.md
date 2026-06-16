# snproxy

A local REST API proxy for ServiceNow, built on top of the SN Utils browser extension's
internal WebSocket channel.

---

## The problem

ServiceNow instances live behind SSO, session cookies, and IP allowlists. Getting programmatic
access from a local tool — a script, an MCP server, a CI job — usually means fighting through
some combination of:

- OAuth service accounts that need an admin to provision
- REST API keys that expire or get locked to specific IP ranges  
- Basic auth that SSO-only orgs have disabled entirely
- Personal auth tokens that the instance has turned off

Every "simple" path hits a wall that requires someone with elevated access to unblock.
Meanwhile the browser just… works, because you already have a session.

---

## The hack

[SN Utils](https://snutils.com) is a browser extension for ServiceNow power-users. It ships
two pieces that matter here:

1. **The VS Code extension** (`sn-scriptsync`) — syncs ServiceNow scripts to disk. It starts a
   WebSocket server on `ws://127.0.0.1:1978/` so it can talk to the browser.
2. **The Helper Tab** — a special browser tab that the extension keeps open. It connects to the
   WS server and acts as a relay: it has your authenticated browser session and will execute
   whatever commands come through the socket — queries, script runs, field writes, screenshots,
   the lot.

The critical detail: the Helper Tab is running in your browser with your SSO session already
established. It doesn't re-authenticate. It just uses the existing cookies.

The WS server checks the HTTP `Origin` header on upgrade, but only rejects `http://` and
`https://` origins. A local script connecting without an `Origin` header (or with a
`chrome-extension://` origin) passes straight through.

So: **replace the VS Code extension's WS server with our own**. The Helper Tab connects to us
instead. We hold that authenticated socket and front it with a normal HTTP REST API that any
tool can call.

```
┌─────────────────────────────────────┐
│  Chrome — SN Utils Helper Tab       │
│  (authenticated session, all orgs)  │
└──────────────┬──────────────────────┘
               │  ws://127.0.0.1:1978/
               ▼
┌─────────────────────────────────────┐
│  snproxy  (this program)            │
│                                     │
│  WS server  ←→  HTTP REST  :8766   │
└──────────────┬──────────────────────┘
               │  HTTP JSON
               ▼
   curl / MCP server / any tool
```

No tokens. No admin. No IP allowlists. Just the browser session you already have.

---

## Prerequisites

1. Chrome (or any Chromium browser) with the [SN Utils extension](https://snutils.com) installed
2. The Helper Tab open (`snutils helper` from the extension menu)
3. Your ServiceNow instance approved in the Helper Tab (one-time click, persists across restarts)
4. **VS Code with sn-scriptsync must NOT be running** — it would conflict on port 1978

---

## Build

```bash
cargo build --release
```

Binary lands at `target/release/snproxy`. Or with Nix:

```bash
nix build          # output at ./result/bin/snproxy
nix develop        # drop into a shell with cargo, rust-analyzer, websocat, jq
```

---

## Run

```bash
./target/release/snproxy
```

```
snproxy
  WebSocket (Helper Tab) : ws://127.0.0.1:1978
  HTTP REST API          : http://127.0.0.1:8766
  Event stream (SSE)     : http://127.0.0.1:8766/events

Waiting for SN Utils Helper Tab to connect...
```

Open the Helper Tab in Chrome. It will connect automatically (it polls for the WS server).
The banner in the Helper Tab will confirm the connection.

```
--host <HOST>       Bind address [default: 127.0.0.1]
--ws-port <PORT>    WebSocket port [default: 1978]
--port <PORT>       HTTP API port [default: 8766]
--timeout <SECS>    Response wait timeout [default: 30]
```

---

## API overview

All endpoints accept and return JSON. Endpoints that call ServiceNow block until the response
arrives (up to `--timeout` seconds), then return the result directly in the HTTP response.

### Health

```bash
curl http://127.0.0.1:8766/health
# {"status":"ready","helper_tab_connected":true}
```

### Quick examples

```bash
# List open incidents
curl 'http://127.0.0.1:8766/records/incident?instance=dev12345.service-now.com&q=active%3Dtrue&limit=5'

# Run a Glide script and get the output synchronously
curl -X POST http://127.0.0.1:8766/scripts/bg \
  -H 'Content-Type: application/json' \
  -d '{"instance":"dev12345.service-now.com","script":"gs.info(gs.getUserName())"}'

# Proxy any ServiceNow REST call through the browser session
curl -X POST http://127.0.0.1:8766/rest \
  -H 'Content-Type: application/json' \
  -d '{"instance":"dev12345.service-now.com","endpoint":"/api/now/table/sys_user","query_params":{"sysparm_limit":"3"}}'

# Take a screenshot of the active SN tab
curl -X POST http://127.0.0.1:8766/browser/screenshot \
  -H 'Content-Type: application/json' \
  -d '{"instance":"dev12345.service-now.com","url":"incident_list.do"}'

# Stream all raw WebSocket events
curl -N http://127.0.0.1:8766/events
```

---

## API reference

| Area | Doc |
|------|-----|
| Record CRUD + schema (`GET/POST/PATCH/DELETE /records/:table`, `GET /records/:table/schema`) | [docs/records.md](docs/records.md) |
| Background scripts & slash commands (`/scripts/*`) | [docs/scripts.md](docs/scripts.md) |
| Browser-authenticated REST passthrough (`/rest`) | [docs/rest.md](docs/rest.md) |
| Browser automation — forms, navigation, screenshots (`/browser/*`) | [docs/browser.md](docs/browser.md) |
| Context switching — update set, scope, domain (`/context`) | [docs/context.md](docs/context.md) |
| Development artifact creation (`/artifacts`) | [docs/artifacts.md](docs/artifacts.md) |
| Raw WebSocket passthrough & protocol internals (`/raw`, `/events`) | [docs/protocol.md](docs/protocol.md) |

---

## Known limitations

**Port conflict**: snproxy and VS Code's sn-scriptsync both want `:1978`. Run one or the other.

**One active Helper Tab**: the last Helper Tab to connect wins. Multiple tabs work, but only
the most recent connection receives outbound commands.

**`agentRestApi` requires SN Utils Pro** for the browser passthrough endpoints (`/rest`,
and `GET`/`PATCH`/`DELETE /records`). Record listing via `agentQueryRecords` works on the
free tier.
