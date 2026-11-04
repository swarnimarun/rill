use crate::{
    ast::{ParamMode, TypeExpr},
    check::{self, Type},
};
use std::collections::BTreeMap;

pub(super) fn type_texts(types: &[&Type]) -> Vec<String> {
    fn render(ty: &Type, budget: &mut usize, names: &mut BTreeMap<usize, String>) -> String {
        if *budget == 0 {
            return "…".into();
        }
        *budget -= 1;
        match ty {
            Type::Var(id) => {
                let next = names.len();
                names
                    .entry(*id)
                    .or_insert_with(|| {
                        if next < 26 {
                            format!("'{}", (b'a' + next as u8) as char)
                        } else {
                            format!("'t{next}")
                        }
                    })
                    .clone()
            }
            Type::Int => "i32".into(),
            Type::Bool => "bool".into(),
            Type::Unit => "unit".into(),
            Type::CStr => "cstr".into(),
            Type::CU8 => "u8".into(),
            Type::CU32 => "u32".into(),
            Type::CU64 => "u64".into(),
            Type::CF32 => "f32".into(),
            Type::StaticStr => "str".into(),
            Type::U8x16 => "u8x16".into(),
            Type::SystemBoxI32 => "SystemBoxI32".into(),
            Type::SystemBytes => "SystemBytes".into(),
            Type::Bytes(length) => format!("Bytes<{length}>"),
            Type::CPointer { pointee, is_const } => {
                format!("{}<{pointee}>", if *is_const { "const_ptr" } else { "ptr" })
            }
            Type::Callable {
                params,
                modes,
                result,
                ..
            } => {
                let arguments = params
                    .iter()
                    .zip(modes)
                    .map(|(ty, mode)| {
                        let mode = match mode {
                            ParamMode::Value => "",
                            ParamMode::Read => "read ",
                            ParamMode::Inout => "inout ",
                        };
                        format!("{mode}{}", render(ty, budget, names))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({arguments}) -> {}", render(result, budget, names))
            }
            Type::Nominal(name, args) => {
                let name = name
                    .strip_prefix("M$")
                    .and_then(|name| name.split_once('$'))
                    .map_or(name.as_str(), |(_, name)| name);
                if args.is_empty() {
                    name.to_owned()
                } else {
                    format!(
                        "{name}<{}>",
                        args.iter()
                            .map(|arg| render(arg, budget, names))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
        }
    }
    let mut budget = 256;
    let mut names = BTreeMap::new();
    types
        .iter()
        .map(|ty| render(ty, &mut budget, &mut names))
        .collect()
}

pub(super) fn type_text(ty: &Type) -> String {
    type_texts(&[ty]).pop().unwrap()
}

pub(super) fn annotation(ty: &TypeExpr) -> String {
    if ty.args.is_empty() {
        ty.name.clone()
    } else {
        format!(
            "{}<{}>",
            ty.name,
            ty.args
                .iter()
                .map(annotation)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

pub(super) fn function_detail(
    name: &str,
    function: &crate::ast::Function,
    signature: &check::FunctionType,
) -> String {
    let types = type_texts(
        &signature
            .params
            .iter()
            .chain([&signature.result])
            .collect::<Vec<_>>(),
    );
    let arguments = function
        .params
        .iter()
        .zip(&types)
        .map(|(parameter, ty)| {
            let mode = match parameter.mode {
                ParamMode::Value => "",
                ParamMode::Read => "read ",
                ParamMode::Inout => "inout ",
            };
            format!("{mode}{}: {ty}", parameter.name)
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("fn {name}({arguments}) -> {}", types.last().unwrap())
}
