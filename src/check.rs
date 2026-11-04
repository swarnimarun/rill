//! Type inference and exhaustiveness checking for the sequential subset.
use crate::ast::{
    BinaryOp, Expr, ExternConst, ExternFunction, ExternType, ParamMode, Pattern, Program, TypeExpr,
};
use std::collections::{HashMap, HashSet};

mod alloc;
mod exhaustiveness;
mod graph;
mod layout;
mod owned_types;
mod ownership;
mod solver;
mod type_cursor;
pub use owned_types::CheckedVariant;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Var(usize),
    Int,
    Bool,
    Unit,
    CStr,
    CU8,
    CU32,
    CU64,
    CF32,
    CPointer { pointee: String, is_const: bool },
    StaticStr,
    Bytes(usize),
    U8x16,
    SystemBoxI32,
    SystemBytes,
    Nominal(String, Vec<Type>),
}
struct Scheme {
    quantified: Vec<usize>,
    ty: Type,
    mutable: bool,
    borrowed: bool,
    local_id: usize,
}

#[derive(Clone)]
struct Loan {
    root: usize,
    path: Vec<usize>,
    exclusive: bool,
}
#[derive(Clone)]
pub struct FunctionType {
    pub quantified: Vec<usize>,
    pub params: Vec<Type>,
    pub result: Type,
}

#[cfg(test)]
pub(crate) type ConcreteInstanceKey = (String, Vec<Type>, Type);

/// Inferred expression and function types. The borrow keeps expression identities
/// valid until a consumer finishes lowering this exact program.
pub struct CheckedProgram<'a> {
    pub program: &'a Program,
    pub functions: HashMap<String, FunctionType>,
    expression_types: HashMap<*const Expr, Type>,
    field_info: HashMap<*const Expr, Vec<FieldInfo>>,
    record_info: HashMap<*const Expr, RecordInfo>,
    // Source binding identities, not names; no cloned type trees are retained.
    polymorphic_locals: HashMap<*const Expr, usize>,
    local_uses: HashMap<*const Expr, usize>,
    has_owned: bool,
    owned_type_cache: std::sync::Mutex<HashMap<Type, bool>>,
    adt_variant_cache: std::sync::Mutex<HashMap<Type, Vec<CheckedVariant>>>,
}

#[derive(Clone, Debug)]
pub struct FieldInfo {
    pub record: String,
    pub index: usize,
    pub arity: usize,
}
#[derive(Clone, Debug)]
pub struct RecordInfo {
    pub constructor: String,
    pub field_indices: Vec<usize>,
}

