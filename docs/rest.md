# REST Passthrough

Proxy any ServiceNow REST call through the browser's authenticated session via `agentRestApi`.

This is the escape hatch — use it when none of the higher-level endpoints cover your use case.  The browser sends the request using its existing session cookies, so **no tokens or credentials are required**.

> Requires **SN Utils Pro** extension.

---

## Endpoint

```
POST /rest
Content-Type: application/json
```

```json
{
  "instance": "dev12345.service-now.com",
  "method": "GET",
  "endpoint": "/api/now/table/sys_user",
  "query_params": {
    "sysparm_limit": "5",
    "sysparm_fields": "user_name,email,active"
  }
}
```

| field          | required | default  | description                                                  |
|----------------|----------|----------|--------------------------------------------------------------|
| `instance`     | yes      | —        | ServiceNow hostname                                          |
| `method`       | no       | `"GET"`  | HTTP method: `GET POST PUT PATCH DELETE`                     |
| `endpoint`     | yes      | —        | ServiceNow API path, e.g. `/api/now/table/incident`          |
| `body`         | no       | —        | Request body for POST / PUT / PATCH (JSON object)            |
| `query_params` | no       | —        | Query string parameters as a JSON object                     |

**Response**
```json
{
  "status": 200,
  "data": { "result": [ ... ] }
}
```

On HTTP errors from ServiceNow (4xx, 5xx) the endpoint returns HTTP 502 with `{ "error": "<message>" }`.

---

## Examples

**Create an incident**
```json
{
  "instance": "dev12345.service-now.com",
  "method": "POST",
  "endpoint": "/api/now/table/incident",
  "body": {
    "short_description": "snproxy test incident",
    "category": "software",
    "urgency": "2"
  }
}
```

**Call a Scripted REST API**
```json
{
  "instance": "dev12345.service-now.com",
  "method": "GET",
  "endpoint": "/api/x_myco_myapp/v1/items",
  "query_params": { "active": "true" }
}
```

**Aggregate query**
```json
{
  "instance": "dev12345.service-now.com",
  "method": "GET",
  "endpoint": "/api/now/stats/incident",
  "query_params": {
    "sysparm_query": "active=true",
    "sysparm_count": "true"
  }
}
```
