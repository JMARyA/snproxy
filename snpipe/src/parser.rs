use crate::ast::*;
use crate::error::{Result, SnpipeError};

// ── Tokens ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Str(String),
    Int(i64),

    Pipe,      // |>
    Arrow,     // ->
    EqEq,      // ==
    BangEq,    // !=
    LtEq,      // <=
    GtEq,      // >=
    AmpAmp,    // &&
    PipePipe,  // ||
    EqTilde,   // =~
    BangTilde, // !~
    QQ,        // ??

    Dot,
    Lt,
    Gt,
    Eq,
    Bang,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    LParen,
    RParen,
    Colon,
    Comma,

    Eof,
}

// ── Lexer ─────────────────────────────────────────────────────────────────────

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0, line: 1 }
    }

    fn peek_ch(&self) -> Option<char> {
        self.src.get(self.pos..)?.chars().next()
    }

    fn peek_ch2(&self) -> Option<char> {
        let s = self.src.get(self.pos..)?;
        let mut it = s.chars();
        it.next()?;
        it.next()
    }

    fn next_ch(&mut self) -> Option<char> {
        let c = self.src.get(self.pos..)?.chars().next()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn skip_ws(&mut self) {
        loop {
            match self.peek_ch() {
                Some(' ') | Some('\t') | Some('\r') | Some('\n') => {
                    self.next_ch();
                }
                Some('-') if self.peek_ch2() == Some('-') => {
                    while !matches!(self.peek_ch(), None | Some('\n')) {
                        self.next_ch();
                    }
                }
                _ => break,
            }
        }
    }

    fn read_string(&mut self) -> Result<String> {
        let mut s = String::new();
        loop {
            match self.next_ch() {
                Some('"') => return Ok(s),
                Some('\\') => match self.next_ch() {
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('$') => { s.push('\\'); s.push('$'); }
                    Some(c) => { s.push('\\'); s.push(c); }
                    None => return Err(SnpipeError::parse(self.line, "unterminated escape")),
                },
                // preserve ${...} interpolation raw
                Some(c) => s.push(c),
                None => return Err(SnpipeError::parse(self.line, "unterminated string")),
            }
        }
    }

    fn read_ident(&mut self, first: char) -> String {
        let mut s = String::new();
        s.push(first);
        while let Some(c) = self.peek_ch() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.next_ch();
            } else {
                break;
            }
        }
        s
    }

    fn read_int(&mut self, first: char) -> i64 {
        let mut s = String::new();
        s.push(first);
        while let Some(c) = self.peek_ch() {
            if c.is_ascii_digit() {
                s.push(c);
                self.next_ch();
            } else {
                break;
            }
        }
        s.parse().unwrap_or(0)
    }

    fn next_token(&mut self) -> Result<(Token, usize)> {
        self.skip_ws();
        let line = self.line;
        let tok = match self.next_ch() {
            None => Token::Eof,
            Some('"') => Token::Str(self.read_string()?),
            Some('[') => Token::LBracket,
            Some(']') => Token::RBracket,
            Some('{') => Token::LBrace,
            Some('}') => Token::RBrace,
            Some('(') => Token::LParen,
            Some(')') => Token::RParen,
            Some(':') => Token::Colon,
            Some(',') => Token::Comma,
            Some('.') => Token::Dot,
            Some('|') => match self.peek_ch() {
                Some('>') => { self.next_ch(); Token::Pipe }
                Some('|') => { self.next_ch(); Token::PipePipe }
                _ => return Err(SnpipeError::parse(line, "expected '|>' or '||'")),
            },
            Some('-') => match self.peek_ch() {
                Some('>') => { self.next_ch(); Token::Arrow }
                _ => return Err(SnpipeError::parse(line, "expected '->', not bare '-'")),
            },
            Some('=') => match self.peek_ch() {
                Some('=') => { self.next_ch(); Token::EqEq }
                Some('~') => { self.next_ch(); Token::EqTilde }
                _ => Token::Eq,
            },
            Some('!') => match self.peek_ch() {
                Some('=') => { self.next_ch(); Token::BangEq }
                Some('~') => { self.next_ch(); Token::BangTilde }
                _ => Token::Bang,
            },
            Some('<') => match self.peek_ch() {
                Some('=') => { self.next_ch(); Token::LtEq }
                _ => Token::Lt,
            },
            Some('>') => match self.peek_ch() {
                Some('=') => { self.next_ch(); Token::GtEq }
                _ => Token::Gt,
            },
            Some('&') => match self.peek_ch() {
                Some('&') => { self.next_ch(); Token::AmpAmp }
                _ => return Err(SnpipeError::parse(line, "expected '&&'")),
            },
            Some('?') => match self.peek_ch() {
                Some('?') => { self.next_ch(); Token::QQ }
                _ => return Err(SnpipeError::parse(line, "expected '??'")),
            },
            Some(c) if c.is_alphabetic() || c == '_' => Token::Ident(self.read_ident(c)),
            Some(c) if c.is_ascii_digit() => Token::Int(self.read_int(c)),
            Some(c) => return Err(SnpipeError::parse(line, format!("unexpected char '{c}'"))),
        };
        Ok((tok, line))
    }

    fn tokenize(mut self) -> Result<Vec<(Token, usize)>> {
        let mut out = Vec::new();
        loop {
            let tok = self.next_token()?;
            let done = tok.0 == Token::Eof;
            out.push(tok);
            if done { break; }
        }
        Ok(out)
    }
}

