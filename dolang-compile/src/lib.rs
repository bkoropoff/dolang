#![deny(warnings)]

pub(crate) mod ast;
pub(crate) mod cfg;
pub(crate) mod constant;
pub mod diag;
pub(crate) mod elab;
pub(crate) mod emit;
pub(crate) mod flow;
pub(crate) mod lex;
pub(crate) mod lower;
pub(crate) mod origin;
pub(crate) mod parse;
pub(crate) mod sig;
pub mod source;
pub(crate) mod sym;

use std::{
    convert::Infallible,
    error,
    fmt::{self, Display},
    io::{self, Write},
    mem,
    ops::ControlFlow,
    path::Path,
};

use crate::{ast::visit, lex::Comment};

use self::{
    ast::visit::{Node, NodeKind},
    elab::Elaborater,
    emit::Emitter,
    lex::Lexer,
    lower::Lowerer,
    parse::Parser,
    source::{Diags, File},
};

pub use ast::{Context, visit::Token};

#[cfg(feature = "debug")]
use dolang_util::debug_eprintln;
use dolang_util::intern::{self, BinTable};

use ast::Res;

const STD_PRELUDE: &[&str] = &[
    "Array", "Bin", "BinBuf", "Bool", "Dict", "Float", "Func", "Int", "Range", "Record", "Set",
    "Str", "StrBuf", "Sym", "Tuple", "Type", "array", "bool", "class", "dbg", "dict", "float",
    "getter", "int", "record", "setter", "static", "str", "sym", "tuple", "type",
];

#[derive(Debug)]
enum ErrorInfo {
    Fail,
    Io(io::Error),
}

/// Kind of compilation error.
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Compilation failed; consult diagnostics.
    Fail,
    /// I/O error emitting bytecode.
    Io,
}

/// Compile error
#[derive(Debug)]
pub struct Error(ErrorInfo);

impl Error {
    /// Get kind of error
    pub fn kind(&self) -> ErrorKind {
        match &self.0 {
            ErrorInfo::Fail => ErrorKind::Fail,
            ErrorInfo::Io(_) => ErrorKind::Io,
        }
    }

    /// Get underlying [`io::Error`], if applicable
    pub fn as_io(&self) -> Option<&io::Error> {
        match &self.0 {
            ErrorInfo::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error(ErrorInfo::Io(value))
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            ErrorInfo::Fail => "compilation failed".fmt(f),
            ErrorInfo::Io(e) => e.fmt(f),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self.0 {
            ErrorInfo::Fail => None,
            ErrorInfo::Io(error) => Some(error),
        }
    }
}

impl From<lower::Error> for Error {
    fn from(_: lower::Error) -> Self {
        Self(ErrorInfo::Fail)
    }
}

/// Origin of a resolved identifier
pub use diag::Origin;

/// Token emitter
pub trait EmitToken {
    /// Emit token
    fn emit(
        &mut self,
        token: Token,
        span: diag::Span,
        origin: Option<diag::Origin>,
        context: Context,
    );
}

/// Callback function as token emitter
impl<F> EmitToken for F
where
    F: FnMut(Token, diag::Span, Option<diag::Origin>, Context),
{
    fn emit(
        &mut self,
        token: Token,
        span: diag::Span,
        origin: Option<diag::Origin>,
        context: Context,
    ) {
        self(token, span, origin, context)
    }
}

struct VisitAdapter<'a, 'e> {
    file: &'a File<'a>,
    origintab: &'a origin::Table,
    emit: &'e mut dyn EmitToken,
}

struct CallAdapter<'a, 'b, 'e> {
    parent: &'b mut VisitAdapter<'a, 'e>,
    seen_arg0: bool,
}

struct CallIdentAdapter<'a, 'b, 'e>(&'b mut VisitAdapter<'a, 'e>);

impl visit::Visit for CallIdentAdapter<'_, '_, '_> {
    type Break = Infallible;

    fn node<T: Node + ?Sized>(&mut self, node: &T) -> ControlFlow<Self::Break> {
        self.0.node(node)
    }

    fn token(
        &mut self,
        _token: Token,
        span: source::Span,
        origin: Option<origin::Id>,
    ) -> ControlFlow<Self::Break> {
        self.0
            .emit_token(Token::Variable, span, origin, Context::Call)
    }
}

