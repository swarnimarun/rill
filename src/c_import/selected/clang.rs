//! All unsafe operations are confined to libclang's stable, runtime-loaded C API.
//! Cursors/types never escape their live translation unit; no Rust callback may
//! unwind through C. The library remains loaded until all C resources are gone.
#![allow(non_upper_case_globals)]
use super::facts::{
    BuiltinKind, ConstantFact, ConstantType, ConstantValue, FrontendFacts, ScalarProbe,
};
use super::{identifier, Automatic, Selection};
use crate::ast::ExternType;
use clang_sys::*;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::{CStr, CString, OsString};
use std::fmt::Write;
use std::path::PathBuf;
use std::ptr;

const MAX_DIAGNOSTICS: usize = 32 * 1024;
const MAX_CURSORS: usize = 200_000;
const PROBES: [(&str, &str, ExternType); 7] = [
    ("__rill_import_i32", "int32_t", ExternType::I32),
    ("__rill_import_u8", "uint8_t", ExternType::U8),
    ("__rill_import_u32", "uint32_t", ExternType::U32),
    ("__rill_import_u64", "uint64_t", ExternType::U64),
    ("__rill_import_f32", "float", ExternType::F32),
    ("__rill_import_bool", "_Bool", ExternType::Bool),
    ("__rill_import_void", "void", ExternType::Unit),
];

pub(super) struct Imported {
    pub module: String,
    pub facts: FrontendFacts,
}

struct Library(bool);
impl Library {
    fn load() -> Result<Self, String> {
        let existing = clang_sys::is_loaded();
        if !existing {
            clang_sys::load().map_err(|e| {
                format!(
                    "E-C-IMPORT: cannot load libclang: {}",
                    bounded(&e, MAX_DIAGNOSTICS)
                )
            })?;
        }
        let library = Self(!existing);
        if clang_sys::get_library()
            .and_then(|l| l.version())
            .is_none_or(|v| v < clang_sys::Version::V12_0)
        {
            return Err("E-C-IMPORT: libclang 12 or newer is required".into());
        }
        Ok(library)
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        if self.0 {
            let _ = clang_sys::unload();
        }
    }
}
struct Index(CXIndex);
impl Drop for Index {
    fn drop(&mut self) {
        unsafe { clang_disposeIndex(self.0) }
    }
}
struct Unit(CXTranslationUnit);
impl Drop for Unit {
    fn drop(&mut self) {
        unsafe { clang_disposeTranslationUnit(self.0) }
    }
}
struct Target(CXTargetInfo);
impl Drop for Target {
    fn drop(&mut self) {
        unsafe { clang_TargetInfo_dispose(self.0) }
    }
}
struct Evaluation(CXEvalResult);
impl Drop for Evaluation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { clang_EvalResult_dispose(self.0) }
        }
    }
}

