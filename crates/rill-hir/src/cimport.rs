//! Importing a C header as Rill declarations: `type SDL3 = @include("SDL3/SDL.h");`.
//!
//! The C side is read by `rill-cimport`, which reports prototypes, typedefs
//! and `#define` constants as C-level facts. This module turns those into the
//! item vocabulary the rest of the compiler already understands:
//!
//! * a prototype becomes a bodyless `extern "c"` function whose effects are
//!   `<io>` (a C call can do anything, so the conservative row is the honest
//!   one);
//! * a `struct`/`union` typedef becomes a record whose C name is kept, so code
//!   generation emits `typedef SDL_FRect …` instead of a struct of its own;
//! * an `enum` typedef becomes `i32`, its enumerators become constants;
//! * a `#define NAME <literal>` becomes a constant that emits the literal
//!   where it is used.
//!
//! Declarations that cannot be expressed (variadic functions, function-pointer
//! parameters, array parameters, types the mapper does not know) are left out:
//! a C header is mostly wider than what a first cut can type, and a call to a
//! function that is not imported reports an unknown name at the call site.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rill_ast::{EffectItem, EffectRow, FieldDef, Ident, RecordDef, Type, TypeBody};
use rill_cimport::{CType, CTypeKind};
use rill_span::{Interner, Span, Symbol};

use crate::hir::{ConstantDef, FnDef, ItemKind, Param, ParamSlot, TypeDef};

/// One declaration a header contributed.
pub struct ImportedDecl {
    /// The C name, which is also the item's name.
    pub name: Symbol,
    pub kind: ItemKind,
    /// Documentation carried over from the header, one entry per line.
    pub docs: Vec<String>,
    /// Where the declaration sits in the header, for goto definition.
    pub file: PathBuf,
    pub line: u32,
}

/// Everything one `@include` produced.
pub struct Imported {
    pub decls: Vec<ImportedDecl>,
    /// Resolved header paths, in visit order.
    pub headers: Vec<PathBuf>,
}

/// Where `@include` looks for headers: the caller's directories first (the
/// source file's own directory among them), then `$RILL_INCLUDE_PATH`, `$CPATH`
/// and the usual system prefixes.
pub fn default_include_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for var in ["RILL_INCLUDE_PATH", "CPATH"] {
        if let Some(value) = std::env::var_os(var) {
            for part in std::env::split_paths(&value) {
                if !part.as_os_str().is_empty() {
                    dirs.push(part);
                }
            }
        }
    }
    for prefix in [
        "/opt/homebrew/include",
        "/usr/local/include",
        "/usr/include",
    ] {
        dirs.push(PathBuf::from(prefix));
    }
    dirs
}

/// The header `path` names, resolved against `dirs`.
pub fn find_header(path: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let candidate = Path::new(path);
    if candidate.is_absolute() && candidate.is_file() {
        return Some(candidate.to_path_buf());
    }
    for dir in dirs {
        let full = dir.join(path);
        if full.is_file() {
            return Some(full);
        }
    }
    None
}