struct MethodAdapter<'a, 'b, 'e>(&'b mut VisitAdapter<'a, 'e>);

impl visit::Visit for MethodAdapter<'_, '_, '_> {
    type Break = Infallible;

    fn node<T: Node + ?Sized>(&mut self, node: &T) -> ControlFlow<Self::Break> {
        self.0.node(node)
    }

    fn token(
        &mut self,
        token: Token,
        span: source::Span,
        origin: Option<origin::Id>,
    ) -> ControlFlow<Self::Break> {
        let context = match token {
            Token::Field => Context::Call,
            _ => Context::None,
        };
        self.0.emit_token(token, span, origin, context)
    }
}

impl visit::Visit for CallAdapter<'_, '_, '_> {
    type Break = Infallible;

    fn node<T: Node + ?Sized>(&mut self, node: &T) -> ControlFlow<Self::Break> {
        if self.seen_arg0 {
            node.accept(self.parent)
        } else {
            self.seen_arg0 = true;
            match node.kind() {
                NodeKind::Index | NodeKind::Call => node.accept(&mut CallAdapter {
                    parent: self.parent,
                    seen_arg0: false,
                }),
                NodeKind::Ident => node.accept(&mut CallIdentAdapter(self.parent)),
                NodeKind::Field => node.accept(&mut MethodAdapter(self.parent)),
                _ => node.accept(self.parent),
            }
        }
    }

    fn token(
        &mut self,
        token: Token,
        span: source::Span,
        origin: Option<origin::Id>,
    ) -> ControlFlow<Self::Break> {
        self.parent.emit_token(token, span, origin, Context::None)
    }
}

fn convert_span(file: &File, span: source::Span) -> diag::Span {
    let coords = file.coord_span(span);
    diag::Span::new(
        diag::Pos::new(span.start as usize, coords.start.line, coords.start.column),
        diag::Pos::new(span.end as usize, coords.end.line, coords.end.column),
    )
}

fn convert_origin(file: &File, internal: &origin::Origin) -> Option<diag::Origin> {
    match internal {
        origin::Origin::ImportItem { module, item, name } => Some(diag::Origin::ImportItem {
            module: convert_span(file, *module),
            item: convert_span(file, *item),
            name: convert_span(file, *name),
        }),
        origin::Origin::ImportModule { module, name } => Some(diag::Origin::ImportModule {
            module: convert_span(file, *module),
            name: convert_span(file, *name),
        }),
        origin::Origin::PreludeModule { module, name } => Some(diag::Origin::PreludeModule {
            module: module.clone(),
            name: name.clone(),
        }),
        origin::Origin::PreludeItem { module, item, name } => Some(diag::Origin::PreludeItem {
            module: module.clone(),
            item: item.clone(),
            name: name.clone(),
        }),
        origin::Origin::Class { span } => Some(diag::Origin::Class {
            span: convert_span(file, *span),
        }),
        origin::Origin::Def { span } => Some(diag::Origin::Def {
            span: convert_span(file, *span),
        }),
        origin::Origin::Bind { span } => Some(diag::Origin::Bind {
            span: convert_span(file, *span),
        }),
        origin::Origin::Method { span, class } => Some(diag::Origin::Method {
            span: convert_span(file, *span),
            class: convert_span(file, *class),
        }),
        origin::Origin::Field { span, class } => Some(diag::Origin::Field {
            span: convert_span(file, *span),
            class: convert_span(file, *class),
        }),
        origin::Origin::Param { span } => Some(diag::Origin::Param {
            span: convert_span(file, *span),
        }),
        origin::Origin::SelfParam { span } => Some(diag::Origin::SelfParam {
            span: convert_span(file, *span),
        }),
        origin::Origin::Synthetic | origin::Origin::Repl => None,
    }
}

impl VisitAdapter<'_, '_> {
    fn emit_token(
        &mut self,
        token: Token,
        span: source::Span,
        origin: Option<origin::Id>,
        context: Context,
    ) -> ControlFlow<Infallible> {
        let diag_span = convert_span(self.file, span);
        let diag_origin = origin.and_then(|id| convert_origin(self.file, &self.origintab[id]));
        self.emit.emit(token, diag_span, diag_origin, context);
        ControlFlow::Continue(())
    }
}

impl visit::Visit for VisitAdapter<'_, '_> {
    type Break = Infallible;