impl CheckedProgram<'_> {
    pub fn has_c_values(&self) -> bool {
        self.expression_types.values().any(|ty| {
            matches!(
                ty,
                Type::CU8 | Type::CU32 | Type::CU64 | Type::CF32 | Type::CPointer { .. }
            )
        })
    }
    pub fn has_owned(&self) -> bool {
        self.has_owned
    }
    pub fn expression_type(&self, expression: &Expr) -> Option<&Type> {
        self.expression_types.get(&(expression as *const Expr))
    }

    pub fn field_info(&self, expression: &Expr) -> Option<&[FieldInfo]> {
        self.field_info
            .get(&(expression as *const Expr))
            .map(Vec::as_slice)
    }
    pub fn record_info(&self, expression: &Expr) -> Option<&RecordInfo> {
        self.record_info.get(&(expression as *const Expr))
    }

    pub(crate) fn polymorphic_local(&self, expression: &Expr) -> Option<usize> {
        self.polymorphic_locals
            .get(&(expression as *const Expr))
            .copied()
    }
    pub(crate) fn local_use(&self, expression: &Expr) -> Option<usize> {
        self.local_uses.get(&(expression as *const Expr)).copied()
    }

    /// Compute a sound inline capacity over all reachable specializations.
    pub fn max_inline_nodes(&self) -> Result<usize, String> {
        layout::max_inline_nodes(self)
    }

    #[cfg(test)]
    pub(crate) fn reference_instance_keys(&self) -> Result<HashSet<ConcreteInstanceKey>, String> {
        layout::reference_instance_keys(self)
    }
}
#[derive(Clone)]
struct Constructor {
    owner: String,
    params: Vec<String>,
    fields: Vec<TypeExpr>,
    record_fields: Option<Vec<String>>,
}
struct Checker {
    allocation_types: Option<crate::ast::AllocationTypes>,
    solver: solver::Solver,
    level: usize,
    functions: HashMap<String, FunctionType>,
    parameter_modes: HashMap<String, Vec<ParamMode>>,
    next_local: usize,
    active_loans: Vec<Loan>,
    constructors: HashMap<String, Constructor>,
    variants: HashMap<String, Vec<String>>,
    pending_matches: Vec<(Type, Vec<Pattern>)>,
    equality_types: Vec<Type>,
    pointer_types: Vec<Type>,
    constants: HashMap<String, Type>,
    expression_types: HashMap<*const Expr, Type>,
    field_info: HashMap<*const Expr, Vec<FieldInfo>>,
    record_info: HashMap<*const Expr, RecordInfo>,
    // Source binding identities, not names; no cloned type trees are retained.
    polymorphic_locals: HashMap<*const Expr, usize>,
    local_uses: HashMap<*const Expr, usize>,
    has_owned: bool,
}
impl Checker {
    fn conflicts(&self, root: usize, path: &[usize], exclusive: bool) -> bool {
        self.active_loans.iter().any(|loan| {
            loan.root == root
                && (loan.path.starts_with(path) || path.starts_with(&loan.path))
                && (exclusive || loan.exclusive)
        })
    }
    fn local_id(&mut self) -> usize {
        let id = self.next_local;
        self.next_local += 1;
        id
    }
    fn fresh(&mut self) -> Type {
        self.solver.fresh(self.level)
    }
    fn resolve(&self, ty: &Type) -> Result<Type, String> {
        self.solver.resolve(ty)
    }
    fn copy_type(&self, ty: &Type) -> Result<Type, String> {
        self.solver.copy_type(ty)
    }
    fn copy_scheme(&self, scheme: &Scheme) -> Result<Scheme, String> {
        Ok(Scheme {
            quantified: scheme.quantified.clone(),
            ty: self.copy_type(&scheme.ty)?,
            mutable: scheme.mutable,
            borrowed: scheme.borrowed,
            local_id: scheme.local_id,
        })
    }
    fn copy_function(&self, scheme: &FunctionType) -> Result<FunctionType, String> {
        Ok(FunctionType {
            quantified: scheme.quantified.clone(),
            params: scheme
                .params
                .iter()
                .map(|ty| self.copy_type(ty))
                .collect::<Result<_, _>>()?,
            result: self.copy_type(&scheme.result)?,
        })
    }
    fn unify(&mut self, left: Type, right: Type) -> Result<(), String> {
        self.solver.unify(left, right)
    }
    fn substitute(&self, ty: &Type, replacements: &HashMap<usize, Type>) -> Result<Type, String> {
        self.solver.substitute(ty, replacements)
    }
    fn instantiate(&mut self, scheme: &Scheme) -> Result<Type, String> {
        let repl = scheme
            .quantified
            .iter()
            .map(|id| (*id, self.fresh()))
            .collect();
        self.substitute(&scheme.ty, &repl)
    }
    fn instantiate_function(&mut self, scheme: &FunctionType) -> Result<(Vec<Type>, Type), String> {
        let repl = scheme
            .quantified
            .iter()
            .map(|id| (*id, self.fresh()))
            .collect();
        Ok((
            scheme
                .params
                .iter()
                .map(|t| self.substitute(t, &repl))
                .collect::<Result<_, _>>()?,
            self.substitute(&scheme.result, &repl)?,
        ))
    }
    fn free_type(&self, ty: &Type, out: &mut HashSet<usize>) {
        self.solver.free_type(ty, out);
    }
    fn generalize(
        &mut self,
        mut free: HashSet<usize>,
        environment: &HashSet<usize>,
    ) -> Result<Vec<usize>, String> {
        // Pending raw pointer obligations remain monomorphic until a concrete
        // foreign context determines pointee identity and constness.
        let mut pointers = HashSet::new();
        for ty in &self.pointer_types {
            if let Type::Var(id) = self.resolve(ty)? {
                pointers.insert(id);
            }
        }
        free.retain(|id| !pointers.contains(id));
        // A captured environment may have acquired new representatives or
        // structural bindings since its free variable IDs were collected.
        let mut current_environment = HashSet::new();
        for id in environment {
            self.free_type(&Type::Var(*id), &mut current_environment);
        }
        let mut quantified = Vec::new();
        for id in free {
            let by_level = self.solver.level(id) > self.level;
            let by_environment = !current_environment.contains(&id);
            if by_level != by_environment {
                return Err(
                    "E-INTERNAL-TYPE: generalization level/environment disagreement".into(),
                );
            }
            if by_level {
                quantified.push(id);
            }
        }
        for id in pointers {
            self.solver.lower_variable(id, self.level);
        }
        // This is the only construction point for nonempty private schemes.
        // Consumers rely on sorted IDs for subquadratic membership checks.
        quantified.sort_unstable();
        Ok(quantified)
    }
    /// The initial value restriction is intentionally syntactic. In particular,
    /// a pure-looking call or branch is not evidence of safe generalization.
    /// Names follow the same checked declaration/local precedence as inference.
    fn nonexpansive(&self, value: &Expr, locals: &HashMap<String, Scheme>) -> bool {
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            match value {
                Expr::Int(_) | Expr::Bool(_) | Expr::Unit | Expr::Str(_) => {}
                Expr::Var(name) => {
                    if let Some(binding) = locals.get(name) {
                        if binding.mutable
                            || binding.borrowed
                            || self
                                .active_loans
                                .iter()
                                .any(|loan| loan.root == binding.local_id)
                        {
                            return false;
                        }
                    } else if !self.constants.contains_key(name)
                        && !self.constructors.get(name).is_some_and(|constructor| {
                            constructor.record_fields.is_none() && constructor.fields.is_empty()
                        })
                    {
                        return false;
                    }
                }
                Expr::Call(name, arguments) => {
                    if !self
                        .constructors
                        .get(name)
                        .is_some_and(|constructor| constructor.record_fields.is_none())
                    {
                        return false;
                    }
                    pending.extend(arguments);
                }
                Expr::Record(_, fields) => {
                    // Successful inference recorded the resolved nominal constructor.
                    if !self.record_info.contains_key(&(value as *const Expr)) {
                        return false;
                    }
                    pending.extend(fields.iter().map(|(_, value)| value));
                }
                _ => return false,
            }
        }
        true
    }

    /// Exclude known owners even in inactive variants or phantom arguments.
    /// Unresolved variables still obey the independent environment/level test;
    /// later owner instantiations retain the existing monomorphic owner checks.
    fn has_owner_dependency(&self, ty: &Type) -> Result<bool, String> {
        let resolved = self.resolve(ty)?;
        let mut types = vec![&resolved];
        let mut declarations = Vec::new();
        while let Some(ty) = types.pop() {
            match ty {
                Type::SystemBoxI32 | Type::SystemBytes => return Ok(true),
                Type::Nominal(name, arguments) => {
                    declarations.push(name.as_str());
                    types.extend(arguments);
                }
                _ => {}
            }
        }
        // All arguments, including phantom arguments, count as dependencies.
        // That conservative rule lets us inspect each declaration once without
        // expanding generic fields into potentially enormous substituted trees.
        let mut visited = HashSet::new();
        while let Some(name) = declarations.pop() {
            if !visited.insert(name) {
                continue;
            }
            for variant in &self.variants[name] {
                let constructor = &self.constructors[variant];
                let parameters: HashSet<_> = constructor.params.iter().collect();
                let mut fields: Vec<_> = constructor.fields.iter().collect();
                while let Some(field) = fields.pop() {
                    if parameters.contains(&field.name)
                        || matches!(field.name.as_str(), "ptr" | "const_ptr")
                    {
                        continue;
                    }
                    if matches!(field.name.as_str(), "SystemBoxI32" | "SystemBytes") {
                        return Ok(true);
                    }
                    if self.variants.contains_key(&field.name) {
                        declarations.push(field.name.as_str());
                    }
                    fields.extend(&field.args);
                }
            }
        }
        Ok(false)
    }

    fn free_scheme(&self, scheme: &Scheme, out: &mut HashSet<usize>) {
        let mut v = HashSet::new();
        self.free_type(&scheme.ty, &mut v);
        v.retain(|i| scheme.quantified.binary_search(i).is_err());
        out.extend(v);
    }
    fn free_functions(&self, out: &mut HashSet<usize>) {
        for f in self.functions.values() {
            let mut v = HashSet::new();
            for t in f.params.iter().chain(std::iter::once(&f.result)) {
                self.free_type(t, &mut v)
            }
            v.retain(|i| f.quantified.binary_search(i).is_err());
            out.extend(v);
        }
    }
    fn declared_type(&self, t: &TypeExpr, vars: &HashMap<String, Type>) -> Result<Type, String> {
        if let Some(ty) = vars.get(&t.name) {
            if !t.args.is_empty() {
                return Err(format!(
                    "E-TYPE: type parameter '{}' takes no arguments",
                    t.name
                ));
            }
            return self.copy_type(ty);
        }
        if matches!(t.name.as_str(), "ptr" | "const_ptr") {
            if t.args.len() != 1 || !t.args[0].args.is_empty() {
                return Err("E-FFI: pointer types require one C pointee name".into());
            }
            return Ok(Type::CPointer {
                pointee: t.args[0].name.clone(),
                is_const: t.name == "const_ptr",
            });
        }
        let primitive = match t.name.as_str() {
            "u8" => Some(Type::CU8),
            "u32" => Some(Type::CU32),
            "u64" => Some(Type::CU64),
            "f32" => Some(Type::CF32),
            "i32" => Some(Type::Int),
            "bool" => Some(Type::Bool),
            "unit" => Some(Type::Unit),
            "str" => Some(Type::StaticStr),
            "SystemBoxI32" => Some(Type::SystemBoxI32),
            "SystemBytes" => Some(Type::SystemBytes),
            "u8x16" => Some(Type::U8x16),
            _ => None,
        };
        if let Some(ty) = primitive {
            if !t.args.is_empty() {
                return Err(format!("E-TYPE: '{}' takes no arguments", t.name));
            }
            return Ok(ty);
        }
        let count = self
            .type_param_count(&t.name)
            .ok_or_else(|| format!("E-TYPE: unknown type '{}'", t.name))?;
        if count != t.args.len() {
            return Err(format!(
                "E-ARITY: type '{}' expects {count} arguments, got {}",
                t.name,
                t.args.len()
            ));
        }
        Ok(Type::Nominal(
            t.name.clone(),
            t.args
                .iter()
                .map(|x| self.declared_type(x, vars))
                .collect::<Result<_, _>>()?,
        ))
    }
    fn type_param_count(&self, name: &str) -> Option<usize> {
        self.variants
            .get(name)
            .and_then(|v| v.first())
            .and_then(|v| self.constructors.get(v))
            .map(|c| c.params.len())
    }
    fn constructor_type(&mut self, name: &str) -> Result<(Vec<Type>, Type), String> {
        let c = self
            .constructors
            .get(name)
            .cloned()
            .ok_or_else(|| format!("E-NAME: unknown constructor '{name}'"))?;
        let mut vars = HashMap::new();
        let mut args = Vec::new();
        for p in &c.params {
            let t = self.fresh();
            vars.insert(p.clone(), self.copy_type(&t)?);
            args.push(t);
        }
        let fields = c
            .fields
            .iter()
            .map(|f| self.declared_type(f, &vars))
            .collect::<Result<_, _>>()?;
        Ok((fields, Type::Nominal(c.owner, args)))
    }
    fn record_field(
        &mut self,
        binding: &Scheme,
        fields: &[String],
    ) -> Result<(Vec<FieldInfo>, Type), String> {
        if fields.is_empty() || fields.len() > 16 {
            return Err("E-RECORD: field path must contain 1 to 16 fields".into());
        }
        // Project from this use's instance, never the quantified scheme itself.
        // Mutable and borrowed roots have empty schemes and retain their type.
        let mut ty = self.instantiate(binding)?;
        let mut path = Vec::with_capacity(fields.len());
        for field in fields {
            let Type::Nominal(record, args) = self.resolve(&ty)? else {
                return Err(
                    "E-RECORD: field access requires a known record type (annotate the parameter)"
                        .into(),
                );
            };
            let constructor_name = self
                .variants
                .get(&record)
                .and_then(|v| v.first())
                .ok_or("E-RECORD: unknown record type")?;
            let constructor = &self.constructors[constructor_name];
            let names = constructor
                .record_fields
                .as_ref()
                .ok_or("E-RECORD: field access requires a record")?;
            let index = names
                .iter()
                .position(|name| name == field)
                .ok_or_else(|| format!("E-RECORD: record has no field '{field}'"))?;
            let params = constructor.params.iter().cloned().zip(args).collect();
            ty = self.declared_type(&constructor.fields[index], &params)?;
            path.push(FieldInfo {
                record,
                index,
                arity: names.len(),
            });
        }
        Ok((path, ty))
    }
    fn infer_pattern(
        &mut self,
        pattern: &Pattern,
        expected: Type,
        locals: &mut HashMap<String, Scheme>,
        bound: &mut HashSet<String>,
    ) -> Result<(), String> {
        match pattern {
            Pattern::Wildcard => Ok(()),
            Pattern::Binding(n) => {
                if !bound.insert(n.clone()) {
                    return Err(format!("E-NAME: duplicate pattern binding '{n}'"));
                }
                let local_id = self.local_id();
                locals.insert(
                    n.clone(),
                    Scheme {
                        quantified: vec![],
                        ty: expected,
                        mutable: false,
                        borrowed: false,
                        local_id,
                    },
                );
                Ok(())
            }
            Pattern::Bool(_) => self.unify(expected, Type::Bool),
            Pattern::Int(_) => self.unify(expected, Type::Int),
            Pattern::Unit => self.unify(expected, Type::Unit),
            Pattern::Constructor(n, ps) => {
                if self
                    .constructors
                    .get(n)
                    .is_some_and(|c| c.record_fields.is_some())
                {
                    return Err("E-RECORD: positional record patterns are unsupported".into());
                }
                let (fields, result) = self.constructor_type(n)?;
                if fields.len() != ps.len() {
                    return Err(format!(
                        "E-ARITY: constructor '{n}' expects {} arguments, got {}",
                        fields.len(),
                        ps.len()
                    ));
                }
                self.unify(expected, result)?;
                for (p, t) in ps.iter().zip(fields) {
                    self.infer_pattern(p, t, locals, bound)?
                }
                Ok(())
            }
        }
    }
    fn infer_bytes(
        &mut self,
        name: &str,
        args: &[Expr],
        locals: &mut HashMap<String, Scheme>,
    ) -> Result<Type, String> {
        let expected = match name {
            "bytes_new" | "bytes_len" => 1,
            "bytes_get" => 2,
            "bytes_set" => 3,
            _ => unreachable!(),
        };
        if args.len() != expected {
            return Err(format!(
                "E-ARITY: '{name}' expects {expected} arguments, got {}",
                args.len()
            ));
        }
        if name == "bytes_new" {
            let Expr::Int(capacity) = &args[0] else {
                return Err("E-BUFFER: bytes_new requires a literal capacity from 0 to 256".into());
            };
            if !(0..=256).contains(capacity) {
                return Err("E-BUFFER: bytes_new capacity must be from 0 to 256".into());
            }
            self.infer(&args[0], locals)?;
            return Ok(Type::Bytes(*capacity as usize));
        }
        let buffer = self.infer(&args[0], locals)?;
        let buffer = self.resolve(&buffer)?;
        if !matches!(buffer, Type::Bytes(_)) {
            return Err("E-BUFFER: expected a byte buffer with a known capacity".into());
        }
        for arg in &args[1..] {
            let ty = self.infer(arg, locals)?;
            self.unify(ty, Type::Int)?;
        }
        Ok(if name == "bytes_set" {
            buffer
        } else {
            Type::Int
        })
    }

    fn infer_owned(
        &mut self,
        name: &str,
        args: &[Expr],
        locals: &mut HashMap<String, Scheme>,
    ) -> Result<Type, String> {
        let expected = match name {
            "box_write" | "owned_bytes_get" => 2,
            "owned_bytes_set" => 3,
            _ => 1,
        };
        if args.len() != expected {
            return Err(format!(
                "E-ARITY: '{name}' expects {expected} arguments, got {}",
                args.len()
            ));
        }
        self.has_owned = true;
        if matches!(
            name,
            "system_box_i32"
                | "system_bytes_zeroed"
                | "system_box_i32_abort"
                | "system_bytes_zeroed_abort"
        ) {
            let value = self.infer(&args[0], locals)?;
            self.unify(value, Type::Int)?;
            let owner = if matches!(name, "system_box_i32" | "system_box_i32_abort") {
                Type::SystemBoxI32
            } else {
                Type::SystemBytes
            };
            if matches!(name, "system_box_i32" | "system_bytes_zeroed") {
                let metadata = self
                    .allocation_types
                    .as_ref()
                    .ok_or("E-ALLOC: import std.alloc to use fallible system allocation")?;
                return Ok(Type::Nominal(
                    metadata.result_type.clone(),
                    vec![owner, Type::Nominal(metadata.error_type.clone(), vec![])],
                ));
            }
            return Ok(owner);
        }
        let exclusive = matches!(name, "box_write" | "owned_bytes_set");
        let root = match (&args[0], exclusive) {
            (Expr::Inout(root), true) | (Expr::Var(root), false) => root,
            _ => return Err(format!("E-LOAN: {name} requires a direct owner root")),
        };
        let binding = locals
            .get(root)
            .map(|binding| self.copy_scheme(binding))
            .transpose()?
            .ok_or_else(|| format!("E-NAME: unknown owner '{root}'"))?;
        if exclusive && !binding.mutable {
            return Err(format!("E-LOAN: {name} root '{root}' must be mutable"));
        }
        if self.conflicts(binding.local_id, &[], exclusive) {
            return Err(format!("E-LOAN: cannot borrow root '{root}'"));
        }
        let owner_type = if matches!(name, "box_read" | "box_write") {
            Type::SystemBoxI32
        } else {
            Type::SystemBytes
        };
        self.unify(self.copy_type(&binding.ty)?, owner_type)?;
        self.expression_types
            .insert(&args[0] as *const Expr, binding.ty);
        let enclosing = self.active_loans.clone();
        self.active_loans.push(Loan {
            root: binding.local_id,
            path: vec![],
            exclusive,
        });
        for arg in &args[1..] {
            let ty = self.infer(arg, locals)?;
            self.unify(ty, Type::Int)?;
        }
        self.active_loans = enclosing;
        Ok(if exclusive { Type::Unit } else { Type::Int })
    }

    fn infer_vector(
        &mut self,
        name: &str,
        args: &[Expr],
        locals: &mut HashMap<String, Scheme>,
    ) -> Result<Type, String> {
        let expected = match name {
            "u8x16_any" | "u8x16_all" => 1,
            "u8x16_load" | "u8x16_lane" | "u8x16_and" | "u8x16_or" | "u8x16_xor" | "u8x16_eq" => 2,
            _ => unreachable!(),
        };
        if args.len() != expected {
            return Err(format!(
                "E-ARITY: '{name}' expects {expected} arguments, got {}",
                args.len()
            ));
        }
        let first = self.infer(&args[0], locals)?;
        if name == "u8x16_load" {
            if !matches!(self.resolve(&first)?, Type::Bytes(_)) {
                return Err("E-VECTOR: load requires a byte buffer with a known capacity".into());
            }
        } else {
            self.unify(first, Type::U8x16)?;
        }
        if let Some(second) = args.get(1) {
            let ty = self.infer(second, locals)?;
            self.unify(
                ty,
                if matches!(name, "u8x16_load" | "u8x16_lane") {
                    Type::Int
                } else {
                    Type::U8x16
                },
            )?;
        }
        Ok(match name {
            "u8x16_lane" => Type::Int,
            "u8x16_any" | "u8x16_all" => Type::Bool,
            _ => Type::U8x16,
        })
    }

    fn infer(&mut self, expr: &Expr, locals: &mut HashMap<String, Scheme>) -> Result<Type, String> {
        let ty = self.infer_inner(expr, locals)?;
        self.expression_types
            .insert(expr as *const Expr, self.copy_type(&ty)?);
        Ok(ty)
    }

    fn infer_inner(
        &mut self,
        expr: &Expr,
        locals: &mut HashMap<String, Scheme>,
    ) -> Result<Type, String> {
        match expr {
            Expr::Int(_) => Ok(Type::Int),
            Expr::Bool(_) => Ok(Type::Bool),
            Expr::Unit => Ok(Type::Unit),
            Expr::Str(value) => {
                if value.len() > i32::MAX as usize {
                    return Err("E-STRING: literal exceeds i32 byte length".into());
                }
                Ok(Type::StaticStr)
            }
            Expr::CStr(_) => Err(
                "E-FFI: C string literals are only allowed as direct cstr foreign arguments".into(),
            ),
            Expr::Inout(_) | Expr::InoutField(_, _) => {
                Err("E-LOAN: inout is only valid as an argument to an inout parameter".into())
            }
            Expr::Move(name) => {
                let binding = locals
                    .get(name)
                    .map(|binding| self.copy_scheme(binding))
                    .transpose()?
                    .ok_or_else(|| format!("E-NAME: unknown owner '{name}'"))?;
                if self.conflicts(binding.local_id, &[], true) {
                    return Err(format!("E-LOAN: cannot move borrowed root '{name}'"));
                }
                self.has_owned = true;
                Ok(binding.ty)
            }
            Expr::Var(n) => {
                if let Some(s) = locals
                    .get(n)
                    .map(|binding| self.copy_scheme(binding))
                    .transpose()?
                {
                    self.local_uses.insert(expr as *const Expr, s.local_id);
                    if self.conflicts(s.local_id, &[], false) {
                        return Err(format!(
                            "E-LOAN: cannot read exclusively borrowed root '{n}'"
                        ));
                    }
                    return self.instantiate(&s);
                }
                if let Some(ty) = self.constants.get(n) {
                    return self.copy_type(ty);
                }
                if self.constructors.contains_key(n) {
                    if self.constructors[n].record_fields.is_some() {
                        return Err("E-RECORD: use record construction syntax".into());
                    }
                    let (fields, result) = self.constructor_type(n)?;
                    if !fields.is_empty() {
                        return Err(format!(
                            "E-ARITY: constructor '{n}' expects {} arguments",
                            fields.len()
                        ));
                    }
                    return Ok(result);
                }
                Err(format!("E-NAME: unknown variable '{n}'"))
            }
            Expr::Record(name, fields) => {
                let constructor_name = self
                    .variants
                    .get(name)
                    .and_then(|v| v.first())
                    .cloned()
                    .ok_or_else(|| format!("E-RECORD: unknown record '{name}'"))?;
                let names = self.constructors[&constructor_name]
                    .record_fields
                    .clone()
                    .ok_or_else(|| format!("E-RECORD: '{name}' is not a record"))?;
                let (types, result) = self.constructor_type(&constructor_name)?;
                if fields.len() != names.len() {
                    return Err(format!(
                        "E-RECORD: record '{name}' requires {} fields",
                        names.len()
                    ));
                }
                let mut seen = HashSet::new();
                let mut indices = Vec::new();
                for (field, value) in fields {
                    let index = names
                        .iter()
                        .position(|n| n == field)
                        .ok_or_else(|| format!("E-RECORD: unknown field '{field}'"))?;
                    if !seen.insert(index) {
                        return Err(format!("E-RECORD: duplicate field '{field}'"));
                    }
                    let ty = self.infer(value, locals)?;
                    self.unify(ty, self.copy_type(&types[index])?)?;
                    indices.push(index);
                }
                self.record_info.insert(
                    expr as *const Expr,
                    RecordInfo {
                        constructor: constructor_name,
                        field_indices: indices,
                    },
                );
                Ok(result)
            }
            Expr::Field(root, field) => {
                let binding = locals
                    .get(root)
                    .map(|binding| self.copy_scheme(binding))
                    .transpose()?
                    .ok_or_else(|| format!("E-NAME: unknown record root '{root}'"))?;
                self.local_uses
                    .insert(expr as *const Expr, binding.local_id);
                let (info, ty) = self.record_field(&binding, field)?;
                let indices: Vec<_> = info.iter().map(|step| step.index).collect();
                if self.conflicts(binding.local_id, &indices, false) {
                    return Err(format!(
                        "E-LOAN: cannot read exclusively borrowed root '{root}'"
                    ));
                }
                self.field_info.insert(expr as *const Expr, info);
                Ok(ty)
            }
            Expr::FieldAssign(root, field, value) => {
                let binding = locals
                    .get(root)
                    .map(|binding| self.copy_scheme(binding))
                    .transpose()?
                    .ok_or_else(|| format!("E-NAME: unknown record root '{root}'"))?;
                if !binding.mutable {
                    return Err(format!(
                        "E-MUT: cannot assign to immutable binding '{root}'"
                    ));
                }
                let (info, field_type) = self.record_field(&binding, field)?;
                let indices: Vec<_> = info.iter().map(|step| step.index).collect();
                if self.conflicts(binding.local_id, &indices, true) {
                    return Err(format!("E-LOAN: cannot write borrowed root '{root}'"));
                }
                let ty = self.infer(value, locals)?;
                self.unify(ty, field_type)?;
                self.field_info.insert(expr as *const Expr, info);
                Ok(Type::Unit)
            }
            Expr::Binary(op, a, b) => {
                let a = self.infer(a, locals)?;
                let b = self.infer(b, locals)?;
                match op {
                    BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Rem => {
                        self.unify(a, Type::Int)?;
                        self.unify(b, Type::Int)?;
                        Ok(Type::Int)
                    }
                    BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        self.unify(a, Type::Int)?;
                        self.unify(b, Type::Int)?;
                        Ok(Type::Bool)
                    }
                    BinaryOp::And | BinaryOp::Or => {
                        self.unify(a, Type::Bool)?;
                        self.unify(b, Type::Bool)?;
                        Ok(Type::Bool)
                    }
                    BinaryOp::Eq | BinaryOp::Ne => {
                        self.unify(self.copy_type(&a)?, b)?;
                        self.equality_types.push(a);
                        Ok(Type::Bool)
                    }
                }
            }
            Expr::If(c, a, b) => {
                let c = self.infer(c, locals)?;
                self.unify(c, Type::Bool)?;
                let a = self.infer(a, locals)?;
                let b = self.infer(b, locals)?;
                self.unify(self.copy_type(&a)?, b)?;
                self.resolve(&a)
            }
            Expr::While(condition, body) => {
                let condition = self.infer(condition, locals)?;
                self.unify(condition, Type::Bool)?;
                let body = self.infer(body, locals)?;
                self.unify(body, Type::Unit)?;
                Ok(Type::Unit)
            }
            Expr::Let(n, value, body) => {
                self.level += 1;
                let inferred = self.infer(value, locals);
                self.level -= 1;
                let ty = inferred?;
                let mut free = HashSet::new();
                self.free_type(&ty, &mut free);
                let mut env = HashSet::new();
                for s in locals.values() {
                    self.free_scheme(s, &mut env)
                }
                self.free_functions(&mut env);
                // Keep the level/environment cross-check independent of the
                // value restriction. Rejected candidates become weak variables
                // at this boundary, so immutable aliases cannot generalize them.
                let mut quantified = self.generalize(free.clone(), &env)?;
                if !self.nonexpansive(value, locals) || self.has_owner_dependency(&ty)? {
                    for id in free {
                        self.solver.lower_variable(id, self.level);
                    }
                    quantified.clear();
                }
                let local_id = self.local_id();
                if !quantified.is_empty() {
                    self.polymorphic_locals
                        .insert(expr as *const Expr, local_id);
                }
                let old = locals.insert(
                    n.clone(),
                    Scheme {
                        quantified,
                        ty,
                        mutable: false,
                        borrowed: false,
                        local_id,
                    },
                );
                let result = self.infer(body, locals);
                match old {
                    Some(v) => {
                        locals.insert(n.clone(), v);
                    }
                    None => {
                        locals.remove(n);
                    }
                }
                result
            }
            Expr::LetMut(n, value, body) => {
                let ty = self.infer(value, locals)?;
                let local_id = self.local_id();
                let old = locals.insert(
                    n.clone(),
                    Scheme {
                        quantified: Vec::new(),
                        ty,
                        mutable: true,
                        borrowed: false,
                        local_id,
                    },
                );
                let result = self.infer(body, locals);
                match old {
                    Some(v) => {
                        locals.insert(n.clone(), v);
                    }
                    None => {
                        locals.remove(n);
                    }
                }
                result
            }
            Expr::Assign(n, value) => {
                let binding = locals
                    .get(n)
                    .map(|binding| self.copy_scheme(binding))
                    .transpose()?
                    .ok_or_else(|| format!("E-NAME: unknown variable '{n}'"))?;
                if !binding.mutable {
                    return Err(format!("E-MUT: cannot assign to immutable binding '{n}'"));
                }
                if self.conflicts(binding.local_id, &[], true) {
                    return Err(format!("E-LOAN: cannot write borrowed root '{n}'"));
                }
                let value = self.infer(value, locals)?;
                self.unify(binding.ty, value)?;
                Ok(Type::Unit)
            }
            Expr::Call(n, args) => {
                if matches!(n.as_str(), "u8" | "u32" | "u64" | "f32") {
                    if args.len() != 1 {
                        return Err(format!("E-ARITY: {n} expects one argument"));
                    }
                    let ty = self.infer(&args[0], locals)?;
                    self.unify(ty, Type::Int)?;
                    return Ok(match n.as_str() {
                        "u8" => Type::CU8,
                        "u32" => Type::CU32,
                        "u64" => Type::CU64,
                        _ => Type::CF32,
                    });
                }
                if n == "c_null" {
                    if !args.is_empty() {
                        return Err("E-ARITY: c_null expects no arguments".into());
                    }
                    let ty = self.fresh();
                    self.pointer_types.push(self.copy_type(&ty)?);
                    return Ok(ty);
                }
                if n == "is_null" {
                    if args.len() != 1 {
                        return Err("E-ARITY: is_null expects one argument".into());
                    }
                    let ty = self.infer(&args[0], locals)?;
                    self.pointer_types.push(ty);
                    return Ok(Type::Bool);
                }

                if matches!(
                    n.as_str(),
                    "system_box_i32"
                        | "system_bytes_zeroed"
                        | "system_box_i32_abort"
                        | "box_read"
                        | "box_write"
                        | "system_bytes_zeroed_abort"
                        | "owned_bytes_len"
                        | "owned_bytes_get"
                        | "owned_bytes_set"
                ) {
                    return self.infer_owned(n, args, locals);
                }
                if matches!(
                    n.as_str(),
                    "u8x16_load"
                        | "u8x16_lane"
                        | "u8x16_and"
                        | "u8x16_or"
                        | "u8x16_xor"
                        | "u8x16_eq"
                        | "u8x16_any"
                        | "u8x16_all"
                ) {
                    return self.infer_vector(n, args, locals);
                }
                if matches!(
                    n.as_str(),
                    "bytes_new" | "bytes_len" | "bytes_get" | "bytes_set"
                ) {
                    return self.infer_bytes(n, args, locals);
                }
                if self
                    .constructors
                    .get(n)
                    .is_some_and(|c| c.record_fields.is_some())
                {
                    return Err("E-RECORD: use record construction syntax".into());
                }
                let (params, result) = if self.constructors.contains_key(n) {
                    self.constructor_type(n)?
                } else {
                    let s = self
                        .functions
                        .get(n)
                        .map(|signature| self.copy_function(signature))
                        .transpose()?
                        .ok_or_else(|| format!("E-NAME: unknown function '{n}'"))?;
                    self.instantiate_function(&s)?
                };
                if params.len() != args.len() {
                    return Err(format!(
                        "E-ARITY: '{n}' expects {} arguments, got {}",
                        params.len(),
                        args.len()
                    ));
                }
                let modes = self
                    .parameter_modes
                    .get(n)
                    .cloned()
                    .unwrap_or_else(|| vec![ParamMode::Value; params.len()]);
                let enclosing_loans = self.active_loans.clone();
                for ((a, p), mode) in args.iter().zip(params).zip(modes) {
                    if mode == ParamMode::Inout {
                        let (name, field) = match a {
                            Expr::Inout(name) => (name, None),
                            Expr::InoutField(name, field) => (name, Some(field)),
                            _ => {
                                return Err(
                                    "E-LOAN: inout parameter requires an inout argument".into()
                                )
                            }
                        };
                        let binding = locals
                            .get(name)
                            .map(|binding| self.copy_scheme(binding))
                            .transpose()?
                            .ok_or_else(|| format!("E-NAME: unknown variable '{name}'"))?;
                        if !binding.mutable {
                            return Err(format!("E-LOAN: inout root '{name}' must be mutable"));
                        }
                        let (place_path, ty) = if let Some(field) = field {
                            let (info, ty) = self.record_field(&binding, field)?;
                            let indices = info.iter().map(|step| step.index).collect();
                            self.field_info.insert(a as *const Expr, info);
                            (indices, ty)
                        } else {
                            (Vec::new(), self.copy_type(&binding.ty)?)
                        };
                        if self.conflicts(binding.local_id, &place_path, true) {
                            return Err(format!(
                                "E-LOAN: cannot exclusively borrow borrowed root '{name}'"
                            ));
                        }
                        self.unify(self.copy_type(&ty)?, p)?;
                        self.expression_types.insert(a as *const Expr, ty);
                        self.active_loans.push(Loan {
                            root: binding.local_id,
                            path: place_path,
                            exclusive: true,
                        });
                        continue;
                    }
                    if mode == ParamMode::Read {
                        // A local name borrows its existing root. Any other
                        // expression is evaluated into a fresh call temporary.
                        let place = match a {
                            Expr::Var(name) => locals.get(name).map(|b| (b.local_id, Vec::new())),
                            Expr::Field(root, field) => {
                                let binding = locals.get(root).ok_or_else(|| {
                                    format!("E-NAME: unknown record root '{root}'")
                                })?;
                                let (info, _) = self.record_field(binding, field)?;
                                Some((
                                    binding.local_id,
                                    info.iter().map(|step| step.index).collect(),
                                ))
                            }
                            _ => None,
                        };
                        let ty = self.infer(a, locals)?;
                        self.unify(ty, p)?;
                        if let Some((root, path)) = place {
                            self.active_loans.push(Loan {
                                root,
                                path,
                                exclusive: false,
                            });
                        }
                        continue;
                    }
                    if p == Type::CStr {
                        let Expr::CStr(value) = a else {
                            return Err(
                                "E-FFI: cstr foreign arguments must be direct C string literals"
                                    .into(),
                            );
                        };
                        if value.contains('\0') {
                            return Err("E-FFI: C string literal contains NUL".into());
                        }
                        self.expression_types.insert(a as *const Expr, Type::CStr);
                        continue;
                    }
                    let t = self.infer(a, locals)?;
                    self.unify(t, p)?
                }
                self.active_loans = enclosing_loans;
                Ok(result)
            }
            Expr::Match(scrutinee, arms) => {
                let ty = self.infer(scrutinee, locals)?;
                let result = self.fresh();
                let mut patterns = Vec::new();
                for arm in arms {
                    let mut scope = locals
                        .iter()
                        .map(|(name, binding)| Ok((name.clone(), self.copy_scheme(binding)?)))
                        .collect::<Result<HashMap<_, _>, String>>()?;
                    self.infer_pattern(
                        &arm.pattern,
                        self.copy_type(&ty)?,
                        &mut scope,
                        &mut HashSet::new(),
                    )?;
                    let body = self.infer(&arm.body, &mut scope)?;
                    self.unify(self.copy_type(&result)?, body)?;
                    patterns.push(arm.pattern.clone());
                }
                self.pending_matches.push((ty, patterns));
                Ok(result)
            }
        }
    }
}
fn calls(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Binary(_, a, b) | Expr::Let(_, a, b) | Expr::LetMut(_, a, b) | Expr::While(a, b) => {
            calls(a, out);
            calls(b, out)
        }
        Expr::If(a, b, c) => {
            calls(a, out);
            calls(b, out);
            calls(c, out)
        }
        Expr::Call(n, args) => {
            out.push(n.clone());
            for a in args {
                calls(a, out)
            }
        }
        Expr::Match(e, arms) => {
            calls(e, out);
            for a in arms {
                calls(&a.body, out)
            }
        }
        Expr::Record(_, fields) => {
            for (_, value) in fields {
                calls(value, out);
            }
        }
        Expr::FieldAssign(_, _, value) => calls(value, out),
        Expr::Assign(_, value) => calls(value, out),
        _ => {}
    }
}

