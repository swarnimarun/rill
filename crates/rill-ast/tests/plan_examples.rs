//! M0 verification: every Rill example from PLAN.md §2–§11 must parse into a
//! lossless CST and a desugared AST without diagnostics.
//!
//! Round-trip accuracy is checked for each fixture: the CST text must be
//! byte-identical to the input.

use rill_ast::{parse_and_lower_text, Expr, Frontend, Item, Stmt, TypeBody};
use rill_diagnostics::render_all;

struct Fixture {
    name: &'static str,
    source: &'static str,
}

fn check(fixture: &Fixture) -> Frontend {
    let (frontend, sm) = parse_and_lower_text(fixture.name, fixture.source);
    assert!(
        !frontend.diagnostics.has_errors(),
        "{} produced diagnostics:\n{}",
        fixture.name,
        render_all(&sm, &frontend.diagnostics)
    );
    assert_eq!(
        frontend.cst.text(&sm),
        fixture.source,
        "{} must round-trip byte-for-byte",
        fixture.name
    );
    frontend.module.items.len();
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

#[test]
fn all_plan_examples_parse_losslessly() {
    check_all(FIXTURES);
}

#[test]
fn showcase_ast_shape() {
    let frontend = check(&FIXTURES[14]);
    let module = &frontend.module;
    // use x2, type, project, transfer, quicksort, main
    assert_eq!(module.items.len(), 7, "showcase item count");

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
    assert_eq!(effects, vec!["decides", "transacts"]);
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
    assert_eq!(body.tail.as_deref().unwrap().span().len(), 4, "`0i32` tail");
}
