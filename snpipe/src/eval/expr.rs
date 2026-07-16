use std::collections::HashMap;

use regex::Regex;
use serde_json::{json, Value};

use crate::ast::{BinOp, Expr, Segment};
use crate::error::{Result, SnpipeError};

// ── Evaluation context ────────────────────────────────────────────────────────

/// Local variable scope for expression evaluation.
/// Typically contains the current row plus any outer loop variables.
pub type Scope = HashMap<String, Value>;

pub fn scope_from_row(var: &str, row: &Value) -> Scope {
    let mut s = Scope::new();
    s.insert(var.to_string(), row.clone());
    s
}

pub fn scope_with(mut parent: Scope, var: &str, val: Value) -> Scope {
    parent.insert(var.to_string(), val);
    parent
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn eval(expr: &Expr, scope: &Scope) -> Result<Value> {
    match expr {
        Expr::Str(s)       => Ok(Value::String(s.clone())),
        Expr::Int(n)       => Ok(json!(n)),
        Expr::Bool(b)      => Ok(json!(b)),
        Expr::Null         => Ok(Value::Null),
        Expr::EmptyList    => Ok(json!([])),

        Expr::Field(segs)  => eval_field(segs, scope),
        Expr::Not(inner)   => Ok(json!(!is_truthy(&eval(inner, scope)?))),

        Expr::BinOp { op, left, right } => eval_binop(op, left, right, scope),
        Expr::Coalesce(l, r)            => eval_coalesce(l, r, scope),

        Expr::ListFilter { list, var, cond } => eval_list_filter(list, var, cond, scope),
        Expr::ListMap    { list, var, body } => eval_list_map(list, var, body, scope),
        Expr::ListDedup(inner)               => eval_list_dedup(inner, scope),
    }
}

// ── Field path ────────────────────────────────────────────────────────────────

fn eval_field(segs: &[Segment], scope: &Scope) -> Result<Value> {
    if segs.is_empty() {
        return Ok(Value::Null);
    }

    // First segment must be a variable name (never Flatten at position 0)
    let first = match &segs[0] {
        Segment::Field(name) => name,
        Segment::Flatten => return Err(SnpipeError::other("field path cannot start with []")),
    };

    let mut cur = scope.get(first).cloned().unwrap_or(Value::Null);
    let mut list_mode = false;

    for seg in &segs[1..] {
        match seg {
            Segment::Flatten => {
                // Switch to list mode. cur should already be an array.
                if !cur.is_array() {
                    // wrap scalar as single-element list
                    cur = json!([cur]);
                }
                list_mode = true;
            }
            Segment::Field(name) => {
                if list_mode {
                    // map over elements
                    let arr = cur.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
                    cur = Value::Array(
                        arr.iter().map(|item| get_field(item, name)).collect(),
                    );
                } else {
                    cur = get_field(&cur, name);
                }
            }
        }
    }

    Ok(cur)
}

fn get_field(val: &Value, name: &str) -> Value {
    match val {
        Value::Object(m) => m.get(name).cloned().unwrap_or(Value::Null),
        Value::Null => Value::Null,
        // for arrays: implicit map (shouldn't normally happen but be defensive)
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|item| get_field(item, name)).collect())
        }
        _ => Value::Null,
    }
}

// ── BinOp ─────────────────────────────────────────────────────────────────────

fn eval_binop(op: &BinOp, left: &Expr, right: &Expr, scope: &Scope) -> Result<Value> {
    // Short-circuit for And/Or
    match op {
        BinOp::And => {
            let l = eval(left, scope)?;
            if !is_truthy(&l) { return Ok(json!(false)); }
            return Ok(json!(is_truthy(&eval(right, scope)?)));
        }
        BinOp::Or => {
            let l = eval(left, scope)?;
            if is_truthy(&l) { return Ok(json!(true)); }
            return Ok(json!(is_truthy(&eval(right, scope)?)));
        }
        _ => {}
    }

    let l = eval(left, scope)?;
    let r = eval(right, scope)?;

    let result = match op {
        BinOp::Eq  => json!(val_eq(&l, &r)),
        BinOp::Ne  => json!(!val_eq(&l, &r)),
        BinOp::Lt  => json!(val_cmp(&l, &r) < 0),
        BinOp::Gt  => json!(val_cmp(&l, &r) > 0),
        BinOp::Le  => json!(val_cmp(&l, &r) <= 0),
        BinOp::Ge  => json!(val_cmp(&l, &r) >= 0),
        BinOp::Contains   => json!(val_contains(&l, &r)),
        BinOp::StartsWith => json!(as_str(&l).starts_with(as_str(&r).as_str())),
        BinOp::EndsWith   => json!(as_str(&l).ends_with(as_str(&r).as_str())),
        BinOp::RegexMatch    => json!(regex_match(&l, &r)?),
        BinOp::RegexNotMatch => json!(!regex_match(&l, &r)?),
        BinOp::And | BinOp::Or => unreachable!(),
    };
    Ok(result)
}