fn type_deps(t: &TypeExpr, names: &HashSet<String>, params: &[String], out: &mut HashSet<String>) {
    if matches!(t.name.as_str(), "ptr" | "const_ptr") {
        return;
    }
    if names.contains(&t.name) && !params.contains(&t.name) {
        out.insert(t.name.clone());
    }
    for a in &t.args {
        type_deps(a, names, params, out)
    }
}

fn reject_recursive_types(program: &Program, type_names: &HashSet<String>) -> Result<(), String> {
    // Without an indirection type, recursive nominal layouts cannot be finite.
    let mut deps: HashMap<String, HashSet<String>> = HashMap::new();
    for decl in &program.types {
        let mut d = HashSet::new();
        for v in &decl.variants {
            for f in &v.fields {
                type_deps(f, type_names, &decl.params, &mut d)
            }
        }
        deps.insert(decl.name.clone(), d);
    }
    // Traversal and witness selection follow declaration order, not hash order.
    for outer in &program.types {
        let k = &outer.name;
        for inner in &program.types {
            let i = &inner.name;
            if deps[i].contains(k) {
                let trans = deps[k].clone();
                deps.get_mut(i).unwrap().extend(trans);
            }
        }
    }
    for decl in &program.types {
        let name = &decl.name;
        if deps[name].contains(name) {
            return Err(format!(
                "E-TYPE: recursive type '{name}' requires indirection"
            ));
        }
    }
    Ok(())
}

