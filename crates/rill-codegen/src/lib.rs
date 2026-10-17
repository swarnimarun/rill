//! `rill-codegen` — C backend used as the bootstrap target (PLAN.md §7.1).
//!
//! Typed HIR is emitted as portable C plus a small libc-backed runtime
//! (`printf`, `malloc`), then compiled by the platform toolchain:
//!
//! * `--target native` — host `cc`, dynamic: runs locally, so tests can assert
//!   stdout and the exit code;
//! * `--target linux-musl` — `zig cc -target <arch>-linux-musl -static`, giving
//!   a genuinely statically linked ELF.
//!
//! The MLIR backend (`rill-mlir`) replaces this crate once the `rill` dialect
//! lowering lands; the front end and type tables consumed here are unchanged by
//! that switch.
//!
//! Lowered today: functions (including recursion and non-capturing local
//! functions), records, enums, tuples, fixed arrays, ranges in `for`, `if`,
//! `match`, casts, text (`++`, `as String`) and `io::println`.
//! Reported as unsupported rather than miscompiled: `<decides>` (`?`, `|`,
//! `f[x]`), handlers (`option`/`one`/`all`), structured concurrency,
//! projections and `<transacts>` rollback — those arrive with M2–M5.

use std::collections::HashSet;

use rill_ast::{BinaryOp, LiteralKind, UnaryOp};
use rill_diagnostics::{Diagnostic, Diagnostics};
use rill_hir::{
    Block, Cond, ElseBranch, Expr, ExprKind, Hir, ItemId, ItemKind, LocalId, Pattern, Res, Stmt,
};
use rill_span::{Span, Symbol};
use rill_ty::{Checked, ItemTy, NominalId, NominalKind, Ty};

/// Emits a complete C translation unit, or diagnostics explaining what could
/// not be lowered.
pub fn emit_c(
    frontend: &rill_ast::Frontend,
    hir: &Hir,
    checked: &Checked,
) -> (String, Diagnostics) {
    let mut gen = Generator::new(frontend, hir, checked);
    gen.module();
    let diagnostics = std::mem::take(&mut gen.diagnostics);
    (gen.finish(), diagnostics)
}

const RUNTIME: &str = r#"// ---- rill runtime (libc-backed bootstrap) ---------------------------------
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

typedef int8_t   r_i8;
typedef int16_t  r_i16;
typedef int32_t  r_i32;
typedef int64_t  r_i64;
typedef uint8_t  r_u8;
typedef uint16_t r_u16;
typedef uint32_t r_u32;
typedef uint64_t r_u64;
typedef float    r_f32;
typedef double   r_f64;
typedef int32_t  r_bool;
typedef const char *r_text;
typedef struct { int32_t unused; } r_unit;

static r_unit rill_unit(void) { r_unit u; u.unused = 0; return u; }

// Text is a heap string; the arena keeps concat/formatting allocation-free per
// call, which is fine for bootstrap-scale programs.
static char rill_text_arena[1 << 20];
static size_t rill_text_used = 0;

static r_text rill_text_alloc(size_t n) {
  if (rill_text_used + n > sizeof(rill_text_arena)) {
    rill_text_used = 0;
  }
  char *p = &rill_text_arena[rill_text_used];
  rill_text_used += n;
  return p;
}

static r_text rill_text_concat(r_text a, r_text b) {
  size_t la = strlen(a), lb = strlen(b);
  char *p = (char *)rill_text_alloc(la + lb + 1);
  memcpy(p, a, la);
  memcpy(p + la, b, lb);
  p[la + lb] = 0;
  return p;
}

static r_text rill_i64_to_text(r_i64 v) {
  char *p = (char *)rill_text_alloc(32);
  snprintf(p, 32, "%lld", (long long)v);
  return p;
}

static r_text rill_f64_to_text(r_f64 v) {
  char *p = (char *)rill_text_alloc(64);
  snprintf(p, 64, "%g", (double)v);
  return p;
}

static void rill_println(r_text s) { printf("%s\n", s); }

// Maps a file read-only; the program gets its input without copying it.
static r_text rill_map_file(r_text path) {
  int fd = open(path, O_RDONLY);
  if (fd < 0) { return ""; }
  struct stat st;
  if (fstat(fd, &st) != 0 || st.st_size == 0) { close(fd); return ""; }
  void *data = mmap(NULL, (size_t)st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
  if (data == MAP_FAILED) { close(fd); return ""; }
  return (r_text)data;
}

static r_i64 rill_file_size(r_text path) {
  int fd = open(path, O_RDONLY);
  if (fd < 0) { return 0; }
  struct stat st;
  r_i64 size = (fstat(fd, &st) == 0) ? (r_i64)st.st_size : 0;
  close(fd);
  return size;
}

// Reads a whole file as text; programs get their input without a stdlib.
static r_text rill_read_file(r_text path) {
  FILE *f = fopen(path, "rb");
  if (!f) { return ""; }
  fseek(f, 0, SEEK_END);
  long size = ftell(f);
  fseek(f, 0, SEEK_SET);
  char *buffer = (char *)malloc((size_t)size + 1);
  if (!buffer) { fclose(f); return ""; }
  size_t read = fread(buffer, 1, (size_t)size, f);
  buffer[read] = 0;
  fclose(f);
  return buffer;
}
static r_i64 rill_size_i64(r_i64 n) { return n; }
static size_t rill_size_slice(size_t n) { return n; }
// ---- end runtime ---------------------------------------------------------
"#;

struct Generator<'a> {
    frontend: &'a rill_ast::Frontend,
    hir: &'a Hir,
    checked: &'a Checked,
    runtime: String,
    types: String,
    protos: String,
    defs: String,
    /// Statements of the function currently being emitted.
    body: String,
    temp: usize,
    emitted_types: HashSet<String>,
    diagnostics: Diagnostics,
    scopes: Vec<Vec<(LocalId, String)>>,
    /// `inout` parameters are C pointers; the local reads as `(*p_x)` so every
    /// existing place/expression path keeps working.
    inout_locals: std::collections::HashMap<LocalId, String>,
}

impl<'a> Generator<'a> {
    fn new(frontend: &'a rill_ast::Frontend, hir: &'a Hir, checked: &'a Checked) -> Self {
        Generator {
            frontend,
            hir,
            checked,
            runtime: RUNTIME.to_string(),
            types: String::new(),
            protos: String::new(),
            defs: String::new(),
            body: String::new(),
            temp: 0,
            emitted_types: HashSet::new(),
            diagnostics: Diagnostics::new(),
            scopes: Vec::new(),
            inout_locals: std::collections::HashMap::new(),
        }
    }

    fn finish(self) -> String {
        format!(
            "{}{}{}{}\n",
            self.runtime, self.types, self.protos, self.defs
        )
    }

    fn text(&self, sym: Symbol) -> &str {
        self.frontend.interner.get(sym)
    }