fn bounded(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

// The CXString comes from libclang and is disposed exactly once after copying.
unsafe fn string(value: CXString) -> String {
    let raw = clang_getCString(value);
    let result = if raw.is_null() {
        String::new()
    } else {
        CStr::from_ptr(raw).to_string_lossy().into_owned()
    };
    clang_disposeString(value);
    result
}

unsafe fn location(cursor: CXCursor) -> String {
    let mut file = ptr::null_mut();
    let (mut line, mut column) = (0, 0);
    clang_getSpellingLocation(
        clang_getCursorLocation(cursor),
        &mut file,
        &mut line,
        &mut column,
        ptr::null_mut(),
    );
    let name = if file.is_null() {
        "<C input>".into()
    } else {
        string(clang_getFileName(file))
    };
    format!("{}:{line}:{column}", bounded(&name, 1024))
}

unsafe fn reject(cursor: CXCursor, symbol: &str, reason: impl std::fmt::Display) -> String {
    format!(
        "E-C-IMPORT: {symbol} at {}: {}",
        location(cursor),
        bounded(&reason.to_string(), 4096)
    )
}

impl Unit {
    fn parse(index: &Index, source: &str, flags: &[OsString]) -> Result<Self, String> {
        let unit = Self::parse_probe(index, source, flags)?;
        unit.check_diagnostics()?;
        Ok(unit)
    }

    // Macro activity can be queried even when its value is not a valid C
    // initializer. Callers must check diagnostics before using any AST types.
    fn parse_probe(index: &Index, source: &str, flags: &[OsString]) -> Result<Self, String> {
        let filename = CString::new("__rill_import_input.c").unwrap();
        let text = CString::new(source).map_err(|_| "E-C-IMPORT: NUL in generated C input")?;
        let args = [
            OsString::from("-x"),
            OsString::from("c"),
            OsString::from("-std=c11"),
        ]
        .into_iter()
        .chain(flags.iter().cloned())
        .map(|arg| {
            CString::new(arg.as_encoded_bytes()).map_err(|_| "E-C-IMPORT: NUL in C argument")
        })
        .collect::<Result<Vec<_>, _>>()?;
        let pointers: Vec<_> = args.iter().map(|arg| arg.as_ptr()).collect();
        let mut unsaved = CXUnsavedFile {
            Filename: filename.as_ptr(),
            Contents: text.as_ptr(),
            Length: source.len() as _,
        };
        let mut raw = ptr::null_mut();
        // Strings and pointer arrays outlive parsing; the TU owns its parsed data.
        let code = unsafe {
            clang_parseTranslationUnit2(
                index.0,
                filename.as_ptr(),
                pointers.as_ptr(),
                pointers.len() as _,
                &mut unsaved,
                1,
                CXTranslationUnit_DetailedPreprocessingRecord,
                &mut raw,
            )
        };
        if code != CXError_Success || raw.is_null() {
            if !raw.is_null() {
                unsafe { clang_disposeTranslationUnit(raw) };
            }
            return Err(format!("E-C-IMPORT: libclang could not parse C input (error {code}); check target, sysroot and resource headers"));
        }
        Ok(Self(raw))
    }

    fn check_diagnostics(&self) -> Result<(), String> {
        let mut messages = String::new();
        let mut errors = 0;
        unsafe {
            for n in 0..clang_getNumDiagnostics(self.0) {
                let diagnostic = clang_getDiagnostic(self.0, n);
                if clang_getDiagnosticSeverity(diagnostic) >= CXDiagnostic_Error {
                    errors += 1;
                    if messages.len() < MAX_DIAGNOSTICS {
                        let message = string(clang_formatDiagnostic(
                            diagnostic,
                            clang_defaultDiagnosticDisplayOptions(),
                        ));
                        messages.push_str(bounded(&message, MAX_DIAGNOSTICS - messages.len()));
                        messages.push('\n');
                    }
                }
                clang_disposeDiagnostic(diagnostic);
            }
        }
        if errors == 0 {
            Ok(())
        } else {
            Err(format!(
                "E-C-IMPORT: C frontend reported {errors} error(s):\n{messages}"
            ))
        }
    }

    fn selected(&self, names: HashSet<String>) -> Result<HashMap<String, Vec<CXCursor>>, String> {
        struct Visit {
            names: HashSet<String>,
            found: HashMap<String, Vec<CXCursor>>,
            visited: usize,
            failed: bool,
        }
        extern "C" fn visit(
            cursor: CXCursor,
            _: CXCursor,
            data: CXClientData,
        ) -> CXChildVisitResult {
            // libclang calls synchronously with the same valid stack pointer.
            let state = unsafe { &mut *(data.cast::<Visit>()) };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                state.visited += 1;
                if state.visited > MAX_CURSORS {
                    state.failed = true;
                    return CXChildVisit_Break;
                }
                let kind = clang_getCursorKind(cursor);
                if matches!(
                    kind,
                    CXCursor_FunctionDecl
                        | CXCursor_MacroDefinition
                        | CXCursor_EnumConstantDecl
                        | CXCursor_TypedefDecl
                        | CXCursor_VarDecl
                ) {
                    let name = string(clang_getCursorSpelling(cursor));
                    if state.names.contains(&name) {
                        state.found.entry(name).or_default().push(cursor);
                    }
                }
                if kind == CXCursor_EnumDecl {
                    CXChildVisit_Recurse
                } else {
                    CXChildVisit_Continue
                }
            }));
            match result {
                Ok(result) => result,
                Err(_) => {
                    state.failed = true;
                    CXChildVisit_Break
                }
            }
        }
        let mut state = Visit {
            names,
            found: HashMap::new(),
            visited: 0,
            failed: false,
        };
        unsafe {
            clang_visitChildren(
                clang_getTranslationUnitCursor(self.0),
                visit,
                (&mut state as *mut Visit).cast(),
            );
        }
        if state.failed {
            Err("E-C-IMPORT: declaration traversal failed or exceeded 200000 cursors".into())
        } else {
            Ok(state.found)
        }
    }

    fn dependencies(&self) -> Result<Vec<PathBuf>, String> {
        struct Visit {
            paths: BTreeSet<PathBuf>,
            bytes: usize,
            error: Option<String>,
        }
        extern "C" fn visit(
            file: CXFile,
            _: *mut CXSourceLocation,
            length: u32,
            data: CXClientData,
        ) {
            let state = unsafe { &mut *data.cast::<Visit>() };
            if state.error.is_some() || file.is_null() || length == 0 {
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                let name = clang_getFileName(file);
                let raw = clang_getCString(name);
                let bytes = if raw.is_null() {
                    Vec::new()
                } else {
                    CStr::from_ptr(raw).to_bytes().to_vec()
                };
                clang_disposeString(name);
                if bytes.is_empty() || bytes.len() > 16 * 1024 {
                    return Err("E-C-IMPORT: invalid or excessive header dependency path".into());
                }
                #[cfg(unix)]
                let path = {
                    use std::os::unix::ffi::OsStringExt;
                    PathBuf::from(OsString::from_vec(bytes))
                };
                #[cfg(not(unix))]
                let path = PathBuf::from(String::from_utf8(bytes).map_err(|_| {
                    "E-C-IMPORT: header dependency path is not representable on this host"
                })?);
                let path = if path.is_absolute() {
                    path
                } else {
                    std::env::current_dir()
                        .map_err(|e| format!("E-C-IMPORT: cannot resolve header dependency: {e}"))?
                        .join(path)
                };
                if path.as_os_str().as_encoded_bytes().len() > 16 * 1024 {
                    return Err("E-C-IMPORT: excessive absolute header dependency path".into());
                }
                if state.paths.insert(path.clone()) {
                    state.bytes += path.as_os_str().as_encoded_bytes().len();
                    if state.paths.len() > 1024 || state.bytes > 512 * 1024 {
                        return Err(
                            "E-C-IMPORT: header dependencies exceed 1024 paths or 512 KiB".into(),
                        );
                    }
                }
                Ok::<(), String>(())
            }));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => state.error = Some(error),
                Err(_) => {
                    state.error = Some("E-C-IMPORT: header dependency traversal failed".into())
                }
            }
        }
        let mut state = Visit {
            paths: BTreeSet::new(),
            bytes: 0,
            error: None,
        };
        unsafe {
            clang_getInclusions(self.0, visit, (&mut state as *mut Visit).cast());
        }
        match state.error {
            Some(error) => Err(error),
            None => Ok(state.paths.into_iter().collect()),
        }
    }

    fn expanded_macro(&self, name: &str, offset: usize) -> Result<Option<CXCursor>, String> {
        struct Visit<'a> {
            name: &'a str,
            file: CXFile,
            offset: usize,
            definition: Option<CXCursor>,
            visited: usize,
            failed: bool,
        }
        extern "C" fn visit(
            cursor: CXCursor,
            _: CXCursor,
            data: CXClientData,
        ) -> CXChildVisitResult {
            let state = unsafe { &mut *data.cast::<Visit<'_>>() };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                state.visited += 1;
                if state.visited > MAX_CURSORS {
                    state.failed = true;
                    return CXChildVisit_Break;
                }
                if clang_getCursorKind(cursor) == CXCursor_MacroExpansion
                    && string(clang_getCursorSpelling(cursor)) == state.name
                {
                    let mut file = ptr::null_mut();
                    let mut offset = 0;
                    clang_getExpansionLocation(
                        clang_getCursorLocation(cursor),
                        &mut file,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        &mut offset,
                    );
                    if !file.is_null()
                        && clang_File_isEqual(file, state.file) != 0
                        && offset as usize >= state.offset
                    {
                        state.definition = Some(clang_getCursorReferenced(cursor));
                    }
                }
                CXChildVisit_Continue
            }));
            match result {
                Ok(result) => result,
                Err(_) => {
                    state.failed = true;
                    CXChildVisit_Break
                }
            }
        }
        let filename = CString::new("__rill_import_input.c").unwrap();
        let mut state = Visit {
            name,
            offset,
            file: unsafe { clang_getFile(self.0, filename.as_ptr()) },
            definition: None,
            visited: 0,
            failed: false,
        };
        unsafe {
            clang_visitChildren(
                clang_getTranslationUnitCursor(self.0),
                visit,
                (&mut state as *mut Visit<'_>).cast(),
            );
        }
        if state.failed {
            Err("E-C-IMPORT: macro expansion traversal failed".into())
        } else {
            Ok(state.definition)
        }
    }
}