fn valid_constructor_name(name: &str) -> bool {
    let source_name = if let Some(rest) = name.strip_prefix("M$") {
        let Some((id, source)) = rest.split_once('$') else {
            return false;
        };
        if id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) {
            return false;
        }
        source
    } else {
        name
    };
    let mut chars = source_name.bytes();
    chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn valid_source_name(name: &str) -> bool {
    let source_name = if let Some(rest) = name.strip_prefix("M$") {
        let Some((id, source)) = rest.split_once('$') else {
            return false;
        };
        if id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) {
            return false;
        }
        source
    } else {
        name
    };
    let mut chars = source_name.bytes();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn valid_foreign_name(name: &str) -> bool {
    const C_KEYWORDS: &[&str] = &[
        "auto",
        "break",
        "case",
        "char",
        "const",
        "continue",
        "default",
        "do",
        "double",
        "else",
        "enum",
        "extern",
        "float",
        "for",
        "goto",
        "if",
        "inline",
        "int",
        "long",
        "register",
        "restrict",
        "return",
        "short",
        "signed",
        "sizeof",
        "static",
        "struct",
        "switch",
        "typedef",
        "union",
        "unsigned",
        "void",
        "volatile",
        "while",
        "_Alignas",
        "_Alignof",
        "_Atomic",
        "_Bool",
        "_Complex",
        "_Generic",
        "_Imaginary",
        "_Noreturn",
        "_Static_assert",
        "_Thread_local",
        // These names are already declared or used by the generated runtime.
        "main",
        "int32_t",
        "int64_t",
        "int8_t",
        "int16_t",
        "uint8_t",
        "uint16_t",
        "uint32_t",
        "uint64_t",
        "intmax_t",
        "uintmax_t",
        "intptr_t",
        "uintptr_t",
        "size_t",
        "printf",
        "puts",
        "fputs",
        "fputc",
        "fwrite",
        "exit",
        "stderr",
        "stdout",
        "FILE",
        "INT32_MIN",
        "INT32_MAX",
        "INT32_C",
        "UINT32_C",
        "RILL_SIMD_NEON",
        "uint8x16_t",
        "vld1q_u8",
        "vandq_u8",
        "vorrq_u8",
        "veorq_u8",
        "vceqq_u8",
        "vst1q_u8",
        "vdupq_n_u8",
        "PRId32",
    ];
    let mut chars = name.chars();
    let valid_start = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    valid_start
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !C_KEYWORDS.contains(&name)
        && !name.starts_with("rl_")
        && !name.starts_with('_')
}
pub(crate) fn extern_type(ty: &ExternType) -> Type {
    match ty {
        ExternType::I32 => Type::Int,
        ExternType::Bool => Type::Bool,
        ExternType::Unit => Type::Unit,
        ExternType::CStr => Type::CStr,
        ExternType::U8 => Type::CU8,
        ExternType::U32 => Type::CU32,
        ExternType::U64 => Type::CU64,
        ExternType::F32 => Type::CF32,
        ExternType::Pointer { pointee, is_const } => Type::CPointer {
            pointee: pointee.clone(),
            is_const: *is_const,
        },
    }
}
pub(crate) fn validate_extern_type(ty: &ExternType) -> Result<(), String> {
    if let ExternType::Pointer { pointee, .. } = ty {
        let mut chars = pointee.chars();
        if !chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err("E-FFI: invalid C pointee name".into());
        }
    }
    Ok(())
}

