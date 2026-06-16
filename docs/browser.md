# Browser Automation API

Control the browser tab where the SN Utils Helper Tab is open.  These endpoints let you read form state, set field values (through the real `g_form` API so client scripts fire), trigger UI actions, navigate, click elements, capture screenshots, and manage tabs.

---

## Read form state

```
GET /browser/form
```

| param      | required | description                                      |
|------------|----------|--------------------------------------------------|
| `instance` | yes      | ServiceNow hostname                              |
| `url`      | no       | URL pattern to match (narrows to a specific tab) |
| `tab_id`   | no       | Target specific tab by numeric ID                |
| `fields`   | no       | Comma-separated field names; all fields if omitted |

**Response**
```json
{
  "table": "incident",
  "sys_id": "abc123",
  "is_new_record": false,
  "fields": {
    "short_description": { "value": "Printer on fire", "displayValue": "Printer on fire" },
    "state": { "value": "1", "displayValue": "New" }
  }
}
```

---

## Set a field value

```
POST /browser/form
Content-Type: application/json
```

Sets a field through `g_form.setValue()`, which fires onChange client scripts.

```json
{
  "instance": "dev12345.service-now.com",
  "field": "short_description",
  "value": "Updated by snproxy",
  "url": "nav_to.do?uri=incident.do",
  "tab_id": 12
}
```

For reference fields, pass `display_value` alongside `value`:
```json
{
  "instance": "dev12345.service-now.com",
  "field": "assigned_to",
  "value": "abc123",
  "display_value": "Jane Smith"
}
```

**Response**
```json
{ "set": true, "field": "short_description", "value": "Updated by snproxy" }
```

---

## Trigger a UI action

```
POST /browser/form/action
Content-Type: application/json
```

Clicks a UI action button (save, submit, or any custom verb).

```json
{
  "instance": "dev12345.service-now.com",
  "ui_action": "sysverb_save",
  "suppress_dialogs": true,
  "url": "nav_to.do?uri=incident.do"
}
```

| field              | required | default | description                              |
|--------------------|----------|---------|------------------------------------------|
| `instance`         | yes      | —       | ServiceNow hostname                      |
| `ui_action`        | yes      | —       | `"save"`, `"submit"`, `"sysverb_*"`, or custom name |
| `suppress_dialogs` | no       | `true`  | Auto-dismiss confirmation dialogs        |
| `url`              | no       | —       | URL pattern to target a tab              |
| `tab_id`           | no       | —       | Target specific tab by ID                |

**Response**
```json
{ "triggered": true, "ui_action": "sysverb_save" }
```

---

## Navigate a tab

```
POST /browser/navigate
Content-Type: application/json
```

```json
{
  "instance": "dev12345.service-now.com",
  "url": "/now/nav/ui/classic/params/target/incident.do%3Fsys_id%3Dabc123",
  "wait_for_load": true,
  "discard_unsaved": true
}
```

| field             | required | default | description                                |
|-------------------|----------|---------|--------------------------------------------|
| `instance`        | yes      | —       | ServiceNow hostname                        |
| `url`             | yes      | —       | Absolute or relative URL                   |
| `tab_id`          | no       | —       | Target tab; opens new tab if not found     |
| `new_tab`         | no       | `false` | Open in a new browser tab                  |
| `wait_for_load`   | no       | `true`  | Block until the page finishes loading      |
| `discard_unsaved` | no       | `true`  | Discard unsaved form changes automatically |

**Response**
```json
{ "navigated": true, "tab_id": 12, "url": "...", "title": "Incident INC0001234" }
```

---

## Click an element

```
POST /browser/click
Content-Type: application/json
```

```json
{
  "instance": "dev12345.service-now.com",
  "selector": "#sysverb_update",
  "suppress_dialogs": true
}
```

**Response**
```json
{ "clicked": true, "selector": "#sysverb_update" }
```

---

## Take a screenshot

```
POST /browser/screenshot
Content-Type: application/json
```

```json
{
  "instance": "dev12345.service-now.com",
  "url": "incident.do",
  "tab_id": 12
}
```

`url` or `tab_id` is required.

**Response**
```json
{
  "image_data": "<base64-encoded PNG>",
  "url": "https://dev12345.service-now.com/incident.do",
  "tab_id": 12,
  "tab_title": "Incident INC0001234"
}
```

---

## Activate / open a tab

```
POST /browser/tab
Content-Type: application/json
```

Focuses an existing tab matching the URL, or opens a new one.

```json
{
  "instance": "dev12345.service-now.com",
  "url": "incident.do?sys_id=abc123",
  "open_if_not_found": true,
  "reload": false
}
```

**Response**
```json
{ "tab_id": 12, "url": "...", "title": "...", "opened": false, "reloaded": false }
```