    fn node<T: Node + ?Sized>(&mut self, node: &T) -> ControlFlow<Self::Break> {
        if matches!(node.kind(), NodeKind::Call) {
            node.accept(&mut CallAdapter {
                parent: self,
                seen_arg0: false,
            })
        } else {
            node.accept(self)
        }
    }

    fn token(
        &mut self,
        token: Token,
        span: source::Span,
        origin: Option<origin::Id>,
    ) -> ControlFlow<Self::Break> {
        self.emit_token(token, span, origin, Context::None)
    }
}

/// Compilation mode
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub enum Mode<'a> {
    /// Compile as script: return value is value of final statement or early return
    Script,
    /// Compile as module: return value is a module of top-level bindings, or that of early return
    Module { name: &'a str },
    /// Compile in REPL mode:
    /// - Return value is a module of top-level bindings, including private bindings (e.g. imports)
    /// - Early return is disallowed at top level
    /// - Value of final statement is bound to `_`
    Repl,
}

#[derive(Debug, Clone)]
pub(crate) struct PreludeItem {
    item: String,
    bind: String,
    res: Option<Res>,
}

#[derive(Debug, Clone)]
pub(crate) enum PreludeImport {
    Items {
        module: String,
        items: Vec<PreludeItem>,
    },
    ModuleAsIs {
        module: String,
        bind: String,
        res: Option<Res>,
        insert: bool,
    },
    ModuleRenamed {
        module: String,
        bind: String,
        res: Option<Res>,
    },
}

/// Prelude configurer
pub struct Prelude<'a, 'b> {
    config: &'b mut Config<'a>,
}

impl<'a, 'b> Prelude<'a, 'b> {
    fn module_name_first(module: &str) -> &str {
        if let Some((first, _)) = module.split_once(".") {
            first
        } else {
            module
        }
    }

    /// Clear the prelude, including any default imports
    pub fn clear(self) -> Self {
        self.config.prelude.clear();
        self
    }

    /// Imports the named module, as by:
    /// ```do
    /// import module
    /// ```
    pub fn import_module(self, module: impl Into<String>) -> Self {
        let module = module.into();
        let bind = Self::module_name_first(&module).to_owned();

        self.config.prelude.push(PreludeImport::ModuleAsIs {
            module,
            bind,
            res: None,
            insert: false,
        });
        self
    }

    /// Imports the named module under a different name, as by:
    /// ```do
    /// import module: name
    /// ```
    pub fn import_module_with_name(
        self,
        module: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        self.config.prelude.push(PreludeImport::ModuleRenamed {
            module: module.into(),
            bind: name.into(),
            res: None,
        });
        self
    }

    /// Import items from a module.  Returns a builder object to configure individual items.
    pub fn import_items(self, module: impl Into<String>) -> Items<'a, 'b> {
        self.config.prelude.push(PreludeImport::Items {
            module: module.into(),
            items: Vec::new(),
        });
        Items {
            config: self.config,
        }
    }
}

/// Item import builder.
#[must_use]
pub struct Items<'a, 'b> {
    config: &'b mut Config<'a>,
}

impl<'a, 'b> Items<'a, 'b> {
    /// Imports the given item, as by:
    /// ```do
    /// import module:
    ///   - item
    /// ```
    pub fn item(self, item: impl Into<String>) -> Self {
        let item = item.into();
        match self.config.prelude.last_mut().unwrap() {
            PreludeImport::Items { items, .. } => items.push(PreludeItem {
                item: item.clone(),
                bind: item,
                res: None,
            }),
            _ => unreachable!(),
        };
        self
    }

    /// Imports the given items, as by:
    /// ```do
    /// import module:
    ///   - item
    ///   - ...
    /// ```
    pub fn items(mut self, items: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for item in items.into_iter() {
            self = self.item(item);
        }
        self
    }

    /// Imports the given item under a different name, as by:
    /// ```do
    /// import module:
    ///   item: name
    /// ```
    pub fn item_with_name(self, item: impl Into<String>, name: impl Into<String>) -> Self {
        match self.config.prelude.last_mut().unwrap() {
            PreludeImport::Items { items, .. } => items.push(PreludeItem {
                item: item.into(),
                bind: name.into(),
                res: None,
            }),
            _ => unreachable!(),
        };
        self
    }

