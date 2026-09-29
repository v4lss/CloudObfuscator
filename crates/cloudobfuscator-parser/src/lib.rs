use anyhow::{anyhow, Result};
use std::path::Path;
use swc_common::comments::SingleThreadedComments;
use swc_common::sync::Lrc;
use swc_common::{FileName, Globals, Mark, SourceMap, GLOBALS};
use swc_ecma_ast::{EsVersion, Module, ModuleItem, Script};
use swc_ecma_codegen::text_writer::JsWriter;
use swc_ecma_codegen::{Config as CodegenConfig, Emitter};
use swc_ecma_parser::{lexer::Lexer, EsSyntax, Parser, StringInput, Syntax, TsSyntax};
use swc_ecma_transforms_base::resolver;
use swc_ecma_visit::VisitMutWith;

pub struct ParsedModule {
    pub module: Module,
    pub comments: SingleThreadedComments,
    pub cm: Lrc<SourceMap>,
    pub unresolved_mark: Mark,
    pub top_level_mark: Mark,
    pub filename: String,
    pub stats: SourceStats,
    pub is_esm: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SourceStats {
    pub bytes: usize,
    pub lines: usize,
    pub items: usize,
}

fn syntax_for(path: &str) -> Syntax {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("js")
        .to_ascii_lowercase();
    match ext.as_str() {
        "ts" => Syntax::Typescript(TsSyntax {
            tsx: false,
            ..Default::default()
        }),
        "mts" | "cts" => Syntax::Typescript(TsSyntax {
            tsx: false,
            no_early_errors: true,
            ..Default::default()
        }),
        "tsx" => Syntax::Typescript(TsSyntax {
            tsx: true,
            ..Default::default()
        }),
        "jsx" => Syntax::Es(EsSyntax {
            jsx: true,
            ..Default::default()
        }),
        _ => Syntax::Es(EsSyntax {
            jsx: false,
            allow_return_outside_function: true,
            ..Default::default()
        }),
    }
}

pub fn parse(source: &str, filename: &str) -> Result<ParsedModule> {
    GLOBALS.set(&Globals::new(), || parse_inner(source, filename))
}

pub fn parse_in_scope(source: &str, filename: &str) -> Result<ParsedModule> {
    parse_inner(source, filename)
}

fn parse_inner(source: &str, filename: &str) -> Result<ParsedModule> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom(filename.to_string())),
        source.to_string(),
    );
    let comments = SingleThreadedComments::default();
    let syntax = syntax_for(filename);

    let make_lexer = || {
        Lexer::new(
            syntax,
            EsVersion::EsNext,
            StringInput::from(&*fm),
            Some(&comments),
        )
    };

    let mut parser = Parser::new_from(make_lexer());
    let mut module = match parser.parse_module() {
        Ok(m) => m,
        Err(first) => {
            let mut fallback = Parser::new_from(make_lexer());
            match fallback.parse_script() {
                Ok(Script { span, body, shebang }) => Module {
                    span,
                    body: body.into_iter().map(ModuleItem::Stmt).collect(),
                    shebang,
                },
                Err(_) => {
                    let msg = first.kind().msg().to_string();
                    return Err(anyhow!("failed to parse {}: {}", filename, msg));
                }
            }
        }
    };

    for err in parser.take_errors() {
        let msg = err.into_kind().msg().to_string();
        if msg.is_empty() {
            continue;
        }
        return Err(anyhow!("{}: {}", filename, msg));
    }

    let is_esm = has_module_syntax(&module);
    let stats = SourceStats {
        bytes: source.len(),
        lines: source.lines().count(),
        items: module.body.len(),
    };

    let unresolved_mark = Mark::new();
    let top_level_mark = Mark::new();

    let mut pass = resolver(unresolved_mark, top_level_mark, false);
    module.visit_mut_with(&mut pass);

    Ok(ParsedModule {
        module,
        comments,
        cm,
        unresolved_mark,
        top_level_mark,
        filename: filename.to_string(),
        stats,
        is_esm,
    })
}

pub fn emit(
    module: &Module,
    cm: Lrc<SourceMap>,
    comments: &SingleThreadedComments,
    minify: bool,
) -> Result<String> {
    let cfg = CodegenConfig::default()
        .with_target(EsVersion::EsNext)
        .with_minify(minify)
        .with_ascii_only(false)
        .with_omit_last_semi(false);
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    {
        let writer = JsWriter::new(cm.clone(), "\n", &mut buf, None);
        let mut emitter = Emitter {
            cfg,
            cm,
            comments: Some(comments),
            wr: Box::new(writer),
        };
        emitter.emit_module(module)?;
    }
    Ok(String::from_utf8(buf)?)
}

pub fn has_module_syntax(module: &Module) -> bool {
    module.body.iter().any(|item| {
        matches!(
            item,
            ModuleItem::ModuleDecl(decl) if matches!(
                decl,
                swc_ecma_ast::ModuleDecl::Import(_)
                    | swc_ecma_ast::ModuleDecl::ExportDecl(_)
                    | swc_ecma_ast::ModuleDecl::ExportNamed(_)
                    | swc_ecma_ast::ModuleDecl::ExportDefaultDecl(_)
                    | swc_ecma_ast::ModuleDecl::ExportAll(_)
            )
        )
    })
}
