use crate::ast::{InputSource, InputTransform, LetDecl};
use crate::error::{Result, SnpipeError};

/// Load and apply transforms to a let binding, returning the final string list.
pub fn load_let(decl: &LetDecl) -> Result<Vec<String>> {
    let mut values = load_source(&decl.source)?;
    for xform in &decl.transforms {
        values = apply_transform(values, xform);
    }
    Ok(values)
}

fn load_source(src: &InputSource) -> Result<Vec<String>> {
    match src {
        InputSource::Csv { path, col, skip } => load_csv(path, *col, *skip),
        InputSource::Literal(vals) => Ok(vals.clone()),
    }
}

fn load_csv(path: &str, col: usize, skip: usize) -> Result<Vec<String>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)?;

    let mut values = Vec::new();
    for (i, result) in rdr.records().enumerate() {
        if i < skip {
            continue;
        }
        let record = result?;
        if let Some(val) = record.get(col) {
            let v = val.trim().to_string();
            if !v.is_empty() {
                values.push(v);
            }
        }
    }

    if values.is_empty() {
        eprintln!("WARN  input: CSV '{path}' col={col} skip={skip} produced 0 values");
    }

    Ok(values)
}

fn apply_transform(mut values: Vec<String>, xform: &InputTransform) -> Vec<String> {
    match xform {
        InputTransform::Trim => {
            values.iter_mut().for_each(|s| *s = s.trim().to_string());
            values.retain(|s| !s.is_empty());
            values
        }
        InputTransform::Dedup => {
            let mut seen = std::collections::HashSet::new();
            values.retain(|s| seen.insert(s.clone()));
            values
        }
        InputTransform::WarnEmpty => {
            let empty = values.iter().filter(|s| s.is_empty()).count();
            if empty > 0 {
                eprintln!("WARN  input: {empty} empty value(s) in input source");
            }
            values
        }
    }
}

/// Escape SN encoded query special chars in a value intended for an IN list.
/// Chars `^`, `&`, `=` are SN query syntax chars and break encoded queries if unescaped.
pub fn escape_sn_value(s: &str) -> String {
    // SN URL-encodes values in encoded queries; for IN lists we need to
    // escape ^ (field separator) and & (URL param separator).
    // In practice we percent-encode these chars.
    s.replace('%', "%25")
     .replace('^', "%5E")
     .replace('&', "%26")
}
