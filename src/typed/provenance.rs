//! Optional, snapshot-owned function-instance origins, not a complete typed or
//! generated-C source map. No expression, storage, capture or cleanup coverage
//! is implied. All ordinary lowering and verification still run unchanged.
use super::{instances, TypedMain};
use crate::{
    ast::{Expr, ParamMode},
    check::{self, CheckedProgram, Type},
    modules::{
        provenance::{Contribution, Lowering, ResolvedNode, ResolvedSymbol, SourceProgram},
        resolve::{Kind, SymbolId},
    },
    source_map::{AstNode, AstPath},
    syntax::AstDeclaration,
};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Clone)]
struct Domain(Arc<()>);
impl PartialEq for Domain {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Domain {}
impl Hash for Domain {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}
impl fmt::Debug for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("snapshot")
    }
}
macro_rules! handle {
    ($name:ident) => {
        /// Opaque address in one immutable paired result. Not a persistent or
        /// cross-compilation identifier; foreign-domain lookup returns `None`.
        #[derive(Clone, PartialEq, Eq, Hash)]
        pub struct $name {
            domain: Domain,
            slot: usize,
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple(stringify!($name)).field(&self.slot).finish()
            }
        }
    };
}
handle!(DefinitionId);
handle!(FunctionInstanceId);
handle!(SourceNodeId);

