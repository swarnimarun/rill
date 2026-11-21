//! In-memory compiler views for the local learning playground. The host runs
//! each request in a bounded worker. No view invokes a compiler tool or reads
//! a project directory. Semantic views use the ordinary module and type rules.
use crate::{
    ast::Program,
    kernel_ir::{Kernel, Target},
    lexer::Kind,
    modules::{self, provenance::SourceProgram, resolve},
    source_map::AstNode,
    syntax::SyntaxTree,
};
use serde_json::{json, Value};
use std::fmt::{self, Write};

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_INPUTS: usize = 256;
const OUTPUT_LIMIT: &str = "E-EXPLORER-LIMIT: compiler output exceeds 256 KiB";
const NATIVE_BOUNDARY: &str = "E-EXPLORER-NATIVE: native directives and extern declarations require a local project and the Rill CLI";

/// Inspect one Rill source buffer using a named compiler view. Both success
/// and failure carry text so the caller can display compiler results verbatim.
pub fn inspect(request: &Value) -> Value {
    let diagnostics = match evaluate(request) {
        Ok(output) if output.len() <= MAX_OUTPUT_BYTES => {
            return json!({"ok": true, "output": output, "diagnostics": ""});
        }
        Err(error) if error.len() <= MAX_OUTPUT_BYTES => error,
        _ => OUTPUT_LIMIT.to_owned(),
    };
    json!({"ok": false, "output": "", "diagnostics": diagnostics})
}

fn evaluate(request: &Value) -> Result<String, String> {
    let source = request
        .get("source")
        .and_then(Value::as_str)
        .ok_or("E-EXPLORER-REQUEST: source must be a string")?;
    let view = request
        .get("view")
        .and_then(Value::as_str)
        .ok_or("E-EXPLORER-REQUEST: view must be a string")?;
    if source.len() > MAX_SOURCE_BYTES {
        return Err("E-EXPLORER-LIMIT: source exceeds 64 KiB".into());
    }
    if view == "tokens" {
        return tokens(source);
    }
    let tree = SyntaxTree::parse(source);
    let parsed = tree.parsed_module().ok_or_else(|| {
        tree.diagnostics()
            .iter()
            .map(|d| {
                let pos = d.span.start;
                let location = format!("{}:{}", pos.line, pos.column);
                let message = d
                    .message
                    .strip_prefix(&format!("{location}: "))
                    .unwrap_or(&d.message);
                let mut report =
                    format!("playground.rill:{location}: {}: {message}", d.code.as_str(),);
                if let Some(fix) = d.insertion {
                    write!(
                        report,
                        "\n  {} at {}:{}",
                        fix.title, fix.position.line, fix.position.column
                    )
                    .unwrap();
                }
                report
            })
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    reject_native(&parsed.program)?;
    match view {
        "ast" => return display_bounded(format_args!("{:#?}", parsed.program)),
        "format" => return crate::formatter::format(source),
        "check" | "run" | "types" | "typed-ir" | "c" | "rir" | "nvptx" | "vulkan" | "ir-eval" => (),
        _ => return Err(format!("E-EXPLORER-REQUEST: unknown view '{view}'")),
    }
    let require_main = matches!(view, "run" | "c" | "typed-ir")
        || matches!(view, "check" | "types")
            && parsed.program.functions.iter().any(|f| f.name == "main");
    let graph = modules::load_buffer(source).map_err(|e| e.to_string())?;
    let resolved =
        modules::compile::prepare_with_sources(graph, require_main).map_err(|e| e.to_string())?;
    let program = resolved.program();
    // Apply the same admission rule to the whole imported graph before any execution.
    reject_native(program)?;
    match view {
        "check" => Ok("Type check passed.".to_owned()),
        "run" => display_bounded(format_args!("{}", crate::interpreter::run(program)?)),
        "types" => signatures(&resolved),
        "typed-ir" => {
            let mut output = BoundedText(String::new());
            crate::typed::inspection::describe(&resolved, &mut output)?;
            Ok(output.0)
        }
        "c" => crate::backend::emit(program),
        _ => inspect_kernel(&resolved, view, request),
    }
}

fn reject_native(program: &Program) -> Result<(), String> {
    if program.native != crate::c_import::Config::default()
        || !program.externs.is_empty()
        || !program.constants.is_empty()
    {
        Err(NATIVE_BOUNDARY.into())
    } else {
        Ok(())
    }
}

fn tokens(source: &str) -> Result<String, String> {
    let tokens = crate::lexer::lex(source)?;
    let mut output = BoundedText(String::new());
    for token in tokens {
        if matches!(token.kind, Kind::At | Kind::Extern) {
            return Err(NATIVE_BOUNDARY.into());
        }
        writeln!(output, "{}:{}\t{:?}", token.line, token.column, token.kind)
            .map_err(|_| OUTPUT_LIMIT.to_owned())?;
    }
    Ok(output.0)
}

fn signatures(source: &SourceProgram) -> Result<String, String> {
    let checked = crate::check::analyze_library(source.program())?;
    let mut output = BoundedText(String::new());
    for symbol in source.symbols() {
        if symbol.symbol.kind != resolve::Kind::Function
            || source
                .source(symbol.source)
                .is_none_or(|s| s.module != "@entry")
        {
            continue;
        }
        let Some(AstNode::Function(function)) = source.node(&symbol.target) else {
            return Err("E-EXPLORER: missing resolved function body".into());
        };
        let detail = crate::lsp::analysis::function_detail(
            &symbol.symbol.name,
            function,
            &checked.functions[&function.name],
        );
        writeln!(output, "{detail}").map_err(|_| OUTPUT_LIMIT.to_owned())?;
    }
    Ok(output.0)
}

fn inspect_kernel(source: &SourceProgram, view: &str, request: &Value) -> Result<String, String> {
    let entry = match request.get("entry") {
        None => "clamp",
        Some(value) => value
            .as_str()
            .ok_or("E-EXPLORER-REQUEST: entry must be a function name")?,
    };
    let text = crate::kernel_source::lower_program(source, entry)?;
    if view == "rir" {
        return Ok(text);
    }
    let kernel = Kernel::parse(&text)?;
    match view {
        "nvptx" => Ok(kernel.emit_llvm(Target::Nvptx)),
        "vulkan" => Ok(kernel.emit_llvm(Target::Vulkan)),
        "ir-eval" => {
            let inputs = match request.get("inputs") {
                None => &[][..],
                Some(value) => value
                    .as_array()
                    .ok_or("E-EXPLORER-REQUEST: inputs must be an array of signed i32 values")?
                    .as_slice(),
            };
            if inputs.len() > MAX_INPUTS {
                return Err("E-EXPLORER-LIMIT: use at most 256 input lanes".into());
            }
            let mut result = String::new();
            for input in inputs {
                let n = input
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or("E-EXPLORER-REQUEST: each lane must be a signed i32 integer")?;
                writeln!(result, "{}", kernel.eval(n)).unwrap();
            }
            Ok(result)
        }
        _ => Err(format!("E-EXPLORER-REQUEST: unknown kernel view '{view}'")),
    }
}

struct BoundedText(String);

impl Write for BoundedText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if text.len() > MAX_OUTPUT_BYTES - self.0.len() {
            return Err(fmt::Error);
        }
        self.0.push_str(text);
        Ok(())
    }
}

fn display_bounded(args: fmt::Arguments<'_>) -> Result<String, String> {
    let mut output = BoundedText(String::new());
    output
        .write_fmt(args)
        .map_err(|_| OUTPUT_LIMIT.to_owned())?;
    Ok(output.0)
}
