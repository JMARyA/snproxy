pub mod context;
pub mod coverage;
pub mod expr;
pub mod fetch;
pub mod input;
pub mod resolve;
pub mod transform;

use serde_json::Value;
use sncore::Client;

use crate::ast::{Pipeline, Sink, Step};
use crate::error::Result;
use context::ExecContext;

// ── Public API ────────────────────────────────────────────────────────────────

pub async fn run(
    pipeline: Pipeline,
    client: Client,
    instance: String,
    output_path: Option<String>,
) -> Result<()> {
    let mut ctx = ExecContext::new(client, instance);

    // Load let bindings
    for decl in &pipeline.lets {
        let values = input::load_let(decl)?;
        eprintln!("  · let {} = {} values", decl.name, values.len());
        ctx.lets.insert(decl.name.clone(), values);
    }

    eprintln!("\nPipeline: {}", pipeline.name.as_deref().unwrap_or("unnamed"));

    // Fetch source
    let rows = fetch::run_fetch(&pipeline.source, &ctx).await?;

    // Run steps
    let rows = run_steps(&pipeline.steps, rows, &mut ctx).await?;

    // Sink
    eprintln!("\n  → {} rows total", rows.len());
    match &pipeline.sink {
        Sink::Csv(path) => {
            let path = output_path.as_deref().or(path.as_deref());
            crate::output::write_csv(&rows, path)?;
        }
        Sink::Json(path) => {
            let path = output_path.as_deref().or(path.as_deref());
            crate::output::write_json(&rows, path)?;
        }
        Sink::Table => {
            crate::output::write_table(&rows)?;
        }
    }

    // Print stage summary
    eprintln!("\n{}", summary_table(&ctx));
    Ok(())
}

pub(crate) async fn run_steps(
    steps: &[Step],
    mut rows: Vec<Value>,
    ctx: &mut ExecContext,
) -> Result<Vec<Value>> {
    for step in steps {
        rows = run_step(step, rows, ctx).await?;
    }
    Ok(rows)
}

async fn run_step(step: &Step, rows: Vec<Value>, ctx: &mut ExecContext) -> Result<Vec<Value>> {
    match step {
        Step::Coverage(s)    => coverage::run_coverage(s, rows, ctx).await,
        Step::Resolve(s)     => resolve::run_resolve(s, rows, ctx).await,
        Step::ResolveList(s) => resolve::run_resolve_list(s, rows, ctx).await,
        Step::FlatMap(s)     => transform::run_flat_map(s, rows, ctx).await,
        Step::Map(s)         => transform::run_map(s, rows, ctx).await,
        Step::Filter(s)      => transform::run_filter(s, rows, ctx).await,
        Step::Dedup { on_field } => transform::run_dedup(on_field, rows, ctx).await,
        Step::WarnEmpty { message } => transform::run_warn_empty(message, rows, ctx).await,
    }
}

// ── Explain ───────────────────────────────────────────────────────────────────

pub fn explain(pipeline: &Pipeline) {
    println!("Pipeline: {}", pipeline.name.as_deref().unwrap_or("unnamed"));
    println!();

    for decl in &pipeline.lets {
        match &decl.source {
            crate::ast::InputSource::Csv { path, col, skip } => {
                println!("  let {} = csv {:?} [col={}, skip={}]", decl.name, path, col, skip);
            }
            crate::ast::InputSource::Literal(v) => {
                println!("  let {} = [{} values]", decl.name, v.len());
            }
        }
    }

    println!();
    println!(
        "  fetch {}  fields=[{}]  chunk_size={}  paginate={}",
        pipeline.source.table,
        pipeline.source.fields.join(", "),
        pipeline.source.chunk_size,
        pipeline.source.paginate,
    );
    if let Some(q) = &pipeline.source.query {
        println!("    where {:?}", q);
    }

    for step in &pipeline.steps {
        match step {
            Step::Coverage(s) => println!(
                "  |> coverage {} on {} [missing={:?}]",
                s.source_name, s.on_field, s.on_missing
            ),
            Step::Resolve(s) => println!(
                "  |> resolve .{} → {} [{}]  on_missing={:?}",
                s.field, s.table, s.fields.join(","), s.on_missing
            ),
            Step::ResolveList(s) => println!(
                "  |> resolve_list .{} → {} [{}]  on_missing={:?}",
                s.field, s.table, s.fields.join(","), s.on_missing
            ),
            Step::FlatMap(s) => println!("  |> flat_map {} {{ ... }}", s.var),
            Step::Map(s) => println!("  |> map {} {{ {} fields }}", s.var, s.fields.len()),
            Step::Filter(s) => println!("  |> filter {}: <expr>", s.var),
            Step::Dedup { on_field } => println!(
                "  |> dedup{}",
                on_field.as_deref().map(|f| format!(" on {f}")).unwrap_or_default()
            ),
            Step::WarnEmpty { message } => println!(
                "  |> warn_empty{}",
                message.as_deref().map(|m| format!(" {:?}", m)).unwrap_or_default()
            ),
        }
    }

    println!();
    match &pipeline.sink {
        Sink::Csv(p)  => println!("  sink: to_csv{}", p.as_deref().map(|p| format!(" {p:?}")).unwrap_or_default()),
        Sink::Json(p) => println!("  sink: to_json{}", p.as_deref().map(|p| format!(" {p:?}")).unwrap_or_default()),
        Sink::Table   => println!("  sink: to_table"),
    }
}

// ── Summary table ─────────────────────────────────────────────────────────────

fn summary_table(ctx: &ExecContext) -> String {
    let stages = ctx.log.snapshot();
    if stages.is_empty() {
        return String::new();
    }

    let name_w = stages.iter().map(|s| s.name.len()).max().unwrap_or(10).max(10);
    let mut out = format!(
        "  ┌─{:─<nw$}─┬──────┬──────┬──────┬────────┐\n",
        "",
        nw = name_w
    );
    out += &format!(
        "  │ {:<nw$} │   In │  Out │  Err │     ms │\n",
        "Stage",
        nw = name_w
    );
    out += &format!(
        "  ├─{:─<nw$}─┼──────┼──────┼──────┼────────┤\n",
        "",
        nw = name_w
    );
    for s in &stages {
        let warn = if !s.warnings.is_empty() { " !" } else { "  " };
        out += &format!(
            "  │ {:<nw$} │{:>5} │{:>5} │{:>5} │{:>7} │{}\n",
            s.name,
            s.rows_in,
            s.rows_out,
            s.errors.len(),
            s.duration_ms,
            warn,
            nw = name_w,
        );
    }
    out += &format!(
        "  └─{:─<nw$}─┴──────┴──────┴──────┴────────┘\n",
        "",
        nw = name_w
    );
    out
}