/// Reads `path` (as written in the source) and maps every declaration the
/// scanner understood. `module` is the name the declarations live under, so a
/// signature can say `SDL3::SDL_FRect`.
pub fn import(
    module: Symbol,
    path: &str,
    span: Span,
    dirs: &[PathBuf],
    interner: &mut Interner,
) -> Result<Imported, String> {
    let Some(header) = find_header(path, dirs) else {
        return Err(format!("cannot find the header `{path}`"));
    };
    let scanned = rill_cimport::scan_header(&header, dirs)?;
    let mut mapper = Mapper {
        module,
        types: scanned
            .types
            .iter()
            .map(|decl| (decl.name.as_str(), &decl.kind))
            .collect(),
        interner,
        span,
    };
    let mut decls = Vec::new();
    for function in &scanned.functions {
        if function.skipped.is_some() {
            continue;
        }
        let Some(def) = mapper.function(function) else {
            continue;
        };
        decls.push(ImportedDecl {
            name: mapper.symbol(&function.name),
            kind: ItemKind::Fn(def),
            docs: function.docs.clone(),
            file: function.header.clone(),
            line: function.line,
        });
    }
    for ty in &scanned.types {
        let Some(def) = mapper.type_decl(&ty.name, &ty.kind) else {
            continue;
        };
        decls.push(ImportedDecl {
            name: mapper.symbol(&ty.name),
            kind: ItemKind::Type(def),
            docs: ty.docs.clone(),
            file: ty.header.clone(),
            line: ty.line,
        });
    }
    for constant in &scanned.constants {
        let Some(ty) = mapper.c_type(&constant.ty) else {
            continue;
        };
        decls.push(ImportedDecl {
            name: mapper.symbol(&constant.name),
            kind: ItemKind::Constant(ConstantDef {
                value: constant.value.clone(),
                ty,
            }),
            docs: constant.docs.clone(),
            file: constant.header.clone(),
            line: constant.line,
        });
    }
    // A library named after its header is the common case (`SDL3/SDL.h` links
    // `-lSDL3`, `SDL3_image/SDL_image.h` links `-lSDL3_image`); `@link` says so
    // explicitly when the guess would be wrong.
    Ok(Imported {
        decls,
        headers: scanned.headers,
    })
}

struct Mapper<'a> {
    module: Symbol,
    types: HashMap<&'a str, &'a CTypeKind>,
    interner: &'a mut Interner,
    span: Span,
}

