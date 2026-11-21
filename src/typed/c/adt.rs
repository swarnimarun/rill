use super::*;
pub(super) fn zero(ty: Type) -> String {
    match ty {
        Type::Adt { .. } => format!("({}){{0}}", c_type(ty)),
        _ => "NULL".into(),
    }
}
pub(super) fn drop_value(ty: Type, value: &str) -> String {
    match ty {
        Type::Adt { id, owned: true } => format!("rl_drop_adt_{id}(&({value}));"),
        Type::OwnedI32 | Type::OwnedBytes => format!("rl_release({value});"),
        _ => String::new(),
    }
}
pub(super) fn layouts(emitter: &mut Emitter<'_>) {
    for (id, adt) in emitter.program.adts.iter().enumerate() {
        if adt.record_fields.is_some() {
            records::layout(emitter, id);
            continue;
        }
        emitter.out.push_str("typedef struct { int tag; ");
        if adt
            .variants
            .iter()
            .any(|variant| !variant.fields.is_empty())
        {
            emitter.out.push_str("union {\n");
            for (tag, variant) in adt.variants.iter().enumerate() {
                if variant.fields.is_empty() {
                    continue;
                }
                emitter.out.push_str("struct { ");
                for (field, ty) in variant.fields.iter().enumerate() {
                    write!(emitter.out, "{} f{field}; ", c_type(*ty)).unwrap();
                }
                writeln!(emitter.out, "}} v{tag};").unwrap();
            }
            emitter.out.push_str("} payload; ");
        }
        writeln!(emitter.out, "}} rl_adt_{id};").unwrap();
        // The same layout limit used for scalar values bounds target padding.
        writeln!(
            emitter.out,
            "_Static_assert(sizeof(rl_adt_{id}) <= {}, \"unsupported nominal layout\");",
            type_cost(
                Type::Adt {
                    id,
                    owned: adt.owned
                },
                &emitter.adt_costs
            )
        )
        .unwrap();
        if adt.owned {
            writeln!(
                emitter.out,
                "static void rl_drop_adt_{id}(const rl_adt_{id} *value) {{ switch(value->tag) {{"
            )
            .unwrap();
            for (tag, variant) in adt.variants.iter().enumerate() {
                writeln!(emitter.out, "case {tag}:").unwrap();
                for (field, ty) in variant.fields.iter().enumerate().rev() {
                    emitter
                        .out
                        .push_str(&drop_value(*ty, &format!("value->payload.v{tag}.f{field}")));
                }
                emitter.out.push_str("break;\n");
            }
            emitter.out.push_str("default: rl_trap(); } }\n");
        }
    }
}
impl Emitter<'_> {
    pub(super) fn drop_cleanup(&mut self, cleanup: &super::super::Cleanup) {
        // Verification currently admits only atomic owner cleanup. Owned
        // record source guards stay closed until both consumers support it.
        assert_eq!(cleanup.tree, super::super::CleanupTree::DropWholeOwner);
        let id = cleanup.local;
        let ty = self.owners[&id];
        self.out
            .push_str(&drop_value(ty, &format!("rl_l_{}", id.0)));
        writeln!(self.out, " rl_l_{} = {};", id.0, zero(ty)).unwrap();
    }
    pub(super) fn constructor(
        &mut self,
        result: &str,
        ty: &str,
        adt: usize,
        variant: usize,
        fields: &[ExprNode],
    ) -> EmitOutcome {
        let mut values = Vec::with_capacity(fields.len());
        for field in fields {
            values.push(emit_value!(self.expr(field)));
        }
        let fields = values;
        writeln!(
            self.out,
            "{ty} {result} = ({ty}){{0}}; {result}.tag = {variant};"
        )
        .unwrap();
        for (index, value) in fields.iter().enumerate() {
            writeln!(self.out, "{result}.payload.v{variant}.f{index} = {value};").unwrap();
        }
        let _ = adt;
        EmitOutcome::Value(result.to_owned())
    }
    pub(super) fn match_value(
        &mut self,
        result: &str,
        ty: &str,
        value: &ExprNode,
        arms: &[super::super::AdtArm],
    ) -> EmitOutcome {
        let Type::Adt { id, .. } = value.ty else {
            unreachable!()
        };
        let value_ty = value.ty;
        let value = emit_value!(self.expr(value));
        writeln!(self.out, "{ty} {result}; switch ({value}.tag) {{").unwrap();
        let mut has_default = false;
        let mut normal = false;
        for arm in arms {
            let fields = if let Some(tag) = arm.variant {
                writeln!(self.out, "case {tag}: {{").unwrap();
                self.program.adts[id].variants[tag].fields.clone()
            } else {
                has_default = true;
                self.out.push_str("default: {\n");
                vec![value_ty]
            };
            for (field, (id, field_ty)) in arm.fields.iter().zip(fields).enumerate() {
                if let Some(id) = id {
                    self.add_cost(field_ty);
                    let source = if let Some(tag) = arm.variant {
                        format!("{value}.payload.v{tag}.f{field}")
                    } else {
                        value.clone()
                    };
                    writeln!(self.out, "{} rl_l_{} = {source};", c_type(field_ty), id.0).unwrap();
                    if field_ty.is_owner() {
                        self.owners.insert(*id, field_ty);
                    }
                }
            }
            if let EmitOutcome::Value(body) = self.expr(&arm.body) {
                normal = true;
                for id in &arm.cleanup {
                    self.drop_cleanup(id);
                }
                writeln!(self.out, "{result} = {body}; break;").unwrap();
            }
            self.out.push_str("}\n");
        }
        if !has_default {
            self.out.push_str("default: rl_trap();\n");
        }
        self.out.push_str("}\n");
        if normal {
            EmitOutcome::Value(result.to_owned())
        } else {
            EmitOutcome::NoNormal
        }
    }
}

