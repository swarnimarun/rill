use super::*;
use crate::ast::{MatchArm, Pattern};
impl Lower<'_, '_> {
    pub(super) fn same_states(a: &HashMap<String, Local>, b: &HashMap<String, Local>) -> bool {
        a.iter().filter(|(_, l)| l.ty.is_owner()).all(|(name, l)| {
            b.get(name).is_some_and(|r| {
                r.id == l.id
                    && r.ty == l.ty
                    && r.initialization == l.initialization
                    && r.loaned == l.loaned
                    && r.shared == l.shared
                    && r.field_loans == l.field_loans
            })
        })
    }

    pub(super) fn constructor(
        &mut self,
        ty: Type,
        name: &str,
        args: &[Expr],
    ) -> Result<ExprKind, String> {
        let Type::Adt { id, .. } = ty else {
            return Err("E-TYPED-ADT: constructor requires nominal type".into());
        };
        let variant = self.registry.types[id]
            .variants
            .iter()
            .position(|v| v.name == name)
            .ok_or("E-TYPED-ADT: wrong constructor")?;
        let fields = args
            .iter()
            .enumerate()
            .map(|(i, a)| self.expr_child(a, NodeRole::ConstructorField(i)))
            .collect::<Result<_, _>>()?;
        Ok(ExprKind::Constructor {
            adt: id,
            variant,
            fields,
        })
    }
    pub(super) fn match_adt(
        &mut self,
        value: &Expr,
        arms: &[MatchArm],
    ) -> Result<ExprKind, String> {
        let value = self.expr(value)?;
        let Type::Adt { id, .. } = value.ty else {
            return Err("E-TYPED-ADT: match requires nominal type".into());
        };
        let variants = self.registry.types[id].variants.clone();
        let outer = self.locals.clone();
        let mut lowered = Vec::new();
        let mut joined = None;
        for arm in arms {
            self.locals = outer.clone();
            let (variant, patterns, field_types) = match &arm.pattern {
                Pattern::Constructor(name, patterns) => {
                    let variant = variants
                        .iter()
                        .position(|v| &v.name == name)
                        .ok_or("E-TYPED-ADT: wrong match constructor")?;
                    (
                        Some(variant),
                        patterns.clone(),
                        variants[variant].fields.clone(),
                    )
                }
                Pattern::Binding(_) | Pattern::Wildcard => {
                    (None, vec![arm.pattern.clone()], vec![value.ty])
                }
                _ => return Err("E-TYPED-ADT: unsupported pattern".into()),
            };
            if patterns.len() != field_types.len() {
                return Err("E-TYPED-ADT: wrong pattern field count".into());
            }
            let mut fields = Vec::new();
            let mut bindings = Vec::new();
            for (pattern, ty) in patterns.iter().zip(&field_types) {
                match pattern {
                    Pattern::Binding(name) => {
                        let local = self.fresh()?;
                        self.locals.insert(
                            name.clone(),
                            Local {
                                id: local,
                                ty: *ty,
                                mutable: false,
                                initialization: Initialization::full(*ty, &self.registry.types)?,
                                loaned: false,
                                shared: 0,
                                field_loans: Vec::new(),
                                mode: ParamMode::Value,
                            },
                        );
                        fields.push(Some(local));
                        bindings.push((local, Some(name.clone()), *ty));
                    }
                    Pattern::Wildcard if ty.is_owner() => {
                        let local = self.fresh()?;
                        fields.push(Some(local));
                        bindings.push((local, None, *ty));
                    }
                    Pattern::Wildcard => fields.push(None),
                    _ => {
                        return Err(
                            "E-TYPED-ADT: nested or literal field pattern unsupported".into()
                        )
                    }
                }
            }
            let body = self.expr(&arm.body)?;
            let mut cleanup = Vec::new();
            for (id, name, ty) in bindings.iter().rev() {
                if ty.is_owner() {
                    let initialization = if let Some(name) = name {
                        self.locals
                            .get(name)
                            .filter(|local| local.id == *id)
                            .ok_or("E-TYPED-ADT: lost payload state")?
                            .initialization
                            .clone()
                    } else {
                        Initialization::full(*ty, &self.registry.types)?
                    };
                    if let Some(entry) =
                        initialization.cleanup_entry(*id, *ty, &self.registry.types)?
                    {
                        cleanup.push(entry);
                    }
                }
            }
            let after = outer
                .iter()
                .map(|(name, old)| {
                    let local = self
                        .locals
                        .get(name)
                        .filter(|l| l.id == old.id)
                        .cloned()
                        .unwrap_or_else(|| old.clone());
                    (name.clone(), local)
                })
                .collect::<HashMap<_, _>>();
            if let Some(first) = &joined {
                if !Self::same_states(first, &after) {
                    return Err("E-TYPED-ADT: incompatible arm ownership".into());
                }
            } else {
                joined = Some(after);
            }
            lowered.push(AdtArm {
                variant,
                fields,
                body,
                cleanup,
            });
        }
        self.locals = joined.unwrap_or(outer);
        Ok(ExprKind::Match {
            value: Box::new(value),
            arms: lowered,
        })
    }
}