    fn err(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(span, message));
    }

    fn unsupported(&mut self, span: Span, what: &str, milestone: &str) {
        self.diagnostics.push(
            Diagnostic::error(span, format!("{what} is not lowered by the C backend yet"))
                .note(format!("planned with {milestone} in PLAN.md")),
        );
    }

    // -- module ------------------------------------------------------------

    fn module(&mut self) {
        // A module scope holds the file-scope statics below, so functions can
        // refer to them without parameters.
        self.push_scope();
        // Type definitions first: structs may reference each other.
        for item in self.hir.items() {
            let ItemKind::Type(def) = &item.kind else {
                continue;
            };
            let Some(ItemTy::Nominal(id)) = self.checked.item_ty(item.id) else {
                continue;
            };
            let id = *id;
            match &def.body {
                rill_ast::TypeBody::Record(_) | rill_ast::TypeBody::View(_) => {
                    match &self.checked.nominal(id).kind {
                        NominalKind::Record(fields) | NominalKind::View(fields) => {
                            let name = format!("st_{}", self.nominal_name(id));
                            if self.emitted_types.insert(name.clone()) {
                                let mut body = format!("typedef struct {} {{\n", name);
                                for field in fields {
                                    let ty = self.c_type(&field.ty);
                                    let fname = self.text(field.name).to_string();
                                    body.push_str(&format!("  {ty} {fname};\n"));
                                }
                                body.push_str(&format!("}} {name};\n"));
                                self.types.push_str(&body);
                            }
                        }
                        _ => {}
                    }
                }
                rill_ast::TypeBody::Variants(_) => {
                    self.emit_enum(id);
                }
                _ => {}
            }
        }

        for item in self.hir.items() {
            match &item.kind {
                ItemKind::Fn(def) => {
                    if def.body.is_none() {
                        continue; // signatures/trait members have no body to emit
                    }
                    self.emit_fn(item.id, def);
                }
                ItemKind::Signature(def) => {
                    let _ = def;
                }
                ItemKind::Project(_) => {
                    self.unsupported(item.span, "`project` bodies", "M2/M4");
                }
                ItemKind::Builder(_)
                | ItemKind::Trait(_)
                | ItemKind::Impl(_)
                | ItemKind::Use(_) => {}
                ItemKind::Stmt(stmt) => self.module_static(stmt),
                ItemKind::Type(_) | ItemKind::Placeholder => {}
            }
        }
    }

    /// Module-level `var` / `x := …` declarations become file-scope statics:
    /// pools, arenas and scratch buffers live outside the stack.
    fn module_static(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Var {
                local, ty, init, ..
            } => {
                let init_code = self.expr(init);
                let checked_ty = self.local_ty(*local);
                let declared = if matches!(checked_ty, Ty::Error) {
                    ty.as_ref()
                        .cloned()
                        .map(|ast| self.syntax_to_c(&ast))
                        .unwrap_or_else(|| "r_i64".to_string())
                } else {
                    self.c_type(&checked_ty)
                };
                let name = format!("g_{}", self.text(self.hir.local(*local).name));
                self.defs
                    .push_str(&format!("static {declared} {name} = {init_code};\n"));
                self.bind(*local, name);
            }
            Stmt::Bind { pattern, value, .. } => {
                // Let a module-level binding initialise from a static too.
                let value_code = self.expr(value);
                self.bind_pattern(pattern, &value_code);
            }
            Stmt::Expr(expr) => {
                let value = self.expr(expr);
                self.emit_discarded(&value);
                // Module-level expressions run before `main`; keep them in a
                // synthetic initialiser so they cannot land in a function body.
                let body = std::mem::take(&mut self.body);
                self.defs.push_str(&body);
            }
            _ => {}
        }
    }

    fn nominal_name(&self, id: NominalId) -> String {
        let name = self.checked.nominal(id).name;
        let text = self.text(name).to_string();
        if text.is_empty() {
            format!("anon{}", id.0)
        } else {
            text
        }
    }

    fn emit_enum(&mut self, id: NominalId) {
        let name = format!("en_{}", self.nominal_name(id));
        if !self.emitted_types.insert(name.clone()) {
            return;
        }
        let variants = match &self.checked.nominal(id).kind {
            NominalKind::Enum(variants) => variants.clone(),
            _ => return,
        };
        let mut body = format!("typedef struct {name} {{\n  int32_t tag;\n  union {{\n");
        for (index, variant) in variants.iter().enumerate() {
            let vname = self.text(variant.name).to_string();
            if variant.payload.is_empty() {
                body.push_str(&format!("    int32_t _{index}; // {vname}\n"));
            } else {
                body.push_str("    struct {\n");
                for (field, ty) in variant.payload.iter().enumerate() {
                    let ty = self.c_type(ty);
                    body.push_str(&format!("      {ty} _{field};\n"));
                }
                body.push_str(&format!("    }} {vname};\n"));
            }
        }
        body.push_str("  } payload;\n} ");
        body.push_str(&name);
        body.push_str(";\n");
        for (index, variant) in variants.iter().enumerate() {
            let vname = self.text(variant.name).to_string();
            body.push_str(&format!("#define {name}_TAG_{vname} {index}\n"));
        }
        self.types.push_str(&body);
    }

    // -- types -------------------------------------------------------------

    fn c_type(&mut self, ty: &Ty) -> String {
        match ty {
            Ty::Int { bits, signed } => {
                format!("r_{}{bits}", if *signed { 'i' } else { 'u' })
            }
            Ty::Float { bits } => format!("r_f{bits}"),
            Ty::Bool => "r_bool".into(),
            Ty::Unit => "r_unit".into(),
            Ty::Text => "r_text".into(),
            Ty::Tuple(elems) => {
                let name = self.mangled_tuple(elems);
                if self.emitted_types.insert(name.clone()) {
                    let mut body = format!("typedef struct {name} {{\n");
                    for (index, elem) in elems.iter().enumerate() {
                        let ty = self.c_type(elem);
                        body.push_str(&format!("  {ty} _{index};\n"));
                    }
                    body.push_str(&format!("}} {name};\n"));
                    self.types.push_str(&body);
                }
                name
            }
            Ty::Array { elem, len } => {
                let elem_ty = self.c_type(elem);
                match len {
                    Some(len) => {
                        let name = format!("{}_n{len}", self.mangle(elem));
                        if self.emitted_types.insert(name.clone()) {
                            self.types.push_str(&format!(
                                "typedef struct {{ {elem_ty} v[{len}]; }} {name};\n"
                            ));
                        }
                        name
                    }
                    None => {
                        let name = format!("sl_{}", self.mangle(elem));
                        if self.emitted_types.insert(name.clone()) {
                            self.types.push_str(&format!(
                                "typedef struct {{ {elem_ty} *ptr; size_t len; }} {name};\n"
                            ));
                        }
                        name
                    }
                }
            }
            Ty::Range(elem) => format!(
                "struct {{ {} start; {} end; }}",
                self.c_type(elem),
                self.c_type(elem)
            ),
            Ty::Record(id) | Ty::Nominal(id) => match &self.checked.nominal(*id).kind {
                NominalKind::Enum(_) => format!("en_{}", self.nominal_name(*id)),
                NominalKind::Alias(inner) => {
                    let inner = inner.clone();
                    self.c_type(&inner)
                }
                _ => {
                    let name = format!("st_{}", self.nominal_name(*id));
                    // Definitions are emitted with the module; recurse lazily if
                    // this type was not pre-registered (anonymous monomorphs).
                    if !self.emitted_types.contains(&name) {
                        if let NominalKind::Record(fields) | NominalKind::View(fields) =
                            self.checked.nominal(*id).kind.clone()
                        {
                            self.emitted_types.insert(name.clone());
                            let mut body = format!("typedef struct {name} {{\n");
                            for field in fields {
                                let ty = self.c_type(&field.ty);
                                let fname = self.text(field.name).to_string();
                                body.push_str(&format!("  {ty} {fname};\n"));
                            }
                            body.push_str(&format!("}} {name};\n"));
                            self.types.push_str(&body);
                        }
                    }
                    name
                }
            },
            Ty::Fn { params, ret, .. } => {
                let params: Vec<String> = params.iter().map(|p| self.c_type(p)).collect();
                let ret = self.c_type(ret);
                if params.is_empty() {
                    format!("{ret} (*)(void)")
                } else {
                    format!("{ret} (*)({})", params.join(", "))
                }
            }
            Ty::Choice(inner) => {
                let inner = inner.clone();
                self.c_type(&inner)
            }
            Ty::Var(_) | Ty::Error => "r_i64".into(),
        }
    }

    /// A declaration of `name` with type `ty`. Function pointers need the
    /// name inside the declarator (`r_i32 (*f)(r_i32)`), not before it.
    fn c_decl(&mut self, ty: &Ty, name: &str) -> String {
        match ty {
            Ty::Fn { params, ret, .. } => {
                let ret = self.c_type(ret);
                let params: Vec<String> = params.iter().map(|p| self.c_type(p)).collect();
                let params = if params.is_empty() {
                    "void".to_string()
                } else {
                    params.join(", ")
                };
                format!("{ret} (*{name})({params})")
            }
            other => {
                let c_ty = self.c_type(other);
                format!("{c_ty} {name}")
            }
        }
    }

    fn mangle(&mut self, ty: &Ty) -> String {
        match ty {
            Ty::Int { bits, signed } => format!("{}{}", if *signed { 'i' } else { 'u' }, bits),
            Ty::Float { bits } => format!("f{bits}"),
            Ty::Bool => "b".into(),
            Ty::Unit => "u".into(),
            Ty::Text => "s".into(),
            Ty::Tuple(elems) => {
                let parts: Vec<String> = elems.iter().map(|e| self.mangle(e)).collect();
                format!("t{}", parts.join("_"))
            }
            Ty::Array { elem, len } => match len {
                Some(len) => format!("a{}{len}", self.mangle(elem)),
                None => format!("sl{}", self.mangle(elem)),
            },
            Ty::Range(elem) => format!("r{}", self.mangle(elem)),
            Ty::Record(id) | Ty::Nominal(id) => self.nominal_name(*id),
            Ty::Fn { .. } => "fn".into(),
            Ty::Choice(inner) => self.mangle(inner),
            Ty::Var(_) | Ty::Error => "e".into(),
        }
    }

    fn mangled_tuple(&mut self, elems: &[Ty]) -> String {
        let parts: Vec<String> = elems.iter().map(|e| self.mangle(e)).collect();
        format!("tup_{}", parts.join("_"))
    }

    // -- functions ---------------------------------------------------------

    fn emit_fn(&mut self, id: ItemId, def: &rill_hir::FnDef) {
        let name = self.item_c_name(id);
        let ret_ty = match self.checked.item_ty(id) {
            Some(ItemTy::Fn { ret, .. }) => ret.clone(),
            _ => Ty::Unit,
        };
        let ret = self.c_type(&ret_ty);
        let params = self.param_list(id, def);
        let is_main = self
            .hir
            .item(id)
            .name
            .map(|n| self.text(n) == "main")
            .unwrap_or(false);
        let (ret_decl, final_name) = if is_main {
            // The program's `main` becomes the C entry point; its value is the
            // process exit code.
            ("int".to_string(), "main".to_string())
        } else {
            (ret.clone(), name.clone())
        };
        // The program's `main` must be visible to the C runtime's entry point.
        let linkage = if is_main { "" } else { "static " };
        self.protos
            .push_str(&format!("{linkage}{ret_decl} {final_name}({params});\n"));

        let previous = std::mem::take(&mut self.body);
        // A body sees module-level statics but no enclosing function's locals.
        let previous_scopes = std::mem::take(&mut self.scopes);
        self.temp = 0;
        self.scopes
            .push(previous_scopes.first().cloned().unwrap_or_default());
        self.push_scope();
        for param in &def.params {
            if let (Some(local), Some(name)) = (param.local, param.name) {
                let c_name = format!("p_{}", self.text(name));
                if param.convention == Some(rill_ast::Convention::Inout) {
                    // A mutable view: the C parameter is a pointer and the local
                    // denotes the pointee, so `t.field` and `set t.field = v`
                    // need no special cases.
                    self.inout_locals.insert(local, c_name.clone());
                    self.bind(local, format!("(*{c_name})"));
                } else {
                    self.bind(local, c_name);
                }
            }
        }
        let tail_value = def.body.as_ref().map(|body| self.expr(body));
        self.body.push('\n');
        let mut body = std::mem::take(&mut self.body);
        if is_main {
            let value = tail_value.unwrap_or_else(|| "0".to_string());
            body.push_str(&format!("  return (int)({value});\n"));
        } else if matches!(ret_ty, Ty::Unit) {
            // A unit-returning tail call still has side effects.
            if let Some(value) = tail_value.as_deref() {
                if value != "rill_unit()" {
                    body.push_str(&format!("  (void)({value});\n"));
                }
            }
            body.push_str("  return rill_unit();\n");
        } else if let Some(value) = tail_value {
            body.push_str(&format!("  return {value};\n"));
        }
        self.pop_scope();
        self.body = previous;
        self.scopes = previous_scopes;
        self.defs.push_str(&format!(
            "{linkage}{ret_decl} {final_name}({params}) {{\n{body}}}\n"
        ));
    }

    /// Parameter list from the *checked* signature: nominal parameters must use
    /// their checked C type (`en_Shape`), never a syntax guess.
    /// A lambda becomes a C function. A lambda whose body reads locals declared
    /// outside itself needs a closure environment, which is reported until the
    /// closure pass lands.
    fn lambda(&mut self, whole: &Expr, params: &[Pattern], body: &Block) -> String {
        let mut referenced = Vec::new();
        collect_local_refs_block(body, &mut referenced);
        let captured: Vec<LocalId> = referenced
            .into_iter()
            .filter(|id| {
                let info = self.hir.local(*id);
                // A local whose declaration lies outside the lambda's own span
                // is a capture.
                let inside = info.span.start >= whole.span.start
                    && info.span.end <= whole.span.end
                    && !whole.span.is_dummy();
                !inside
            })
            .collect();
        if !captured.is_empty() {
            self.unsupported(
                whole.span,
                "a closure that captures locals",
                "M3 (closure environments)",
            );
            return "0".into();
        }

        let name = self.fresh("lambda");
        let param_tys: Vec<Ty> = params
            .iter()
            .map(|pattern| self.pattern_ty(pattern))
            .collect();
        let ret_ty = match &body.tail {
            Some(tail) => self.expr_ty(tail),
            None => Ty::Unit,
        };
        let ret = self.c_type(&ret_ty);
        let params_c: Vec<String> = param_tys
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                let ty = ty.clone();
                self.c_decl(&ty, &format!("a{index}"))
            })
            .collect();

        let previous_body = std::mem::take(&mut self.body);
        let previous_scopes = std::mem::take(&mut self.scopes);
        self.temp = 0;
        self.scopes
            .push(previous_scopes.first().cloned().unwrap_or_default());
        self.push_scope();
        for (index, pattern) in params.iter().enumerate() {
            self.bind_pattern(pattern, &format!("a{index}"));
        }
        let value = self.block(body);
        let mut lambda_body = std::mem::take(&mut self.body);
        if matches!(ret_ty, Ty::Unit) {
            if value != "rill_unit()" {
                lambda_body.push_str(&format!("  (void)({value});\n"));
            }
            lambda_body.push_str("  return rill_unit();\n");
        } else {
            lambda_body.push_str(&format!("  return {value};\n"));
        }
        self.pop_scope();
        self.body = previous_body;
        self.scopes = previous_scopes;

        self.defs.push_str(&format!(
            "static {ret} {name}({}) {{\n{lambda_body}}}\n",
            if params_c.is_empty() {
                "void".to_string()
            } else {
                params_c.join(", ")
            }
        ));
        name
    }

    fn param_list(&mut self, id: ItemId, def: &rill_hir::FnDef) -> String {
        let checked_params: Vec<Ty> = match self.checked.item_ty(id).cloned() {
            Some(ItemTy::Fn { params, .. }) => params,
            _ => Vec::new(),
        };
        let mut parts = Vec::new();
        for (index, param) in def.params.iter().enumerate() {
            if param.convention == Some(rill_ast::Convention::Inout) {
                let element = checked_params
                    .get(index)
                    .cloned()
                    .filter(|ty| !matches!(ty, Ty::Error))
                    .map(|ty| self.c_type(&ty))
                    .unwrap_or_else(|| "r_i64".to_string());
                let name = param
                    .name
                    .map(|n| self.text(n).to_string())
                    .unwrap_or_else(|| "_".to_string());
                parts.push(format!("{element} *p_{name}"));
                continue;
            }
            let ty = match checked_params.get(index).cloned() {
                Some(ty) if !matches!(ty, Ty::Error) => self.c_type(&ty),
                _ => param
                    .ty
                    .clone()
                    .map(|ast| self.syntax_to_c(&ast))
                    .unwrap_or_else(|| "r_i64".to_string()),
            };
            let name = param
                .name
                .map(|n| self.text(n).to_string())
                .unwrap_or_else(|| "_".to_string());
            let ty_struct = match checked_params.get(index).cloned() {
                Some(ty) if !matches!(ty, Ty::Error) => ty,
                _ => Ty::Error,
            };
            if let Ty::Fn { .. } = ty_struct {
                parts.push(self.c_decl(&ty_struct, &format!("p_{name}")));
            } else {
                parts.push(format!("{ty} p_{name}"));
            }
        }
        if parts.is_empty() {
            "void".to_string()
        } else {
            parts.join(", ")
        }
    }
    fn syntax_to_c(&mut self, ast: &rill_ast::Type) -> String {
        match ast {
            rill_ast::Type::Path { segments, .. } => {
                let name = self.text(segments[0].sym).to_string();
                match name.as_str() {
                    "i8" => "r_i8".into(),
                    "i16" => "r_i16".into(),
                    "i32" => "r_i32".into(),
                    "i64" | "isize" => "r_i64".into(),
                    "u8" => "r_u8".into(),
                    "u16" => "r_u16".into(),
                    "u32" => "r_u32".into(),
                    "u64" | "usize" => "r_u64".into(),
                    "f32" => "r_f32".into(),
                    "f64" => "r_f64".into(),
                    "Bool" => "r_bool".into(),
                    "String" | "Text" => "r_text".into(),
                    _ => format!("st_{name}"),
                }
            }
            rill_ast::Type::App { ctor, args, .. } => {
                let name = match &**ctor {
                    rill_ast::Type::Path { segments, .. } => self.text(segments[0].sym).to_string(),
                    _ => String::new(),
                };
                match name.as_str() {
                    "Array" | "Slice" | "Box" => args
                        .first()
                        .map(|a| self.syntax_to_c(a))
                        .unwrap_or_else(|| "void *".into()),
                    _ => format!("st_{name}"),
                }
            }
            rill_ast::Type::Tuple { elems, .. } if elems.is_empty() => "r_unit".into(),
            rill_ast::Type::Array { elem, len, .. } => {
                let elem_ty = self.syntax_to_c(elem);
                match len.as_ref().and_then(|l| match &**l {
                    rill_ast::Expr::Literal { text, .. } => text.parse::<u64>().ok(),
                    _ => None,
                }) {
                    Some(n) => format!("struct {{ {elem_ty} v[{n}]; }}"),
                    None => format!("struct {{ {elem_ty} *ptr; size_t len; }}"),
                }
            }
            _ => "r_i64".into(),
        }
    }

    fn item_c_name(&self, id: ItemId) -> String {
        match self.hir.item(id).name {
            Some(name) => format!("rill_{}", self.text(name)),
            None => format!("rill_item{}", id.0),
        }
    }

    // -- scopes ------------------------------------------------------------

    fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn bind(&mut self, local: LocalId, c_name: String) {
        let scope = self.scopes.last_mut().expect("a scope is open");
        scope.retain(|(id, _)| *id != local);
        scope.push((local, c_name));
    }

    fn c_name(&self, local: LocalId) -> Option<&str> {
        for scope in self.scopes.iter().rev() {
            for (id, name) in scope.iter().rev() {
                if *id == local {
                    return Some(name);
                }
            }
        }
        None
    }

    fn fresh(&mut self, prefix: &str) -> String {
        let name = format!("{prefix}{}", self.temp);
        self.temp += 1;
        name
    }

    /// Emits an expression whose value is discarded. The emitted text carries
    /// the expression's side effects, so dropping it silently would lose calls.
    fn emit_discarded(&mut self, value: &str) {
        self.body.push_str(&format!("  (void)({value});\n"));
    }

    fn typed_temp(&mut self, ty: &Ty) -> String {
        let name = self.fresh("t_");
        let c_ty = self.c_type(ty);
        self.body.push_str(&format!("  {c_ty} {name};\n"));
        name
    }

    /// True for `comptime { zeroed () }`: emitted as a C zero initialiser, so
    /// pools and scratch buffers need no element list.
    fn is_zeroed(&self, block: &Block) -> bool {
        if !block.stmts.is_empty() {
            return false;
        }
        let Some(tail) = block.tail.as_deref() else {
            return false;
        };
        match &tail.kind {
            ExprKind::Call { callee, args } if args.is_empty() => match callee.path_res() {
                Some((segments, _)) => segments
                    .last()
                    .map(|s| self.text(*s) == "zeroed")
                    .unwrap_or(false),
                None => false,
            },
            _ => false,
        }
    }

    fn expr_ty(&self, expr: &Expr) -> Ty {
        self.checked.expr_ty(expr.id).cloned().unwrap_or(Ty::Error)
    }

    fn local_ty(&self, local: LocalId) -> Ty {
        self.checked.local_ty(local).cloned().unwrap_or(Ty::Error)
    }

    // -- expressions -------------------------------------------------------

    fn expr(&mut self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Literal { kind, text } => self.literal(*kind, text),
            ExprKind::Path { segments, res } => match res {
                Res::Local(id) => match self.c_name(*id) {
                    Some(name) => name.to_string(),
                    None => {
                        self.err(expr.span, "internal error: local without C storage");
                        "0".into()
                    }
                },
                Res::Item(id) => {
                    // A nominal in value position: a payload-less variant is a
                    // value (`Empty`), anything else needs arguments.
                    if let Some(ItemTy::Nominal(nominal)) = self.checked.item_ty(*id) {
                        let nominal = *nominal;
                        return self.construct(expr, nominal, segments, &[]);
                    }
                    self.item_c_name(*id)
                }
                Res::Import(_) => {
                    let text: Vec<&str> = segments.iter().map(|s| self.text(*s)).collect();
                    if text.len() == 2 && text[0] == "io" && text[1] == "println" {
                        "rill_println".into()
                    } else if text.len() == 2 && text[0] == "io" && text[1] == "read_file" {
                        "rill_read_file".into()
                    } else if text.len() == 2 && text[0] == "io" && text[1] == "map_file" {
                        "rill_map_file".into()
                    } else if text.len() == 2 && text[0] == "io" && text[1] == "file_size" {
                        "rill_file_size".into()
                    } else {
                        self.unsupported(
                            expr.span,
                            &format!("the import `{}`", text.join("::")),
                            "M3 (stdlib)",
                        );
                        "0".into()
                    }
                }
                Res::Builtin => {
                    self.unsupported(
                        expr.span,
                        "this builtin outside `comptime { zeroed () }`",
                        "M3 (intrinsics)",
                    );
                    "0".into()
                }
                Res::Field => {
                    self.unsupported(expr.span, "an unresolved field reference", "M2");
                    "0".into()
                }
                Res::Error => "0".into(),
            },
            ExprKind::Tuple(elems) => {
                let ty = self.expr_ty(expr);
                let name = match &ty {
                    Ty::Tuple(elems) => self.mangled_tuple(elems),
                    _ => "r_i64".into(),
                };
                let parts: Vec<String> = elems.iter().map(|e| self.expr(e)).collect();
                let fields: Vec<String> = parts
                    .iter()
                    .enumerate()
                    .map(|(index, value)| format!("._{index} = {value}"))
                    .collect();
                format!("({name}){{ {} }}", fields.join(", "))
            }
            ExprKind::Array(elems) => {
                let ty = self.expr_ty(expr);
                let values: Vec<String> = elems.iter().map(|e| self.expr(e)).collect();
                match &ty {
                    Ty::Array { elem, len: Some(_) } => {
                        let name = self.mangle(elem);
                        let temp = self.fresh("arr_");
                        self.body.push_str(&format!(
                            "  {name} {temp} = {{ .v = {{ {} }} }};\n",
                            values.join(", ")
                        ));
                        temp
                    }
                    _ => {
                        self.unsupported(expr.span, "this array literal", "M3");
                        "0".into()
                    }
                }
            }
            ExprKind::Block(block) => self.block(block),
            ExprKind::Lambda { params, body } => self.lambda(expr, params, body),
            ExprKind::Call { callee, args } => self.call(expr, callee, args),
            ExprKind::Index { base, args } => {
                let base = self.expr(base);
                let index = args
                    .first()
                    .map(|arg| self.expr(&arg.value))
                    .unwrap_or_else(|| "0".into());
                let base_ty = self.expr_ty(base_expr(expr));
                match base_ty {
                    Ty::Array { len: Some(_), .. } => format!("({base}).v[{index}]"),
                    Ty::Array { len: None, .. } => format!("({base}).ptr[{index}]"),
                    Ty::Text => {
                        format!("((r_u8)((const unsigned char *)({base}))[{index}])")
                    }
                    _ => {
                        self.unsupported(expr.span, "this index expression", "M3");
                        "0".into()
                    }
                }
            }
            ExprKind::Try { .. } => {
                self.unsupported(expr.span, "`?` on a failable value", "M3 (`<decides>`)");
                "0".into()
            }
            ExprKind::Unary { op, operand } => {
                let value = self.expr(operand);
                match op {
                    UnaryOp::Neg => format!("(-({value}))"),
                    UnaryOp::Not => format!("(!({value}))"),
                    UnaryOp::Deref => format!("(*({value}))"),
                    UnaryOp::Size => {
                        let operand_ty = self.expr_ty(operand);
                        match operand_ty {
                            Ty::Array { len: Some(n), .. } => n.to_string(),
                            Ty::Array { len: None, .. } => format!("((r_i64)({value}).len)"),
                            Ty::Text => format!("((r_i64)strlen({value}))"),
                            _ => "0".into(),
                        }
                    }
                    UnaryOp::Shapeof | UnaryOp::Reshape => {
                        self.unsupported(expr.span, "shaped-tensor operations", "M5");
                        "0".into()
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(expr, *op, lhs, rhs),
            ExprKind::Cast { expr: inner, ty } => {
                let value = self.expr(inner);
                let from = self.expr_ty(inner);
                let to_text = matches!(
                    ty,
                    rill_ast::Type::Path { segments, .. }
                        if matches!(self.text(segments[0].sym), "String" | "Text")
                );
                if to_text {
                    return match from {
                        Ty::Int { .. } => format!("rill_i64_to_text((r_i64)({value}))"),
                        Ty::Float { .. } => format!("rill_f64_to_text((r_f64)({value}))"),
                        Ty::Text => value,
                        other => {
                            let display = format!("{other:?}");
                            self.unsupported(
                                expr.span,
                                &format!("casting `{display}` to text"),
                                "M3 (stdlib)",
                            );
                            "0".into()
                        }
                    };
                }
                if matches!(from, Ty::Text) {
                    self.unsupported(expr.span, "casting text to a number", "M3 (stdlib)");
                    return "0".into();
                }
                let target = self.syntax_to_c(ty);
                format!("(({target})({value}))")
            }
            ExprKind::Is { expr: inner, .. } => {
                self.expr(inner);
                "0".into()
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let ty = self.expr_ty(expr);
                let result = self.typed_temp(&ty);
                let condition = self.condition(cond);
                self.body.push_str(&format!("  if ({condition}) {{\n"));
                let then_value = self.block(then_branch);
                if matches!(ty, Ty::Unit) {
                    // A unit branch still has to run its tail expression.
                    self.emit_discarded(&then_value);
                } else {
                    self.body
                        .push_str(&format!("    {result} = {then_value};\n"));
                }
                self.body.push_str("  }");
                match else_branch {
                    Some(ElseBranch::Block(block)) => {
                        self.body.push_str(" else {\n");
                        let value = self.block(block);
                        if matches!(ty, Ty::Unit) {
                            self.emit_discarded(&value);
                        } else {
                            self.body.push_str(&format!("    {result} = {value};\n"));
                        }
                        self.body.push_str("  }\n");
                    }
                    Some(ElseBranch::If(other)) => {
                        self.body.push_str(" else {\n");
                        let value = self.expr(other);
                        if matches!(ty, Ty::Unit) {
                            self.emit_discarded(&value);
                        } else {
                            self.body.push_str(&format!("    {result} = {value};\n"));
                        }
                        self.body.push_str("  }\n");
                    }
                    None => self.body.push('\n'),
                }
                if matches!(ty, Ty::Unit) {
                    "rill_unit()".into()
                } else {
                    result
                }
            }
            ExprKind::Match { scrutinee, arms } => self.match_expr(expr, scrutinee, arms),
            ExprKind::Comptime { body } => {
                if self.is_zeroed(body) {
                    // C zero-fills the rest of an aggregate initialiser, which is
                    // exactly what `zeroed ()` promises.
                    let ty = self.expr_ty(expr);
                    let c_ty = self.c_type(&ty);
                    format!("({c_ty}){{0}}")
                } else {
                    self.unsupported(expr.span, "`comptime` evaluation", "M3 (rill-comptime)");
                    "0".into()
                }
            }
            ExprKind::Unsafe { body } => self.block(body),
            ExprKind::Handler { kind, .. } => {
                let name = match kind {
                    rill_ast::HandlerKind::Option => "option",
                    rill_ast::HandlerKind::One => "one",
                    rill_ast::HandlerKind::All => "all",
                };
                self.unsupported(
                    expr.span,
                    &format!("the `{name} {{ … }}` handler"),
                    "M3/M4 (rooted in `<decides>` lowering)",
                );
                "0".into()
            }
            ExprKind::Concurrent { kind, .. } => {
                let name = match kind {
                    rill_ast::ConcurrentKind::Sync => "sync",
                    rill_ast::ConcurrentKind::Race => "race",
                    rill_ast::ConcurrentKind::Rush => "rush",
                    rill_ast::ConcurrentKind::Branch => "branch",
                };
                self.unsupported(
                    expr.span,
                    &format!("`{name} {{ … }}` structured concurrency"),
                    "M5 (`<suspends>`)",
                );
                "0".into()
            }
            ExprKind::Error => "0".into(),
        }
    }

    fn literal(&mut self, kind: LiteralKind, text: &str) -> String {
        match kind {
            LiteralKind::Int => {
                let cleaned = text.replace('_', "");
                let (digits, suffix) = rill_ty::split_numeric_suffix(&cleaned);
                match suffix {
                    Some("i8") => format!("((r_i8){digits})"),
                    Some("i16") => format!("((r_i16){digits})"),
                    Some("i32") => format!("((r_i32){digits})"),
                    Some("i64") | Some("isize") => format!("((r_i64){digits})"),
                    Some("u8") => format!("((r_u8){digits})"),
                    Some("u16") => format!("((r_u16){digits})"),
                    Some("u32") => format!("((r_u32){digits})"),
                    Some("u64") | Some("usize") => format!("((r_u64){digits})"),
                    _ => digits.to_string(),
                }
            }
            LiteralKind::Float => {
                let cleaned = text.replace('_', "");
                let (digits, suffix) = rill_ty::split_numeric_suffix(&cleaned);
                match suffix {
                    Some("f32") => format!("((r_f32){digits})"),
                    Some("f64") => format!("((r_f64){digits})"),
                    _ => format!("((r_f64){digits})"),
                }
            }
            LiteralKind::Bool => {
                if text == "true" {
                    "1".into()
                } else {
                    "0".into()
                }
            }
            LiteralKind::Text | LiteralKind::RawText => {
                let inner = match kind {
                    LiteralKind::RawText => text
                        .trim_start_matches("r#\"")
                        .trim_end_matches("\"#")
                        .to_string(),
                    _ => text.trim_matches('"').to_string(),
                };
                format!("\"{}\"", escape_c(&inner))
            }
            LiteralKind::Char => format!("'{}'", escape_c(text.trim_matches('\''))),
        }
    }

    fn binary(&mut self, whole: &Expr, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> String {
        match op {
            BinaryOp::Flow => match &rhs.kind {
                ExprKind::Index { base, args } => self.indexed_member(whole, lhs, base, args),
                _ => self.field_access(whole, lhs, rhs),
            },
            BinaryOp::Concat => {
                let left = self.expr(lhs);
                let right = self.expr(rhs);
                let ty = self.expr_ty(lhs);
                match ty {
                    Ty::Text => format!("rill_text_concat({left}, {right})"),
                    other => {
                        let display = format!("{other:?}");
                        self.unsupported(
                            whole.span,
                            &format!("`++` on `{display}`"),
                            "M3 (stdlib)",
                        );
                        "0".into()
                    }
                }
            }
            _ => {
                let left = self.expr(lhs);
                let right = self.expr(rhs);
                let operator = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Rem => "%",
                    BinaryOp::BitAnd => "&",
                    BinaryOp::BitXor => "^",
                    BinaryOp::BitOr => "|",
                    BinaryOp::Shl => "<<",
                    BinaryOp::Shr => ">>",
                    BinaryOp::And => "&&",
                    BinaryOp::Or => "||",
                    BinaryOp::Eq | BinaryOp::EqEq => "==",
                    BinaryOp::NotEq => "!=",
                    BinaryOp::Lt => "<",
                    BinaryOp::Gt => ">",
                    BinaryOp::LtEq => "<=",
                    BinaryOp::GtEq => ">=",
                    BinaryOp::Concat
                    | BinaryOp::RangeInclusive
                    | BinaryOp::RangeHalfOpen
                    | BinaryOp::Choice
                    | BinaryOp::Flow => {
                        self.unsupported(whole.span, "this operator", "M3");
                        return "0".into();
                    }
                };
                format!("(({left}) {operator} ({right}))")
            }
        }
    }

    /// `record.field` / `tuple.0` / `slice.field`.
    fn field_access(&mut self, whole: &Expr, lhs: &Expr, rhs: &Expr) -> String {
        let base = self.expr(lhs);
        let field = match rhs.path_res() {
            Some((segments, _)) => self.text(segments[0]).to_string(),
            None => match &rhs.kind {
                ExprKind::Literal { text, .. } => text.clone(),
                _ => {
                    self.unsupported(whole.span, "this `.` selection", "M3");
                    return "0".into();
                }
            },
        };
        let base_ty = self.expr_ty(lhs);
        match base_ty {
            Ty::Tuple(_) => format!("({base})._{field}"),
            Ty::Record(_) | Ty::Nominal(_) => format!("({base}).{field}"),
            _ => {
                self.unsupported(whole.span, "this field projection", "M2 (projections)");
                "0".into()
            }
        }
    }

    /// `x.field[i]` — select the field, then index into it.
    fn indexed_member(
        &mut self,
        whole: &Expr,
        lhs: &Expr,
        member: &Expr,
        args: &[rill_hir::Arg],
    ) -> String {
        let base = self.expr(lhs);
        let field = match member.path_res() {
            Some((segments, _)) => self.text(segments[0]).to_string(),
            None => {
                self.unsupported(whole.span, "this indexed `.` selection", "M3");
                return "0".into();
            }
        };
        let index = args
            .first()
            .map(|arg| self.expr(&arg.value))
            .unwrap_or_else(|| "0".into());
        // The member's own type was recorded while checking.
        match self.expr_ty(member) {
            Ty::Array { len: Some(_), .. } => format!("({base}).{field}.v[{index}]"),
            Ty::Array { len: None, .. } => format!("({base}).{field}.ptr[{index}]"),
            Ty::Text => format!("((r_u8)({base}).{field}[{index}])"),
            _ => {
                self.unsupported(whole.span, "this indexed member", "M3");
                "0".into()
            }
        }
    }

    fn condition(&mut self, cond: &Cond) -> String {
        match cond {
            Cond::Expr(expr) => self.expr(expr),
            Cond::Bind { value, .. } => {
                // `if (x := e)` — the binding is checked; the C backend only
                // lowers it when `e` is a plain value.
                self.unsupported(
                    cond.span(),
                    "an `if (x := …)` failable binding",
                    "M3 (`<decides>`)",
                );
                let _ = self.expr(value);
                "1".into()
            }
        }
    }

    fn block(&mut self, block: &Block) -> String {
        self.push_scope();
        for stmt in &block.stmts {
            self.stmt(stmt);
        }
        let value = match &block.tail {
            Some(tail) => self.expr(tail),
            None => "rill_unit()".to_string(),
        };
        self.pop_scope();
        value
    }

    // -- statements --------------------------------------------------------

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Error(_) => {}
            Stmt::Var {
                local, ty, init, ..
            } => {
                let init_code = self.expr(init);
                let checked_ty = self.local_ty(*local);
                let declared = if matches!(checked_ty, Ty::Error) {
                    ty.as_ref()
                        .map(|ast| self.syntax_to_c(&ast.clone()))
                        .unwrap_or_else(|| "r_i64".to_string())
                } else {
                    self.c_type(&checked_ty)
                };
                let name = format!("v_{}", self.text(self.hir.local(*local).name));
                self.body
                    .push_str(&format!("  {declared} {name} = {init_code};\n"));
                self.bind(*local, name);
            }
            Stmt::Bind { pattern, value, .. } => {
                let value_code = self.expr(value);
                self.bind_pattern(pattern, &value_code);
            }
            Stmt::Set { place, value, .. } => {
                let target = self.place(place);
                let value_code = self.expr(value);
                self.body.push_str(&format!("  {target} = {value_code};\n"));
            }
            Stmt::Yield { span, .. } => {
                self.unsupported(*span, "`yield`", "M2 (projections)");
            }
            Stmt::TryBind { span, .. } => {
                self.unsupported(*span, "`let?`", "M3 (`<decides>`)");
            }
            Stmt::When { span, .. } => {
                self.unsupported(*span, "`when` guards", "M3 (`<decides>`)");
            }
            Stmt::While { cond, body, .. } => {
                // `while` is emitted as a counted-free loop with an explicit
                // break: a condition that desugars to an `if` (`a and b`, `a or
                // b`) hoists statements, and those must run *per iteration*, not
                // once before the loop.
                self.body.push_str("  for (;;) {\n");
                self.push_scope();
                let condition = self.expr(cond);
                self.body
                    .push_str(&format!("    if (!({condition})) {{ break; }}\n"));
                self.block_inner(body);
                self.pop_scope();
                self.body.push_str("  }\n");
            }
            Stmt::Break { .. } => self.body.push_str("  break;\n"),
            Stmt::For {
                pattern,
                iter,
                body,
                span,
            } => self.for_loop(*span, pattern, iter, body),
            Stmt::Let { item, .. } => {
                // Local `let`s are emitted as file-scope static functions.
                if let Some(inner) = self.hir.try_item(*item) {
                    if let ItemKind::Fn(def) = &inner.kind {
                        self.emit_fn(*item, def);
                    }
                }
            }
            Stmt::Expr(expr) => {
                // The value is discarded, but the expression must still *run*:
                // calls carry their side effects in the emitted text.
                let value = self.expr(expr);
                if !matches!(self.expr_ty(expr), Ty::Unit) {
                    self.body.push_str(&format!("  (void)({value});\n"));
                } else {
                    self.body.push_str(&format!("  {value};\n"));
                }
            }
        }
    }

    fn for_loop(&mut self, span: Span, pattern: &Pattern, iter: &Expr, body: &Block) {
        let Pattern::Binding { local, .. } = pattern else {
            self.unsupported(span, "this loop pattern", "M3");
            return;
        };
        let index = format!("i_{}", self.text(self.hir.local(*local).name));
        let element = self.c_type(&self.local_ty(*local));
        let iter_ty = self.expr_ty(iter);
        match (&iter_ty, &iter.kind) {
            (Ty::Range(_), ExprKind::Binary { op, lhs, rhs }) => {
                let start_code = self.expr(lhs);
                let end_code = self.expr(rhs);
                let comparison = if *op == BinaryOp::RangeInclusive {
                    "<="
                } else {
                    "<"
                };
                self.body.push_str(&format!(
                    "  for ({element} {index} = {start_code}; {index} {comparison} {end_code}; {index} += 1) {{\n"
                ));
                self.push_scope();
                self.bind(*local, index.clone());
                self.block_inner(body);
                self.pop_scope();
                self.body.push_str("  }\n");
            }
            (Ty::Array { len, .. }, _) => {
                let iter_code = self.expr(iter);
                let length = match len {
                    Some(n) => n.to_string(),
                    None => format!("((r_i64)({iter_code}).len)"),
                };
                let access = match len {
                    Some(_) => format!("({iter_code}).v[{index}]"),
                    None => format!("({iter_code}).ptr[{index}]"),
                };
                let item = format!("{index}_item");
                self.body.push_str(&format!(
                    "  for (r_i64 {index} = 0; {index} < {length}; {index} += 1) {{\n    {element} {item} = {access};\n"
                ));
                self.push_scope();
                self.bind(*local, item);
                self.block_inner(body);
                self.pop_scope();
                self.body.push_str("  }\n");
            }
            _ => {
                self.unsupported(span, "this loop iterator", "M3");
            }
        }
    }

    /// The statements and tail of a block, without opening a scope.
    fn block_inner(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.stmt(stmt);
        }
        if let Some(tail) = &block.tail {
            let value = self.expr(tail);
            if !matches!(self.expr_ty(tail), Ty::Unit) {
                self.emit_discarded(&value);
            } else {
                self.body.push_str(&format!("  {value};\n"));
            }
        }
    }

    /// An assignable C lvalue.
    fn place(&mut self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Path {
                res: Res::Local(id),
                ..
            } => self
                .c_name(*id)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "0".into()),
            ExprKind::Index { base, args } => {
                let base = self.expr(base);
                let index = args
                    .first()
                    .map(|a| self.expr(&a.value))
                    .unwrap_or_else(|| "0".into());
                let base_ty = self.expr_ty(base_expr(expr));
                match base_ty {
                    Ty::Array { len: Some(_), .. } => format!("({base}).v[{index}]"),
                    _ => format!("({base}).ptr[{index}]"),
                }
            }
            ExprKind::Binary {
                op: BinaryOp::Flow,
                lhs,
                rhs,
            } => {
                let base = self.expr(lhs);
                let field = match rhs.path_res() {
                    Some((segments, _)) => self.text(segments[0]).to_string(),
                    None => "0".to_string(),
                };
                let base_ty = self.expr_ty(lhs);
                match base_ty {
                    Ty::Tuple(_) => format!("({base})._{field}"),
                    _ => format!("({base}).{field}"),
                }
            }
            _ => {
                self.unsupported(expr.span, "this assignment target", "M2 (places)");
                "0".into()
            }
        }
    }

    fn bind_pattern(&mut self, pattern: &Pattern, value: &str) {
        for line in self.pattern_bindings(pattern, value) {
            self.body.push_str(&line);
            self.body.push('\n');
        }
    }

    /// C declarations binding a pattern's variables to parts of `value`.
    fn pattern_bindings(&mut self, pattern: &Pattern, value: &str) -> Vec<String> {
        match pattern {
            Pattern::Wildcard(_) | Pattern::Error(_) | Pattern::Literal { .. } => Vec::new(),
            Pattern::Binding { local, .. } => {
                let name = format!("v_{}", self.text(self.hir.local(*local).name));
                let ty = self.local_ty(*local);
                let decl = self.c_decl(&ty, &name);
                self.bind(*local, name.clone());
                vec![format!("  {decl} = {value};")]
            }
            Pattern::Tuple { elems, .. } => {
                let temp = self.fresh("tup_");
                let ty = self.pattern_ty(pattern);
                let c_ty = self.c_type(&ty);
                let mut lines = vec![format!("  {c_ty} {temp} = {value};")];
                for (index, elem) in elems.iter().enumerate() {
                    let field = format!("{temp}._{index}");
                    lines.extend(self.pattern_bindings(elem, &field));
                }
                lines
            }
            Pattern::Constructor { path, args, .. } => {
                let variant = self.text(path[0]).to_string();
                let base = format!("({value}).payload.{variant}");
                let mut lines = Vec::new();
                for (index, arg) in args.iter().enumerate() {
                    let access = format!("{base}._{index}");
                    lines.extend(self.pattern_bindings(arg, &access));
                }
                lines
            }
        }
    }

    /// The C type of a destructuring pattern: a tuple of its bindings' types.
    fn pattern_ty(&mut self, pattern: &Pattern) -> Ty {
        match pattern {
            Pattern::Binding { local, .. } => self.local_ty(*local),
            Pattern::Wildcard(_) => Ty::i32(),
            Pattern::Tuple { elems, .. } => {
                Ty::Tuple(elems.iter().map(|e| self.pattern_ty(e)).collect())
            }
            _ => Ty::i32(),
        }
    }

    /// `match` — a guarded arm sequence with a `done` flag: guards work without
    /// gotos, and the result is zero-initialised when no arm matches.
    fn match_expr(
        &mut self,
        whole: &Expr,
        scrutinee: &Expr,
        arms: &[rill_hir::MatchArm],
    ) -> String {
        let value = self.expr(scrutinee);
        let scrutinee_ty = self.expr_ty(scrutinee);
        let c_ty = self.c_type(&scrutinee_ty);
        let slot = self.fresh("scrut");
        self.body.push_str(&format!("  {c_ty} {slot} = {value};\n"));
        let result_ty = self.expr_ty(whole);
        let result_ty = if matches!(result_ty, Ty::Unit) {
            Ty::i32()
        } else {
            result_ty
        };
        let result = self.fresh("matched");
        let result_c = self.c_type(&result_ty);
        self.body.push_str(&format!("  {result_c} {result} = 0;\n"));
        let done = self.fresh("done");
        self.body.push_str(&format!("  r_bool {done} = 0;\n"));
        for arm in arms {
            let test = self.arm_test(&slot, &scrutinee_ty, arm);
            self.body
                .push_str(&format!("  if (!{done} && ({test})) {{\n"));
            self.push_scope();
            for line in self.pattern_bindings(&arm.pattern, &slot) {
                self.body.push_str(&format!("  {line}\n"));
            }
            let guarded = match &arm.guard {
                Some(guard) => {
                    let guard_code = self.expr(guard);
                    self.body.push_str(&format!("    if ({guard_code}) {{\n"));
                    true
                }
                None => false,
            };
            let body = self.expr(&arm.body);
            self.body.push_str(&format!("    {result} = {body};\n"));
            self.body.push_str(&format!("    {done} = 1;\n"));
            if guarded {
                self.body.push_str("    }\n");
            }
            self.pop_scope();
            self.body.push_str("  }\n");
        }
        result
    }

    /// The C condition an arm tests; payload bindings come from
    /// [`Self::pattern_bindings`].
    fn arm_test(&mut self, slot: &str, scrutinee_ty: &Ty, arm: &rill_hir::MatchArm) -> String {
        match &arm.pattern {
            Pattern::Wildcard(_) | Pattern::Binding { .. } | Pattern::Tuple { .. } => {
                "1".to_string()
            }
            Pattern::Literal { text, .. } => {
                let trimmed = text.trim();
                if trimmed.starts_with('"') {
                    format!(
                        "strcmp({slot}, {}) == 0",
                        self.literal(rill_ast::LiteralKind::Text, trimmed)
                    )
                } else if trimmed == "true" {
                    format!("({slot} != 0)")
                } else if trimmed == "false" {
                    format!("({slot} == 0)")
                } else if trimmed.starts_with('\'') {
                    format!(
                        "{slot} == {}",
                        self.literal(rill_ast::LiteralKind::Char, trimmed)
                    )
                } else {
                    format!("{slot} == ({trimmed})")
                }
            }
            Pattern::Constructor { path, .. } => {
                let variant = self.text(path[0]).to_string();
                match scrutinee_ty {
                    Ty::Nominal(id) | Ty::Record(id) => match &self.checked.nominal(*id).kind {
                        NominalKind::Enum(variants) => {
                            match variants.iter().position(|v| self.text(v.name) == variant) {
                                Some(_) => {
                                    let enum_name = format!("en_{}", self.nominal_name(*id));
                                    format!("{slot}.tag == {enum_name}_TAG_{variant}")
                                }
                                None if self.nominal_name(*id) == variant => "1".to_string(),
                                None => {
                                    let names: Vec<&str> =
                                        variants.iter().map(|v| self.text(v.name)).collect();
                                    self.diagnostics.push(
                                        Diagnostic::error(
                                            arm.span,
                                            format!("`{variant}` is not a variant"),
                                        )
                                        .note(format!("variants: {}", names.join(", "))),
                                    );
                                    "0".to_string()
                                }
                            }
                        }
                        _ => "1".to_string(),
                    },
                    Ty::Error => "0".to_string(),
                    _ => {
                        self.unsupported(arm.span, "this match pattern", "M3");
                        "0".to_string()
                    }
                }
            }
            Pattern::Error(_) => "0".to_string(),
        }
    }

    fn call(&mut self, whole: &Expr, callee: &Expr, args: &[rill_hir::Arg]) -> String {
        // Enum construction: `Type::Variant a b` or a record construction
        // `Name a b`.
        if let ExprKind::Path {
            segments,
            res: Res::Item(id),
        } = &callee.kind
        {
            if let Some(ItemTy::Nominal(nominal)) = self.checked.item_ty(*id) {
                return self.construct(whole, *nominal, segments, args);
            }
        }
        if let ExprKind::Binary {
            op: BinaryOp::Flow, ..
        } = &callee.kind
        {
            self.unsupported(
                whole.span,
                "a `.`-method call",
                "M2/M3 (projections and stdlib)",
            );
            return "0".into();
        }
        let callee_code = self.expr(callee);
        let callee_ty = self.expr_ty(callee);
        let mut arguments: Vec<String> = args
            .iter()
            .map(|arg| {
                let value = self.expr(&arg.value);
                match arg.convention {
                    // An `inout` argument passes the place itself; when the place
                    // is already a pointer local this is `&(*p_x)`, i.e. `p_x`.
                    Some(rill_ast::Convention::Inout) => format!("&({value})"),
                    _ => value,
                }
            })
            .collect();
        if let Ty::Fn { params, .. } = &callee_ty {
            // Pass conventions do not change the C ABI for value types.
            for (param, argument) in params.iter().zip(arguments.iter_mut()) {
                if let Ty::Unit = param {
                    *argument = "rill_unit()".to_string();
                }
            }
        }
        format!("{callee_code}({})", arguments.join(", "))
    }

    fn construct(
        &mut self,
        whole: &Expr,
        nominal: NominalId,
        segments: &[Symbol],
        args: &[rill_hir::Arg],
    ) -> String {
        let kind = self.checked.nominal(nominal).kind.clone();
        let values: Vec<String> = args.iter().map(|arg| self.expr(&arg.value)).collect();
        match kind {
            NominalKind::Record(fields) | NominalKind::View(fields) => {
                let temp = self.fresh("rec_");
                let name = self.c_type(&Ty::Record(nominal));
                let field_inits: Vec<String> = fields
                    .iter()
                    .zip(values.iter())
                    .map(|(field, value)| format!(".{} = {value}", self.text(field.name)))
                    .collect();
                self.body.push_str(&format!(
                    "  {name} {temp} = {{ {} }};\n",
                    field_inits.join(", ")
                ));
                temp
            }
            NominalKind::Enum(variants) => {
                let variant_name = match segments.last() {
                    Some(name) => self.text(*name).to_string(),
                    None => {
                        self.unsupported(whole.span, "this enum constructor", "M3");
                        return "0".into();
                    }
                };
                let Some(index) = variants
                    .iter()
                    .position(|v| self.text(v.name) == variant_name)
                else {
                    let names: Vec<&str> = variants.iter().map(|v| self.text(v.name)).collect();
                    self.diagnostics.push(
                        Diagnostic::error(whole.span, format!("`{variant_name}` is not a variant"))
                            .note(format!("variants: {}", names.join(", "))),
                    );
                    return "0".into();
                };
                let c_name = self.c_type(&Ty::Nominal(nominal));
                let temp = self.fresh("variant_");
                if values.is_empty() {
                    self.body
                        .push_str(&format!("  {c_name} {temp} = {{ .tag = {index} }};\n"));
                } else {
                    let inits: Vec<String> = values
                        .iter()
                        .enumerate()
                        .map(|(field, value)| format!("._{field} = {value}"))
                        .collect();
                    self.body.push_str(&format!(
                        "  {c_name} {temp} = {{ .tag = {index}, .payload.{variant_name} = {{ {} }} }};\n",
                        inits.join(", ")
                    ));
                }
                temp
            }
            NominalKind::Opaque | NominalKind::Alias(_) => {
                self.unsupported(whole.span, "this constructor", "M3");
                "0".into()
            }
        }
    }
}

