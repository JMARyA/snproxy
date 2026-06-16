# Artifacts API

Create ServiceNow development artifacts through the Helper Tab.

`POST /artifacts` uses the `createRecord` WS action, which is specifically designed for
development artifacts (Script Includes, Business Rules, UI Scripts, etc.).  It adds the new
record to the active update set and opens it in the browser editor.

For general-purpose record creation (incidents, tasks, users, custom tables) use
[`POST /records/:table`](records.md#create-a-record) instead, which goes through `agentRestApi`
and works on any table.  For table schema inspection use
[`GET /records/:table/schema`](records.md#inspect-table-schema).

---

## Create an artifact

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

### Common artifact tables

| Table                | Description               |
|----------------------|---------------------------|
| `sys_script_include` | Script Includes           |
| `sys_ui_script`      | UI Scripts                |
| `sys_script`         | Business Rules            |
| `sys_ui_action`      | UI Actions                |
| `sys_ws_operation`   | Scripted REST Operations  |
| `sys_atf_test`       | ATF Test definitions      |