    /// Finish item imports.
    ///
    /// Calling this method may be necessary to ensure changes take effect.
    pub fn commit(self) -> Prelude<'a, 'b> {
        Prelude {
            config: self.config,
        }
    }
}

/// Compiler configuration.
///
/// A configuration carries no source, so it may be reused to build any number of
/// [`Unit`]s.
pub struct Config<'a> {
    mode: Mode<'a>,
    prelude: Vec<PreludeImport>,
    recover: bool,
}

impl Default for Config<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Config<'a> {
    /// Create a new configuration with the default prelude
    pub fn new() -> Self {
        let mut this = Self {
            mode: Mode::Script,
            prelude: Default::default(),
            recover: false,
        };
        this.prelude()
            .import_module("std")
            .import_module("strand")
            .import_items("std")
            .items(STD_PRELUDE.iter().copied())
            .commit()
            .import_items("strand")
            .items(["fork", "pipeline", "stream", "put", "spawn"])
            .commit();
        this
    }

    /// Change compilation mode
    ///
    /// Default: [`Mode::Script`]
    pub fn mode(&mut self, mode: Mode<'a>) -> &mut Self {
        self.mode = mode;
        self
    }

    /// Recover from errors to the greatest extent possible, retaining a partial tree so
    /// that diagnostics and tokens remain available for malformed source.
    ///
    /// A [`Unit`] which recovered from an error cannot be emitted; [`Unit::emit`] will
    /// fail with [`ErrorKind::Fail`].
    ///
    /// Default: `false`
    pub fn recover(&mut self, recover: bool) -> &mut Self {
        self.recover = recover;
        self
    }

    /// Configure a prelude, a collection of standard imports which are injected into the code.
    ///
    /// Note that prelude imports which are not referenced by the code are omitted from compilation, even
    /// if importing them would have side effects.
    pub fn prelude(&mut self) -> Prelude<'a, '_> {
        Prelude { config: self }
    }

    /// Parse and elaborate source into a [`Unit`].
    ///
    /// This operation is infallible: errors are recorded in the unit and surfaced through
    /// [`Unit::diagnostics`] and [`Unit::emit`].
    ///
    /// # Arguments
    /// - `path`: The path of the source file; used in backtraces
    /// - `content`: The source as a byte slice
    pub fn unit<'b>(&self, path: &'b Path, content: &'b [u8]) -> Unit<'b>
    where
        'a: 'b,
    {
        let mut compiler = Compiler {
            file: File::new(path, content),
            origintab: Default::default(),
            symtab: sym::Table::new(),
            bintab: BinTable::new(),
            consttab: constant::Table::new(),
            packtab: sig::PackTable::new(),
            unpacktab: sig::UnpackTable::new(),
            mode: self.mode.clone(),
            prelude: self.prelude.clone(),
        };
        let mut prelude = mem::take(&mut compiler.prelude);
        let diags = Diags::new();
        let mut comments = vec![];

        let (mut ast, mut failed) = {
            let mut collect = |span| comments.push(span);
            let mut parser = compiler.parser(&diags, Some(&mut collect as &mut dyn Comment));
            let ast = parser.parse(self.recover);
            let failed = parser.failed();
            (ast, failed)
        };
        #[cfg(feature = "debug")]
        if let Err(e) = compiler.export_ast_dot(&ast, false) {
            debug_eprintln!("AST DOT export failed: {e}")
        }

        {
            let mut elab = compiler.elaborater(&diags);
            elab.elaborate(&mut ast, &mut prelude);
            failed |= elab.failed();
        }
        #[cfg(feature = "debug")]
        if let Err(e) = compiler.export_ast_dot(&ast, true) {
            debug_eprintln!("Resolved AST DOT export failed: {e}")
        }

        compiler.prelude = prelude;
        Unit {
            compiler,
            ast,
            comments,
            diags,
            failed,
        }
    }
}

/// A parsed and elaborated compilation unit.
///
/// A unit retains the compiler state which produced it, so diagnostics are resolved
/// lazily as they are iterated.
pub struct Unit<'a> {
    compiler: Compiler<'a>,
    ast: ast::Root,
    comments: Vec<source::Span>,
    diags: Diags,
    failed: bool,
}

