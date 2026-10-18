//! Scans C headers written in the SDL3 house style and reports the declarations
//! the Rill front end will import.
//!
//! Intentionally dependency-free (std only). This is the C side of the import
//! pipeline: everything reported here is a C-level fact - names stay `String`s
//! and types stay [`CType`]. Turning those into Rill types happens elsewhere.
//!
//! The parsing rules, in the order they are applied:
//!
//! - comments are removed first; then preprocessor continuation lines (those
//!   ending in `\`) are dropped, so a multi-line macro never leaks into a
//!   declaration
//! - `#include` is followed depth-first and deduped by canonical path; a
//!   missing include is ignored rather than an error
//! - `#if`/`#ifdef` conditionals are not evaluated - SDL declares the same API
//!   in every branch, so scanning straight through is correct here
//! - a declaration is the text between `;` at brace/paren/bracket depth zero,
//!   which lets prototypes and record bodies span lines
//! - a `{` at depth zero opens a record/enum body only when the declaration
//!   starts with `typedef`/`struct`/`union`/`enum`; any other body (a `static
//!   inline` definition, a macro body) is skipped wholesale
//! - only declarations that carry a prototype marker are functions - `extern`,
//!   `SDL_DECLSPEC` or raylib's `RLAPI` - which keeps helper macro invocations
//!   (`SDL_COMPILE_TIME_ASSERT(...)`) out of the results
//! - the all-caps macros a header decorates prototypes with (`SDLCALL`,
//!   `RLAPI`, `SDL_ALLOC_SIZE2(1, 2)`, `SDL_OUT_BYTECAP(len)`...) are dropped, so
//!   both the return type and the parameters are what is left around them
//! - the first declaration of a name wins

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Scan `path`, following its includes, and report everything importable.
///
/// Only the root header is required to be readable; missing includes are
/// skipped.
pub fn scan_header(path: &Path, include_dirs: &[PathBuf]) -> Result<CModule, String> {
    let mut scanner = Scanner {
        module: CModule::default(),
        include_dirs: include_dirs.to_vec(),
        visited: HashSet::new(),
        alias_inner: HashMap::new(),
        seen_functions: HashSet::new(),
        seen_types: HashSet::new(),
        seen_constants: HashSet::new(),
    };
    scanner.visit(&canonical(path), true)?;
    Ok(scanner.module)
}

#[derive(Clone, Debug, Default)]
pub struct CModule {
    pub functions: Vec<CFunction>,
    pub types: Vec<CTypeDecl>,
    pub constants: Vec<CConstant>,
    /// Absolute paths of every header visited, in visit order (the driver uses
    /// this to fingerprint the build).
    pub headers: Vec<std::path::PathBuf>,
}

#[derive(Clone, Debug)]
pub struct CFunction {
    pub name: String,
    pub params: Vec<CParam>,
    pub ret: CType,
    /// Some(reason) when the declaration was seen but is not callable from Rill
    /// (variadic, function-pointer parameter, array parameter, unknown type...).
    pub skipped: Option<String>,
    /// Header the declaration came from.
    pub header: std::path::PathBuf,
}

#[derive(Clone, Debug)]
pub struct CParam {
    pub name: String,
    pub ty: CType,
}

#[derive(Clone, PartialEq, Debug)]
pub enum CType {
    Void,                           // `void` (also the return of a void fn)
    Int { bits: u8, signed: bool }, // int, unsigned, Uint8/16/32/64, Sint8..., size_t (usize -> 64 unsigned), ptrdiff_t, char (8 signed)
    Float { bits: u8 },             // float (32), double (64)
    Bool,                           // C `bool`
    Text,                           // `const char *`
    Ptr { mutable: bool, inner: Box<CType> },
    Named(String),    // a struct/union/enum/typedef name
    Unmapped(String), // anything else, with the C spelling
}

#[derive(Clone, Debug)]
pub struct CTypeDecl {
    pub name: String, // the typedef name (`SDL_FRect`, `SDL_Window`)
    pub kind: CTypeKind,
    pub header: std::path::PathBuf,
}

#[derive(Clone, Debug)]
pub enum CTypeKind {
    /// `typedef struct X X;` - declared, never defined here.
    Opaque,
    /// `typedef struct/union X { fields } X;` - fields in declaration order.
    Record(Vec<CField>),
    /// `typedef Uint32 SDL_InitFlags;` - resolved one level, inner may be Named
    /// (another typedef) only when the name is not resolvable in this scan.
    Alias(CType),
    /// `typedef enum X { ... } X;` - the enumerators are reported as constants.
    Enum,
}

#[derive(Clone, Debug)]
pub struct CField {
    pub name: String,
    pub ty: CType,
}

#[derive(Clone, Debug)]
pub struct CConstant {
    pub name: String,
    /// The literal text exactly as written after the macro name, e.g. `0x00000020u`.
    pub value: String,
    pub ty: CType,
    pub header: std::path::PathBuf,
}

// ---------------------------------------------------------------------------
// Scanner state
// ---------------------------------------------------------------------------

