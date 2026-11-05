use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rill-sdk-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        for name in ["bin", "include", "lib"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        fs::write(root.join("bin/tool"), "fixture").unwrap();
        let fixture = Self(root);
        fixture.write(serde_json::json!({
            "version":1, "rill":env!("CARGO_PKG_VERSION"),
            "tools":{"slangc":"bin/tool"},
            "packages":{"sdl3":{"include_dirs":["include"],"library_dirs":["lib"],"libraries":["SDL3","dependency","SDL3"]}},
            "runtime_dirs":["lib"]
        }));
        fixture
    }
    fn write(&self, value: Value) {
        fs::write(self.0.join(MARKER), value.to_string()).unwrap();
    }
    fn value(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.0.join(MARKER)).unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn relocatable_tools_and_ordered_package_flags() {
    let fixture = Fixture::new();
    let bundle = Bundle::load(&fixture.0).unwrap();
    assert_eq!(
        bundle.tool("slangc").unwrap(),
        Some(fixture.0.join("bin/tool").canonicalize().unwrap())
    );
    assert!(bundle.tool("missing").unwrap().is_none());
    let package = bundle.package("sdl3").unwrap().unwrap();
    assert_eq!(
        package.cflags,
        vec![
            OsString::from("-I"),
            fixture
                .0
                .join("include")
                .canonicalize()
                .unwrap()
                .into_os_string()
        ]
    );
    let tail: Vec<_> = package
        .link_args
        .iter()
        .rev()
        .take(3)
        .rev()
        .cloned()
        .collect();
    assert_eq!(
        tail,
        vec![
            OsString::from("-lSDL3"),
            "-ldependency".into(),
            "-lSDL3".into()
        ]
    );
    assert!(bundle.package("other").unwrap().is_none());
}

#[test]
fn invalid_versions_unknown_fields_and_duplicate_keys_fail() {
    let fixture = Fixture::new();
    let original = fixture.value();
    for (key, value) in [
        ("version", serde_json::json!(2)),
        ("rill", serde_json::json!("other")),
        ("extra", serde_json::json!(true)),
    ] {
        let mut invalid = original.clone();
        invalid[key] = value;
        fixture.write(invalid);
        assert!(Bundle::load(&fixture.0).is_err());
    }
    let duplicate = format!("{{\"version\":1,\"rill\":{:?},\"tools\":{{\"slangc\":\"bin/tool\",\"slangc\":\"bin/tool\"}},\"packages\":{{}},\"runtime_dirs\":[]}}", env!("CARGO_PKG_VERSION"));
    fs::write(fixture.0.join(MARKER), duplicate).unwrap();
    assert!(Bundle::load(&fixture.0)
        .err()
        .unwrap()
        .contains("duplicate"));
}

#[test]
fn tools_and_package_paths_are_confined() {
    let fixture = Fixture::new();
    for path in ["../outside", "/tmp", "bin/../bin/tool", "missing", "lib"] {
        let mut value = fixture.value();
        value["tools"]["slangc"] = path.into();
        fixture.write(value);
        let bundle = Bundle::load(&fixture.0).unwrap();
        assert!(bundle.tool("slangc").is_err(), "{path}");
    }
    let mut value = fixture.value();
    value["packages"]["sdl3"]["include_dirs"] = serde_json::json!(["../outside"]);
    fixture.write(value);
    assert!(Bundle::load(&fixture.0).unwrap().package("sdl3").is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(std::env::temp_dir(), fixture.0.join("escape")).unwrap();
        let mut value = fixture.value();
        value["runtime_dirs"] = serde_json::json!(["escape"]);
        fixture.write(value);
        assert!(Bundle::load(&fixture.0).is_err());
    }
}
