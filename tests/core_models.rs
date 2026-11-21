//! Finite source-language models. Only the differential harness calls Rill.
use std::{fmt::Write as _, fs, process::Command};

/// A case is a complete Rill function body and the observation predicted by
/// the model, or the required diagnostic category for a rejected program.
struct Case {
    body: String,
    expected: Result<i32, &'static str>,
}

fn check_cases(label: &str, prelude: &str, cases: &[Case], owners: bool) -> usize {
    let mut accepted = Vec::new();
    for case in cases {
        let source = format!("{prelude}\nfn main() {{ {} }}", case.body);
        let program = rill::parser::parse(&source).unwrap_or_else(|e| panic!("{source}\n{e}"));
        match case.expected {
            Err(code) => {
                let error = rill::check::check(&program).expect_err(&source);
                assert!(error.starts_with(code), "{source}\n{error}");
                assert_eq!(
                    rill::backend::emit(&program).unwrap_err(),
                    error,
                    "{source}"
                );
                assert_eq!(
                    rill::interpreter::run(&program).unwrap_err(),
                    error,
                    "{source}"
                );
            }
            Ok(expected) => {
                let checked =
                    rill::check::analyze(&program).unwrap_or_else(|e| panic!("{source}\n{e}"));
                assert_eq!(checked.has_owned(), owners);
                let outcome =
                    rill::interpreter::run(&program).unwrap_or_else(|e| panic!("{source}\n{e}"));
                assert_eq!(outcome.to_string(), expected.to_string(), "{source}");
                if !owners {
                    assert_eq!(
                        rill::interpreter::run_reference(&program).unwrap(),
                        outcome,
                        "{source}"
                    );
                }
                accepted.push((&case.body, expected));
            }
        }
    }
    assert!(!accepted.is_empty() && accepted.len() < cases.len());
    let dir = std::env::temp_dir().join(format!("rill-core-model-{label}-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    for batch in accepted.chunks(16) {
        let mut source = prelude.to_owned();
        let mut checks = Vec::new();
        for (i, (body, expected)) in batch.iter().enumerate() {
            writeln!(source, "\nfn case{i}() {{ {body} }}").unwrap();
            // A distinct failure index identifies a mismatch, without allowing
            // errors to cancel one another when batching C builds.
            checks.push(format!("if case{i}() == {expected} {{"));
        }
        writeln!(source, "fn main() {{ {} 0 ", checks.join(" ")).unwrap();
        for i in (1..=batch.len()).rev() {
            write!(source, "}} else {{ {i} }} ").unwrap();
        }
        source.push('}');
        fs::write(
            dir.join("model.c"),
            rill::compile(&source).unwrap_or_else(|e| panic!("{source}\n{e}")),
        )
        .unwrap();
        for opt in ["-O0", "-O3"] {
            let binary = dir.join("check");
            let build = Command::new("cc")
                .args(["-std=c11", "-pedantic-errors", opt])
                .arg(dir.join("model.c"))
                .arg("-o")
                .arg(&binary)
                .output()
                .unwrap();
            assert!(
                build.status.success(),
                "{}",
                String::from_utf8_lossy(&build.stderr)
            );
            let run = Command::new(binary).output().unwrap();
            assert!(
                run.status.success(),
                "{source}\n{}",
                String::from_utf8_lossy(&run.stderr)
            );
            assert_eq!(
                run.stdout,
                b"0\n",
                "{source}\n{opt}: {}",
                String::from_utf8_lossy(&run.stdout)
            );
        }
    }
    fs::remove_dir_all(dir).unwrap();
    accepted.len()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Payload {
    Int,
    Bool,
}
use Payload::{Bool, Int};

#[derive(Clone, Copy)]
enum OptionOp {
    Read(Payload),
    Write(Payload),
}
use OptionOp::{Read, Write};

impl OptionOp {
    fn payload(self) -> Payload {
        match self {
            Read(p) | Write(p) => p,
        }
    }

    fn source(self, slot: &str) -> String {
        match self {
            Read(Int) => format!("observe_int({slot})"),
            Read(Bool) => format!("observe_bool({slot})"),
            Write(Int) => format!("{{ {slot} <- Some(7); 0 }}"),
            Write(Bool) => format!("{{ {slot} <- Some(true); 0 }}"),
        }
    }

    fn step(self, state: &mut Option<Payload>) -> i32 {
        match self {
            Write(p) => {
                *state = Some(p);
                0
            }
            Read(p) => match (*state, p) {
                (None, Int) => 1,
                (None, Bool) => 2,
                (Some(Int), Int) => 7,
                (Some(Bool), Bool) => 3,
                _ => panic!("model reached a stuck read after its static judgment"),
            },
        }
    }
}

#[test]
fn finite_option_specialization_and_snapshot_model() {
    let prelude = "type Option<a> = None | Some(a)
fn absent() { None }
fn observe_int(x: Option<i32>) { match x { None => 1 | Some(n) => n } }
fn observe_bool(x: Option<bool>) { match x { None => 2 | Some(b) => if b { 3 } else { 4 } } }";
    let read = [Read(Int), Read(Bool)];
    let read_write = [Read(Int), Read(Bool), Write(Int), Write(Bool)];
    let mut cases = Vec::new();
    for (binding, generalized, initial, ops) in [
        ("let slot = None;", true, None, &read[..]),
        ("let slot = absent();", false, None, &read[..]),
        ("let mut slot = None;", false, None, &read_write[..]),
        ("let mut slot = Some(7);", false, Some(Int), &read_write[..]),
        (
            "let mut slot = Some(true);",
            false,
            Some(Bool),
            &read_write[..],
        ),
    ] {
        for &first in ops {
            for &second in ops {
                for &last in &read {
                    let body = format!("{binding} let x = {}; let snapshot = slot; let y = {}; let z = {}; 100 * x + 10 * y + z",
                        first.source("slot"), second.source("slot"), last.source("snapshot"));
                    // Existential ground assignment for a monotype, fresh
                    // assignment per occurrence for the immutable scheme.
                    let well_typed = generalized
                        || [Int, Bool].into_iter().any(|ty| {
                            initial.is_none_or(|p| p == ty)
                                && [first, second, last].iter().all(|op| op.payload() == ty)
                        });
                    let expected = if well_typed {
                        let mut slot = initial;
                        let x = first.step(&mut slot);
                        let mut snapshot = slot;
                        let y = second.step(&mut slot);
                        let z = last.step(&mut snapshot);
                        Ok(100 * x + 10 * y + z)
                    } else {
                        Err("E-TYPE:")
                    };
                    cases.push(Case { body, expected });
                }
            }
        }
    }
    assert_eq!(cases.len(), 112);
    assert_eq!(check_cases("option", prelude, &cases, false), 26);
}

#[derive(Clone, Copy)]
enum BoxOp {
    ReadA,
    ReadB,
    SetA,
    TakeA,
    TakeB,
    CopyA,
}
use BoxOp::*;

impl BoxOp {
    fn source(self) -> &'static str {
        match self {
            ReadA => "box_read(a)",
            ReadB => "box_read(b)",
            SetA => "{ box_write(inout a, 7); box_read(a) }",
            TakeA => "{ let taken = move a; box_read(taken) }",
            TakeB => "{ let taken = move b; box_read(taken) }",
            CopyA => "{ let copied = a; box_read(copied) }",
        }
    }

    fn step(self, store: &mut [Option<i32>; 2]) -> Option<i32> {
        match self {
            ReadA => store[0],
            ReadB => store[1],
            SetA => {
                *store[0].as_mut()? = 7;
                store[0]
            }
            TakeA => store[0].take(),
            TakeB => store[1].take(),
            CopyA => None,
        }
    }
}

#[test]
fn finite_affine_branch_join_model() {
    let mut cases = Vec::new();
    for left in [ReadA, SetA, TakeA, TakeB] {
        for right in [ReadA, SetA, TakeA, TakeB] {
            for suffix in [ReadA, ReadB, TakeA, CopyA] {
                for condition in [false, true] {
                    let body = format!("let mut a = system_box_i32_abort(1); let b = system_box_i32_abort(2); let x = if {condition} {{ {} }} else {{ {} }}; let y = {}; x * 10 + y", left.source(), right.source(), suffix.source());
                    let mut yes = [Some(1), Some(2)];
                    let mut no = yes;
                    let x = left.step(&mut yes).unwrap();
                    let y = right.step(&mut no).unwrap();
                    let same_live_set = yes.map(|v| v.is_some()) == no.map(|v| v.is_some());
                    let (first, mut selected) = if condition { (x, yes) } else { (y, no) };
                    let expected = if same_live_set {
                        suffix.step(&mut selected).map(|v| 10 * first + v)
                    } else {
                        None
                    };
                    cases.push(Case {
                        body,
                        expected: expected.ok_or("E-OWNED:"),
                    });
                }
            }
        }
    }
    assert_eq!(cases.len(), 128);
    assert_eq!(check_cases("owner", "", &cases, true), 30);
}
