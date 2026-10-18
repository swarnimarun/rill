//! M0 verification: every Rill example from PLAN.md §2–§11 must parse into a
//! lossless CST and a desugared AST without diagnostics.
//!
//! Round-trip accuracy is checked for each fixture: the CST text must be
//! byte-identical to the input.

use rill_ast::{
    parse_and_lower_text, Block, Cond, ElseBranch, Expr, Frontend, Item, Module, Pattern, Stmt,
    Type, TypeBody, TypeDecl,
};
use rill_diagnostics::render_all;

struct Fixture {
    name: &'static str,
    source: &'static str,
}

fn check(fixture: &Fixture) -> Frontend {
    check_source(fixture.name, fixture.source)
}

fn check_source(name: &str, source: &str) -> Frontend {
    let (frontend, sm) = parse_and_lower_text(name, source);
    assert!(
        !frontend.diagnostics.has_errors(),
        "{name} produced diagnostics:\n{}",
        render_all(&sm, &frontend.diagnostics)
    );
    assert_eq!(
        frontend.cst.text(&sm),
        source,
        "{name} must round-trip byte-for-byte"
    );
    // A clean parse must not hide a lowering gap behind `Error` nodes.
    let errors = find_errors(&frontend.module);
    assert!(
        errors.is_empty(),
        "{name} lowered to error nodes: {errors:?}"
    );
    frontend
}

fn check_all(fixtures: &[Fixture]) {
    for fixture in fixtures {
        check(fixture);
    }
}

/// §3.3 — a failable call turned total by a fallback choice.
const SAFE_HEAD: &str = "let safe_head (xs: Array i32) -> i32 <computes> = xs[0] | 0;\n";

/// §4.1 — disjoint field splitting with `inout`.
const UPDATE_TRANSFORM: &str = "\
type Transform = record {
    var position: Vec3;
    var rotation: Quat;
    var scale: Vec3;
};

let update_transform (t: inout Transform) <computes> = {
    # Both `t.position` and `t.rotation` are borrowed simultaneously!
    # Because `position` != `rotation`, the Place Checker proves disjointness:
    # Place(t.position) is disjoint from Place(t.rotation).
     rotate_point (inout t.position, t.rotation);
};
";

/// §4.2 — bounded array projection plus a virtual, computed projection.
const PROJECTIONS: &str = "\
project at (xs: inout Array a, idx: usize) -> inout a <decides> = {
    idx < size xs;
    ptr := unsafe { array_raw_ptr_mut (inout xs) + idx };
    yield inout (*ptr);
};

type PackedColor = record { var rgba: u32; };

project redChannel (c: inout PackedColor) -> inout u8 <computes> = {
    var r: u8 = ((c.rgba >> 24) & 0xFF) as u8;
    yield inout r;
    set c.rgba = (c.rgba & 0x00FFFFFF) | ((r as u32) << 24);
};
";

const PROJECTION_USE: &str = "\
let paint () = {
    var color = PackedColor 0x10203040u32;
    set (color . redChannel) = 0xFFu8;
};
";

/// §4.3 — `view` types and built-in slice splitting.
const SLICE_SPLIT: &str = "\
type MatrixView a = view {
    data: Slice a;
    rows: usize;
    cols: usize;
};

project split_at (s: inout Slice a, mid: usize) -> inout (Slice a, Slice a) <decides> = {
    mid <= size s;
    var pair = unsafe { raw_slice_split (inout s, mid) };
    yield inout pair;
};
";

/// §5.1 — nondeterministic choice and comprehension.
const CHOICE: &str = "\
let choose () = all {
    x := (1 | 2 | 3);
    y := (10 | 20);
    x + y
};

let ranges () = all {
    x := 1..3;
    y := (a | b);
    x
};
";

/// §5.2 — speculative mutation that must roll back.
const SPECULATIVE: &str = "\
let damage (player: inout Player) -> () <decides | transacts> = {
    set player.health = 0;
    player.health > 10;
};
";

/// §5.3 — the structured-concurrency quartet.
const CONCURRENT: &str = "\
let combos () = {
    sync { fetch_a (); fetch_b () };
    race { fetch_a (); fetch_b () };
    rush { fetch_a (); fetch_b () };
    branch { fetch_a (); fetch_b () }
};
";

/// §5.4 — `<alloc>` with a scoped arena handler.
const ARENA: &str = "\
let build_list (n: i32) -> Array i32 <alloc> = {
    var out = Array::with_capacity n;
    for (i := 0..n) out . push i;
    out
};

