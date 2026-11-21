use rill::{
    ast::Expr,
    check::{self, Type},
    parser,
};

fn rejects(source: &str, code: &str) {
    let program = parser::parse(source).unwrap();
    let error = check::analyze(&program)
        .err()
        .expect("invalid callable was accepted");
    assert!(error.contains(code), "{error}");
}

#[test]
fn inferred_callable_signatures_preserve_type_and_environment_variables() {
    let program=parser::parse("fn apply(f,x) { f(x) } fn main() { let identity = fn(value) { value }; if identity(true) { apply(identity, 42) } else { 0 } }").unwrap();
    let checked = check::analyze(&program).unwrap();
    assert!(checked.functions["apply"].quantified.len() >= 3);
    assert!(matches!(
        &checked.functions["apply"].params[0],
        Type::Callable { .. }
    ));
    assert_eq!(checked.functions["main"].result, Type::Int);
    let Expr::Let(_, lambda, _) = &program.functions[1].body else {
        panic!("missing lambda binding")
    };
    assert!(checked.closure_info(lambda).unwrap().captures.is_empty());
}

#[test]
fn callable_occurs_checks_and_argument_types_are_enforced() {
    rejects("fn bad(f) { f(f) } fn main(){0}", "infinite type");
    rejects("fn main(){let f=fn(x){x+1}; f(true)}", "E-TYPE");
    rejects("fn main(){1(2)}", "E-TYPE");
    rejects("fn main(){let f=fn(x){x}; f()}", "E-ARITY");
    rejects("fn main(){let f=fn(x){x}; f(1,2)}", "E-ARITY");
    rejects("fn main(){fn(x,x){x}}", "duplicate lambda");
    rejects("fn main(){let f=fn(x){x}; f==f}", "equality");
}

#[test]
fn captured_resources_and_loans_are_never_silently_copied() {
    rejects(
        "fn main(){let mut count=0; let f=fn(){count}; f()}",
        "E-FUNCTION-CAPTURE",
    );
    rejects(
        "fn bad(read value){fn(){value}} fn main(){0}",
        "E-FUNCTION-CAPTURE",
    );
    rejects(
        "fn main(){let owner=system_box_i32_abort(42); let f=fn(){box_read(owner)}; f()}",
        "E-FUNCTION-CAPTURE",
    );
    rejects("fn take(owner: SystemBoxI32, n){box_read(owner)+n} fn main(){let owner=system_box_i32_abort(1); let f=take(move owner); f(41)}","E-FUNCTION-CAPTURE");
    rejects(
        "fn bad(inout x){x<-1} fn main(){let f=bad; 0}",
        "E-FUNCTION-MODE",
    );
}

#[test]
fn native_values_and_incompatible_concrete_environments_are_explicit_errors() {
    rejects(
        "extern fn foreign(x:i32)->i32; fn main(){let f=foreign; f(42)}",
        "E-FUNCTION-FFI",
    );
    rejects("fn main(){let f=byte_len; f(\"x\")}", "E-FUNCTION-FFI");
    rejects(
        "fn main(){let f=if true {fn(x){x}} else {fn(x){x+1}}; f(41)}",
        "E-TYPE",
    );
    rejects(
        "fn first(x){x} fn second(x){x} fn main(){let f=if true {first} else {second}; f(42)}",
        "E-TYPE",
    );
}

#[test]
fn constructor_calls_do_not_capture_same_named_locals_but_values_do() {
    let program=parser::parse("type Option<a> =None|Some(a) fn main(){let mut Some=0; let f=fn(x){Some(x)}; match f(42){Some(x)=>x|None=>0}}").unwrap();
    let checked = check::analyze(&program).unwrap();
    let Expr::LetMut(_, _, body) = &program.functions[0].body else {
        panic!("missing outer binding")
    };
    let Expr::Let(_, lambda, _) = body.as_ref() else {
        panic!("missing closure")
    };
    assert!(checked.closure_info(lambda).unwrap().captures.is_empty());
    assert_eq!(
        rill::interpreter::run_reference(&program)
            .unwrap()
            .to_string(),
        "42"
    );
    assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "42");
    rejects(
        "type Option<a> =None|Some(a) fn main(){let mut Some=0;let f=fn(){Some};f()}",
        "E-FUNCTION-CAPTURE",
    );
}

#[test]
fn source_main_result_never_exposes_generated_environments() {
    rejects("fn main(){fn(x){x}}", "E-FUNCTION-RESULT");
    rejects(
        "type Holder<a> ={callback:a} fn main(){record Holder {callback=fn(x){x}}}",
        "E-FUNCTION-RESULT",
    );
}
