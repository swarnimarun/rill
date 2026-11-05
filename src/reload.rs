//! Synchronous checked-program reload for embedded hosts.
//!
//! A host names monomorphic `i32` hooks and keeps its state as ordinary `i32`
//! arguments/results. Candidates are checked without executing them, then an
//! exclusive `replace` publishes the complete generation between calls. Calls
//! use the ordinary checked interpreter; each starts a fresh Rill invocation.
//! This does not patch native executables, migrate live frames or unload code.

use crate::ast::{Expr, Function, ParamMode, Program};
use crate::check::Type;
use std::collections::BTreeMap;

/// A checked library generation with fixed, explicitly named integer hooks.
///
/// Programs must omit `main`, owners, native dependencies and foreign values.
/// Hook parameters use value mode, with closed `i32` parameter/result types.
/// Helpers may use other supported pure Copy types and ordinary polymorphism.
/// No Rill state, references or active frames survive a call through this API.
pub struct Session {
    program: Program,
    hooks: BTreeMap<String, usize>,
    generation: i32,
}

impl Session {
    /// Check a library and register its hook contract without executing code.
    ///
    /// Hook names must exactly match source function names in `program`;
    /// resolved module names are accepted. The hook list must be nonempty and
    /// contain no duplicates. A generic function is not implicitly specialized
    /// into an integer hook: its inferred signature must already be closed.
    pub fn new(program: Program, hooks: &[&str]) -> Result<Self, String> {
        let hooks = validate(&program, hooks)?;
        Ok(Self {
            program,
            hooks,
            generation: 0,
        })
    }

    /// Check and publish a complete replacement between synchronous calls.
    ///
    /// Every registered hook must keep its parameter modes, arity and integer
    /// types. Helpers can change with the candidate. Any validation error or
    /// generation overflow leaves the previous program and generation intact.
    /// Candidate functions are never invoked during validation.
    pub fn replace(&mut self, candidate: Program) -> Result<i32, String> {
        let names: Vec<_> = self.hooks.keys().map(String::as_str).collect();
        let hooks = validate(&candidate, &names)?;
        if hooks != self.hooks {
            return Err("E-RELOAD: replacement changes a registered hook's arity".into());
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("E-RELOAD: generation counter exhausted")?;
        self.program = candidate;
        self.generation = generation;
        Ok(generation)
    }

    /// The accepted generation, starting at zero and advancing on replacement.
    pub fn generation(&self) -> i32 {
        self.generation
    }

    /// Invoke one registered hook with caller-owned integer state.
    ///
    /// Each invocation clones the library, adds a synthetic entry point and
    /// follows the ordinary interpreter checking/lowering path. Runtime traps
    /// are returned as errors; they do not replace the accepted generation.
    /// Rechecking makes this an iteration API, not a low-overhead native call.
    pub fn call_i32(&self, name: &str, args: &[i32]) -> Result<i32, String> {
        let arity = self
            .hooks
            .get(name)
            .ok_or_else(|| format!("E-RELOAD: unregistered hook '{name}'"))?;
        if args.len() != *arity {
            return Err(format!(
                "E-RELOAD: hook '{name}' expects {arity} arguments, got {}",
                args.len()
            ));
        }
        let invocation = invocation(&self.program, name, args);
        match crate::interpreter::run(&invocation)? {
            crate::interpreter::Value::Int(value) => Ok(value),
            _ => Err("E-RELOAD: checked integer hook returned a non-integer value".into()),
        }
    }
}

fn validate(program: &Program, names: &[&str]) -> Result<BTreeMap<String, usize>, String> {
    if names.is_empty() {
        return Err("E-RELOAD: at least one hook is required".into());
    }
    if program
        .functions
        .iter()
        .any(|function| function.name == "main")
    {
        return Err("E-RELOAD: a reload library must not define main".into());
    }
    if program.native != crate::c_import::Config::default()
        || !program.constants.is_empty()
        || !program.externs.is_empty()
    {
        return Err(
            "E-RELOAD: native dependencies and foreign declarations are unsupported".into(),
        );
    }
    let checked = crate::check::analyze_library(program)?;
    if checked.has_owned() || checked.has_c_values() {
        return Err("E-RELOAD: owners and foreign values are unsupported".into());
    }
    let mut hooks = BTreeMap::new();
    for &name in names {
        if hooks.contains_key(name) {
            return Err(format!("E-RELOAD: duplicate hook '{name}'"));
        }
        let function = program
            .functions
            .iter()
            .find(|function| function.name == name)
            .ok_or_else(|| format!("E-RELOAD: missing source hook '{name}'"))?;
        let signature = &checked.functions[name];
        if !signature.quantified.is_empty()
            || signature.params.iter().any(|ty| *ty != Type::Int)
            || signature.result != Type::Int
            || function
                .params
                .iter()
                .any(|parameter| parameter.mode != ParamMode::Value)
        {
            return Err(format!(
                "E-RELOAD: hook '{name}' requires closed value i32 parameters and an i32 result"
            ));
        }
        // Emission proves this reachable hook can use the ordinary checked
        // backend, including callable conversion and representation checks.
        // Integer zero operands only establish a call shape; no code executes.
        crate::backend::emit(&invocation(program, name, &vec![0; function.params.len()]))?;
        hooks.insert(name.to_owned(), function.params.len());
    }
    Ok(hooks)
}

fn invocation(program: &Program, name: &str, args: &[i32]) -> Program {
    let mut invocation = program.clone();
    invocation.functions.push(Function {
        name: "main".into(),
        params: Vec::new(),
        result_annotation: None,
        body: Expr::Call(name.into(), args.iter().copied().map(Expr::Int).collect()),
    });
    invocation
}

#[cfg(test)]
mod tests {
    use super::Session;

    #[test]
    fn generation_exhaustion_keeps_the_previous_program() {
        let program = crate::parser::parse("fn update(x) { x + 1 }").unwrap();
        let mut session = Session::new(program, &["update"]).unwrap();
        session.generation = i32::MAX;
        let candidate = crate::parser::parse("fn update(x) { x + 2 }").unwrap();
        assert!(session
            .replace(candidate)
            .unwrap_err()
            .contains("exhausted"));
        assert_eq!(session.generation(), i32::MAX);
        assert_eq!(session.call_i32("update", &[41]).unwrap(), 42);
    }
}