pub(super) fn printers(emitter: &mut Emitter<'_>) {
    for (id, adt) in emitter.program.adts.iter().enumerate() {
        if adt.owned {
            continue;
        }
        if adt.record_fields.is_some() {
            records::printer(emitter, id);
            continue;
        }
        writeln!(
            emitter.out,
            "static void rl_print_adt_{id}(const rl_adt_{id} *value) {{ switch(value->tag) {{"
        )
        .unwrap();
        for (tag, variant) in adt.variants.iter().enumerate() {
            let name = crate::display::constructor_name(&variant.name);
            let name = name
                .as_bytes()
                .iter()
                .map(|byte| format!("\\{byte:03o}"))
                .collect::<String>();
            writeln!(emitter.out, "case {tag}: fputs(\"{name}\", stdout);").unwrap();
            if !variant.fields.is_empty() {
                emitter.out.push_str("fputc('(', stdout);\n");
            }
            for (field, ty) in variant.fields.iter().enumerate() {
                if field > 0 {
                    emitter.out.push_str("fputs(\", \", stdout);\n");
                }
                let value = format!("value->payload.v{tag}.f{field}");
                print_value(emitter, *ty, &value);
            }
            if !variant.fields.is_empty() {
                emitter.out.push_str("fputc(')', stdout);\n");
            }
            emitter.out.push_str("break;\n");
        }
        emitter.out.push_str("default: rl_trap(); } }\n");
    }
}

pub(super) fn print_value(emitter: &mut Emitter<'_>, ty: Type, value: &str) {
    match ty {
        Type::Int => writeln!(emitter.out, "printf(\"%\" PRId32, {value});").unwrap(),
        Type::CU8 | Type::CU32 | Type::CU64 => {
            writeln!(emitter.out, "printf(\"%\" PRIu64, (uint64_t)({value}));").unwrap()
        }
        Type::CF32 => writeln!(emitter.out, "printf(\"%.9g\", (double)({value}));").unwrap(),
        Type::RawPointer(_) => writeln!(emitter.out, "printf(\"%p\", (void *)({value}));").unwrap(),
        Type::Bool => writeln!(
            emitter.out,
            "fputs({value} ? \"true\" : \"false\", stdout);"
        )
        .unwrap(),
        Type::Unit => emitter.out.push_str("fputs(\"()\", stdout);\n"),
        Type::StaticStr => {
            writeln!(emitter.out, "fwrite({value}.data, 1, {value}.len, stdout);").unwrap()
        }
        Type::U8x16 => writeln!(emitter.out, "rl_print_vector({value});").unwrap(),
        Type::Bytes(n) => {
            writeln!(emitter.out,"fputs(\"bytes[\", stdout); for(size_t i=0;i<{n};++i) {{ if(i) fputs(\", \", stdout); printf(\"%u\", (unsigned){value}.data[i]); }} fputc(']', stdout);").unwrap();
        }
        Type::Adt { id, .. } => writeln!(emitter.out, "rl_print_adt_{id}(&({value}));").unwrap(),
        _ => unreachable!("verified Copy aggregate cannot contain owner"),
    }
}