// ── Parser ────────────────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<(Token, usize)>) -> Self {
        Self { tokens, pos: 0 }
    }

    // ── Lookahead (all return owned/cloned to avoid borrow conflicts) ─────────

    fn peek(&self) -> Token {
        self.tokens.get(self.pos).map(|(t, _)| t.clone()).unwrap_or(Token::Eof)
    }

    fn peek_at(&self, n: usize) -> Token {
        self.tokens.get(self.pos + n).map(|(t, _)| t.clone()).unwrap_or(Token::Eof)
    }

    fn line(&self) -> usize {
        self.tokens.get(self.pos).map(|(_, l)| *l).unwrap_or(0)
    }

    fn is_ident(&self, kw: &str) -> bool {
        matches!(self.peek(), Token::Ident(s) if s == kw)
    }

    fn is_pipe(&self) -> bool {
        self.peek() == Token::Pipe
    }

    fn is_eof(&self) -> bool {
        self.peek() == Token::Eof
    }

    // ── Consume ───────────────────────────────────────────────────────────────

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).map(|(t, _)| t.clone()).unwrap_or(Token::Eof);
        if self.pos < self.tokens.len() { self.pos += 1; }
        tok
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            got => Err(SnpipeError::parse(self.line(), format!("expected identifier, got {got:?}"))),
        }
    }

    fn expect_str(&mut self) -> Result<String> {
        match self.advance() {
            Token::Str(s) => Ok(s),
            got => Err(SnpipeError::parse(self.line(), format!("expected string, got {got:?}"))),
        }
    }

    fn expect_int(&mut self) -> Result<i64> {
        match self.advance() {
            Token::Int(n) => Ok(n),
            got => Err(SnpipeError::parse(self.line(), format!("expected integer, got {got:?}"))),
        }
    }

    fn expect_tok(&mut self, expected: Token) -> Result<()> {
        let got = self.advance();
        if got == expected {
            Ok(())
        } else {
            Err(SnpipeError::parse(self.line(), format!("expected {expected:?}, got {got:?}")))
        }
    }

    fn expect_kw(&mut self, kw: &str) -> Result<()> {
        match self.advance() {
            Token::Ident(s) if s == kw => Ok(()),
            got => Err(SnpipeError::parse(self.line(), format!("expected '{kw}', got {got:?}"))),
        }
    }

    // ── Pipeline ──────────────────────────────────────────────────────────────

    fn parse_pipeline(&mut self) -> Result<Pipeline> {
        let mut lets = Vec::new();
        while self.is_ident("let") {
            lets.push(self.parse_let_decl()?);
        }

        let source = self.parse_source()?;

        let mut steps = Vec::new();
        let mut sink = None;
        while self.is_pipe() && !self.is_eof() {
            self.advance(); // |>
            if matches!(self.peek(), Token::Ident(s) if matches!(s.as_str(), "to_csv"|"to_json"|"to_table")) {
                sink = Some(self.parse_sink()?);
                break;
            }
            steps.push(self.parse_step()?);
        }

        Ok(Pipeline { name: None, lets, source, steps, sink: sink.unwrap_or(Sink::Table) })
    }

    // ── Let bindings ──────────────────────────────────────────────────────────

    fn parse_let_decl(&mut self) -> Result<LetDecl> {
        self.expect_kw("let")?;
        let name = self.expect_ident()?;
        self.expect_tok(Token::Eq)?;

        let source = match self.peek() {
            Token::Ident(s) if s == "csv" => {
                self.advance();
                let path = self.expect_str()?;
                self.expect_tok(Token::LBracket)?;
                let mut col = 0usize;
                let mut skip = 0usize;
                while !matches!(self.peek(), Token::RBracket | Token::Eof) {
                    let key = self.expect_ident()?;
                    self.expect_tok(Token::Eq)?;
                    let val = self.expect_int()? as usize;
                    match key.as_str() {
                        "col"  => col  = val,
                        "skip" => skip = val,
                        k => return Err(SnpipeError::parse(self.line(), format!("unknown csv option '{k}'"))),
                    }
                    if self.peek() == Token::Comma { self.advance(); }
                }
                self.expect_tok(Token::RBracket)?;
                InputSource::Csv { path, col, skip }
            }
            _ => return Err(SnpipeError::parse(self.line(), "expected 'csv' in let binding")),
        };

        let mut transforms = Vec::new();
        loop {
            if !self.is_pipe() { break; }
            // only consume |> if followed by an input transform keyword
            let is_xform = matches!(
                self.peek_at(1),
                Token::Ident(s) if matches!(s.as_str(), "trim"|"dedup"|"warn_empty")
            );
            if !is_xform { break; }
            self.advance(); // |>
            transforms.push(match self.expect_ident()?.as_str() {
                "trim"       => InputTransform::Trim,
                "dedup"      => InputTransform::Dedup,
                "warn_empty" => InputTransform::WarnEmpty,
                s => return Err(SnpipeError::parse(self.line(), format!("unknown transform '{s}'"))),
            });
        }

        Ok(LetDecl { name, source, transforms })
    }

    // ── Source ────────────────────────────────────────────────────────────────

    fn parse_source(&mut self) -> Result<Source> {
        self.expect_kw("from")?;
        let table = self.expect_ident()?;
        let mut src = Source { table, ..Source::default() };

        loop {
            match self.peek() {
                Token::Ident(s) if s == "where" => {
                    self.advance();
                    src.query = Some(self.expect_str()?);
                }
                Token::Ident(s) if s == "pick" => {
                    self.advance();
                    src.fields = self.parse_ident_list()?;
                }
                Token::Ident(s) if s == "chunk" => {
                    self.advance();
                    self.expect_tok(Token::Colon)?;
                    src.chunk_size = self.expect_int()? as usize;
                }
                Token::Ident(s) if s == "paginate" => {
                    self.advance();
                    src.paginate = true;
                }
                Token::Ident(s) if s == "escape_values" => {
                    self.advance();
                    src.escape_values = true;
                }
                _ => break,
            }
        }
        Ok(src)
    }

    // ── Steps ─────────────────────────────────────────────────────────────────

    fn parse_step(&mut self) -> Result<Step> {
        match self.peek() {
            Token::Ident(s) => match s.as_str() {
                "coverage"     => Ok(Step::Coverage(self.parse_coverage()?)),
                "resolve_list" => Ok(Step::ResolveList(self.parse_resolve_list()?)),
                "resolve"      => Ok(Step::Resolve(self.parse_resolve()?)),
                "flat_map"     => Ok(Step::FlatMap(self.parse_flat_map()?)),
                "map"          => Ok(Step::Map(self.parse_map()?)),
                "filter"       => Ok(Step::Filter(self.parse_filter()?)),
                "dedup" => {
                    self.advance();
                    let on_field = if self.is_ident("on") {
                        self.advance();
                        Some(self.expect_ident()?)
                    } else { None };
                    Ok(Step::Dedup { on_field })
                }
                "warn_empty" => {
                    self.advance();
                    let message = if matches!(self.peek(), Token::Str(_)) {
                        Some(self.expect_str()?)
                    } else { None };
                    Ok(Step::WarnEmpty { message })
                }
                s => Err(SnpipeError::parse(self.line(), format!("unknown step '{s}'"))),
            },
            got => Err(SnpipeError::parse(self.line(), format!("expected step keyword, got {got:?}"))),
        }
    }

    fn parse_coverage(&mut self) -> Result<CoverageStep> {
        self.expect_kw("coverage")?;
        let source_name = self.expect_ident()?;
        self.expect_kw("on")?;
        let on_field = self.expect_ident()?;

        let mut step = CoverageStep {
            source_name, on_field,
            match_trim: false,
            match_case_insensitive: false,
            on_missing: OnMissing::Warn,
            on_duplicate: OnDuplicate::Warn,
        };

        loop {
            match self.peek() {
                Token::Pipe | Token::RBrace | Token::Eof => break,
                Token::Ident(s) if s == "match" => {
                    self.advance();
                    self.expect_tok(Token::Colon)?;
                    loop {
                        match self.peek() {
                            Token::Ident(s) if s == "trim" => { self.advance(); step.match_trim = true; }
                            Token::Ident(s) if s == "case_insensitive" => { self.advance(); step.match_case_insensitive = true; }
                            _ => break,
                        }
                        if self.peek() == Token::Comma { self.advance(); } else { break; }
                    }
                }
                Token::Ident(s) if s == "missing" => {
                    self.advance(); self.expect_tok(Token::Colon)?;
                    step.on_missing = self.parse_on_missing()?;
                }
                Token::Ident(s) if s == "on_duplicate" => {
                    self.advance(); self.expect_tok(Token::Colon)?;
                    step.on_duplicate = self.parse_on_duplicate()?;
                }
                _ => break,
            }
        }
        Ok(step)
    }

    fn parse_resolve(&mut self) -> Result<ResolveStep> {
        self.expect_kw("resolve")?;
        self.expect_tok(Token::Dot)?;
        let field = self.expect_ident()?;
        self.expect_tok(Token::Arrow)?;
        let table = self.expect_ident()?;
        let fields = self.parse_ident_list()?;

        let mut step = ResolveStep {
            field, table, fields,
            skip_null_id: false,
            on_missing: OnMissing::Warn,
            on_error: OnError::KeepRow,
        };
        loop {
            match self.peek() {
                Token::Pipe | Token::RBrace | Token::Eof => break,
                Token::Ident(s) if s == "skip_null_id" => { self.advance(); step.skip_null_id = true; }
                Token::Ident(s) if s == "on_missing" => {
                    self.advance(); self.expect_tok(Token::Colon)?;
                    step.on_missing = self.parse_on_missing()?;
                }
                Token::Ident(s) if s == "on_error" => {
                    self.advance(); self.expect_tok(Token::Colon)?;
                    step.on_error = self.parse_on_error()?;
                }
                _ => break,
            }
        }
        Ok(step)
    }

    fn parse_resolve_list(&mut self) -> Result<ResolveListStep> {
        self.expect_kw("resolve_list")?;
        self.expect_tok(Token::Dot)?;
        let field = self.expect_ident()?;
        self.expect_tok(Token::Arrow)?;
        let table = self.expect_ident()?;
        let fields = self.parse_ident_list()?;

        let mut step = ResolveListStep { field, table, fields, ..ResolveListStep::default() };
        loop {
            match self.peek() {
                Token::Pipe | Token::RBrace | Token::Eof => break,
                Token::Ident(s) if s == "skip_null_id" => { self.advance(); step.skip_null_id = true; }
                Token::Ident(s) if s == "skip_empty"   => { self.advance(); step.skip_empty = true; }
                Token::Ident(s) if s == "separator" => {
                    self.advance(); self.expect_tok(Token::Colon)?;
                    let s = self.expect_str()?;
                    step.separator = s.chars().next().unwrap_or(',');
                }
                Token::Ident(s) if s == "on_missing" => {
                    self.advance(); self.expect_tok(Token::Colon)?;
                    step.on_missing = self.parse_on_missing()?;
                }
                Token::Ident(s) if s == "on_error" => {
                    self.advance(); self.expect_tok(Token::Colon)?;
                    step.on_error = self.parse_on_error()?;
                }
                _ => break,
            }
        }
        Ok(step)
    }

    fn parse_flat_map(&mut self) -> Result<FlatMapStep> {
        self.expect_kw("flat_map")?;
        let var = self.expect_ident()?;
        self.expect_tok(Token::LBrace)?;
        let pipeline = self.parse_pipeline()?;
        self.expect_tok(Token::RBrace)?;
        Ok(FlatMapStep { var, pipeline: Box::new(pipeline) })
    }

    fn parse_map(&mut self) -> Result<MapStep> {
        self.expect_kw("map")?;
        let var = self.expect_ident()?;
        self.expect_tok(Token::LBrace)?;
        let mut fields = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let key = self.expect_ident()?;
            self.expect_tok(Token::Colon)?;
            let expr = self.parse_expr()?;
            fields.push((key, expr));
        }
        self.expect_tok(Token::RBrace)?;
        Ok(MapStep { var, fields })
    }

    fn parse_filter(&mut self) -> Result<FilterStep> {
        self.expect_kw("filter")?;
        let var = self.expect_ident()?;
        self.expect_tok(Token::Colon)?;
        let expr = self.parse_expr()?;
        Ok(FilterStep { var, expr })
    }

    // ── Sink ──────────────────────────────────────────────────────────────────

    fn parse_sink(&mut self) -> Result<Sink> {
        match self.expect_ident()?.as_str() {
            "to_csv" => {
                let path = if matches!(self.peek(), Token::Str(_)) { Some(self.expect_str()?) } else { None };
                Ok(Sink::Csv(path))
            }
            "to_json" => {
                let path = if matches!(self.peek(), Token::Str(_)) { Some(self.expect_str()?) } else { None };
                Ok(Sink::Json(path))
            }
            "to_table" => Ok(Sink::Table),
            s => Err(SnpipeError::parse(self.line(), format!("unknown sink '{s}'"))),
        }
    }

    // ── Option helpers ────────────────────────────────────────────────────────

    fn parse_on_missing(&mut self) -> Result<OnMissing> {
        match self.expect_ident()?.as_str() {
            "warn"  => Ok(OnMissing::Warn),
            "error" => Ok(OnMissing::Error),
            "skip"  => Ok(OnMissing::Skip),
            s => Err(SnpipeError::parse(self.line(), format!("expected warn/error/skip, got '{s}'"))),
        }
    }

    fn parse_on_duplicate(&mut self) -> Result<OnDuplicate> {
        match self.expect_ident()?.as_str() {
            "warn"  => Ok(OnDuplicate::Warn),
            "error" => Ok(OnDuplicate::Error),
            "skip"  => Ok(OnDuplicate::Skip),
            s => Err(SnpipeError::parse(self.line(), format!("expected warn/error/skip, got '{s}'"))),
        }
    }

    fn parse_on_error(&mut self) -> Result<OnError> {
        match self.expect_ident()?.as_str() {
            "keep_row" => Ok(OnError::KeepRow),
            "drop_row" => Ok(OnError::DropRow),
            "abort"    => Ok(OnError::Abort),
            s => Err(SnpipeError::parse(self.line(), format!("expected keep_row/drop_row/abort, got '{s}'"))),
        }
    }

    fn parse_ident_list(&mut self) -> Result<Vec<String>> {
        self.expect_tok(Token::LBracket)?;
        let mut out = Vec::new();
        while !matches!(self.peek(), Token::RBracket | Token::Eof) {
            out.push(self.expect_ident()?);
            if self.peek() == Token::Comma { self.advance(); }
        }
        self.expect_tok(Token::RBracket)?;
        Ok(out)
    }

    // ── Expressions ───────────────────────────────────────────────────────────
    //
    // Precedence (low → high):
    //   or → and → not → cmp → coalesce → postfix → atom

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.peek() == Token::PipePipe {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinOp { op: BinOp::Or, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_not()?;
        while self.peek() == Token::AmpAmp {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::BinOp { op: BinOp::And, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.peek() == Token::Bang {
            self.advance();
            return Ok(Expr::Not(Box::new(self.parse_not()?)));
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<Expr> {
        let left = self.parse_coalesce()?;
        let op = match self.peek() {
            Token::EqEq      => BinOp::Eq,
            Token::BangEq    => BinOp::Ne,
            Token::Lt        => BinOp::Lt,
            Token::Gt        => BinOp::Gt,
            Token::LtEq      => BinOp::Le,
            Token::GtEq      => BinOp::Ge,
            Token::EqTilde   => BinOp::RegexMatch,
            Token::BangTilde => BinOp::RegexNotMatch,
            Token::Ident(s) if s == "contains"    => BinOp::Contains,
            Token::Ident(s) if s == "starts_with" => BinOp::StartsWith,
            Token::Ident(s) if s == "ends_with"   => BinOp::EndsWith,
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_coalesce()?;
        Ok(Expr::BinOp { op, left: Box::new(left), right: Box::new(right) })
    }

    fn parse_coalesce(&mut self) -> Result<Expr> {
        let left = self.parse_postfix()?;
        if self.peek() == Token::QQ {
            self.advance();
            let right = self.parse_postfix()?;
            return Ok(Expr::Coalesce(Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_atom()?;
        loop {
            match self.peek() {
                Token::Dot => {
                    match self.peek_at(1) {
                        Token::Ident(m) if m == "filter" => {
                            self.advance(); self.advance(); // . filter
                            self.expect_tok(Token::LParen)?;
                            let var = self.expect_ident()?;
                            self.expect_tok(Token::Colon)?;
                            let cond = self.parse_expr()?;
                            self.expect_tok(Token::RParen)?;
                            expr = Expr::ListFilter { list: Box::new(expr), var, cond: Box::new(cond) };
                        }
                        Token::Ident(m) if m == "map" => {
                            self.advance(); self.advance(); // . map
                            self.expect_tok(Token::LParen)?;
                            let var = self.expect_ident()?;
                            self.expect_tok(Token::Colon)?;
                            let body = self.parse_expr()?;
                            self.expect_tok(Token::RParen)?;
                            expr = Expr::ListMap { list: Box::new(expr), var, body: Box::new(body) };
                        }
                        Token::Ident(m) if m == "dedup" => {
                            self.advance(); self.advance(); // . dedup
                            self.expect_tok(Token::LParen)?;
                            self.expect_tok(Token::RParen)?;
                            expr = Expr::ListDedup(Box::new(expr));
                        }
                        Token::Ident(_) => {
                            self.advance(); // .
                            let field = self.expect_ident()?;
                            expr = extend_field(expr, Segment::Field(field));
                        }
                        _ => break,
                    }
                }
                // [].field — flatten then extract
                Token::LBracket
                    if self.peek_at(1) == Token::RBracket && self.peek_at(2) == Token::Dot =>
                {
                    self.advance(); // [
                    self.advance(); // ]
                    self.advance(); // .
                    let field = self.expect_ident()?;
                    expr = extend_field(expr, Segment::Flatten);
                    expr = extend_field(expr, Segment::Field(field));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_atom(&mut self) -> Result<Expr> {
        match self.peek() {
            Token::Str(s)  => { self.advance(); Ok(Expr::Str(s)) }
            Token::Int(n)  => { self.advance(); Ok(Expr::Int(n)) }
            Token::Ident(s) if s == "true"  => { self.advance(); Ok(Expr::Bool(true)) }
            Token::Ident(s) if s == "false" => { self.advance(); Ok(Expr::Bool(false)) }
            Token::Ident(s) if s == "null"  => { self.advance(); Ok(Expr::Null) }
            // [] — empty list literal
            Token::LBracket if self.peek_at(1) == Token::RBracket
                            && self.peek_at(2) != Token::Dot =>
            {
                self.advance(); self.advance();
                Ok(Expr::EmptyList)
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect_tok(Token::RParen)?;
                Ok(expr)
            }
            Token::Ident(s) => { self.advance(); Ok(Expr::Field(vec![Segment::Field(s)])) }
            got => Err(SnpipeError::parse(self.line(), format!("expected expression, got {got:?}"))),
        }
    }
}

// When extending a field path: if expr is already Expr::Field, push the segment.
// Otherwise wrap in a ListMap (unusual but handles e.g. postfix on a string literal).
fn extend_field(expr: Expr, seg: Segment) -> Expr {
    match expr {
        Expr::Field(mut segs) => { segs.push(seg); Expr::Field(segs) }
        other => Expr::ListMap {
            list: Box::new(other),
            var: String::from("__v"),
            body: Box::new(Expr::Field(vec![Segment::Field("__v".into()), seg])),
        },
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn parse(src: &str) -> Result<Pipeline> {
    let tokens = Lexer::new(src).tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse_pipeline()
}