struct Types(Vec<(CXType, ExternType)>);
impl Types {
    unsafe fn new(found: &HashMap<String, Vec<CXCursor>>) -> Result<Self, String> {
        let mut types = Vec::new();
        for (probe, _, ty) in PROBES {
            let cursors = found
                .get(probe)
                .ok_or_else(|| format!("E-C-IMPORT: missing frontend scalar probe {probe}"))?;
            if cursors.len() != 1 || clang_getCursorKind(cursors[0]) != CXCursor_TypedefDecl {
                return Err(format!(
                    "E-C-IMPORT: conflicting frontend scalar probe {probe}"
                ));
            }
            let canonical = clang_getCanonicalType(clang_getTypedefDeclUnderlyingType(cursors[0]));
            let supported = match ty {
                ExternType::I32 => {
                    matches!(canonical.kind, CXType_Int | CXType_Long | CXType_LongLong)
                }
                ExternType::U8 | ExternType::U32 | ExternType::U64 => matches!(
                    canonical.kind,
                    CXType_UChar | CXType_UShort | CXType_UInt | CXType_ULong | CXType_ULongLong
                ),
                ExternType::F32 => canonical.kind == CXType_Float,
                ExternType::Bool => canonical.kind == CXType_Bool,
                ExternType::Unit => canonical.kind == CXType_Void,
                _ => false,
            };
            if !supported {
                return Err(format!(
                    "E-C-IMPORT: unsupported target scalar definition for {probe}"
                ));
            }
            types.push((canonical, ty));
        }
        Ok(Self(types))
    }

