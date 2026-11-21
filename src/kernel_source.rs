//! Lower one checked Rill function into the version-zero map.i32 profile.
use crate::{
    ast::{BinaryOp, Expr, ParamMode},
    check::{analyze_library, Type},
    kernel_ir::{Kernel, MAX_OPS},
    modules::{
        compile::load_library_with_sources_and_overlays, provenance::SourceProgram, resolve::Kind,
    },
    source_map::{AstNode, AstPath, AstStep, OriginKind},
};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    path::{Path, PathBuf},
};

struct Lower<'a> {
    source: &'a SourceProgram,
    input: &'a str,
    output: String,
    definitions: usize,
    constants: HashMap<i32, String>,
}

impl Lower<'_> {
    fn error(&self, path: &AstPath, reason: &str) -> String {
        if let Some(origin) = self.source.origin(path).and_then(|n| n.origins.first()) {
            if let Some(file) = self.source.source(origin.source) {
                let pos = origin.span.start;
                return format!(
                    "{}:{}:{}: E-GPU-PROFILE: {reason}",
                    file.path.display(),
                    pos.line,
                    pos.column
                );
            }
        }
        format!("E-GPU-PROFILE: {reason}")
    }

    fn define(&mut self, path: &AstPath, operation: String) -> Result<String, String> {
        if self.definitions >= MAX_OPS {
            return Err(self.error(path, "map body exceeds 256 IR definitions"));
        }
        let name = format!("%v{}", self.definitions);
        self.definitions += 1;
        writeln!(self.output, "{name} = {operation}").unwrap();
        Ok(name)
    }

    fn constant(&mut self, path: &AstPath, n: i32) -> Result<String, String> {
        if let Some(name) = self.constants.get(&n) {
            return Ok(name.clone());
        }
        let name = self.define(path, format!("const.i32 {n}"))?;
        self.constants.insert(n, name.clone());
        Ok(name)
    }

    fn predicate(&mut self, expr: &Expr, path: &AstPath) -> Result<String, String> {
        let Expr::Binary(BinaryOp::Lt, left, right) = expr else {
            return Err(self.error(path, "map condition must be a signed i32 '<' comparison"));
        };
        let left = self.value(left, &path.child(AstStep::BinaryLeft))?;
        let right = self.value(right, &path.child(AstStep::BinaryRight))?;
        self.define(path, format!("slt.i32 {left} {right}"))
    }

    fn value(&mut self, expr: &Expr, path: &AstPath) -> Result<String, String> {
        match expr {
            Expr::Var(n) if n == self.input => Ok("%input".into()),
            Expr::Int(n) => self.constant(path, *n),
            // Ordinary negative literals are parsed as 0 - magnitude. Use the
            // original operator role to distinguish them from written subtraction.
            Expr::Binary(BinaryOp::Sub, _, arg)
                if self.source.origin(path).is_some_and(|node| {
                    node.origins
                        .first()
                        .is_some_and(|o| o.provenance == OriginKind::UnaryNegation)
                }) =>
            {
                let value = match arg.as_ref() {
                    Expr::Int(n) => n.checked_neg(),
                    _ => None,
                }
                .ok_or_else(|| {
                    self.error(
                        path,
                        "only representable negative i32 literals can be negated in a map",
                    )
                })?;
                self.constant(path, value)
            }
            Expr::If(cond, yes, no) => {
                let cond = self.predicate(cond, &path.child(AstStep::Condition))?;
                // Eager select preserves evaluation because every admitted arm
                // is total and pure. Validate both, including unselected arms.
                let yes = self.value(yes, &path.child(AstStep::Then))?;
                let no = self.value(no, &path.child(AstStep::Else))?;
                self.define(path, format!("select.i32 {cond} {yes} {no}"))
            }
            Expr::Call(..) | Expr::Apply(..) => {
                Err(self.error(path, "calls are unsupported in a map body"))
            }
            _ => Err(self.error(
                path,
                "map values must be i32 literals, the input parameter, or an i32 conditional",
            )),
        }
    }
}

/// Resolve and type-check the source graph, lower the named entry-module function,
/// then verify its emitted map.i32 IR. File and profile failures return diagnostics.
pub fn lower(entry: &Path, name: &str) -> Result<String, String> {
    lower_file(entry, name, &BTreeMap::new())
}

