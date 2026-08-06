use std::borrow::Cow;
use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JVal};
use tf_provider::schema::{Attribute, AttributeConstraint, AttributeType, Block, Description, Schema};
use tf_provider::value::{Value, ValueEmpty, ValueMap, ValueString};
use tf_provider::{map, AttributePath, Diagnostics, Resource};

use crate::client::unwrap_sn_field;
use crate::provider::client;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RecordState<'a> {
    #[serde(borrow = "'a")]
    /// ServiceNow table name, e.g. "incident"
    pub table: ValueString<'a>,
    pub sys_id: ValueString<'a>,
    /// Arbitrary field name -> value. Only fields listed here are managed
    /// (sent on create/update); everything else on the record is left alone.
    pub fields: ValueMap<'a, ValueString<'a>>,
    /// Computed mirror of every non-sys_* field currently on the record,
    /// refreshed on every read/create/update — lets you browse the full
    /// record (`terraform show`) and copy values into `fields` without
    /// needing a separate `terraform import`.
    pub all_fields: ValueMap<'a, ValueString<'a>>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SnRecordResource;

fn owned_string(v: &ValueString<'_>) -> String {
    v.as_ref_option().map(|s| s.to_string()).unwrap_or_default()
}

fn declared_keys(fields: &ValueMap<'_, ValueString<'_>>) -> Vec<String> {
    fields.as_ref_option().map(|m| m.keys().map(|k| k.to_string()).collect()).unwrap_or_default()
}

fn fields_to_json(fields: &ValueMap<'_, ValueString<'_>>) -> Map<String, JVal> {
    fields
        .as_ref_option()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.to_string(), JVal::String(owned_string(v))))
                .collect()
        })
        .unwrap_or_default()
}

fn field_value_to_string(v: &JVal) -> String {
    match unwrap_sn_field(v) {
        JVal::String(s) => s,
        JVal::Null => String::new(),
        other => other.to_string(),
    }
}

/// Project a ServiceNow record JSON object down to only the fields the config
/// declared, unwrapping SN's `{value, display_value}` reference shape as we go.
fn json_to_fields(keys: &[String], record: &JVal) -> ValueMap<'static, ValueString<'static>> {
    let map: BTreeMap<Cow<'static, str>, ValueString<'static>> = keys
        .iter()
        .map(|k| {
            let s = field_value_to_string(record.get(k).unwrap_or(&JVal::Null));
            (Cow::Owned(k.clone()), ValueString::from(Cow::Owned(s)))
        })
        .collect();
    Value::from(map)
}

/// Warn (not error) for any declared field whose value ServiceNow didn't
/// actually store as sent — typically a business rule recalculating it (e.g.
/// `priority` derived from impact/urgency). The write still "succeeds" from
/// Terraform's point of view; the mismatch is surfaced here immediately
/// rather than only showing up as a silent recurring diff on the next plan.
fn warn_on_drift(diags: &mut Diagnostics, declared: &ValueMap<'_, ValueString<'_>>, record: &JVal) {
    let Some(map) = declared.as_ref_option() else { return };
    for (k, v) in map {
        let sent = owned_string(v);
        let actual = field_value_to_string(record.get(k.as_ref()).unwrap_or(&JVal::Null));
        if sent != actual {
            diags.warning(
                format!("`{k}` did not take effect as declared"),
                format!(
                    "set to {sent:?}, but ServiceNow returned {actual:?} — likely rewritten by a \
                     business rule (e.g. `priority` recalculated from impact/urgency). The declared \
                     value will keep being resent every apply and show up as a recurring diff; see \
                     `all_fields` for the true current value, or manage the underlying driver field \
                     instead of this one."
                ),
                AttributePath::new("fields"),
            );
        }
    }
}

/// Same as `json_to_fields`, but takes every field on the record instead of a
/// declared subset — used by `import` and to populate `all_fields`, so a
/// scraped record can be pasted into HCL and pruned down, rather than
/// starting from nothing. Two kinds of noise are dropped:
/// - `sys_*` fields (audit/metadata: sys_created_on, sys_mod_count,
///   sys_updated_by, ...) — not meant to be managed, writing them back would
///   fight ServiceNow's own bookkeeping.
/// - fields ServiceNow returned as an empty string — most tables have
///   hundreds of unused fields that default to `""`; surfacing all of them
///   would swamp the handful that actually have values. A field explicitly
///   *managed* via `fields` still tracks "" precisely — this filtering only
///   applies to this read-only scrape view.
fn json_to_fields_all(record: &JVal) -> ValueMap<'static, ValueString<'static>> {
    let map: BTreeMap<Cow<'static, str>, ValueString<'static>> = record
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(k, _)| !k.starts_with("sys_"))
        .filter_map(|(k, v)| {
            let s = field_value_to_string(v);
            (!s.is_empty()).then(|| (Cow::Owned(k.clone()), ValueString::from(Cow::Owned(s))))
        })
        .collect();
    Value::from(map)
}

