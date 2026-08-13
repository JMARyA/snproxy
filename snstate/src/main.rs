// snstate — Terraform-like state management for ServiceNow records.
//
// Workflow:
//   snstate pull incident -i dev12345 -q "active=true" -f "short_description,state,priority"
//   # edit incident/<sys_id>.toml ...
//   snstate plan
//   snstate push          (or: snstate apply)
//
// File layout per record:
//   <dir>/<table>/<sys_id>.toml        — editable desired state
//   <dir>/<table>/<sys_id>.state.toml  — baseline (last pulled / last pushed)
//
// Record file format:
//   number            = "INC0010042"
//   short_description = "Something broke"
//   state             = "1"
//
//   [_meta]
//   instance = "dev12345.service-now.com"
//   table    = "incident"
//   sys_id   = "abc1234..."
//
// (_meta is written by snstate; scalar fields are user-editable.
//  TOML places subtables after scalars, so _meta naturally ends up at the bottom.)
//
// New records: place a .toml file with no [_meta] block (or _meta without sys_id).
// After push the file is renamed to <new_sys_id>.toml and _meta is populated.

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use reqwest::{Client, Method};
use serde_json::{json, Map, Value as JVal};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use toml::Value as TVal;

const META_KEY: &str = "_meta";

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "snstate",
    about = "Terraform-like state management for ServiceNow records.\n\
             Typical flow: pull → edit → plan → push"
)]
struct Cli {
    /// snproxy server base URL
    #[arg(long, default_value = "http://localhost:8766", env = "SNPROXY_URL")]
    server: String,
    /// Working directory containing <table>/<sys_id>.toml files
    #[arg(long, short = 'd', default_value = "./res")]
    dir: PathBuf,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Import records from ServiceNow into local TOML files
    Pull(PullArgs),
    /// Show local modifications vs last pull/push (no SN connection needed)
    Status,
    /// Show what push would change against live ServiceNow state
    Plan(TargetArgs),
    /// Re-read current state from ServiceNow and update local state files
    Refresh(RefreshArgs),
    /// Apply local changes to ServiceNow
    Push(PushArgs),
    /// Alias for push
    Apply(PushArgs),
}

#[derive(Args)]
struct PullArgs {
    /// Table to import (e.g. incident, cmdb_ci, sys_script_include)
    table: String,
    /// Pull a single record by sys_id instead of querying the list
    sys_id: Option<String>,
    /// ServiceNow instance hostname or short name
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// Encoded query, e.g. "active=true^category=software" (list mode only)
    #[arg(long, short = 'q')]
    query: Option<String>,
    /// Comma-separated fields to store (omit for all fields)
    #[arg(long, short = 'f')]
    fields: Option<String>,
    /// Max records to import (list mode only)
    #[arg(long, short = 'l', default_value_t = 100)]
    limit: u32,
}

#[derive(Args)]
struct TargetArgs {
    /// Optional filter: "incident" (whole table) or "incident/<sys_id>" (single record)
    target: Option<String>,
    /// Override instance (defaults to _meta.instance from the file)
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: Option<String>,
}

#[derive(Args)]
struct RefreshArgs {
    /// Optional filter: "incident" (whole table) or "incident/<sys_id>" (single record)
    target: Option<String>,
    /// Override instance (defaults to _meta.instance from the file)
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: Option<String>,
}

#[derive(Args)]
struct PushArgs {
    /// Optional filter: "incident" or "incident/<sys_id>"
    target: Option<String>,
    /// Override instance (defaults to _meta.instance from the file)
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: Option<String>,
    /// Show what would be sent without making any changes
    #[arg(long, short = 'n')]
    dry_run: bool,
    /// Push all records even if they match the baseline
    #[arg(long)]
    force: bool,
}

// ── TOML ↔ JSON conversion ────────────────────────────────────────────────────

fn toml_to_json(v: TVal) -> JVal {
    match v {
        TVal::String(s)   => JVal::String(s),
        TVal::Integer(i)  => json!(i),
        TVal::Float(f)    => json!(f),
        TVal::Boolean(b)  => JVal::Bool(b),
        TVal::Datetime(d) => JVal::String(d.to_string()),
        TVal::Array(a)    => JVal::Array(a.into_iter().map(toml_to_json).collect()),
        TVal::Table(t)    => JVal::Object(t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect()),
    }
}

