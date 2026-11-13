use rill::abi::{
    copy::{emit_copy_library, Artifact, Field, Schema, Value, Variant},
    Export,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

fn root(name: &str) -> Export {
    Export {
        source: name.into(),
        c_name: format!("rill_{name}"),
    }
}
fn emit(source: &str, roots: &[Export]) -> Result<Artifact, String> {
    emit_copy_library(&rill::parser::parse(source)?, roots)
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rill-copy-abi-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_c(artifact: &Artifact, caller: &str, expected: &[u8], instrument: bool) {
    let fixture = Fixture::new();
    let c = if instrument {
        artifact.c.replace(
            "static void rl_enter(void) {",
            "static int host_calls; static void rl_enter(void) { ++host_calls;",
        )
    } else {
        artifact.c.clone()
    };
    fs::write(fixture.0.join("library.c"), c + caller).unwrap();
    for optimization in ["-O0", "-O3"] {
        let executable = fixture.0.join("caller");
        let build = Command::new("cc")
            .args([
                "-std=c11",
                optimization,
                "-Werror=implicit-function-declaration",
            ])
            .arg(fixture.0.join("library.c"))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{optimization}: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let output = Command::new(&executable).output().unwrap();
        assert!(
            output.status.success(),
            "{optimization} status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, expected, "{optimization}");
        assert!(output.stderr.is_empty());
    }
}

fn point() -> Schema {
    Schema::Record {
        name: "Point".into(),
        fields: vec![
            Field {
                name: "x".into(),
                schema: Schema::I32,
            },
            Field {
                name: "ready".into(),
                schema: Schema::Bool,
            },
            Field {
                name: "speed".into(),
                schema: Schema::F32,
            },
        ],
    }
}
fn record(x: i32, ready: bool, speed: f32) -> Value {
    Value::Record(vec![
        ("x".into(), Value::I32(x)),
        ("ready".into(), Value::Bool(ready)),
        ("speed".into(), Value::F32(speed.to_bits())),
    ])
}

#[test]
fn public_copy_values_and_function_decoders_expose_checked_access() {
    let mut value = record(7, true, -0.0);
    assert_eq!(value.field("x").and_then(Value::as_i32), Some(7));
    assert_eq!(value.field("ready").and_then(Value::as_bool), Some(true));
    assert_eq!(
        value
            .field("speed")
            .and_then(Value::as_f32)
            .map(f32::to_bits),
        Some((-0.0_f32).to_bits())
    );
    *value.field_mut("x").unwrap() = Value::I32(9);
    assert_eq!(value.field("x").and_then(Value::as_i32), Some(9));
    assert_eq!(Value::Bool(false).as_i32(), None);
    assert_eq!(Value::I32(1).as_bool(), None);
    assert_eq!(Value::I32(1).as_f32(), None);
    assert!(value.field_mut("missing").is_none());

    let function = rill::abi::copy::Function {
        name: "rill_identity".into(),
        parameters: vec![Schema::F32],
        result: Schema::F32,
    };
    let argument = Value::F32(0x7fc1_2345);
    let bytes = function
        .encode_arguments(std::slice::from_ref(&argument))
        .unwrap();
    assert_eq!(
        function.decode_arguments(&bytes).unwrap().first(),
        Some(&argument)
    );
    assert!(function.decode_arguments(&[0, 0, 0]).is_err());
    assert_eq!(function.decode_result(&bytes).unwrap(), argument);
    assert!(function.decode_result(&[0, 0, 0]).is_err());
}

#[test]
fn portable_codec_uses_named_fields_tags_and_exact_scalar_bits() {
    let schema = point();
    let value = record(-2, true, -0.0);
    let bytes = schema.encode(&value).unwrap();
    assert_eq!(bytes, [254, 255, 255, 255, 1, 0, 0, 0, 128]);
    assert_eq!(schema.decode(&bytes).unwrap(), value);
    let state = Schema::Enum {
        name: "State".into(),
        variants: vec![
            Variant {
                name: "Idle".into(),
                fields: vec![],
            },
            Variant {
                name: "Run".into(),
                fields: vec![schema],
            },
        ],
    };
    let value = Value::Enum {
        variant: "Run".into(),
        fields: vec![value],
    };
    let bytes = state.encode(&value).unwrap();
    assert_eq!(&bytes[..4], &[1, 0, 0, 0]);
    assert_eq!(state.decode(&bytes).unwrap(), value);
    assert!(state.decode(&[2, 0, 0, 0]).is_err());
    for bits in [
        0, 0x80000000, 1, 0x7f800000, 0xff800000, 0x7fc12345, 0x7f800001,
    ] {
        assert_eq!(
            Schema::F32
                .decode(&Schema::F32.encode(&Value::F32(bits)).unwrap())
                .unwrap(),
            Value::F32(bits)
        );
    }
    assert!(Schema::Bool.decode(&[2]).is_err());
    assert!(Schema::I32.decode(&[0, 0, 0]).is_err());
    assert!(Schema::I32.decode(&[0, 0, 0, 0, 0]).is_err());
    assert!(point()
        .encode(&Value::Record(vec![("x".into(), Value::I32(1))]))
        .is_err());
    let mut fields = match record(1, false, 0.0) {
        Value::Record(fields) => fields,
        _ => unreachable!(),
    };
    fields[1].0 = "x".into();
    assert!(point().encode(&Value::Record(fields)).is_err());
}

#[test]
fn codec_rejects_bad_schemas_unknown_json_fields_and_depth_excess() {
    let duplicate = Schema::Record {
        name: "Bad".into(),
        fields: vec![
            Field {
                name: "x".into(),
                schema: Schema::I32,
            },
            Field {
                name: "x".into(),
                schema: Schema::I32,
            },
        ],
    };
    assert!(duplicate.validate().is_err());
    assert!(Schema::Enum {
        name: "Empty".into(),
        variants: vec![]
    }
    .validate()
    .is_err());
    let mut depth = Schema::I32;
    for _ in 0..16 {
        depth = Schema::Record {
            name: "Nested".into(),
            fields: vec![Field {
                name: "field".into(),
                schema: depth,
            }],
        };
    }
    depth.validate().unwrap();
    let text = serde_json::to_vec(&depth).unwrap();
    let round: Schema = serde_json::from_slice(&text).unwrap();
    assert_eq!(depth, round);
    depth = Schema::Record {
        name: "TooDeep".into(),
        fields: vec![Field {
            name: "field".into(),
            schema: depth,
        }],
    };
    assert!(depth.validate().is_err());
    assert!(serde_json::from_str::<Schema>(r#"{"kind":"i32","unexpected":0}"#).is_err());
    assert!(serde_json::from_str::<Value>(r#"{"kind":"i32","value":1,"unexpected":0}"#).is_err());
    let wide = Schema::Record {
        name: "Wide".into(),
        fields: (0..4096)
            .map(|n| Field {
                name: format!("f{n}"),
                schema: Schema::I32,
            })
            .collect(),
    };
    assert!(wide.validate().is_err());
}

const SOURCE: &str = r#"
type Point = { x:i32; ready:bool; speed:f32 }
type State = Idle | Run(Point)
fn update(state:State, delta:i32) -> State {
    match state {
        Idle => Run(record Point { x=delta; ready=true; speed=f32(delta) }) |
        Run(p) => Run(record Point { speed=f32_add(p.speed, f32(delta)); ready=not p.ready; x=p.x+delta })
    }
}
"#;

#[test]
fn c11_bridge_calls_once_and_rejects_malformed_input_before_entry() {
    let artifact = emit(SOURCE, &[root("update")]).unwrap();
    assert_eq!(artifact, emit(SOURCE, &[root("update")]).unwrap());
    assert!(!artifact.c.contains("int main("));
    assert_eq!(
        artifact.functions[0].parameters[0],
        Schema::Enum {
            name: "State".into(),
            variants: vec![
                Variant {
                    name: "Idle".into(),
                    fields: vec![]
                },
                Variant {
                    name: "Run".into(),
                    fields: vec![point()]
                }
            ]
        }
    );
    let input = Value::Enum {
        variant: "Run".into(),
        fields: vec![record(40, false, 0.5)],
    };
    let bytes = artifact.functions[0]
        .encode_arguments(&[input, Value::I32(2)])
        .unwrap();
    assert_eq!(
        artifact.functions[0]
            .decode_arguments(&bytes)
            .unwrap()
            .len(),
        2
    );
    let expected = artifact.functions[0]
        .result
        .encode(&Value::Enum {
            variant: "Run".into(),
            fields: vec![record(42, true, 2.5)],
        })
        .unwrap();
    let caller = format!(
        r#"
int main(void) {{
unsigned char input[] = {{{}}};
unsigned char out[64]; uint32_t n=999;
if (rill_update(input,sizeof(input),out,sizeof(out),&n)!=0 || n!={} || host_calls!=1) return 1;
if (fwrite(out,1,n,stdout)!=n) return 2;
if (rill_update(input,sizeof(input),out,1,&n)!=3 || n!=0 || host_calls!=1) return 3;
input[8]=2;
if (rill_update(input,sizeof(input),out,sizeof(out),&n)!=2 || host_calls!=1) return 4;
input[8]=0; input[0]=2;
if (rill_update(input,sizeof(input),out,sizeof(out),&n)!=2 || host_calls!=1) return 5;
input[0]=1;
if (rill_update(input,sizeof(input)-1,out,sizeof(out),&n)!=2 || host_calls!=1) return 6;
if (rill_update(input,sizeof(input),NULL,sizeof(out),&n)!=3 || host_calls!=1) return 7;
if (rill_update(input,sizeof(input),out,sizeof(out),NULL)!=3 || host_calls!=1) return 8;
if (rill_abi_invoke_copy_v1(10,input,sizeof(input),out,sizeof(out),&n)!=1 || host_calls!=1) return 9;
return 0;
}}
"#,
        bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(","),
        expected.len()
    );
    run_c(&artifact, &caller, &expected, true);
    let executable=rill::parser::parse(&(SOURCE.to_owned()+"fn main(){update(Run(record Point{x=40;ready=false;speed=f32_div(f32(1),f32(2))}),2)}")).unwrap();
    let interpreted = rill::interpreter::run(&executable).unwrap();
    assert_eq!(
        interpreted.to_string(),
        "Run(Point { x = 42; ready = true; speed = 2.5 })"
    );
}

#[test]
fn pure_f32_identity_transports_bits_without_opening_the_scalar_i32_abi() {
    let source = "fn identity(value:f32)->f32{value}";
    let roots = [root("identity")];
    let artifact = emit(source, &roots).unwrap();
    assert!(rill::abi::emit_i32_library(&rill::parser::parse(source).unwrap(), &roots).is_err());
    run_c(
        &artifact,
        r#"
int main(void) {
uint32_t cases[]={0,0x80000000u,1,0x7f800000u,0xff800000u,0x7fc12345u};
for (unsigned i=0;i<sizeof(cases)/sizeof(cases[0]);++i) {
unsigned char input[4],out[4]; uint32_t c=0,n=0;
rl_copy_write_word(input,&c,cases[i]);
if (rill_identity(input,4,out,4,&n)!=0 || n!=4 || memcmp(input,out,4)!=0) return 1;
}
return 0;
}
"#,
        b"",
        false,
    );
}

#[test]
fn nested_concrete_generic_storage_and_multiple_zero_argument_roots_use_real_ids() {
    let declarations = [
        "type Slot<a> = Empty | Occupied(a)",
        "type Inner = { flag:bool; value:Slot<i32> }",
        "type Outer = { inner:Inner; last:i32 }",
        "fn identity(x){x}",
        "fn yes()->bool{identity(true)}",
        "fn update(state:Outer)->Outer{identity(state)}",
        "fn initial()->Outer{record Outer{last=9;inner=record Inner{value=Occupied(42);flag=true}}}",
    ];
    let roots = [root("initial"), root("update"), root("yes")];
    let artifact = emit(&declarations.join("\n"), &roots).unwrap();
    let initial = Value::Record(vec![
        (
            "inner".into(),
            Value::Record(vec![
                ("flag".into(), Value::Bool(true)),
                (
                    "value".into(),
                    Value::Enum {
                        variant: "Occupied".into(),
                        fields: vec![Value::I32(42)],
                    },
                ),
            ]),
        ),
        ("last".into(), Value::I32(9)),
    ]);
    let expected = artifact.functions[0].result.encode(&initial).unwrap();
    let mut reordered = declarations.to_vec();
    reordered.reverse();
    let reordered = emit(&reordered.join("\n"), &roots).unwrap();
    assert_eq!(artifact.manifest, reordered.manifest);
    for artifact in [artifact, reordered] {
        run_c(
            &artifact,
            r#"
int main(void) {
unsigned char first[64],second[64],yes[1]; uint32_t n=0,m=0,b=0;
if (rill_initial(NULL,0,first,sizeof(first),&n)!=0) return 1;
if (rill_update(first,n,second,sizeof(second),&m)!=0 || m!=n || memcmp(first,second,n)!=0) return 2;
if (rill_yes(NULL,0,yes,1,&b)!=0 || b!=1 || yes[0]!=1) return 3;
if (fwrite(second,1,m,stdout)!=m) return 4;
return 0;
}
"#,
            &expected,
            false,
        );
    }
}

#[test]
fn public_byte_header_supports_an_independent_c_translation_unit() {
    let artifact = emit("fn update(value:bool)->bool{not value}", &[root("update")]).unwrap();
    assert!(artifact
        .header
        .contains("#define RILL_COPY_WIRE_ABI_VERSION 1"));
    assert!(!artifact.header.contains("rl_fn_"));
    let fixture = Fixture::new();
    fs::write(fixture.0.join("library.c"), artifact.c).unwrap();
    fs::write(fixture.0.join("library.h"), artifact.header).unwrap();
    fs::write(
        fixture.0.join("caller.c"),
        r#"
#include "library.h"
#include "library.h"
#include <stddef.h>
int main(void) {
unsigned char input[1]={0},out[1]={0}; uint32_t length=0;
if (rill_update(input,1,out,1,&length)!=0 || length!=1 || out[0]!=1) return 1;
input[0]=1;
if (rill_update(input,1,out,1,&length)!=0 || length!=1 || out[0]!=0) return 2;
input[0]=2;
if (rill_update(input,1,out,1,&length)!=2 || length!=0) return 3;
return 0;
}
"#,
    )
    .unwrap();
    for optimization in ["-O0", "-O3"] {
        let executable = fixture.0.join("caller");
        let output = Command::new("cc")
            .args([
                "-std=c11",
                optimization,
                "-Werror=implicit-function-declaration",
            ])
            .arg(fixture.0.join("library.c"))
            .arg(fixture.0.join("caller.c"))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(Command::new(&executable).status().unwrap().success());
    }
}

#[test]
fn every_source_declaration_is_checked_and_foreign_owners_or_unknown_storage_reject() {
    for source in [
        "fn update(x:i32)->i32{x} fn unused()->i32{false}",
        "fn update(x:i32)->i32{x} fn unused(x:ptr<Thing>)->i32{1}",
        "fn update(x:i32)->i32{x} type Unused={field:u32}",
        "fn update(x:i32)->i32{x} fn unused(x:SystemBoxI32)->i32{1}",
        "fn update(read x:i32)->i32{x}",
        "fn update(x){x}",
        "fn update(x:unit)->unit{x}",
        "fn update(x:str)->str{x}",
        "fn update(x:bool)->bool{x} fn main(){1}",
        "extern fn foreign(x:i32)->i32; fn update(x:i32)->i32{x}",
    ] {
        let parsed = rill::parser::parse(source).unwrap();
        assert!(
            emit_copy_library(&parsed, &[root("update")]).is_err(),
            "accepted {source}"
        );
    }
    assert!(emit(
        "fn update(x:i32)->i32{x} fn unused(x:f32)->f32{x}",
        &[root("update")]
    )
    .is_ok());
}

#[test]
fn module_ids_and_declaration_order_do_not_brand_structural_schemas() {
    let fixture = Fixture::new();
    let entry = fixture.0.join("entry.rill");
    let mut original = None;
    for (module,source) in [
        ("alpha","export type Point={x:i32;ready:bool;speed:f32} export fn update(x:Point)->Point{x}"),
        ("beta","type Extra=No|Yes export type Point={x:i32;ready:bool;speed:f32} export fn unused()->i32{1} export fn update(x:Point)->Point{x}"),
    ] {
        let overlays=BTreeMap::from([(entry.clone(),format!("import {module}; fn host(x:{module}.Point)->{module}.Point{{{module}.update(x)}}")),(fixture.0.join(format!("{module}.rill")),source.into())]);
        let loaded=rill::modules::compile::load_library_with_sources_and_overlays(&entry,&overlays).unwrap();
        let symbol=loaded.symbols().iter().find(|symbol|symbol.symbol.name=="host").unwrap();
        let rill::source_map::AstNode::Function(function)=loaded.node(&symbol.target).unwrap() else {panic!("function")};
        let artifact=emit_copy_library(loaded.program(),&[Export{source:function.name.clone(),c_name:"rill_host".into()}]).unwrap();
        assert_eq!(artifact.functions[0].parameters,[point()]);
        assert!(!artifact.manifest.contains("M$"));
        if let Some(original)=&original { assert_eq!(&artifact.manifest,original); } else {original=Some(artifact.manifest);}
    }
}