impl Unit<'_> {
    /// Iterate diagnostics generated while building the unit.
    ///
    /// Diagnostics are yielded in the order they were generated.
    pub fn diagnostics(&self) -> impl Iterator<Item = diag::Diag> + '_ {
        self.diags.iter().map(|diag| diag.resolve(&self.compiler))
    }

    /// Emit semantic tokens for the unit.
    ///
    /// The precise order of emitted tokens is not specified, but will be approximately in
    /// textual order.
    ///
    /// # Arguments
    /// - `tokens`: Where to send semantic tokens.
    pub fn tokens(&self, tokens: &mut impl EmitToken) {
        let ControlFlow::Continue(()) = self.ast.accept(&mut VisitAdapter {
            file: &self.compiler.file,
            origintab: &self.compiler.origintab,
            emit: tokens,
        });
        for comment in self.comments.iter() {
            let content = self.compiler.file.str(*comment);
            let slice = content.trim_end();
            tokens.emit(
                Token::Comment,
                convert_span(
                    &self.compiler.file,
                    source::Span {
                        start: comment.start,
                        end: comment.start + slice.len() as u32,
                    },
                ),
                None,
                ast::Context::None,
            );
        }
    }

    /// Compile the unit, writing bytecode.
    ///
    /// # Arguments
    /// - `write`: Where to write bytecode.
    ///
    /// # Errors
    /// - [`ErrorKind::Fail`]: The unit contains at least one error; consult
    ///   [`Unit::diagnostics`].
    /// - [`ErrorKind::Io`]: Writing bytecode failed with an [`io::Error`].
    pub fn emit(mut self, write: &mut impl Write) -> Result<(), Error> {
        if self.failed {
            return Err(Error(ErrorInfo::Fail));
        }
        let mut lowerer = self.compiler.lowerer();
        let graph = lowerer.run(&self.ast)?;
        #[cfg(feature = "debug")]
        {
            // Export DOT file if environment variable is set
            if let Ok(output) = std::env::var("DOLANG_EXPORT_DOT")
                && let Err(e) = self.compiler.export_cfg_dot(&graph, output)
            {
                debug_eprintln!("DOT export failed: {e}");
            }
        }
        let mut emitter = self.compiler.emitter(&graph);
        Ok(emitter.emit(write)?)
    }
}

/// Compiler state backing a [`Unit`].
pub(crate) struct Compiler<'a> {
    file: File<'a>,
    origintab: origin::Table,
    symtab: sym::Table,
    bintab: BinTable,
    consttab: constant::Table,
    packtab: sig::PackTable,
    unpacktab: sig::UnpackTable,
    mode: Mode<'a>,
    prelude: Vec<PreludeImport>,
}

impl Compiler<'_> {
    fn parser<'b>(
        &'b mut self,
        diags: &'b Diags,
        comment: Option<&'b mut dyn Comment>,
    ) -> Parser<'b> {
        Parser::new(Lexer::new(&self.file, diags, comment), &self.file, diags)
    }

    fn elaborater<'b>(&'b mut self, diags: &'b Diags) -> Elaborater<'b> {
        Elaborater::new(
            self.mode.clone(),
            &self.file,
            &mut self.bintab,
            &mut self.symtab,
            &mut self.origintab,
            diags,
        )
    }

    fn lowerer(&mut self) -> Lowerer<'_> {
        Lowerer {
            mode: self.mode.clone(),
            file: &self.file,
            symtab: &mut self.symtab,
            bintab: &mut self.bintab,
            consttab: &mut self.consttab,
            packtab: &mut self.packtab,
            unpacktab: &mut self.unpacktab,
            origintab: &self.origintab,
            prelude: &self.prelude,
            sentinel_const: None,
        }
    }

    fn emitter<'b>(&'b self, graph: &'b cfg::Graph) -> Emitter<'b> {
        Emitter {
            file: &self.file,
            graph,
            bintab: &self.bintab,
            symtab: &self.symtab,
            consttab: &self.consttab,
            packtab: &self.packtab,
            unpacktab: &self.unpacktab,
            debugbintab: Default::default(),
            mode: self.mode.clone(),
        }
    }
}

