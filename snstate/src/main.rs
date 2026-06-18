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
use std::path::{Path, PathBuf};
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
    dir.join(table).join(format!("{name}.state.toml"))
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

/// Fields from a record file, excluding _meta and sys_id (identity, not a writable field).
fn editable_fields(v: &JVal) -> Map<String, JVal> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, _)| k.as_str() != META_KEY && k.as_str() != "sys_id")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default()
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
            if fname.ends_with(".state.toml") || !fname.ends_with(".toml") {
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
        println!("       {field}: {:?} -> {:?}", old, new);
    }
    for field in added {
        println!("       + {field} (not in current SN state)");
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
            Err(e) => { eprintln!("  skip {sys_id}: {e}"); continue; }
        };

        let record = &resp["record"];
        if record.is_null() {
            eprintln!("  skip {sys_id}: no record in response");
            continue;
        }

        let mut file = Map::new();
        if let Some(obj) = record.as_object() {
            for (k, v) in obj {
                file.insert(k.clone(), v.clone());
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
        write_toml(&spath, &content)?;

        let label = record.get("number")
            .or_else(|| record.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| format!("  ({s})"))
            .unwrap_or_default();

        println!("  {}  {}/{sys_id}.toml{label}", if is_new { "pulled " } else { "updated" }, args.table);
        written += 1;
    }

    println!("\n{written} record(s) written to {}/", args.table);
    Ok(())
}

fn cmd_status(dir: &Path) -> Result<()> {
    let records = collect_records(dir, None)?;
    if records.is_empty() {
        println!("No records found in {}.", dir.display());
        println!("Run `snstate pull <table> -i <instance>` to import records.");
        return Ok(());
    }

    let mut any = false;
    for (table, name, path) in &records {
        let local = read_toml(path)?;
        let spath = state_path(dir, table, name);

        if !spath.exists() {
            println!("  +  {table}/{name}.toml  (no baseline — will CREATE on push)");
            any = true;
            continue;
        }

        let state  = read_toml(&spath)?;
        let (changed, added) = diff(&editable_fields(&local), &editable_fields(&state));

        if has_diff(&changed, &added) {
            println!("  M  {table}/{name}.toml  ({} field(s) modified)", changed.len() + added.len());
            any = true;
        } else {
            println!("  =  {table}/{name}.toml");
        }
    }

    if !any {
        println!("\nNo local changes. Run `snstate plan` to compare against live ServiceNow.");
    } else {
        println!("\nRun `snstate plan` to diff against live SN, or `snstate push` to apply.");
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
            println!("  +  {table}/{name}.toml  will CREATE");
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
            Err(e) => { eprintln!("  !  {table}/{name}: fetch failed — {e}"); continue; }
        };

        let sn_record = sn_resp["records"].as_array().and_then(|a| a.first()).cloned().unwrap_or(json!({}));
        let sn_fields: Map<String, JVal> = sn_record.as_object()
            .map(|o| o.iter().filter(|(k, _)| k.as_str() != "sys_id").map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let (changed, added) = diff(&local_fields, &sn_fields);
        if has_diff(&changed, &added) {
            println!("  ~  {table}/{name}.toml");
            print_diff(&changed, &added);
            total_updates += 1;
        } else {
            println!("  =  {table}/{name}.toml  (no changes)");
            total_same += 1;
        }
    }

    println!();
    if total_updates > 0 || total_creates > 0 {
        println!("Plan: {total_updates} to update, {total_creates} to create, {total_same} unchanged.");
        println!("Run `snstate push` to apply.");
    } else {
        println!("ServiceNow is already up to date ({total_same} record(s) checked).");
    }
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
        println!("Dry run — no changes will be made.\n");
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
        let sys_id = meta_str(&local, "sys_id").map(String::from);

        // Skip if unchanged vs baseline (unless --force or new record)
        if !args.force && sys_id.is_some() {
            let spath = state_path(dir, table, name);
            if spath.exists() {
                let state = read_toml(&spath)?;
                let (changed, added) = diff(&fields, &editable_fields(&state));
                if !has_diff(&changed, &added) {
                    println!("  -  {table}/{name}.toml  (unchanged, skipping)");
                    skipped += 1;
                    continue;
                }
            }
        }

        match sys_id {
            Some(ref id) => {
                if args.dry_run {
                    println!("  ~  {table}/{name}.toml  would PATCH {id} ({} field(s))", fields.len());
                    continue;
                }
                let url  = format!("{server}/records/{table}/{id}");
                let body = json!({ "instance": instance, "fields": fields });
                match api(&client, Method::PATCH, &url, Some(&body)).await {
                    Ok(_) => {
                        println!("  ok  {table}/{name}.toml  patched");
                        write_toml(&state_path(dir, table, name), &local)?;
                        updated += 1;
                    }
                    Err(e) => eprintln!("  err {table}/{name}.toml  PATCH failed: {e}"),
                }
            }
            None => {
                if args.dry_run {
                    println!("  +  {table}/{name}.toml  would CREATE ({} field(s))", fields.len());
                    continue;
                }
                let url  = format!("{server}/records/{table}");
                let body = json!({ "instance": instance, "fields": fields });
                match api(&client, Method::POST, &url, Some(&body)).await {
                    Ok(resp) => {
                        let new_id = resp["sys_id"].as_str().unwrap_or("unknown").to_string();
                        println!("  ok  {table}/{name}.toml  created → {new_id}");

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
                            println!("       renamed {name}.toml -> {new_id}.toml");
                        }
                        created += 1;
                    }
                    Err(e) => eprintln!("  err {table}/{name}.toml  CREATE failed: {e}"),
                }
            }
        }
    }

    if !args.dry_run {
        println!("\n{updated} updated, {created} created, {skipped} skipped.");
    }
    Ok(())
}

// ── entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e:#}");
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
        Cmd::Push(args) |
        Cmd::Apply(args)            => cmd_push(&server, dir, args).await,
    }
}
