use serde_json::{Map, Value};

/// Normalise an instance identifier to a bare hostname, matching sncore/snstate/sncli.
pub fn normalize_instance(s: &str) -> String {
    let s = s.trim_end_matches('/');
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(s);
    if s.contains('.') {
        s.to_string()
    } else {
        format!("{s}.service-now.com")
    }
}

/// Unwrap ServiceNow's `{"value": "...", "display_value": "..."}` reference wrapper
/// down to the plain sys_id/value, matching snstate's convention.
pub fn unwrap_sn_field(v: &Value) -> Value {
    if let Value::Object(o) = v {
        if let Some(val) = o.get("value") {
            return val.clone();
        }
    }
    v.clone()
}

/// Client for the snproxy `/records` HTTP API. No ServiceNow credentials are
/// held here — snproxy fronts the browser's authenticated session, this just
/// talks to snproxy over localhost.
pub struct RecordApi {
    http: reqwest::Client,
    server: String,
    instance: String,
}

impl RecordApi {
    pub fn new(server: String, instance: String) -> Self {
        Self { http: reqwest::Client::new(), server, instance: normalize_instance(&instance) }
    }

    async fn read_body(resp: reqwest::Response) -> anyhow::Result<(reqwest::StatusCode, Value)> {
        let status = resp.status();
        let text = resp.text().await?;
        if text.is_empty() {
            anyhow::bail!("HTTP {status} (empty body)");
        }
        let v: Value = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("HTTP {status}: non-JSON response ({e}): {text}"))?;
        Ok((status, v))
    }

    fn error_message(status: reqwest::StatusCode, v: &Value) -> String {
        let msg = v.get("error").and_then(|e| e.as_str()).unwrap_or("request failed");
        format!("HTTP {status}: {msg}")
    }

    pub async fn create(&self, table: &str, fields: Map<String, Value>) -> anyhow::Result<Value> {
        let url = format!("{}/records/{table}", self.server);
        let body = serde_json::json!({ "instance": self.instance, "fields": fields });
        let resp = self.http.post(&url).json(&body).send().await?;
        let (status, v) = Self::read_body(resp).await?;
        if !status.is_success() {
            anyhow::bail!(Self::error_message(status, &v));
        }
        Ok(v["record"].clone())
    }

    /// Returns `Ok(None)` if the record no longer exists (HTTP 404), `Err` for any
    /// other failure so a transient/network error never gets mistaken for deletion.
    pub async fn get(
        &self,
        table: &str,
        sys_id: &str,
        fields: &[String],
    ) -> anyhow::Result<Option<Value>> {
        let url = format!("{}/records/{table}/{sys_id}", self.server);
        let mut req = self.http.get(&url).query(&[("instance", self.instance.as_str())]);
        if !fields.is_empty() {
            req = req.query(&[("fields", fields.join(","))]);
        }
        let resp = req.send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let (status, v) = Self::read_body(resp).await?;
        if !status.is_success() {
            anyhow::bail!(Self::error_message(status, &v));
        }
        let record = v["record"].clone();
        if record.is_null() || record.as_object().is_some_and(|o| o.is_empty()) {
            return Ok(None);
        }
        Ok(Some(record))
    }

    pub async fn update(
        &self,
        table: &str,
        sys_id: &str,
        fields: Map<String, Value>,
    ) -> anyhow::Result<Value> {
        let url = format!("{}/records/{table}/{sys_id}", self.server);
        let body = serde_json::json!({ "instance": self.instance, "fields": fields });
        let resp = self.http.patch(&url).json(&body).send().await?;
        let (status, v) = Self::read_body(resp).await?;
        if !status.is_success() {
            anyhow::bail!(Self::error_message(status, &v));
        }
        Ok(v["record"].clone())
    }

    pub async fn delete(&self, table: &str, sys_id: &str) -> anyhow::Result<()> {
        let url = format!("{}/records/{table}/{sys_id}", self.server);
        let resp = self.http.delete(&url).query(&[("instance", self.instance.as_str())]).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        let (status, v) = Self::read_body(resp).await?;
        if !status.is_success() {
            anyhow::bail!(Self::error_message(status, &v));
        }
        Ok(())
    }
}
