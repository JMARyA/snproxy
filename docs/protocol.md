# WebSocket Protocol Reference

snproxy impersonates the `sn-scriptsync` VS Code extension by binding a WebSocket server at `ws://127.0.0.1:1978/`.  The SN Utils browser extension connects to this port (it has no way to distinguish snproxy from the real sn-scriptsync).  Once connected, snproxy gains an authenticated, bidirectional channel into the user's active ServiceNow browser session.

---

## Connection handshake

When the Helper Tab connects, snproxy sends two messages to initialise the extension:

```json
["Connected to snproxy"]
```

```json
{
  "action": "bannerMessage",
  "message": "snproxy active — REST API on http://127.0.0.1:8766",
  "class": "alert alert-primary"
}
```

These mirror what the real sn-scriptsync sends so the extension's UI shows a "connected" state.

---

## Request correlation

Every message snproxy sends to the Helper Tab includes an `agentRequestId` field.  The browser echoes this ID back in its response.  snproxy uses this to route responses to the correct waiting caller (per-request oneshot channels) rather than relying on fragile FIFO ordering.

```json
{
  "action": "agentQueryRecords",
  "instance": "dev12345.service-now.com",
  "tableName": "incident",
  "queryString": "sysparm_fields=sys_id,short_description&sysparm_limit=10&sysparm_query=active=true",
  "agentRequestId": "sp_42"
}
```

The Helper Tab response:
```json
{
  "action": "agentQueryRecordsResponse",
  "agentRequestId": "sp_42",
  "records": [ ... ],
  "count": 3
}
```

---

## Actions reference

### `agentQueryRecords`

Query records from any table.

| field         | description                                                                  |
|---------------|------------------------------------------------------------------------------|
| `tableName`   | Table name (e.g. `incident`)                                                 |
| `queryString` | `sysparm_*` parameters joined with `&`, e.g. `sysparm_fields=sys_id&sysparm_limit=20&sysparm_query=active=true` |

Response action: `agentQueryRecordsResponse`

---

### `agentRestApi`

Proxy any ServiceNow REST call through the browser session.

| field         | description                                           |
|---------------|-------------------------------------------------------|
| `method`      | HTTP method: `GET POST PUT PATCH DELETE`               |
| `endpoint`    | ServiceNow API path, e.g. `/api/now/table/incident`   |
| `body`        | Request body (object, for POST/PUT/PATCH)             |
| `queryParams` | Query parameters (object)                             |
| `appName`     | Identifies the caller in SN Utils logs                |

Response: same `agentRequestId` echoed back with `status` and `data`.

> Requires **SN Utils Pro**.

---

### `agentRunBackgroundScript`

Run a server-side Glide script and return the captured output.  **Blocking** — the response arrives only after the script completes.

| field    | description                         |
|----------|-------------------------------------|
| `script` | Full JavaScript to execute on the server |

Response: `{ "success": true, "output": "*** Script: ...\n" }`

> Requires `background_script_execute` role.

---

### `createRecord`

Create a new record and open it in the browser.

| field       | description                                     |
|-------------|-------------------------------------------------|
| `tableName` | Target table                                    |
| `scope`     | Application scope sys_id or name (default `global`) |
| `payload`   | Field values as an object; `name` is required   |

Response: `{ "success": true, "newRecord": { "sys_id": "...", "name": "...", "url": "..." } }`

---

### `requestTableStructure`

Return field metadata for a table.

| field       | description  |
|-------------|--------------|
| `tableName` | Table to inspect |

Response: `{ "fields": [ { "name": "...", "type": "...", "label": "...", "mandatory": true } ] }`

---

### `switchContext`

Switch update set, application scope, or domain.

| field        | description                                          |
|--------------|------------------------------------------------------|
| `switchType` | `"updateset"` \| `"application"` \| `"domain"`       |
| `value`      | Name or `sys_id` of the target context               |
| `reloadTab`  | Whether to reload the active tab after switching     |

---

### `runSlashCommand`

Execute an SN Utils slash command in the Helper Tab.

| field     | description                              |
|-----------|------------------------------------------|
| `command` | Full slash command, e.g. `/token`        |
| `autoRun` | Submit automatically without user input  |
| `url`     | URL pattern to restrict to a specific tab |
| `tabId`   | Target a specific browser tab            |

---

### Browser automation actions

| action               | description                                               |
|----------------------|-----------------------------------------------------------|
| `agentGetFormState`  | Read live field values from the active SN form            |
| `agentSetField`      | Set a field via `g_form.setValue()` (fires client scripts)|
| `agentRunUiAction`   | Click a UI action button                                  |
| `agentNavigate`      | Navigate a tab to a URL                                   |
| `agentClickElement`  | Click a DOM element by CSS selector                       |
| `takeScreenshot`     | Capture a tab as a PNG; response contains base64 imageData|
| `activateTab`        | Focus or open a browser tab by URL pattern                |

---

## Raw passthrough

Use `POST /raw` to send any JSON payload directly to the Helper Tab and receive the correlated response.  Useful for actions not covered by the higher-level endpoints or for experimentation.

```json
{
  "action": "someUndocumentedAction",
  "instance": "dev12345.service-now.com",
  "customParam": "value"
}
```

## Event stream

Subscribe to `GET /events` (Server-Sent Events) to receive every message the Helper Tab sends, in real time.  This is useful for monitoring async output from fire-and-forget actions or for debugging.

```bash
curl -N http://127.0.0.1:8766/events
```
