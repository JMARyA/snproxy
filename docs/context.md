# Context Switching

Switch the active update set, application scope, or domain on the connected ServiceNow instance.

---

## Switch context

```
PUT /context
Content-Type: application/json
```

```json
{
  "instance": "dev12345.service-now.com",
  "type": "updateset",
  "value": "My Update Set",
  "reload_tab": true
}
```

| field        | required | default | description                                                  |
|--------------|----------|---------|--------------------------------------------------------------|
| `instance`   | yes      | —       | ServiceNow hostname                                          |
| `type`       | yes      | —       | `"updateset"` \| `"application"` \| `"domain"`              |
| `value`      | yes      | —       | Name or `sys_id` of the target update set / scope / domain   |
| `reload_tab` | no       | `true`  | Reload the active SN tab after switching                     |

**Response**
```json
{
  "switched": true,
  "type": "updateset",
  "value": "My Update Set",
  "reloaded": true
}
```

**Error (invalid type)**
```
HTTP 400
{ "error": "type must be one of: updateset, application, domain" }
```

---

## Examples

**Switch update set by name**
```json
{ "instance": "dev12345.service-now.com", "type": "updateset", "value": "Sprint 14 Changes" }
```

**Switch application scope**
```json
{ "instance": "dev12345.service-now.com", "type": "application", "value": "x_myco_myapp", "reload_tab": false }
```

**Switch domain**
```json
{ "instance": "dev12345.service-now.com", "type": "domain", "value": "TOP" }
```