#[async_trait]
impl Resource for SnRecordResource {
    type State<'a> = RecordState<'a>;
    type PrivateState<'a> = ValueEmpty;
    type ProviderMetaState<'a> = ValueEmpty;

    fn schema(&self, _diags: &mut Diagnostics) -> Option<Schema> {
        Some(Schema {
            version: 1,
            block: Block {
                version: 1,
                description: Description::plain(
                    "A single ServiceNow table record, addressed dynamically by table \
                     name and an arbitrary field map. No per-table schema modeling — \
                     field names/values are only validated by ServiceNow itself.",
                ),
                attributes: map! {
                    "table" => Attribute {
                        attr_type: AttributeType::String,
                        description: Description::plain("ServiceNow table name, e.g. \"incident\""),
                        constraint: AttributeConstraint::Required,
                        ..Default::default()
                    },
                    "sys_id" => Attribute {
                        attr_type: AttributeType::String,
                        description: Description::plain("Record sys_id"),
                        constraint: AttributeConstraint::Computed,
                        ..Default::default()
                    },
                    "fields" => Attribute {
                        attr_type: AttributeType::Map(AttributeType::String.into()),
                        description: Description::plain("Field name -> value. Only these fields are managed."),
                        constraint: AttributeConstraint::Required,
                        ..Default::default()
                    },
                    "all_fields" => Attribute {
                        attr_type: AttributeType::Map(AttributeType::String.into()),
                        description: Description::plain("Every non-sys_* field on the record (computed, read-only) — for browsing/scraping into `fields`, not managed."),
                        constraint: AttributeConstraint::Computed,
                        ..Default::default()
                    },
                },
                ..Default::default()
            },
        })
    }