    unsafe fn scalar(&self, ty: CXType) -> Option<ExternType> {
        let canonical = clang_getCanonicalType(ty);
        // clang_equalTypes includes qualifiers; compare builtin kinds after
        // canonicalization, not textual spelling or width alone. Each supported
        // integer kind denotes one C type, unlike record/enum kinds.
        self.0
            .iter()
            .find(|(probe, _)| probe.kind == canonical.kind)
            .map(|(_, rill)| rill.clone())
    }

    unsafe fn convert(&self, ty: CXType, result: bool) -> Result<ExternType, String> {
        let canonical = clang_getCanonicalType(ty);
        if clang_isVolatileQualifiedType(canonical) != 0
            || clang_isRestrictQualifiedType(canonical) != 0
            || clang_getAddressSpace(canonical) != 0
        {
            return Err(
                "volatile, restrict or non-default address-space types are unsupported".into(),
            );
        }
        if let Some(scalar) = self.scalar(ty) {
            if scalar == ExternType::Unit && !result {
                return Err("void parameter is unsupported".into());
            }
            return Ok(scalar);
        }
        if canonical.kind != CXType_Pointer {
            return Err(format!("unsupported C type {} (aggregates, enums, callbacks and unrepresented scalars need a C shim)", string(clang_getTypeSpelling(ty))));
        }
        // Use the original pointee to preserve typedef identity; query canonical
        // qualifiers because qualifiers inherited through typedefs are otherwise lost.
        let mut pointer = ty;
        for _ in 0..64 {
            pointer = match pointer.kind {
                CXType_Elaborated => clang_Type_getNamedType(pointer),
                CXType_Typedef => {
                    clang_getTypedefDeclUnderlyingType(clang_getTypeDeclaration(pointer))
                }
                _ => break,
            };
        }
        let mut pointee = clang_getPointeeType(pointer);
        if pointee.kind == CXType_Invalid {
            return Err("cannot recover pointer typedef structure".into());
        }
        let underlying = clang_getCanonicalType(pointee);
        if clang_isVolatileQualifiedType(underlying) != 0
            || clang_isRestrictQualifiedType(underlying) != 0
            || clang_getAddressSpace(underlying) != 0
        {
            return Err("qualified/address-space pointee is unsupported".into());
        }
        let is_const = clang_isConstQualifiedType(underlying) != 0;
        if matches!(underlying.kind, CXType_Char_S | CXType_Char_U) && is_const && !result {
            return Ok(ExternType::CStr);
        }
        let pointee_name = if let Some(scalar) = self.scalar(pointee) {
            if scalar == ExternType::Unit {
                "void".into()
            } else {
                spelling(&scalar)
            }
        } else if underlying.kind == CXType_Record {
            while pointee.kind == CXType_Elaborated {
                pointee = clang_Type_getNamedType(pointee);
            }
            if pointee.kind != CXType_Typedef {
                return Err("struct/union-tag-only pointers need a named typedef".into());
            }
            let declaration = clang_getTypeDeclaration(pointee);
            let name = string(clang_getCursorSpelling(declaration));
            if !identifier(&name) {
                return Err("opaque pointee typedef is not representable in Rill".into());
            }
            name
        } else {
            return Err(
                "nested pointers, callbacks, arrays and this pointee type are unsupported".into(),
            );
        };
        Ok(ExternType::Pointer {
            pointee: pointee_name,
            is_const,
        })
    }
}