/// Locals referenced anywhere in `expr` (used for capture analysis).
fn collect_local_refs_expr(expr: &Expr, out: &mut Vec<LocalId>) {
    match &expr.kind {
        ExprKind::Path {
            res: Res::Local(id),
            ..
        } => {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        ExprKind::Tuple(elems) | ExprKind::Array(elems) => {
            for element in elems {
                collect_local_refs_expr(element, out);
            }
        }
        ExprKind::Block(block)
        | ExprKind::Comptime { body: block }
        | ExprKind::Unsafe { body: block } => collect_local_refs_block(block, out),
        ExprKind::Lambda { body, .. } => collect_local_refs_block(body, out),
        ExprKind::Call { callee, args } | ExprKind::Index { base: callee, args } => {
            collect_local_refs_expr(callee, out);
            for arg in args {
                collect_local_refs_expr(&arg.value, out);
            }
        }
        ExprKind::Try { base } => collect_local_refs_expr(base, out),
        ExprKind::Unary { operand, .. } => collect_local_refs_expr(operand, out),
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_local_refs_expr(lhs, out);
            collect_local_refs_expr(rhs, out);
        }
        ExprKind::Cast { expr, .. } | ExprKind::Is { expr, .. } => {
            collect_local_refs_expr(expr, out)
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            match cond {
                Cond::Expr(expr) => collect_local_refs_expr(expr, out),
                Cond::Bind { value, .. } => collect_local_refs_expr(value, out),
            }
            collect_local_refs_block(then_branch, out);
            match else_branch {
                Some(ElseBranch::Block(block)) => collect_local_refs_block(block, out),
                Some(ElseBranch::If(expr)) => collect_local_refs_expr(expr, out),
                None => {}
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_local_refs_expr(scrutinee, out);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_local_refs_expr(guard, out);
                }
                collect_local_refs_expr(&arm.body, out);
            }
        }
        ExprKind::Handler { body, .. } => collect_local_refs_block(body, out),
        ExprKind::Concurrent { arms, .. } => {
            for arm in arms {
                collect_local_refs_expr(arm, out);
            }
        }
        ExprKind::Literal { .. } | ExprKind::Path { .. } | ExprKind::Error => {}
    }
}