    async fn validate<'a>(&self, diags: &mut Diagnostics, config: Self::State<'a>) -> Option<()> {
        if owned_string(&config.table).is_empty() {
            diags.error_short("`table` cannot be empty", AttributePath::new("table"));
            return None;
        }
        if declared_keys(&config.fields).is_empty() {
            diags.error_short("`fields` cannot be empty", AttributePath::new("fields"));
            return None;
        }
        Some(())
    }

    async fn read<'a>(
        &self,
        diags: &mut Diagnostics,
        state: Self::State<'a>,
        private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<(Self::State<'a>, Self::PrivateState<'a>)> {
        let table = owned_string(&state.table);
        let sys_id = owned_string(&state.sys_id);
        let keys = declared_keys(&state.fields);

        // Fetch the whole record — needed to populate `all_fields`, and it lets
        // `fields` still be projected down to only the declared keys below.
        match client().get(&table, &sys_id, &[]).await {
            Ok(None) => None,
            Ok(Some(record)) => {
                let fields = json_to_fields(&keys, &record);
                let all_fields = json_to_fields_all(&record);
                Some((
                    RecordState { table: state.table, sys_id: state.sys_id, fields, all_fields },
                    private_state,
                ))
            }
            Err(e) => {
                diags.root_error("snproxy request failed", e.to_string());
                Some((state, private_state))
            }
        }
    }

    async fn plan_create<'a>(
        &self,
        _diags: &mut Diagnostics,
        proposed_state: Self::State<'a>,
        _config_state: Self::State<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<(Self::State<'a>, Self::PrivateState<'a>)> {
        let mut state = proposed_state;
        state.sys_id = ValueString::Unknown;
        state.all_fields = Value::Unknown;
        Some((state, Default::default()))
    }

    /// Never returns a `requires_replace` — destroy+recreate on a ServiceNow
    /// record is not a safe substitute for update: it mints a new sys_id and
    /// silently orphans every reference to the old one elsewhere on the
    /// platform (parent/child records, approvals, attachments, journal
    /// entries...). A changed `table` can't be reconciled by PATCH either, so
    /// it's rejected outright rather than replaced.
    async fn plan_update<'a>(
        &self,
        diags: &mut Diagnostics,
        prior_state: Self::State<'a>,
        proposed_state: Self::State<'a>,
        _config_state: Self::State<'a>,
        prior_private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<(Self::State<'a>, Self::PrivateState<'a>, Vec<AttributePath>)> {
        if prior_state.table != proposed_state.table {
            diags.error_short(
                "`table` cannot be changed on an existing snterra_record — remove this \
                 resource and create a new one instead of changing its table",
                AttributePath::new("table"),
            );
            return None;
        }
        // Only mark all_fields unknown (needs recomputing) when `fields` is
        // actually changing. Doing this unconditionally would make every plan
        // show a change, even with nothing to update — never converging.
        let fields_changed = prior_state.fields != proposed_state.fields;
        let mut state = proposed_state;
        state.all_fields = if fields_changed { Value::Unknown } else { prior_state.all_fields };
        Some((state, prior_private_state, Vec::new()))
    }

    async fn plan_destroy<'a>(
        &self,
        _diags: &mut Diagnostics,
        _prior_state: Self::State<'a>,
        prior_private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<Self::PrivateState<'a>> {
        Some(prior_private_state)
    }

    async fn create<'a>(
        &self,
        diags: &mut Diagnostics,
        planned_state: Self::State<'a>,
        _config_state: Self::State<'a>,
        private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<(Self::State<'a>, Self::PrivateState<'a>)> {
        let table = owned_string(&planned_state.table);
        let body = fields_to_json(&planned_state.fields);

        match client().create(&table, body).await {
            Ok(record) => {
                let sys_id = record.get("sys_id").and_then(|v| v.as_str()).unwrap_or_default();
                let all_fields = json_to_fields_all(&record);
                // `fields` must come back exactly as planned — Terraform's protocol
                // requires a non-computed attribute's post-apply value to match the
                // plan exactly, and ServiceNow business rules can silently rewrite
                // what we sent (e.g. `priority` recalculated from impact/urgency).
                // The true post-write value is only ever visible via `all_fields`;
                // warn_on_drift flags the mismatch immediately instead of leaving
                // it to surface as a silent recurring diff on the next plan.
                warn_on_drift(diags, &planned_state.fields, &record);
                Some((
                    RecordState {
                        table: planned_state.table,
                        sys_id: ValueString::from(Cow::Owned(sys_id.to_string())),
                        fields: planned_state.fields,
                        all_fields,
                    },
                    private_state,
                ))
            }
            Err(e) => {
                diags.root_error("failed to create record via snproxy", e.to_string());
                None
            }
        }
    }

    async fn update<'a>(
        &self,
        diags: &mut Diagnostics,
        prior_state: Self::State<'a>,
        planned_state: Self::State<'a>,
        _config_state: Self::State<'a>,
        private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<(Self::State<'a>, Self::PrivateState<'a>)> {
        let table = owned_string(&planned_state.table);
        let sys_id = owned_string(&prior_state.sys_id);
        let body = fields_to_json(&planned_state.fields);

        match client().update(&table, &sys_id, body).await {
            Ok(record) => {
                let all_fields = json_to_fields_all(&record);
                // Same reasoning as create(): return `fields` exactly as planned,
                // never re-derived from ServiceNow's response.
                warn_on_drift(diags, &planned_state.fields, &record);
                Some((
                    RecordState {
                        table: planned_state.table,
                        sys_id: prior_state.sys_id,
                        fields: planned_state.fields,
                        all_fields,
                    },
                    private_state,
                ))
            }
            Err(e) => {
                diags.root_error("failed to update record via snproxy", e.to_string());
                None
            }
        }
    }

    async fn destroy<'a>(
        &self,
        diags: &mut Diagnostics,
        state: Self::State<'a>,
        _private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<()> {
        let table = owned_string(&state.table);
        let sys_id = owned_string(&state.sys_id);

        match client().delete(&table, &sys_id).await {
            Ok(()) => Some(()),
            Err(e) => {
                diags.root_error("failed to delete record via snproxy", e.to_string());
                None
            }
        }
    }

    async fn import<'a>(
        &self,
        diags: &mut Diagnostics,
        id: String,
    ) -> Option<(Self::State<'a>, Self::PrivateState<'a>)> {
        let Some((table, sys_id)) = id.split_once('/') else {
            diags.root_error_short("import id must be \"<table>/<sys_id>\"");
            return None;
        };

        match client().get(table, sys_id, &[]).await {
            Ok(None) => {
                diags.root_error_short("record not found");
                None
            }
            Ok(Some(record)) => Some((
                RecordState {
                    table: ValueString::from(Cow::Owned(table.to_string())),
                    sys_id: ValueString::from(Cow::Owned(sys_id.to_string())),
                    fields: json_to_fields_all(&record),
                    all_fields: json_to_fields_all(&record),
                },
                Default::default(),
            )),
            Err(e) => {
                diags.root_error("snproxy request failed", e.to_string());
                None
            }
        }
    }
}
