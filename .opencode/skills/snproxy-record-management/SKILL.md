---
name: snproxy-record-management
description: Generic CRUD and schema operations on any ServiceNow table via sncli — create, read, update, delete records, inspect table schemas, and craft encoded queries.
license: MIT
---

## What this is

Generic record operations on any ServiceNow table through `sncli`. Unlike ticket-handling (which is specific to incident/task tables), this skill covers *any* table — custom tables, sys_ tables, cmdb_ci, and so on.

**When to use this**: creating configuration items, managing users/groups, updating custom application data, working with tables that aren't incident/task/request.

## Core patterns

### 1. Inspect a table before touching it

Always check the schema first so you know field names, types, and mandatory fields:

```
sncli records schema incident -i dev12345
```

This returns every field with its name, type, label, and whether it's mandatory. For reference fields, the type tells you the target table.

### 2. Query records from any table

```
sncli records list <table> -i <instance> -q '<encoded_query>' -f '<fields>' -l <limit>
```

Tables you'll commonly query:

| Table | Contents |
|-------|----------|
| `sys_user` | Users |
| `sys_user_group` | Groups |
| `cmdb_ci` | CI items (all types) |
| `sys_dictionary` | Field definitions (metadata) |
| `sys_script_include` | Server-side scripts |
| `sys_script` | Business rules |
| `sys_ui_script` | Client-side scripts |
| `sys_choice` | Choice list options for fields |
| `sys_attachment` | Attachments metadata |

### 3. Fetch a single record by sys_id

```
sncli records get <table> <sys_id> -i <instance>
```

Use `-f` to get specific fields when you want a subset:

```
sncli records get sys_user 62826bf03710200044e0bfc8bcbe5d31 -i dev12345 -f 'user_name,email,department,manager'
```

### 4. Create a record

```
sncli records create <table> -i <instance> -f '{"field1":"value1","field2":"value2"}'
```

The response includes the new `sys_id`. Store it if you need to reference the record later.

### 5. Update a record (partial)

Only send the fields that changed — ServiceNow merges the payload:

```
sncli records update <table> <sys_id> -i <instance> -f '{"field_name":"new_value"}'
```

### 6. Delete a record

```
sncli records delete <table> <sys_id> -i <instance>
```

## Working with field types

### Reference fields

Reference values must be `sys_id` strings. Find the sys_id by querying the target table:

```
sncli records list sys_user -i dev12345 -q 'user_name=john.smith' -f 'sys_id,user_name,name'
```

Then use the returned sys_id:

```
sncli records update incident <sys_id> -i dev12345 -f '{"assigned_to":"<sys_id_from_above>"}'
```

### Choice fields (select lists)

Find valid choices from `sys_choice`:

```
sncli records list sys_choice -i dev12345 -q 'name=<table>^element=<field>' -f 'value,label,sequence'
```

### Date/time fields

ISO 8601 format: `2025-12-01 08:00:00` or `2025-12-01`. ServiceNow stores in UTC.

### Boolean fields

Use `"true"` or `"false"` as strings.

## Encoded query reference

Build complex queries with ServiceNow's encoded query syntax:

| Example | Meaning |
|---------|---------|
| `active=true` | Equals |
| `state!=6` | Not equals |
| `priority<3` | Less than |
| `stateIN1,2,3` | In list |
| `assigned_toISEMPTY` | No value |
| `numberSTARTSWITHINC` | Prefix match |
| `short_descriptionLIKEnetwork` | Contains |
| `nameLIKEJohn` | Contains "John" |
| `sys_created_on>=javascript:gs.daysAgoStart(7)` | Last 7 days |
| `active=true^priority=1` | AND |
| `state=1^ORstate=2` | OR (field-level) |
| `active=true^priority=1^NQstate=3` | OR (query group) |
| `assigned_to.department.nameLIKET` | Dot-walk to related field |

## Creating records in custom scoped tables

Scoped tables need the application scope specified:

```
sncli records create x_myco_custom_table -i dev12345 -f '{"name":"My Record","field1":"value"}'
```

The `instance` parameter in sncli auto-normalizes (short name → full domain), so `dev12345` works as-is.

## Gotchas

- Mandatory fields: check via `schema` first. Trying to create a record without mandatory fields returns an error.
- Field names in ServiceNow use underscores (`short_description`), not spaces.
- The encoded query `^` character may need escaping in some shells. Wrap in single quotes.
- `ORDERBY` sorting goes in the `--order_by` param (e.g. `--order_by ORDERBYDESCsys_created_on`), *not* in the query string.
- Table names are case-sensitive in ServiceNow (usually lowercase with underscores).
