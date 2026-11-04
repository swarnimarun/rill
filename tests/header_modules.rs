use rill::modules::{self, compile, headers};
use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
}

fn search_snapshot(
    fixture: &Fixture,
    flags: &[OsString],
    positive: &[PathBuf],
) -> headers::Snapshot {
    let mut dependencies = positive.to_vec();
    dependencies.extend(headers::search_dependencies(flags, positive).unwrap());
    headers::prepare(
        &fixture.graph(),
        "@entry",
        "export extern fn native_add(a: i32, b: i32) -> i32;",
        &dependencies,
    )
    .unwrap()
}

#[test]
fn prepared_interface_cannot_bless_a_header_changed_during_the_build() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
    let header = fixture.header("api.h", "int native_add(int,int);\n");
    let snapshot = headers::prepare(
        &fixture.graph(),
        "@entry",
        "export extern fn native_add(a: i32, b: i32) -> i32;",
        std::slice::from_ref(&header),
    )
    .unwrap();
    fs::write(header, "float native_add(float,float);\n").unwrap();
    assert!(!snapshot.publish().unwrap());
    assert!(headers::apply_cached(&mut fixture.graph())
        .unwrap()
        .is_empty());
}

#[test]
fn newly_available_optional_headers_invalidate_the_published_interface() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
    let header = fixture.header("api.h", "#if __has_include(\"optional.h\")\n#include \"optional.h\"\n#else\nint native_add(int,int);\n#endif\n");
    assert!(search_snapshot(&fixture, &[], &[header]).publish().unwrap());
    assert_eq!(
        headers::apply_cached(&mut fixture.graph()).unwrap().len(),
        1
    );
    fixture.header("optional.h", "float native_add(float,float);\n");
    assert!(headers::apply_cached(&mut fixture.graph())
        .unwrap()
        .is_empty());
}

#[test]
fn an_earlier_header_or_previously_missing_search_directory_invalidates_the_interface() {
    for missing in [false, true] {
        let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
        let early = fixture.root.join("early");
        let late = fixture.root.join("late");
        if !missing {
            fs::create_dir(&early).unwrap();
        }
        fs::create_dir(&late).unwrap();
        let header = fixture.header("late/api.h", "int native_add(int,int);\n");
        let flags = [
            OsString::from("-I"),
            early.as_os_str().to_owned(),
            OsString::from("-I"),
            late.as_os_str().to_owned(),
        ];
        assert!(search_snapshot(&fixture, &flags, &[header])
            .publish()
            .unwrap());
        if missing {
            fs::create_dir(&early).unwrap();
        }
        fixture.header("early/api.h", "float native_add(float,float);\n");
        assert!(headers::apply_cached(&mut fixture.graph())
            .unwrap()
            .is_empty());
    }
}

#[test]
fn creating_a_build_output_in_the_search_tree_skips_stale_publication() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
    let header = fixture.header("api.h", "int native_add(int,int);\n");
    let snapshot = search_snapshot(&fixture, &[], &[header]);
    fixture.header("program", "compiled native output");
    assert!(!snapshot.publish().unwrap());
}

#[test]
fn search_dependencies_accept_joined_and_separate_flags_and_prune_child_directories() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { 0 }");
    let parent = fixture.root.join("parent");
    let child = parent.join("child");
    let flags = [
        OsString::from("-I"),
        parent.as_os_str().to_owned(),
        OsString::from(format!("-isystem{}", child.display())),
        OsString::from("-iquote"),
        fixture.root.join("quoted").into_os_string(),
        OsString::from(format!(
            "-idirafter{}",
            fixture.root.join("after").display()
        )),
        OsString::from("-resource-dir"),
        fixture.root.join("resource").into_os_string(),
        OsString::from(format!(
            "-resource-dir={}",
            fixture.root.join("joined-resource").display()
        )),
    ];
    let dependencies = headers::search_dependencies(&flags, &[]).unwrap();
    assert!(dependencies.contains(&parent));
    assert!(!dependencies.contains(&child));
    assert!(dependencies.contains(&fixture.root.join("quoted")));
    assert!(dependencies.contains(&fixture.root.join("after")));
    assert!(dependencies.contains(&fixture.root.join("resource/include")));
    assert!(dependencies.contains(&fixture.root.join("joined-resource/include")));
    assert!(headers::search_dependencies(&["-isystem".into()], &[]).is_err());
}