fn json_to_toml(v: JVal) -> TVal {
    match v {
        JVal::Null        => TVal::String(String::new()),
        JVal::Bool(b)     => TVal::Boolean(b),
        JVal::Number(n)   => {
            if let Some(i) = n.as_i64() { TVal::Integer(i) }
            else { TVal::Float(n.as_f64().unwrap_or(0.0)) }
        }
        JVal::String(s)   => TVal::String(s),
        JVal::Array(a)    => TVal::Array(a.into_iter().map(json_to_toml).collect()),
        JVal::Object(o)   => TVal::Table(o.into_iter().map(|(k, v)| (k, json_to_toml(v))).collect()),
    }
}

// ── file helpers ──────────────────────────────────────────────────────────────

fn record_path(dir: &Path, table: &str, name: &str) -> PathBuf {
    dir.join(table).join(format!("{name}.toml"))
}

fn state_path(dir: &Path, table: &str, name: &str) -> PathBuf {
    dir.join(".state").join(table).join(format!("{name}.toml"))
}

fn write_toml(path: &Path, value: &JVal) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let tv = json_to_toml(value.clone());
    let text = toml::to_string_pretty(&tv)
        .with_context(|| format!("serializing TOML for {}", path.display()))?;
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

fn read_toml(path: &Path) -> Result<JVal> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let tv: TVal = toml::from_str(&text)
        .with_context(|| format!("parsing TOML in {}", path.display()))?;
    Ok(toml_to_json(tv))
}

/// Fields from a record file, excluding _meta/_state and ServiceNow's own
/// audit/bookkeeping columns. State metadata stored alongside the baseline so
/// we can detect instance changes.
const STATE_META_KEY: &str = "_state";

/// `sys_*` fields (sys_id, sys_created_on, sys_mod_count, sys_updated_by, ...)
/// are managed by ServiceNow itself. `pull` stores them (useful context when
/// inspecting a record), but writing them back on `push` fights ServiceNow's
/// own bookkeeping and commonly gets rejected with a 403 — see the same
/// exclusion in ../snterra/src/resource.rs.
fn editable_fields(v: &JVal) -> Map<String, JVal> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, _)| k.as_str() != META_KEY && k.as_str() != STATE_META_KEY && !k.starts_with("sys_"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn state_meta(v: &JVal) -> Option<&JVal> {
    v.get(STATE_META_KEY)
}

fn set_state_meta(v: &mut Map<String, JVal>, instance: &str) {
    let sm = v.entry(STATE_META_KEY.to_string())
        .or_insert_with(|| JVal::Object(Default::default()))
        .as_object_mut()
        .unwrap();
    sm.insert("instance".to_string(), JVal::String(instance.to_string()));
}

fn meta_str<'a>(v: &'a JVal, key: &str) -> Option<&'a str> {
    v.get(META_KEY)?.get(key)?.as_str()
}

/// Collect all `*.toml` (not `*.state.toml`) under `dir`, filtered by an optional
/// `"table"` or `"table/name"` target string.
fn collect_records(dir: &Path, target: Option<&str>) -> Result<Vec<(String, String, PathBuf)>> {
    let (filter_table, filter_name) = parse_target(target);

    let table_dirs: Vec<(String, PathBuf)> = match &filter_table {
        Some(t) => {
            let p = dir.join(t);
            if p.is_dir() { vec![(t.clone(), p)] } else { vec![] }
        }
        None => {
            std::fs::read_dir(dir)
                .with_context(|| format!("reading directory {}", dir.display()))?
                .filter_map(|e| e.ok())
                .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                .filter(|(name, path)| !name.starts_with('.') && path.is_dir())
                .collect()
        }
    };

    let mut results = Vec::new();
    for (table, table_dir) in table_dirs {
        for entry in std::fs::read_dir(&table_dir)
            .with_context(|| format!("reading {}", table_dir.display()))?
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            let fname = path.file_name().unwrap().to_string_lossy().to_string();
            if !fname.ends_with(".toml") {
                continue;
            }
            let name = fname.trim_end_matches(".toml").to_string();
            if let Some(ref fn_filter) = filter_name {
                if &name != fn_filter {
                    continue;
                }
            }
            results.push((table.clone(), name, path));
        }
    }

    results.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    Ok(results)
}

fn parse_target(target: Option<&str>) -> (Option<String>, Option<String>) {
    match target {
        None => (None, None),
        Some(t) => {
            let mut parts = t.splitn(2, '/');
            (parts.next().map(String::from), parts.next().map(String::from))
        }
    }
}

