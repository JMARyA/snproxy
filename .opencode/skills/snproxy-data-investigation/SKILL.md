---
name: snproxy-data-investigation
description: Explore and investigate ServiceNow data — map table schemas, follow reference chains, trace relationships, audit trails, and dig into record history through sncli.
license: MIT
---

## What this is

Investigate how data is structured and connected in a ServiceNow instance. Unlike ticket-handling (operational) or record-management (CRUD), this skill is about *understanding* the data model — finding the right table, tracing reference links, following audit trails, and mapping relationships.

## Core investigation patterns

### 1. Explore a table's schema

Before digging into records, understand the fields available:

```
sncli records schema incident -i dev12345
```

The response shows each field's name, type, label, and whether it's mandatory. Reference field types name the target table (e.g. `assigned_to → sys_user`).

Compare schemas across tables to understand inheritance:

```
sncli records schema task -i dev12345
sncli records schema incident -i dev12345
sncli records schema change_request -i dev12345
```

### 2. Find the right table for something

ServiceNow has hundreds of tables. Common starting points:

| What you're looking for | Table |
|-------------------------|-------|
| Users | `sys_user` |
| Groups / teams | `sys_user_group` |
| All CIs | `cmdb_ci` |
| A specific CI type | `cmdb_ci_server`, `cmdb_ci_network`, etc. |
| Business Rules | `sys_script` |
| Script Includes | `sys_script_include` |
| UI Scripts | `sys_ui_script` |
| ACLs | `sys_security_acl` |
| Dictionary entries (field meta) | `sys_dictionary` |
| Choice options | `sys_choice` |
| Workflows | `wf_workflow` |
| Catalog items | `sc_cat_item` |
| Catalog requests | `sc_request` |
| Catalog tasks | `sc_task` |
| Audit (field-level history) | `sys_audit` |

Search for a table by name:

```
sncli records list sys_dictionary -i dev12345 -q 'nameLIKEincident' -f 'name,element,label,reference'
```

### 3. Follow reference chains (dot-walking)

Reference fields point to other tables. Follow them with dot-walking in encoded queries.

Find records where the assigned user's department name starts with "IT":

```
sncli records list incident -i dev12345 -q 'assigned_to.department.nameSTARTSWITHIT' -l 20 -f 'number,assigned_to,assigned_to.department'
```

Dot-walk through a user to their manager:

```
sncli records get sys_user <sys_id> -i dev12345 -f 'user_name,manager,manager.name,manager.email'
```

This works in scripts too:

```javascript
var gr = new GlideRecord('incident');
gr.get('sys_id_here');
var manager = gr.assigned_to.manager.getDisplayValue();
gs.info('Manager: ' + manager);
```

### 4. Trace related records

Find all incidents related to a specific CI:

```
sncli records list incident -i dev12345 -q 'cmdb_ci=<ci_sys_id>' -f 'number,short_description,state'
```

Find all records referencing a specific user:

```javascript
// Use a script to search multiple tables for a user
var tables = ['incident', 'sc_task', 'change_request', 'problem'];
var userId = 'user_sys_id_here';
for (var i = 0; i < tables.length; i++) {
  var gr = new GlideRecord(tables[i]);
  gr.addQuery('assigned_to', userId);
  gr.setLimit(5);
  gr.query();
  while (gr.next()) {
    gs.info(tables[i] + ': ' + gr.getDisplayValue('number') + ' - ' + gr.getValue('short_description'));
  }
}
```

### 5. Audit trail (field-level history)

Check who changed what and when on a record:

```
sncli records list sys_audit -i dev12345 \
  -q 'documentkey=<record_sys_id>' \
  -f 'sys_created_on,sys_created_by,fieldname,oldvalue,newvalue' \
  --order_by ORDERBYsys_created_on
```

Note: `sys_audit` can be very large. Always limit with a date range or specific record.

### 6. Check table inheritance (class hierarchy)

ServiceNow uses vertical inheritance. Find what a table extends:

```
sncli records list sys_dictionary -i dev12345 -q 'name=incident^element=sys_class_name' -f 'name,element,reference'
```

Or check the `sys_db_object` table for table metadata:

```
sncli records list sys_db_object -i dev12345 -q 'name=incident' -f 'name,label,super_class,super_class.name'
```

### 7. Find fields referencing a specific table

Need to know all fields that point to `sys_user`?

```
sncli records list sys_dictionary -i dev12345 -q 'reference=sys_user' -f 'name,element,label,reference' -l 50
```

This shows every table and field that has a reference to users.

## Script-powered investigation

For investigations that need more than queries, use GlideScripts.

**Find orphaned records** (referencing deleted records):

```javascript
var gr = new GlideRecord('incident');
gr.addEncodedQuery('assignment_groupISNOTEMPTY');
gr.setLimit(100);
gr.query();
while (gr.next()) {
  var group = gr.assignment_group.getRefRecord();
  if (!group.isValid()) {
    gs.info('Orphaned: ' + gr.number + ' points to deleted group ' + gr.getValue('assignment_group'));
  }
}
```

**Compare two records' fields**:

```javascript
var gr1 = new GlideRecord('incident');
gr1.get('sys_id_1');
var gr2 = new GlideRecord('incident');
gr2.get('sys_id_2');
var diffs = [];
for (var f in gr1) {
  if (gr1[f] !== gr2[f] && typeof gr1[f] !== 'function') {
    diffs.push({field: f, old: gr1[f], new: gr2[f]});
  }
}
gs.info(JSON.stringify(diffs));
```

**Count records by state/category**:

```javascript
var ga = new GlideAggregate('incident');
ga.addActiveQuery();
ga.addAggregate('COUNT', 'state');
ga.groupBy('state');
ga.query();
while (ga.next()) {
  gs.info('State ' + ga.getValue('state') + ': ' + ga.getAggregate('COUNT', 'state'));
}
```

## Gotchas

- Table names are case-sensitive in ServiceNow. Convention is lowercase with underscores.
- Reference fields store `sys_id` internally. Use dot-walking or `getDisplayValue()` to get human-readable names.
- `sys_audit` is huge on production instances. Always filter by `documentkey` (a specific record) or use a date range.
- `sys_dictionary` is the source of truth for field metadata — every table, every field, every reference.
- CMDB tables (`cmdb_ci_*`) all extend `cmdb_ci`, which extends `cmdb`. Querying `cmdb_ci` returns ALL CI types unless you filter by `sys_class_name`.
