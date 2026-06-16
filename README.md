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
The banner in the Helper Tab will update to confirm.

Available flags:

```
--host <HOST>            Bind address [default: 127.0.0.1]
--ws-port <WS_PORT>      WebSocket port [default: 1978]
--http-port <HTTP_PORT>  HTTP API port [default: 8766]
--timeout <TIMEOUT>      Response wait timeout in seconds [default: 30]
```

---

## HTTP API

All endpoints accept and return JSON. Endpoints that trigger ServiceNow queries block until
the response arrives (up to `--timeout` seconds). Endpoints that write or run scripts return
immediately with `{"status":"sent"}` — watch the [event stream](#event-stream-sse) for output.

### `GET /health`

Check whether a Helper Tab is connected.

```bash
curl http://127.0.0.1:8766/health
```

```json
{"status":"ready","helper_tab_connected":true}
```

---

### `POST /query`

Query any ServiceNow table. Blocks until the response arrives.

```bash
curl -X POST http://127.0.0.1:8766/query \
  -H 'Content-Type: application/json' \
  -d '{
    "instance": "dev12345",
    "table": "sys_user",
    "encoded_query": "active=true^last_name=Smith"
  }'
```

`encoded_query` is a standard ServiceNow encoded query string (the `sysparm_query` parameter
you see in URLs). The alias `"query"` is also accepted.

Response is the raw `agentQueryRecordsResponse` message from the Helper Tab.

---

### `POST /bg`

Run a background script (Glide server-side JavaScript) on the instance.
Returns immediately. Output comes back as an event — pipe the [event stream](#event-stream-sse)
to see it.

```bash
curl -X POST http://127.0.0.1:8766/bg \
  -H 'Content-Type: application/json' \
  -d '{
    "instance": "dev12345",
    "code": "gs.info(gs.getUserName())"
  }'
```

---

### `POST /update`

Write a field value on an existing record.

```bash
curl -X POST http://127.0.0.1:8766/update \
  -H 'Content-Type: application/json' \
  -d '{
    "instance": "dev12345",
    "table": "sys_script_include",
    "sys_id": "abc123...",
    "field": "script",
    "content": "var MyUtil = Class.create();\nMyUtil.prototype = {};"
  }'
```

---

### `POST /slash`

Run any SN Utils slash command. Useful for `/token`, `/tn`, `/bg`, etc.

```bash
curl -X POST http://127.0.0.1:8766/slash \
  -H 'Content-Type: application/json' \
  -d '{
    "instance": "dev12345",
    "command": "/token"
  }'
```

---

### `POST /screenshot`

Capture a screenshot of a ServiceNow page. Blocks until the image is returned.
Optionally navigate to a URL first.

```bash
curl -X POST http://127.0.0.1:8766/screenshot \
  -H 'Content-Type: application/json' \
  -d '{
    "instance": "dev12345",
    "url": "/now/nav/ui/classic/params/target/sys_script_include_list.do"
  }'
```

Response includes base64 image data from the Helper Tab.

---

### `POST /switch`

Switch update set, application scope, or domain on the connected instance.

```bash
curl -X POST http://127.0.0.1:8766/switch \
  -H 'Content-Type: application/json' \
  -d '{
    "instance": "dev12345",
    "switch_type": "updateSet",
    "value": "My Update Set"
  }'
```

`switch_type`: `"updateSet"` | `"scope"` | `"domain"`

---

### `POST /command`

Raw JSON passthrough. Send any WebSocket action directly. For actions with a known synchronous
response (`agentQueryRecords`, `takeScreenshot`, `createArtifact`), blocks and returns the
response. Everything else is fire-and-forget.

```bash
curl -X POST http://127.0.0.1:8766/command \
  -H 'Content-Type: application/json' \
  -d '{
    "action": "agentQueryRecords",
    "instance": "dev12345",
    "table": "sys_atf_test",
    "encodedQuery": "active=true"
  }'
```

---

### `GET /events` — Event stream (SSE)

Every message received from the Helper Tab is broadcast here as a Server-Sent Event.
This is how you observe output from fire-and-forget operations (background scripts,
field writes, slash commands).

```bash
curl -N http://127.0.0.1:8766/events
```

```
data: {"action":"bannerMessage","message":"Script executed","class":"alert alert-success"}
data: {"action":"agentQueryRecordsResponse","records":[...]}
```

In another terminal, run a background script and watch the output arrive:

```bash
curl -X POST http://127.0.0.1:8766/bg \
  -H 'Content-Type: application/json' \
  -d '{"instance":"dev12345","code":"gs.info(new GlideRecord(\"sys_user\").getRowCount())"}'
```

---

## Known limitations

**Concurrent requests of the same type**: requests are correlated to responses by action type
using a FIFO queue. Two simultaneous `/query` calls against the same instance will get their
responses matched in order. Interleaving queries to different tables from concurrent callers
could theoretically mismatch — in practice this isn't an issue for sequential tooling.

**Port conflict**: snproxy and VS Code's sn-scriptsync both want `:1978`. Run one or the other.

**One Helper Tab**: the last Helper Tab to connect wins. Multiple tabs work, but only the most
recent connection receives outbound commands.

---

## WS protocol reference

Outbound actions snproxy can send to the Helper Tab:

| Action | Description |
|--------|-------------|
| `runSlashCommand` | Execute an SN Utils slash command (`/bg`, `/token`, `/tn`, …) |
| `agentQueryRecords` | Run an encoded query against any table |
| `saveFieldAsFile` | Write a field value to an existing record |
| `takeScreenshot` | Capture a page (navigates if `url` given) |
| `uploadAttachment` | Attach a file (disk or base64) to a record |
| `switchContext` | Switch update set, scope, or domain |
| `createArtifact` | Create a new record from a fields payload |

Inbound actions the Helper Tab sends back:

| Action | Description |
|--------|-------------|
| `agentQueryRecordsResponse` | Query results |
| `screenshotResponse` | Screenshot data |
| `createRecordResponse` | Artifact creation result |
| `saveFieldAsFile` | Script content synced FROM ServiceNow |
| `saveWidget` | Widget files synced from ServiceNow |

All events appear on the SSE stream regardless of whether an HTTP call is waiting for them.
