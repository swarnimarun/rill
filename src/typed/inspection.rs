//! Read-only listing of reachable instances and executable operations. The
//! checker retains richer source and control witnesses separately from this view.
use super::{control::Tree, *};
use crate::{ast::Program, modules::provenance::SourceProgram, source_map::AstNode};
use std::fmt::{self, Write};

pub(crate) fn describe(source: &SourceProgram, out: &mut impl Write) -> Result<(), String> {
    let checked = crate::check::analyze(source.program())?;
    // This is the same path as lower_main, including verify(&result). Keep the
    // instance keys from this particular lowering to label specializations.
    let (program, instances) = lower_main_traced(&checked, None, None)?;
    let mut names = HashMap::new();
    for symbol in source.symbols() {
        if let Some(AstNode::Function(f)) = source.node(&symbol.target) {
            let file = source
                .source(symbol.source)
                .ok_or("E-TYPED-DUMP: missing source module")?;
            let label = if file.module == "@entry" {
                symbol.symbol.name.clone()
            } else {
                format!("{}.{}", file.module, symbol.symbol.name)
            };
            names.insert(f.name.as_str(), label);
        }
    }
    listing(out, &program, &instances, checked.program, &names)
        .map_err(|_| "E-EXPLORER-LIMIT: host IR listing exceeds the output budget".into())
}

fn listing(
    out: &mut impl Write,
    program: &TypedMain,
    instances: &instances::Instances,
    source: &Program,
    names: &HashMap<&str, String>,
) -> fmt::Result {
    writeln!(out, "Verified host IR\nentry f{}", program.entry)?;
    for (i, (function, instance)) in program.functions.iter().zip(&instances.list).enumerate() {
        let origin = &source.functions[instance.source];
        let name = names
            .get(origin.name.as_str())
            .map_or(origin.name.as_str(), String::as_str);
        write!(out, "\nfn f{i} {name}(")?;
        for (idx, ((id, ty, mode), param)) in function.params.iter().zip(&origin.params).enumerate()
        {
            if idx > 0 {
                out.write_str(", ")?;
            }
            let mode = match mode {
                ParamMode::Value => "",
                ParamMode::Read => "read ",
                ParamMode::Inout => "inout ",
            };
            write!(
                out,
                "{mode}%{} {}: {}",
                id.0,
                param.name,
                type_name(*ty, program)
            )?;
        }
        let result = match function.result {
            ResultRepr::Value(t) => type_name(t, program),
            ResultRepr::NoNormal(_) => "no normal result".into(),
            ResultRepr::Discarded(_) => "discarded logical result".into(),
        };
        writeln!(out, ") -> {result}")?;
        let root = match &function.body {
            Computation::Value(v) => Tree::Value(v),
            Computation::NoNormal(v) => Tree::NoNormal(v),
            Computation::Discarded(v) => Tree::Discarded(v),
        };
        let mut pending = vec![(root, 1)];
        while let Some((node, depth)) = pending.pop() {
            write!(out, "{:width$}", "", width = 2 * depth)?;
            match node {
                Tree::Value(v) => {
                    operation(out, &v.kind)?;
                    writeln!(out, " : {}", type_name(v.ty, program))?;
                }
                Tree::NoNormal(n) => {
                    use NoNormalKind::*;
                    let op = match &n.kind {
                        Call(f, _) => format!("Call f{f}"),
                        InterruptedCall(_) => "InterruptedCall".into(),
                        Let { local, .. } => format!("Let %{}", local.0),
                        DiscardLet { .. } => "DiscardLet".into(),
                        NoStorageLet { .. } => "NoStorageLet".into(),
                        If { .. } => "If".into(),
                        Match { .. } => "Match".into(),
                    };
                    writeln!(out, "{op} : no normal result")?;
                }
                Tree::Discarded(n) => {
                    use DiscardKind::*;
                    let op = match &n.kind {
                        Call(f, _) => format!("Call f{f}"),
                        Value(_) => "Value".into(),
                        FromNoNormal(_) => "FromNoNormal".into(),
                        Constructor { .. } => "Constructor".into(),
                        Record { .. } => "Record".into(),
                        Let { local, .. } => format!("Let %{}", local.0),
                        DiscardLet { .. } => "DiscardLet".into(),
                        NoStorageLet { .. } => "NoStorageLet".into(),
                        If { .. } => "If".into(),
                        Match { .. } => "Match".into(),
                    };
                    writeln!(out, "{op} : discarded logical result")?;
                }
            }
            let first = pending.len();
            node.children(|child| pending.push((child, depth + 1)));
            pending[first..].reverse();
        }
        for action in &function.param_cleanup {
            writeln!(
                out,
                "  parameter cleanup %{} {:?}",
                action.local.0, action.tree
            )?;
        }
    }
    Ok(())
}

fn type_name(ty: Type, program: &TypedMain) -> String {
    match ty {
        Type::Int => "i32",
        Type::Unit => "unit",
        Type::Bool => "bool",
        Type::StaticStr => "str",
        Type::CU8 => "u8",
        Type::CU32 => "u32",
        Type::CU64 => "u64",
        Type::CF32 => "f32",
        Type::U8x16 => "u8x16",
        Type::OwnedI32 => "SystemBoxI32",
        Type::OwnedBytes => "SystemBytes",
        Type::Bytes(n) => return format!("bytes<{n}>"),
        Type::RawPointer(id) => return format!("raw pointer #{}", id.0),
        Type::Adt { id, .. } => {
            return format!(
                "{} [layout {id}]",
                crate::display::constructor_name(&program.adts[id].name)
            )
        }
    }
    .into()
}