#[test]
fn parent_directory_search_paths_are_not_lexically_pruned() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
    let project = fixture.root.join("project");
    let outside = fixture.root.join("outside");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&outside).unwrap();
    let header = fixture.header("project/api.h", "#if __has_include(<optional.h>)\n#include <optional.h>\n#else\nint native_add(int,int);\n#endif\n");
    let escaped = project.join("../outside");
    let flags = [
        OsString::from("-I"),
        project.into_os_string(),
        OsString::from("-I"),
        escaped.as_os_str().to_owned(),
    ];
    assert!(
        headers::search_dependencies(&flags, std::slice::from_ref(&header))
            .unwrap()
            .contains(&escaped)
    );
    assert!(search_snapshot(&fixture, &flags, &[header])
        .publish()
        .unwrap());
    fixture.header("outside/optional.h", "float native_add(float,float);\n");
    assert!(headers::apply_cached(&mut fixture.graph())
        .unwrap()
        .is_empty());
}

#[test]
fn sysroot_relative_search_directories_track_new_optional_headers() {
    for sysroot_flag in ["-isysroot", "--sysroot"] {
        let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
        let sdk = fixture.root.join("sdk");
        fs::create_dir_all(sdk.join("late")).unwrap();
        let header = fixture.header("sdk/late/api.h", "#if __has_include(<optional.h>)\n#include <optional.h>\n#else\nint native_add(int,int);\n#endif\n");
        let early = sdk.join("early");
        let flags = [
            sysroot_flag.into(),
            sdk.into_os_string(),
            "-I=/early".into(),
        ];
        assert!(
            headers::search_dependencies(&flags, std::slice::from_ref(&header))
                .unwrap()
                .contains(&early)
        );
        assert!(search_snapshot(&fixture, &flags, &[header])
            .publish()
            .unwrap());
        fs::create_dir(&early).unwrap();
        fixture.header("sdk/early/optional.h", "float native_add(float,float);\n");
        assert!(headers::apply_cached(&mut fixture.graph())
            .unwrap()
            .is_empty());
    }
}

#[cfg(unix)]
#[test]
fn hardlink_identity_changes_invalidate_positive_and_previously_skipped_headers() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
    let first = fixture.header("api.h", "#pragma once\nint native_add(int,int);\n");
    let second = fixture.root.join("duplicate.h");
    fs::hard_link(&first, &second).unwrap();
    assert!(search_snapshot(&fixture, &[], std::slice::from_ref(&first))
        .publish()
        .unwrap());
    let positive = headers::InputState::capture(std::slice::from_ref(&second)).unwrap();
    let bytes = fs::read(&second).unwrap();
    fs::remove_file(&second).unwrap();
    fs::write(&second, bytes).unwrap();
    assert!(!positive.current());
    assert!(headers::apply_cached(&mut fixture.graph())
        .unwrap()
        .is_empty());
}

#[test]
fn authoritative_input_state_survives_wire_roundtrip_and_rejects_generation_races() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
    let header = fixture.header("api.h", "int native_add(int,int);\n");
    let state = headers::capture_inputs(&[], std::slice::from_ref(&header), None).unwrap();
    let decoded = headers::InputState::from_wire(&state.to_wire().unwrap()).unwrap();
    assert!(decoded.current());
    assert!(decoded.covers(std::slice::from_ref(&header)));
    assert!(!decoded.covers(&[fixture.root.join("not-captured.h")]));
    fs::write(&header, "float native_add(float,float);\n").unwrap();
    assert!(!decoded.current());
    assert!(headers::prepare_with_state(
        &fixture.graph(),
        "@entry",
        "export extern fn native_add(a: i32,b: i32) -> i32;",
        &decoded
    )
    .is_err());
}