#[cfg(feature = "debug")]
impl Compiler<'_> {
    /// Generate a graphviz DOT representation of an AST
    fn ast_to_dot<N: Node + ?Sized>(&self, ast: &N, writer: &mut impl Write) -> io::Result<()> {
        use crate::ast::{dot::DotVisitor, visit::Visit};
        use dot_writer::DotWriter;

        let mut writer = DotWriter::from(writer);
        writer.set_pretty_print(true);
        let mut digraph = writer.digraph();
        let mut visitor = DotVisitor::new(&mut digraph, self);
        match visitor.node(ast) {
            ControlFlow::Continue(()) => Ok(()),
            ControlFlow::Break(e) => Err(e),
        }
    }

    /// Export AST to a DOT file based on the DOLANG_EXPORT_DOT environment variable
    /// Similar to the CFG export functionality
    fn export_ast_dot<N: Node + ?Sized>(&self, ast: &N, res: bool) -> io::Result<()> {
        use std::{
            fs,
            path::{self, Component},
        };

        if let Ok(output) = std::env::var("DOLANG_EXPORT_DOT") {
            let src = path::absolute(self.file.path())?;
            let cwd = std::env::current_dir()?;
            let rel = src.strip_prefix(&cwd).unwrap_or(&src);
            let comps: Vec<_> = rel
                .components()
                .filter_map(|c| {
                    if let Component::Normal(c) = c {
                        Some(c.to_string_lossy())
                    } else {
                        None
                    }
                })
                .collect();
            let name = comps.join("_");
            fs::create_dir_all(&output)?;
            let out = std::path::Path::new(&output)
                .join(&name)
                .with_extension(if res { "res.dot" } else { "ast.dot" });
            let mut file = fs::File::create(&out)?;

            self.ast_to_dot(ast, &mut file)?;
            debug_eprintln!("AST DOT exported to: {}", out.display());
        }

        Ok(())
    }

    fn export_cfg_dot(&mut self, graph: &cfg::Graph, output: String) -> Result<(), io::Error> {
        use std::{
            fs,
            path::{self, Component},
        };
        let src = path::absolute(self.file.path())?;
        let cwd = std::env::current_dir()?;
        let rel = src.strip_prefix(cwd).unwrap_or(&src);
        let comps: Vec<_> = rel
            .components()
            .filter_map(|c| {
                if let Component::Normal(c) = c {
                    Some(c.to_string_lossy())
                } else {
                    None
                }
            })
            .collect();
        let name = comps.join("_");
        fs::create_dir_all(&output)?;
        let out = Path::new(&output).join(&name).with_extension("cfg.dot");
        let mut file = fs::File::create(&out)?;
        graph.dot(self, &mut file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config<'a>() -> Config<'a> {
        let mut config = Config::new();
        // The default prelude imports modules which are not available here
        config.prelude().clear();
        config
    }

    #[test]
    fn valid_source_emits_bytecode() {
        let unit = config().unit(Path::new("<test>"), b"let x = 1\nx\n");
        assert!(
            !unit
                .diagnostics()
                .any(|d| d.severity() == diag::Severity::Error)
        );
        let mut tokens = 0;
        unit.tokens(&mut |_, _, _, _| tokens += 1);
        assert!(tokens != 0);
        let mut out = Vec::new();
        unit.emit(&mut out).unwrap();
        assert!(!out.is_empty());
    }

    #[test]
    fn recovered_unit_reports_diagnostics_and_refuses_to_emit() {
        let source = b"let x = 1\nlet = 2\n";
        let unit = config().recover(true).unit(Path::new("<test>"), source);

        let diags: Vec<_> = unit.diagnostics().collect();
        assert!(diags.iter().any(|d| d.severity() == diag::Severity::Error));

        // Tokens remain available despite the error
        let mut tokens = 0;
        unit.tokens(&mut |_, _, _, _| tokens += 1);
        assert!(tokens != 0);

        let mut out = Vec::new();
        assert!(matches!(
            unit.emit(&mut out).unwrap_err().kind(),
            ErrorKind::Fail
        ));
    }

    #[test]
    fn unrecovered_unit_reports_diagnostics() {
        let source = b"let x = 1\nlet = 2\n";
        let unit = config().unit(Path::new("<test>"), source);

        assert!(
            unit.diagnostics()
                .any(|d| d.severity() == diag::Severity::Error)
        );
        let mut out = Vec::new();
        assert!(matches!(
            unit.emit(&mut out).unwrap_err().kind(),
            ErrorKind::Fail
        ));
    }
}
