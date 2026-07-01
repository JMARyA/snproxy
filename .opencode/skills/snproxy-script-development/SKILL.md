---
name: snproxy-script-development
description: Write, run, and debug ServiceNow GlideScripts via sncli's background script runner — iterate on scripts, inspect GlideRecord results, and parse structured output.
license: MIT
---

## What this is

Run server-side JavaScript (GlideScripts) on a ServiceNow instance through `sncli scripts bg`. Scripts execute in the instance's global scope with full access to the Glide API — GlideRecord, GlideAggregate, gs, GlideDateTime, etc.

This is useful when:
- You need to inspect data that's too complex for a query (multi-table joins, aggregation, conditional logic)
- You want to test a script snippet before putting it in a Business Rule / Script Include
- You need to perform batch operations (mass update, data cleanup)
- The REST API doesn't expose what you need

## Running a script

Inline script:

```
sncli scripts bg -i dev12345 -s 'gs.info("hello from snproxy")'
```

From a file:

```
sncli scripts bg -i dev12345 -f myscript.js
```

Get structured JSON output (includes `lines` array):

```
sncli scripts bg -i dev12345 -s 'gs.info("hello")' --json
```

## Common GlideRecord patterns

### Query records

```javascript
var gr = new GlideRecord('incident');
gr.addActiveQuery();
gr.setLimit(5);
gr.query();
while (gr.next()) {
  gs.info(gr.getDisplayValue('number') + ' | ' + gr.getValue('short_description'));
}
```

### Query with conditions

```javascript
var gr = new GlideRecord('incident');
gr.addEncodedQuery('active=true^priority=1');
gr.query();
gs.info('Count: ' + gr.getRowCount());
while (gr.next()) {
  gs.info(gr.number + ' ' + gr.short_description);
}
```

### Get a single record by sys_id

```javascript
var gr = new GlideRecord('incident');
gr.get('sys_id_here');
gs.info(gr.short_description);
```

### Create a record

```javascript
var gr = new GlideRecord('incident');
gr.initialize();
gr.short_description = 'Created via snproxy script';
gr.category = 'software';
gr.urgency = 2;
var sysId = gr.insert();
gs.info('Created: ' + sysId);
```

### Update records

```javascript
var gr = new GlideRecord('incident');
gr.addEncodedQuery('active=true^assigned_toISEMPTY');
gr.query();
while (gr.next()) {
  gr.assignment_group = 'group_sys_id';
  gr.setWorkNote('Auto-assigned by cleanup script');
  gr.update();
  gs.info('Updated: ' + gr.number);
}
```

### Delete records

```javascript
var gr = new GlideRecord('incident');
gr.get('sys_id_here');
gr.deleteRecord();
```

## Outputting structured data

For results the AI can parse, output JSON through `gs.info`:

```javascript
var results = [];
var gr = new GlideRecord('incident');
gr.setLimit(10);
gr.addActiveQuery();
gr.query();
while (gr.next()) {
  results.push({
    number: gr.getDisplayValue('number'),
    state: gr.getDisplayValue('state'),
    priority: gr.getValue('priority'),
    short_description: gr.getValue('short_description')
  });
}
gs.info(JSON.stringify(results));
```

Then use `--json` flag when running:

```
sncli scripts bg -i dev12345 -f query.js --json
```

The response includes both `output` (plain text) and `lines` (array of lines). Parse the JSON line from `lines`.

## Debugging tips

- `gs.info()` is your print statement. Use it liberally.
- `gs.print()` works too but offers no advantage over `gs.info()`.
- Check the `lines` array in `--json` output — each `gs.info()` call becomes one line.
- Errors include JavaScript stack traces in the output.
- Scripts run with `background_script_execute` role — if you get permission errors, check with your SN admin.
- `setLimit()` is required for production tables — without it, large queries can timeout.

## Common Glide API methods

| Method | Purpose |
|--------|---------|
| `gr.get(sys_id)` | Fetch by sys_id |
| `gr.addQuery(field, op, value)` | Add condition |
| `gr.addEncodedQuery(string)` | Add encoded query |
| `gr.addActiveQuery()` | Shortcut for `active=true` |
| `gr.setLimit(n)` | Max results |
| `gr.orderBy(field)` | Sort ascending |
| `gr.query()` | Execute |
| `gr.next()` | Iterate |
| `gr.getRowCount()` | Count results (after query) |
| `gr.getValue(field)` | Raw value |
| `gr.getDisplayValue(field)` | Display value |
| `gr.setValue(field, val)` | Set field |
| `gr.insert()` | Create, returns sys_id |
| `gr.update()` | Save changes |
| `gr.deleteRecord()` | Delete |

## Gotchas

- GlideScripts are **real JavaScript** (ECMAScript 5-ish). No arrow functions, no `let`/`const` in older instances (use `var`).
- `gr.getRowCount()` returns the actual count only for GlideRecord — for large tables it may return -1 (performance optimization). Use GlideAggregate with `COUNT` for reliable counts.
- Always `gr.initialize()` before inserting a new record.
- Scripts that take too long will be killed by ServiceNow (usually 60-second timeout).
- The `--json` flag gives you structured output. Without it, you get plain text (good for quick checks).
- You can use `sncli records` for simple CRUD. Reach for scripts when you need logic (loops, conditions, cross-table operations).