#[test]
fn input_state_wire_rejects_unknown_fields_duplicates_invalid_paths_and_bounds() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { 0 }");
    let header = fixture.header("api.h", "int native_add(int,int);\n");
    let state = headers::capture_inputs(&[], &[header], None).unwrap();
    let valid: serde_json::Value = serde_json::from_slice(&state.to_wire().unwrap()).unwrap();
    let mut invalid = valid.clone();
    invalid["unexpected"] = true.into();
    assert!(headers::InputState::from_wire(&serde_json::to_vec(&invalid).unwrap()).is_err());
    let mut invalid = valid.clone();
    let duplicate = invalid["dependencies"][0].clone();
    invalid["dependencies"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert!(headers::InputState::from_wire(&serde_json::to_vec(&invalid).unwrap()).is_err());
    let mut invalid = valid.clone();
    invalid["dependencies"][0]["extra"] = true.into();
    assert!(headers::InputState::from_wire(&serde_json::to_vec(&invalid).unwrap()).is_err());
    let mut invalid = valid.clone();
    invalid["dependencies"][0]["bytes"] = (33u64 * 1024 * 1024).into();
    assert!(headers::InputState::from_wire(&serde_json::to_vec(&invalid).unwrap()).is_err());
    #[cfg(unix)]
    {
        let mut invalid = valid.clone();
        invalid["dependencies"][0]["path"] =
            serde_json::json!([114, 101, 108, 97, 116, 105, 118, 101]);
        assert!(headers::InputState::from_wire(&serde_json::to_vec(&invalid).unwrap()).is_err());
        let mut invalid = valid.clone();
        invalid["dependencies"][0]["canonical"] = serde_json::json!([47, 0]);
        assert!(headers::InputState::from_wire(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    assert!(headers::InputState::from_wire(&vec![b' '; 2 * 1024 * 1024 + 1]).is_err());
}

#[cfg(unix)]
#[test]
fn directory_symlinks_are_tracked_and_cycles_terminate() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
    let first = fixture.root.join("first");
    let second = fixture.root.join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let header = fixture.header("first/api.h", "int native_add(int,int);\n");
    fixture.header("second/api.h", "int native_add(int,int);\n");
    symlink(&first, first.join("cycle")).unwrap();
    let search = fixture.root.join("search");
    symlink(&first, &search).unwrap();
    let flags = [OsString::from("-I"), search.as_os_str().to_owned()];
    assert!(search_snapshot(&fixture, &flags, &[header])
        .publish()
        .unwrap());
    assert_eq!(
        headers::apply_cached(&mut fixture.graph()).unwrap().len(),
        1
    );
    fs::remove_file(&search).unwrap();
    symlink(&second, &search).unwrap();
    assert!(headers::apply_cached(&mut fixture.graph())
        .unwrap()
        .is_empty());
}

#[test]
fn search_directory_depth_is_bounded() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
    let header = fixture.header("api.h", "int native_add(int,int);\n");
    let mut nested = fixture.root.clone();
    for _ in 0..65 {
        nested.push("d");
        fs::create_dir(&nested).unwrap();
    }
    let mut dependencies = vec![header];
    dependencies.extend(headers::search_dependencies(&[], &dependencies).unwrap());
    assert!(headers::prepare(
        &fixture.graph(),
        "@entry",
        "export extern fn native_add(a: i32,b: i32) -> i32;",
        &dependencies
    )
    .unwrap_err()
    .message
    .contains("traversal limit"));
}

#[cfg(unix)]
#[test]
fn shared_directory_symlink_dags_do_not_repeat_traversal_exponentially() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
    for level in 0..12 {
        fs::create_dir(fixture.root.join(format!("level{level}"))).unwrap();
    }
    for level in 0..11 {
        for branch in 0..4 {
            symlink(
                fixture.root.join(format!("level{}", level + 1)),
                fixture.root.join(format!("level{level}/branch{branch}")),
            )
            .unwrap();
        }
    }
    let header = fixture.header("level0/api.h", "int native_add(int,int);\n");
    let mut dependencies = vec![header];
    dependencies.extend(headers::search_dependencies(&[], &dependencies).unwrap());
    let snapshot = headers::prepare(
        &fixture.graph(),
        "@entry",
        "export extern fn native_add(a: i32,b: i32) -> i32;",
        &dependencies,
    )
    .unwrap();
    assert!(snapshot.publish().unwrap());
    assert_eq!(
        headers::apply_cached(&mut fixture.graph()).unwrap().len(),
        1
    );
}
impl Fixture {
    fn new(source: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rill-header-modules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("main.rill"), source).unwrap();
        Self { root }
    }
    fn graph(&self) -> modules::ModuleGraph {
        modules::load(&self.root.join("main.rill")).unwrap()
    }
    fn header(&self, name: &str, source: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, source).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn request_discovery_respects_lexical_bindings_constructors_and_field_labels() {
    let fixture = Fixture::new("@include(\"api.h\"); type Item = { field: i32 } type Choice = Some(i32) | None fn identity(value) { value } fn main() { let local = 3; let holder = record Item { field = local }; let closure = local; let result = match Some(1) { Some(bound) => bound | None => 0 }; identity(holder.field) + closure + result + native_value + native_add(20,22) }");
    let requests = headers::requests(&fixture.graph()).unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].names, ["native_add", "native_value"]);
}