/// Closed source types. Nominals reference actual resolver declarations, not
/// lowered spellings, generated C names, inference variables or layout slots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConcreteType {
    Int,
    Bool,
    Unit,
    StaticStr,
    U8x16,
    Bytes(usize),
    OwnedI32,
    OwnedBytes,
    Nominal {
        definition: DefinitionId,
        arguments: Vec<ConcreteType>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionInstance {
    pub id: FunctionInstanceId,
    pub definition: DefinitionId,
    pub parameters: Vec<(ParamMode, ConcreteType)>,
    pub result: ConcreteType,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestOrigin {
    /// The entry request has no fabricated caller or source call span.
    Entry,
    Call {
        caller: FunctionInstanceId,
        site: SourceNodeId,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceRequest {
    pub callee: FunctionInstanceId,
    pub origin: RequestOrigin,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FunctionDisposition {
    NotInstantiated,
    Instantiated(Vec<FunctionInstanceId>),
}

/// Immutable ownership of the exact input snapshots, typed result and index.
/// A whole clone keeps its domain; a new lowering always creates a new domain.
/// No API attaches this index to an edited AST or typed clone. `into_typed`
/// deliberately discards it. Moving or querying this result needs no file I/O.
#[derive(Clone, Debug)]
pub struct FunctionOriginProgram {
    sources: SourceProgram,
    typed: TypedMain,
    domain: Domain,
    instances: Vec<FunctionInstance>,
    requests: Vec<InstanceRequest>,
    functions: Vec<usize>,
    dispositions: HashMap<usize, Vec<FunctionInstanceId>>,
}
impl FunctionOriginProgram {
    pub fn sources(&self) -> &SourceProgram {
        &self.sources
    }
    pub fn typed(&self) -> &TypedMain {
        &self.typed
    }
    pub fn into_typed(self) -> TypedMain {
        self.typed
    }
    /// Final typed function order, after the existing specialization permutation.
    pub fn instances(&self) -> &[FunctionInstance] {
        &self.instances
    }
    /// Entry first, then worklist discovery order. Repeated and recursive
    /// requests are edges, not an expanded runtime stack trace.
    pub fn requests(&self) -> &[InstanceRequest] {
        &self.requests
    }
    pub fn entry(&self) -> FunctionInstanceId {
        self.instance_id(self.typed.entry)
    }
    pub fn instance(&self, id: &FunctionInstanceId) -> Option<&FunctionInstance> {
        (id.domain == self.domain)
            .then(|| self.instances.get(id.slot))
            .flatten()
    }
    pub fn definition(&self, id: &DefinitionId) -> Option<&ResolvedSymbol> {
        (id.domain == self.domain)
            .then(|| self.sources.symbols().get(id.slot))
            .flatten()
    }
    pub fn source_node(&self, id: &SourceNodeId) -> Option<&ResolvedNode> {
        (id.domain == self.domain)
            .then(|| self.sources.nodes().get(id.slot))
            .flatten()
    }
    pub fn function_definitions(&self) -> impl Iterator<Item = DefinitionId> + '_ {
        self.functions.iter().map(|&slot| DefinitionId {
            domain: self.domain.clone(),
            slot,
        })
    }
    pub fn function_disposition(&self, id: &DefinitionId) -> Option<FunctionDisposition> {
        self.definition(id)?;
        self.dispositions.get(&id.slot).map(|instances| {
            if instances.is_empty() {
                FunctionDisposition::NotInstantiated
            } else {
                FunctionDisposition::Instantiated(instances.clone())
            }
        })
    }
    fn instance_id(&self, slot: usize) -> FunctionInstanceId {
        FunctionInstanceId {
            domain: self.domain.clone(),
            slot,
        }
    }
}

fn fail(message: impl fmt::Display) -> String {
    format!("E-TYPED-SOURCE-MAP: {message}")
}
fn charge(trace: &mut instances::Trace, amount: usize) -> Result<(), String> {
    trace.charge(amount);
    trace.check()
}

/// Lower the existing typed subset and index its reachable Rill function
/// instances. Does not map native/fallback output, compiler-generated C helpers,
/// expressions, captures, storage or generated C bytes. Missing provenance is a
/// map error, never a guessed or inherited location.
///
/// Ordinary semantic/lowering/verification errors take precedence over optional
/// metadata limits or corruption. Existing APIs and backend fallback are intact.
pub fn lower_main_with_function_origins(
    sources: SourceProgram,
) -> Result<FunctionOriginProgram, String> {
    lower_with_trace(sources, instances::Trace::default())
}
// Native values can be present only in an inactive variant or phantom
// argument, without a raw expression type in the source checker. Inspect the
// complete verified representation before publishing this narrower index.
fn has_native_representation(typed: &TypedMain) -> bool {
    typed.native.has_foreign()
        || !typed.native.pointers.is_empty()
        || typed.adts.iter().any(|adt| {
            adt.arguments
                .iter()
                .chain(adt.variants.iter().flat_map(|variant| &variant.fields))
                .any(|ty| ty.is_raw())
        })
        || typed.functions.iter().any(|function| {
            function.result.is_raw()
                || function.params.iter().any(|(_, ty, _)| ty.is_raw())
                || super::any_expr(&function.body, &|kind| {
                    matches!(
                        kind,
                        super::ExprKind::ForeignCall(..)
                            | super::ExprKind::ForeignConst(..)
                            | super::ExprKind::ForeignConvert(..)
                            | super::ExprKind::NullPointer
                            | super::ExprKind::IsNull(..)
                    )
                })
        })
}

fn lower_with_trace(
    sources: SourceProgram,
    mut trace: instances::Trace,
) -> Result<FunctionOriginProgram, String> {
    let checked = check::analyze(sources.program())?;
    let (typed, instances) = super::lower_main_traced(&checked, Some(&mut trace))?;
    // Keep ordinary semantic errors first, including existing native rejection.
    // If ordinary lowering later supports native nodes, this narrower API still
    // reports its explicit mapping boundary, never a partial successful index.
    if !sources.program().externs.is_empty()
        || !sources.program().constants.is_empty()
        || !sources.program().native.includes.is_empty()
        || checked.has_c_values()
        || has_native_representation(&typed)
    {
        return Err(
            "E-TYPED-UNSUPPORTED: function origin index does not map native programs".into(),
        );
    }
    trace.check()?;
    let bridge = Bridge::new(&sources, &mut trace)?;
    validate(&sources, &checked, &typed, &instances, &bridge, &mut trace)?;
    let domain = Domain(Arc::new(()));
    let definition_id = |slot| DefinitionId {
        domain: domain.clone(),
        slot,
    };
    let instance_id = |slot| FunctionInstanceId {
        domain: domain.clone(),
        slot,
    };
    let mut records = Vec::new();
    let mut dispositions = bridge
        .functions
        .iter()
        .map(|&slot| (slot, Vec::new()))
        .collect::<HashMap<_, _>>();
    for (slot, instance) in instances.list.iter().enumerate() {
        charge(&mut trace, 1)?;
        let definition = bridge.functions[instance.source];
        let parameters = sources.program().functions[instance.source]
            .params
            .iter()
            .zip(&instance.params)
            .map(|(param, ty)| Ok((param.mode, concrete(ty, &bridge, &domain, &mut trace)?)))
            .collect::<Result<_, String>>()?;
        records.push(FunctionInstance {
            id: instance_id(slot),
            definition: definition_id(definition),
            parameters,
            result: concrete(&instance.result, &bridge, &domain, &mut trace)?,
        });
        dispositions
            .get_mut(&definition)
            .ok_or_else(|| fail("missing function disposition"))?
            .push(instance_id(slot));
    }
    let requests = trace
        .requests
        .iter()
        .map(|request| {
            let origin = match (&request.caller, &request.path) {
                (None, None) => RequestOrigin::Entry,
                (Some(caller), Some(path)) => RequestOrigin::Call {
                    caller: instance_id(*caller),
                    site: SourceNodeId {
                        domain: domain.clone(),
                        slot: bridge.nodes[path],
                    },
                },
                _ => unreachable!("validated request"),
            };
            InstanceRequest {
                callee: instance_id(request.callee),
                origin,
            }
        })
        .collect();
    let Bridge {
        functions,
        nominal,
        symbols,
        nodes,
        calls,
    } = bridge;
    drop((nominal, symbols, nodes, calls));
    drop(checked);
    Ok(FunctionOriginProgram {
        sources,
        typed,
        domain,
        instances: records,
        requests,
        functions,
        dispositions,
    })
}

/// Bridge only through already recorded resolver targets. The type-name map is
/// the exact checked nominal key at that target, not a name/source-text guess.
struct Bridge<'a> {
    functions: Vec<usize>,
    nominal: HashMap<&'a str, usize>,
    symbols: HashMap<SymbolId, usize>,
    nodes: HashMap<&'a AstPath, usize>,
    calls: Vec<HashSet<usize>>,
}
impl<'a> Bridge<'a> {
    fn new(sources: &'a SourceProgram, trace: &mut instances::Trace) -> Result<Self, String> {
        charge(trace, sources.program().functions.len())?;
        let function_names = sources
            .program()
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .collect::<HashSet<_>>();
        let mut result = Self {
            functions: vec![usize::MAX; sources.program().functions.len()],
            nominal: HashMap::new(),
            symbols: HashMap::new(),
            nodes: HashMap::new(),
            calls: vec![HashSet::new(); sources.program().functions.len()],
        };
        for (slot, symbol) in sources.symbols().iter().enumerate() {
            charge(trace, 1 + symbol.target.steps.len())?;
            if result.symbols.insert(symbol.symbol.id, slot).is_some() {
                return Err(fail("duplicate source symbol"));
            }
            if symbol.target.steps.is_empty() {
                match (
                    symbol.target.declaration,
                    symbol.symbol.kind,
                    sources.node(&symbol.target),
                ) {
                    (
                        AstDeclaration::Function(index),
                        Kind::Function,
                        Some(AstNode::Function(_)),
                    ) => {
                        if result.functions[index] != usize::MAX {
                            return Err(fail("duplicate function origin"));
                        }
                        result.functions[index] = slot;
                    }
                    (AstDeclaration::Type(_), Kind::Type, Some(AstNode::TypeDeclaration(decl)))
                        if result.nominal.insert(&decl.name, slot).is_some() =>
                    {
                        return Err(fail("duplicate nominal origin"));
                    }
                    _ => {}
                }
            }
        }
        if result.functions.contains(&usize::MAX) {
            return Err(fail(
                "function has no source declaration; compiler-generated functions are not mapped",
            ));
        }
        for (slot, node) in sources.nodes().iter().enumerate() {
            charge(trace, 1 + node.path.steps.len())?;
            if result.nodes.insert(&node.path, slot).is_some() {
                return Err(fail("duplicate source node"));
            }
            let Some(AstNode::Expression(Expr::Call(name, _))) = sources.node(&node.path) else {
                continue;
            };
            let AstDeclaration::Function(caller) = node.path.declaration else {
                return Err(fail("call outside function"));
            };
            let origin = node
                .origins
                .first()
                .ok_or_else(|| fail("call has no source origin"))?;
            if origin.contribution != Contribution::Retained {
                return Err(fail("call has no retained origin"));
            }
            if let Lowering::ResolvedName(id) = origin.lowering {
                let symbol = sources
                    .symbol(id)
                    .ok_or_else(|| fail("call has missing resolver identity"))?;
                if symbol.symbol.kind == Kind::Function {
                    let Some(AstNode::Function(target)) = sources.node(&symbol.target) else {
                        return Err(fail("native or generated call is not mapped"));
                    };
                    if name != &target.name {
                        return Err(fail("call disagrees with resolved declaration"));
                    }
                    result.calls[caller].insert(slot);
                }
            } else if function_names.contains(name.as_str()) {
                return Err(fail("function call has no resolved source identity"));
            }
        }
        Ok(result)
    }
}
fn concrete(
    ty: &Type,
    bridge: &Bridge<'_>,
    domain: &Domain,
    trace: &mut instances::Trace,
) -> Result<ConcreteType, String> {
    charge(trace, 1)?;
    Ok(match ty {
        Type::Int => ConcreteType::Int,
        Type::Bool => ConcreteType::Bool,
        Type::Unit => ConcreteType::Unit,
        Type::StaticStr => ConcreteType::StaticStr,
        Type::U8x16 => ConcreteType::U8x16,
        Type::Bytes(n) => ConcreteType::Bytes(*n),
        Type::SystemBoxI32 => ConcreteType::OwnedI32,
        Type::SystemBytes => ConcreteType::OwnedBytes,
        Type::Nominal(name, arguments) => ConcreteType::Nominal {
            definition: DefinitionId {
                domain: domain.clone(),
                slot: *bridge
                    .nominal
                    .get(name.as_str())
                    .ok_or_else(|| fail("nominal has no source declaration"))?,
            },
            arguments: arguments
                .iter()
                .map(|ty| concrete(ty, bridge, domain, trace))
                .collect::<Result<_, _>>()?,
        },
        _ => return Err(fail("signature is not a closed supported Rill type")),
    })
}
fn signature_matches(
    ty: &Type,
    actual: super::Type,
    typed: &TypedMain,
    trace: &mut instances::Trace,
) -> Result<bool, String> {
    charge(trace, 1)?;
    if let Type::Nominal(name, arguments) = ty {
        let super::Type::Adt { id, owned } = actual else {
            return Ok(false);
        };
        let Some(layout) = typed.adts.get(id) else {
            return Ok(false);
        };
        if layout.origin != super::AdtOrigin::Source
            || &layout.name != name
            || layout.owned != owned
            || layout.arguments.len() != arguments.len()
        {
            return Ok(false);
        }
        for (ty, actual) in arguments.iter().zip(&layout.arguments) {
            if !signature_matches(ty, *actual, typed, trace)? {
                return Ok(false);
            }
        }
        Ok(true)
    } else {
        Ok(super::primitive_type(ty).is_ok_and(|expected| expected == actual))
    }
}

fn validate(
    sources: &SourceProgram,
    checked: &CheckedProgram<'_>,
    typed: &TypedMain,
    instances: &instances::Instances,
    bridge: &Bridge<'_>,
    trace: &mut instances::Trace,
) -> Result<(), String> {
    if instances.list.len() != typed.functions.len() || instances.entry != typed.entry {
        return Err(fail("typed function table differs from instances"));
    }
    charge(trace, 1)?;
    let entry = instances
        .list
        .get(instances.entry)
        .ok_or_else(|| fail("invalid entry instance"))?;
    if !sources
        .program()
        .functions
        .get(entry.source)
        .is_some_and(|function| function.name == "main")
    {
        return Err(fail("entry instance does not identify main"));
    }
    if bridge.functions.len() != sources.program().functions.len()
        || bridge.calls.len() != sources.program().functions.len()
    {
        return Err(fail("incomplete source function index"));
    }
    for (source, &slot) in bridge.functions.iter().enumerate() {
        charge(trace, 1)?;
        let symbol = sources
            .symbols()
            .get(slot)
            .ok_or_else(|| fail("invalid function definition slot"))?;
        if symbol.symbol.kind != Kind::Function
            || symbol.target != AstPath::root(AstDeclaration::Function(source))
        {
            return Err(fail(
                "function source identity differs from declaration slot",
            ));
        }
    }
    for (name, &slot) in &bridge.nominal {
        charge(trace, 1)?;
        let symbol = sources
            .symbols()
            .get(slot)
            .ok_or_else(|| fail("invalid nominal definition slot"))?;
        if symbol.symbol.kind != Kind::Type
            || !matches!(sources.node(&symbol.target), Some(AstNode::TypeDeclaration(decl)) if decl.name == *name)
        {
            return Err(fail("nominal identity differs from declaration slot"));
        }
    }
    let mut expected = 1usize;
    for (instance, function) in instances.list.iter().zip(&typed.functions) {
        charge(trace, 1)?;
        let source = sources
            .program()
            .functions
            .get(instance.source)
            .ok_or_else(|| fail("invalid instance source declaration"))?;
        expected = expected
            .checked_add(bridge.calls[instance.source].len())
            .ok_or_else(|| fail("request count overflow"))?;
        let params = &source.params;
        if params.len() != function.params.len()
            || instance.params.len() != function.params.len()
            || !signature_matches(&instance.result, function.result, typed, trace)?
        {
            return Err(fail(
                "typed instance signature differs from source specialization",
            ));
        }
        for ((source, ty), (_, actual, mode)) in
            params.iter().zip(&instance.params).zip(&function.params)
        {
            if source.mode != *mode || !signature_matches(ty, *actual, typed, trace)? {
                return Err(fail("typed parameter differs from source specialization"));
            }
        }
    }
    if trace.requests.len() != expected {
        return Err(fail("missing or extra function request"));
    }
    let mut seen = HashSet::new();
    // Take the bounded records temporarily so validation can charge one shared
    // budget without copying paths, signatures or the request graph.
    let requests = std::mem::take(&mut trace.requests);
    let result = (|| {
        for (ordinal, request) in requests.iter().enumerate() {
            charge(trace, 1)?;
            if request.callee >= instances.list.len() {
                return Err(fail("invalid callee instance"));
            }
            match (request.caller, &request.path) {
                (None, None) if ordinal == 0 && request.callee == instances.entry => {}
                (Some(caller), Some(path)) => {
                    charge(trace, path.steps.len())?;
                    let caller = instances
                        .list
                        .get(caller)
                        .ok_or_else(|| fail("invalid caller instance"))?;
                    let node_slot = *bridge
                        .nodes
                        .get(path)
                        .ok_or_else(|| fail("request has no source node"))?;
                    if path.declaration != AstDeclaration::Function(caller.source)
                        || !bridge.calls[caller.source].contains(&node_slot)
                        || !seen.insert((request.caller, node_slot))
                    {
                        return Err(fail("duplicate or mismatched call origin"));
                    }
                    let Some(AstNode::Expression(expr @ Expr::Call(name, arguments))) =
                        sources.node(path)
                    else {
                        return Err(fail("request is not an actual call"));
                    };
                    let params = arguments
                        .iter()
                        .map(|arg| checked_type(checked, arg, &caller.substitution, trace))
                        .collect::<Result<_, _>>()?;
                    let result = checked_type(checked, expr, &caller.substitution, trace)?;
                    if instances.call(name, params, result).map_err(fail)? != request.callee {
                        return Err(fail("call targets the wrong complete function instance"));
                    }
                    let Lowering::ResolvedName(symbol) =
                        sources.nodes()[node_slot].origins[0].lowering
                    else {
                        return Err(fail("call lacks resolved identity"));
                    };
                    if bridge.symbols.get(&symbol).copied()
                        != Some(bridge.functions[instances.list[request.callee].source])
                    {
                        return Err(fail("callee declaration identity differs"));
                    }
                }
                _ => return Err(fail("invalid entry or call request")),
            }
        }
        Ok(())
    })();
    trace.requests = requests;
    result
}
fn checked_type(
    checked: &CheckedProgram<'_>,
    expr: &Expr,
    context: &instances::Substitution,
    trace: &mut instances::Trace,
) -> Result<Type, String> {
    let ty = checked
        .expression_type(expr)
        .ok_or_else(|| fail("missing checked call type"))?;
    // Charge the expanded substitution before allocating it during validation.
    charge(trace, 1)?;
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            Type::Var(id) => {
                charge(trace, 1)?;
                pending.push(context.get(id).ok_or_else(|| fail("unclosed call type"))?)
            }
            Type::Nominal(_, args) => {
                charge(trace, args.len())?;
                pending.extend(args);
            }
            _ => {}
        }
    }
    instances::expression_type(checked, expr, context).map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    const PHANTOM: &str = "type Phantom<a> = Marker\nfn make(x) { Marker }\nfn one(x: Phantom<i32>) { 40 }\nfn two(x: Phantom<bool>) { 2 }\nfn main() { one(make(0)) + two(make(0)) }";
    fn source(text: &str) -> SourceProgram {
        let path = std::env::temp_dir().join(format!(
            "rill-origin-private-{}-{}.rill",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, text).unwrap();
        let source = crate::modules::compile::load_program_with_sources(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        source
    }
    fn corrupt(
        mut change: impl FnMut(&mut instances::Trace, &instances::Instances, &SourceProgram),
    ) {
        let sources = source(PHANTOM);
        let checked = check::analyze(sources.program()).unwrap();
        let mut trace = instances::Trace::default();
        let (typed, instances) =
            super::super::lower_main_traced(&checked, Some(&mut trace)).unwrap();
        let bridge = Bridge::new(&sources, &mut trace).unwrap();
        validate(&sources, &checked, &typed, &instances, &bridge, &mut trace).unwrap();
        change(&mut trace, &instances, &sources);
        let error =
            validate(&sources, &checked, &typed, &instances, &bridge, &mut trace).unwrap_err();
        assert!(error.starts_with("E-TYPED-SOURCE-MAP:"), "{error}");
        assert_eq!(super::super::evaluate(&typed).unwrap().to_string(), "42");
        assert!(super::super::emit_c(&typed).is_ok());
    }
    #[test]
    fn malformed_request_graph_rejects_missing_duplicate_wrong_path_and_ids() {
        corrupt(|trace, _, _| {
            trace.requests.pop();
        });
        corrupt(|trace, _, _| {
            trace.requests[1] = trace.requests[2].clone();
        });
        corrupt(|trace, _, _| {
            trace.requests[1].callee = usize::MAX;
        });
        corrupt(|trace, _, _| {
            trace.requests[1].caller = Some(usize::MAX);
        });
        corrupt(|trace, _, _| {
            trace.requests[1].path.as_mut().unwrap().steps.clear();
        });
        corrupt(|trace, _, _| {
            trace.requests[1].path.as_mut().unwrap().declaration =
                AstDeclaration::Function(usize::MAX);
        });
        corrupt(|trace, instances, _| {
            trace.requests[0].callee = (instances.entry + 1) % instances.list.len();
        });
        // Reintroducing the discovery-time entry ID after sorting must fail.
        corrupt(|trace, _, _| {
            trace.requests[0].callee = 0;
        });
    }
    #[test]
    fn same_parameter_wrong_result_instance_is_not_a_valid_origin() {
        corrupt(|trace, instances, sources| {
            let make = sources
                .symbols()
                .iter()
                .find(|s| s.symbol.name == "make")
                .unwrap();
            let AstDeclaration::Function(source) = make.target.declaration else {
                panic!()
            };
            let ids = instances
                .list
                .iter()
                .enumerate()
                .filter(|(_, i)| i.source == source)
                .map(|(id, _)| id)
                .collect::<Vec<_>>();
            assert_eq!(ids.len(), 2);
            assert_eq!(instances.list[ids[0]].params, instances.list[ids[1]].params);
            assert_ne!(instances.list[ids[0]].result, instances.list[ids[1]].result);
            let request = trace
                .requests
                .iter_mut()
                .find(|r| r.callee == ids[0])
                .unwrap();
            request.callee = ids[1];
        });
    }
    #[test]
    fn malformed_definition_bridge_and_typed_signature_are_rejected() {
        let sources = source(PHANTOM);
        let checked = check::analyze(sources.program()).unwrap();
        let mut trace = instances::Trace::default();
        let (mut typed, instances) =
            super::super::lower_main_traced(&checked, Some(&mut trace)).unwrap();
        let mut bridge = Bridge::new(&sources, &mut trace).unwrap();
        bridge.functions.swap(0, 1);
        assert!(
            validate(&sources, &checked, &typed, &instances, &bridge, &mut trace)
                .unwrap_err()
                .starts_with("E-TYPED-SOURCE-MAP:")
        );
        bridge.functions.swap(0, 1);
        let symbol = sources.symbols()[bridge.functions[0]].symbol.id;
        let slot = bridge.symbols.remove(&symbol).unwrap();
        assert!(
            validate(&sources, &checked, &typed, &instances, &bridge, &mut trace)
                .unwrap_err()
                .starts_with("E-TYPED-SOURCE-MAP:")
        );
        bridge.symbols.insert(symbol, slot);
        let saved = bridge.functions.pop().unwrap();
        assert!(
            validate(&sources, &checked, &typed, &instances, &bridge, &mut trace)
                .unwrap_err()
                .starts_with("E-TYPED-SOURCE-MAP:")
        );
        bridge.functions.push(saved);
        typed.functions[0].result = super::super::Type::Bool;
        assert!(
            validate(&sources, &checked, &typed, &instances, &bridge, &mut trace)
                .unwrap_err()
                .starts_with("E-TYPED-SOURCE-MAP:")
        );
    }
    #[test]
    fn entry_unused_and_nominal_definition_swaps_are_rejected() {
        let sources = source("type A = A type B = B fn unused(x: B) { x } fn main() { A }");
        let checked = check::analyze(sources.program()).unwrap();
        let mut trace = instances::Trace::default();
        let (typed, instances) =
            super::super::lower_main_traced(&checked, Some(&mut trace)).unwrap();
        assert_eq!(trace.requests.len(), 1);
        let mut bridge = Bridge::new(&sources, &mut trace).unwrap();
        bridge.functions.swap(0, 1);
        assert!(
            validate(&sources, &checked, &typed, &instances, &bridge, &mut trace)
                .unwrap_err()
                .starts_with("E-TYPED-SOURCE-MAP:")
        );
        bridge.functions.swap(0, 1);
        let mut names = bridge.nominal.keys().copied().collect::<Vec<_>>();
        names.sort();
        let a = bridge.nominal[names[0]];
        let b = bridge.nominal[names[1]];
        bridge.nominal.insert(names[0], b);
        bridge.nominal.insert(names[1], a);
        assert!(
            validate(&sources, &checked, &typed, &instances, &bridge, &mut trace)
                .unwrap_err()
                .starts_with("E-TYPED-SOURCE-MAP:")
        );
    }

    #[test]
    fn entry_cannot_claim_a_same_signature_unused_definition() {
        let sources = source("fn unused() { 7 } fn main() { 42 }");
        let checked = check::analyze(sources.program()).unwrap();
        let mut trace = instances::Trace::default();
        let (typed, mut instances) =
            super::super::lower_main_traced(&checked, Some(&mut trace)).unwrap();
        let bridge = Bridge::new(&sources, &mut trace).unwrap();
        validate(&sources, &checked, &typed, &instances, &bridge, &mut trace).unwrap();
        let unused = sources
            .symbols()
            .iter()
            .find(|symbol| symbol.symbol.name == "unused")
            .unwrap();
        let AstDeclaration::Function(unused) = unused.target.declaration else {
            panic!("unused must be a function declaration");
        };
        instances.list[instances.entry].source = unused;
        let error =
            validate(&sources, &checked, &typed, &instances, &bridge, &mut trace).unwrap_err();
        assert_eq!(
            error,
            "E-TYPED-SOURCE-MAP: entry instance does not identify main"
        );
        instances.list[instances.entry].source = usize::MAX;
        assert!(
            validate(&sources, &checked, &typed, &instances, &bridge, &mut trace)
                .unwrap_err()
                .starts_with("E-TYPED-SOURCE-MAP:")
        );
        assert_eq!(super::super::evaluate(&typed).unwrap().to_string(), "42");
    }

    #[test]
    fn invalid_slots_and_foreign_domains_do_not_resolve() {
        let mapped = lower_main_with_function_origins(source("fn main() { 42 }")).unwrap();
        let foreign = lower_main_with_function_origins(source("fn main() { 42 }")).unwrap();
        assert!(mapped
            .instance(&FunctionInstanceId {
                domain: mapped.domain.clone(),
                slot: usize::MAX
            })
            .is_none());
        assert!(mapped
            .definition(&DefinitionId {
                domain: mapped.domain.clone(),
                slot: usize::MAX
            })
            .is_none());
        assert!(mapped
            .source_node(&SourceNodeId {
                domain: mapped.domain.clone(),
                slot: usize::MAX
            })
            .is_none());
        assert!(foreign.instance(&mapped.entry()).is_none());
        assert!(mapped.clone().instance(&mapped.entry()).is_some());
    }
    #[test]
    fn mapping_limits_count_duplicate_requests_without_masking_semantic_errors() {
        let sources = source("fn id(x) { x } fn main() { id(20) + id(22) }");
        assert!(
            lower_with_trace(sources.clone(), instances::Trace::with_limits(3, 8_000_000)).is_ok()
        );
        let error = lower_with_trace(sources.clone(), instances::Trace::with_limits(2, 8_000_000))
            .unwrap_err();
        assert!(error.starts_with("E-TYPED-SOURCE-MAP-LIMIT:"));
        let error =
            lower_with_trace(sources.clone(), instances::Trace::with_limits(3, 0)).unwrap_err();
        assert!(error.starts_with("E-TYPED-SOURCE-MAP-LIMIT:"));
        let checked = check::analyze(sources.program()).unwrap();
        assert_eq!(
            super::super::evaluate(&super::super::lower_main(&checked).unwrap())
                .unwrap()
                .to_string(),
            "42"
        );
        // This is checked source but remains unsupported by ordinary typed
        // lowering. A trace failure has already latched before it is reached.
        let unsupported =
            source("type Option<a> = None | Some(a) fn main() { let mut x = None; 42 }");
        let checked = check::analyze(unsupported.program()).unwrap();
        let ordinary = super::super::lower_main(&checked).unwrap_err();
        assert_eq!(
            lower_with_trace(unsupported, instances::Trace::with_limits(0, 0)).unwrap_err(),
            ordinary
        );
    }
    #[test]
    fn deep_generic_signatures_are_bounded_without_duplicating_discovery() {
        // Build the deep concrete type with shallow expressions, keeping the
        // independent parser syntax-depth guard outside this signature test.
        let mut body = "let n0 = 42;".to_owned();
        for level in 0..32 {
            body.push_str(&format!("let n{} = Nest(n{level});", level + 1));
        }
        let sources = source(&format!(
            "type Nest<a> = Nest(a) fn id(x) {{ x }} fn main() {{ {body} id(n32) }}"
        ));
        let checked = check::analyze(sources.program()).unwrap();
        let ordinary = super::super::lower_main(&checked).unwrap();
        let expected_c = super::super::emit_c(&ordinary).unwrap();
        let mapped = lower_main_with_function_origins(sources.clone()).unwrap();
        assert_eq!(super::super::emit_c(mapped.typed()).unwrap(), expected_c);
        assert_eq!(mapped.instances().len(), 2);
        let id = mapped
            .instances()
            .iter()
            .find(|i| mapped.definition(&i.definition).unwrap().symbol.name == "id")
            .unwrap();
        let mut ty = &id.result;
        let mut depth = 0;
        while let ConcreteType::Nominal { arguments, .. } = ty {
            assert_eq!(arguments.len(), 1);
            depth += 1;
            ty = &arguments[0];
        }
        assert_eq!(depth, 32);
        assert_eq!(ty, &ConcreteType::Int);
        assert!(
            lower_with_trace(sources, instances::Trace::with_limits(2, 32))
                .unwrap_err()
                .starts_with("E-TYPED-SOURCE-MAP-LIMIT:")
        );
    }
}
