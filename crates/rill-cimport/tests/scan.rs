//! Scanner tests: a written-on-the-spot fixture pair, plus (when the machine
//! has one) a real SDL3 install.

use std::path::{Path, PathBuf};

use rill_cimport::{scan_header, CModule, CType, CTypeKind};

/// A scratch directory for the fixtures of one call. The name is unique per call
/// rather than per process: the tests run in parallel, and two of them sharing a
/// directory would race on the remove-and-rewrite of the same fixture.
fn scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rill-cimport-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

const DEP_H: &str = r#"
#ifndef DEP_H_
#define DEP_H_

#define DEP_CONST 42

typedef struct DepOpaque DepOpaque;

#endif
"#;

const FIXTURE_H: &str = r#"
#ifndef FIXTURE_H_
#define FIXTURE_H_

#include "dep.h"

typedef struct FixtureOpaque FixtureOpaque;

/* a record: fields keep declaration order, `int x, y;` shares the type */
typedef struct FixtureRecord
{
    int a;
    const char *name;
    FixtureOpaque *handle;
    float x, y;
    Uint8 padding[4];
} FixtureRecord;

/* a union: consumers only use the leading members */
typedef union FixtureUnion
{
    Uint32 type;
    void *payload;
} FixtureUnion;

typedef enum FixtureEnum
{
    FIXTURE_A = 0,
    FIXTURE_B,
    FIXTURE_C = 5,
    FIXTURE_D
} FixtureEnum;

typedef Uint32 FixtureFlags;

#define FIXTURE_INT 0x00000020u /* a plain integer */
#define FIXTURE_FLOAT 1.5f
#define FIXTURE_FUNC(x) ((x) + 1) /* function-like: skipped */
#define FIXTURE_STRING "hello"    /* not a literal: skipped */

/* a multi-line prototype with a plain return type */
extern SDL_DECLSPEC bool SDLCALL FixtureMultiLine(const char *title,
                                                  FixtureOpaque *handle,
                                                  FixtureFlags flags);

extern SDL_DECLSPEC void SDLCALL FixtureVariadic(const char *fmt, ...);

extern SDL_DECLSPEC const char *SDLCALL FixtureReturnsText(void);

extern SDL_DECLSPEC void SDLCALL FixtureArrayParam(int argc, char *argv[]);

extern SDL_DECLSPEC void SDLCALL FixtureFnPtrParam(void (*callback)(void));

typedef void (SDLCALL *FixtureCallback)(void *userdata);

#endif
"#;

fn scan_fixture() -> CModule {
    let dir = scratch("fixture");
    std::fs::write(dir.join("dep.h"), DEP_H).expect("write dep.h");
    std::fs::write(dir.join("fixture.h"), FIXTURE_H).expect("write fixture.h");
    scan_header(&dir.join("fixture.h"), &[]).expect("scan fixture")
}

fn function<'a>(module: &'a CModule, name: &str) -> &'a rill_cimport::CFunction {
    module
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("no function {name}"))
}

fn type_decl<'a>(module: &'a CModule, name: &str) -> &'a rill_cimport::CTypeDecl {
    module
        .types
        .iter()
        .find(|decl| decl.name == name)
        .unwrap_or_else(|| panic!("no type {name}"))
}

fn constant<'a>(module: &'a CModule, name: &str) -> &'a rill_cimport::CConstant {
    module
        .constants
        .iter()
        .find(|constant| constant.name == name)
        .unwrap_or_else(|| panic!("no constant {name}"))
}

#[test]
fn headers_are_visited_depth_first_and_absolutely() {
    let dir = scratch("fixture");
    std::fs::write(dir.join("dep.h"), DEP_H).expect("write dep.h");
    std::fs::write(dir.join("fixture.h"), FIXTURE_H).expect("write fixture.h");
    let module = scan_header(&dir.join("fixture.h"), &[]).expect("scan fixture");

    let expected: Vec<PathBuf> = [dir.join("fixture.h"), dir.join("dep.h")]
        .iter()
        .map(|path| std::fs::canonicalize(path).expect("canonicalize"))
        .collect();
    assert_eq!(module.headers, expected);
}

