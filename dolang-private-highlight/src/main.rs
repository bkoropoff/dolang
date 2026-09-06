use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use clap::Parser;
use dolang::compile::{Config, Context, Origin, Pos, Severity, Span, Token};

use serde_json::{Value, json};

#[derive(Parser)]
struct Cli {
    /// Source path
    path: Option<PathBuf>,
}

fn pos_repr(pos: &Pos) -> Value {
    json!({
        "offset": pos.byte_offset(),
        "line": pos.line_offset(),
        "col": pos.column_offset(),
    })
}

fn span_repr(span: &Span) -> Value {
    json!({
        "start": pos_repr(&span.start()),
        "end": pos_repr(&span.end()),
    })
}

fn token_kind(token: &Token) -> &'static str {
    match token {
        Token::Comment => "comment",
        Token::Constant => "constant",
        Token::Delim => "delim",
        Token::Escape => "escape",
        Token::Field => "field",
        Token::Method => "method",
        Token::Key => "key",
        Token::ModuleName => "module_name",
        Token::ModuleItem => "module_item",
        Token::Keyword => "keyword",
        Token::Literal => "literal",
        Token::Number => "number",
        Token::Operator => "operand",
        Token::StringDelim => "string_delim",
        Token::Variable => "variable",
        Token::Sigil => "sigil",
    }
}

fn severity_kind(severity: &Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        _ => "error",
    }
}

fn origin_kind(origin: &Origin) -> &'static str {
    match origin {
        Origin::ImportItem { .. } => "import_item",
        Origin::ImportModule { .. } => "import_module",
        Origin::PreludeModule { .. } => "prelude_module",
        Origin::PreludeItem { .. } => "prelude_item",
        Origin::Class { .. } => "class",
        Origin::Def { .. } => "def",
        Origin::Bind { .. } => "bind",
        Origin::Method { .. } => "method",
        Origin::Field { .. } => "field",
        Origin::Param { .. } => "param",
        Origin::SelfParam { .. } => "self_param",
    }
}

fn context_kind(context: &Context) -> Option<&'static str> {
    match context {
        Context::None => None,
        Context::Call => Some("call"),
    }
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let (path, content) = if let Some(path) = &cli.path {
        (path.as_ref(), fs::read(path)?)
    } else {
        let mut content = vec![];
        io::stdin().read_to_end(&mut content)?;
        (Path::new("<stdin>"), content)
    };
    let unit = Config::new().recover(true).unit(path, &content);
    let mut tokens = vec![];
    unit.tokens(&mut |token, span, origin, context| {
        let mut obj = json!({
            "kind": token_kind(&token),
            "span": span_repr(&span),
        });
        if let Some(origin) = origin {
            obj.as_object_mut()
                .unwrap()
                .insert("origin".into(), origin_kind(&origin).into());
        }
        if let Some(context) = context_kind(&context) {
            obj.as_object_mut()
                .unwrap()
                .insert("context".into(), context.into());
        }
        tokens.push(obj);
    });
    let diagnostics: Vec<_> = unit
        .diagnostics()
        .map(|diag| {
            json!({
                "kind": severity_kind(&diag.severity()),
                "span": span_repr(&diag.span()),
            })
        })
        .collect();
    let result: Vec<_> = tokens.into_iter().chain(diagnostics).collect();
    serde_json::to_writer_pretty(io::stdout(), &result)?;
    Ok(())
}