/// Validate rules belonging to one declaration, before module coalescing.
pub(crate) fn validate_extern_function(foreign: &ExternFunction) -> Result<(), String> {
    if !valid_foreign_name(&foreign.name) {
        return Err(format!(
            "E-FFI: invalid or reserved C function name '{}'",
            foreign.name
        ));
    }
    let mut params = HashSet::new();
    for param in &foreign.params {
        if !params.insert(&param.name) {
            return Err(format!(
                "E-NAME: duplicate parameter '{}' in '{}'",
                param.name, foreign.name
            ));
        }
        if param.ty == ExternType::Unit {
            return Err(format!("E-FFI: parameter '{}' cannot be unit", param.name));
        }
    }
    if foreign.result == ExternType::CStr {
        return Err("E-FFI: cstr foreign results are unsupported".into());
    }
    for ty in foreign
        .params
        .iter()
        .map(|p| &p.ty)
        .chain(std::iter::once(&foreign.result))
    {
        validate_extern_type(ty)?;
    }
    Ok(())
}

pub(crate) fn validate_extern_constant(constant: &ExternConst) -> Result<(), String> {
    validate_extern_type(&constant.ty)?;
    if !valid_foreign_name(&constant.name)
        || matches!(constant.ty, ExternType::Unit | ExternType::CStr)
    {
        return Err(format!(
            "E-FFI: invalid or duplicate foreign constant '{}'",
            constant.name
        ));
    }
    Ok(())
}

/// Check function bodies and declarations before evaluation or code generation.
pub fn check(program: &Program) -> Result<(), String> {
    analyze(program).map(|_| ())
}