fn lower_file(
    entry: &Path,
    name: &str,
    overlays: &BTreeMap<PathBuf, String>,
) -> Result<String, String> {
    let source =
        load_library_with_sources_and_overlays(entry, overlays).map_err(|e| e.to_string())?;
    let symbol = source
        .symbols()
        .iter()
        .find(|s| {
            s.symbol.kind == Kind::Function
                && s.symbol.name == name
                && source
                    .source(s.source)
                    .is_some_and(|f| f.module == "@entry")
        })
        .ok_or_else(|| format!("E-GPU-ENTRY: no function '{name}' in the entry module"))?;
    let Some(AstNode::Function(function)) = source.node(&symbol.target) else {
        return Err(format!(
            "E-GPU-ENTRY: '{name}' requires a Rill function body"
        ));
    };
    let checked = analyze_library(source.program())?;
    let signature = &checked.functions[&function.name];
    if !signature.quantified.is_empty()
        || signature.params != [Type::Int]
        || signature.result != Type::Int
        || function.params[0].mode != ParamMode::Value
    {
        return Err(format!(
            "E-GPU-SIGNATURE: '{name}' requires one by-value i32 parameter and an i32 result"
        ));
    }
    let mut lower = Lower {
        source: &source,
        input: &function.params[0].name,
        output: "rir 0\nkernel map.i32 workgroup_x 64\n".into(),
        definitions: 0,
        constants: HashMap::new(),
    };
    let result = lower.value(&function.body, &symbol.target.child(AstStep::FunctionBody))?;
    writeln!(lower.output, "return {result}").unwrap();
    Kernel::parse(&lower.output)
        .map_err(|error| format!("E-GPU-IR: generated map failed verification: {error}"))?;
    Ok(lower.output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(src: &str) -> Result<String, String> {
        let entry = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/gpu-ir/buffer.rill");
        lower_file(
            &entry,
            "map",
            &BTreeMap::from([(entry.clone(), src.into())]),
        )
    }

    #[test]
    fn lowers_checked_inference_and_signed_literals_deterministically() {
        let text = lower("fn map(x) { if x < 0 { 0 } else { x } }").unwrap();
        assert_eq!(text, "rir 0\nkernel map.i32 workgroup_x 64\n%v0 = const.i32 0\n%v1 = slt.i32 %input %v0\n%v2 = select.i32 %v1 %v0 %input\nreturn %v2\n");
        for n in [i32::MIN, -42, 0, 42, i32::MAX] {
            let src = format!("fn map(x: i32) {{ {n} }}");
            assert!(lower(&src)
                .unwrap()
                .ends_with(&format!("%v0 = const.i32 {n}\nreturn %v0\n")));
        }
        for body in ["if x < 0 { x } else { if x < 2 { 1 } else { 2 } }", "x"] {
            assert!(lower(&format!("fn map(x: i32) {{ {body} }}")).is_ok());
        }
    }

    #[test]
    fn verified_maps_agree_with_both_source_interpreters() {
        for body in [
            "if x < 0 { 0 } else { x }",
            "if x < -17 { 14 } else { if 3 < x { -42 } else { x } }",
            "if (if x < 0 { 4 } else { 2 }) < x { -1 } else { x }",
        ] {
            let source = format!("fn map(x:i32) {{ {body} }}");
            let kernel = Kernel::parse(&lower(&source).unwrap()).unwrap();
            for input in [i32::MIN, -18, -17, -1, 0, 2, 3, 4, i32::MAX] {
                let prog = crate::parser::parse(&format!("{source} fn main() {{ map({input}) }}"))
                    .unwrap();
                let actual = kernel.eval(input).to_string();
                assert_eq!(
                    actual,
                    crate::interpreter::run(&prog).unwrap().to_string(),
                    "{body} at {input}"
                );
                assert_eq!(
                    actual,
                    crate::interpreter::run_reference(&prog)
                        .unwrap()
                        .to_string(),
                    "{body} at {input}"
                );
            }
        }
    }

    #[test]
    fn definition_budget_matches_the_ir_consumer() {
        let mut body = "x".to_owned();
        for _ in 0..7 {
            body = format!("if x < 0 {{ {body} }} else {{ {body} }}");
        }
        // 127 comparisons, 127 selects, and two distinct literals.
        let body = body.replacen("{ x }", "{ 1 }", 1);
        let output = lower(&format!("fn map(x:i32) {{ {body} }}")).unwrap();
        assert_eq!(
            output.lines().filter(|line| line.starts_with('%')).count(),
            256
        );
        let body = body.replacen("{ x }", "{ 2 }", 1);
        assert!(lower(&format!("fn map(x:i32) {{ {body} }}"))
            .unwrap_err()
            .contains("exceeds 256"));
    }

    #[test]
    fn requires_a_checked_monomorphic_value_signature_and_entry_identity() {
        for src in [
            "fn map(x) { x }",
            "fn map(x: bool) { 1 }",
            "fn map(x: i32) { x < 0 }",
            "fn map(read x: i32) { x }",
            "fn map(inout x: i32) { x }",
            "fn map() { 0 }",
            "fn map(x: i32,y:i32) { x }",
        ] {
            assert!(
                lower(src).unwrap_err().starts_with("E-GPU-SIGNATURE:"),
                "{src}"
            );
        }
        assert!(lower("extern fn map(x:i32) -> i32;")
            .unwrap_err()
            .starts_with("E-GPU-ENTRY:"));
        assert!(lower("fn unused(x) { 1 + true } fn map(x:i32){x}")
            .unwrap_err()
            .contains("E-TYPE:"));
        let entry = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/gpu-ir/buffer.rill");
        let mut overlays = BTreeMap::from([
            (entry.clone(), "import helper; fn other() {0}".into()),
            (
                entry.with_file_name("helper.rill"),
                "export fn map(x:i32) { 42 }".into(),
            ),
        ]);
        assert!(lower_file(&entry, "map", &overlays)
            .unwrap_err()
            .contains("entry module"));
        overlays.insert(entry.clone(), "import helper; fn map(x:i32) { x }".into());
        assert!(lower_file(&entry, "map", &overlays)
            .unwrap()
            .ends_with("return %input\n"));
    }

    #[test]
    fn rejects_lazy_traps_effects_and_general_cpu_expressions_with_locations() {
        for (body, why) in [
            ("if x < x { 1 / 0 } else { x }", "map values"),
            (
                "if x < x { read_device() } else { x }",
                "calls are unsupported",
            ),
            ("if true { x } else { 0 }", "map condition"),
            ("if x > 1 { x } else { 0 }", "map condition"),
            ("0 - 7", "map values"),
            ("-x", "negative i32 literals"),
            ("-(-2147483648)", "negative i32 literals"),
            ("let y = x; y", "map values"),
        ] {
            let src = format!("extern fn read_device() -> i32;\nfn map(x:i32) {{ {body} }}");
            let error = lower(&src).unwrap_err();
            assert!(
                error.contains("buffer.rill:2:") && error.contains(why),
                "{body}: {error}"
            );
        }
    }
}
