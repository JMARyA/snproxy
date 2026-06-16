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
| `q`        | no       | `""`                                          | [SN encoded query](#encoded-query-syntax), e.g. `active=true^category=software` |
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

Works for any table — incidents, tasks, users, custom tables, etc. Uses `agentRestApi POST`.
For creating development artifacts (Script Includes, Business Rules, etc.) that should be added
to an update set and opened in the browser, use [`POST /artifacts`](artifacts.md) instead.

```json
{
  "instance": "dev12345.service-now.com",
  "fields": {
    "short_description": "Printer on fire",
    "category": "hardware",
    "urgency": "1"
  }
}
```

**Response**
```json
{ "sys_id": "abc123", "table": "incident", "record": { ... } }
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

---

## Inspect table schema

```
GET /records/:table/schema?instance=dev12345.service-now.com
```

Returns field metadata for any table — useful before creating or updating records.

**Response**
```json
{
  "table": "incident",
  "fields": [
    { "name": "short_description", "type": "string",  "label": "Short description", "mandatory": true  },
    { "name": "state",             "type": "integer", "label": "State",             "mandatory": false }
  ]
}
```

---

## Encoded query syntax

The `q` parameter uses ServiceNow's **encoded query** format — a compact, URL-safe string that maps directly to SQL `WHERE` clauses. Build one visually by filtering a ServiceNow list view and right-clicking the breadcrumb → **Copy query**.

### Operators

| Operator | Example | Description |
|----------|---------|-------------|
| `=` | `priority=1` | Equals |
| `!=` | `state!=6` | Not equals |
| `<` `>` `<=` `>=` | `priority<3` | Comparison |
| `IN` | `stateIN1,2,3` | In comma-separated list |
| `NOT IN` | `stateNOT IN6,7,8` | Not in list |
| `STARTSWITH` | `numberSTARTSWITHINC` | Starts with string |
| `ENDSWITH` | `short_descriptionENDSWITHurgent` | Ends with string |
| `LIKE` | `short_descriptionLIKEnetwork` | Contains substring |
| `NOT LIKE` | `short_descriptionNOT LIKEtest` | Does not contain |
| `ISEMPTY` | `assignment_groupISEMPTY` | Field has no value |
| `ISNOTEMPTY` | `assignment_groupISNOTEMPTY` | Field has a value |
| `BETWEEN` | `priorityBETWEEN1@2` | Numeric range (inclusive, `@` separator) |
| `INSTANCEOF` | `sys_class_nameINSTANCEOFincident` | Filter by table inheritance |
| `ORDERBY` | `^ORDERBYDESCsys_created_on` | Sorting (in `order_by` param) |

### Combining conditions

| Separator | Meaning | Example |
|-----------|---------|---------|
| `^` | AND | `active=true^priority=1` |
| `^OR` | OR | `state=1^ORstate=2` (field-level OR) |
| `^NQ` | New query group | `active=true^priority=1^NQstate=3^category=hardware` — OR between condition blocks |

### Reference fields

Use `sys_id` as the value, or dot-walk to a related field:

```
assignment_group=6816f79cc0a8016401c5a33be04be441
assignment_group.nameLIKENetwork
assigned_to.department.nameSTARTSWITHIT
```

### Dates / relative times

```
opened_at>=javascript:gs.daysAgoStart(7)
sys_created_onONThis week@javascript:gs.beginningOfThisWeek()@javascript:gs.endOfThisWeek()
sys_updated_onONLast month@javascript:gs.beginningOfLastMonth()@javascript:gs.endOfLastMonth()
```

### Text search

```
123TEXTQUERY321network outage
```

### Order of evaluation

Conditions are evaluated left-to-right with no explicit grouping. For complex AND+OR logic, build the filter in the ServiceNow list view and copy the query — the platform handles precedence correctly.