// ── HTTP helper ───────────────────────────────────────────────────────────────

async fn api(client: &Client, method: Method, url: &str, body: Option<&JVal>) -> Result<JVal> {
    let mut req = client.request(method.clone(), url);
    if let Some(b) = body {
        req = req.json(b);
    }
    let resp = req.send().await.with_context(|| format!("{method} {url}"))?;
    let status = resp.status();
    let text = resp.text().await.with_context(|| format!("{method} {url} — reading body"))?;
    if text.is_empty() {
        bail!("{method} {url} → HTTP {status} (empty body)");
    }
    let v: JVal = serde_json::from_str(&text)
        .with_context(|| format!("{method} {url} → non-JSON: {text}"))?;
    if !status.is_success() {
        let msg = v.get("error").and_then(|e| e.as_str()).unwrap_or(&text);
        bail!("{method} {url} → HTTP {status}: {msg}");
    }
    Ok(v)
}

fn normalize_instance(s: &str) -> String {
    let s = s.trim_end_matches('/');
    let s = s.strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    if s.contains('.') { s.to_string() } else { format!("{s}.service-now.com") }
}

fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ── color ─────────────────────────────────────────────────────────────────────

fn color_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
    })
}

fn paint(code: &str, s: &str) -> String {
    if color_enabled() { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
}

fn green(s: &str)  -> String { paint("32", s) }
fn red(s: &str)    -> String { paint("31", s) }
fn yellow(s: &str) -> String { paint("33", s) }
fn cyan(s: &str)   -> String { paint("36", s) }
fn dim(s: &str)    -> String { paint("2", s) }
fn bold(s: &str)   -> String { paint("1", s) }

// ── diff helpers ──────────────────────────────────────────────────────────────

/// Returns (changed [(field, old, new)], added [field only in local]).
fn diff(local: &Map<String, JVal>, reference: &Map<String, JVal>) -> (Vec<(String, String, String)>, Vec<String>) {
    let mut changed = Vec::new();
    let mut added = Vec::new();

    for (k, lv) in local {
        match reference.get(k) {
            Some(rv) => {
                let l = val_str(lv);
                let r = val_str(rv);
                if l != r {
                    changed.push((k.clone(), r, l));
                }
            }
            None => added.push(k.clone()),
        }
    }
    changed.sort_by(|a, b| a.0.cmp(&b.0));
    added.sort();
    (changed, added)
}

/// Unwrap SN's `{"value": "...", "display_value": "..."}` wrapper to just the value.
fn unwrap_sn_field(v: &JVal) -> JVal {
    if let JVal::Object(o) = v {
        if let Some(val) = o.get("value") {
            return val.clone();
        }
    }
    v.clone()
}

fn val_str(v: &JVal) -> String {
    match v {
        JVal::String(s) => s.clone(),
        JVal::Null      => String::new(),
        other           => other.to_string(),
    }
}

fn has_diff(changed: &[(String, String, String)], added: &[String]) -> bool {
    !changed.is_empty() || !added.is_empty()
}

fn print_diff(changed: &[(String, String, String)], added: &[String]) {
    for (field, old, new) in changed {
        println!("       {field}: {} -> {}", red(&format!("{old:?}")), green(&format!("{new:?}")));
    }
    for field in added {
        println!("       {} {field} (not in current SN state)", green("+"));
    }
}

// ── commands ──────────────────────────────────────────────────────────────────

async fn cmd_pull(server: &str, dir: &Path, args: PullArgs) -> Result<()> {
    let client = Client::new();
    let instance = normalize_instance(&args.instance);
    let fields_param = args.fields.as_deref().unwrap_or("").to_string();

    // Step 1: collect sys_ids to fetch — either the one provided, or query the list
    let sys_ids: Vec<String> = if let Some(ref sid) = args.sys_id {
        vec![sid.clone()]
    } else {
        let mut url = format!(
            "{server}/records/{table}?instance={inst}&limit={limit}&fields=sys_id",
            table = urlenc(&args.table),
            inst  = urlenc(&instance),
            limit = args.limit,
        );
        if let Some(q) = &args.query {
            url.push_str(&format!("&q={}", urlenc(q)));
        }

        let resp = api(&client, Method::GET, &url, None).await?;
        let records = resp["records"].as_array().cloned().unwrap_or_default();
        if records.is_empty() {
            println!("No records returned for {}.", args.table);
            return Ok(());
        }
        records.iter()
            .filter_map(|r| r["sys_id"].as_str().filter(|s| !s.is_empty()).map(String::from))
            .collect()
    };

    if sys_ids.is_empty() {
        println!("No records returned for {}.", args.table);
        return Ok(());
    }

    // Step 2: fetch each record individually to get full field data (agentQueryRecords
    // only returns sys_id; agentRestApi GET returns all fields).
    let mut written = 0usize;
    for sys_id in &sys_ids {
        let mut record_url = format!(
            "{server}/records/{t}/{s}?instance={inst}",
            t    = urlenc(&args.table),
            s    = urlenc(sys_id),
            inst = urlenc(&instance),
        );
        if !fields_param.is_empty() {
            record_url.push_str(&format!("&fields={}", urlenc(&fields_param)));
        }

        let resp = match api(&client, Method::GET, &record_url, None).await {
            Ok(v)  => v,
            Err(e) => { eprintln!("  {} {sys_id}: {e}", red("skip")); continue; }
        };

        let record = &resp["record"];
        if record.is_null() {
            eprintln!("  {} {sys_id}: no record in response", red("skip"));
            continue;
        }

        let mut file = Map::new();
        if let Some(obj) = record.as_object() {
            for (k, v) in obj {
                file.insert(k.clone(), unwrap_sn_field(v));
            }
        }
        file.insert(META_KEY.to_string(), json!({
            "instance": instance,
            "table":    args.table,
            "sys_id":   sys_id,
        }));
        let content = JVal::Object(file);

        let rpath = record_path(dir, &args.table, sys_id);
        let spath = state_path(dir, &args.table, sys_id);
        let is_new = !rpath.exists();

        write_toml(&rpath, &content)?;
        // Baseline records the instance so we can detect cloning later
        let mut sc = content.as_object().cloned().unwrap_or_default();
        set_state_meta(&mut sc, &instance);
        write_toml(&spath, &JVal::Object(sc))?;

        let label = record.get("number")
            .or_else(|| record.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| format!("  ({s})"))
            .unwrap_or_default();

        let verb = if is_new { green("pulled ") } else { yellow("updated") };
        println!("  {verb}  {}/{sys_id}.toml{label}", args.table);
        written += 1;
    }

    println!("\n{} record(s) written to {}/", bold(&written.to_string()), args.table);
    Ok(())
}

enum RecordStatus {
    New,
    Modified,
    Unchanged,
}

/// Local-only comparison (record file vs its baseline) — no network access,
/// safe to run off the main thread.
fn record_status(dir: &Path, table: &str, name: &str, path: &Path) -> Result<(String, RecordStatus)> {
    let local = read_toml(path)?;
    let spath = state_path(dir, table, name);

    if !spath.exists() {
        return Ok((
            format!("  {}  {table}/{name}.toml  (no baseline — will CREATE on push)", green("+")),
            RecordStatus::New,
        ));
    }

    let state = read_toml(&spath)?;
    let (changed, added) = diff(&editable_fields(&local), &editable_fields(&state));

    if has_diff(&changed, &added) {
        let n = changed.len() + added.len();
        Ok((
            format!("  {}  {table}/{name}.toml  ({n} field(s) modified)", yellow("M")),
            RecordStatus::Modified,
        ))
    } else {
        Ok((format!("  {}  {table}/{name}.toml", dim("=")), RecordStatus::Unchanged))
    }
}

fn cmd_status(dir: &Path) -> Result<()> {
    let records = collect_records(dir, None)?;
    if records.is_empty() {
        println!("No records found in {}.", dir.display());
        println!("Run `snstate pull <table> -i <instance>` to import records.");
        return Ok(());
    }

    // Purely local file I/O (record + baseline reads) — fan it out across
    // threads instead of walking records one at a time.
    let workers = std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4).min(records.len());
    let chunk_size = records.len().div_ceil(workers.max(1)).max(1);

    let results: Vec<Result<(String, RecordStatus)>> = std::thread::scope(|scope| {
        records
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk.iter()
                        .map(|(table, name, path)| record_status(dir, table, name, path))
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .flat_map(|h| h.join().expect("status worker panicked"))
            .collect()
    });

    let mut to_add = 0usize;
    let mut to_change = 0usize;
    let mut unchanged = 0usize;

    for r in results {
        let (line, status) = r?;
        println!("{line}");
        match status {
            RecordStatus::New       => to_add += 1,
            RecordStatus::Modified  => to_change += 1,
            RecordStatus::Unchanged => unchanged += 1,
        }
    }

    println!(
        "\n{} {} to add, {} to change, {} unchanged.",
        bold("Plan:"),
        paint("1;32", &to_add.to_string()),
        paint("1;33", &to_change.to_string()),
        dim(&unchanged.to_string()),
    );

    if to_add == 0 && to_change == 0 {
        println!("No local changes. Run `snstate plan` to compare against live ServiceNow.");
    } else {
        println!("Run `snstate plan` to diff against live SN, or `snstate push` to apply.");
    }
    Ok(())
}

