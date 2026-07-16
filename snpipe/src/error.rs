use thiserror::Error;

#[derive(Debug, Error)]
pub enum SnpipeError {
    #[error("parse error at line {line}: {msg}")]
    Parse { line: usize, msg: String },

    #[error("eval error in stage '{stage}': {msg}")]
    Eval { stage: String, msg: String },

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] ::csv::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

impl SnpipeError {
    pub fn parse(line: usize, msg: impl Into<String>) -> Self {
        Self::Parse { line, msg: msg.into() }
    }
    pub fn eval(stage: impl Into<String>, msg: impl Into<String>) -> Self {
        Self::Eval { stage: stage.into(), msg: msg.into() }
    }
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, SnpipeError>;
