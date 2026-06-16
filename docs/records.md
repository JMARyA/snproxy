# Records API

CRUD operations against any ServiceNow table. Queries go through `agentQueryRecords`; single-record reads, updates, and deletes go through `agentRestApi` (browser-authenticated REST); creates use the `createRecord` action.

---

## List records

```
GET /records/:table
```

**Query params**

| param      | required | default                                       | description                                            |
|------------|----------|-----------------------------------------------|--------------------------------------------------------|
| `instance` | yes      | —                                             | ServiceNow hostname, e.g. `dev12345.service-now.com`   |
| `q`        | no       | `""`                                          | SN encoded query, e.g. `active=true^category=software` |
| `fields`   | no       | `sys_id,name,sys_created_on,sys_updated_on`   | Comma-separated field names                            |
| `limit`    | no       | `20`                                          | Max records to return                                  |
| `order_by` | no       | `""`                                          | Appended to query, e.g. `ORDERBYname`                  |

**Response**
```json
{ "table": "incident", "count": 3, "records": [ ... ] }
```

---

## Get one record

```
GET /records/:table/:sys_id
```

| param      | required | description                           |
|------------|----------|---------------------------------------|
| `instance` | yes      | ServiceNow hostname                   |
| `fields`   | no       | Comma-separated field names; all if omitted |

**Response**
```json
{ "table": "incident", "sys_id": "abc123", "record": { ... } }
```

---

## Create a record

```
POST /records/:table
Content-Type: application/json
```

```json
{
  "instance": "dev12345.service-now.com",
  "scope": "global",
  "fields": {
    "name": "My script include",
    "script": "var MyClass = Class.create(); ...",
    "active": "true"
  }
}
```

`scope` defaults to `"global"`.  `fields.name` is required.

**Response**
```json
{ "sys_id": "abc123", "name": "My script include", "table": "sys_script_include", "scope": "global" }
```

---

## Update a record

```
PATCH /records/:table/:sys_id
Content-Type: application/json
```

```json
{
  "instance": "dev12345.service-now.com",
  "fields": { "active": "false" }
}
```

**Response**
```json
{ "table": "sys_script_include", "sys_id": "abc123", "updated": true, "record": { ... } }
```

---

## Delete a record

```
DELETE /records/:table/:sys_id?instance=dev12345.service-now.com
```

**Response**
```json
{ "table": "sys_script_include", "sys_id": "abc123", "deleted": true }
```
