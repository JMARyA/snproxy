/// Top-level pipeline definition
#[derive(Debug, Clone)]
pub struct Pipeline {
    pub name: Option<String>,
    pub lets: Vec<LetDecl>,
    pub source: Source,
    pub steps: Vec<Step>,
    pub sink: Sink,
}

// ── Let bindings ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LetDecl {
    pub name: String,
    pub source: InputSource,
    pub transforms: Vec<InputTransform>,
}

#[derive(Debug, Clone)]
pub enum InputSource {
    Csv { path: String, col: usize, skip: usize },
    Literal(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputTransform {
    Trim,
    Dedup,
    WarnEmpty,
}

// ── Source (from) ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Source {
    pub table: String,
    pub query: Option<String>,
    pub fields: Vec<String>,
    pub chunk_size: usize,
    pub paginate: bool,
    pub escape_values: bool,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            table: String::new(),
            query: None,
            fields: vec![],
            chunk_size: 50,
            paginate: false,
            escape_values: false,
        }
    }
}

// ── Steps ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Step {
    Coverage(CoverageStep),
    Resolve(ResolveStep),
    ResolveList(ResolveListStep),
    FlatMap(FlatMapStep),
    Map(MapStep),
    Filter(FilterStep),
    Dedup { on_field: Option<String> },
    WarnEmpty { message: Option<String> },
}

#[derive(Debug, Clone)]
pub struct CoverageStep {
    pub source_name: String,
    pub on_field: String,
    pub match_trim: bool,
    pub match_case_insensitive: bool,
    pub on_missing: OnMissing,
    pub on_duplicate: OnDuplicate,
}

#[derive(Debug, Clone)]
pub struct ResolveStep {
    pub field: String,
    pub table: String,
    pub fields: Vec<String>,
    pub skip_null_id: bool,
    pub on_missing: OnMissing,
    pub on_error: OnError,
}

#[derive(Debug, Clone)]
pub struct ResolveListStep {
    pub field: String,
    pub table: String,
    pub fields: Vec<String>,
    pub separator: char,
    pub skip_empty: bool,
    pub skip_null_id: bool,
    pub on_missing: OnMissing,
    pub on_error: OnError,
}

impl Default for ResolveListStep {
    fn default() -> Self {
        Self {
            field: String::new(),
            table: String::new(),
            fields: vec![],
            separator: ',',
            skip_empty: false,
            skip_null_id: false,
            on_missing: OnMissing::Warn,
            on_error: OnError::KeepRow,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FlatMapStep {
    pub var: String,
    pub pipeline: Box<Pipeline>,
}

#[derive(Debug, Clone)]
pub struct MapStep {
    pub var: String,
    pub fields: Vec<(String, Expr)>,
}

#[derive(Debug, Clone)]
pub struct FilterStep {
    pub var: String,
    pub expr: Expr,
}

// ── Sink ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Sink {
    Csv(Option<String>),
    Json(Option<String>),
    Table,
}

// ── Options ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Default)]
pub enum OnMissing {
    #[default]
    Warn,
    Error,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum OnDuplicate {
    #[default]
    Warn,
    Error,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum OnError {
    #[default]
    KeepRow,
    DropRow,
    Abort,
}

// ── Expressions ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Expr {
    /// field path: `row.field.sub` — segments include "[]" for list flatten
    Field(Vec<Segment>),
    Str(String),
    Int(i64),
    Bool(bool),
    Null,
    EmptyList,

    BinOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Not(Box<Expr>),

    /// left ?? right
    Coalesce(Box<Expr>, Box<Expr>),

    /// expr |> filter var: cond
    ListFilter {
        list: Box<Expr>,
        var: String,
        cond: Box<Expr>,
    },
    /// expr |> map var: body
    ListMap {
        list: Box<Expr>,
        var: String,
        body: Box<Expr>,
    },
    /// expr |> dedup
    ListDedup(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Field(String),
    /// `[]` — flatten: extract next field from each element
    Flatten,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Eq, Ne, Lt, Gt, Le, Ge,
    And, Or,
    Contains,
    StartsWith,
    EndsWith,
    RegexMatch,
    RegexNotMatch,
}
