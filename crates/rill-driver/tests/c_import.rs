//! `@link` and `@include`: a script that reads a C header and links a library,
//! and the SDL3 demos built against a system install.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rill-c-{}", tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create workdir");
    dir
}

/// `cc` flags that build a shared library this platform's loader finds through
/// the rpath the driver passes at link time.
fn shared_library_flags() -> Vec<&'static str> {
    if cfg!(target_os = "macos") {
        vec!["-dynamiclib", "-install_name", "@rpath/libhello.dylib"]
    } else {
        vec!["-shared"]
    }
}

#[test]
fn imports_a_header_and_links_the_library() {
    let dir = workdir("hello");
    std::fs::write(
        dir.join("hello.h"),
        "#define TRIPLE_FACTOR 3\n\
         \n\
         typedef struct Point { float x; float y; } Point;\n\
         \n\
         extern int triple(int n);\n\
         extern const char *hello_name(void);\n\
         extern float point_sum(Point p);\n",
    )
    .expect("write header");
    std::fs::write(
        dir.join("hello.c"),
        "#include \"hello.h\"\n\
         int triple(int n) { return n * TRIPLE_FACTOR; }\n\
         const char *hello_name(void) { return \"hello from C\"; }\n\
         float point_sum(Point p) { return p.x + p.y; }\n",
    )
    .expect("write source");
    let library = if cfg!(target_os = "macos") {
        dir.join("libhello.dylib")
    } else {
        dir.join("libhello.so")
    };
    let status = Command::new("cc")
        .args(["-O2", "-o"])
        .arg(&library)
        .arg(dir.join("hello.c"))
        .args(shared_library_flags())
        .status()
        .expect("run cc");
    assert!(status.success(), "the fixture library must build");

    // `@include` reads the header next to the script; `@link` names the library
    // to find in the loader's paths (LIBRARY_PATH here, the system prefixes
    // otherwise).
    std::fs::write(
        dir.join("hello.rill"),
        "use std::io;\n\
         \n\
         @link(\"hello\");\n\
         \n\
         type HELLO = @include(\"hello.h\");\n\
         use HELLO::*;\n\
         \n\
         let main () -> i32 <io> = {\n\
             io::println (hello_name ());\n\
             io::println (\"triple = \" ++ (triple (5) as String));\n\
             var p = Point (1.5, 2.25);\n\
             io::println (\"sum = \" ++ (point_sum (p) as String));\n\
             triple (4)\n\
         };\n",
    )
    .expect("write script");

    let output = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .arg("exec")
        .arg(dir.join("hello.rill"))
        .env("LIBRARY_PATH", &dir)
        .env("RILL_CACHE_DIR", dir.join("cache"))
        .output()
        .expect("run rillc exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The program's value *is* its exit code, so a non-zero status is the
    // expected shape here; what must not appear is a diagnostic.
    assert!(
        stderr.is_empty(),
        "the script must build and run without diagnostics: {stderr}"
    );
    assert!(
        stdout.contains("hello from C"),
        "the C function's string must print: {stdout}"
    );
    assert!(
        stdout.contains("triple = 15"),
        "the header's macro must reach the C call: {stdout}"
    );
    assert!(
        stdout.contains("sum = 3.75"),
        "a struct built in Rill must pass by value: {stdout}"
    );
    assert_eq!(
        output.status.code(),
        Some(12),
        "the exit code is the program's value"
    );

    // A cached run reuses the binary; the same answer must come out.
    let cached = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .arg("exec")
        .arg(dir.join("hello.rill"))
        .env("LIBRARY_PATH", &dir)
        .env("RILL_CACHE_DIR", dir.join("cache"))
        .output()
        .expect("run rillc exec again");
    assert_eq!(cached.status.code(), Some(12));
    assert_eq!(String::from_utf8_lossy(&cached.stdout), stdout);
}

#[test]
fn a_missing_library_is_reported_not_guessed() {
    let dir = workdir("missing");
    std::fs::write(
        dir.join("missing.rill"),
        "@link(\"rill_no_such_library\");\nlet main () -> i32 <computes> = 0;\n",
    )
    .expect("write script");
    let output = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .args(["build"])
        .arg(dir.join("missing.rill"))
        .arg("-o")
        .arg(dir.join("missing"))
        // Keep the search deterministic and outside the host's own paths.
        .env("LIBRARY_PATH", &dir)
        .output()
        .expect("run rillc build");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "a missing library must fail");
    assert!(
        stderr.contains("cannot find `rill_no_such_library`"),
        "the diagnostic names the library: {stderr}"
    );
}

/// SDL3 has to be installed for the demos to build; the header decides.
fn sdl3_available() -> bool {
    let dirs = ["/opt/homebrew/include", "/usr/local/include", "/usr/include"];
    dirs.iter().any(|dir| Path::new(dir).join("SDL3/SDL.h").is_file())
}

fn run_demo(name: &str) -> (std::process::Output, String) {
    let demo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/sdl3")
        .join(name);
    let cache = std::env::temp_dir().join(format!(
        "rill-sdl3-cache-{}",
        name.trim_end_matches(".rill")
    ));
    let _ = std::fs::remove_dir_all(&cache);
    let output = Command::new(env!("CARGO_BIN_EXE_rillc"))
        .arg("exec")
        .arg(&demo)
        .env("RILL_SDL3_DEMO", "1")
        .env("SDL_VIDEODRIVER", "dummy")
        .env("RILL_CACHE_DIR", &cache)
        .output()
        .expect("run the demo");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    (output, stdout)
}

#[test]
fn sdl3_window_demo_runs_headless() {
    if !sdl3_available() {
        eprintln!("skipping: SDL3's headers are not installed");
        return;
    }
    let (output, stdout) = run_demo("window.rill");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the window demo must run headless: {stderr}"
    );
    assert!(
        stdout.contains("demo frames 120"),
        "the demo must run its fixed burst: {stdout}"
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn sdl3_sidescroller_plays_itself_headless() {
    if !sdl3_available() {
        eprintln!("skipping: SDL3's headers are not installed");
        return;
    }
    let (output, stdout) = run_demo("sidescroller.rill");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "the game must run headless without diagnostics: {stderr}"
    );
    // The demo mode walks right at a fixed step, so the coins it reaches - and
    // therefore the score - are the same every run.
    assert!(
        stdout.contains("score 3"),
        "the walk must collect the ground-level coins: {stdout}"
    );
    assert!(
        stdout.contains("demo frames 240 score 3"),
        "the summary line pins the level: {stdout}"
    );
    assert_eq!(
        output.status.code(),
        Some(3),
        "the game's score is its exit code"
    );
}