#[test]
fn functions_are_split_on_semicolons() {
    let module = scan_fixture();

    let multiline = function(&module, "FixtureMultiLine");
    assert_eq!(multiline.skipped, None);
    assert_eq!(multiline.ret, CType::Bool);
    assert_eq!(multiline.params.len(), 3);
    assert_eq!(multiline.params[0].name, "title");
    assert_eq!(multiline.params[0].ty, CType::Text);
    assert_eq!(multiline.params[1].name, "handle");
    assert_eq!(
        multiline.params[1].ty,
        CType::Ptr {
            mutable: true,
            inner: Box::new(CType::Named("FixtureOpaque".to_string())),
        }
    );
    assert_eq!(
        multiline.params[2].ty,
        CType::Named("FixtureFlags".to_string())
    );
    assert_eq!(multiline.header, module.headers[0]);

    let variadic = function(&module, "FixtureVariadic");
    assert_eq!(variadic.skipped.as_deref(), Some("variadic"));

    let text = function(&module, "FixtureReturnsText");
    assert_eq!(text.ret, CType::Text);
    assert!(text.params.is_empty());

    assert_eq!(
        function(&module, "FixtureArrayParam").skipped.as_deref(),
        Some("array parameter")
    );
    assert_eq!(
        function(&module, "FixtureFnPtrParam").skipped.as_deref(),
        Some("function-pointer parameter")
    );

    // a typedef'd function pointer is not a function
    assert!(module
        .functions
        .iter()
        .all(|function| function.name != "FixtureCallback"));
}

#[test]
fn records_unions_opaque_types_and_aliases() {
    let module = scan_fixture();

    assert!(matches!(
        type_decl(&module, "FixtureOpaque").kind,
        CTypeKind::Opaque
    ));

    let record = &type_decl(&module, "FixtureRecord").kind;
    let CTypeKind::Record(fields) = record else {
        panic!("FixtureRecord is {record:?}");
    };
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert_eq!(names, ["a", "name", "handle", "x", "y", "padding"]);
    assert_eq!(
        fields[0].ty,
        CType::Int {
            bits: 32,
            signed: true
        }
    );
    assert_eq!(fields[1].ty, CType::Text);
    assert_eq!(
        fields[2].ty,
        CType::Ptr {
            mutable: true,
            inner: Box::new(CType::Named("FixtureOpaque".to_string())),
        }
    );
    assert_eq!(fields[3].ty, CType::Float { bits: 32 });
    assert_eq!(fields[4].ty, CType::Float { bits: 32 });
    assert!(matches!(fields[5].ty, CType::Unmapped(_)));

    let union = &type_decl(&module, "FixtureUnion").kind;
    let CTypeKind::Record(members) = union else {
        panic!("FixtureUnion is {union:?}");
    };
    assert_eq!(members[0].name, "type");
    assert_eq!(
        members[0].ty,
        CType::Int {
            bits: 32,
            signed: false
        }
    );

    assert!(matches!(
        type_decl(&module, "FixtureEnum").kind,
        CTypeKind::Enum
    ));
    match &type_decl(&module, "FixtureFlags").kind {
        CTypeKind::Alias(CType::Int {
            bits: 32,
            signed: false,
        }) => {}
        other => panic!("FixtureFlags is {other:?}"),
    }

    // the function-pointer typedef is reported unmapped so consumers skip it
    match &type_decl(&module, "FixtureCallback").kind {
        CTypeKind::Alias(CType::Unmapped(spelling)) => {
            assert!(spelling.contains("FixtureCallback"), "{spelling}");
        }
        other => panic!("FixtureCallback is {other:?}"),
    }
}

#[test]
fn constants_and_enumerators() {
    let module = scan_fixture();

    let int = constant(&module, "FIXTURE_INT");
    assert_eq!(int.value, "0x00000020u");
    assert_eq!(
        int.ty,
        CType::Int {
            bits: 32,
            signed: false
        }
    );

    let float = constant(&module, "FIXTURE_FLOAT");
    assert_eq!(float.value, "1.5f");
    assert_eq!(float.ty, CType::Float { bits: 32 });

    // a constant from an included header, with its own header recorded
    let dep = constant(&module, "DEP_CONST");
    assert_eq!(dep.value, "42");
    assert_eq!(dep.header, module.headers[1]);

    // function-like macros and non-literal bodies are not constants
    for skipped in ["FIXTURE_FUNC", "FIXTURE_STRING"] {
        assert!(module
            .constants
            .iter()
            .all(|constant| constant.name != skipped));
    }

    let int_ty = CType::Int {
        bits: 32,
        signed: true,
    };
    for (name, value) in [
        ("FIXTURE_A", "0"),
        ("FIXTURE_B", "1"),
        ("FIXTURE_C", "5"),
        ("FIXTURE_D", "6"),
    ] {
        let enumerator = constant(&module, name);
        assert_eq!(enumerator.value, value, "{name}");
        assert_eq!(enumerator.ty, int_ty, "{name}");
    }
}