fn spelling(ty: &ExternType) -> String {
    match ty {
        ExternType::I32 => "i32".into(),
        ExternType::U8 => "u8".into(),
        ExternType::U32 => "u32".into(),
        ExternType::U64 => "u64".into(),
        ExternType::F32 => "f32".into(),
        ExternType::Bool => "bool".into(),
        ExternType::Unit => "unit".into(),
        ExternType::CStr => "cstr".into(),
        ExternType::Pointer { pointee, is_const } => {
            format!("{}<{pointee}>", if *is_const { "const_ptr" } else { "ptr" })
        }
    }
}

unsafe fn function_declaration(
    cursor: CXCursor,
    name: &str,
    types: &Types,
) -> Result<String, String> {
    if clang_getCursorKind(cursor) != CXCursor_FunctionDecl {
        return Err(reject(
            cursor,
            name,
            "selection is not an ordinary function; macros are not foreign function symbols",
        ));
    }
    let ty = clang_getCanonicalType(clang_getCursorType(cursor));
    if ty.kind != CXType_FunctionProto {
        return Err(reject(
            cursor,
            name,
            "unprototyped functions are unsupported",
        ));
    }
    if clang_isFunctionTypeVariadic(ty) != 0 {
        return Err(reject(cursor, name, "variadic functions are unsupported"));
    }
    if clang_getFunctionTypeCallingConv(ty) != CXCallingConv_C {
        return Err(reject(
            cursor,
            name,
            "non-default calling convention is unsupported",
        ));
    }
    if clang_getCursorLinkage(cursor) != CXLinkage_External {
        return Err(reject(cursor, name, "function must have external linkage"));
    }
    let result = types
        .convert(clang_getCursorResultType(cursor), true)
        .map_err(|e| reject(cursor, name, e))?;
    let argc = clang_Cursor_getNumArguments(cursor);
    if !(0..=128).contains(&argc) {
        return Err(reject(cursor, name, "invalid or excessive argument count"));
    }
    let mut params = Vec::new();
    for n in 0..argc {
        let parameter = clang_Cursor_getArgument(cursor, n as _);
        let ty = types
            .convert(clang_getCursorType(parameter), false)
            .map_err(|e| reject(parameter, name, e))?;
        params.push(format!("arg{n}: {}", spelling(&ty)));
    }
    Ok(format!(
        "export extern fn {name}({}) -> {};\n",
        params.join(", "),
        spelling(&result)
    ))
}

enum MacroBinding {
    Function(String),
    Constant,
    Missing,
}