let process_packet (buf: inout [u8; 4096]) -> i32 <computes> = {
    StackArena::run (inout buf) {
        items := build_list 100;
        items . fold 0 { acc, x -> acc + x }
    }
};
";

/// §6.1/§6.2 — a user builder and its use.
const SEQ_BUILDER: &str = "\
builder seq = {
    let yield_val (x: a) -> Step a <computes> = Step::Yield x;
    let combine (s1: Step a, s2: () -> Step a) -> Step a <computes> = Step::Chain s1 s2;
    let ret (u: ()) -> Step a <computes> = Step::Done;
};

let evens (n: i32) = seq {
    for (i : 0..n) {
        if (i % 2 = 0) { yield i } else { () }
    }
};
";

/// §6.1 — the `let?` half of the protocol.
const TRY_BIND: &str = "\
let read_one (s: Slice u8) = {
    let? byte = parse_byte s;
    byte
};
";

/// §6.3 — the SIMD builder.
const SIMD: &str = "\
let saxpy (a: f32, x: Slice f32, y: inout Slice f32) <computes> = {
    simd {
        lane := lanes 8;
        idx := range (0 ..< size x) lane;
        vx := load x[idx];
        vy := load y[idx];
        store (inout y[idx]) (fma a vx vy)
    }
};
";

/// §9 — arity and calling conventions.
const ARITY: &str = "\
let add (x: i32, y: i32) = x + y;
let add_curried (x: i32) (y: i32) = x + y;
let direct () = add 10 20;
let flowed () = 10 . add 20;
let partial () = {
    inc := add 1;
    inc 41
};
let boxed () = (add 1) as dyn (i32 -> i32);
";

/// §10.1 — `core` style trait/impl declarations.
const CORE_TRAITS: &str = "\
trait Allocator a = {
    let alloc (self: inout a, n: usize) -> &raw u8 <alloc>;
};

impl Allocator PageAllocator = {
    let alloc (self: inout PageAllocator, n: usize) -> &raw u8 <alloc> = {
        page_alloc n
    };
};

let swap (a: inout t, b: inout t) <computes> = { };
";

/// §11 — the end-to-end showcase, taken verbatim from `examples/showcase.rill`.
const SHOWCASE: &str = include_str!("../../../examples/showcase.rill");

/// §2.3/§2.4 — grammar-level shapes: variants, `dyn`, raw pointers, effects.
const GRAMMAR_SHAPES: &str = "\
type Option a = | None | Some a;
type Result v e = | Ok v | Err e;
type Table a = [a; 4096];
type Bytes = [u8];
type Callback = (i32) -> i32;
type Handler = dyn ((i32) -> i32 <io>);

count : forall_like a -> usize <computes>;

extern \"c\" let puts (s: &raw u8) -> i32 <io>;
extern let malloc (n: usize) -> &raw mut u8 <alloc>;

let constrained (x: a) -> a where a is Show | a is Eq <computes> = x;

let indexed (xs: Array i32, i: usize) -> i32 <decides> = xs[i];

let nested (p: Pair) = match p {
    Pair (a, b) -> a + b,
    _ -> 0
};
";

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "safe_head.rill",
        source: SAFE_HEAD,
    },
    Fixture {
        name: "update_transform.rill",
        source: UPDATE_TRANSFORM,
    },
    Fixture {
        name: "projections.rill",
        source: PROJECTIONS,
    },
    Fixture {
        name: "projection_use.rill",
        source: PROJECTION_USE,
    },
    Fixture {
        name: "slice_split.rill",
        source: SLICE_SPLIT,
    },
    Fixture {
        name: "choice.rill",
        source: CHOICE,
    },
    Fixture {
        name: "speculative.rill",
        source: SPECULATIVE,
    },
    Fixture {
        name: "concurrent.rill",
        source: CONCURRENT,
    },
    Fixture {
        name: "arena.rill",
        source: ARENA,
    },
    Fixture {
        name: "seq_builder.rill",
        source: SEQ_BUILDER,
    },
    Fixture {
        name: "try_bind.rill",
        source: TRY_BIND,
    },
    Fixture {
        name: "simd.rill",
        source: SIMD,
    },
    Fixture {
        name: "arity.rill",
        source: ARITY,
    },
    Fixture {
        name: "core_traits.rill",
        source: CORE_TRAITS,
    },
    Fixture {
        name: "showcase.rill",
        source: SHOWCASE,
    },
    Fixture {
        name: "grammar_shapes.rill",
        source: GRAMMAR_SHAPES,
    },
];

// ---------------------------------------------------------------------------
// Error-node audit
// ---------------------------------------------------------------------------