#[test]
fn missing_includes_are_ignored() {
    let dir = scratch("missing");
    std::fs::write(
        dir.join("only.h"),
        "#include <nowhere/absent.h>\n#include \"absent.h\"\nextern SDL_DECLSPEC void SDLCALL Only(void);\n",
    )
    .expect("write only.h");
    let module = scan_header(&dir.join("only.h"), std::slice::from_ref(&dir)).expect("scan only.h");
    assert_eq!(module.headers.len(), 1);
    assert_eq!(function(&module, "Only").ret, CType::Void);
}

#[test]
fn unreadable_root_is_an_error() {
    let err = scan_header(Path::new("/definitely/not/here.h"), &[]).expect_err("must fail");
    assert!(err.contains("cannot read"), "{err}");
}

/// The real thing: scan the SDL3 install on this machine. Skipped when the
/// header is absent.
#[test]
fn sdl3_real_install() {
    let root = Path::new("/opt/homebrew/include/SDL3/SDL.h");
    if !root.exists() {
        println!(
            "skipping SDL3 scan test: {} is not installed",
            root.display()
        );
        return;
    }
    let module = scan_header(root, &[PathBuf::from("/opt/homebrew/include")]).expect("scan SDL3");

    let init = function(&module, "SDL_Init");
    assert_eq!(init.ret, CType::Bool);
    assert_eq!(init.params.len(), 1);
    assert_eq!(init.params[0].ty, CType::Named("SDL_InitFlags".to_string()));

    let window = function(&module, "SDL_CreateWindow");
    assert_eq!(
        window.ret,
        CType::Ptr {
            mutable: true,
            inner: Box::new(CType::Named("SDL_Window".to_string())),
        }
    );
    let params: Vec<CType> = window.params.iter().map(|param| param.ty.clone()).collect();
    assert_eq!(
        params,
        vec![
            CType::Text,
            CType::Int {
                bits: 32,
                signed: true
            },
            CType::Int {
                bits: 32,
                signed: true
            },
            CType::Named("SDL_WindowFlags".to_string()),
        ]
    );

    let fill = function(&module, "SDL_RenderFillRect");
    assert_eq!(
        fill.params[1].ty,
        CType::Ptr {
            mutable: false,
            inner: Box::new(CType::Named("SDL_FRect".to_string())),
        }
    );

    assert!(matches!(
        type_decl(&module, "SDL_Window").kind,
        CTypeKind::Opaque
    ));

    let event = &type_decl(&module, "SDL_Event").kind;
    let CTypeKind::Record(fields) = event else {
        panic!("SDL_Event is {event:?}");
    };
    assert_eq!(fields[0].name, "type");
    assert_eq!(
        fields[0].ty,
        CType::Int {
            bits: 32,
            signed: false
        }
    );

    let video = constant(&module, "SDL_INIT_VIDEO");
    assert_eq!(video.value, "0x00000020u");
    assert_eq!(
        video.ty,
        CType::Int {
            bits: 32,
            signed: false
        }
    );

    let skipped = module
        .functions
        .iter()
        .filter(|function| function.skipped.is_some())
        .count();
    println!(
        "SDL3 scan: {} functions ({} skipped), {} types, {} constants, {} headers",
        module.functions.len(),
        skipped,
        module.types.len(),
        module.constants.len(),
        module.headers.len(),
    );
}

/// raylib's house style: the C++ guard is a real `extern "C" {`, prototypes are
/// marked `RLAPI` instead of `extern`, and records are passed by value.
const RAYLIB_STYLE_H: &str = r#"
#ifndef RAYLIB_STYLE_H_
#define RAYLIB_STYLE_H_