impl Mapper<'_> {
    fn symbol(&mut self, name: &str) -> Symbol {
        self.interner.intern(name)
    }

    fn path(&mut self, name: &str) -> Type {
        let sym = self.symbol(name);
        Type::Path {
            segments: vec![Ident::new(sym, self.span)],
            span: self.span,
        }
    }

    /// A reference to a type the imported header declares, written so it stays
    /// inside the module: `SDL3::SDL_FRect`.
    fn module_path(&mut self, name: &str) -> Type {
        let module = self.module;
        let sym = self.symbol(name);
        Type::Path {
            segments: vec![Ident::new(module, self.span), Ident::new(sym, self.span)],
            span: self.span,
        }
    }

    fn function(&mut self, function: &rill_cimport::CFunction) -> Option<FnDef> {
        let ret = self.c_type(&function.ret)?;
        let mut params = Vec::new();
        for param in &function.params {
            params.push(self.param(param)?);
        }
        Some(FnDef {
            abi: Some("c".to_string()),
            params,
            ret: Some(ParamSlot {
                convention: None,
                ty: ret,
                span: self.span,
            }),
            constraints: Vec::new(),
            effects: self.io_effects(),
            body: None,
        })
    }

    /// A parameter, and the place convention its C type implies.
    ///
    /// `T *` covers two different things in C headers, and the distinction
    /// decides how a call site passes it:
    ///
    /// * an out- or in-place parameter over a struct, union or scalar
    ///   (`SDL_Event *`, `const SDL_FRect *`, `int *numkeys`) is a place in
    ///   Rill: `inout T`, called as `f (inout value)`, which emits `&value`;
    /// * a pointer *value* - opaque handles (`SDL_Window *`) and `void *`,
    ///   which a caller already holds from somewhere else - keeps its pointer
    ///   type.
    fn param(&mut self, param: &rill_cimport::CParam) -> Option<Param> {
        let name = self.symbol(&param.name);
        let (convention, ty) = match &param.ty {
            CType::Ptr { inner, .. } => {
                let pointee_is_place = match self.resolve_named(inner) {
                    Some(CTypeKind::Record(_)) | Some(CTypeKind::Enum) => true,
                    Some(CTypeKind::Alias(inner)) => {
                        matches!(inner, CType::Int { .. } | CType::Float { .. } | CType::Bool)
                    }
                    Some(CTypeKind::Opaque) => false,
                    None => matches!(
                        &**inner,
                        CType::Int { .. } | CType::Float { .. } | CType::Bool
                    ),
                };
                if pointee_is_place {
                    (Some(rill_ast::Convention::Inout), self.c_type(inner)?)
                } else {
                    (None, self.c_type(&param.ty)?)
                }
            }
            _ => (None, self.c_type(&param.ty)?),
        };
        Some(Param {
            local: None,
            name: Some(name),
            convention,
            ty: Some(ty),
            comptime: false,
            span: self.span,
        })
    }

    /// The kind of a name seen in the scan, following typedef aliases.
    fn resolve_named(&self, ty: &CType) -> Option<&CTypeKind> {
        let mut current = ty;
        for _ in 0..8 {
            match current {
                CType::Named(name) => match self.types.get(name.as_str()) {
                    Some(CTypeKind::Alias(inner)) => current = inner,
                    Some(kind) => return Some(*kind),
                    None => return None,
                },
                _ => return None,
            }
        }
        None
    }

    fn type_decl(&mut self, name: &str, kind: &CTypeKind) -> Option<TypeDef> {
        let body = match kind {
            // A struct that is only forward-declared in C: an empty record with
            // the header's name, so `&raw mut SDL_Window` names a real type.
            CTypeKind::Opaque => TypeBody::Record(RecordDef {
                fields: Vec::new(),
                deinit: None,
                span: self.span,
            }),
            CTypeKind::Record(fields) => {
                let mut mapped = Vec::new();
                for field in fields {
                    // A field the mapper cannot type keeps a hole rather than
                    // dropping the field: designs by position (`SDL_FRect`)
                    // would shift everything after it.
                    let ty = self.c_type(&field.ty).unwrap_or(Type::Error(self.span));
                    let name = self.symbol(&field.name);
                    mapped.push(FieldDef {
                        mutable: false,
                        name: Ident::new(name, self.span),
                        ty,
                        docs: field.docs.clone(),
                        span: self.span,
                    });
                }
                TypeBody::Record(RecordDef {
                    fields: mapped,
                    deinit: None,
                    span: self.span,
                })
            }
            // An enum is an `int` to every caller; its values arrive as
            // constants.
            CTypeKind::Enum => TypeBody::Alias(self.path("i32")),
            CTypeKind::Alias(inner) => TypeBody::Alias(self.c_type(inner)?),
        };
        Some(TypeDef {
            params: Vec::new(),
            body,
            c_name: Some(self.symbol(name)),
        })
    }

    /// C types to the Rill types that spell them. `None` is "not expressible",
    /// which drops the declaration that needed it.
    fn c_type(&mut self, ty: &CType) -> Option<Type> {
        Some(match ty {
            CType::Void => Type::unit(self.span),
            CType::Bool => self.path("Bool"),
            CType::Text => self.path("Text"),
            CType::Int { bits, signed } => {
                self.path(&format!("{}{bits}", if *signed { 'i' } else { 'u' }))
            }
            CType::Float { bits } => self.path(&format!("f{bits}")),
            CType::Ptr { mutable, inner } => Type::Ptr {
                mutable: *mutable,
                pointee: Box::new(self.c_type(inner)?),
                span: self.span,
            },
            CType::Named(name) => match self.types.get(name.as_str()) {
                Some(CTypeKind::Alias(inner)) => self.c_type(inner)?,
                Some(CTypeKind::Enum) => self.path("i32"),
                Some(CTypeKind::Opaque) | Some(CTypeKind::Record(_)) => self.module_path(name),
                None => return None,
            },
            CType::Unmapped(_) => return None,
        })
    }

    /// C has no effects of its own; every imported call is `<io>`.
    fn io_effects(&mut self) -> EffectRow {
        let name = self.symbol("io");
        EffectRow {
            items: vec![EffectItem {
                name: Ident::new(name, self.span),
                args: Vec::new(),
                span: self.span,
            }],
            span: self.span,
        }
    }
}
