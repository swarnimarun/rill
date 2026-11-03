//! C pattern tests only project a payload after its enclosing tag matched.
use super::*;
use crate::typed::{CopyArm, CopyPattern};

impl Emitter<'_> {
    pub(super) fn match_copy(
        &mut self,
        result: &str,
        ty: &str,
        value: &ExprNode,
        arms: &[CopyArm],
    ) {
        let value_type = value.ty;
        let value = self.expr(value);
        writeln!(self.out, "{ty} {result};").unwrap();
        for (index, arm) in arms.iter().enumerate() {
            if index != 0 {
                self.out.push_str("else ");
            }
            let condition = condition(&arm.pattern, &value);
            writeln!(self.out, "if ({condition}) {{").unwrap();
            self.bind_pattern(&arm.pattern, value_type, &value);
            let body = self.expr(&arm.body);
            writeln!(self.out, "{result} = {body}; }}").unwrap();
        }
        self.out.push_str("else { rl_trap(); }\n");
    }

    fn bind_pattern(&mut self, pattern: &CopyPattern, ty: Type, value: &str) {
        match pattern {
            CopyPattern::Binding(id) => {
                self.add_cost(ty);
                writeln!(self.out, "{} rl_l_{} = {value};", c_type(ty), id.0).unwrap();
            }
            CopyPattern::Constructor {
                adt,
                variant,
                fields,
            } => {
                let types = self.program.adts[*adt].variants[*variant].fields.clone();
                for (index, (field, ty)) in fields.iter().zip(types).enumerate() {
                    self.bind_pattern(field, ty, &format!("{value}.payload.v{variant}.f{index}"));
                }
            }
            _ => {}
        }
    }
}

fn condition(pattern: &CopyPattern, value: &str) -> String {
    match pattern {
        CopyPattern::Wildcard | CopyPattern::Binding(_) | CopyPattern::Unit => "1".into(),
        CopyPattern::Int(n) => format!("({value} == INT32_C({n}))"),
        CopyPattern::Bool(b) => format!("({value} == {})", i32::from(*b)),
        CopyPattern::Constructor {
            variant, fields, ..
        } => {
            let mut tests = vec![format!("({value}.tag == {variant})")];
            for (index, field) in fields.iter().enumerate() {
                tests.push(condition(
                    field,
                    &format!("{value}.payload.v{variant}.f{index}"),
                ));
            }
            format!("({})", tests.join(" && "))
        }
    }
}