/// Collects every `Error` node in a lowered module. A clean parse must produce
/// none: this is what makes the corpus check meaningful for every CST kind,
/// instead of only checking that no diagnostic was emitted.
fn find_errors(module: &Module) -> Vec<String> {
    let mut finder = ErrorFinder { found: Vec::new() };
    for item in &module.items {
        finder.item(item);
    }
    finder.found
}

struct ErrorFinder {
    found: Vec<String>,
}

impl ErrorFinder {
    fn push(&mut self, what: &str, span: rill_span::Span) {
        self.found.push(format!("{what} at {span:?}"));
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Error(span) => self.push("item", *span),
            Item::Use(_) => {}
            Item::Signature(decl) => self.ty(&decl.ty),
            Item::Fn(decl) => self.fn_decl(decl),
            Item::Project(decl) => {
                for group in &decl.params {
                    for param in &group.params {
                        if let Some(ty) = &param.ty {
                            self.ty(ty);
                        }
                    }
                }
                if let Some(ret) = &decl.ret {
                    self.ty(&ret.ty);
                }
                self.expr(&decl.body);
            }
            Item::Type(decl) => {
                for param in &decl.params {
                    if let Some(ty) = &param.comptime_ty {
                        self.ty(ty);
                    }
                }
                self.type_body(&decl.body);
            }
            Item::Trait(decl) => {
                for member in &decl.members {
                    self.item(member);
                }
            }
            Item::Impl(decl) => {
                for member in &decl.members {
                    self.item(member);
                }
            }
            Item::Builder(decl) => {
                for member in &decl.members {
                    self.item(member);
                }
            }
            Item::Extern(decl) => self.item(&decl.inner),
            Item::Stmt(stmt) => self.stmt(stmt),
        }
    }

    fn fn_decl(&mut self, decl: &rill_ast::FnDecl) {
        for group in &decl.params {
            for param in &group.params {
                if let Some(ty) = &param.ty {
                    self.ty(ty);
                }
            }
        }
        if let Some(ret) = &decl.ret {
            self.ty(&ret.ty);
        }
        if let Some(body) = &decl.body {
            self.expr(body);
        }
    }

    fn type_body(&mut self, body: &TypeBody) {
        match body {
            TypeBody::Error(span) => self.push("type body", *span),
            TypeBody::Record(record) | TypeBody::View(record) => {
                for field in &record.fields {
                    self.ty(&field.ty);
                }
                if let Some(deinit) = &record.deinit {
                    self.block(&deinit.body);
                }
            }
            TypeBody::Variants(variants) => {
                for variant in variants {
                    for payload in &variant.payload {
                        self.ty(payload);
                    }
                }
            }
            TypeBody::Alias(ty) => self.ty(ty),
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Error(span) => self.push("statement", *span),
            Stmt::Var { ty, init, .. } => {
                if let Some(ty) = ty {
                    self.ty(ty);
                }
                self.expr(init);
            }
            Stmt::Set { place, value, .. } => {
                self.expr(place);
                self.expr(value);
            }
            Stmt::Yield { value, .. } => self.expr(value),
            Stmt::Bind { name, value, .. } => {
                self.pattern(name);
                self.expr(value);
            }
            Stmt::TryBind { value, .. } => self.expr(value),
            Stmt::When { cond, .. } => self.cond(cond),
            Stmt::While { cond, body, .. } => {
                self.expr(cond);
                self.block(body);
            }
            Stmt::Break { .. } => {}
            Stmt::For {
                pattern,
                iter,
                body,
                ..
            } => {
                self.pattern(pattern);
                self.expr(iter);
                self.block(body);
            }
            Stmt::Let(decl) => self.fn_decl(decl),
            Stmt::Expr(expr) => self.expr(expr),
        }
    }

    fn cond(&mut self, cond: &Cond) {
        match cond {
            Cond::Expr(expr) => self.expr(expr),
            Cond::Bind { pattern, value, .. } => {
                self.pattern(pattern);
                self.expr(value);
            }
        }
    }

    fn block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.stmt(stmt);
        }
        if let Some(tail) = &block.tail {
            self.expr(tail);
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Error(span) => self.push("expression", *span),
            Expr::Literal { .. } | Expr::Path { .. } => {}
            Expr::Tuple { elems, .. } | Expr::Array { elems, .. } => {
                for elem in elems {
                    self.expr(elem);
                }
            }
            Expr::Block(block) => self.block(block),
            Expr::Lambda { body, .. } => self.block(body),
            Expr::Call { callee, args, .. }
            | Expr::Index {
                base: callee, args, ..
            } => {
                self.expr(callee);
                for arg in args {
                    self.expr(&arg.value);
                }
            }
            Expr::Try { base, .. } => self.expr(base),
            Expr::Unary { operand, .. } => self.expr(operand),
            Expr::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            Expr::Cast { expr, ty, .. } | Expr::Is { expr, ty, .. } => {
                self.expr(expr);
                self.ty(ty);
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.cond(cond);
                self.block(then_branch);
                match else_branch {
                    Some(ElseBranch::Block(block)) => self.block(block),
                    Some(ElseBranch::If(expr)) => self.expr(expr),
                    None => {}
                }
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee);
                for arm in arms {
                    self.pattern(&arm.pattern);
                    if let Some(guard) = &arm.guard {
                        self.expr(guard);
                    }
                    self.expr(&arm.body);
                }
            }
            Expr::Comptime { body, .. }
            | Expr::Unsafe { body, .. }
            | Expr::Handler { body, .. } => self.block(body),
            Expr::Concurrent { arms, .. } => {
                for arm in arms {
                    self.expr(arm);
                }
            }
        }
    }

    fn ty(&mut self, ty: &Type) {
        match ty {
            Type::Error(span) => self.push("type", *span),
            Type::Path { .. } => {}
            Type::App { ctor, args, .. } => {
                self.ty(ctor);
                for arg in args {
                    self.ty(arg);
                }
            }
            Type::Tuple { elems, .. } => {
                for elem in elems {
                    self.ty(elem);
                }
            }
            Type::Array { elem, len, .. } => {
                self.ty(elem);
                if let Some(len) = len {
                    self.expr(len);
                }
            }
            Type::Fn { params, ret, .. } => {
                for param in params {
                    self.ty(&param.ty);
                }
                if let Some(ret) = ret {
                    self.ty(ret);
                }
            }
            Type::Ptr { pointee, .. } => self.ty(pointee),
            Type::Dyn { inner, .. } => self.ty(inner),
        }
    }

    fn pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Error(span) => self.push("pattern", *span),
            Pattern::Constructor { args, .. } => {
                for arg in args {
                    self.pattern(arg);
                }
            }
            Pattern::Tuple { elems, .. } => {
                for elem in elems {
                    self.pattern(elem);
                }
            }
            Pattern::Wildcard(_) | Pattern::Binding { .. } | Pattern::Literal { .. } => {}
        }
    }
}

