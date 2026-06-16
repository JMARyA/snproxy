# Scripts API

Execute server-side Glide scripts and SN Utils slash commands through the connected Helper Tab.

---

## Run a background script (blocking)

```
POST /scripts/bg
Content-Type: application/json
```

Uses `agentRunBackgroundScript` — a fully correlated call that **blocks** until ServiceNow finishes executing the script and returns the captured output.  This is not fire-and-forget.

```json
{
  "instance": "dev12345.service-now.com",
  "script": "gs.info('Hello from snproxy'); var gr = new GlideRecord('incident'); gr.setLimit(1); gr.query(); gs.info(gr.getRowCount());"
}
```

**Response**

The raw ServiceNow HTML response is automatically parsed: the `*** Script:` prefix, `<BR/>` tags,
and HTML entities (`&quot;`, `&lt;`, etc.) are stripped, and only the clean output lines are
returned. An additional `lines` array provides each line individually.

```json
{
  "executed": true,
  "output": "Hello from snproxy\n1",
  "lines": [
    "Hello from snproxy",
    "1"
  ]
}
```

**Broader examples**

Query records and build a report:
```bash
curl -X POST http://127.0.0.1:8766/scripts/bg \
  -H 'Content-Type: application/json' \
  -d '{
    "instance": "dev12345.service-now.com",
    "script": "var gr = new GlideRecord(\"incident\"); gr.setLimit(5); gr.addActiveQuery(); gr.query(); gs.info(\"Active: \" + gr.getRowCount()); while (gr.next()) { gs.print(gr.number + \" | \" + gr.short_description); }"
  }'
```

Export structured data as JSON:
```bash
curl -X POST http://127.0.0.1:8766/scripts/bg \
  -H 'Content-Type: application/json' \
  -d '{
    "instance": "dev12345.service-now.com",
    "script": "var items = []; var gr = new GlideRecord(\"incident\"); gr.setLimit(3); gr.query(); while (gr.next()) { items.push({number: gr.getDisplayValue(\"number\"), state: gr.getDisplayValue(\"state\") }); } gs.info(JSON.stringify(items));"
  }'
```

Error output is preserved with stack traces:
```json
{
  "executed": true,
  "output": "before\nScript execution error: \"undefinedVar\" is not defined.\n   script : Line(3) ...",
  "lines": ["before", "Script execution error: \"undefinedVar\" is not defined.", ...]
}
```

On failure (syntax error, Glide exception, etc.) the endpoint returns HTTP 502 with `{ "error": "<message>" }`.

> **Note:** ServiceNow requires the `background_script_execute` role to run background scripts.

---

## Run a slash command

```
POST /scripts/slash
Content-Type: application/json
```

Sends a slash command to the SN Utils Helper Tab.  Most slash commands have side effects in the browser (e.g. `/token`, `/tn`, `/nav`).

```json
{
  "instance": "dev12345.service-now.com",
  "command": "/token",
  "auto_run": true
}
```

| field      | required | default | description                                              |
|------------|----------|---------|----------------------------------------------------------|
| `instance` | yes      | —       | ServiceNow hostname                                      |
| `command`  | yes      | —       | Full slash command, e.g. `/tn my_update_set`             |
| `url`      | no       | —       | URL pattern to restrict which tab receives the command   |
| `tab_id`   | no       | —       | Target specific browser tab by ID                        |
| `auto_run` | no       | `true`  | Submit the command automatically without user input      |

**Response**
```json
{ "executed": true, "command": "/token", "tab_id": 12, "auto_run": true }
```

### Common slash commands

| Command              | Description                                |
|----------------------|--------------------------------------------|
| `/token`             | Copy the current session token             |
| `/tn <name>`         | Switch to update set by name               |
| `/bg <script>`       | Run a background script (fire-and-forget)  |
| `/nav <url>`         | Navigate to a relative URL                 |
| `/tech`              | Open Technical Support URL                 |
