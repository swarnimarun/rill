use rill::{
    backend, check, interpreter,
    lsp::{analysis, text},
    modules::compile::load_program_with_sources,
    parser,
    source_map::{AstPath, AstStep},
    syntax::{AstDeclaration, SyntaxTree},
    typed,
};
use std::{collections::BTreeMap, fs, process::Command};

#[test]
fn capacity_literals_admit_only_closed_bounded_lengths_and_keep_nominal_names() {
    for bad in [
        "bytes<257>",
        "bytes<-1>",
        "bytes<1,2>",
        "bytes<1+1>",
        "Other<2>",
        "bytes<>",
    ] {
        let source = format!("fn take(x:{bad}) {{ x }} fn main() {{ 0 }}");
        assert!(parser::parse(&source).is_err(), "{source}");
    }
    for source in [
        "fn take(x:bytes<0>) { x } fn main(){take(bytes_new(1))}",
        "fn wrong()->bytes<255>{ bytes_new(256) } fn main(){0}",
        "type B = B(bytes<2>) fn main(){B(bytes_new(3))}",
    ] {
        let error = rill::compile(source).unwrap_err();
        assert!(
            error.starts_with("E-TYPE:") && error.contains("bytes<"),
            "{error}"
        );
    }
    for source in [
        "fn dormant(x:ptr<bytes<2>>){0} fn main(){0}",
        "extern fn foreign(x:bytes<2>)->i32; fn main(){0}",
        "extern fn foreign(x:ptr<bytes<2>>)->i32; fn main(){0}",
    ] {
        let error = rill::compile(source).unwrap_err();
        assert!(!error.contains("$bytes"), "{error}");
    }
    let source = "type bytes<a> = Legacy(a) fn both(x:bytes<i32>, b:bytes<4>){match x{Legacy(n)=>n+bytes_len(b)}} fn main(){both(Legacy(38),bytes_new(4))}";
    assert_eq!(
        interpreter::run(&parser::parse(source).unwrap())
            .unwrap()
            .to_string(),
        "42"
    );
    let source = "fn take(x:bytes<000>)->bytes<0>{x} fn main(){bytes_len(take(bytes_new(0)))}";
    assert_eq!(
        interpreter::run(&parser::parse(source).unwrap())
            .unwrap()
            .to_string(),
        "0"
    );
}

#[test]
fn nested_capacity_matches_across_module_schema_layout_origins_lsp_and_c() {
    let dir = std::env::temp_dir().join(format!("rill-fixed-byte-types-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let library = "export type Packet = { tag: i32; body: bytes<256> }
export type Maybe<a> = Some(a) | None
export fn payload(x:Packet)->Maybe<bytes<256>>{Some(x.body)}
export fn replace(inout slot:bytes<256>, n:i32){slot <- bytes_set(slot,255,n)}";
    let entry = "import packet as p;
fn main() {
    let mut slot = bytes_new(256);
    p.replace(inout slot,41);
    let frozen = record p.Packet { tag = 1; body = slot };
    p.replace(inout slot,0);
    match p.payload(frozen) { p.Some(b) => frozen.tag + bytes_get(b,255) | p.None => 0 }
}";
    fs::write(dir.join("packet.rill"), library).unwrap();
    let path = dir.join("main.rill");
    fs::write(&path, entry).unwrap();
    let tree = SyntaxTree::parse(library);
    let path_in_type = AstPath::root(AstDeclaration::Function(0))
        .child(AstStep::FunctionResult)
        .child(AstStep::TypeArgument(0));
    let origin = tree
        .ast_source_map()
        .unwrap()
        .origin(&path_in_type)
        .unwrap()
        .span;
    assert_eq!(&library[origin.byte_range()], "bytes<256>");
    let loaded = load_program_with_sources(&path).unwrap();
    let program = loaded.program();
    let reference = interpreter::run_reference(program).unwrap();
    assert_eq!(reference.to_string(), "42");
    let typed = typed::lower_main(&check::analyze(program).unwrap()).unwrap();
    assert_eq!(typed::evaluate(&typed).unwrap(), reference);
    for c in [
        backend::emit(program).unwrap(),
        backend::emit_reference(program).unwrap(),
    ] {
        let source = dir.join("main.c");
        fs::write(&source, c).unwrap();
        for opt in ["-O0", "-O3"] {
            let exe = dir.join("test");
            let build = Command::new("cc")
                .args(["-std=c11", "-pedantic-errors", opt])
                .arg(&source)
                .arg("-o")
                .arg(&exe)
                .output()
                .unwrap();
            assert!(
                build.status.success(),
                "{}",
                String::from_utf8_lossy(&build.stderr)
            );
            let run = Command::new(exe).output().unwrap();
            assert!(run.status.success());
            assert_eq!(run.stdout, b"42\n");
        }
    }
    let mapped = typed::provenance::lower_main_with_copy_node_origins(loaded).unwrap();
    assert_eq!(typed::evaluate(mapped.typed()).unwrap(), reference);

    let uri = text::path_to_uri(&path).unwrap();
    let hover =
        analysis::hover(&uri, entry, entry.rfind("slot").unwrap(), &BTreeMap::new()).unwrap();
    assert!(hover.to_string().contains("bytes<256>"), "{hover}");
    let functions = analysis::document_symbols(library);
    assert!(!serde_json::to_string(&functions)
        .unwrap()
        .contains("$bytes"));
    fs::remove_dir_all(dir).unwrap();
}
