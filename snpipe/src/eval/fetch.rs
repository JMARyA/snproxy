use std::collections::HashMap;

use futures::future::join_all;
use serde_json::Value;

use crate::ast::Source;
use crate::error::{Result, SnpipeError};
use crate::eval::context::{ExecContext, StageTimer};
use crate::eval::input::escape_sn_value;

const MAX_LIMIT: u32 = 1000;
const MAX_CONCURRENT_CHUNKS: usize = 5;

/// Execute the `from` source — fetch records from SN via snproxy.
/// Handles: chunked IN queries, pagination, escape_values, interpolation.
pub async fn run_fetch(source: &Source, ctx: &ExecContext) -> Result<Vec<Value>> {
    let mut timer = StageTimer::start(format!("fetch {}", source.table), 0);

    let rows = fetch_source(source, ctx, &mut timer).await?;
    let n = rows.len();
    ctx.record(timer.finish(n));
    Ok(rows)
}

async fn fetch_source(
    source: &Source,
    ctx: &ExecContext,
    timer: &mut StageTimer,
) -> Result<Vec<Value>> {
    let fields = source.fields.join(",");
    let base_query = source.query.as_deref().unwrap_or("");

    // Interpolate ${letname} in the query string with IN lists from let bindings.
    // E.g. "u_short_nameIN${names}" → "u_short_nameINACME,FOO,BAR"
    let queries = interpolate_query(base_query, ctx, source.chunk_size, source.escape_values)?;

    if queries.is_empty() {
        // No IN substitution — single query
        return fetch_paginated(&source.table, &fields, base_query, source, ctx, timer).await;
    }

    // Chunked: run queries in parallel batches of MAX_CONCURRENT_CHUNKS
    let mut all_rows = Vec::new();
    for batch in queries.chunks(MAX_CONCURRENT_CHUNKS) {
        let futs = batch.iter().map(|q| {
            let table = source.table.clone();
            let fields = fields.clone();
            let q = q.clone();
            let ctx = ctx.clone();
            let paginate = source.paginate;
            async move {
                fetch_single(&table, &fields, &q, MAX_LIMIT, paginate, &ctx).await
            }
        });
        let results = join_all(futs).await;
        for res in results {
            match res {
                Ok(rows) => all_rows.extend(rows),
                Err(e) => {
                    timer.push_error(None, format!("chunk fetch failed: {e}"));
                }
            }
        }
    }
    Ok(all_rows)
}

/// Expand `${varname}` placeholders in a query string.
/// If any placeholder is found, splits the values into chunks and returns
/// one query string per chunk. Returns empty vec if no placeholders.
fn interpolate_query(
    query: &str,
    ctx: &ExecContext,
    chunk_size: usize,
    escape: bool,
) -> Result<Vec<String>> {
    // Find placeholders: ${identifier}
    let re = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();

    if !re.is_match(query) {
        return Ok(vec![]);
    }

    // Collect all placeholders and their resolved value lists
    let mut replacements: HashMap<String, Vec<String>> = HashMap::new();
    for cap in re.captures_iter(query) {
        let full = cap.get(0).unwrap().as_str().to_string();
        let name = cap.get(1).unwrap().as_str().trim();
        if replacements.contains_key(&full) {
            continue;
        }
        let values = ctx
            .get_let(name)
            .ok_or_else(|| SnpipeError::other(format!("undefined variable '{name}' in query")))?
            .clone();
        let values = if escape {
            values.iter().map(|s| escape_sn_value(s)).collect()
        } else {
            values
        };
        replacements.insert(full, values);
    }

    // For simplicity: if there's exactly one placeholder, chunk its values.
    // Multiple placeholders are expanded as-is (no cross-product).
    if replacements.len() == 1 {
        let (placeholder, values) = replacements.iter().next().unwrap();
        let chunks: Vec<Vec<String>> = values.chunks(chunk_size).map(|c| c.to_vec()).collect();
        return Ok(chunks
            .into_iter()
            .map(|chunk| query.replace(placeholder.as_str(), &chunk.join(",")))
            .collect());
    }

    // Multiple placeholders: substitute all at once (no chunking)
    let mut q = query.to_string();
    for (placeholder, values) in &replacements {
        let joined = if escape {
            values.iter().map(|s| escape_sn_value(s)).collect::<Vec<_>>().join(",")
        } else {
            values.join(",")
        };
        q = q.replace(placeholder.as_str(), &joined);
    }
    Ok(vec![q])
}

async fn fetch_paginated(
    table: &str,
    fields: &str,
    query: &str,
    source: &Source,
    ctx: &ExecContext,
    timer: &mut StageTimer,
) -> Result<Vec<Value>> {
    let mut all = Vec::new();
    let rows = fetch_single(table, fields, query, MAX_LIMIT, source.paginate, ctx).await?;
    let count = rows.len();
    all.extend(rows);

    if source.paginate && count == MAX_LIMIT as usize {
        timer.push_warning(format!(
            "first page returned {MAX_LIMIT} rows (limit), fetching more"
        ));
        let mut offset = MAX_LIMIT;
        loop {
            let q = if query.is_empty() {
                format!("ORDERBYsys_created_on")
            } else {
                format!("{query}^ORDERBYsys_created_on")
            };
            // snproxy doesn't expose offset directly; use sysparm_offset via raw query append
            let paged_q = format!("{q}&sysparm_offset={offset}");
            let rows = ctx
                .client
                .list_records(table, fields, &paged_q, MAX_LIMIT, "")
                .await
                .map_err(|e| SnpipeError::Http(e))?;
            let n = rows.len();
            all.extend(rows);
            if n < MAX_LIMIT as usize { break; }
            offset += MAX_LIMIT;
        }
    }

    Ok(all)
}

async fn fetch_single(
    table: &str,
    fields: &str,
    query: &str,
    limit: u32,
    _paginate: bool,
    ctx: &ExecContext,
) -> Result<Vec<Value>> {
    ctx.client
        .list_records(table, fields, query, limit, "")
        .await
        .map_err(|e| SnpipeError::Http(e))
}
