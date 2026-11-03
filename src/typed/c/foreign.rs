//! ABI-facing native calls. Rill values are snapshots before the C call;
//! `_Bool` belongs only to the ABI and converts to/from internal typed bool.
use super::*;
use crate::ast::ExternType;
use crate::typed::{ForeignArg, ForeignConstantId, ForeignConvert, ForeignFunctionId};

impl Emitter<'_> {
    pub(super) fn foreign_call(
        &mut self,
        result: &str,
        id: ForeignFunctionId,
        args: &[ForeignArg],
    ) {
        let function = &self.program.native.functions[id.0 as usize];
        let mut values = Vec::new();
        for (arg, param) in args.iter().zip(&function.params) {
            let value = match arg {
                ForeignArg::StaticCStr(value) => literal(value),
                ForeignArg::Value(value) => {
                    // expr always writes a fresh local, including a place read.
                    // Later arguments may mutate that original Rill slot.
                    let value = self.expr(value);
                    if param.ty == ExternType::Bool {
                        let abi = self.temp();
                        self.add_cost(Type::Bool);
                        writeln!(self.out, "_Bool {abi} = ({value} != 0);").unwrap();
                        abi
                    } else {
                        value
                    }
                }
            };
            values.push(value);
        }
        let value = format!("{}({})", function.name, values.join(", "));
        match function.result {
            ExternType::I32 => writeln!(self.out, "int32_t {result} = {value};").unwrap(),
            ExternType::Bool => self.foreign_bool(result, &value),
            ExternType::Unit => {
                writeln!(self.out, "{value}; rl_unit {result} = (rl_unit){{0}};").unwrap();
            }
            ExternType::U8
            | ExternType::U32
            | ExternType::U64
            | ExternType::F32
            | ExternType::Pointer { .. } => {
                let ty = c_type(
                    self.program
                        .native
                        .runtime_type(&function.result)
                        .expect("verified native result"),
                );
                writeln!(self.out, "{ty} {result} = {value};").unwrap();
            }
            ExternType::CStr => unreachable!("verified native result"),
        }
    }

    pub(super) fn foreign_constant(&mut self, result: &str, id: ForeignConstantId) {
        let constant = &self.program.native.constants[id.0 as usize];
        match constant.ty {
            ExternType::I32 => {
                writeln!(self.out, "int32_t {result} = (int32_t)({});", constant.name).unwrap();
            }
            ExternType::Bool => self.foreign_bool(result, &constant.name),
            ExternType::U8
            | ExternType::U32
            | ExternType::U64
            | ExternType::F32
            | ExternType::Pointer { .. } => {
                let ty = c_type(
                    self.program
                        .native
                        .runtime_type(&constant.ty)
                        .expect("verified native constant"),
                );
                writeln!(self.out, "{ty} {result} = ({ty})({});", constant.name).unwrap();
            }
            ExternType::Unit | ExternType::CStr => unreachable!("verified native constant"),
        }
    }

    pub(super) fn foreign_convert(
        &mut self,
        result: &str,
        ty: &str,
        conversion: ForeignConvert,
        input: &ExprNode,
    ) {
        let input = self.expr(input);
        match conversion {
            ForeignConvert::U8 => {
                writeln!(self.out, "if ({input} < 0 || {input} > 255) rl_trap();").unwrap()
            }
            ForeignConvert::U32 | ForeignConvert::U64 => {
                writeln!(self.out, "if ({input} < 0) rl_trap();").unwrap()
            }
            ForeignConvert::F32 => {}
        }
        writeln!(self.out, "{ty} {result} = ({ty}){input};").unwrap();
    }

    fn foreign_bool(&mut self, result: &str, value: &str) {
        let abi = self.temp();
        self.add_cost(Type::Bool);
        writeln!(
            self.out,
            "_Bool {abi} = ({value}); int {result} = {abi} ? 1 : 0;"
        )
        .unwrap();
    }
}

fn literal(value: &str) -> String {
    let mut literal = String::from("\"");
    for byte in value.as_bytes() {
        write!(literal, "\\{byte:03o}").unwrap();
    }
    literal.push('"');
    literal
}