struct Scanner {
    module: CModule,
    include_dirs: Vec<PathBuf>,
    /// canonical path -> already scanned
    visited: HashSet<PathBuf>,
    /// typedef name -> the type it resolved to, for one-level inner resolution
    alias_inner: HashMap<String, CType>,
    seen_functions: HashSet<String>,
    seen_types: HashSet<String>,
    seen_constants: HashSet<String>,
}

impl Scanner {
    /// Read one header and scan it, then everything it includes.
    ///
    /// `required` is only set for the root header: everything else is a missing
    /// include, which is ignored.
    fn visit(&mut self, path: &Path, required: bool) -> Result<(), String> {
        let path = canonical(path);
        if !self.visited.insert(path.clone()) {
            return Ok(());
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                if required {
                    return Err(format!("cannot read {}: {err}", path.display()));
                }
                return Ok(());
            }
        };
        self.module.headers.push(path.clone());
        let text = String::from_utf8_lossy(&bytes);
        self.scan_source(&text, &path);
        Ok(())
    }

    /// Split a header into includes, `#define`d constants and declarations.
    fn scan_source(&mut self, src: &str, header: &Path) {
        let cleaned = strip_comments(src);
        let mut code = String::new();
        // an odd number of lines are consumed by a multi-line macro
        let mut in_macro = false;
        for line in cleaned.lines() {
            let line = line.trim();
            if in_macro {
                in_macro = line.ends_with('\\');
                continue;
            }
            if line.is_empty() {
                continue;
            }
            if line.ends_with('\\') {
                in_macro = true;
                continue;
            }
            // `extern "C" {` / the matching `}` around the C++ guard
            if line == "}" || line.starts_with("extern \"C\"") {
                continue;
            }
            if let Some(rest) = line.strip_prefix('#') {
                self.directive(rest.trim_start(), header);
                continue;
            }
            code.push_str(line);
            code.push('\n');
        }
        self.declarations(&code, header);
    }

    fn directive(&mut self, rest: &str, header: &Path) {
        if let Some(after) = strip_directive(rest, "include") {
            let after = after.trim_start();
            let target = if let Some(rest) = after.strip_prefix('<') {
                rest.split_once('>').map(|(name, _)| (name, false))
            } else {
                after
                    .strip_prefix('"')
                    .and_then(|rest| rest.split_once('"'))
                    .map(|(name, _)| (name, true))
            };
            if let Some((name, quoted)) = target {
                if let Some(found) = self.resolve_include(name, quoted, header) {
                    // includes are never required - ignore unreadable ones
                    let _ = self.visit(&found, false);
                }
            }
            return;
        }
        if let Some(after) = strip_directive(rest, "define") {
            self.define(after, header);
        }
    }

    /// `<SDL3/SDL_rect.h>` is looked up in the include dirs, `"SDL_rect.h"` next
    /// to the including file.
    fn resolve_include(&self, name: &str, quoted: bool, from: &Path) -> Option<PathBuf> {
        if quoted {
            if let Some(dir) = from.parent() {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        for dir in &self.include_dirs {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // declarations
    // -----------------------------------------------------------------------

    /// Walk the header body, cutting it into declarations.
    fn declarations(&mut self, code: &str, header: &Path) {
        let mut buf = String::new();
        let mut paren = 0u32;
        let mut bracket = 0u32;
        let mut brace = 0u32;
        let mut skipping = false;
        let mut skip_depth = 0u32;
        for c in code.chars() {
            if skipping {
                match c {
                    '{' => skip_depth += 1,
                    '}' => {
                        skip_depth -= 1;
                        if skip_depth == 0 {
                            skipping = false;
                        }
                    }
                    _ => {}
                }
                continue;
            }
            match c {
                '{' if brace == 0 => {
                    let head = buf.trim();
                    if head.is_empty() {
                        // a stray brace (a macro tail that survived): nothing to
                        // attach it to, so ignore it
                    } else if head.starts_with("typedef") || starts_record(head) {
                        brace += 1;
                        buf.push(c);
                    } else {
                        // a function definition body or similar: not a declaration
                        skipping = true;
                        skip_depth = 1;
                        buf.clear();
                        paren = 0;
                        bracket = 0;
                    }
                }
                '{' => {
                    brace += 1;
                    buf.push(c);
                }
                '}' if brace > 0 => {
                    brace -= 1;
                    buf.push(c);
                }
                '}' => {}
                '(' => {
                    paren += 1;
                    buf.push(c);
                }
                ')' => {
                    paren = paren.saturating_sub(1);
                    buf.push(c);
                }
                '[' => {
                    bracket += 1;
                    buf.push(c);
                }
                ']' => {
                    bracket = bracket.saturating_sub(1);
                    buf.push(c);
                }
                ';' if paren == 0 && bracket == 0 && brace == 0 => {
                    self.declaration(buf.trim(), header);
                    buf.clear();
                }
                _ => buf.push(c),
            }
        }
        // a trailing partial declaration is not a declaration
    }

    fn declaration(&mut self, unit: &str, header: &Path) {
        let unit = normalize(unit);
        if unit.is_empty() {
            return;
        }
        if unit.starts_with("typedef") {
            self.typedef_decl(&unit, header);
        } else if PROTOTYPE_DECORATIONS
            .iter()
            .any(|word| starts_with_word(&unit, word))
        {
            self.function_decl(&unit, header);
        }
    }

    fn function_decl(&mut self, unit: &str, header: &Path) {
        let text = normalize(&strip_words(&strip_attributes(unit), FUNCTION_NOISE));
        let Some(open) = text.find('(') else { return };
        let Some(close) = matching(&text, open) else {
            return;
        };
        let head = text[..open].trim();
        let Some((name_start, name)) = last_ident(head) else {
            return;
        };
        if name_start == 0 {
            // no return type: not a declaration shape we import
            return;
        }
        let ret = parse_type(&head[..name_start]);
        let mut skipped: Option<String> = None;

        let mut params = Vec::new();
        let raw_params = text[open + 1..close].trim();
        if !(raw_params.is_empty() || raw_params == "void") {
            for raw in split_top_level(raw_params, ',') {
                let param = normalize(&strip_words(&strip_attributes(&raw), FUNCTION_NOISE));
                if param.is_empty() {
                    continue;
                }
                if param == "..." {
                    skipped = Some("variadic".to_string());
                    continue;
                }
                if param.contains('(') {
                    skipped = Some("function-pointer parameter".to_string());
                    continue;
                }
                if param.contains('[') {
                    skipped = Some("array parameter".to_string());
                    continue;
                }
                let (param_name, param_ty) = match last_ident(&param) {
                    Some((start, found)) if start > 0 => {
                        (found.to_string(), parse_type(&param[..start]))
                    }
                    _ => (String::new(), parse_type(&param)),
                };
                if let CType::Unmapped(spelling) = &param_ty {
                    skipped = Some(format!("parameter type `{spelling}` has no mapping"));
                }
                params.push(CParam {
                    name: param_name,
                    ty: param_ty,
                });
            }
        }
        if skipped.is_none() {
            if let CType::Unmapped(spelling) = &ret {
                skipped = Some(format!("return type `{spelling}` has no mapping"));
            }
        }

        if !self.seen_functions.insert(name.to_string()) {
            return;
        }
        self.module.functions.push(CFunction {
            name: name.to_string(),
            params,
            ret,
            skipped,
            header: header.to_path_buf(),
        });
    }

    fn typedef_decl(&mut self, unit: &str, header: &Path) {
        let text = normalize(unit.strip_prefix("typedef").unwrap_or(unit));
        if text.is_empty() {
            return;
        }

        if text.contains('{') {
            self.record_or_enum(&text, header);
            return;
        }

        // `typedef <ret> (CALLBACK *Name)(...)` - a typedef'd function pointer is
        // not callable, and its C type has no Rill shape: report it unmapped so
        // consumers skip it
        if let Some(open) = text.find('(') {
            if let Some(close) = matching(&text, open) {
                let inner = &text[open + 1..close];
                if inner.contains('*') {
                    if let Some((_, name)) = last_ident(inner) {
                        self.push_type(CTypeDecl {
                            name: name.to_string(),
                            kind: CTypeKind::Alias(CType::Unmapped(format!("typedef {text}"))),
                            header: header.to_path_buf(),
                        });
                    }
                    return;
                }
            }
        }

        // `typedef struct X Y;` / `typedef union X Y;`: declared, never defined
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() == 3
            && matches!(words[0], "struct" | "union")
            && is_ident(words[1])
            && is_ident(words[2])
        {
            self.push_type(CTypeDecl {
                name: words[2].to_string(),
                kind: CTypeKind::Opaque,
                header: header.to_path_buf(),
            });
            return;
        }

        // `typedef <type> <Name>;`
        let Some((name_start, name)) = last_ident(&text) else {
            return;
        };
        if name_start == 0 {
            return;
        }
        let spelled = normalize(&text[..name_start]);
        if spelled.is_empty() {
            return;
        }
        if text.contains('[') {
            // an array typedef has no Rill shape
            self.push_type(CTypeDecl {
                name: name.to_string(),
                kind: CTypeKind::Alias(CType::Unmapped(format!("typedef {text}"))),
                header: header.to_path_buf(),
            });
            return;
        }
        let mut inner = parse_type(&spelled);
        if let CType::Named(other) = &inner {
            if let Some(resolved) = self.alias_inner.get(other) {
                inner = resolved.clone();
            }
        }
        if self.seen_types.insert(name.to_string()) {
            self.alias_inner.insert(name.to_string(), inner.clone());
            self.module.types.push(CTypeDecl {
                name: name.to_string(),
                kind: CTypeKind::Alias(inner),
                header: header.to_path_buf(),
            });
        }
    }

    /// `typedef struct/union/enum ... { ... } Name;`
    fn record_or_enum(&mut self, text: &str, header: &Path) {
        let Some(open) = text.find('{') else { return };
        let Some(close) = matching(text, open) else {
            return;
        };
        let head = normalize(&text[..open]);
        let keyword = head.split_whitespace().next().unwrap_or("");
        let tail = normalize(&text[close + 1..]);
        let Some((_, name)) = last_ident(&tail) else {
            return;
        };
        let name = name.to_string();
        let body = &text[open + 1..close];
        match keyword {
            "enum" => {
                if !self.seen_types.insert(name.clone()) {
                    return;
                }
                self.module.types.push(CTypeDecl {
                    name,
                    kind: CTypeKind::Enum,
                    header: header.to_path_buf(),
                });
                self.enumerators(body, header);
            }
            "struct" | "union" => {
                let fields = parse_fields(body);
                if !self.seen_types.insert(name.clone()) {
                    return;
                }
                self.module.types.push(CTypeDecl {
                    name,
                    kind: CTypeKind::Record(fields),
                    header: header.to_path_buf(),
                });
            }
            _ => {}
        }
    }

    /// Every enumerator of `typedef enum ... { ... }`, with implicit values
    /// computed (+1) and every value as decimal text.
    fn enumerators(&mut self, body: &str, header: &Path) {
        let mut values: HashMap<String, i64> = HashMap::new();
        let mut previous: Option<i64> = None;
        for item in split_top_level(body, ',') {
            let item = normalize(&item);
            if item.is_empty() {
                continue;
            }
            let (name, expr) = match item.split_once('=') {
                Some((left, right)) => (normalize(left), Some(normalize(right))),
                None => (item.clone(), None),
            };
            if !is_ident(&name) {
                continue;
            }
            let computed = match &expr {
                Some(expr) => eval_int(expr, &values),
                None => previous.map(|value| value + 1).or(Some(0)),
            };
            let value = match (computed, &expr) {
                (Some(value), _) => value.to_string(),
                // an enum value we cannot fold is kept as written
                (None, Some(expr)) => expr.clone(),
                (None, None) => continue,
            };
            if let Some(value) = computed {
                values.insert(name.clone(), value);
                previous = Some(value);
            }
            self.push_constant(CConstant {
                name,
                value,
                ty: CType::Int {
                    bits: 32,
                    signed: true,
                },
                header: header.to_path_buf(),
            });
        }
    }

    /// `#define NAME <literal>`, skipping function-like and non-literal macros.
    fn define(&mut self, rest: &str, header: &Path) {
        let rest = rest.trim_start();
        let name_end = rest
            .bytes()
            .position(|c| !is_ident_byte(c))
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        if !is_ident(name) {
            return;
        }
        let after = &rest[name_end..];
        if after.starts_with('(') {
            // function-like macro
            return;
        }
        let body = normalize(after);
        if body.is_empty() {
            return;
        }
        let Some(ty) = literal_type(&body) else {
            return;
        };
        self.push_constant(CConstant {
            name: name.to_string(),
            value: body,
            ty,
            header: header.to_path_buf(),
        });
    }

    fn push_type(&mut self, decl: CTypeDecl) {
        if self.seen_types.insert(decl.name.clone()) {
            self.module.types.push(decl);
        }
    }

    fn push_constant(&mut self, constant: CConstant) {
        if self.seen_constants.insert(constant.name.clone()) {
            self.module.constants.push(constant);
        }
    }
}

// ---------------------------------------------------------------------------
// type spelling -> CType
// ---------------------------------------------------------------------------

/// How a header marks a prototype, which is what the scanner keys on: SDL writes
/// `extern SDL_DECLSPEC …` and raylib writes `RLAPI …`. A declaration carrying
/// none of these is not imported - a variable declaration has the same shape as
/// one, so the marker is what tells them apart.
const PROTOTYPE_DECORATIONS: &[&str] = &["extern", "SDL_DECLSPEC", "SDLMAIN_DECLSPEC", "RLAPI"];

/// Macro-ish words that may decorate a declaration or a type; they are dropped
/// before the text is split into a return type, a name and parameters.
const FUNCTION_NOISE: &[&str] = &[
    "extern",
    "static",
    "inline",
    "SDL_DECLSPEC",
    "SDLMAIN_DECLSPEC",
    "SDLCALL",
    "RLAPI",
    "__cdecl",
    "SDL_PRINTF_FORMAT_STRING",
    "SDL_PRINTF_VARARG_FUNC",
    "SDL_FORCE_INLINE",
    "SDL_INLINE",
];

const TYPE_NOISE: &[&str] = &[
    "const",
    "volatile",
    "restrict",
    "__restrict",
    "__restrict__",
    "extern",
    "static",
    "inline",
    "__inline",
    "__inline__",
    "SDL_DECLSPEC",
    "SDLMAIN_DECLSPEC",
    "SDLCALL",
    "__cdecl",
    "SDL_PRINTF_FORMAT_STRING",
    "SDL_RESTRICT",
    "SDL_FORCE_INLINE",
    "SDL_INLINE",
    "_Nullable",
    "_Nonnull",
];

/// The all-caps `SDL_*` macros SDL decorates prototypes with: they never name
/// the declared thing, so they are dropped wherever they appear. Names that do
/// name types (`SDL_GUID`, `SDL_TLSID`) are deliberately absent.
const ATTRIBUTE_MACROS: &[&str] = &[
    "SDL_DECLSPEC",
    "SDLMAIN_DECLSPEC",
    "SDLCALL",
    "SDL_MALLOC",
    "SDL_ALLOC_SIZE",
    "SDL_ALLOC_SIZE2",
    "SDL_PRINTF_FORMAT_STRING",
    "SDL_SCANF_FORMAT_STRING",
    "SDL_PRINTF_VARARG_FUNC",
    "SDL_PRINTF_VARARG_FUNCV",
    "SDL_SCANF_VARARG_FUNC",
    "SDL_SCANF_VARARG_FUNCV",
    "SDL_WPRINTF_VARARG_FUNC",
    "SDL_WPRINTF_VARARG_FUNCV",
    "SDL_IN_BYTECAP",
    "SDL_OUT_BYTECAP",
    "SDL_INOUT_BYTECAP",
    "SDL_IN_CAP",
    "SDL_OUT_CAP",
    "SDL_INOUT_CAP",
    "SDL_IN_Z_CAP",
    "SDL_OUT_Z_CAP",
    "SDL_INOUT_Z_CAP",
    "SDL_ACQUIRE",
    "SDL_ACQUIRE_SHARED",
    "SDL_RELEASE",
    "SDL_RELEASE_GENERIC",
    "SDL_RELEASE_SHARED",
    "SDL_TRY_ACQUIRE",
    "SDL_TRY_ACQUIRE_SHARED",
    "SDL_ACQUIRED_BEFORE",
    "SDL_ACQUIRED_AFTER",
    "SDL_REQUIRES",
    "SDL_REQUIRES_SHARED",
    "SDL_RETURN_CAPABILITY",
    "SDL_EXCLUDES",
    "SDL_UNUSED",
    "SDL_FALLTHROUGH",
    "SDL_DEPRECATED",
    "SDL_ANALYZER_NORETURN",
    "SDL_WEAK",
    "SDL_SIMD_ALIGNED",
    "SDL_PREALLOC",
    "SDL_RESTRICT",
    "SDL_ZERO_TERMINATED",
    "SDL_FILE",
    "SDL_LINE",
    "SDL_FUNCTION",
    "SDL_FUNCTION_POINTER_IS_VOID_POINTER",
];

/// Parse one type spelling. Pointer levels are wrapped outside-in, and
/// `const char *` is the crate's [`CType::Text`] rather than a pointer to char.
fn parse_type(text: &str) -> CType {
    let spelled = normalize(&strip_attributes(text));
    if spelled.is_empty() {
        return CType::Unmapped(spelled);
    }

    // `const`/`volatile` before the first `*` belongs to the pointee, which is
    // what decides whether the pointer is mutable
    let first_star = spelled.find('*').unwrap_or(spelled.len());
    let pointee_const = spelled[..first_star].split_whitespace().any(|word| {
        matches!(
            word,
            "const" | "volatile" | "restrict" | "__restrict" | "__restrict__" | "SDL_RESTRICT"
        )
    });
    let stars = spelled.matches('*').count();

    let cleaned = strip_words(&spelled, TYPE_NOISE);
    let mut words: Vec<&str> = cleaned
        .split(|c: char| c == '*' || c.is_whitespace())
        .filter(|word| !word.is_empty())
        .collect();
    // `struct X`, `union X`, `enum X` all name the type itself
    if words.len() == 2 && matches!(words[0], "struct" | "union" | "enum") {
        words = vec![words[1]];
    }

    let base = match words.as_slice() {
        [] => None,
        words => primitive(words).or_else(|| match words {
            [name] if is_ident(name) => Some(CType::Named((*name).to_string())),
            _ => None,
        }),
    };
    let mut ty = base.unwrap_or_else(|| CType::Unmapped(spelled.clone()));

    if stars == 1
        && pointee_const
        && matches!(
            ty,
            CType::Int {
                bits: 8,
                signed: true
            }
        )
    {
        return CType::Text;
    }
    for _ in 0..stars {
        ty = CType::Ptr {
            mutable: !pointee_const,
            inner: Box::new(ty),
        };
    }
    ty
}

/// The C primitives and the fixed-width spellings SDL uses for them. Names like
/// `Uint32`/`Sint8` are recognised directly so `typedef Uint32 SDL_InitFlags;`
/// resolves without needing `SDL_stdinc.h`'s `typedef uint32_t Uint32;` first.
fn primitive(words: &[&str]) -> Option<CType> {
    let int = |bits: u8, signed: bool| Some(CType::Int { bits, signed });
    match words.join(" ").as_str() {
        "void" => Some(CType::Void),
        "bool" | "_Bool" => Some(CType::Bool),
        "char" | "signed char" => int(8, true),
        "unsigned char" => int(8, false),
        "short" | "short int" | "signed short" | "signed short int" => int(16, true),
        "unsigned short" | "unsigned short int" => int(16, false),
        "int" | "signed" | "signed int" => int(32, true),
        "unsigned" | "unsigned int" => int(32, false),
        "long" | "long int" | "signed long" | "signed long int" => int(64, true),
        "unsigned long" | "unsigned long int" => int(64, false),
        "long long" | "long long int" | "signed long long" | "signed long long int" => {
            int(64, true)
        }
        "unsigned long long" | "unsigned long long int" => int(64, false),
        "size_t" => int(64, false),
        "ssize_t" | "ptrdiff_t" | "intptr_t" => int(64, true),
        "uintptr_t" => int(64, false),
        "float" => Some(CType::Float { bits: 32 }),
        "double" => Some(CType::Float { bits: 64 }),
        "int8_t" | "Sint8" => int(8, true),
        "uint8_t" | "Uint8" => int(8, false),
        "int16_t" | "Sint16" => int(16, true),
        "uint16_t" | "Uint16" => int(16, false),
        "int32_t" | "Sint32" => int(32, true),
        "uint32_t" | "Uint32" => int(32, false),
        "int64_t" | "Sint64" => int(64, true),
        "uint64_t" | "Uint64" => int(64, false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// record fields
// ---------------------------------------------------------------------------

fn parse_fields(body: &str) -> Vec<CField> {
    let mut fields = Vec::new();
    for unit in split_top_level(body, ';') {
        let unit = normalize(&unit);
        if unit.is_empty() {
            continue;
        }
        let first_word = unit.split_whitespace().next().unwrap_or("");
        if matches!(first_word, "struct" | "union" | "enum") {
            nested_record(&unit, &mut fields);
            continue;
        }
        // `int w, h;` declares several fields that share the type
        let parts = split_top_level(&unit, ',');
        let first = strip_bitfield(&normalize(&parts[0]));
        // the array suffix is cut off first: its size may name an enumerator
        let (declarator, suffix) = split_array(&first);
        let Some((name_start, name)) = last_ident(declarator) else {
            continue;
        };
        let spelled = normalize(&declarator[..name_start]);
        if spelled.is_empty() {
            // an unnamed declarator: `Uint32 : 4;` or `Uint32 [4];`
            let mut anonymous = declarator.to_string();
            anonymous.push_str(suffix);
            fields.push(CField {
                name: String::new(),
                ty: parse_type(&anonymous),
            });
            continue;
        }
        let ty = if suffix.is_empty() {
            parse_type(&spelled)
        } else {
            // arrays have no Rill shape; keep the spelling minus the field name
            CType::Unmapped(normalize(&format!("{spelled} {suffix}")))
        };
        fields.push(CField {
            name: name.to_string(),
            ty: ty.clone(),
        });
        for part in &parts[1..] {
            let part = strip_bitfield(&normalize(part));
            if let Some((_, name)) = last_ident(&part) {
                fields.push(CField {
                    name: name.to_string(),
                    ty: ty.clone(),
                });
            }
        }
    }
    fields
}

/// A field whose type is declared inline: `struct SDL_Foo { ... } foo;`,
/// `struct SDL_Foo foo;` or an anonymous `union { ... } value;`.
fn nested_record(unit: &str, fields: &mut Vec<CField>) {
    let Some(open) = unit.find('{') else {
        // `struct TAG name;`
        let Some((name_start, name)) = last_ident(unit) else {
            return;
        };
        let head = normalize(&unit[..name_start]);
        if let Some(tag) = head.split_whitespace().nth(1) {
            fields.push(CField {
                name: name.to_string(),
                ty: CType::Named(tag.to_string()),
            });
        }
        return;
    };
    let Some(close) = matching(unit, open) else {
        return;
    };
    let head = normalize(&unit[..open]);
    let kind = head.split_whitespace().next().unwrap_or("struct");
    let tag = head.split_whitespace().nth(1).map(str::to_string);
    let inner = parse_fields(&unit[open + 1..close]);
    let tail = normalize(&unit[close + 1..]);
    if let Some(tag) = tag {
        for name in declarators(&tail) {
            fields.push(CField {
                name,
                ty: CType::Named(tag.clone()),
            });
        }
    } else if tail.is_empty() {
        // anonymous and unused as a named field: keep the members themselves
        fields.extend(inner);
    } else {
        for name in declarators(&tail) {
            fields.push(CField {
                name,
                ty: CType::Unmapped(format!("<anonymous {kind}>")),
            });
        }
    }
}

fn declarators(text: &str) -> Vec<String> {
    split_top_level(text, ',')
        .iter()
        .filter_map(|part| {
            let part = strip_bitfield(&normalize(part));
            last_ident(&part).map(|(_, name)| name.to_string())
        })
        .collect()
}

fn strip_bitfield(text: &str) -> String {
    match text.find(':') {
        Some(index) => text[..index].trim_end().to_string(),
        None => text.to_string(),
    }
}

/// Split a declarator into the part before its array suffix and the suffix
/// itself (`float data[3]` -> `float data`, `[3]`).
fn split_array(text: &str) -> (&str, &str) {
    let mut depth = 0i32;
    for (index, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '[' if depth == 0 => return (&text[..index], &text[index..]),
            _ => {}
        }
    }
    (text, "")
}

// ---------------------------------------------------------------------------
// literals
// ---------------------------------------------------------------------------

/// The type of a `#define` body, or `None` when the body is not a single
/// literal (an expression, a string, a call...).
fn literal_type(body: &str) -> Option<CType> {
    let text = body.trim();
    if text.contains(char::is_whitespace) {
        return None;
    }
    if matches!(text, "true" | "false" | "bool") {
        return Some(CType::Bool);
    }
    let (negative, digits_text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if digits_text.is_empty() {
        return None;
    }

    // integer literal, with the usual u/l/ull suffixes
    let digits = digits_text.trim_end_matches(['u', 'U', 'l', 'L']);
    let suffix = &digits_text[digits.len()..];
    let (radix, body) = if let Some(rest) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, rest)
    } else {
        (10, digits)
    };
    let well_formed = !body.is_empty()
        && body.chars().all(|c| {
            if radix == 16 {
                c.is_ascii_hexdigit()
            } else {
                c.is_ascii_digit()
            }
        });
    if well_formed {
        let value = i64::from_str_radix(body, radix).ok();
        let suffix = suffix.to_ascii_lowercase();
        let unsigned = !negative && suffix.contains('u');
        let too_wide = match value {
            Some(value) => value > i64::from(i32::MAX),
            None => true,
        };
        let wide = suffix.contains('l') || too_wide;
        return Some(CType::Int {
            bits: if wide { 64 } else { 32 },
            signed: !unsigned,
        });
    }

    // float literal: `1.5`, `1.5f`, `1e9`
    let (mantissa, single) = match digits_text.strip_suffix(['f', 'F']) {
        Some(rest) => (rest, true),
        None => (digits_text, false),
    };
    let is_float = mantissa.contains('.')
        && !mantissa.is_empty()
        && mantissa.chars().all(|c| {
            c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-'
        });
    if is_float {
        return Some(CType::Float {
            bits: if single { 32 } else { 64 },
        });
    }
    None
}

/// Fold an integer constant expression as far as it is worth it: literals and
/// enumerators already seen. Anything else is left unresolved.
fn eval_int(expr: &str, known: &HashMap<String, i64>) -> Option<i64> {
    let text = normalize(expr);
    let numeric =
        text.starts_with(|c: char| c.is_ascii_digit()) || (text.starts_with('-') && text.len() > 1);
    if numeric {
        let digits = text.trim_end_matches(['u', 'U', 'l', 'L']);
        let (negative, digits) = match digits.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, digits),
        };
        let value = if let Some(rest) = digits
            .strip_prefix("0x")
            .or_else(|| digits.strip_prefix("0X"))
        {
            i64::from_str_radix(rest, 16).ok()
        } else {
            digits.parse::<i64>().ok()
        };
        return value.map(|value| if negative { -value } else { value });
    }
    known.get(&text).copied()
}

// ---------------------------------------------------------------------------
// text helpers
// ---------------------------------------------------------------------------

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_ident(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with(|c: char| c.is_ascii_digit())
        && text.bytes().all(is_ident_byte)
}

fn starts_record(text: &str) -> bool {
    matches!(
        text.split_whitespace().next(),
        Some("struct" | "union" | "enum")
    )
}

/// Whether `text` starts with `word` as a whole word (`RLAPI`, not `RLAPIX`).
fn starts_with_word(text: &str, word: &str) -> bool {
    match text.strip_prefix(word) {
        Some(rest) => !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'),
        None => false,
    }
}

/// Drop SDL's attribute macros, with their argument list when they have one -
/// `SDL_ALLOC_SIZE2(1, 2)`, `SDL_OUT_BYTECAP(len)`, `SDL_PRINTF_VARARG_FUNC(1)`.
fn strip_attributes(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if is_ident_byte(bytes[index]) {
            let start = index;
            while index < bytes.len() && is_ident_byte(bytes[index]) {
                index += 1;
            }
            let word = &text[start..index];
            if !ATTRIBUTE_MACROS.contains(&word) {
                out.push_str(word);
                continue;
            }
            out.push(' ');
            let mut look = index;
            while look < bytes.len() && bytes[look].is_ascii_whitespace() {
                look += 1;
            }
            if look < bytes.len() && bytes[look] == b'(' {
                if let Some(close) = matching(text, look) {
                    index = close + 1;
                }
            }
        } else {
            let ch = text[index..].chars().next().unwrap_or(' ');
            out.push(ch);
            index += ch.len_utf8();
        }
    }
    out
}

/// Remove whole-word occurrences of `words`, leaving the rest of the text alone.
fn strip_words(text: &str, words: &[&str]) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if is_ident_byte(bytes[index]) {
            let start = index;
            while index < bytes.len() && is_ident_byte(bytes[index]) {
                index += 1;
            }
            let word = &text[start..index];
            if words.contains(&word) {
                out.push(' ');
            } else {
                out.push_str(word);
            }
        } else {
            let ch = text[index..].chars().next().unwrap_or(' ');
            out.push(ch);
            index += ch.len_utf8();
        }
    }
    out
}

