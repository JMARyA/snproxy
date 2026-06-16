# Artifacts API

Create ServiceNow records through the Helper Tab and inspect table schemas.

The `/artifacts` endpoint is a convenience wrapper around `createRecord`.  For general-purpose record creation use [`POST /records/:table`](records.md#create-a-record).  The key difference here is that `createRecord` opens the record in the browser after creation, which can be useful for development workflows.

---

## Create an artifact (record)

```
POST /artifacts
Content-Type: application/json
```

```json
{
  "instance": "dev12345.service-now.com",
  "table": "sys_script_include",
  "scope": "x_myco_myapp",
  "fields": {
    "name": "MyUtils",
    "api_name": "x_myco_myapp.MyUtils",
    "script": "var MyUtils = Class.create();\nMyUtils.prototype = { type: 'MyUtils' };",
    "active": "true",
    "access": "public"
  }
}
```

| field      | required | default    | description                          |
|------------|----------|------------|--------------------------------------|
| `instance` | yes      | —          | ServiceNow hostname                  |
| `table`    | yes      | —          | Target table name                    |
| `scope`    | no       | `"global"` | Application scope for the new record |
| `fields`   | yes      | —          | Field values; `name` is required     |

**Response**
```json
{
  "sys_id": "abc123",
  "name": "MyUtils",
  "table": "sys_script_include",
  "scope": "x_myco_myapp",
  "url": "https://dev12345.service-now.com/sys_script_include.do?sys_id=abc123"
}
```

---

## Get table field metadata

Inspect the schema of any table — useful for discovering field names and types before creating or updating records.

```
GET /artifacts/metadata?instance=dev12345.service-now.com&table=sys_script_include
```

| param      | required | description         |
|------------|----------|---------------------|
| `instance` | yes      | ServiceNow hostname |
| `table`    | yes      | Table name to inspect |

**Response**
```json
{
  "table": "sys_script_include",
  "fields": [
    { "name": "name",    "type": "string",  "label": "Name",    "mandatory": true  },
    { "name": "script",  "type": "script",  "label": "Script",  "mandatory": false },
    { "name": "active",  "type": "boolean", "label": "Active",  "mandatory": false }
  ]
}
```

### Common tables for script development

| Table                    | Description                        |
|--------------------------|------------------------------------|
| `sys_script_include`     | Script Includes                    |
| `sys_ui_script`          | UI Scripts                         |
| `sys_script`             | Business Rules                     |
| `sys_ui_action`          | UI Actions                         |
| `sys_ws_operation`       | Scripted REST Operations           |
| `sys_flow_context`       | Flow Designer contexts             |
| `sys_atf_test`           | ATF Test definitions               |
| `sys_update_set`         | Update Sets                        |