#[test]
fn imported_consumer_requests_are_owned_by_the_header_module() {
    let fixture = Fixture::new("import api as c; type Item = { count: i32 } fn main() { let c = record Item { count = 20 }; c.count + c.native_add(20,2) }");
    fs::write(
        fixture.root.join("api.rill"),
        "@include(\"api.h\"); export extern const EXISTING: i32;",
    )
    .unwrap();
    let requests = headers::requests(&fixture.graph()).unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].module, "api");
    assert_eq!(requests[0].names, ["native_add"]);
    assert_eq!(requests[0].native.includes, ["api.h"]);
}

#[test]
fn nested_bindings_do_not_leak_into_later_header_reference_discovery() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { let run = { let native_add = 1; native_add }; let value = { let native_value = 2; native_value }; native_add(value, native_value) }");
    let names = &headers::requests(&fixture.graph()).unwrap()[0].names;
    assert_eq!(names, &["native_add", "native_value"]);
}

#[test]
fn apply_exports_generated_declarations_and_preserves_native_inputs() {
    let fixture = Fixture::new("import api; fn main() { api.native_add(20, api.API_VALUE) }");
    fs::write(fixture.root.join("api.rill"), "@include(\"api.h\");").unwrap();
    let mut graph = fixture.graph();
    headers::apply(&mut graph, "api", "@include(\"other.h\"); @link(\"other\"); export extern const API_VALUE: i32; export extern fn native_add(a: i32, b: i32) -> i32;").unwrap();
    let program = compile::lower_graph(&graph).unwrap();
    assert_eq!(program.externs[0].name, "native_add");
    assert_eq!(program.constants[0].name, "API_VALUE");
    assert_eq!(program.native.includes, ["api.h"]);
    assert!(program.native.links.is_empty());
    assert_eq!(
        fs::read_to_string(fixture.root.join("api.rill")).unwrap(),
        "@include(\"api.h\");"
    );
}

#[test]
fn explicit_private_externs_and_rill_wrappers_override_generated_names() {
    let fixture = Fixture::new("import api; fn main() { api.wrapper(42) }");
    fs::write(fixture.root.join("api.rill"), "@include(\"api.h\"); extern fn hidden(value: i32) -> i32; export fn wrapper(value) { hidden(value) }").unwrap();
    let mut graph = fixture.graph();
    headers::apply(
        &mut graph,
        "api",
        "export extern fn hidden(arg0: i32) -> i32; export extern fn wrapper(arg0: i32) -> i32;",
    )
    .unwrap();
    assert!(compile::lower_graph(&graph).is_ok());
    graph.modules.get_mut("@entry").unwrap().source =
        "import api; fn main() { api.hidden(42) }".into();
    assert_eq!(
        compile::lower_graph(&graph).unwrap_err().code,
        "E-MODULE-PRIVATE"
    );
}