/// Analyze a program and retain inferred types for a lowering pass.
pub fn analyze(program: &Program) -> Result<CheckedProgram<'_>, String> {
    alloc::validate(program)?;
    let mut checker = Checker {
        allocation_types: program.allocation_types.clone(),
        solver: solver::Solver::default(),
        level: 0,
        functions: HashMap::new(),
        parameter_modes: HashMap::new(),
        next_local: 0,
        active_loans: Vec::new(),
        constructors: HashMap::new(),
        variants: HashMap::new(),
        pending_matches: Vec::new(),
        equality_types: Vec::new(),
        pointer_types: Vec::new(),
        constants: HashMap::new(),
        expression_types: HashMap::new(),
        field_info: HashMap::new(),
        record_info: HashMap::new(),
        polymorphic_locals: HashMap::new(),
        local_uses: HashMap::new(),
        has_owned: false,
    };
    program.native.validate()?;
    let intrinsic_names: HashSet<&str> = [
        "u8",
        "u32",
        "u64",
        "f32",
        "c_null",
        "is_null",
        "byte_len",
        "byte_at",
        "bytes_new",
        "bytes_len",
        "bytes_get",
        "bytes_set",
        "u8x16_load",
        "u8x16_lane",
        "u8x16_and",
        "u8x16_or",
        "u8x16_xor",
        "u8x16_eq",
        "u8x16_any",
        "u8x16_all",
        "system_box_i32",
        "system_bytes_zeroed",
        "system_box_i32_abort",
        "box_read",
        "box_write",
        "system_bytes_zeroed_abort",
        "owned_bytes_len",
        "owned_bytes_get",
        "owned_bytes_set",
    ]
    .into_iter()
    .collect();
    checker.functions.insert(
        "byte_len".into(),
        FunctionType {
            quantified: vec![],
            params: vec![Type::StaticStr],
            result: Type::Int,
        },
    );
    checker.functions.insert(
        "byte_at".into(),
        FunctionType {
            quantified: vec![],
            params: vec![Type::StaticStr, Type::Int],
            result: Type::Int,
        },
    );
    let mut type_names = HashSet::new();
    for decl in &program.types {
        if matches!(
            decl.name.as_str(),
            "i32"
                | "bool"
                | "unit"
                | "str"
                | "u8x16"
                | "SystemBoxI32"
                | "SystemBytes"
                | "u8"
                | "u32"
                | "u64"
                | "f32"
                | "ptr"
                | "const_ptr"
        ) || !type_names.insert(decl.name.clone())
        {
            return Err(format!(
                "E-NAME: duplicate or reserved type '{}'",
                decl.name
            ));
        }
        if decl.variants.is_empty() {
            return Err(format!("E-TYPE: type '{}' has no constructors", decl.name));
        }
        if let Some(fields) = &decl.record_fields {
            if !valid_source_name(&decl.name) {
                return Err(format!("E-RECORD: invalid record name {:?}", decl.name));
            }
            if decl.variants.len() != 1 || decl.variants[0].fields.len() != fields.len() {
                return Err(format!(
                    "E-RECORD: invalid record declaration '{}'",
                    decl.name
                ));
            }
            let mut names = HashSet::new();
            for name in fields {
                if !valid_source_name(name) || name.starts_with("M$") {
                    return Err(format!("E-RECORD: invalid field name {name:?}"));
                }
                if !names.insert(name) {
                    return Err(format!("E-RECORD: duplicate field '{name}'"));
                }
            }
        }
        let mut params = HashSet::new();
        for p in &decl.params {
            if !params.insert(p) {
                return Err(format!("E-NAME: duplicate type parameter '{p}'"));
            }
        }
    }
    for decl in &program.types {
        let mut vs = Vec::new();
        for v in &decl.variants {
            if intrinsic_names.contains(v.name.as_str()) {
                return Err(format!(
                    "E-NAME: constructor '{}' conflicts with a builtin",
                    v.name
                ));
            }
            if !valid_constructor_name(&v.name) {
                return Err(format!(
                    "E-NAME: constructor '{}' is not a valid constructor name",
                    v.name
                ));
            }
            if checker
                .constructors
                .insert(
                    v.name.clone(),
                    Constructor {
                        owner: decl.name.clone(),
                        params: decl.params.clone(),
                        fields: v.fields.clone(),
                        record_fields: decl.record_fields.clone(),
                    },
                )
                .is_some()
            {
                return Err(format!("E-NAME: duplicate constructor '{}'", v.name));
            }
            vs.push(v.name.clone());
        }
        checker.variants.insert(decl.name.clone(), vs);
    }
    for decl in &program.types {
        let vars = decl
            .params
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), Type::Var(i)))
            .collect();
        for v in &decl.variants {
            for field in &v.fields {
                checker.declared_type(field, &vars)?;
            }
        }
    }
    reject_recursive_types(program, &type_names)?;
    let mut foreign_names = HashSet::new();
    for foreign in &program.externs {
        validate_extern_function(foreign)?;
        if checker.constructors.contains_key(&foreign.name)
            || intrinsic_names.contains(foreign.name.as_str())
            || !foreign_names.insert(foreign.name.clone())
        {
            return Err(format!(
                "E-NAME: duplicate external function '{}'",
                foreign.name
            ));
        }
        let result = extern_type(&foreign.result);
        checker.functions.insert(
            foreign.name.clone(),
            FunctionType {
                quantified: Vec::new(),
                params: foreign.params.iter().map(|p| extern_type(&p.ty)).collect(),
                result,
            },
        );
    }
    for constant in &program.constants {
        validate_extern_constant(constant)?;
        if checker.constructors.contains_key(&constant.name)
            || intrinsic_names.contains(constant.name.as_str())
            || !foreign_names.insert(constant.name.clone())
        {
            return Err(format!(
                "E-FFI: invalid or duplicate foreign constant '{}'",
                constant.name
            ));
        }
        if program.native.includes.is_empty() {
            return Err("E-FFI: foreign constants require @include".into());
        }
        checker
            .constants
            .insert(constant.name.clone(), extern_type(&constant.ty));
    }
    let mut names = HashMap::new();
    for (i, f) in program.functions.iter().enumerate() {
        if checker.constructors.contains_key(&f.name)
            || intrinsic_names.contains(f.name.as_str())
            || foreign_names.contains(&f.name)
            || names.insert(f.name.clone(), i).is_some()
        {
            return Err(format!(
                "E-NAME: duplicate function or constructor '{}'",
                f.name
            ));
        }
        let mut params = HashSet::new();
        for p in &f.params {
            if !params.insert(&p.name) {
                return Err(format!(
                    "E-NAME: duplicate parameter '{}' in '{}'",
                    p.name, f.name
                ));
            }
        }
        checker
            .parameter_modes
            .insert(f.name.clone(), f.params.iter().map(|p| p.mode).collect());
    }
    let main = names.get("main").ok_or("E-NAME: missing main function")?;
    if !program.functions[*main].params.is_empty() {
        return Err("E-ARITY: main must take zero arguments".into());
    }
    let n = program.functions.len();
    let mut edges = vec![Vec::new(); n];
    for (i, f) in program.functions.iter().enumerate() {
        let mut dependencies = Vec::new();
        calls(&f.body, &mut dependencies);
        for name in dependencies {
            if checker.constructors.contains_key(&name)
                || foreign_names.contains(&name)
                || intrinsic_names.contains(name.as_str())
            {
                continue;
            }
            let j = names
                .get(&name)
                .ok_or_else(|| format!("E-NAME: unknown function '{name}'"))?;
            edges[i].push(*j);
        }
    }
    // Retain only completed components' monomorphic free variables. Scanning
    // all earlier generic signatures for each component would be quadratic.
    let mut environment = HashSet::new();
    for group in graph::components(&edges) {
        checker.level += 1;
        for &i in &group {
            let f = &program.functions[i];
            let params = f.params.iter().map(|_| checker.fresh()).collect();
            let result = checker.fresh();
            checker.functions.insert(
                f.name.clone(),
                FunctionType {
                    quantified: Vec::new(),
                    params,
                    result,
                },
            );
        }
        for &i in &group {
            let f = &program.functions[i];
            let signature = checker.copy_function(&checker.functions[&f.name])?;
            for (param, ty) in f.params.iter().zip(&signature.params) {
                if let Some(annotation) = &param.annotation {
                    let declared = checker.declared_type(annotation, &HashMap::new())?;
                    checker.unify(checker.copy_type(ty)?, declared)?;
                }
            }
            let mut locals = HashMap::new();
            for (param, ty) in f.params.iter().zip(signature.params) {
                let local_id = checker.local_id();
                locals.insert(
                    param.name.clone(),
                    Scheme {
                        quantified: Vec::new(),
                        ty,
                        mutable: param.mode == ParamMode::Inout,
                        borrowed: param.mode != ParamMode::Value,
                        local_id,
                    },
                );
            }
            let body = checker.infer(&f.body, &mut locals)?;
            checker.unify(body, signature.result)?;
        }
        checker.level -= 1;
        for &i in &group {
            let name = &program.functions[i].name;
            let mut signature = checker.copy_function(&checker.functions[name])?;
            let mut free = HashSet::new();
            for ty in signature
                .params
                .iter()
                .chain(std::iter::once(&signature.result))
            {
                checker.free_type(ty, &mut free);
            }
            signature.quantified = checker.generalize(free, &environment)?;
            checker.functions.insert(name.clone(), signature);
        }
        for &i in &group {
            let signature = &checker.functions[&program.functions[i].name];
            let mut free = HashSet::new();
            for ty in signature
                .params
                .iter()
                .chain(std::iter::once(&signature.result))
            {
                checker.free_type(ty, &mut free);
            }
            free.retain(|id| signature.quantified.binary_search(id).is_err());
            environment.extend(free);
        }
    }
    for (ty, patterns) in &checker.pending_matches {
        if !checker.exhaustive(ty, patterns)? {
            return Err("E-MATCH: non-exhaustive match".into());
        }
    }
    for ty in &checker.equality_types {
        if !matches!(
            checker.resolve(ty)?,
            Type::Int
                | Type::Bool
                | Type::Unit
                | Type::CU8
                | Type::CU32
                | Type::CU64
                | Type::CF32
                | Type::CPointer { .. }
        ) {
            return Err("E-TYPE: equality requires a concrete primitive type".into());
        }
    }
    for ty in &checker.pointer_types {
        if !matches!(checker.resolve(ty)?, Type::CPointer { .. }) {
            return Err("E-FFI: c_null/is_null requires a concrete raw C pointer context".into());
        }
    }
    let expression_types = checker
        .expression_types
        .iter()
        .map(|(expr, ty)| Ok((*expr, checker.resolve(ty)?)))
        .collect::<Result<_, String>>()?;
    let functions = checker
        .functions
        .iter()
        .map(|(name, signature)| {
            Ok((
                name.clone(),
                FunctionType {
                    quantified: signature.quantified.clone(),
                    params: signature
                        .params
                        .iter()
                        .map(|t| checker.resolve(t))
                        .collect::<Result<_, _>>()?,
                    result: checker.resolve(&signature.result)?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    let mut checked = CheckedProgram {
        program,
        functions,
        expression_types,
        field_info: checker.field_info,
        record_info: checker.record_info,
        polymorphic_locals: checker.polymorphic_locals,
        local_uses: checker.local_uses,
        has_owned: checker.has_owned,
        owned_type_cache: std::sync::Mutex::new(HashMap::new()),
        adt_variant_cache: std::sync::Mutex::new(HashMap::new()),
    };
    // Owner annotations and inactive owner variants still carry affine type
    // obligations, even when this program never executes an allocation.
    fn names_owner(ty: &TypeExpr) -> bool {
        matches!(ty.name.as_str(), "SystemBoxI32" | "SystemBytes")
            || ty.args.iter().any(names_owner)
    }
    checked.has_owned |= program.types.iter().any(|decl| {
        decl.variants
            .iter()
            .any(|variant| variant.fields.iter().any(names_owner))
    }) || program.functions.iter().any(|function| {
        function
            .params
            .iter()
            .any(|param| param.annotation.as_ref().is_some_and(names_owner))
    });
    fn unresolved(ty: &Type) -> bool {
        match ty {
            Type::Var(_) => true,
            Type::Nominal(_, arguments) => arguments.iter().any(unresolved),
            _ => false,
        }
    }
    if !checked.has_owned {
        for ty in checked.expression_types.values() {
            // Ordinary polymorphic Copy programs still use the generic path.
            // Concrete metadata failures must never select that fallback.
            if !unresolved(ty) && checked.type_is_owner(ty)? {
                checked.has_owned = true;
                break;
            }
        }
    }
    if checked.has_owned {
        ownership::verify(&checked)?;
    }
    Ok(checked)
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::ast::Expr;
    use crate::parser::parse;
    fn checked(source: &str) -> Result<(), String> {
        check(&parse(source).unwrap())
    }
    #[test]
    fn inference_and_name_validation() {
        assert!(checked("let id x = x\nlet main = let x = id(1) in id(true)").is_ok());
        assert!(checked("let main = missing")
            .unwrap_err()
            .contains("E-NAME"));
        assert!(checked("let main = if true then 1 else false")
            .unwrap_err()
            .contains("E-TYPE"));
        assert!(checked("let main x = x").unwrap_err().contains("E-ARITY"));
        assert!(
            checked("let f x = if true then f(1) else f(false)\nlet main = ()")
                .unwrap_err()
                .contains("E-TYPE")
        );
    }
    #[test]
    fn generic_constructors_and_patterns() {
        let prefix = "type Option<a> = None | Some(a)\n";
        assert!(checked(&format!(
            "{prefix}let main = match Some(3) {{ Some(x) => x | None => 0 }}"
        ))
        .is_ok());
        assert!(checked(&format!("{prefix}let main = let x = Some(1) in Some(true)")).is_ok());
        assert!(checked(&format!("{prefix}let main = Some(1, 2)"))
            .unwrap_err()
            .contains("E-ARITY"));
        assert!(checked(&format!(
            "{prefix}let main = match Some(3) {{ Some(x) => x }}"
        ))
        .unwrap_err()
        .contains("E-MATCH"));
        assert!(checked(&format!(
            "{prefix}let main = match Some(3) {{ Some(true) => 1 | None => 0 }}"
        ))
        .unwrap_err()
        .contains("E-TYPE"));
    }
    #[test]
    fn nested_pattern_exhaustiveness() {
        let prefix = "type Option<a> = None | Some(a)\n";
        assert!(checked(&format!("{prefix}let main = match Some(true) {{ Some(true) => 1 | Some(false) => 2 | None => 3 }}")).is_ok());
        assert!(checked(&format!(
            "{prefix}let main = match Some(true) {{ Some(true) => 1 | None => 3 }}"
        ))
        .unwrap_err()
        .contains("E-MATCH"));
        assert!(checked(&format!(
            "{prefix}let main = match Some(1) {{ Some(1) => 1 | None => 3 }}"
        ))
        .unwrap_err()
        .contains("E-MATCH"));
    }
    #[test]
    fn rejects_invalid_declarations() {
        assert!(checked("type List = Nil | Cons(List)\nlet main = ()")
            .unwrap_err()
            .contains("indirection"));
        assert!(checked("type X = A(Missing)\nlet main = ()")
            .unwrap_err()
            .contains("unknown type"));
        assert!(checked("type X<a> = A(a)\nlet main = A(A(1))").is_ok());
    }

    #[test]
    fn recursive_declaration_witness_ignores_hash_iteration_order() {
        let names: std::collections::HashSet<String> = ["Alpha", "Beta", "Gamma"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let hash_order: Vec<_> = names.iter().cloned().collect();
        let declaration_order: Vec<_> = hash_order.iter().rev().collect();
        let mut source = String::new();
        for (index, name) in declaration_order.iter().enumerate() {
            let next = declaration_order[(index + 1) % declaration_order.len()];
            source.push_str(&format!("type {name} = {name}({next})\n"));
        }
        source.push_str("fn main() { 42 }");
        let program = parse(&source).unwrap();

        // Use this exact set, without rebuilding it: the old set-based witness
        // always disagrees with the source's first declaration, for every seed.
        assert_ne!(program.types[0].name, hash_order[0]);
        assert_eq!(
            super::reject_recursive_types(&program, &names).unwrap_err(),
            format!(
                "E-TYPE: recursive type '{}' requires indirection",
                program.types[0].name
            )
        );
    }

    #[test]
    fn analysis_retains_types_and_polymorphic_signatures() {
        let program =
            parse("type Option<a> = None | Some(a)\nlet id x = x\nlet main = id(Some(1))").unwrap();
        let checked = super::analyze(&program).unwrap();
        let id = &checked.functions["id"];
        assert_eq!(id.quantified.len(), 1);
        assert_eq!(id.params[0], id.result);
        let body = &program.functions[1].body;
        assert_eq!(
            checked.expression_type(body),
            Some(&super::Type::Nominal(
                "Option".into(),
                vec![super::Type::Int]
            ))
        );
    }

    #[test]
    fn inline_layout_sizes_and_specialization() {
        let program = parse("type Pair = Pair(i32, bool)\ntype Option<a> = None | Some(a)\nlet main = Some(Pair(1, true))").unwrap();
        assert_eq!(super::analyze(&program).unwrap().max_inline_nodes(), Ok(4));
        let program =
            parse("type Option<a> = None | Some(a)\nlet id x = x\nlet main = id(Some(1))").unwrap();
        assert_eq!(super::analyze(&program).unwrap().max_inline_nodes(), Ok(2));
        let program = parse("type Option<a> = None | Some(a)\nlet main = let n = None in if true then Some(1) else n").unwrap();
        assert!(super::analyze(&program)
            .unwrap()
            .max_inline_nodes()
            .unwrap_err()
            .contains("local specialization"));
    }

    #[test]
    fn checks_scalar_foreign_declarations_and_calls() {
        for reserved in [
            "uint32_t",
            "size_t",
            "int32_t",
            "fwrite",
            "UINT32_C",
            "RILL_SIMD_NEON",
            "uint8x16_t",
            "vld1q_u8",
        ] {
            assert!(
                checked(&format!("extern fn {reserved}() -> i32; let main = 0"))
                    .unwrap_err()
                    .contains("E-FFI")
            );
        }
        assert!(checked("extern fn add(x: i32, y: i32) -> i32; let main = add(1, 2)").is_ok());
        assert!(checked("extern fn notify(x: i32) -> unit; let main = notify(3)").is_ok());
        assert!(
            checked("extern fn add(x: i32, y: i32) -> i32; let main = add(1)")
                .unwrap_err()
                .contains("E-ARITY")
        );
        assert!(
            checked("extern fn add(x: i32) -> i32; let main = add(true)")
                .unwrap_err()
                .contains("E-TYPE")
        );
        assert!(checked("extern fn add(x: unit) -> i32; let main = 0")
            .unwrap_err()
            .contains("E-FFI"));
        assert!(
            checked("extern fn add(x: i32, x: i32) -> i32; let main = 0")
                .unwrap_err()
                .contains("E-NAME")
        );
        assert!(
            checked("extern fn add() -> i32; let add = 1\nlet main = add()")
                .unwrap_err()
                .contains("E-NAME")
        );
        assert!(checked("extern fn rl_make() -> i32; let main = 0")
            .unwrap_err()
            .contains("E-FFI"));
        assert!(checked("extern fn return() -> i32; let main = 0")
            .unwrap_err()
            .contains("E-FFI"));
        assert!(checked("extern fn main() -> i32; let main = 0")
            .unwrap_err()
            .contains("E-FFI"));
    }

    #[test]
    fn c_strings_are_direct_foreign_arguments_only() {
        assert!(
            checked("extern fn display(x: cstr) -> i32; let main = display(c\"hello\")").is_ok()
        );
        assert!(
            checked("extern fn display(x: cstr) -> i32; let main = display(1)")
                .unwrap_err()
                .contains("E-FFI")
        );
        assert!(checked("let main = c\"hello\"")
            .unwrap_err()
            .contains("E-FFI"));
        assert!(checked("let id x = x\nlet main = id(c\"hello\")")
            .unwrap_err()
            .contains("E-FFI"));
        assert!(checked("extern fn foreign() -> cstr; let main = 0")
            .unwrap_err()
            .contains("E-FFI"));
        let mut program =
            parse("extern fn display(x: cstr) -> i32; let main = display(c\"safe\")").unwrap();
        program.functions[0].body =
            Expr::Call("display".into(), vec![Expr::CStr("bad\0data".into())]);
        assert!(check(&program).unwrap_err().contains("NUL"));
    }

    #[test]
    fn while_requires_boolean_condition_and_unit_body() {
        assert!(checked("let main = while false do ()").is_ok());
        assert!(checked("let main = while 0 do ()")
            .unwrap_err()
            .contains("E-TYPE"));
        assert!(checked("let main = while false do 1")
            .unwrap_err()
            .contains("E-TYPE"));
        assert!(checked("let main = (while false do ()) + 1")
            .unwrap_err()
            .contains("E-TYPE"));
    }

    #[test]
    fn mutable_locals_are_initialized_monomorphic_and_scoped() {
        assert!(checked("let main = let mut x = 0 in let update = x <- x + 1 in x").is_ok());
        assert!(checked("let main = let x = 0 in x <- 1")
            .unwrap_err()
            .contains("E-MUT"));
        assert!(checked("let main = x <- 1").unwrap_err().contains("E-NAME"));
        assert!(checked("let main = let mut x = 0 in x <- true")
            .unwrap_err()
            .contains("E-TYPE"));
        assert!(checked("let main = let mut x = 0 in let x = 1 in x <- 2")
            .unwrap_err()
            .contains("E-MUT"));
        assert!(checked("let main = let mut x = 0 in (x <- 1) + 2")
            .unwrap_err()
            .contains("E-TYPE"));
        assert!(checked("type Option<a> = None | Some(a)\nlet main = let mut x = None in let y = x in let update = x <- Some(1) in if true then y else Some(true)").unwrap_err().contains("E-TYPE"));
    }

    #[test]
    fn static_strings_have_separate_types_and_checked_builtins() {
        assert!(checked("let main = byte_at(\"é\", 1)").is_ok());
        assert!(checked("type Message = Message(str)\nlet main = Message(\"hello\")").is_ok());
        assert!(checked("let main = byte_len(1)")
            .unwrap_err()
            .contains("E-TYPE"));
        assert!(checked("let main = byte_at(\"a\")")
            .unwrap_err()
            .contains("E-ARITY"));
        assert!(checked("let byte_len x = x\nlet main = 0")
            .unwrap_err()
            .contains("E-NAME"));
        assert!(checked("extern fn byte_len(x: i32) -> i32; let main = 0")
            .unwrap_err()
            .contains("E-NAME"));
        assert!(checked("let main = \"a\" == \"a\"")
            .unwrap_err()
            .contains("E-TYPE"));
        assert!(checked("extern fn f(x: cstr) -> unit; let main = f(\"a\")")
            .unwrap_err()
            .contains("E-FFI"));
    }

    #[test]
    fn byte_buffers_have_literal_capacity_and_value_semantics() {
        assert!(checked("let main = let mut b = bytes_new(4) in let u = b <- bytes_set(b, 0, 255) in bytes_get(b, 0)").is_ok());
        assert!(checked("type Option<a> = None | Some(a)\nlet main = Some(bytes_new(8))").is_ok());
        assert!(checked("let main = bytes_new(257)")
            .unwrap_err()
            .contains("E-BUFFER"));
        assert!(checked("let main = bytes_new(-1)")
            .unwrap_err()
            .contains("E-BUFFER"));
        assert!(checked("let main = let n = 4 in bytes_new(n)")
            .unwrap_err()
            .contains("E-BUFFER"));
        assert!(checked("let main = bytes_get(bytes_new(4), true)")
            .unwrap_err()
            .contains("E-TYPE"));
        assert!(
            checked("let main = let mut b = bytes_new(4) in b <- bytes_new(8)")
                .unwrap_err()
                .contains("E-TYPE")
        );
        assert!(
            checked("let f b = bytes_len(b)\nlet main = f(bytes_new(4))")
                .unwrap_err()
                .contains("E-BUFFER")
        );
    }

    #[test]
    fn byte_vectors_have_explicit_types_and_builtin_signatures() {
        assert!(
            checked("let any x = u8x16_any(x)\nlet main = any(u8x16_load(bytes_new(16), 0))")
                .is_ok()
        );
        assert!(
            checked("type Box = Box(u8x16)\nlet main = Box(u8x16_load(bytes_new(16), 0))").is_ok()
        );
        assert!(checked("let main = u8x16_load(bytes_new(8), 0)").is_ok()); // Bounds are checked at runtime.
        assert!(checked("let main = u8x16_load(1, 0)")
            .unwrap_err()
            .contains("E-VECTOR"));
        assert!(checked("let main = u8x16_load(bytes_new(16), true)")
            .unwrap_err()
            .contains("E-TYPE"));
        assert!(
            checked("let main = u8x16_lane(u8x16_load(bytes_new(16), 0))")
                .unwrap_err()
                .contains("E-ARITY")
        );
        assert!(
            checked("let main = u8x16_and(u8x16_load(bytes_new(16), 0), 3)")
                .unwrap_err()
                .contains("E-TYPE")
        );
        assert!(
            checked("let main = u8x16_load(bytes_new(16), 0) == u8x16_load(bytes_new(16), 0)")
                .unwrap_err()
                .contains("E-TYPE")
        );
        assert!(checked("let u8x16_any x = x\nlet main = 1")
            .unwrap_err()
            .contains("E-NAME"));
        assert!(checked("type u8x16 = Vector\nlet main = 1")
            .unwrap_err()
            .contains("E-NAME"));
    }

    #[test]
    fn record_fields_require_known_types_and_exact_initializers() {
        let prefix = "type Point = { x: i32; y: i32 }\n";
        assert!(checked(&format!(
            "{prefix}let get (p: Point) = p.x\nlet main = get(record Point {{ y = 2; x = 1 }})"
        ))
        .is_ok());
        assert!(checked(&format!("{prefix}let get p = p.x\nlet main = 0"))
            .unwrap_err()
            .contains("E-RECORD"));
        assert!(
            checked(&format!("{prefix}let main = record Point {{ x = 1 }}"))
                .unwrap_err()
                .contains("E-RECORD")
        );
        assert!(checked(&format!(
            "{prefix}let main = record Point {{ x = 1; x = 2 }}"
        ))
        .unwrap_err()
        .contains("E-RECORD"));
        assert!(checked(&format!(
            "{prefix}let main = record Point {{ x = 1; z = 2 }}"
        ))
        .unwrap_err()
        .contains("E-RECORD"));
        assert!(checked(&format!(
            "{prefix}let main = record Point {{ x = true; y = 2 }}"
        ))
        .unwrap_err()
        .contains("E-TYPE"));
        assert!(checked(&format!(
            "{prefix}let main = let p = record Point {{ x = 1; y = 2 }} in p.x <- 3"
        ))
        .unwrap_err()
        .contains("E-MUT"));
        assert!(checked(&format!("{prefix}let main = Point(1, 2)"))
            .unwrap_err()
            .contains("E-RECORD"));
        assert!(checked("let unused (x: Missing) = 1\nlet main = 0")
            .unwrap_err()
            .contains("E-TYPE"));
    }

    #[test]
    fn field_loans_distinguish_siblings_and_overlap_the_root() {
        let prefix = "type Pair = { left: i32; right: i32 }\nlet put (inout a) n = a <- n\nlet pair (inout a) (inout b) = let z = a <- 1 in b <- 2\nlet view (read a) (inout b) = b <- a\n";
        assert!(checked(&format!("{prefix}let main = let mut p = record Pair {{ left = 0; right = 0 }} in pair(inout p.left, inout p.right)")).is_ok());
        assert!(checked(&format!("{prefix}let main = let mut p = record Pair {{ left = 0; right = 0 }} in view(p.left, inout p.right)")).is_ok());
        assert!(checked(&format!("{prefix}let main = let mut p = record Pair {{ left = 0; right = 0 }} in put(inout p.left, (let z = p.right <- 5 in p.right))")).is_ok());
        assert!(checked(&format!("{prefix}let main = let mut p = record Pair {{ left = 0; right = 0 }} in pair(inout p.left, inout p.left)")).unwrap_err().contains("E-LOAN"));
        assert!(checked(&format!("{prefix}let main = let mut p = record Pair {{ left = 0; right = 0 }} in view(p.left, inout p.left)")).unwrap_err().contains("E-LOAN"));
        assert!(checked(&format!("{prefix}let main = let mut p = record Pair {{ left = 0; right = 0 }} in put(inout p.left, (let z = p.left <- 5 in 0))")).unwrap_err().contains("E-LOAN"));
        assert!(checked("type Pair = { left: i32; right: i32 }\nlet overlap (inout a) (read b) = ()\nlet main = let mut p = record Pair { left = 0; right = 0 } in overlap(inout p.left, p)").unwrap_err().contains("E-LOAN"));
        assert!(checked("type Pair = { left: i32; right: i32 }\nlet overlap (read a) (inout b) = ()\nlet main = let mut p = record Pair { left = 0; right = 0 } in overlap(p, inout p.left)").unwrap_err().contains("E-LOAN"));
    }

    #[test]
    fn nested_field_loan_overlap_uses_path_prefixes() {
        let prefix = "type Inner = { x: i32; y: i32 }\ntype Outer = { inner: Inner; z: i32 }\nlet pair (inout a) (inout b) = ()\n";
        let init = "let mut p = record Outer { inner = record Inner { x = 1; y = 2 }; z = 3 } in ";
        for call in [
            "pair(inout p.inner.x, inout p.inner.y)",
            "pair(inout p.inner.x, inout p.z)",
        ] {
            assert!(
                checked(&format!("{prefix}let main = {init}{call}")).is_ok(),
                "{call}"
            );
        }
        for call in [
            "pair(inout p.inner, inout p.inner.x)",
            "pair(inout p.inner.x, inout p.inner)",
            "pair(inout p.inner.x, inout p.inner.x)",
            "pair(inout p, inout p.inner.x)",
        ] {
            assert!(
                checked(&format!("{prefix}let main = {init}{call}"))
                    .unwrap_err()
                    .contains("E-LOAN"),
                "{call}"
            );
        }
        assert!(checked(&format!("{prefix}let use (inout a) b = ()\nlet main = {init}use(inout p.inner.x, (let changed = p.inner.y <- 4 in p.inner.y))")).is_ok());
        assert!(checked(&format!("{prefix}let use (inout a) b = ()\nlet main = {init}use(inout p.inner.x, (let changed = p.inner <- record Inner {{ x = 4; y = 5 }} in 0))")).unwrap_err().contains("E-LOAN"));
        assert!(checked(&format!("{prefix}let main = {init}p.inner.x.y"))
            .unwrap_err()
            .contains("E-RECORD"));
    }

    #[test]
    fn inout_requires_mutable_roots_and_disjoint_later_arguments() {
        let defs = "let bump (inout a) n = a <- a + n\nlet forward (inout a) = bump(inout a, 1)\n";
        assert!(checked(&format!(
            "{defs}let main = let mut x = 0 in forward(inout x)"
        ))
        .is_ok());
        assert!(
            checked(&format!("{defs}let main = let x = 0 in forward(inout x)"))
                .unwrap_err()
                .contains("E-LOAN")
        );
        assert!(checked(&format!("{defs}let main = forward(inout missing)"))
            .unwrap_err()
            .contains("E-NAME"));
        assert!(
            checked(&format!("{defs}let main = let mut x = 0 in forward(x)"))
                .unwrap_err()
                .contains("E-LOAN")
        );
        assert!(checked(&format!(
            "{defs}let main = let mut x = 0 in bump(inout x, x)"
        ))
        .unwrap_err()
        .contains("E-LOAN"));
        assert!(checked(&format!(
            "{defs}let main = let mut x = 0 in bump(inout x, (let ignored = x <- 3 in 1))"
        ))
        .unwrap_err()
        .contains("E-LOAN"));
        assert!(checked(&format!(
            "{defs}let main = let mut x = 0 in bump(inout x, (let x = 1 in x))"
        ))
        .is_ok());
        assert!(checked(
            "let pair (inout a) (inout b) = ()\nlet main = let mut x = 0 in pair(inout x, inout x)"
        )
        .unwrap_err()
        .contains("E-LOAN"));
        assert!(checked(
            "let pair n (inout a) = a <- n\nlet main = let mut x = 0 in pair(x, inout x)"
        )
        .is_ok());
        assert!(
            checked("let read_value x = x\nlet main = let mut x = 0 in read_value(inout x)")
                .unwrap_err()
                .contains("E-LOAN")
        );
        assert!(checked("let read_value x = x\nlet pair (inout a) n = ()\nlet main = let mut x = 0 in pair(inout x, read_value(x))").unwrap_err().contains("E-LOAN"));
        assert!(checked("let inc (inout a) = let u = a <- a + 1 in a\nlet pair (inout a) n = ()\nlet main = let mut x = 0 in pair(inout x, inc(inout x))").unwrap_err().contains("E-LOAN"));
        assert!(checked("let pair (inout a) (inout b) = let u = a <- 1 in b <- 2\nlet main = let mut x = 0 in let mut y = 0 in pair(inout x, inout y)").is_ok());
    }

    #[test]
    fn shared_read_loans_allow_reads_but_reject_overlapping_writes() {
        assert!(checked(
            "let sum (read a) (read b) = a + b\nlet main = let mut x = 4 in sum(x, x)"
        )
        .is_ok());
        assert!(checked("let store value (inout destination) = destination <- value\nlet main = let mut x = 4 in store(x, inout x)").is_ok());
        assert!(checked("let store (read value) (inout destination) = destination <- value\nlet main = let mut x = 4 in store(x, inout x)").unwrap_err().contains("E-LOAN"));
        assert!(checked("let bump (inout x) = x <- x + 1\nlet observe (read x) y = x + y\nlet main = let mut x = 4 in observe(x, (let ignored = bump(inout x) in 0))").unwrap_err().contains("E-LOAN"));
        assert!(checked("let copied (read x) = let mut y = x in let ignored = y <- y + 1 in y\nlet main = let mut x = 4 in copied(x)").is_ok());
        assert!(checked("let observe (read x) = x\nlet main = observe(1)").is_ok());
        assert!(
            checked("let bump (read x) = x <- 1\nlet main = let x = 0 in bump(x)")
                .unwrap_err()
                .contains("E-MUT")
        );
        assert!(checked("let set (inout x) = x <- 1\nlet forward (read x) = set(inout x)\nlet main = let x = 0 in forward(x)").unwrap_err().contains("E-LOAN"));
        assert!(checked("let inner (read x) = x\nlet outer (read x) = inner(x)\nlet main = let x = 4 in outer(x)").is_ok());
        assert!(checked("let pair (read a) b = b\nlet main = let mut x = 0 in pair(x, (let mut x = 1 in let z = x <- 2 in x))").is_ok());
        assert!(checked("let store (read value) (inout dest) = dest <- value\nlet main = let mut x = 0 in store((let changed = x <- 2 in x), inout x)").is_ok());
        assert!(checked("let store (inout dest) (read value) = dest <- value\nlet main = let mut x = 0 in store(inout x, (let changed = x <- 2 in x))").unwrap_err().contains("E-LOAN"));
        assert!(checked("let store (read value) (inout dest) = dest <- value\nlet main = let mut x = 0 in store((x), inout x)").unwrap_err().contains("E-LOAN"));
        assert!(checked("type T = Empty | Filled(i32)\nlet inspect (read t) = match t { Empty => 0 | Filled(x) => x }\nlet main = inspect(Empty)").is_ok());
        assert!(checked("let get (read x) = x\nlet add a b = a + b\nlet main = let mut x = 1 in add(get(x), (let z = x <- 2 in x))").is_ok());
        assert!(checked("let get (read x) = x\nlet add (read a) b = a + b\nlet main = let mut x = 1 in add(x, get(x))").is_ok());
        assert!(checked("let get (read x) = x\nlet add (read a) b = a + b\nlet main = let mut x = 1 in add(x, (let q = get(x) in let z = x <- 2 in q))").unwrap_err().contains("E-LOAN"));
        assert!(checked("let pair (inout a) b = a <- b\nlet main = let mut x = 1 in pair(inout x, (let mut x = 2 in let z = x <- 3 in x))").is_ok());
        assert!(checked("let pair (inout a) b = a <- b\nlet main = let mut x = 1 in pair(inout x, (let mut x = x in x))").unwrap_err().contains("E-LOAN"));
        assert!(checked("let inc (inout n) = n <- n + 1\nlet pair (inout a) b = a <- b\nlet main = let mut x = 1 in pair(inout x, (let mut x = 2 in let z = inc(inout x) in x))").is_ok());
        assert!(checked("type One = One(i32)\nlet pair (inout a) b = a <- b\nlet main = let mut x = 1 in pair(inout x, match One(4) { One(x) => x })").is_ok());
    }
}
