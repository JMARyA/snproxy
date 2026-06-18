use serde_json::Value;
use std::time::Duration;

use crate::types::SnTableMeta;

#[derive(Debug, Clone, Default)]
pub struct HealthInfo {
    pub status: String, // "ready" | "no_session" | "waiting"
    #[allow(dead_code)]
    pub helper_tab_connected: bool,
    #[allow(dead_code)]
    pub sn_session: bool,
    #[allow(dead_code)]
    pub instance_url: String,
    pub instance_name: String,
}

impl HealthInfo {
    pub fn is_ready(&self) -> bool {
        self.status == "ready"
    }

    pub fn status_label(&self) -> &str {
        match self.status.as_str() {
            "ready" => "READY",
            "no_session" => "NO SESSION",
            "waiting" => "WAITING",
            _ => "OFFLINE",
        }
    }
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    pub base_url: String,
}

impl Client {
    pub fn new(port: u16) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client init failed");
        Self { http, base_url: format!("http://127.0.0.1:{port}") }
    }

    pub async fn health(&self) -> Result<HealthInfo, String> {
        let resp = self
            .http
            .get(format!("{}/health", self.base_url))
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let j: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(HealthInfo {
            status: j["status"].as_str().unwrap_or("unknown").into(),
            helper_tab_connected: j["helper_tab_connected"].as_bool().unwrap_or(false),
            sn_session: j["sn_session"].as_bool().unwrap_or(false),
            instance_url: j["instance_url"].as_str().unwrap_or("").into(),
            instance_name: j["instance_name"].as_str().unwrap_or("").into(),
        })
    }

    pub async fn list_records(
        &self,
        table: &str,
        fields: &str,
        q: &str,
        limit: u32,
        order_by: &str,
    ) -> Result<Vec<Value>, String> {
        let mut params: Vec<(&str, String)> = vec![
            ("instance", "auto".into()),
            ("fields", fields.into()),
            ("limit", limit.to_string()),
        ];
        if !q.is_empty() {
            params.push(("q", q.into()));
        }
        if !order_by.is_empty() {
            params.push(("order_by", order_by.into()));
        }

        let resp = self
            .http
            .get(format!("{}/records/{table}", self.base_url))
            .query(&params)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            // try to extract a readable error message
            let msg = if let Ok(j) = serde_json::from_str::<Value>(&body) {
                j["error"].as_str().unwrap_or(&body).to_string()
            } else {
                format!("HTTP {status}: {body}")
            };
            return Err(msg);
        }

        let j: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(j["records"].as_array().cloned().unwrap_or_default())
    }

    /// `display_value`: "false" (raw sys_ids), "true" (display strings), "all" (both as {value, display_value})
    pub async fn get_record(&self, table: &str, sys_id: &str, display_value: &str) -> Result<Value, String> {
        let resp = self
            .http
            .get(format!("{}/records/{table}/{sys_id}", self.base_url))
            .query(&[("instance", "auto"), ("display_value", display_value)])
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }

        let j: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(j["record"].clone())
    }

    pub async fn run_script(&self, script: &str) -> Result<String, String> {
        let body = serde_json::json!({ "instance": "auto", "script": script });
        let resp = self
            .http
            .post(format!("{}/scripts/bg", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(text);
        }

        let j: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(j["output"].as_str().unwrap_or("").into())
    }

    pub async fn list_all_tables(&self, filter: &str) -> Result<Vec<(String, String)>, String> {
        let q = if filter.is_empty() {
            String::new()
        } else {
            format!("nameLIKE{filter}^ORlabelLIKE{filter}")
        };
        let records =
            self.list_records("sys_db_object", "name,label", &q, 500, "ORDERBYname").await?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                let name = r["name"].as_str()?.to_string();
                let label = r["label"].as_str().unwrap_or(&name).to_string();
                Some((name, label))
            })
            .collect())
    }

    pub async fn schema(&self, table: &str) -> Result<SnTableMeta, String> {
        let resp = self
            .http
            .get(format!("{}/records/{table}/schema", self.base_url))
            .query(&[("instance", "auto")])
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }

        let j: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(SnTableMeta {
            label:   j["label"].as_str().unwrap_or(table).to_string(),
            name:    j["table"].as_str().unwrap_or(table).to_string(),
            columns: serde_json::from_value(j["columns"].clone()).unwrap_or_default(),
        })
    }
}
