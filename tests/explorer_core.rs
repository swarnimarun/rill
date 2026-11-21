use rill::explorer::{inspect, MAX_SOURCE_BYTES};
use serde_json::{json, Value};

fn view(source: &str, stage: &str) -> Value {
    inspect(&json!({"source": source, "view": stage}))
}

fn success(response: Value) -> String {
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["diagnostics"], "");
    response["output"].as_str().unwrap().to_owned()
}

fn failure(response: Value) -> String {
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(response["output"], "");
    response["diagnostics"].as_str().unwrap().to_owned()
}

#[test]
fn frontend_types_and_host_execution_share_the_language() {
    let source = include_str!("../examples/conditional-pattern.rill");
    assert_eq!(success(view(source, "run")), "42");
    assert_eq!(success(view(source, "check")), "Type check passed.");
    let types = success(view(source, "types"));
    assert!(
        types.contains("fn answer(read input: Option<i32>) -> i32"),
        "{types}"
    );
    assert!(types.contains("fn main() -> i32"));
    assert_eq!(types.lines().count(), 2);
    assert!(success(view(source, "tokens")).contains("Fn"));
    assert!(success(view(source, "ast")).contains("Match"));
    assert!(success(view(source, "c")).contains("int main("));
    assert_eq!(
        success(view(source, "format")),
        rill::formatter::format(source).unwrap()
    );
    assert!(success(view("fn id(x) { x }", "types")).contains("fn id(x: 'a) -> 'a"));
    assert!(failure(view("fn id(x) { x }", "run")).contains("missing main"));
    let invalid = "fn main() { true + 1 }";
    assert!(success(view(invalid, "ast")).contains("Binary"));
    for stage in ["types", "check", "c", "run"] {
        assert!(failure(view(invalid, stage)).contains("E-TYPE"));
    }
    let malformed = "fn main() { let x = 4\n x }";
    assert!(failure(view(malformed, "check")).contains("E-PARSE-BINDING-SEMICOLON"));
    assert!(success(view(malformed, "tokens")).contains("Let"));
}

#[test]
fn live_owner_and_kernel_programs_use_the_verified_paths() {
    let owner =
        "fn main() { let b = system_box_i32_abort(5); let moved = move b; box_read(moved) }";
    assert_eq!(success(view(owner, "run")), "5");
    assert!(failure(view(
        &owner.replace("box_read(moved)", "box_read(b)"),
        "run"
    ))
    .contains("E-"));
    let source = include_str!("../examples/gpu-ir/clamp.rill");
    assert!(success(view(source, "rir")).starts_with("rir 0\n"));
    let llvm = success(view(source, "nvptx"));
    assert!(llvm.contains("nvptx64-nvidia-cuda"));
    assert!(success(view(source, "vulkan")).contains("spirv1.5-unknown-vulkan1.2-compute"));
    let request = json!({"source": source.replace("clamp", "clip"), "view": "ir-eval", "entry": "clip", "inputs": [i32::MIN,-1,0,42,i32::MAX]});
    assert_eq!(success(inspect(&request)), "0\n0\n0\n42\n2147483647\n");
    let unsupported = "fn clamp(x:i32) { x + 1 }";
    assert!(failure(view(unsupported, "nvptx")).contains("playground.rill:1:"));
    assert!(failure(view(unsupported, "rir")).contains("E-GPU-PROFILE"));
    assert!(failure(view("fn other(x:i32) { x }", "vulkan")).contains("E-GPU-ENTRY"));
    for inputs in [
        json!(4),
        json!([2147483648_i64]),
        json!([-1.5]),
        json!(["8"]),
        json!(vec![0; 257]),
    ] {
        let mut bad = request.clone();
        bad["inputs"] = inputs;
        assert!(failure(inspect(&bad)).contains("E-EXPLORER"));
    }
}

#[test]
fn buffers_have_a_closed_module_and_native_boundary() {
    let std = "import std.option_ops as ops; fn main() { 7 }";
    assert_eq!(success(view(std, "run")), "7");
    let local = "import helper as h; fn main() { 1 }";
    assert!(failure(view(local, "check")).contains("local import helper"));
    assert!(failure(view("import std.missing; fn main(){0}", "check")).contains("E-MODULE-STDLIB"));
    for text in [
        "@include(\"math.h\"); fn main(){1}",
        "@pkg_config(\"sdl3\"); fn main(){1}",
        "@link(\"m\"); fn main(){1}",
        "extern fn dormant() -> i32; fn main() { 1 }",
    ] {
        for stage in ["check", "run", "c", "types", "rir"] {
            assert!(failure(view(text, stage)).contains("E-EXPLORER-NATIVE"));
        }
    }
    assert_eq!(
        success(view("fn main() { \"@link extern\" }", "run")),
        "@link extern"
    );
    assert!(failure(inspect(&json!({"source":false, "view":"run"}))).contains("source must"));
    assert!(failure(view(&" ".repeat(MAX_SOURCE_BYTES + 1), "run")).contains("E-EXPLORER-LIMIT"));
}

#[test]
fn host_ir_shows_verified_instances_places_and_cleanup() {
    let scalar = "fn twice(x) { x + x } fn main() { twice(21) }";
    let dump = success(view(scalar, "typed-ir"));
    for part in [
        "Verified host IR",
        "entry f",
        " twice(",
        "x: i32",
        " main() -> i32",
        "Call f",
        "Binary Add : i32",
        "Read %",
    ] {
        assert!(dump.contains(part), "{dump}");
    }

    let owner =
        "fn main() { let b = system_box_i32_abort(5); let moved = move b; box_read(moved) }";
    let dump = success(view(owner, "typed-ir"));
    for part in [
        "OwnerLet",
        "Alloc box",
        "Move %",
        "OwnerRead",
        "scope cleanup",
        "DropWholeOwner",
    ] {
        assert!(dump.contains(part), "{dump}");
    }
    let unknown_layout = "type O<a> = None | Some(a) fn main() { None }";
    assert_eq!(success(view(unknown_layout, "check")), "Type check passed.");
    assert!(failure(view(unknown_layout, "typed-ir")).contains("E-TYPED-UNSUPPORTED"));
    assert!(failure(view("fn helper(x:i32) { x }", "typed-ir")).contains("missing main"));
}