fn val_eq(l: &Value, r: &Value) -> bool {
    // normalize: compare as strings if one side is a string
    match (l, r) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        // allow comparing number to string "0" etc.
        (Value::Number(n), Value::String(s)) | (Value::String(s), Value::Number(n)) => {
            n.to_string() == *s
        }
        // null == "" or "" == null
        (Value::Null, Value::String(s)) | (Value::String(s), Value::Null) => s.is_empty(),
        // Array equality
        (Value::Array(a), Value::Array(b)) => a == b,
        _ => false,
    }
}

fn val_cmp(l: &Value, r: &Value) -> i32 {
    match (l, r) {
        (Value::Number(a), Value::Number(b)) => {
            let af = a.as_f64().unwrap_or(0.0);
            let bf = b.as_f64().unwrap_or(0.0);
            af.partial_cmp(&bf).map(|o| o as i32).unwrap_or(0)
        }
        _ => as_str(l).cmp(&as_str(r)) as i32,
    }
}

fn val_contains(l: &Value, r: &Value) -> bool {
    match l {
        Value::Array(arr) => arr.iter().any(|item| val_eq(item, r)),
        _ => as_str(l).contains(as_str(r).as_str()),
    }
}

fn regex_match(val: &Value, pattern: &Value) -> Result<bool> {
    let s = as_str(val);
    let p = as_str(pattern);
    let re = Regex::new(&p)
        .map_err(|e| SnpipeError::other(format!("invalid regex '{p}': {e}")))?;
    Ok(re.is_match(&s))
}

fn as_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

// ── Coalesce ──────────────────────────────────────────────────────────────────

fn eval_coalesce(left: &Expr, right: &Expr, scope: &Scope) -> Result<Value> {
    let l = eval(left, scope)?;
    if is_null_or_empty(&l) {
        eval(right, scope)
    } else {
        Ok(l)
    }
}

fn is_null_or_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

// ── List ops ──────────────────────────────────────────────────────────────────

fn eval_list_filter(list: &Expr, var: &str, cond: &Expr, scope: &Scope) -> Result<Value> {
    let l = eval(list, scope)?;
    let arr = match l {
        Value::Array(a) => a,
        Value::Null => return Ok(json!([])),
        other => vec![other],
    };
    let mut out = Vec::new();
    for item in arr {
        let inner_scope = scope_with(scope.clone(), var, item.clone());
        if is_truthy(&eval(cond, &inner_scope)?) {
            out.push(item);
        }
    }
    Ok(Value::Array(out))
}

fn eval_list_map(list: &Expr, var: &str, body: &Expr, scope: &Scope) -> Result<Value> {
    let l = eval(list, scope)?;
    let arr = match l {
        Value::Array(a) => a,
        Value::Null => return Ok(json!([])),
        other => vec![other],
    };
    let mut out = Vec::new();
    for item in arr {
        let inner_scope = scope_with(scope.clone(), var, item);
        out.push(eval(body, &inner_scope)?);
    }
    Ok(Value::Array(out))
}

fn eval_list_dedup(inner: &Expr, scope: &Scope) -> Result<Value> {
    let l = eval(inner, scope)?;
    let arr = match l {
        Value::Array(a) => a,
        other => return Ok(other),
    };
    let mut seen = Vec::new();
    for item in arr {
        if !seen.contains(&item) {
            seen.push(item);
        }
    }
    Ok(Value::Array(seen))
}

// ── Truthiness ────────────────────────────────────────────────────────────────

pub fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(_) => true,
    }
}