// Print only this instruction's immediate operands. Children are walked once
// using the verifier's traversal, and proof metadata never enters the listing.
fn operation(out: &mut impl Write, op: &ExprKind) -> fmt::Result {
    use ExprKind::*;
    match op {
        InterruptedCall(_) => out.write_str("InterruptedCall"),
        DiscardLet { .. } => out.write_str("DiscardLet"),
        NoStorageLet { .. } => out.write_str("NoStorageLet"),
        TryAlloc { kind, .. } => write!(out, "TryAlloc {kind:?}"),
        Int(n) => write!(out, "Int {n}"),
        Str(s) => write!(out, "Str {s:?}"),
        Bool(b) => write!(out, "Bool {b}"),
        Unit => out.write_str("Unit"),
        Read(id) => write!(out, "Read %{}", id.0),
        Record { adt, .. } => write!(out, "Record layout {adt}"),
        FieldRead(p) => write!(out, "FieldRead %{} {:?}", p.root.0, p.path),
        FieldAssign(p, _) => write!(out, "FieldAssign %{} {:?}", p.root.0, p.path),
        Call(f, args) => {
            write!(out, "Call f{f}(")?;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    out.write_str(", ")?;
                }
                match arg {
                    CallArg::Value(_) => out.write_str("value")?,
                    CallArg::ReadTemporary(_) => out.write_str("read temporary")?,
                    CallArg::ReadCopy(id) | CallArg::ReadOwner(id) => {
                        write!(out, "read %{}", id.0)?
                    }
                    CallArg::InoutCopy(id) | CallArg::InoutOwner(id) => {
                        write!(out, "inout %{}", id.0)?
                    }
                    CallArg::Move(id) => write!(out, "move %{}", id.0)?,
                    CallArg::ReadField(p) => write!(out, "read %{} {:?}", p.root.0, p.path)?,
                    CallArg::InoutField(p) => write!(out, "inout %{} {:?}", p.root.0, p.path)?,
                }
            }
            out.write_char(')')
        }
        ForeignCall(id, _) => write!(out, "ForeignCall #{}", id.0),
        ForeignConst(id) => write!(out, "ForeignConst #{}", id.0),
        ForeignConvert(op, _) => write!(out, "ForeignConvert {op:?}"),
        NullPointer => out.write_str("NullPointer"),
        IsNull(_) => out.write_str("IsNull"),
        Constructor { adt, variant, .. } => {
            write!(out, "Constructor layout {adt} variant {variant}")
        }
        Match { arms, .. } => {
            write!(out, "Match ({} arms)", arms.len())?;
            for (i, arm) in arms.iter().enumerate() {
                for action in &arm.cleanup {
                    write!(
                        out,
                        " [arm {i} cleanup %{} {:?}]",
                        action.local.0, action.tree
                    )?;
                }
            }
            Ok(())
        }
        CopyMatch { arms, .. } => write!(out, "CopyMatch ({} arms)", arms.len()),
        ByteLen(_) => out.write_str("ByteLen"),
        ByteAt(..) => out.write_str("ByteAt"),
        BytesNew(n) => write!(out, "BytesNew {n}"),
        Vector(op, _) => write!(out, "Vector {op:?}"),
        BytesLen(_) => out.write_str("BytesLen"),
        BytesGet(..) => out.write_str("BytesGet"),
        BytesSet(..) => out.write_str("BytesSet"),
        OwnerLet {
            local,
            init,
            cleanup,
            mutable,
            ..
        } => {
            write!(
                out,
                "OwnerLet {} %{} = ",
                if *mutable { "mut" } else { "value" },
                local.0
            )?;
            match init {
                OwnerInit::Alloc(_) => out.write_str("Alloc box")?,
                OwnerInit::BytesAlloc(_) => out.write_str("Alloc bytes")?,
                OwnerInit::Move(id) => write!(out, "Move %{}", id.0)?,
                OwnerInit::Call(_) => out.write_str("Call")?,
                OwnerInit::Expr(_) => out.write_str("Expr")?,
            }
            if let Some(action) = cleanup {
                write!(
                    out,
                    " [scope cleanup %{} {:?}]",
                    action.local.0, action.tree
                )?;
            }
            Ok(())
        }
        OwnerBytesLen(id) => write!(out, "OwnerBytesLen %{}", id.0),
        OwnerBytesGet(id, _) => write!(out, "OwnerBytesGet %{}", id.0),
        OwnerBytesSet(id, ..) => write!(out, "OwnerBytesSet %{}", id.0),
        OwnerRead(id) => write!(out, "OwnerRead %{}", id.0),
        OwnerMove(id) => write!(out, "OwnerMove %{}", id.0),
        OwnerWrite(id, _) => write!(out, "OwnerWrite %{}", id.0),
        Let { local, mutable, .. } => write!(
            out,
            "Let {} %{}",
            if *mutable { "mut" } else { "value" },
            local.0
        ),
        Assign(id, _) => write!(out, "Assign %{}", id.0),
        Binary(op, ..) => write!(out, "Binary {op:?}"),
        If(..) => out.write_str("If"),
        While(..) => out.write_str("While"),
    }
}