#[test]
fn invalid_generated_source_cannot_mutate_the_module() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { 42 }");
    for generated in [
        "fn main() { 0 }",
        "export extern fn bad(x: unit) -> i32;",
        "export extern const DUPLICATE: i32; export extern fn DUPLICATE() -> i32;",
    ] {
        let mut graph = fixture.graph();
        let before = graph.modules["@entry"].source.clone();
        assert!(headers::apply(&mut graph, "@entry", generated).is_err());
        assert_eq!(graph.modules["@entry"].source, before);
    }
}

#[test]
fn snapshot_validates_transitive_file_bytes_even_when_length_stays_the_same() {
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
    let header = fixture.header("api.h", "#include \"nested.h\"\n");
    let nested = fixture.header("nested.h", "int native_add(int,int);\n");
    let source = "export extern fn native_add(a: i32, b: i32) -> i32;";
    headers::publish(
        &fixture.graph(),
        "@entry",
        source,
        &[header, nested.clone()],
    )
    .unwrap();
    let mut graph = fixture.graph();
    assert_eq!(headers::apply_cached(&mut graph).unwrap().len(), 1);
    fs::write(&nested, "int native_sub(int,int);\n").unwrap();
    let mut graph = fixture.graph();
    assert!(headers::apply_cached(&mut graph).unwrap().is_empty());
}

#[test]
fn snapshot_matches_the_whole_graph_native_configuration_and_accepts_unsaved_bodies() {
    let fixture = Fixture::new("import api; @link(\"first\"); fn main() { api.native_add(20,22) }");
    fs::write(fixture.root.join("api.rill"), "@include(\"api.h\");").unwrap();
    let header = fixture.header("api.h", "int native_add(int,int);\n");
    headers::publish(
        &fixture.graph(),
        "api",
        "export extern fn native_add(a: i32, b: i32) -> i32;",
        &[header],
    )
    .unwrap();
    let mut graph = fixture.graph();
    graph.modules.get_mut("@entry").unwrap().source =
        "import api; @link(\"first\"); fn main() { api.native_add(10,32) }".into();
    assert_eq!(headers::apply_cached(&mut graph).unwrap().len(), 1);
    assert!(compile::lower_graph(&graph).is_ok());
    let mut graph = fixture.graph();
    graph.modules.get_mut("@entry").unwrap().source =
        "import api; @link(\"other\"); fn main() { api.native_add(10,32) }".into();
    assert!(headers::apply_cached(&mut graph).unwrap().is_empty());
    assert!(compile::lower_graph(&graph).is_err());
}

#[cfg(unix)]
#[test]
fn snapshot_rejects_retargeted_header_symlinks() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new("@include(\"api.h\"); fn main() { native_add(20,22) }");
    let first = fixture.header("first.h", "int native_add(int,int);\n");
    let second = fixture.header("second.h", "int native_add(int,int);\n");
    let link = fixture.root.join("api.h");
    symlink(first, &link).unwrap();
    headers::publish(
        &fixture.graph(),
        "@entry",
        "export extern fn native_add(a: i32, b: i32) -> i32;",
        std::slice::from_ref(&link),
    )
    .unwrap();
    fs::remove_file(&link).unwrap();
    symlink(second, &link).unwrap();
    assert!(headers::apply_cached(&mut fixture.graph())
        .unwrap()
        .is_empty());
}

#[test]
fn moving_an_include_between_modules_invalidates_the_old_interface() {
    let fixture = Fixture::new("import api; fn main() { api.native_add(20,22) }");
    fs::write(fixture.root.join("api.rill"), "@include(\"api.h\");").unwrap();
    let header = fixture.header("api.h", "int native_add(int,int);\n");
    let before = fixture.graph();
    headers::publish(
        &before,
        "api",
        "export extern fn native_add(a: i32, b: i32) -> i32;",
        &[header],
    )
    .unwrap();
    let mut after = before.clone();
    after.modules.get_mut("api").unwrap().source.clear();
    after.modules.get_mut("@entry").unwrap().source =
        "@include(\"api.h\"); import api; fn main() { api.native_add(20,22) }".into();
    assert_eq!(
        headers::native_config(&before).unwrap(),
        headers::native_config(&after).unwrap()
    );
    assert!(headers::apply_cached(&mut after).unwrap().is_empty());
}