async fn cmd_plan(server: &str, dir: &Path, args: TargetArgs) -> Result<()> {
    let client = Client::new();
    let records = collect_records(dir, args.target.as_deref())?;

    if records.is_empty() {
        println!("No records found. Run `snstate pull` first.");
        return Ok(());
    }

    let mut total_updates = 0usize;
    let mut total_creates = 0usize;
    let mut total_same    = 0usize;

    for (table, name, path) in &records {
        let local = read_toml(path)?;
        let instance = args.instance.as_deref()
            .map(normalize_instance)
            .or_else(|| meta_str(&local, "instance").map(String::from))
            .ok_or_else(|| anyhow::anyhow!("{table}/{name}: no instance — use -i or re-pull first"))?;

        let local_fields = editable_fields(&local);
        let sys_id = meta_str(&local, "sys_id");

        let Some(sys_id) = sys_id else {
            println!("  {}  {table}/{name}.toml  will CREATE", green("+"));
            total_creates += 1;
            continue;
        };

        let field_list = local_fields.keys().map(String::as_str).collect::<Vec<_>>().join(",");
        let url = format!(
            "{server}/records/{table}?instance={inst}&q=sys_id%3D{sys_id}&fields={fields}&limit=1",
            inst   = urlenc(&instance),
            fields = urlenc(&field_list),
        );

        let sn_resp = match api(&client, Method::GET, &url, None).await {
            Ok(v)  => v,
            Err(e) => { eprintln!("  {}  {table}/{name}: fetch failed — {e}", red("!")); continue; }
        };

        let sn_record = sn_resp["records"].as_array().and_then(|a| a.first()).cloned().unwrap_or(json!({}));
        let sn_fields: Map<String, JVal> = sn_record.as_object()
            .map(|o| o.iter().filter(|(k, _)| k.as_str() != "sys_id").map(|(k, v)| (k.clone(), unwrap_sn_field(v))).collect())
            .unwrap_or_default();

        let (changed, added) = diff(&local_fields, &sn_fields);
        if has_diff(&changed, &added) {
            println!("  {}  {table}/{name}.toml", yellow("~"));
            print_diff(&changed, &added);
            total_updates += 1;
        } else {
            println!("  {}  {table}/{name}.toml  (no changes)", dim("="));
            total_same += 1;
        }
    }

    println!();
    if total_updates > 0 || total_creates > 0 {
        println!(
            "{} {} to update, {} to create, {} unchanged.",
            bold("Plan:"),
            paint("1;33", &total_updates.to_string()),
            paint("1;32", &total_creates.to_string()),
            dim(&total_same.to_string()),
        );
        println!("Run `snstate push` to apply.");
    } else {
        println!("{} ({total_same} record(s) checked).", green("ServiceNow is already up to date"));
    }
    Ok(())
}