fn type_decl<'a>(frontend: &'a Frontend, name: &str) -> &'a TypeDecl {
    frontend
        .module
        .items
        .iter()
        .find_map(|item| match item {
            Item::Type(decl) if frontend.text(decl.name) == name => Some(decl),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no type declaration named `{name}`"))
}

#[test]
fn all_plan_examples_parse_losslessly() {
    check_all(FIXTURES);
}

#[test]
fn showcase_ast_shape() {
    let frontend = check(&FIXTURES[14]);
    let module = &frontend.module;
    // use, two types, a trait, two impls, three task functions, project,
    // transfer, first_positive, main
    assert_eq!(module.items.len(), 14, "showcase item count");

    let account = module
        .items
        .iter()
        .find_map(|item| match item {
            Item::Type(decl) if frontend.text(decl.name) == "Account" => Some(decl),
            _ => None,
        })
        .expect("Account decl");
    match &account.body {
        TypeBody::Record(record) => {
            assert_eq!(record.fields.len(), 2);
            assert!(!record.fields[0].mutable);
            assert!(record.fields[1].mutable);
        }
        other => panic!("expected a record body, got {other:?}"),
    }

    let transfer = module
        .items
        .iter()
        .find_map(|item| match item {
            Item::Fn(decl) if frontend.text(decl.name) == "transfer" => Some(decl),
            _ => None,
        })
        .expect("transfer decl");
    let effects: Vec<&str> = transfer
        .effects
        .items
        .iter()
        .map(|e| frontend.text(e.name))
        .collect();
    // `transacts` is not inferred by the checker: the trail is implicit in a
    // choice or handler, so the declaration that matches the body is
    // `<decides | writes>`. Recording `transacts` too would be rejected as a
    // declared effect the body does not perform.
    assert_eq!(effects, vec!["decides", "writes"]);
    let body = transfer.body.as_ref().expect("transfer body").clone();
    let Expr::Block(block) = body else {
        panic!("expected a block body")
    };
    assert_eq!(block.stmts.len(), 4, "four guarded statements");
    assert!(matches!(block.stmts[0], Stmt::Expr(_)), "`amount > 0;`");
    assert!(
        matches!(block.stmts[1], Stmt::Set { .. }),
        "`set (from . balance_mut) = ...;`"
    );

    let main = module
        .items
        .iter()
        .find_map(|item| match item {
            Item::Fn(decl) if frontend.text(decl.name) == "main" => Some(decl),
            _ => None,
        })
        .expect("main decl");
    let Expr::Block(body) = main.body.as_ref().expect("main body") else {
        panic!("expected a block body")
    };
    // The tail is an expression whose value becomes the exit status; what it
    // *is* is pinned by the driver test, which runs the program and checks 44.
    assert!(body.tail.is_some(), "`main` has a tail value");
}

/// The plan's own fenced examples, extracted from `PLAN.md` so the corpus
/// cannot silently shrink as the document evolves.
const PLAN: &str = include_str!("../../../PLAN.md");

fn plan_rill_fences() -> Vec<(usize, String)> {
    let mut fences = Vec::new();
    let mut current: Option<(usize, Vec<&str>)> = None;
    for (index, line) in PLAN.lines().enumerate() {
        let trimmed = line.trim_start();
        match current.as_mut() {
            None if trimmed.starts_with("```rill") => current = Some((index + 1, Vec::new())),
            Some((start, lines)) if trimmed.starts_with("```") => {
                let (start, lines) = current.take().expect("borrowed above");
                let _ = start;
                fences.push((start, format!("{}\n", lines.join("\n"))));
            }
            Some((_, lines)) => lines.push(line),
            None => {}
        }
    }
    fences
}

#[test]
fn every_rill_fence_in_the_plan_parses() {
    let fences = plan_rill_fences();
    assert_eq!(
        fences.len(),
        10,
        "PLAN.md must keep exactly ten ```rill blocks"
    );
    for (line, source) in &fences {
        check_source(&format!("PLAN.md:{line}"), source);
    }
}

#[test]
fn type_groups_are_transparent() {
    let frontend = check_source(
        "types.rill",
        "type Callback = (i32) -> i32;\n\
         type Nested = Array (Slice u8);\n\
         type Unit = ();\n\
         type Pair = (i32, i32);\n\
         type Handler = dyn ((i32) -> i32 <io>);\n\
         type Raw = &raw mut u8;\n",
    );

    // `(i32) -> i32` is a function type whose parameter is `i32`, not a 1-tuple.
    match &type_decl(&frontend, "Callback").body {
        TypeBody::Alias(Type::Fn {
            params,
            ret,
            effects,
            ..
        }) => {
            assert_eq!(params.len(), 1);
            assert!(matches!(&params[0].ty, Type::Path { segments, .. } if segments.len() == 1));
            assert_eq!(
                frontend.text(match &params[0].ty {
                    Type::Path { segments, .. } => segments[0],
                    other => panic!("expected `i32`, got {other:?}"),
                }),
                "i32"
            );
            assert!(ret.is_some());
            assert!(effects.is_empty());
        }
        other => panic!("expected a function alias, got {other:?}"),
    }

    // `Array (Slice u8)` applies `Array` to a grouped application.
    match &type_decl(&frontend, "Nested").body {
        TypeBody::Alias(Type::App { ctor, args, .. }) => {
            assert_eq!(args.len(), 1);
            assert!(matches!(**ctor, Type::Path { .. }));
            assert!(matches!(&args[0], Type::App { args, .. } if args.len() == 1));
        }
        other => panic!("expected a type application, got {other:?}"),
    }

    // `()` is unit; `(i32, i32)` is a tuple.
    assert!(
        matches!(&type_decl(&frontend, "Unit").body, TypeBody::Alias(Type::Tuple { elems, .. }) if elems.is_empty())
    );
    assert!(
        matches!(&type_decl(&frontend, "Pair").body, TypeBody::Alias(Type::Tuple { elems, .. }) if elems.len() == 2)
    );

    // `dyn ((i32) -> i32 <io>)` keeps the arrow type and its effect row.
    match &type_decl(&frontend, "Handler").body {
        TypeBody::Alias(Type::Dyn { inner, .. }) => match &**inner {
            Type::Fn {
                params, effects, ..
            } => {
                assert_eq!(params.len(), 1);
                let names: Vec<&str> = effects
                    .items
                    .iter()
                    .map(|item| frontend.text(item.name))
                    .collect();
                assert_eq!(names, vec!["io"]);
            }
            other => panic!("expected a function type inside `dyn`, got {other:?}"),
        },
        other => panic!("expected a `dyn` alias, got {other:?}"),
    }

    // `&raw mut u8` is a mutable raw pointer.
    assert!(matches!(
        &type_decl(&frontend, "Raw").body,
        TypeBody::Alias(Type::Ptr { mutable: true, .. })
    ));
}