// An object macro can rename an ordinary C function. Ask the frontend to
// resolve its expanded initializer; no signature or alias is inferred from
// macro tokens. All cursors and type conversions remain inside this probe TU.
fn function_alias(
    index: &Index,
    source: &str,
    flags: &[OsString],
    name: &str,
    origin: CXCursor,
    has_declaration: bool,
) -> Result<MacroBinding, String> {
    let probe = "__rill_import_function_alias";
    let defined = "__rill_import_macro_defined";
    let text = format!("{source}\n#ifdef {name}\ntypedef int {defined};\n#endif\n__typeof__(&({name})) {probe} = &({name});\n");
    let unit = Unit::parse_probe(index, &text, flags)?;
    let names = PROBES
        .iter()
        .map(|(name, _, _)| (*name).to_owned())
        .chain([probe.to_owned(), defined.to_owned()])
        .collect();
    let found = unit.selected(names)?;
    if !found.contains_key(defined) && !has_declaration {
        return Ok(MacroBinding::Missing);
    }
    if found.contains_key(defined)
        && unit
            .expanded_macro(name, source.len())?
            .is_none_or(|cursor| unsafe { clang_Cursor_isMacroFunctionLike(cursor) != 0 })
    {
        return Err(unsafe { reject(origin, name, "function-like macros are unsupported") });
    }
    if unit.check_diagnostics().is_err() {
        // Integer literal macros, empty guards and other non-functions retain
        // the ordinary constant extraction and its precise diagnostics.
        return Ok(MacroBinding::Constant);
    }
    let cursor = *found
        .get(probe)
        .and_then(|cursors| cursors.first())
        .ok_or_else(|| format!("E-C-IMPORT: {name}: missing macro type probe"))?;
    unsafe {
        let ty = clang_getCanonicalType(clang_getCursorType(cursor));
        if ty.kind != CXType_Pointer
            || !matches!(
                clang_getPointeeType(ty).kind,
                CXType_FunctionProto | CXType_FunctionNoProto
            )
        {
            return Ok(MacroBinding::Constant);
        }
    }
    struct Visit {
        function: Option<CXCursor>,
        visited: usize,
        failed: bool,
    }
    extern "C" fn visit(cursor: CXCursor, _: CXCursor, data: CXClientData) -> CXChildVisitResult {
        let state = unsafe { &mut *data.cast::<Visit>() };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            state.visited += 1;
            if state.visited > MAX_CURSORS {
                state.failed = true;
                return CXChildVisit_Break;
            }
            if clang_getCursorKind(cursor) == CXCursor_DeclRefExpr {
                let referenced = clang_getCursorReferenced(cursor);
                if clang_getCursorKind(referenced) == CXCursor_FunctionDecl {
                    let function = clang_getCanonicalCursor(referenced);
                    if state
                        .function
                        .is_some_and(|previous| clang_equalCursors(previous, function) == 0)
                    {
                        state.failed = true;
                        return CXChildVisit_Break;
                    }
                    state.function = Some(function);
                }
            }
            CXChildVisit_Recurse
        }));
        match result {
            Ok(result) => result,
            Err(_) => {
                state.failed = true;
                CXChildVisit_Break
            }
        }
    }
    let mut state = Visit {
        function: None,
        visited: 0,
        failed: false,
    };
    unsafe {
        clang_visitChildren(cursor, visit, (&mut state as *mut Visit).cast());
    }
    if state.failed {
        return Err(format!(
            "E-C-IMPORT: {name}: macro must refer to one ordinary function"
        ));
    }
    let Some(function) = state.function else {
        return Err(format!("E-C-IMPORT: {name}: macro function pointers without an ordinary function declaration are unsupported"));
    };
    unsafe {
        function_declaration(function, name, &Types::new(&found)?).map(MacroBinding::Function)
    }
}

fn builtin_kind(kind: CXTypeKind) -> Result<BuiltinKind, String> {
    match kind {
        CXType_Int => Ok(BuiltinKind::Int),
        CXType_Long => Ok(BuiltinKind::Long),
        CXType_LongLong => Ok(BuiltinKind::LongLong),
        CXType_UChar => Ok(BuiltinKind::UChar),
        CXType_UShort => Ok(BuiltinKind::UShort),
        CXType_UInt => Ok(BuiltinKind::UInt),
        CXType_ULong => Ok(BuiltinKind::ULong),
        CXType_ULongLong => Ok(BuiltinKind::ULongLong),
        CXType_Float => Ok(BuiltinKind::Float),
        CXType_Bool => Ok(BuiltinKind::Bool),
        _ => Err("E-C-IMPORT: unsupported frontend scalar kind".into()),
    }
}

pub(super) fn extract(selection: &Selection, flags: &[OsString]) -> Result<Imported, String> {
    extract_requested(selection, flags, None).map(|(imported, _)| imported)
}

pub(super) fn extract_automatic(
    request: &Automatic,
    flags: &[OsString],
) -> Result<(Imported, Vec<PathBuf>), String> {
    let selection = Selection {
        native: request.native.clone(),
        c_args: request.c_args.clone(),
        resource_dir: request.resource_dir.clone(),
        ..Selection::default()
    };
    extract_requested(&selection, flags, Some(&request.names))
}