async fn cmd_refresh(server: &str, dir: &Path, args: RefreshArgs) -> Result<()> {
    let client = Client::new();
    let records = collect_records(dir, args.target.as_deref())?;

    if records.is_empty() {
        println!("No records found. Run `snstate pull` first.");
        return Ok(());
    }

    let mut refreshed = 0usize;
    let mut errors = 0usize;

    for (table, name, path) in &records {
        let local = read_toml(path)?;
        let instance = args.instance.as_deref()
            .map(normalize_instance)
            .or_else(|| meta_str(&local, "instance").map(String::from))
            .ok_or_else(|| anyhow::anyhow!("{table}/{name}: no instance — use -i or re-pull first"))?;

        let sys_id = meta_str(&local, "sys_id");
        let Some(sys_id) = sys_id else {
            eprintln!("  {}  {table}/{name}.toml: no sys_id — skipping refresh", red("!"));
            errors += 1;
            continue;
        };

        // Fetch current state from ServiceNow. sys_id must always be requested
        // explicitly (editable_fields() strips it as non-writable) — the
        // existence check below depends on it coming back.
        let local_fields = editable_fields(&local);
        let mut field_list: Vec<&str> = vec!["sys_id"];
        field_list.extend(local_fields.keys().map(String::as_str));
        let field_list = field_list.join(",");
        let url = format!(
            "{server}/records/{table}?instance={inst}&q=sys_id%3D{sys_id}&fields={fields}&limit=1",
            inst   = urlenc(&instance),
            fields = urlenc(&field_list),
        );

        let sn_resp = match api(&client, Method::GET, &url, None).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("  {}  {table}/{name}: fetch failed — {e}", red("!"));
                errors += 1;
                continue;
            }
        };

        let sn_record = sn_resp["records"].as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(json!({}));

        // If the record no longer exists on the instance, remove the baseline only.
        // The local file (desired state) is preserved — user may want to recreate it.
        let is_null_record = sn_record.is_null()
            || sn_record.get("sys_id").is_none();
        if is_null_record {
            println!("  {}  {table}/{name}.toml  deleted on {instance} — state cleared", red("✗"));
            let _ = std::fs::remove_file(state_path(dir, table, name));
            refreshed += 1;
            continue;
        }

        let sn_fields: Map<String, JVal> = sn_record.as_object()
            .map(|o| o.iter()
                .filter(|(k, _)| k.as_str() != "sys_id")
                .map(|(k, v)| (k.clone(), unwrap_sn_field(v)))
                .collect())
            .unwrap_or_default();

        // Update the state file with current values
        let spath = state_path(dir, table, name);
        let mut state_content = Map::new();
        for (k, v) in sn_fields {
            state_content.insert(k, v);
        }
        state_content.insert(STATE_META_KEY.to_string(), json!({ "instance": instance }));
        write_toml(&spath, &JVal::Object(state_content))?;

        println!("  {}  {table}/{name}.toml  refreshed from {instance}", green("ok"));
        refreshed += 1;
    }

    println!(
        "\n{} record(s) refreshed, {} error(s).",
        green(&refreshed.to_string()),
        if errors > 0 { red(&errors.to_string()) } else { dim(&errors.to_string()) },
    );
    Ok(())
}