fn collect_local_refs_block(block: &Block, out: &mut Vec<LocalId>) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Var { init, .. } => collect_local_refs_expr(init, out),
            Stmt::Set { place, value, .. } => {
                collect_local_refs_expr(place, out);
                collect_local_refs_expr(value, out);
            }
            Stmt::Yield { value, .. } => collect_local_refs_expr(value, out),
            Stmt::Bind { value, .. } => collect_local_refs_expr(value, out),
            Stmt::TryBind { value, .. } => collect_local_refs_expr(value, out),
            Stmt::When { cond, .. } => match cond {
                Cond::Expr(expr) => collect_local_refs_expr(expr, out),
                Cond::Bind { value, .. } => collect_local_refs_expr(value, out),
            },
            Stmt::For { iter, body, .. } => {
                collect_local_refs_expr(iter, out);
                collect_local_refs_block(body, out);
            }
            Stmt::While { cond, body, .. } => {
                collect_local_refs_expr(cond, out);
                collect_local_refs_block(body, out);
            }
            Stmt::Break { .. } => {}
            Stmt::Expr(expr) => collect_local_refs_expr(expr, out),
            Stmt::Let { .. } | Stmt::Error(_) => {}
        }
    }
    if let Some(tail) = &block.tail {
        collect_local_refs_expr(tail, out);
    }
}

/// The base expression of an index, for type queries.
fn base_expr(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Index { base, .. } => base,
        _ => expr,
    }
}

/// Escapes a Rill text literal for C, decoding the escapes Rill accepts.
fn escape_c(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push_str("\\n"),
                Some('t') => out.push_str("\\t"),
                Some('r') => out.push_str("\\r"),
                Some('0') => out.push_str("\\0"),
                Some('\\') => out.push_str("\\\\"),
                Some('"') => out.push_str("\\\""),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out
}