/// The last identifier in `text` that does not start with a digit, with its
/// byte offset. Used to split `type name` at the declarator.
fn last_ident(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();
    let mut found = None;
    let mut index = 0;
    while index < bytes.len() {
        if is_ident_byte(bytes[index]) && !bytes[index].is_ascii_digit() {
            let start = index;
            while index < bytes.len() && is_ident_byte(bytes[index]) {
                index += 1;
            }
            found = Some((start, index));
        } else {
            index += 1;
        }
    }
    found.map(|(start, end)| (start, &text[start..end]))
}

/// Index of the delimiter closing the one at `open`, honouring nesting.
fn matching(text: &str, open: usize) -> Option<usize> {
    let opener = text.as_bytes()[open] as char;
    let closer = match opener {
        '(' => ')',
        '{' => '}',
        '[' => ']',
        _ => return None,
    };
    let mut depth = 0i32;
    for (index, c) in text.char_indices().filter(|(index, _)| *index >= open) {
        if c == opener {
            depth += 1;
        } else if c == closer {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

/// Split on `sep` at nesting depth zero, so commas inside parameter lists or
/// record bodies do not cut a declaration apart.
fn split_top_level(text: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    for c in text.chars() {
        match c {
            '(' => {
                paren += 1;
                current.push(c);
            }
            ')' => {
                paren -= 1;
                current.push(c);
            }
            '[' => {
                bracket += 1;
                current.push(c);
            }
            ']' => {
                bracket -= 1;
                current.push(c);
            }
            '{' => {
                brace += 1;
                current.push(c);
            }
            '}' => {
                brace -= 1;
                current.push(c);
            }
            _ if c == sep && paren <= 0 && bracket <= 0 && brace <= 0 => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
}

/// `#define`-like directive keyword check: `include` matches `#include <...>`
/// but not `#include_next`.
fn strip_directive<'a>(rest: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = rest.strip_prefix(keyword)?;
    match rest.bytes().next() {
        Some(byte) if is_ident_byte(byte) => None,
        _ => Some(rest),
    }
}

/// Remove comments, keeping string and char literals (and line structure) intact.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                while let Some(&next) = chars.peek() {
                    if next == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    if next == '\n' {
                        out.push('\n');
                    }
                    previous = next;
                }
            }
            '"' | '\'' => {
                let quote = c;
                out.push(c);
                let mut escaped = false;
                for next in chars.by_ref() {
                    out.push(next);
                    if escaped {
                        escaped = false;
                    } else if next == '\\' {
                        escaped = true;
                    } else if next == quote {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_types() {
        assert_eq!(
            literal_type("0x00000020u"),
            Some(CType::Int {
                bits: 32,
                signed: false
            })
        );
        assert_eq!(
            literal_type("0x1FFFFFFFF"),
            Some(CType::Int {
                bits: 64,
                signed: true
            })
        );
        assert_eq!(
            literal_type("0xFFFFFFFFFFFFFFFFull"),
            Some(CType::Int {
                bits: 64,
                signed: false
            })
        );
        assert_eq!(
            literal_type("42"),
            Some(CType::Int {
                bits: 32,
                signed: true
            })
        );
        assert_eq!(
            literal_type("-1"),
            Some(CType::Int {
                bits: 32,
                signed: true
            })
        );
        assert_eq!(literal_type("1.5f"), Some(CType::Float { bits: 32 }));
        assert_eq!(literal_type("1.5"), Some(CType::Float { bits: 64 }));
        assert_eq!(literal_type("true"), Some(CType::Bool));
        assert_eq!(literal_type("(1 << 2)"), None);
        assert_eq!(literal_type("SDL_UINT64_C(1)"), None);
        assert_eq!(literal_type("\"text\""), None);
    }

    #[test]
    fn type_spellings() {
        assert_eq!(parse_type("const char *"), CType::Text);
        assert_eq!(parse_type("void"), CType::Void);
        assert_eq!(
            parse_type("unsigned int"),
            CType::Int {
                bits: 32,
                signed: false
            }
        );
        assert_eq!(
            parse_type("Uint64"),
            CType::Int {
                bits: 64,
                signed: false
            }
        );
        assert_eq!(
            parse_type("int"),
            CType::Int {
                bits: 32,
                signed: true
            }
        );
        assert_eq!(
            parse_type("const SDL_FRect *"),
            CType::Ptr {
                mutable: false,
                inner: Box::new(CType::Named("SDL_FRect".to_string()))
            }
        );
        assert_eq!(
            parse_type("struct SDL_Window *"),
            CType::Ptr {
                mutable: true,
                inner: Box::new(CType::Named("SDL_Window".to_string())),
            }
        );
        assert_eq!(
            parse_type("long double"),
            CType::Unmapped("long double".to_string())
        );
    }

    #[test]
    fn splitting() {
        assert_eq!(
            split_top_level("a, f(x, y), c", ','),
            vec!["a".to_string(), " f(x, y)".to_string(), " c".to_string()]
        );
        assert_eq!(
            last_ident("Uint32 flags : 4").map(|(_, name)| name),
            Some("flags")
        );
        assert_eq!(
            last_ident("Uint32 : 4").map(|(_, name)| name),
            Some("Uint32")
        );
    }

    #[test]
    fn enum_values_fold() {
        let mut known = HashMap::new();
        known.insert("A".to_string(), 5i64);
        assert_eq!(eval_int("A", &known), Some(5));
        assert_eq!(eval_int("0x10u", &known), Some(16));
        assert_eq!(eval_int("A + 1", &known), None);
    }
}