#if defined(__cplusplus)
extern "C" {            // Prevents name mangling of functions
#endif

#define PI 3.14159265358979323846f

typedef struct RaylibColor
{
    unsigned char r, g, b, a;
} RaylibColor;

typedef enum RaylibKey
{
    RAYLIB_KEY_ESCAPE = 256,
    RAYLIB_KEY_RIGHT = 262
} RaylibKey;

RLAPI void RaylibInitWindow(int width, int height, const char *title);
RLAPI bool RaylibWindowShouldClose(void);
RLAPI void RaylibClearBackground(RaylibColor color);
RLAPI void RaylibTraceLog(int level, const char *text, ...);
RLAPI RaylibColor RaylibFade(RaylibColor color, float alpha);

#if defined(__cplusplus)
}
#endif

#endif
"#;

#[test]
fn raylib_style_prototypes_are_functions() {
    // The `RLAPI` marker is what tells a prototype from a variable declaration,
    // and it is dropped from the text like SDL's decorations, so the return type
    // and the parameters are what is left.
    let dir = scratch("raylib-style");
    std::fs::write(dir.join("raylib_style.h"), RAYLIB_STYLE_H).expect("write the fixture");
    let module = scan_header(&dir.join("raylib_style.h"), &[]).expect("scan the fixture");

    let init = function(&module, "RaylibInitWindow");
    assert_eq!(init.skipped, None);
    assert_eq!(init.ret, CType::Void);
    let params: Vec<CType> = init.params.iter().map(|param| param.ty.clone()).collect();
    assert_eq!(
        params,
        vec![
            CType::Int {
                bits: 32,
                signed: true
            },
            CType::Int {
                bits: 32,
                signed: true
            },
            CType::Text,
        ]
    );

    assert_eq!(
        function(&module, "RaylibWindowShouldClose").ret,
        CType::Bool
    );
    assert_eq!(
        function(&module, "RaylibClearBackground").params[0].ty,
        CType::Named("RaylibColor".to_string())
    );
    assert_eq!(
        function(&module, "RaylibFade").ret,
        CType::Named("RaylibColor".to_string())
    );
    assert_eq!(
        function(&module, "RaylibTraceLog").skipped.as_deref(),
        Some("variadic")
    );

    let record = &type_decl(&module, "RaylibColor").kind;
    let CTypeKind::Record(fields) = record else {
        panic!("RaylibColor is {record:?}");
    };
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert_eq!(
        names,
        ["r", "g", "b", "a"],
        "dense declarators share a type"
    );
    assert_eq!(
        fields[0].ty,
        CType::Int {
            bits: 8,
            signed: false
        }
    );

    assert_eq!(constant(&module, "RAYLIB_KEY_ESCAPE").value, "256");
    assert_eq!(constant(&module, "PI").ty, CType::Float { bits: 32 });
    // The marker is decoration, not a declaration of its own.
    assert!(module
        .constants
        .iter()
        .all(|constant| constant.name != "RLAPI"));
}

/// The other real install: raylib marks its prototypes `RLAPI` and hands records
/// over by value. Skipped when the header is absent.
#[test]
fn raylib_real_install() {
    let root = Path::new("/opt/homebrew/include/raylib.h");
    if !root.exists() {
        println!(
            "skipping raylib scan test: {} is not installed",
            root.display()
        );
        return;
    }
    let module = scan_header(root, &[PathBuf::from("/opt/homebrew/include")]).expect("scan raylib");

    let init = function(&module, "InitWindow");
    assert_eq!(init.ret, CType::Void);
    let params: Vec<CType> = init.params.iter().map(|param| param.ty.clone()).collect();
    assert_eq!(
        params,
        vec![
            CType::Int {
                bits: 32,
                signed: true
            },
            CType::Int {
                bits: 32,
                signed: true
            },
            CType::Text,
        ]
    );

    assert_eq!(function(&module, "WindowShouldClose").ret, CType::Bool);
    assert_eq!(
        function(&module, "ClearBackground").params[0].ty,
        CType::Named("Color".to_string())
    );

    let color = &type_decl(&module, "Color").kind;
    let CTypeKind::Record(fields) = color else {
        panic!("Color is {color:?}");
    };
    assert_eq!(fields.len(), 4);
    assert!(matches!(
        type_decl(&module, "Rectangle").kind,
        CTypeKind::Record(_)
    ));

    let skipped = module
        .functions
        .iter()
        .filter(|function| function.skipped.is_some())
        .count();
    println!(
        "raylib scan: {} functions ({} skipped), {} types, {} constants, {} headers",
        module.functions.len(),
        skipped,
        module.types.len(),
        module.constants.len(),
        module.headers.len(),
    );
}
