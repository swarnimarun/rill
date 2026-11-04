//! Independent optional-origin API semantic-ordering executable probes.
use rill::{check, parser, typed};
use std::{fs, io::Write, path::PathBuf};
fn main() {
    let root = PathBuf::from(
        std::env::var_os("K4_ORIGIN_OUTPUT").expect("fresh approved RAM output directory required"),
    );
    fs::create_dir(&root).unwrap();
    for (name,source,semantic_ok) in [
        ("discard_call","type O<a> = None|Some(a) fn absent(){None} fn main(){let weak=absent();42}",true),
        ("internal_only","type O<a> = None|Some(a) fn main(){let weak=if true{None}else{None};42}",true),
        ("suffix_only","type O<a> = None|Some(a) fn spin(n:i32){spin(n)+1} fn ignore(a,b){42} fn main(){ignore(spin(0),None)}",true),
        ("observed_unknown","type O<a> = None|Some(a) fn absent(){None} fn main(){absent()}",false),
        ("mutable_unknown","type O<a> = None|Some(a) fn absent(){None} fn main(){let mut weak=absent();42}",false),
        ("hidden_read","type O<a> = None|Some(a) fn absent(){None} fn inspect(x){42} fn main(){let weak=absent();if false{inspect(weak)}else{42}}",false),
        ("bad_unused_body","type O<a> = None|Some(a) fn bad(){1+true} fn absent(){None} fn main(){let weak=absent();42}",false),
        ("bad_weak_classes","type O<a> = None|Some(a) fn absent(){None} fn ints(x:O<i32>){()} fn bools(x:O<bool>){()} fn main(){let weak=absent();ints(weak);bools(weak);42}",false),
    ]{
        let ast=parser::parse(source).unwrap();
        let semantic=check::analyze(&ast).and_then(|c|typed::lower_main(&c));
        assert_eq!(semantic.is_ok(),semantic_ok,"{name}: {semantic:?}");
        let path=root.join(format!("{name}.rill"));fs::OpenOptions::new().write(true).create_new(true).open(&path).unwrap().write_all(source.as_bytes()).unwrap();
        let sources=match rill::modules::compile::load_program_with_sources(&path){
            Ok(sources)=>sources,
            Err(error)=>{assert!(!semantic_ok,"{name}: {error}");assert!(error.to_string().contains(semantic.as_ref().unwrap_err().as_str()),"{name}: {error}");println!("PASS independent module-loader semantic rejection {name}");continue;}
        };
        let errors=[typed::provenance::lower_main_with_function_origins(sources.clone()).unwrap_err(),typed::provenance::nodes::lower_main_with_copy_node_origins(sources).unwrap_err()];
        for error in errors{
            if semantic_ok{assert!(error.starts_with("E-TYPED-UNSUPPORTED:")&&error.contains("control results"),"{name}: {error}");}
            else{assert_eq!(error,semantic.as_ref().unwrap_err().as_str(),"{name}");}
        }
        println!("PASS independent origin ordering {name}");
    }
}
