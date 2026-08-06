use std::collections::HashMap;
use std::sync::OnceLock;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tf_provider::schema::{Attribute, AttributeConstraint, AttributeType, Block, Description, Schema};
use tf_provider::value::{ValueEmpty, ValueString};
use tf_provider::{map, Diagnostics, Provider};

use crate::client::RecordApi;
use crate::resource::SnRecordResource;

const DEFAULT_SERVER: &str = "http://127.0.0.1:8766";

static CLIENT: OnceLock<RecordApi> = OnceLock::new();

pub fn client() -> &'static RecordApi {
    CLIENT.get().expect("provider not configured")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnProviderConfig<'a> {
    #[serde(borrow = "'a")]
    /// snproxy HTTP server, default http://127.0.0.1:8766
    pub server: ValueString<'a>,
    /// ServiceNow instance the connected Helper Tab session belongs to
    pub instance: ValueString<'a>,
}

#[derive(Debug, Default, Clone)]
pub struct SnProvider;

#[async_trait]
impl Provider for SnProvider {
    type Config<'a> = SnProviderConfig<'a>;
    type MetaState<'a> = ValueEmpty;

    fn schema(&self, _diags: &mut Diagnostics) -> Option<Schema> {
        Some(Schema {
            version: 1,
            block: Block {
                description: Description::plain(
                    "snterra talks to a local snproxy daemon, which fronts an already \
                     authenticated ServiceNow browser session. No credentials are \
                     configured here.",
                ),
                attributes: map! {
                    "server" => Attribute {
                        attr_type: AttributeType::String,
                        description: Description::plain("snproxy HTTP server URL (default: http://127.0.0.1:8766)"),
                        constraint: AttributeConstraint::Optional,
                        ..Default::default()
                    },
                    "instance" => Attribute {
                        attr_type: AttributeType::String,
                        description: Description::plain("ServiceNow instance name (e.g. \"dev12345\" or \"dev12345.service-now.com\") — must match the instance the snproxy Helper Tab is currently connected to"),
                        constraint: AttributeConstraint::Required,
                        ..Default::default()
                    },
                },
                ..Default::default()
            },
        })
    }

    async fn validate<'a>(&self, diags: &mut Diagnostics, config: Self::Config<'a>) -> Option<()> {
        if config.instance.as_ref_option().is_none_or(|s| s.is_empty()) {
            diags.root_error_short("`instance` is required");
            return None;
        }
        Some(())
    }

    async fn configure<'a>(
        &self,
        diags: &mut Diagnostics,
        _terraform_version: String,
        config: Self::Config<'a>,
    ) -> Option<()> {
        let server = config.server.as_ref_option().map(|s| s.to_string()).unwrap_or_else(|| DEFAULT_SERVER.to_string());
        let Some(instance) = config.instance.as_ref_option().map(|s| s.to_string()) else {
            diags.root_error_short("`instance` is required");
            return None;
        };

        if CLIENT.set(RecordApi::new(server, instance)).is_err() {
            diags.root_error_short("provider already configured");
            return None;
        }

        Some(())
    }

    fn get_resources(
        &self,
        _diags: &mut Diagnostics,
    ) -> Option<HashMap<String, Box<dyn tf_provider::DynamicResource>>> {
        Some(map! {
            "record" => SnRecordResource,
        })
    }

    fn get_data_sources(
        &self,
        _diags: &mut Diagnostics,
    ) -> Option<HashMap<String, Box<dyn tf_provider::DynamicDataSource>>> {
        Some(map! {})
    }
}
