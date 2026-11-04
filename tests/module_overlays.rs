use rill::modules::{
    compile::{
        load_library_with_sources_and_overlays, load_program,
        load_program_with_sources_and_overlays,
    },
    load_with_overlays,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill module overlays {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn entry_and_import_overlays_override_text_without_changing_disk() {
    let directory = Directory::new();
    let disk_main = "import helper; fn main() { helper.answer() }";
    let disk_helper = "export fn answer() { 1 }";
    let entry = directory.write("main.rill", disk_main);
    let helper = directory.write("helper.rill", disk_helper);
    let overlay_main = "import helper; fn main() { helper.answer() + 1 }";
    let overlay_helper = "export fn answer() { 41 }";
    let overlays = BTreeMap::from([
        (entry.clone(), overlay_main.to_owned()),
        (helper.clone(), overlay_helper.to_owned()),
    ]);
    let checked = load_program_with_sources_and_overlays(&entry, &overlays).unwrap();
    assert_eq!(
        rill::interpreter::run(checked.program())
            .unwrap()
            .to_string(),
        "42"
    );
    for source in checked.sources() {
        assert_eq!(source.text, overlays[&source.path]);
    }
    assert_eq!(fs::read_to_string(entry).unwrap(), disk_main);
    assert_eq!(fs::read_to_string(helper).unwrap(), disk_helper);
}

#[test]
fn new_unsaved_files_use_existing_directory_and_keep_origins() {
    let directory = Directory::new();
    let entry = directory.0.join("main.rill");
    let overlays = BTreeMap::from([
        (
            PathBuf::from("main.rill"),
            "import helper; fn main() { helper.answer() }".to_owned(),
        ),
        (
            PathBuf::from("helper.rill"),
            "export fn answer() { 42 }".to_owned(),
        ),
    ]);
    let checked = load_program_with_sources_and_overlays(&entry, &overlays).unwrap();
    assert_eq!(
        rill::interpreter::run(checked.program())
            .unwrap()
            .to_string(),
        "42"
    );
    assert_eq!(checked.sources().len(), 2);
    for source in checked.sources() {
        assert!(source.path.starts_with(&directory.0));
        assert!(!source.path.exists());
        assert_eq!(
            source.text,
            overlays[Path::new(source.path.file_name().unwrap())]
        );
    }
    for node in checked.nodes() {
        for origin in &node.origins {
            assert!(checked.origin_text(origin).is_some());
        }
    }
    assert_eq!(load_program(&entry).unwrap_err().code, "E-MODULE-IO");
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[test]
fn native_binding_overlays_do_not_resolve_or_execute_native_tools() {
    let directory = Directory::new();
    let entry = directory.0.join("main.rill");
    let overlays = BTreeMap::from([
        (entry.clone(), "import sdl; fn main() { if !sdl.SDL_Init(sdl.SDL_INIT_VIDEO) { -3 } else { sdl.SDL_Quit(); 0 } }".to_owned()),
        (directory.0.join("sdl.rill"), "@include(\"rill-missing-SDL3/SDL.h\"); @pkg_config(\"rill-missing-sdl3-editor-test\"); export extern const SDL_INIT_VIDEO: u32; export extern fn SDL_Init(flags: u32) -> bool; export extern fn SDL_Quit() -> unit;".to_owned()),
    ]);
    let checked = load_program_with_sources_and_overlays(&entry, &overlays).unwrap();
    assert_eq!(
        checked.program().native.packages,
        ["rill-missing-sdl3-editor-test"]
    );
    assert_eq!(checked.sources().len(), 2);
}

#[test]
fn library_buffers_still_check_types_and_ownership_without_main() {
    let directory = Directory::new();
    let entry = directory.0.join("library.rill");
    for source in [
        "export fn answer() { 42 }",
        "export fn invert(value) { not value }",
    ] {
        let overlays = BTreeMap::from([(entry.clone(), source.to_owned())]);
        let checked = load_library_with_sources_and_overlays(&entry, &overlays).unwrap();
        assert_eq!(checked.program().functions.len(), 1);
        assert!(load_program_with_sources_and_overlays(&entry, &overlays).is_err());
    }
    for source in [
        "export fn bad() { 1 + true }",
        "export fn bad() { let owner = system_box_i32_abort(1); let moved = move owner; box_read(owner) }",
    ] {
        let overlays = BTreeMap::from([(entry.clone(), source.to_owned())]);
        assert!(load_library_with_sources_and_overlays(&entry, &overlays).is_err(), "{source}");
    }
}

#[test]
fn overlays_keep_cycle_privacy_and_size_checks() {
    let directory = Directory::new();
    let entry = directory.0.join("main.rill");
    let mut overlays = BTreeMap::from([
        (
            entry.clone(),
            "import helper; fn main() { helper.answer() }".to_owned(),
        ),
        (
            directory.0.join("helper.rill"),
            "import main; export fn answer() { 42 }".to_owned(),
        ),
    ]);
    assert_eq!(
        load_with_overlays(&entry, &overlays).unwrap_err().code,
        "E-MODULE-CYCLE"
    );
    overlays.insert(
        directory.0.join("helper.rill"),
        "fn answer() { 42 }".to_owned(),
    );
    assert_eq!(
        load_program_with_sources_and_overlays(&entry, &overlays)
            .unwrap_err()
            .code,
        "E-MODULE-PRIVATE"
    );
    overlays.insert(entry.clone(), " ".repeat(1024 * 1024 + 1));
    assert_eq!(
        load_with_overlays(&entry, &overlays).unwrap_err().code,
        "E-MODULE-LIMIT"
    );
}

#[cfg(unix)]
#[test]
fn overlays_cannot_authorize_symlink_escape_or_replace_dangling_links() {
    use std::os::unix::fs::symlink;
    let directory = Directory::new();
    let outside = Directory::new();
    let entry = directory.write("main.rill", "import helper; fn main() { 0 }");
    let target = outside.write("helper.rill", "export fn answer() { 42 }");
    symlink(&target, directory.0.join("helper.rill")).unwrap();
    let overlays = BTreeMap::from([(
        directory.0.join("helper.rill"),
        "export fn answer() { 0 }".to_owned(),
    )]);
    assert_eq!(
        load_with_overlays(&entry, &overlays).unwrap_err().code,
        "E-MODULE-PATH"
    );
    fs::remove_file(directory.0.join("helper.rill")).unwrap();
    symlink(
        outside.0.join("missing.rill"),
        directory.0.join("helper.rill"),
    )
    .unwrap();
    assert_eq!(
        load_with_overlays(&entry, &overlays).unwrap_err().code,
        "E-MODULE-IO"
    );
}

#[cfg(unix)]
#[test]
fn existing_hardlink_identity_remains_checked_with_overlays() {
    let directory = Directory::new();
    let entry = directory.write("main.rill", "import a; import b; fn main() { 0 }");
    let first = directory.write("a.rill", "export fn answer() { 42 }");
    fs::hard_link(&first, directory.0.join("b.rill")).unwrap();
    let overlays = BTreeMap::from([(first, "export fn answer() { 1 }".to_owned())]);
    assert_eq!(
        load_with_overlays(&entry, &overlays).unwrap_err().code,
        "E-MODULE-DUPLICATE"
    );
}

#[test]
fn unrelated_open_documents_do_not_expand_entry_import_access() {
    let directory = Directory::new();
    let outside = Directory::new();
    let entry = directory.write("main.rill", "fn main() { 42 }");
    let overlays = BTreeMap::from([
        (outside.0.join("unrelated.rill"), "fn invalid( {".to_owned()),
        (
            outside.0.join("missing-parent/scratch.rill"),
            "fn main() { 0 }".to_owned(),
        ),
    ]);
    let graph = load_with_overlays(&entry, &overlays).unwrap();
    assert_eq!(graph.modules.len(), 1);
    assert_eq!(graph.root, directory.0);
    assert!(Path::new(&entry).exists());
}

#[test]
fn an_overlay_cannot_replace_a_directory_with_source_text() {
    let directory = Directory::new();
    let entry = directory.0.join("main.rill");
    fs::create_dir(&entry).unwrap();
    let overlays = BTreeMap::from([(entry.clone(), "fn main() { 42 }".to_owned())]);
    let error = load_with_overlays(&entry, &overlays).unwrap_err();
    assert_eq!(error.code, "E-MODULE-IO");
    assert!(entry.is_dir());
}

#[test]
fn unsaved_library_result_annotations_keep_exact_origin_roles() {
    let directory = Directory::new();
    let entry = directory.0.join("library.rill");
    let text = "export type Row={value:i32} export fn extract(row:Row)->i32{row.value}";
    let overlays = BTreeMap::from([(entry.clone(), text.to_owned())]);
    let sources = load_library_with_sources_and_overlays(&entry, &overlays).unwrap();
    assert_eq!(sources.sources()[0].text, text);
    assert_eq!(
        sources.program().functions[0]
            .result_annotation
            .as_ref()
            .unwrap()
            .name,
        "i32"
    );
    let result = sources
        .nodes()
        .iter()
        .find(|node| node.path.steps == [rill::source_map::AstStep::FunctionResult])
        .unwrap();
    assert_eq!(sources.origin_text(&result.origins[0]), Some("i32"));
    assert!(!entry.exists());
}