async fn cmd_push(server: &str, dir: &Path, args: PushArgs) -> Result<()> {
    let client = Client::new();
    let records = collect_records(dir, args.target.as_deref())?;

    if records.is_empty() {
        println!("No records found. Run `snstate pull` first.");
        return Ok(());
    }

    if args.dry_run {
        println!("{}\n", cyan("Dry run — no changes will be made."));
    }

    let mut updated = 0usize;
    let mut created = 0usize;
    let mut skipped = 0usize;

    for (table, name, path) in &records {
        let local = read_toml(path)?;
        let instance = args.instance.as_deref()
            .map(normalize_instance)
            .or_else(|| meta_str(&local, "instance").map(String::from))
            .ok_or_else(|| anyhow::anyhow!("{table}/{name}: no instance — use -i or re-pull first"))?;

        let fields = editable_fields(&local);
        let sys_id_str = meta_str(&local, "sys_id").map(String::from);
        let spath = state_path(dir, table, name);
        let baseline = if spath.exists() { Some(read_toml(&spath)?) } else { None };

        // A record needs a forced-sys_id CREATE (a "clone") whenever we don't have a
        // baseline confirming it already exists on the target instance: either it was
        // never pulled/refreshed against this instance, or `refresh` found it missing
        // there and dropped the baseline. A baseline with no recorded instance (files
        // from before this existed) is assumed to belong to the target instance rather
        // than treated as a clone.
        let needs_clone = match (&sys_id_str, &baseline) {
            (Some(_), Some(state)) => {
                state_meta(state)
                    .and_then(|m| m.get("instance"))
                    .and_then(|v| v.as_str())
                    .is_some_and(|baseline_instance| baseline_instance != instance)
            }
            (Some(_), None) => true,
            (None, _) => false,
        };

        if needs_clone {
            // Clone: POST with the source sys_id forced, so the record keeps the same
            // identity on the destination instance.
            let id = sys_id_str.as_deref().expect("needs_clone implies Some(sys_id)");
            if args.dry_run {
                println!("  {}  {table}/{name}.toml  would CREATE {id} on {instance}", green("+"));
                continue;
            }
            let url  = format!("{server}/records/{table}");
            let mut clone_fields = fields.clone();
            clone_fields.insert("sys_id".to_string(), json!(id));
            let body = json!({ "instance": instance, "fields": clone_fields });
            match api(&client, Method::POST, &url, Some(&body)).await {
                Ok(_) => {
                    println!("  {}  {table}/{name}.toml  cloned to {instance} (sys_id={id})", green("ok"));
                    let mut state_content = fields.clone();
                    state_content.insert(STATE_META_KEY.to_string(), json!({ "instance": instance }));
                    write_toml(&spath, &JVal::Object(state_content))?;
                    created += 1;
                }
                Err(e) => eprintln!("  {} {table}/{name}.toml  clone failed: {e}", red("err")),
            }
            continue;
        }

        if let Some(state) = &baseline {
            let (changed, added) = diff(&fields, &editable_fields(state));
            if !args.force && !has_diff(&changed, &added) {
                println!("  {}  {table}/{name}.toml  (unchanged, skipping)", dim("-"));
                skipped += 1;
                continue;
            }
        }

        match sys_id_str {
            Some(ref id) => {
                if args.dry_run {
                    println!("  {}  {table}/{name}.toml  would PATCH {id} ({} field(s))", yellow("~"), fields.len());
                    continue;
                }
                let url  = format!("{server}/records/{table}/{id}");
                let body = json!({ "instance": instance, "fields": fields });
                match api(&client, Method::PATCH, &url, Some(&body)).await {
                    Ok(_) => {
                        println!("  {}  {table}/{name}.toml  patched", green("ok"));
                        write_toml(&state_path(dir, table, name), &local)?;
                        updated += 1;
                    }
                    Err(e) => eprintln!("  {} {table}/{name}.toml  PATCH failed: {e}", red("err")),
                }
            }
            None => {
                if args.dry_run {
                    println!("  {}  {table}/{name}.toml  would CREATE ({} field(s))", green("+"), fields.len());
                    continue;
                }
                let url  = format!("{server}/records/{table}");
                let body = json!({ "instance": instance, "fields": fields });
                match api(&client, Method::POST, &url, Some(&body)).await {
                    Ok(resp) => {
                        let new_id = resp["sys_id"].as_str().unwrap_or("unknown").to_string();
                        println!("  {}  {table}/{name}.toml  created → {new_id}", green("ok"));

                        // Rewrite under the real sys_id
                        let mut updated_file = local.as_object().cloned().unwrap_or_default();
                        updated_file.insert("sys_id".to_string(), json!(&new_id));
                        updated_file.insert(META_KEY.to_string(), json!({
                            "instance": instance, "table": table, "sys_id": new_id,
                        }));
                        let content = JVal::Object(updated_file);
                        let new_rpath = record_path(dir, table, &new_id);
                        write_toml(&new_rpath, &content)?;
                        write_toml(&state_path(dir, table, &new_id), &content)?;

                        // Remove placeholder files
                        if path != &new_rpath {
                            let _ = std::fs::remove_file(path);
                            let _ = std::fs::remove_file(state_path(dir, table, &name));
                            println!("       {}", dim(&format!("renamed {name}.toml -> {new_id}.toml")));
                        }
                        created += 1;
                    }
                    Err(e) => eprintln!("  {} {table}/{name}.toml  CREATE failed: {e}", red("err")),
                }
            }
        }
    }

    if !args.dry_run {
        println!(
            "\n{} {} updated, {} created, {} skipped.",
            bold("Apply complete!"),
            paint("1;33", &updated.to_string()),
            paint("1;32", &created.to_string()),
            dim(&skipped.to_string()),
        );
    }
    Ok(())
}

// ── entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("{} {e:#}", bold(&red("error:")));
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let server = cli.server.trim_end_matches('/').to_string();
    let dir = &cli.dir;
    match cli.command {
        Cmd::Pull(args)             => cmd_pull(&server, dir, args).await,
        Cmd::Status                 => cmd_status(dir),
        Cmd::Plan(args)             => cmd_plan(&server, dir, args).await,
        Cmd::Refresh(args)          => cmd_refresh(&server, dir, args).await,
        Cmd::Push(args) |
        Cmd::Apply(args)            => cmd_push(&server, dir, args).await,
    }
}
