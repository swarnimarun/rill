//! The version-zero scalar device map. Construction verifies the complete body.
use std::collections::HashMap;

/// LLVM device calling convention and resource layout for the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Nvptx,
    Vulkan,
}

const NVPTX: &str = include_str!("../examples/gpu-ir/clamp-nvptx.ll");
const VULKAN: &str = include_str!("../examples/gpu-ir/clamp-vulkan.ll");
pub const MAX_OPS: usize = 256;
pub const MAX_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    I32,
    Bool,
}

#[derive(Clone, Copy, Debug)]
enum Op {
    Input,
    Const(i32),
    Slt(usize, usize),
    Select(usize, usize, usize),
}

/// A verified unary i32 map, with no externally mutable instruction storage.
pub struct Kernel {
    ops: Vec<Op>,
    result: usize,
}

fn valid_name(name: &str) -> bool {
    let Some(name) = name.strip_prefix('%') else {
        return false;
    };
    let mut chars = name.bytes();
    matches!(chars.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && chars.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

impl Kernel {
    /// Check the version, instruction types, definitions and resource bounds.
    pub fn parse(source: &str) -> Result<Self, String> {
        if source.len() as u64 > MAX_BYTES {
            return Err("IR exceeds 64 KiB".to_owned());
        }
        let mut lines = source.lines().enumerate().filter_map(|(n, line)| {
            let line = line.split("//").next()?.trim();
            (!line.is_empty()).then_some((n + 1, line))
        });
        for header in ["rir 0", "kernel map.i32 workgroup_x 64"] {
            let (n, line) = lines.next().ok_or_else(|| format!("expected '{header}'"))?;
            if !line.split_ascii_whitespace().eq(header.split(' ')) {
                return Err(format!("line {n}: expected '{header}'"));
            }
        }
        let mut names = HashMap::from([("%input", (0, Ty::I32))]);
        let mut ops = vec![Op::Input];
        let mut result = None;
        for (line_no, line) in lines {
            let fail = |msg: &str| format!("line {line_no}: {msg}");
            if result.is_some() {
                return Err(fail("operation after return"));
            }
            let arg = |name: &str, expected: Ty| -> Result<usize, String> {
                let &(id, actual) = names
                    .get(name)
                    .ok_or_else(|| fail(&format!("unknown value '{name}'")))?;
                if actual != expected {
                    return Err(fail(&format!(
                        "'{name}' requires {expected:?}, found {actual:?}"
                    )));
                }
                Ok(id)
            };
            let words = line.split_ascii_whitespace().collect::<Vec<_>>();
            if let ["return", value] = words.as_slice() {
                result = Some(arg(value, Ty::I32)?);
                continue;
            }
            let [name, "=", body @ ..] = words.as_slice() else {
                return Err(fail("expected a definition or return"));
            };
            if !valid_name(name) || names.contains_key(name) {
                return Err(fail("invalid or duplicate SSA name"));
            }
            if ops.len() > MAX_OPS {
                return Err(fail("kernel exceeds 256 operations"));
            }
            let (op, ty) = match body {
                ["const.i32", n] => (
                    Op::Const(n.parse().map_err(|_| fail("expected signed i32 literal"))?),
                    Ty::I32,
                ),
                ["slt.i32", a, b] => (Op::Slt(arg(a, Ty::I32)?, arg(b, Ty::I32)?), Ty::Bool),
                ["select.i32", c, a, b] => (
                    Op::Select(arg(c, Ty::Bool)?, arg(a, Ty::I32)?, arg(b, Ty::I32)?),
                    Ty::I32,
                ),
                _ => return Err(fail("unknown instruction or wrong operand count")),
            };
            names.insert(*name, (ops.len(), ty));
            ops.push(op);
        }
        Ok(Self {
            ops,
            result: result.ok_or("missing return")?,
        })
    }

    /// Evaluate one active lane using the target-independent semantics.
    pub fn eval(&self, input: i32) -> i32 {
        // The verifier fixes the type of every slot. Predicates are 0 or 1.
        let mut values: Vec<i32> = Vec::with_capacity(self.ops.len());
        for op in &self.ops {
            let value = match *op {
                Op::Input => input,
                Op::Const(n) => n,
                Op::Slt(a, b) => i32::from(values[a] < values[b]),
                Op::Select(c, a, b) => {
                    if values[c] != 0 {
                        values[a]
                    } else {
                        values[b]
                    }
                }
            };
            values.push(value);
        }
        values[self.result]
    }

    /// Emit a complete LLVM module with the selected target ABI.
    pub fn emit_llvm(&self, target: Target) -> String {
        let template = match target {
            Target::Nvptx => NVPTX,
            Target::Vulkan => VULKAN,
        };
        const START: &str = "; begin rill.map";
        const END: &str = "; end rill.map";
        let (prefix, body) = template.split_once(START).expect("checked-in map start");
        let (_, suffix) = body.split_once(END).expect("checked-in map end");
        let mut text = format!("{prefix}{START}\n");
        let mut values = Vec::with_capacity(self.ops.len());
        for (id, op) in self.ops.iter().enumerate() {
            let value = match *op {
                Op::Input => "%input_value".to_owned(),
                Op::Const(n) => n.to_string(),
                Op::Slt(a, b) => {
                    text.push_str(&format!(
                        "  %map_{id} = icmp slt i32 {}, {}\n",
                        values[a], values[b]
                    ));
                    format!("%map_{id}")
                }
                Op::Select(c, a, b) => {
                    text.push_str(&format!(
                        "  %map_{id} = select i1 {}, i32 {}, i32 {}\n",
                        values[c], values[a], values[b]
                    ));
                    format!("%map_{id}")
                }
            };
            values.push(value);
        }
        // LLVM has no SSA move instruction. Adding zero is exact for all i32.
        text.push_str(&format!(
            "  %mapped_value = add i32 {}, 0\n{END}{suffix}",
            values[self.result]
        ));
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_clamp_and_lowers_the_verified_body() {
        let kernel = Kernel::parse(include_str!("../examples/gpu-ir/clamp.rir")).unwrap();
        for n in [i32::MIN, -42, 0, 1, i32::MAX] {
            assert_eq!(kernel.eval(n), n.max(0));
        }
        let constant =
            Kernel::parse("rir 0\nkernel map.i32 workgroup_x 64\n%n = const.i32 -21\nreturn %n")
                .unwrap();
        for target in [Target::Nvptx, Target::Vulkan] {
            let module = kernel.emit_llvm(target);
            assert!(module.contains("icmp slt i32 %input_value, 0"));
            assert!(module.contains("select i1 %map_2, i32 0, i32 %input_value"));
            assert!(constant
                .emit_llvm(target)
                .contains("%mapped_value = add i32 -21, 0"));
        }
    }

    #[test]
    fn rejects_invalid_types_names_control_and_work_limits() {
        let header = "rir 0\nkernel map.i32 workgroup_x 64\n";
        for body in [
            "",
            "return %missing",
            "%x = const.i32 2147483648\nreturn %x",
            "%x = slt.i32 %input %input\nreturn %x",
            "%x = select.i32 %input %input %input\nreturn %x",
            "%x = const.i32 0\n%x = const.i32 1\nreturn %x",
            "return %input\nreturn %input",
            "%bad! = const.i32 1\nreturn %bad!",
            "%0 = load %input\nreturn %0",
        ] {
            assert!(Kernel::parse(&format!("{header}{body}")).is_err(), "{body}");
        }
        let mut source = header.to_string();
        for n in 0..=MAX_OPS {
            source.push_str(&format!("%v{n} = const.i32 0\n"));
        }
        assert!(Kernel::parse(&(source + "return %input")).is_err());
    }
}
