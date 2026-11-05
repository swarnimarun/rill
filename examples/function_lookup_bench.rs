//! Run with `cargo run --release --example function_lookup_bench`.
//! Measures callable detection only, excluding parsing/checking/C compilation.
use rill::ast::{Expr, Function, Program};
use std::{collections::HashSet, hint::black_box, time::Instant};

fn program(count: usize) -> Program {
    let mut program = rill::parser::parse("fn main() { 42 }").unwrap();
    for index in 0..count {
        program.functions.push(Function {
            name: format!("function{index:05}"),
            params: vec![],
            result_annotation: None,
            body: Expr::Call(format!("function{:05}", count - 1), vec![]),
        });
    }
    program.functions.last_mut().unwrap().body = Expr::Int(42);
    program
}

// Archived linear-lookup detector, restricted to this benchmark's leaf bodies.
// The fixture has no nested expressions, lambdas or partial applications.
fn linear_detector(program: &Program) -> bool {
    let names: HashSet<_> = program.functions.iter().map(|f| f.name.as_str()).collect();
    let mut pending: Vec<_> = program.functions.iter().map(|f| &f.body).collect();
    while let Some(expr) = pending.pop() {
        match expr {
            Expr::Int(_) => (),
            Expr::Var(name) => {
                if names.contains(name.as_str()) {
                    return true;
                }
            }
            Expr::Call(name, args) => {
                if program
                    .functions
                    .iter()
                    .any(|f| f.name == *name && f.params.len() != args.len())
                {
                    return true;
                }
                pending.extend(args);
            }
            _ => panic!("unsupported benchmark fixture"),
        }
    }
    false
}

fn median_ns(detector: fn(&Program) -> bool, program: &Program) -> u128 {
    assert!(!detector(black_box(program)));
    let mut times = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        assert!(!detector(black_box(program)));
        times.push(start.elapsed().as_nanos());
    }
    times.sort_unstable();
    times[times.len() / 2]
}

fn main() {
    println!("functions\tlinear_ns\tindexed_ns");
    for count in [1000, 5000, 10000] {
        let program = program(count);
        let before = median_ns(linear_detector, &program);
        let after = median_ns(rill::functions::needs_lowering, &program);
        println!("{count}\t{before}\t{after}");
    }
}
