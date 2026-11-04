use super::*;
pub(super) fn layout(emitter: &mut Emitter<'_>, id: usize) {
    let adt = &emitter.program.adts[id];
    emitter.out.push_str("typedef struct { ");
    if adt.variants[0].fields.is_empty() {
        emitter.out.push_str("unsigned char unused; ");
    }
    for (field, ty) in adt.variants[0].fields.iter().enumerate() {
        write!(emitter.out, "{} f{field}; ", c_type(*ty)).unwrap();
    }
    writeln!(emitter.out, "}} rl_adt_{id};").unwrap();
    writeln!(
        emitter.out,
        "_Static_assert(sizeof(rl_adt_{id}) <= {}, \"unsupported record layout\");",
        emitter.adt_costs[id]
    )
    .unwrap();
}
fn quoted(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|b| format!("\\{b:03o}"))
        .collect()
}
pub(super) fn printer(emitter: &mut Emitter<'_>, id: usize) {
    let adt = &emitter.program.adts[id];
    let names = adt.record_fields.as_ref().expect("verified record");
    let name = quoted(crate::display::constructor_name(&adt.name));
    writeln!(
        emitter.out,
        "static void rl_print_adt_{id}(const rl_adt_{id} *value) {{ fputs(\"{name} {{\", stdout);"
    )
    .unwrap();
    for (index, (name, ty)) in names.iter().zip(&adt.variants[0].fields).enumerate() {
        let prefix = if index == 0 { " " } else { "; " };
        let name = quoted(name);
        writeln!(emitter.out, "fputs(\"{prefix}{name} = \", stdout);").unwrap();
        adt::print_value(emitter, *ty, &format!("value->f{index}"));
    }
    if !names.is_empty() {
        emitter.out.push_str("fputc(' ', stdout);\n");
    }
    emitter.out.push_str("fputc('}', stdout); }\n");
}
impl Emitter<'_> {
    pub(super) fn field_place(&self, place: &super::super::FieldPlace) -> String {
        let mut result = self.copy_place(place.root);
        for step in &place.path {
            write!(result, ".f{}", step.field).unwrap();
        }
        result
    }
    pub(super) fn record(
        &mut self,
        result: &str,
        ty: &str,
        fields: &[(usize, ExprNode)],
    ) -> EmitOutcome {
        let mut values = Vec::with_capacity(fields.len());
        for (index, field) in fields {
            values.push((*index, emit_value!(self.expr(field))));
        }
        writeln!(self.out, "{ty} {result} = ({ty}){{0}};").unwrap();
        for (index, value) in values {
            writeln!(self.out, "{result}.f{index} = {value};").unwrap();
        }
        EmitOutcome::Value(result.to_owned())
    }
}