fn extract_requested(
    selection: &Selection,
    flags: &[OsString],
    automatic: Option<&[String]>,
) -> Result<(Imported, Vec<PathBuf>), String> {
    let mut selection = selection.clone();
    let _library = Library::load()?;
    let raw_index = unsafe { clang_createIndex(1, 0) };
    if raw_index.is_null() {
        return Err("E-C-IMPORT: cannot create libclang index".into());
    }
    let index = Index(raw_index);
    let mut flags = flags.to_vec();
    if let Some(directory) = &selection.resource_dir {
        flags.push("-resource-dir".into());
        flags.push(directory.as_os_str().to_owned());
    }
    let flags = &flags;
    let mut source = selection.native.preamble()?;
    source.push_str("#include <stdint.h>\n#include <limits.h>\n");
    for (probe, c, _) in PROBES {
        writeln!(source, "typedef {c} {probe};").unwrap();
    }
    source.push_str("_Static_assert(CHAR_BIT == 8, \"Rill requires 8-bit bytes\");\n");
    let unit = Unit::parse(&index, &source, flags)?;
    let names = automatic
        .map(|names| names.to_vec())
        .unwrap_or_else(|| {
            selection
                .functions
                .iter()
                .chain(&selection.constants)
                .cloned()
                .collect()
        })
        .into_iter()
        .chain(PROBES.iter().map(|(name, _, _)| (*name).to_owned()))
        .collect();
    let found = unit.selected(names)?;
    let mut aliases = HashMap::new();
    if let Some(names) = automatic {
        for name in names {
            let Some(declarations) = found.get(name) else {
                continue;
            };
            let cursor = declarations
                .iter()
                .copied()
                .find(|cursor| unsafe { clang_getCursorKind(*cursor) } == CXCursor_MacroDefinition)
                .unwrap_or(declarations[0]);
            match unsafe { clang_getCursorKind(cursor) } {
                CXCursor_FunctionDecl => selection.functions.push(name.clone()),
                CXCursor_MacroDefinition => {
                    let has_declaration = declarations.iter().any(|cursor| unsafe {
                        clang_getCursorKind(*cursor) != CXCursor_MacroDefinition
                    });
                    match function_alias(&index, &source, flags, name, cursor, has_declaration)? {
                        MacroBinding::Function(declaration) => {
                            selection.functions.push(name.clone());
                            aliases.insert(name.clone(), declaration);
                        }
                        MacroBinding::Constant => selection.constants.push(name.clone()),
                        MacroBinding::Missing => {}
                    }
                }
                CXCursor_EnumConstantDecl => {
                    selection.constants.push(name.clone());
                }
                _ => {
                    return Err(unsafe {
                        reject(cursor, name, "only foreign functions, object macros and enum constants can be imported as values")
                    });
                }
            }
        }
    }
    let dependencies = if automatic.is_some() {
        unit.dependencies()?
    } else {
        Vec::new()
    };
    // Every cursor and type below belongs to the live `unit`. The second TU
    // used for macro probes is separate, and uses its own scalar probe types.
    unsafe {
        let types = Types::new(&found)?;
        let target = Target(clang_getTranslationUnitTargetInfo(unit.0));
        if target.0.is_null() {
            return Err("E-C-IMPORT: C frontend did not report a target".into());
        }
        let triple = string(clang_TargetInfo_getTriple(target.0));
        let pointer_width = clang_TargetInfo_getPointerWidth(target.0);
        if pointer_width <= 0 {
            return Err("E-C-IMPORT: C frontend did not report pointer width".into());
        }
        let mut module = format!("// Generated selected C bindings for {}.\n// Reuse the same target, sysroot, defines and include paths when building.\n// Raw pointers have no checked lifetime or automatic resource cleanup.\n", bounded(&triple, 256));
        for (directive, values) in [
            ("include", &selection.native.includes),
            ("link", &selection.native.links),
            ("pkg_config", &selection.native.packages),
        ] {
            for value in values {
                writeln!(module, "@{directive}(\"{value}\");").unwrap();
            }
        }
        module.push('\n');
        let mut scalar_probes = Vec::with_capacity(6);
        for ((probe, c, _), (ty, _)) in PROBES.iter().zip(&types.0) {
            if *probe != "__rill_import_void" {
                let size = clang_Type_getSizeOf(*ty);
                if !(1..=1024).contains(&size) {
                    return Err(format!(
                        "E-C-IMPORT: cannot determine supported size of {c}"
                    ));
                }
                scalar_probes.push(ScalarProbe {
                    kind: builtin_kind(ty.kind)?,
                    size_bytes: size as u32,
                });
            }
        }
        let scalar_probes = scalar_probes
            .try_into()
            .map_err(|_| "E-C-IMPORT: incorrect frontend scalar probe count")?;
        let mut facts = FrontendFacts {
            target_triple: triple,
            pointer_bits: pointer_width as u32,
            scalar_probes,
            constants: Vec::with_capacity(selection.constants.len()),
        };
        for name in &selection.functions {
            if let Some(declaration) = aliases.get(name) {
                module.push_str(declaration);
                continue;
            }
            let declarations = found
                .get(name)
                .ok_or_else(|| format!("E-C-IMPORT: {name}: selected function was not found"))?;
            let mut emitted = None;
            for &cursor in declarations {
                let declaration = function_declaration(cursor, name, &types)?;
                if emitted
                    .as_ref()
                    .is_some_and(|previous| previous != &declaration)
                {
                    return Err(reject(cursor, name, "conflicting declarations"));
                }
                emitted = Some(declaration);
            }
            module.push_str(
                &emitted.ok_or_else(|| format!("E-C-IMPORT: {name}: missing declaration"))?,
            );
        }
        for (n, name) in selection.constants.iter().enumerate() {
            let declarations = found
                .get(name)
                .ok_or_else(|| format!("E-C-IMPORT: {name}: selected constant was not found"))?;
            // Automatic macro probes already resolved the active definition;
            // historical definitions before #undef must not change its kind.
            for &cursor in declarations.iter().filter(|_| automatic.is_none()) {
                match clang_getCursorKind(cursor) {
                    CXCursor_MacroDefinition if clang_Cursor_isMacroFunctionLike(cursor) == 0 => {},
                    CXCursor_EnumConstantDecl => {},
                    _ => return Err(reject(cursor, name, "only object macros and enum constants are supported; function macros and runtime variables are not constants")),
                }
            }
            let probe = format!("__rill_import_constant_{n}");
            let probe_source = format!("{source}\n__typeof__({name}) {probe} = ({name});\n_Static_assert(__builtin_constant_p({name}), \"{name} must be a frontend constant\");\n");
            let constant_unit = Unit::parse(&index, &probe_source, flags)
                .map_err(|e| format!("E-C-IMPORT: {name}: {e}"))?;
            let constant_names = PROBES
                .iter()
                .map(|(name, _, _)| (*name).to_owned())
                .chain(std::iter::once(probe.clone()))
                .collect();
            let constants = constant_unit.selected(constant_names)?;
            let constant_types = Types::new(&constants)?;
            let cursor = *constants
                .get(&probe)
                .and_then(|cs| cs.first())
                .ok_or_else(|| format!("E-C-IMPORT: {name}: missing constant type probe"))?;
            let ty = constant_types
                .convert(clang_getCursorType(cursor), false)
                .map_err(|e| reject(declarations[0], name, e))?;
            let rill_type = match ty {
                ExternType::I32 => ConstantType::I32,
                ExternType::U8 => ConstantType::U8,
                ExternType::U32 => ConstantType::U32,
                ExternType::U64 => ConstantType::U64,
                ExternType::Bool => ConstantType::Bool,
                _ => {
                    return Err(reject(
                        declarations[0],
                        name,
                        "only integer/bool constants are supported",
                    ))
                }
            };
            let evaluation = Evaluation(clang_Cursor_Evaluate(cursor));
            if evaluation.0.is_null() || clang_EvalResult_getKind(evaluation.0) != CXEval_Int {
                return Err(reject(
                    declarations[0],
                    name,
                    "constant cannot be evaluated by the C frontend",
                ));
            }
            let value = if clang_EvalResult_isUnsignedInt(evaluation.0) != 0 {
                ConstantValue::Unsigned(clang_EvalResult_getAsUnsigned(evaluation.0))
            } else {
                ConstantValue::Signed(clang_EvalResult_getAsLongLong(evaluation.0))
            };
            writeln!(module, "export extern const {name}: {};", spelling(&ty)).unwrap();
            facts.constants.push(ConstantFact {
                name: name.clone(),
                rill_type,
                value,
            });
        }
        facts.validate(&selection.constants)?;
        Ok((Imported { module, facts }, dependencies))
    }
}
