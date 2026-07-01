---
name: snproxy-ticket-handling
description: Manage ServiceNow tickets (incidents, tasks, requests) through the snproxy proxy — query, triage, update state, assign, add work notes, and escalate.
license: MIT
---

## What this is

This skill lets you handle ServiceNow tickets via `sncli`, which talks to the local `snproxy` daemon. The daemon holds an authenticated browser session so no tokens or API keys are needed — just the instance short name.

**Prerequisite**: snproxy must be running, a Helper Tab must be connected, and `/token` must have been run on the target instance.

## Common workflows

### 1. Find active tickets

List open incidents for a specific instance:

```
sncli records list incident -i dev12345 -q 'active=true' -l 20
```

Filter by category or priority:

```
sncli records list incident -i dev12345 -q 'active=true^priority=1' -f 'sys_id,number,short_description,state,priority,assigned_to'
```

Useful query patterns:

| Goal | Query |
|------|-------|
| My tickets (assigned to me) | `active=true^assigned_to=<my_sys_id>` |
| High priority open | `active=true^priorityIN1,2` |
| Unassigned | `active=true^assigned_toISEMPTY` |
| Recently updated | `sys_updated_on>=javascript:gs.daysAgoStart(7)` |
| By category | `active=true^category=software` |

### 2. Get ticket details

```
sncli records get incident <sys_id> -i dev12345
```

Use `-f` to limit fields when you only need a few:

```
sncli records get incident <sys_id> -i dev12345 -f 'number,short_description,state,assignment_group,assigned_to,sys_created_on'
```

### 3. Update a ticket

Set state to "In Progress" (state=2):

```
sncli records update incident <sys_id> -i dev12345 -f '{"state":"2"}'
```

Common state values for incident: `1`=New, `2`=In Progress, `3`=On Hold, `6`=Resolved, `7`=Closed.

Assign to someone (use their sys_id):

```
sncli records update incident <sys_id> -i dev12345 -f '{"assigned_to":"<user_sys_id>","assignment_group":"<group_sys_id>"}'
```

Add a work note (the field is `work_notes`):

```
sncli records update incident <sys_id> -i dev12345 -f '{"work_notes":"Investigated and found root cause — network interface flapping."}'
```

### 4. Create a ticket

```
sncli records create incident -i dev12345 -f '{"short_description":"Server room temperature high","category":"hardware","urgency":"2","impact":"2"}'
```

### 5. Resolve a ticket

Set state to Resolved (6), add close notes and a resolution code:

```
sncli records update incident <sys_id> -i dev12345 -f '{"state":"6","close_notes":"Rebuilt from backup, monitoring OK","incident_state":"6"}'
```

### 6. Delete a ticket (rare; usually you'd close it)

```
sncli records delete incident <sys_id> -i dev12345
```

## Fields to know

When querying, these fields are useful on most ticket tables:

- `sys_id` — unique record identifier
- `number` — display number (e.g. INC0001234)
- `short_description` — summary
- `description` — full description
- `state` — lifecycle state (numeric)
- `priority` — 1=Critical, 2=High, 3=Moderate, 4=Low, 5=Planning
- `impact` — 1=High, 2=Medium, 3=Low
- `urgency` — 1=High, 2=Medium, 3=Low
- `assigned_to` — reference to sys_user
- `assignment_group` — reference to sys_user_group
- `caller_id` — who reported it
- `category` / `subcategory` — classification
- `work_notes` — internal notes
- `additional_assignee_list` — multi-assign
- `sys_created_on` / `sys_updated_on` — timestamps

## Encoded query cheat sheet

Combine conditions with `^` (AND) or `^OR`:

```
active=true^priority=1                       AND
active=true^ORstate=1                        OR between fields
active=true^priority=1^NQstate=3             OR between condition blocks
active=true^assigned_toISEMPTY               Is empty
active=true^numberSTARTSWITHINC              Starts with
active=true^short_descriptionLIKEnetwork     Contains substring
```

## Gotchas

- Reference field values must be `sys_id` strings, not display names. If you need to find a user's sys_id, query `sys_user`.
- The `-f` (fields) flag for create/update expects a **JSON object string** with quotes. On most shells you'll use single quotes around the JSON.
- Limit defaults to 20. Bump with `-l 100` if you need more.
- `sncli records list` and `get`/`create`/`update`/`delete` use different SN Utils actions internally, but both go through the same authenticated browser session. If one works, the others should too.
