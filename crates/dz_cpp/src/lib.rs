//! C++17 code generation (spec §16).
//!
//! One header and one translation unit per module (§16.1), `dz::`-scoped names (§16.2),
//! and a `#line` directive before every statement so debuggers, profilers and any C++
//! error that slips through all point at `.dz` source (§16.3).
//!
//! **Two inputs, one for each half of a module.** Declarations — what a struct's fields
//! are called, which constructors exist, what order members were written in — come from
//! the AST and the checker's signature table, because the HIR has no items. *Bodies* come
//! from the HIR: every name already bound, every expression already typed (spec §17.1).
//!
//! That split is what M5 bought. M4 emitted bodies from the AST and let C++ infer, which
//! worked while every Dazzle construct had a C++ expression to map onto. `or`, block
//! values and moves do not: `f() or return` is three statements, and knowing whether `x =
//! y` copies or moves needs `y`'s type. Both need the typed tree.
//!
//! **Statement hoisting.** Anything that is an expression in Dazzle but a statement in C++
//! — `or`, a block, an `if` or `switch` used as a value — emits its work as lines *before*
//! the statement being built and leaves a temporary behind as its value. [`Emitter::expr`]
//! may therefore write to the output; every caller is arranged so that it runs before the
//! line it contributes to is written. An immediately-invoked lambda would have been
//! shorter and is wrong: `return` inside one returns from the lambda, so `or return` — the
//! single most common clause in the language — would silently not propagate.

use std::collections::HashMap;
use std::fmt::Write;

use dz_ast::{
    AssignOp, BinaryOp, Dispatch, EnumDecl, Item, ItemKind, MemberKind, StructDecl, UnaryOp,
    UnionDecl,
};
use dz_check::{ItemSig, TypeTable};
use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_hir::ty::{FloatTy, IntTy, Ty};
use dz_hir::{self as hir, BlockId, BodyOwner, ExprId, Hir, LocalId, StmtId};
use dz_project::{ModuleId, Workspace};
use dz_resolve::{ParsedFile, Resolution, SymbolId};
use dz_span::{FileId, SourceMap, Span};

mod reflect;
mod runtime;

pub use runtime::{NATIVE_CPP, NATIVE_H, PRELUDE_H, RUNTIME_CPP, RUNTIME_H};

#[derive(Debug, Clone)]
pub struct GeneratedFile {
    /// Path relative to the build directory, e.g. `cpp/engine.h`.
    pub name: String,
    pub contents: String,
}

pub struct Emitted {
    pub files: Vec<GeneratedFile>,
    pub diagnostics: Diagnostics,
    /// The translation unit holding the real `int main()`, if the root module has one.
    pub entry: Option<String>,
}

/// What is being built (spec §19 M14).
///
/// The two differ only at the edges — the same declarations, the same bodies — but they
/// differ in a way that must not blur: a program must not carry its tests, and a test
/// binary has no use for the program's `main`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// The program. `#[test]` functions are left out entirely, and the root module's
    /// `main` becomes the entry point.
    #[default]
    Program,
    /// The test binary. `#[test]` functions are emitted, and the entry point is a runner
    /// over them; the program's own `main` is not called.
    Tests,
    /// The benchmark binary, the same way for `#[bench]` (spec §19 M15).
    Benches,
}

impl Mode {
    /// The attribute this mode keeps. `Program` keeps neither.
    fn attribute(self) -> Option<&'static str> {
        match self {
            Mode::Program => None,
            Mode::Tests => Some("test"),
            Mode::Benches => Some("bench"),
        }
    }
}

// Eight inputs because emission needs every earlier phase's output plus the mode;
// bundling them into a struct would only move the list somewhere else.
#[allow(clippy::too_many_arguments)]
pub fn emit(
    ws: &Workspace,
    res: &Resolution,
    table: &TypeTable,
    hir: &Hir,
    mono: &dz_mono::Mono,
    map: &SourceMap,
    files: &HashMap<ModuleId, Vec<ParsedFile>>,
    mode: Mode,
) -> Emitted {
    // Every struct's declaration by symbol, across all modules, so an instance of a generic
    // declared in another module can still be emitted from its own AST (spec §8.4).
    let mut struct_asts: HashMap<SymbolId, (FileId, &StructDecl)> = HashMap::new();
    let mut union_asts: HashMap<SymbolId, (FileId, &UnionDecl)> = HashMap::new();
    for (mid, pfs) in files {
        for pf in pfs {
            for item in &pf.ast.items {
                match &item.kind {
                    ItemKind::Struct(s) => {
                        if let Some(sym) = res.lookup(*mid, &s.name.name) {
                            struct_asts.insert(sym, (pf.file, s));
                        }
                    }
                    ItemKind::Union(u) => {
                        if let Some(sym) = res.lookup(*mid, &u.name.name) {
                            union_asts.insert(sym, (pf.file, u));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // The root module's setting governs the whole build, since one C++ invocation
    // compiles every translation unit.
    let standard = cpp_standard_version(&ws.module(ws.root).build.cpp_standard);

    let mut out = Emitted {
        files: vec![
            GeneratedFile { name: "dz_runtime.h".into(), contents: RUNTIME_H.into() },
            GeneratedFile { name: "dz_runtime.cpp".into(), contents: RUNTIME_CPP.into() },
            GeneratedFile { name: "dz_prelude.h".into(), contents: PRELUDE_H.into() },
            GeneratedFile { name: "dz_native.h".into(), contents: NATIVE_H.into() },
            GeneratedFile { name: "dz_native.cpp".into(), contents: NATIVE_CPP.into() },
        ],
        diagnostics: Diagnostics::new(),
        entry: None,
    };
    let reflection: reflect::Shared = Default::default();

    for module in ws.modules() {
        let Some(module_files) = files.get(&module.id) else { continue };
        let mut e = Emitter {
            ws,
            res,
            table,
            hir,
            map,
            module: module.id,
            file: FileId(0),
            diags: Diagnostics::new(),
            header: String::new(),
            out: String::new(),
            indent: 0,
            temp: 0,
            current_fn: String::new(),
            last_line: 0,
            standard,
            body: None,
            names: Vec::new(),
            mono,
            struct_asts: &struct_asts,
            union_asts: &union_asts,
            instance: None,
            impl_instance: None,
            method_instance: None,
            chunks: Vec::new(),
            header_ns: module.id,
            def_prefix: "",
            mode,
            reflect: &reflection,
        };
        e.module(module_files);
        e.finish_header();

        let stem = file_stem(&module.name);
        let ns_open = namespace_open(&module.name);
        let ns_close = namespace_close(&module.name);

        // The header is a sequence of namespace blocks: the module's own, and — for each
        // instance of a generic declared elsewhere — that generic's, since an instance
        // lives with its generic so every module using it names one type (spec §8.4).
        let mut header_body = String::new();
        for (m, text) in std::mem::take(&mut e.chunks) {
            let name = &ws.module(m).name;
            let _ =
                write!(header_body, "{}\n{text}\n{}", namespace_open(name), namespace_close(name));
        }

        let mut includes = String::from("#include \"dz_runtime.h\"\n#include \"dz_prelude.h\"\n");
        // What `std`'s `extern fn`s forward to (spec §7.2); no other module calls it.
        if module.id == ws.prelude {
            includes.push_str("#include \"dz_native.h\"\n");
        }
        for dep in module.deps.iter().filter(|d| ws.module(d.module).foreign.is_none()) {
            let _ = writeln!(includes, "#include \"{}.h\"", file_stem(&ws.module(dep.module).name));
        }
        // `rl = { cpp = "raylib" }` (spec §15.1): the header itself, exactly as the recipe's
        // `import` names it.
        let headers = module.deps.iter().filter_map(|d| ws.module(d.module).foreign.as_ref());
        for header in headers.flat_map(|f| &f.header.requested) {
            // **Angle brackets, deliberately.** A quoted include is searched from the
            // including file's own directory first, and that directory is `build/cpp` —
            // where a module named after the library it imports has just generated
            // `raylib.h`. The quoted form makes that file include *itself*, and its guard
            // is already defined, so the real header expands to nothing and every
            // declaration in it goes missing. The angle form searches only the `-I` paths,
            // which are exactly the include dirs the importer resolved the header with
            // (spec §15.1), so it finds the same file libclang read and can never find
            // generated output.
            let _ = writeln!(includes, "#include <{header}>");
        }

        let guard = format!("DZ_{}_H", stem.to_uppercase());
        let header = format!(
            "// Generated by the Dazzle compiler from module `{}`. Do not edit.\n\
             #ifndef {guard}\n#define {guard}\n\n{includes}\n{header_body}\n\
             #endif  // {guard}\n",
            module.name
        );
        let body = format!(
            "// Generated by the Dazzle compiler from module `{}`. Do not edit.\n\
             #include \"{stem}.h\"\n\n{ns_open}\n{}\n{ns_close}\n",
            module.name, e.out
        );

        out.files.push(GeneratedFile { name: format!("{stem}.h"), contents: header });
        out.files.push(GeneratedFile { name: format!("{stem}.cpp"), contents: body });
        out.diagnostics.extend(e.diags);

        // The root module's `main` becomes the program's real entry point.
        if module.id == ws.root && mode != Mode::Program {
            out.files.push(GeneratedFile {
                name: "dz_main.cpp".into(),
                contents: harness_entry(ws, res, table, mode),
            });
            out.entry = Some("dz_main.cpp".into());
        } else if module.id == ws.root {
            if res.lookup(module.id, "main").is_some() {
                let entry = format!(
                    "// Generated by the Dazzle compiler. Do not edit.\n\
                     #include \"{stem}.h\"\n#include \"dz_native.h\"\n\n\
                     int main(int argc, char** argv) {{\n    \
                     ::dz::native::process_start(argc, argv);\n    {}::main();\n    return 0;\n}}\n",
                    qualified_namespace(&module.name)
                );
                out.files.push(GeneratedFile { name: "dz_main.cpp".into(), contents: entry });
                out.entry = Some("dz_main.cpp".into());
            } else {
                out.diagnostics.push(
                    Diagnostic::error(
                        codes::NO_ENTRY_POINT,
                        format!("module `{}` has no `main`", module.name),
                    )
                    .with_help(
                        "an executable needs `fn main()`; set `output = \"static-lib\"` in \
                         dazzle.toml if this is a library",
                    ),
                );
            }
        }
    }

    // The type table, when anything reads it (spec §13). Written last, since every module's
    // code has by now named the types it reflects on.
    let type_info = ws.module(ws.root).build.type_info;
    if reflection.borrow().is_needed() && type_info != dz_project::TypeInfo::None {
        let mut e = Emitter {
            ws,
            res,
            table,
            hir,
            map,
            module: res.prelude,
            file: FileId(0),
            diags: Diagnostics::new(),
            header: String::new(),
            out: String::new(),
            indent: 0,
            temp: 0,
            current_fn: String::new(),
            last_line: 0,
            standard,
            body: None,
            names: Vec::new(),
            mono,
            struct_asts: &struct_asts,
            union_asts: &union_asts,
            instance: None,
            impl_instance: None,
            method_instance: None,
            chunks: Vec::new(),
            header_ns: res.prelude,
            def_prefix: "",
            mode,
            reflect: &reflection,
        };
        if type_info == dz_project::TypeInfo::All {
            reflect::seed_all(&mut e, files);
        }
        let headers: Vec<String> = out
            .files
            .iter()
            .map(|f| f.name.clone())
            .filter(|n| {
                n.ends_with(".h")
                    && !matches!(n.as_str(), "dz_runtime.h" | "dz_prelude.h" | "dz_native.h")
            })
            .collect();
        let contents = reflect::table_file(&mut e, &headers);
        out.files.push(GeneratedFile { name: "dz_typeinfo.cpp".into(), contents });
        out.diagnostics.extend(e.diags);
    }

    out.diagnostics.sort_by_position();
    out
}

/// The entry point of a test or benchmark binary (spec §19 M14, M15).
///
/// It runs **one** of them per invocation, named on the command line, and lists them when
/// asked. That is what lets the driver report every result even though a failing test
/// panics: a panic aborts the process (§9.8, and there are no exceptions to catch,
/// §16.5), so a runner that called them all in a row would stop at the first failure and
/// have nothing to say about the rest. One process each costs a spawn and buys a full
/// report — and isolates one that corrupts memory from the ones after it.
///
/// A benchmark additionally prints the nanoseconds it took per iteration, having chosen
/// the iteration count itself.
fn harness_entry(ws: &Workspace, res: &Resolution, table: &TypeTable, mode: Mode) -> String {
    let which: &[SymbolId] = match mode {
        Mode::Benches => &table.benches,
        _ => &table.tests,
    };
    let mut list = String::new();
    let mut dispatch = String::new();
    for &sym in which {
        let symbol = res.symbol(sym);
        let module = ws.module(symbol.module);
        // Qualified by module, so two modules may each have a `roundtrips` (spec §3.2).
        let name = format!("{}::{}", module.name, symbol.name);
        let call = format!("{}::{}", qualified_namespace(&module.name), sanitize(&symbol.name));
        let quoted = quote_cpp(&name);
        let _ = writeln!(list, "    std::puts({quoted});");
        let body = if mode == Mode::Benches {
            format!("run_one({call}); return 0;")
        } else {
            format!("{call}(); return 0;")
        };
        let _ = writeln!(dispatch, "    if (which == std::string({quoted})) {{ {body} }}");
    }

    let mut includes = String::new();
    let mut seen: Vec<String> = Vec::new();
    for &sym in which {
        let stem = file_stem(&ws.module(res.symbol(sym).module).name);
        if !seen.contains(&stem) {
            let _ = writeln!(includes, "#include \"{stem}.h\"");
            seen.push(stem);
        }
    }

    // The timing loop. Written here rather than in the shim because it is the harness,
    // not the language: a program that never runs a benchmark should not carry it.
    let timing = if mode == Mode::Benches {
        "\n\
         // Time one benchmark.\n\
         //\n\
         // Two things make a single measurement untrustworthy, and this answers both.\n\
         // A cheap body timed once measures the clock rather than the body, so the\n\
         // iteration count doubles until a batch lasts 50ms. And a machine running other\n\
         // work perturbs any one batch upward (never downward, since noise only ever\n\
         // adds time), so several batches are run and the **fastest** is reported. That is\n\
         // the closest thing to the body's own cost that a shared machine can show.\n\
         static void run_one(void (*body)()) {\n\
         \x20   const std::uint64_t target_ns = 50ull * 1000ull * 1000ull;\n\
         \x20   const int batches = 5;\n\
         \x20   // Warm up: first-call effects (cold caches, lazy page faults) belong to\n\
         \x20   // neither the language nor the benchmark.\n\
         \x20   body();\n\
         \x20   std::uint64_t iters = 1;\n\
         \x20   std::uint64_t elapsed = 0;\n\
         \x20   while (iters < (1ull << 32)) {\n\
         \x20       const std::uint64_t start = ::dz::now_nanos();\n\
         \x20       for (std::uint64_t i = 0; i < iters; ++i) { body(); }\n\
         \x20       elapsed = ::dz::now_nanos() - start;\n\
         \x20       if (elapsed >= target_ns) { break; }\n\
         \x20       iters *= 2;\n\
         \x20   }\n\
         \x20   double best = static_cast<double>(elapsed) / static_cast<double>(iters);\n\
         \x20   for (int b = 1; b < batches; ++b) {\n\
         \x20       const std::uint64_t start = ::dz::now_nanos();\n\
         \x20       for (std::uint64_t i = 0; i < iters; ++i) { body(); }\n\
         \x20       const std::uint64_t took = ::dz::now_nanos() - start;\n\
         \x20       const double per = static_cast<double>(took) /\n\
         \x20                          static_cast<double>(iters);\n\
         \x20       if (per < best) { best = per; }\n\
         \x20   }\n\
         \x20   std::printf(\"%.2f %llu\\n\", best,\n\
         \x20               static_cast<unsigned long long>(iters));\n\
         }\n"
    } else {
        ""
    };

    format!(
        "// Generated by the Dazzle compiler. Do not edit.\n\
         //\n\
         // One per invocation: a failing test panics, and a panic aborts, so running them\n\
         // in one process would lose every result after the first failure. The driver\n\
         // lists them and then runs each by name.\n\
         #include <cstdio>\n#include <cstring>\n#include <cstdint>\n#include <string>\n\
         #include \"dz_runtime.h\"\n#include \"dz_native.h\"\n{includes}\n{timing}\n\
         int main(int argc, char** argv) {{\n\
         \x20   ::dz::native::process_start(argc, argv);\n\
         \x20   if (argc == 2 && std::strcmp(argv[1], \"--list\") == 0) {{\n\
         {list}\
         \x20       return 0;\n\
         \x20   }}\n\
         \x20   if (argc != 2) {{\n\
         \x20       std::fputs(\"usage: <binary> --list | <name>\\n\", stderr);\n\
         \x20       return 2;\n\
         \x20   }}\n\
         \x20   const std::string which(argv[1]);\n\
         {dispatch}\
         \x20   std::fprintf(stderr, \"nothing named %s\\n\", argv[1]);\n\
         \x20   return 2;\n\
         }}\n"
    )
}

// ---------------------------------------------------------------------------

/// A call's arguments, rendered but not yet written out (spec §7.2, 18.49).
#[derive(Default)]
struct CapturedArgs {
    args: Vec<CapturedArg>,
    /// C++ already evaluates the values in order where they are used (a braced
    /// initialiser), so only hoisted statements can put one out of turn.
    sequenced: bool,
}

struct CapturedArg {
    id: ExprId,
    /// Statements the argument needs before the line it is used on.
    pre: String,
    value: String,
    effects: bool,
    /// The same value wherever among the arguments it is evaluated.
    stable: bool,
    /// A place the callee works on — `construct`'s destination, an indexed container —
    /// rather than a value, so a temporary must refer to it, not copy it.
    place: bool,
}

impl CapturedArgs {
    fn any_effects(&self) -> bool {
        self.args.iter().any(|a| a.effects)
    }
}

/// Where a construct that is a statement in C++ should leave the value it produces.
#[derive(Clone, Copy)]
enum Dest<'d> {
    /// Statement position: the value is discarded.
    None,
    /// Assign to this variable.
    Var(&'d str),
    /// Assign the *address* to this pointer variable. Used by `^T or …`, whose success
    /// type is a `&T`: a reference cannot be reseated, so the temporary is a pointer.
    Ptr(&'d str),
}

struct Emitter<'a> {
    ws: &'a Workspace,
    res: &'a Resolution,
    table: &'a TypeTable,
    hir: &'a Hir,
    map: &'a SourceMap,
    module: ModuleId,
    file: FileId,
    diags: Diagnostics,
    header: String,
    out: String,
    indent: usize,
    temp: u32,
    current_fn: String,
    last_line: u32,
    /// The C++ standard being targeted, as a year: 17, 20, 23.
    standard: u32,
    /// The body being emitted. Every expression method reads types out of it.
    body: Option<&'a hir::Body>,
    /// One C++ name per local, by [`LocalId`]. Dazzle allows a `let` to shadow another in
    /// the same block and C++ does not, so the names are made unique once per body.
    names: Vec<String>,
    /// Which instantiations each module emits, with a concrete copy of every body
    /// (spec §8.4).
    mono: &'a dz_mono::Mono,
    /// Every struct's declaration by symbol, so an instance of a generic declared in
    /// another module can be emitted from its AST.
    struct_asts: &'a HashMap<SymbolId, (FileId, &'a StructDecl)>,
    /// The same for unions, whose instances are emitted the same way.
    union_asts: &'a HashMap<SymbolId, (FileId, &'a UnionDecl)>,
    /// The instance being emitted, if any. Its concrete bodies stand in for the template's
    /// in [`Emitter::find_body`], and its arguments are what its own fields are read with.
    instance: Option<&'a dz_mono::Instance>,
    /// The generic implementation being emitted, whose concrete bodies replace the
    /// templates' (spec §7.6, §8.4).
    impl_instance: Option<&'a dz_mono::ImplInstance>,
    /// The generic method instance being emitted: its body replaces the template's, and its
    /// receiver is a parameter named `self` rather than `*this` (spec §8.1, §8.4).
    method_instance: Option<&'a dz_mono::MethodInstance>,
    /// Finished header text, by the namespace it belongs in.
    chunks: Vec<(ModuleId, String)>,
    /// The namespace `header` is currently being written for.
    header_ns: ModuleId,
    /// `"inline "` while an instance's out-of-class definitions are written into a header,
    /// where they may be included by several translation units.
    def_prefix: &'static str,
    /// Program or tests (spec §19 M14). A `#[test]` function is emitted in one and left
    /// out of the other, so a program never carries its tests.
    mode: Mode,
    /// The types reflected on, shared by every module's emitter (spec §13).
    reflect: &'a reflect::Shared,
}

impl<'a> Emitter<'a> {
    // --- plumbing ----------------------------------------------------------

    fn unsupported(&mut self, span: Span, what: &str, owner: &str) {
        self.diags.push(
            Diagnostic::error(codes::CODEGEN_UNSUPPORTED, format!("{what} is not generated yet"))
                .at(self.file, span, "no C++ is emitted for this")
                .with_help(format!("{owner} — see the deferral register in docs/PROGRESS.md")),
        );
    }

    /// The body being emitted. Its lifetime is the arena's, not `self`'s, so reading a
    /// node does not borrow the emitter.
    fn cur(&self) -> &'a hir::Body {
        self.body.expect("no body is being emitted")
    }

    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    /// Emit a `#line` directive so this statement maps back to `.dz` (spec §16.3).
    fn line_directive(&mut self, span: Span) {
        let loc = self.map.get(self.file).location(span.start as usize);
        if loc.line == self.last_line {
            return;
        }
        self.last_line = loc.line;
        let path = self.map.get(self.file).name();
        self.out.push_str(&format!("#line {} \"{}\"\n", loc.line, path.replace('\\', "/")));
    }

    /// A `dz::SourceLocation` literal for the runtime helpers.
    fn source_location(&self, span: Span) -> String {
        let f = self.map.get(self.file);
        let loc = f.location(span.start as usize);
        format!(
            "::dz::SourceLocation{{\"{}\", \"{}\", {}, {}}}",
            f.name().replace('\\', "/"),
            self.current_fn,
            loc.line,
            loc.column
        )
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.temp += 1;
        format!("_dz_{prefix}{}", self.temp)
    }

    /// Run `f` with the output diverted, returning what it wrote and what it returned.
    ///
    /// Used where hoisted statements cannot simply precede the line being built: a `while`
    /// condition is re-evaluated every iteration, so its hoisted work has to go *inside*
    /// the loop rather than before it.
    fn capture<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> (String, R) {
        let saved = std::mem::take(&mut self.out);
        let saved_indent = std::mem::replace(&mut self.indent, 0);
        let value = f(self);
        self.indent = saved_indent;
        let written = std::mem::replace(&mut self.out, saved);
        (written, value)
    }

    /// A value whose hoisted statements have nowhere to go before it: a constant, a field
    /// default, a default argument. C++ allows only an expression in each, so the work
    /// becomes a lambda called on the spot — `[]() { …; return value; }()` — which is an
    /// expression and runs exactly where the value is needed.
    fn immediately_invoked(pre: &str, value: &str) -> String {
        let mut s = "[]() {\n".to_owned();
        for l in pre.lines() {
            // A `#line` directive must start its line.
            if !l.starts_with("#line ") {
                s.push_str("    ");
            }
            s.push_str(l);
            s.push('\n');
        }
        let _ = writeln!(s, "    return {value};");
        s.push_str("}()");
        s
    }

    /// Write a block captured by [`Emitter::capture`] at the current indentation.
    fn write_captured(&mut self, text: &str) {
        for l in text.lines() {
            if l.starts_with("#line ") {
                self.out.push_str(l);
                self.out.push('\n');
            } else {
                self.line(l);
            }
        }
    }

    // --- module ------------------------------------------------------------

    fn module(&mut self, files: &[ParsedFile]) {
        self.reject_trait_objects();
        let mut own_types: Vec<(FileId, &Item, SymbolId)> = Vec::new();
        let mut functions: Vec<(FileId, &Item)> = Vec::new();
        for pf in files {
            for item in &pf.ast.items {
                match &item.kind {
                    // Removed by the driver before this phase (spec §12.10).
                    ItemKind::CfgIf(_) => {}
                    // A generic declaration is a template: only its instances are emitted,
                    // each under its own name (spec §8.4).
                    ItemKind::Struct(s) if !s.generics.is_empty() => {}
                    ItemKind::Union(u) if !u.generics.is_empty() => {}
                    ItemKind::Struct(_) | ItemKind::Enum(_) | ItemKind::Union(_) => {
                        if let Some(sym) = self.symbol_of(item) {
                            own_types.push((pf.file, item, sym));
                        }
                    }
                    // A `#[test]` or `#[bench]` belongs to its own binary and to nothing
                    // else: a program must not carry them (spec §19 M14, M15).
                    ItemKind::Fn(_) if self.excluded(item) => {}
                    ItemKind::Fn(_) | ItemKind::Const(_) | ItemKind::Static(_) => {
                        functions.push((pf.file, item))
                    }
                    // Implementations are emitted from the checker's table, below.
                    ItemKind::Trait(_)
                    | ItemKind::Impl(_)
                    | ItemKind::Macro(_)
                    | ItemKind::MacroInvocation { .. }
                    | ItemKind::Error => {}
                }
            }
        }

        // Types are declared before anything stores them by value — the module's own and
        // its instances in one order, since either may hold the other.
        let mono = self.mono;
        let instances: Vec<&'a dz_mono::Instance> =
            mono.module(self.module).map(|m| m.types.iter().collect()).unwrap_or_default();
        let mut nodes: Vec<(SymbolId, Vec<Ty>)> =
            own_types.iter().map(|(_, _, s)| (*s, Vec::new())).collect();
        nodes.extend(instances.iter().map(|i| (i.sym, i.args.clone())));
        self.forward_decls(&own_types, &instances);
        // Enums store nothing and their values are folded integers, so they can come first.
        // Then every constant a struct could need — a field default names one, a `switch`
        // in a constant's own value reads an enum constant — which is any constant whose
        // type does not itself need a struct or union to be complete. In the order they
        // use each other, since Dazzle has no declaration order (spec §3).
        for &(file, item, sym) in &own_types {
            if let ItemKind::Enum(e) = &item.kind {
                self.file = file;
                self.enum_decl(e, sym);
            }
        }
        // Trait objects are handles a struct may hold by value, so they come before types.
        let traits: Vec<SymbolId> = files
            .iter()
            .flat_map(|pf| pf.ast.items.iter())
            .filter(|item| matches!(item.kind, ItemKind::Trait(_)))
            .filter_map(|item| match &item.kind {
                ItemKind::Trait(t) => self.res.lookup(self.module, &t.name.name),
                _ => None,
            })
            .filter(|t| self.object_safe(*t))
            .collect();
        self.trait_object_decls(&traits);
        let consts: Vec<(FileId, SymbolId)> = functions
            .iter()
            .filter(|(_, item)| matches!(item.kind, ItemKind::Const(_)))
            .filter_map(|(file, item)| Some((*file, self.symbol_of(item)?)))
            .collect();
        let (early, late): (Vec<_>, Vec<_>) =
            self.order_consts(consts).into_iter().partition(|(_, sym)| self.const_is_early(*sym));
        let any_early = !early.is_empty();
        for (file, sym) in early {
            self.file = file;
            self.const_decl(sym);
        }
        if any_early {
            self.header.push('\n');
        }
        for i in self.sort_types(&nodes) {
            match own_types.get(i) {
                Some(&(file, item, sym)) => {
                    self.file = file;
                    match &item.kind {
                        ItemKind::Struct(s) => self.struct_decl(s, sym, &[]),
                        ItemKind::Union(u) => self.union_decl(u, sym, &[]),
                        _ => {}
                    }
                }
                None => self.instance_decl(instances[i - own_types.len()]),
            }
        }
        for (file, sym) in late {
            self.file = file;
            self.const_decl(sym);
        }
        // Statics last among the declarations: every type they hold is complete by now, and
        // their initialisers are constants, so they depend on nothing else (spec §7.7).
        for (file, item) in &functions {
            if let ItemKind::Static(_) = &item.kind {
                self.file = *file;
                let thread_local = item.attrs.iter().any(|a| a.name.name == "thread_local");
                if let Some(sym) = self.symbol_of(item) {
                    self.static_decl(sym, thread_local);
                }
            }
        }
        for (file, item) in &functions {
            self.file = *file;
            let Some(sym) = self.symbol_of(item) else { continue };
            match &item.kind {
                // A template; its instances are emitted below.
                ItemKind::Fn(f) if !f.generics.is_empty() => {}
                // The checker allows a module-level function without a body only as `std`'s
                // `extern fn` (spec §7.2).
                ItemKind::Fn(f) if f.is_extern => self.extern_fn(sym, &f.name.name),
                ItemKind::Fn(f) => self.free_fn(sym, &f.name.name),
                _ => {}
            }
        }
        self.trait_object_bodies(&traits);
        self.impl_prototypes();
        self.impl_vtables();
        // Member bodies come last so they can call anything in the module.
        for &(file, item, sym) in &own_types {
            self.file = file;
            if let ItemKind::Struct(s) = &item.kind {
                self.struct_bodies(s, sym, &[]);
            }
        }
        self.impl_definitions();
        self.instance_definitions();
    }

    /// Can this constant be defined before the module's structs and unions? Only if its type
    /// needs none of them complete: a primitive, a string, an enum, or a pointer.
    fn const_is_early(&self, sym: SymbolId) -> bool {
        fn early(table: &TypeTable, ty: &Ty) -> bool {
            match ty {
                Ty::Bool
                | Ty::Char
                | Ty::Int(_)
                | Ty::Float(_)
                | Ty::Str
                | Ty::CStr
                | Ty::Ptr { .. }
                | Ty::Fn { .. } => true,
                Ty::Array { elem, .. } => early(table, elem),
                Ty::Named { sym, .. } => matches!(table.get(*sym), Some(ItemSig::Enum { .. })),
                _ => false,
            }
        }
        matches!(self.table.get(sym), Some(ItemSig::Const(t)) if early(self.table, t))
    }

    /// Constants in an order where each follows the constants its value names.
    fn order_consts(&self, consts: Vec<(FileId, SymbolId)>) -> Vec<(FileId, SymbolId)> {
        fn visit(
            e: &Emitter<'_>,
            i: usize,
            consts: &[(FileId, SymbolId)],
            state: &mut [u8],
            out: &mut Vec<(FileId, SymbolId)>,
        ) {
            // 1: on the stack. A cycle between constants was reported by the checker.
            if state[i] != 0 {
                return;
            }
            state[i] = 1;
            if let Some(body) = e.find_body(&BodyOwner::Const(consts[i].1)) {
                for x in &body.exprs {
                    if let hir::ExprKind::Const(used) = x.kind {
                        if let Some(j) = consts.iter().position(|(_, s)| *s == used) {
                            visit(e, j, consts, state, out);
                        }
                    }
                }
            }
            state[i] = 2;
            out.push(consts[i]);
        }
        let mut state = vec![0u8; consts.len()];
        let mut out = Vec::with_capacity(consts.len());
        for i in 0..consts.len() {
            visit(self, i, &consts, &mut state, &mut out);
        }
        out
    }

    /// Declare every type before defining any. `sort_types` orders definitions by what they
    /// store by value, but a type may also *mention* one declared after it — `List<T>`'s
    /// `iter()` returns a `ListIterator<T>`, which points back at the `List<T>` — and a
    /// member's signature only needs the name. Dazzle has no declaration order (spec §3),
    /// so every name is made visible up front rather than chasing the cycles.
    fn forward_decls(
        &mut self,
        own_types: &[(FileId, &Item, SymbolId)],
        instances: &[&'a dz_mono::Instance],
    ) {
        let mut any = false;
        for &(_, item, sym) in own_types {
            let line = match &item.kind {
                ItemKind::Struct(s) => format!("struct {};", self.struct_cpp_name(s, sym, &[])),
                ItemKind::Union(u) => format!("struct {};", sanitize(&u.name.name)),
                ItemKind::Enum(e) => {
                    let backing = match self.table.get(sym) {
                        Some(ItemSig::Enum { backing, .. }) => int_name(*backing),
                        _ => "int32_t",
                    };
                    format!("enum class {} : {backing};", sanitize(&e.name.name))
                }
                _ => continue,
            };
            let _ = writeln!(self.header, "{line}");
            any = true;
        }
        for &inst in instances {
            if !self.struct_asts.contains_key(&inst.sym) && !self.union_asts.contains_key(&inst.sym)
            {
                continue;
            }
            // In the generic's own namespace, where its definition will go. Repeating a
            // declaration is harmless, so unlike the definition this needs no guard.
            self.with_instance(inst, |e| {
                let name = e.instance_name(inst.sym, &inst.args);
                let _ = writeln!(e.header, "struct {name};");
            });
            any = true;
        }
        if any {
            self.header.push('\n');
        }
    }

    // --- implementations (spec §7.6) -------------------------------------------
    //
    // An implementation's methods are free functions taking the receiver first. They
    // cannot be C++ members: the type may be declared in another module's header, which
    // C++ cannot reopen, or be `float`, which has no members at all. `Drop` and `Copy` are
    // the exception — they are the struct's destructor and copy constructor, and the rule
    // that they live with the struct is what makes that possible.

    /// The module's own non-generic implementations, each with its method names: those
    /// the `impl` writes, then the trait defaults it inherits.
    fn own_impls(&self) -> Vec<(usize, Vec<String>)> {
        let mono = self.mono;
        let defaults: Vec<&hir::Body> =
            mono.module(self.module).map(|m| m.defaults.iter().collect()).unwrap_or_default();
        self.table
            .impls()
            .iter()
            .enumerate()
            .filter(|(_, imp)| {
                imp.module == self.module && imp.generics.is_empty() && !self.is_special_impl(imp)
            })
            .map(|(idx, imp)| {
                let mut names = imp.method_order.clone();
                for b in &defaults {
                    if let BodyOwner::ImplMethod { imp: i, name } = &b.owner {
                        if *i as usize == idx {
                            names.push(name.clone());
                        }
                    }
                }
                (idx, names)
            })
            .collect()
    }

    fn is_special_impl(&self, imp: &dz_check::ImplSig) -> bool {
        let t = Some(imp.trait_ref.trait_sym);
        t == self.table.prelude_trait_named("Drop") || t == self.table.prelude_trait_named("Copy")
    }

    fn impl_prototypes(&mut self) {
        let empty = HashMap::new();
        for (idx, names) in self.own_impls() {
            for name in names {
                let owner = BodyOwner::ImplMethod { imp: idx as u32, name: name.clone() };
                let fname = self.impl_fn_name(idx, &empty, &name);
                if let Some(head) = self.impl_fn_head(&owner, &fname, true) {
                    let _ = writeln!(self.header, "{head};");
                }
            }
        }
    }

    fn impl_definitions(&mut self) {
        let empty = HashMap::new();
        for (idx, names) in self.own_impls() {
            for name in names {
                let owner = BodyOwner::ImplMethod { imp: idx as u32, name: name.clone() };
                let fname = self.impl_fn_name(idx, &empty, &name);
                let Some(body) = self.find_body(&owner) else { continue };
                self.file = body.file;
                let Some(head) = self.impl_fn_head(&owner, &fname, false) else { continue };
                self.current_fn = fname;
                self.emit_body(&owner, &format!("{}{head} {{", self.def_prefix));
            }
        }
    }

    /// `Mul_f32__generics_Vec2__mul`: the trait with its arguments, the type, the method —
    /// readable, since it is what a debugger and a profiler show (spec §16.2).
    fn impl_fn_name(&self, idx: usize, map: &HashMap<u32, Ty>, method: &str) -> String {
        let imp = &self.table.impls()[idx];
        let self_ty = dz_hir::ty::substitute(&imp.self_ty, map);
        let trait_name = &self.res.symbol(imp.trait_ref.trait_sym).name;
        let args: Vec<String> = imp
            .trait_ref
            .args
            .iter()
            .map(|a| self.mangle(&dz_hir::ty::substitute_with(a, map, Some(&self_ty))))
            .collect();
        let tr = if args.is_empty() {
            trait_name.clone()
        } else {
            format!("{trait_name}_{}", args.join("_"))
        };
        format!("{tr}__{}__{method}", self.mangle(&self_ty))
    }

    /// An implementation's function, qualified relative to the namespace being written.
    fn qualify_impl_fn(&self, idx: usize, map: &HashMap<u32, Ty>, method: &str) -> String {
        let name = self.impl_fn_name(idx, map, method);
        let home = self.table.impls()[idx].module;
        if home == self.module {
            name
        } else {
            format!("{}::{name}", qualified_namespace(&self.ws.module(home).name))
        }
    }

    /// `Ret name(const T& self, params…)`: the receiver is an ordinary first parameter,
    /// `const` unless the method takes `&mut self`.
    fn impl_fn_head(
        &mut self,
        owner: &BodyOwner,
        name: &str,
        with_defaults: bool,
    ) -> Option<String> {
        let body = self.find_body(owner)?;
        let recv = body.params.first().map(|p| body.local(*p))?;
        let recv_ty = self.cpp_ty(&recv.ty);
        let cst = if recv.mutable { "" } else { "const " };
        let ret = self.body_ret_ty(owner);
        let rest = self.decl_params(owner, with_defaults);
        // A member function's `this` is implicit and never "unused"; this parameter is not,
        // and a method that ignores its receiver must not warn (invariant 7).
        let recv = format!("[[maybe_unused]] {cst}{recv_ty}& self");
        let params = if rest.is_empty() { recv } else { format!("{recv}, {rest}") };
        Some(format!("{ret} {name}({params})"))
    }

    /// The function a trait method call reaches (spec §7.6), or `None` for a type's own
    /// method, which is a C++ member. Decided here rather than by the checker because a
    /// call written against a bound (`T: Mul<f32>`) has a concrete receiver only once it is
    /// an instance — and this body is one.
    fn impl_target(
        &self,
        recv: ExprId,
        owner: SymbolId,
        name: &str,
        args: &[ExprId],
    ) -> Option<String> {
        if self.res.symbol(owner).kind != dz_resolve::SymbolKind::Trait {
            return None;
        }
        let body = self.cur();
        let recv_ty = body.ty(recv).peeled().clone();
        let arg = args.first().map(|a| body.ty(*a).clone());
        let (idx, map) = self.table.resolve_impl(owner, &recv_ty, name, arg.as_ref())?;
        Some(self.qualify_impl_fn(idx, &map, name))
    }

    fn with_impl_instance(&mut self, inst: &'a dz_mono::ImplInstance, f: impl FnOnce(&mut Self)) {
        let home = self.table.impls()[inst.imp].module;
        let saved = (self.module, self.impl_instance, self.file);
        self.switch_ns(home);
        self.module = home;
        self.impl_instance = Some(inst);
        self.file = self.table.impls()[inst.imp].file;
        f(self);
        (self.module, self.impl_instance, self.file) = saved;
        self.switch_ns(saved.0);
    }

    /// A generic implementation's functions for one set of arguments: prototypes, then
    /// `inline` definitions, each behind its own include guard like any instance (§8.4).
    fn impl_instance_fns(&mut self, inst: &dz_mono::ImplInstance, bodies: bool) {
        let imp = &self.table.impls()[inst.imp];
        let map: HashMap<u32, Ty> =
            imp.generics.iter().map(|g| g.param.id).zip(inst.args.iter().cloned()).collect();
        let stem = file_stem(&self.ws.module(imp.module).name);
        let names: Vec<String> = inst
            .bodies
            .iter()
            .filter_map(|b| match &b.owner {
                BodyOwner::ImplMethod { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        for name in names {
            let owner = BodyOwner::ImplMethod { imp: inst.imp as u32, name: name.clone() };
            let fname = self.impl_fn_name(inst.imp, &map, &name);
            if !bodies {
                let Some(head) = self.impl_fn_head(&owner, &fname, true) else { continue };
                let guard = format!("DZ_INST_{stem}_{fname}_DECL");
                let _ = writeln!(
                    self.header,
                    "#ifndef {guard}\n#define {guard}\n{head};\n#endif  // {guard}\n"
                );
                continue;
            }
            let Some(head) = self.impl_fn_head(&owner, &fname, false) else { continue };
            self.current_fn = fname.clone();
            let (text, ()) = self.capture(|e| e.emit_body(&owner, &format!("inline {head} {{")));
            let guard = format!("DZ_INST_{stem}_{fname}_DEF");
            let _ = writeln!(self.header, "#ifndef {guard}\n#define {guard}");
            self.header.push_str(&text);
            let _ = writeln!(self.header, "#endif  // {guard}\n");
        }
    }

    // --- trait objects (spec §7.6, §18.6) ---------------------------------------

    /// Can a `^Trait` of this trait exist? Its methods must be callable knowing nothing
    /// but the vtable: no parameters of the trait's own or a method's, no associated types,
    /// every method taking a receiver, and `Self` nowhere but the receiver.
    fn object_safe(&self, sym: SymbolId) -> bool {
        if !self.table.type_params(sym).is_empty() {
            return false;
        }
        let Some(ItemSig::Trait(t)) = self.table.get(sym) else { return false };
        t.assoc_types.is_empty()
            && t.methods.values().all(|m| {
                m.takes_self.is_some()
                    && m.generics.is_empty()
                    && !mentions_self(&m.ret)
                    && !m.params.iter().any(|p| mentions_self(&p.ty))
            })
    }

    /// The trait `inner` names, when a pointer or reference to it is a trait object.
    fn trait_object(&self, inner: &Ty) -> Option<SymbolId> {
        match inner {
            Ty::Named { sym, args }
                if args.is_empty()
                    && self.res.symbol(*sym).kind == dz_resolve::SymbolKind::Trait =>
            {
                Some(*sym)
            }
            _ => None,
        }
    }

    /// `Shape__dyn`, qualified from where it is written.
    fn dyn_name(&self, sym: SymbolId) -> String {
        format!("{}__dyn", self.qualify(sym))
    }

    /// A trait's methods in declaration order, with the name each is called by.
    fn trait_methods(&self, sym: SymbolId) -> Vec<(String, dz_check::FnSig)> {
        let Some(ItemSig::Trait(t)) = self.table.get(sym) else { return Vec::new() };
        t.method_keys
            .iter()
            .filter_map(|k| {
                let sig = t.methods.get(k)?.clone();
                let name = t
                    .overloads
                    .iter()
                    .find(|(_, keys)| keys.contains(k))
                    .map_or_else(|| k.clone(), |(n, _)| n.clone());
                Some((name, sig))
            })
            .collect()
    }

    /// The C++ return type and parameter list of a trait method as the vtable calls it.
    fn dyn_signature(&mut self, sig: &dz_check::FnSig) -> (String, Vec<String>) {
        let ret = self.cpp_ty(&sig.ret);
        let ret = if sig.fallible { format!("::dz::Res<{ret}>") } else { ret };
        let params = sig.params.iter().map(|p| self.cpp_ty(&p.ty)).collect();
        (ret, params)
    }

    /// The vtable and handle of each object-safe trait this module declares, before any
    /// type that might hold a handle by value. Members are declared here and defined by
    /// [`Emitter::trait_object_bodies`], once every type they mention is complete.
    fn trait_object_decls(&mut self, traits: &[SymbolId]) {
        for &sym in traits {
            let name = sanitize(&self.res.symbol(sym).name);
            let methods = self.trait_methods(sym);
            let _ = writeln!(self.header, "struct {name}__vtable {{");
            for (m, sig) in &methods {
                let (ret, params) = self.dyn_signature(sig);
                let recv = if sig.takes_self == Some(true) { "void*" } else { "const void*" };
                let mut all = vec![recv.to_owned()];
                all.extend(params);
                let _ = writeln!(self.header, "    {ret} (*{})({});", sanitize(m), all.join(", "));
            }
            self.header.push_str("};\n\n");
            let _ = writeln!(self.header, "struct {name}__dyn : ::dz::Dyn<{name}__vtable> {{");
            let _ = writeln!(self.header, "    using Dyn::Dyn;");
            // `&(handle)` where a handle is wanted: the same object, not a pointer to one.
            let _ = writeln!(
                self.header,
                "    {name}__dyn(const {name}__dyn* h) : Dyn(*h) {{}}\n    \
                 {name}__dyn(const {name}__dyn& h) = default;\n    \
                 {name}__dyn& operator=(const {name}__dyn& h) = default;\n    \
                 const {name}__dyn& operator*() const {{ return *this; }}"
            );
            let keys: Vec<String> = match self.table.get(sym) {
                Some(ItemSig::Trait(t)) => {
                    t.method_keys.iter().filter(|k| t.methods.contains_key(*k)).cloned().collect()
                }
                _ => Vec::new(),
            };
            for ((m, sig), key) in methods.iter().zip(keys) {
                let (ret, params) = self.dyn_signature(sig);
                // The trait's default arguments go on the declaration, as on any function:
                // `a.alloc(8)` through a handle fills in the rest, and a `#location` default
                // still records the caller (spec §7.2, §12.7).
                let owner = BodyOwner::TraitDefaults { owner: sym, name: key };
                if self.find_body(&owner).is_some() {
                    let list = self.decl_params(&owner, true);
                    let _ = writeln!(self.header, "    {ret} {}({list}) const;", sanitize(m));
                    continue;
                }
                let list: Vec<String> = params
                    .iter()
                    .zip(&sig.params)
                    .enumerate()
                    .map(|(i, (t, p))| {
                        match p.caller_default.as_deref().and_then(caller_intrinsic) {
                            Some(d) => format!("{t} a{i} = {d}"),
                            None => format!("{t} a{i}"),
                        }
                    })
                    .collect();
                let _ =
                    writeln!(self.header, "    {ret} {}({}) const;", sanitize(m), list.join(", "));
            }
            self.header.push_str("};\n\n");
        }
    }

    fn trait_object_bodies(&mut self, traits: &[SymbolId]) {
        for &sym in traits {
            let name = sanitize(&self.res.symbol(sym).name);
            for (m, sig) in self.trait_methods(sym) {
                let (ret, params) = self.dyn_signature(&sig);
                let list: Vec<String> =
                    params.iter().enumerate().map(|(i, t)| format!("{t} a{i}")).collect();
                let mut args = vec!["data".to_owned()];
                args.extend((0..params.len()).map(|i| format!("a{i}")));
                let m = sanitize(&m);
                let _ = writeln!(
                    self.header,
                    "inline {ret} {name}__dyn::{m}({}) const {{ return vt->{m}({}); }}",
                    list.join(", "),
                    args.join(", ")
                );
            }
        }
    }

    /// Each implementation of an object-safe trait gives its vtable, found by the handle
    /// through argument-dependent lookup (spec §7.6). `+[]` makes each entry a plain
    /// function pointer; it calls the implementation's own function with the receiver cast
    /// back to the type it is.
    fn impl_vtables(&mut self) {
        let empty = HashMap::new();
        for (idx, _) in self.own_impls() {
            let imp = self.table.impls()[idx].clone();
            let tsym = imp.trait_ref.trait_sym;
            if !self.object_safe(tsym) || imp.self_ty.has_params() {
                continue;
            }
            let vt = format!("{}__vtable", self.qualify(tsym));
            let self_cpp = self.cpp_ty(&imp.self_ty);
            let mut entries = Vec::new();
            for (m, sig) in self.trait_methods(tsym) {
                let (ret, params) = self.dyn_signature(&sig);
                let mutable = sig.takes_self == Some(true);
                let (recv, cast) = if mutable {
                    ("void* s".to_owned(), format!("*static_cast<{self_cpp}*>(s)"))
                } else {
                    ("const void* s".to_owned(), format!("*static_cast<const {self_cpp}*>(s)"))
                };
                let mut list = vec![recv];
                list.extend(params.iter().enumerate().map(|(i, t)| format!("{t} a{i}")));
                let mut args = vec![cast];
                args.extend((0..params.len()).map(|i| format!("a{i}")));
                let f = self.qualify_impl_fn(idx, &empty, &m);
                entries.push(format!(
                    "        +[]({}) -> {ret} {{ return {f}({}); }},",
                    list.join(", "),
                    args.join(", ")
                ));
            }
            let _ = writeln!(
                self.header,
                "inline const {vt}* dz_vtable(const {vt}*, const {self_cpp}*) {{\n    \
                 static const {vt} v{{\n{}\n    }};\n    return &v;\n}}\n",
                entries.join("\n")
            );
        }
    }

    /// A trait object of a trait that cannot have one — generic, with associated types, or
    /// with a method that mentions `Self` — is refused rather than reaching C++ as a type
    /// that was never declared (invariant 7). Reported once per body or type.
    fn reject_trait_objects(&mut self) {
        let mut found: Vec<(FileId, Span)> = Vec::new();
        for (_, b) in self.hir.bodies() {
            if dz_mono::owner_module(self.res, self.table, &b.owner) != self.module {
                continue;
            }
            // A trait's own default body reads `self` — and `Self` — as the trait itself,
            // which is not a trait object: implementers get it rewritten to their own type.
            let own_trait = dz_mono::owner_sym(&b.owner)
                .filter(|o| self.res.symbol(*o).kind == dz_resolve::SymbolKind::Trait);
            let is_object = |t: &Ty| self.mentions_trait_other_than(t, own_trait);
            // Parameters are locals, so a signature is covered as well as a body.
            let at = b
                .locals
                .iter()
                .find(|l| is_object(&l.ty))
                .map(|l| l.span)
                .or_else(|| is_object(&b.ret).then_some(b.span));
            if let Some(span) = at {
                found.push((b.file, span));
            }
        }
        for (i, s) in self.res.symbols.iter().enumerate() {
            let sym = SymbolId(i as u32);
            if s.module != self.module {
                continue;
            }
            let fields: Vec<&dz_check::FieldSig> = match self.table.get(sym) {
                Some(ItemSig::Struct(sig)) => sig.fields.iter().collect(),
                Some(ItemSig::Union { variants }) => {
                    variants.iter().flat_map(|v| v.fields.iter()).collect()
                }
                _ => Vec::new(),
            };
            if let Some(f) = fields.iter().find(|f| self.mentions_trait(&f.ty)) {
                found.push((s.file, f.span));
            }
        }
        for (file, span) in found {
            self.file = file;
            self.unsupported(
                span,
                "a trait object (`^Trait`) of a generic trait, one with associated types, or one                  whose methods mention `Self`",
                "owed by M12",
            );
        }
    }

    fn mentions_trait(&self, t: &Ty) -> bool {
        self.mentions_trait_other_than(t, None)
    }

    /// Does `t` name a trait as a type anywhere inside it, other than `except`?
    fn mentions_trait_other_than(&self, t: &Ty, except: Option<SymbolId>) -> bool {
        let m = |x: &Ty| self.mentions_trait_other_than(x, except);
        match t {
            Ty::Named { sym, args } => {
                (self.res.symbol(*sym).kind == dz_resolve::SymbolKind::Trait
                    && Some(*sym) != except
                    && !self.object_safe(*sym))
                    || args.iter().any(m)
            }
            Ty::Optional(i) | Ty::Fallible(i) | Ty::Range(i) => m(i),
            Ty::Ref { inner, .. } | Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => m(inner),
            Ty::Array { elem, .. } | Ty::Slice { elem, .. } => m(elem),
            Ty::Tuple(parts) => parts.iter().any(m),
            Ty::Fn { params, ret } => params.iter().any(m) || m(ret),
            _ => false,
        }
    }

    // --- instances (spec §8.4) ---------------------------------------------

    /// Run `f` with an instance's code being emitted: in its generic's own namespace, with
    /// names qualified relative to that namespace, and with its concrete bodies in place of
    /// the template's.
    fn with_instance(&mut self, inst: &'a dz_mono::Instance, f: impl FnOnce(&mut Self)) {
        let home = self.res.symbol(inst.sym).module;
        let saved = (self.module, self.instance, self.file);
        self.switch_ns(home);
        self.module = home;
        self.instance = Some(inst);
        self.file = self.res.symbol(inst.sym).file;
        f(self);
        (self.module, self.instance, self.file) = saved;
        self.switch_ns(saved.0);
    }

    /// An instance's struct definition, behind an include guard so several headers can
    /// each carry it and a translation unit sees it once.
    fn instance_decl(&mut self, inst: &'a dz_mono::Instance) {
        let decl = match (self.struct_asts.get(&inst.sym), self.union_asts.get(&inst.sym)) {
            (Some(&(file, s)), _) => (file, Some(s), None),
            (None, Some(&(file, u))) => (file, None, Some(u)),
            (None, None) => return,
        };
        self.with_instance(inst, |e| {
            e.file = decl.0;
            let guard = e.instance_guard(inst.sym, &inst.args, "");
            let shown = e.instance_shown(inst);
            let _ = writeln!(e.header, "// dazzle: {shown}\n#ifndef {guard}\n#define {guard}");
            match decl {
                (_, Some(s), _) => e.struct_decl(s, inst.sym, &inst.args),
                (_, _, Some(u)) => e.union_decl(u, inst.sym, &inst.args),
                _ => {}
            }
            let _ = writeln!(e.header, "#endif  // {guard}\n");
        });
    }

    /// Instance functions' prototypes, then every instance body, as `inline` definitions in
    /// the header. Prototypes first so instance bodies may call each other in any order.
    fn instance_definitions(&mut self) {
        let mono = self.mono;
        let Some(mi) = mono.module(self.module) else { return };
        for inst in &mi.impls {
            self.with_impl_instance(inst, |e| e.impl_instance_fns(inst, false));
        }
        for inst in &mi.fns {
            self.with_instance(inst, |e| e.instance_fn_prototype(inst));
        }
        for inst in &mi.methods {
            self.with_method_instance(inst, |e| e.method_instance_fn(inst, false));
        }
        for inst in &mi.fns {
            self.with_instance(inst, |e| e.instance_fn_body(inst));
        }
        for inst in &mi.types {
            self.with_instance(inst, |e| e.instance_struct_bodies(inst));
        }
        for inst in &mi.impls {
            self.with_impl_instance(inst, |e| e.impl_instance_fns(inst, true));
        }
        for inst in &mi.methods {
            self.with_method_instance(inst, |e| e.method_instance_fn(inst, true));
        }
    }

    fn with_method_instance(
        &mut self,
        inst: &'a dz_mono::MethodInstance,
        f: impl FnOnce(&mut Self),
    ) {
        let home = self.res.symbol(inst.owner).module;
        let saved = (self.module, self.method_instance, self.file);
        self.switch_ns(home);
        self.module = home;
        self.method_instance = Some(inst);
        self.file = inst.body.file;
        f(self);
        (self.module, self.method_instance, self.file) = saved;
        self.switch_ns(saved.0);
    }

    /// `Holder<i32>::pair<bool>` → `Holder__i32__pair__bool`: the type as an instance names
    /// it, the method, and the method's own arguments (spec §8.4, §16.2).
    fn method_instance_name(
        &self,
        owner: SymbolId,
        owner_args: &[Ty],
        name: &str,
        args: &[Ty],
    ) -> String {
        let ty = if owner_args.is_empty() {
            self.res.symbol(owner).name.clone()
        } else {
            self.instance_name(owner, owner_args)
        };
        let parts: Vec<String> = args.iter().map(|a| self.mangle(a)).collect();
        format!("{ty}__{name}__{}", parts.join("__"))
    }

    fn qualify_method_instance(
        &self,
        owner: SymbolId,
        owner_args: &[Ty],
        name: &str,
        args: &[Ty],
    ) -> String {
        let fname = self.method_instance_name(owner, owner_args, name, args);
        let home = self.res.symbol(owner).module;
        if home == self.module {
            fname
        } else {
            format!("{}::{fname}", qualified_namespace(&self.ws.module(home).name))
        }
    }

    /// One generic method instance as an `inline` free function, taking the receiver as its
    /// first parameter the way an implementation's method does: a C++ member cannot be added
    /// to a type per call site, and an instance is needed wherever it is called (spec §8.4).
    /// `bodies` picks the prototype or the definition, each behind its own include guard.
    fn method_instance_fn(&mut self, inst: &'a dz_mono::MethodInstance, bodies: bool) {
        let owner = BodyOwner::Method { owner: inst.owner, name: inst.name.clone() };
        let fname = self.method_instance_name(inst.owner, &inst.owner_args, &inst.name, &inst.args);
        let has_self = inst.body.params.first().is_some_and(|p| inst.body.local(*p).name == "self");
        let head = if has_self {
            let Some(head) = self.impl_fn_head(&owner, &fname, !bodies) else { return };
            head
        } else {
            let ret = self.body_ret_ty(&owner);
            let params = self.decl_params(&owner, !bodies);
            format!("{ret} {fname}({params})")
        };
        let stem = file_stem(&self.ws.module(self.res.symbol(inst.owner).module).name);
        if !bodies {
            let guard = format!("DZ_INST_{stem}_{fname}_DECL");
            let _ = writeln!(
                self.header,
                "#ifndef {guard}\n#define {guard}\n{head};\n#endif  // {guard}\n"
            );
            return;
        }
        let shown_args: Vec<String> =
            inst.args.iter().map(|a| dz_check::type_name(self.res, a)).collect();
        self.current_fn = format!(
            "{}::{}<{}>",
            self.res.symbol(inst.owner).name,
            inst.name,
            shown_args.join(", ")
        );
        let (text, ()) = self.capture(|e| e.emit_body(&owner, &format!("inline {head} {{")));
        let guard = format!("DZ_INST_{stem}_{fname}_DEF");
        let _ = writeln!(self.header, "#ifndef {guard}\n#define {guard}");
        self.header.push_str(&text);
        let _ = writeln!(self.header, "#endif  // {guard}\n");
    }

    fn instance_fn_prototype(&mut self, inst: &dz_mono::Instance) {
        let owner = BodyOwner::Fn(inst.sym);
        let name = self.instance_name(inst.sym, &inst.args);
        let ret = self.body_ret_ty(&owner);
        let decl = self.decl_params(&owner, true);
        let guard = self.instance_guard(inst.sym, &inst.args, "_DECL");
        let shown = self.instance_shown(inst);
        let _ = writeln!(
            self.header,
            "// dazzle: {shown}\n#ifndef {guard}\n#define {guard}\n{ret} {name}({decl});\n\
             #endif  // {guard}\n"
        );
    }

    fn instance_fn_body(&mut self, inst: &dz_mono::Instance) {
        let owner = BodyOwner::Fn(inst.sym);
        let name = self.instance_name(inst.sym, &inst.args);
        let ret = self.body_ret_ty(&owner);
        let params = self.decl_params(&owner, false);
        self.current_fn = self.instance_shown(inst);
        let head = format!("inline {ret} {name}({params}) {{");
        let (text, ()) = self.capture(|e| e.emit_body(&owner, &head));
        let guard = self.instance_guard(inst.sym, &inst.args, "_DEF");
        let _ = writeln!(self.header, "#ifndef {guard}\n#define {guard}");
        self.header.push_str(&text);
        let _ = writeln!(self.header, "#endif  // {guard}\n");
    }

    fn instance_struct_bodies(&mut self, inst: &dz_mono::Instance) {
        let Some(&(file, s)) = self.struct_asts.get(&inst.sym) else { return };
        self.file = file;
        self.def_prefix = "inline ";
        let (text, ()) = self.capture(|e| e.struct_bodies(s, inst.sym, &inst.args));
        self.def_prefix = "";
        if text.trim().is_empty() {
            return;
        }
        let guard = self.instance_guard(inst.sym, &inst.args, "_DEF");
        let _ = writeln!(self.header, "#ifndef {guard}\n#define {guard}");
        self.header.push_str(&text);
        let _ = writeln!(self.header, "#endif  // {guard}\n");
    }

    /// Direct header text to `m`'s namespace from here on.
    fn switch_ns(&mut self, m: ModuleId) {
        if m == self.header_ns {
            return;
        }
        let text = std::mem::take(&mut self.header);
        if !text.is_empty() {
            self.chunks.push((self.header_ns, text));
        }
        self.header_ns = m;
    }

    /// Close off the last namespace block. A module with no declarations at all still gets
    /// its own (empty) block, which keeps a trivial header's shape unchanged.
    fn finish_header(&mut self) {
        let text = std::mem::take(&mut self.header);
        if !text.is_empty() || self.chunks.is_empty() {
            self.chunks.push((self.header_ns, text));
        }
    }

    /// `Buffer<f32>` → `Buffer__f32`: the declaration's own name and its arguments, each
    /// spelled as part of an identifier. Readable on purpose — this is what a user sees in a
    /// debugger and a profiler (spec §8.6).
    fn instance_name(&self, sym: SymbolId, args: &[Ty]) -> String {
        let parts: Vec<String> = args.iter().map(|a| self.mangle(a)).collect();
        format!("{}__{}", self.res.symbol(sym).name, parts.join("__"))
    }

    /// An instance's name qualified relative to the namespace being written.
    fn qualify_instance(&self, sym: SymbolId, args: &[Ty]) -> String {
        let name = self.instance_name(sym, args);
        let home = self.res.symbol(sym).module;
        if home == self.module {
            name
        } else {
            format!("{}::{name}", qualified_namespace(&self.ws.module(home).name))
        }
    }

    fn instance_guard(&self, sym: SymbolId, args: &[Ty], suffix: &str) -> String {
        let home = self.res.symbol(sym).module;
        let stem = file_stem(&self.ws.module(home).name);
        format!("DZ_INST_{stem}_{}{suffix}", self.instance_name(sym, args))
    }

    /// `Buffer<f32>`, as written, for the `// dazzle:` comment and `#function`.
    fn instance_shown(&self, inst: &dz_mono::Instance) -> String {
        let args: Vec<String> =
            inst.args.iter().map(|a| dz_check::type_name(self.res, a)).collect();
        format!("{}<{}>", self.res.symbol(inst.sym).name, args.join(", "))
    }

    /// One type argument spelled as part of an identifier. A named type carries its module,
    /// as §8.4 writes it (`Buffer__engine_Entity`), and a nested argument list is bracketed
    /// with `_L_`…`_R` so `Pair<Buffer<f32>, i32>` stays distinct from its neighbours.
    fn mangle(&self, t: &Ty) -> String {
        let m = |t: &Ty| self.mangle(t);
        match t {
            Ty::Int(i) => i.name().to_owned(),
            Ty::Float(f) => f.name().to_owned(),
            Ty::Bool => "bool".to_owned(),
            Ty::Char => "char".to_owned(),
            Ty::Str => "str".to_owned(),
            Ty::CStr => "cstr".to_owned(),
            Ty::Void => "void".to_owned(),
            Ty::Ptr { mutable, inner } => {
                format!("{}_{}", if *mutable { "pm" } else { "p" }, m(inner))
            }
            Ty::OwnPtr { mutable, inner } => {
                format!("{}_{}", if *mutable { "opm" } else { "op" }, m(inner))
            }
            Ty::Ref { mutable, inner } => {
                format!("{}_{}", if *mutable { "rm" } else { "r" }, m(inner))
            }
            Ty::Optional(i) => format!("opt_{}", m(i)),
            Ty::Array { len, elem } => format!("a{len}_{}", m(elem)),
            Ty::Slice { mutable, elem } => {
                format!("{}_{}", if *mutable { "sm" } else { "s" }, m(elem))
            }
            Ty::Tuple(parts) => {
                let ps: Vec<String> = parts.iter().map(m).collect();
                format!("t{}_{}", parts.len(), ps.join("_"))
            }
            Ty::Named { sym, args } => {
                let s = self.res.symbol(*sym);
                // An imported type is named by its C++ path, which is unique across the
                // header however its namespaces are arranged (spec §15.2).
                let head = if let Some(cpp) = self.res.cpp_path(*sym) {
                    cpp.trim_start_matches("::").replace("::", "_")
                } else {
                    format!("{}_{}", file_stem(&self.ws.module(s.module).name), s.name)
                };
                if args.is_empty() {
                    head
                } else {
                    let ps: Vec<String> = args.iter().map(m).collect();
                    format!("{head}_L_{}_R", ps.join("_"))
                }
            }
            Ty::Builtin(b) => b.name().to_owned(),
            // Only reached for a declared signature, never for an instance's arguments.
            Ty::Param(p) => p.name.to_string(),
            _ => "x".to_owned(),
        }
    }

    /// Order type declarations so that everything a type stores **by value** is declared
    /// before it, across the module's own types and its instances alike. Pointers and
    /// references do not constrain the order; a forward declaration would do.
    fn sort_types(&self, nodes: &[(SymbolId, Vec<Ty>)]) -> Vec<usize> {
        let deps: Vec<Vec<usize>> = nodes
            .iter()
            .map(|(sym, args)| {
                let fields: Vec<Ty> = match self.table.get(*sym) {
                    Some(ItemSig::Struct(s)) => self.table.field_types(*sym, args, &s.fields),
                    // A variant carrying a struct holds it by value (spec §7.5, 18.45).
                    Some(ItemSig::Union { variants }) => variants
                        .iter()
                        .flat_map(|v| {
                            let mut types = self.table.field_types(*sym, args, &v.fields);
                            types.extend(v.carries.map(Ty::plain));
                            types
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                let mut stored = Vec::new();
                for t in &fields {
                    by_value_named(t, &mut stored);
                }
                // A base is a subobject, so C++ needs it complete before the derived type
                // is defined — exactly as a field held by value does (spec §7.3, 18.32).
                if let Some(ItemSig::Struct(s)) = self.table.get(*sym) {
                    if let Some(base) = s.base {
                        stored.push((base, Vec::new()));
                    }
                }
                stored
                    .iter()
                    .filter_map(|(s, a)| nodes.iter().position(|(ns, na)| ns == s && na == a))
                    .collect()
            })
            .collect();

        fn visit(i: usize, deps: &[Vec<usize>], state: &mut [u8], order: &mut Vec<usize>) {
            // A cycle means two types contain each other by value, which cannot be laid
            // out; the checker will have said so, so just stop recursing.
            if state[i] != 0 {
                return;
            }
            state[i] = 1;
            for &d in &deps[i] {
                visit(d, deps, state, order);
            }
            state[i] = 2;
            order.push(i);
        }
        let mut state = vec![0u8; nodes.len()];
        let mut order = Vec::new();
        // A struct with no fields and no base first. Nothing needs its layout, but a default
        // argument elsewhere may name one of its constants — `allocator = Allocators::Global`
        // in `String(…)` — and C++ wants the type complete there. Such types exist to hold
        // functions and constants (`Allocators`, `Random`, `Directory`), which is exactly
        // what defaults reach for; before `Path` held a `String`, declaration order hid this.
        let fieldless = |i: usize| match self.table.get(nodes[i].0) {
            Some(ItemSig::Struct(s)) => s.fields.is_empty() && s.base.is_none(),
            _ => false,
        };
        for i in (0..nodes.len()).filter(|&i| fieldless(i)) {
            visit(i, &deps, &mut state, &mut order);
        }
        for i in 0..nodes.len() {
            visit(i, &deps, &mut state, &mut order);
        }
        order
    }

    /// A body by owner, preferring the concrete copy of the instance being emitted, then a
    /// default an implementation of the module's inherits (spec §7.6).
    fn find_body(&self, owner: &BodyOwner) -> Option<&'a hir::Body> {
        if let Some(inst) = self.method_instance {
            if &inst.body.owner == owner {
                return Some(&inst.body);
            }
        }
        if let Some(inst) = self.impl_instance {
            if let Some(b) = inst.find(owner) {
                return Some(b);
            }
        }
        if let Some(inst) = self.instance {
            if let Some(b) = inst.find(owner) {
                return Some(b);
            }
        }
        let mono = self.mono;
        if let Some(b) =
            mono.module(self.module).and_then(|m| m.defaults.iter().find(|b| &b.owner == owner))
        {
            return Some(b);
        }
        self.hir.find(owner)
    }

    /// The instance's arguments when `sym` is the type being emitted, and none otherwise.
    fn args_for(&self, sym: SymbolId) -> Vec<Ty> {
        match self.instance {
            Some(i) if i.sym == sym => i.args.clone(),
            _ => Vec::new(),
        }
    }

    /// `Self` for the struct being emitted: the instance's type, or the plain declaration.
    fn self_type(&self, sym: SymbolId) -> Ty {
        Ty::Named { sym, args: self.args_for(sym) }
    }

    fn struct_cpp_name(&self, s: &StructDecl, sym: SymbolId, args: &[Ty]) -> String {
        if args.is_empty() {
            sanitize(&s.name.name)
        } else {
            self.instance_name(sym, args)
        }
    }

    fn symbol_of(&self, item: &Item) -> Option<SymbolId> {
        let name = match &item.kind {
            ItemKind::Fn(f) => &f.name.name,
            ItemKind::Struct(s) => &s.name.name,
            ItemKind::Enum(e) => &e.name.name,
            ItemKind::Union(u) => &u.name.name,
            ItemKind::Const(c) => &c.name.name,
            ItemKind::Static(s) => &s.name.name,
            _ => return None,
        };
        self.res.lookup(self.module, name)
    }

    fn const_decl(&mut self, sym: SymbolId) {
        let Some(body) = self.find_body(&BodyOwner::Const(sym)) else { return };
        let name = self.res.symbol(sym).name.clone();
        let (ty, is_str) = match self.table.get(sym) {
            Some(ItemSig::Const(t)) => (self.cpp_ty(t), matches!(t, Ty::Str)),
            _ => ("auto".to_owned(), false),
        };
        let (pre, value) = self.in_body(body, |e| {
            let root = body.block(body.root);
            match root.stmts.first().map(|s| body.stmt(*s)) {
                Some(hir::Stmt { kind: hir::StmtKind::Expr(v), .. }) => e.expr(*v),
                _ => "0".to_owned(),
            }
        });
        // `constexpr` wherever C++ can evaluate the value while compiling, which is what
        // lets an integer constant size an array. A `str` cannot be — its constructor
        // casts a `const char*` — and neither can a value built by statements: an `or`
        // clause, a block-valued `if` or `switch`. Those are `inline const`, one object
        // for the whole program all the same.
        let (keyword, value) = if pre.trim().is_empty() {
            (if is_str { "const" } else { "constexpr" }, value)
        } else {
            ("const", Self::immediately_invoked(&pre, &value))
        };
        let _ = writeln!(self.header, "inline {keyword} {ty} {} = {value};", sanitize(&name));
    }

    /// `inline T NAME = value;` — one object for the whole program however many translation
    /// units include the header, and `thread_local` gives each thread its own (spec §7.7).
    fn static_decl(&mut self, sym: SymbolId, thread_local: bool) {
        let Some(body) = self.find_body(&BodyOwner::Static(sym)) else { return };
        let name = self.res.symbol(sym).name.clone();
        let ty = match self.table.get(sym) {
            Some(ItemSig::Static(t)) => self.cpp_ty(t),
            _ => "auto".to_owned(),
        };
        let (pre, value) = self.in_body(body, |e| {
            let root = body.block(body.root);
            match root.stmts.first().map(|s| body.stmt(*s)) {
                Some(hir::Stmt { kind: hir::StmtKind::Expr(v), .. }) => e.expr(*v),
                _ => "{}".to_owned(),
            }
        });
        // The checker allows only constants, which never hoist; a lambda keeps a struct
        // literal's C++17 spelling working all the same.
        let value =
            if pre.trim().is_empty() { value } else { Self::immediately_invoked(&pre, &value) };
        // A struct literal is built in a `[&]` lambda, which C++ allows only inside a
        // function. At namespace scope there is nothing to capture — the value is made of
        // constants — so the capture-default is dropped.
        let value = value.replace("[&]", "[]");
        let name = sanitize(&name);
        if thread_local {
            // Declared in the header and defined once in the module's own file. An `inline
            // thread_local` with a destructor links twice on mingw g++, which emits its
            // per-thread initialiser in every translation unit that includes the header.
            let _ = writeln!(self.header, "extern thread_local {ty} {name};");
            let _ = writeln!(self.out, "thread_local {ty} {name} = {value};\n");
        } else {
            let _ = writeln!(self.header, "inline {ty} {name} = {value};");
        }
    }

    fn enum_decl(&mut self, e: &EnumDecl, sym: SymbolId) {
        let backing = match self.table.get(sym) {
            Some(ItemSig::Enum { backing, .. }) => int_name(*backing).to_owned(),
            _ => "int32_t".to_owned(),
        };
        let _ = writeln!(self.header, "enum class {} : {backing} {{", sanitize(&e.name.name));
        for v in &e.variants {
            match &v.value {
                // Enum values are integer constants (checked in `check_enum`), so they
                // never need the statement hoisting an ordinary expression might.
                Some(_) => {
                    let value = self.enum_value(sym, &v.name.name);
                    let _ = writeln!(self.header, "    {} = {value},", sanitize(&v.name.name));
                }
                None => {
                    let _ = writeln!(self.header, "    {},", sanitize(&v.name.name));
                }
            }
        }
        self.header.push_str("};\n\n");
    }

    /// The declared value of one enum variant, read back out of the AST-free signature.
    fn enum_value(&mut self, sym: SymbolId, variant: &str) -> String {
        match self.table.get(sym) {
            Some(ItemSig::Enum { values, variants, .. }) => variants
                .iter()
                .position(|v| v.name == variant)
                .and_then(|i| values.get(i).copied().flatten())
                .map(|v| v.to_string())
                .unwrap_or_else(|| "0".to_owned()),
            _ => "0".to_owned(),
        }
    }

    // --- structs -----------------------------------------------------------

    /// A struct's C++ definition. `args` is empty for an ordinary struct, and holds the type
    /// arguments of an instance of a generic one (spec §8.4), which is emitted from the same
    /// declaration under its mangled name with every type substituted.
    fn struct_decl(&mut self, s: &StructDecl, sym: SymbolId, args: &[Ty]) {
        let name = self.struct_cpp_name(s, sym, args);
        // `struct Button : UiElement` lowers to C++ single inheritance, which is what puts
        // the base's fields first and makes `&Derived` → `&Base` free (spec §7.3, 18.32).
        let base = match self.table.struct_sig(sym) {
            Some(sig) => sig.base,
            None => None,
        };
        let inherited = self.table.struct_sig(sym).map_or(0, |s| s.inherited);
        match base {
            Some(b) => {
                let b = self.qualify(b);
                let _ = writeln!(self.header, "struct {name} : {b} {{");
            }
            None => {
                let _ = writeln!(self.header, "struct {name} {{");
            }
        }

        // Fields, in declaration order, each with its default (spec §6.2). A field with
        // no `= expr` is value-initialised, which is what makes "every value is always
        // initialised" true without writing a default constructor.
        //
        // `field_list` is the base's fields and then this type's own, because that is the
        // one index space the HIR uses; the first `inherited` of them are the base's and
        // it declared them already, so only the rest are written here.
        let fields = self.field_list(sym);
        let declared_in = self.declaring_types(sym);
        let mut defaults: Vec<String> = Vec::new();
        for (index, (fname, fty)) in fields.iter().enumerate() {
            if index < inherited {
                // The base declared this one, with its own `= expr`. `defaults` still
                // needs it, because the move constructor resets every field it stole and
                // the inherited ones are stolen too — and flattening is base-first and
                // order-preserving, so the index means the same thing in the type that
                // declared the field as it does here.
                let owner = declared_in.get(index).copied().flatten().unwrap_or(sym);
                let d = self.field_default(owner, index).unwrap_or_else(|| "{}".to_owned());
                defaults.push(d);
                continue;
            }
            let ty = self.cpp_ty(fty);
            match self.field_default(sym, index) {
                Some(d) => {
                    let _ = writeln!(self.header, "    {ty} {fname} = {d};");
                    defaults.push(d);
                }
                None => {
                    let _ = writeln!(self.header, "    {ty} {fname}{{}};");
                    // `x = {}` is value-initialisation for anything assignable, pointers
                    // included; `T{}` is not — `int32_t*{}` does not parse.
                    defaults.push("{}".to_owned());
                }
            }
        }

        let sig = self.table.struct_sig(sym).cloned().unwrap_or_default();
        let mut ctors = 0usize;
        let mut fns = 0usize;
        for m in &s.members {
            match &m.kind {
                // `Allocators::Global` is a function evaluated where it is named, so the
                // address of a `thread_local` static is each thread's own (spec §7.3).
                MemberKind::Const(c) => {
                    let ty = sig
                        .consts
                        .iter()
                        .find(|x| x.name == c.name.name)
                        .map(|x| x.ty.clone())
                        .unwrap_or(Ty::Error);
                    let ty = self.cpp_ty(&ty);
                    let _ = writeln!(self.header, "    static {ty} {}();", sanitize(&c.name.name));
                }
                MemberKind::Ctor(_) => {
                    let owner = BodyOwner::Ctor { owner: sym, index: ctors };
                    ctors += 1;
                    let params = self.decl_params(&owner, true);
                    let _ = writeln!(self.header, "    {name}({params});");
                }
                MemberKind::Fn(f) => {
                    let key =
                        sig.method_keys.get(fns).cloned().unwrap_or_else(|| f.name.name.clone());
                    fns += 1;
                    // A method with parameters of its own is not a member: each instance is
                    // a free function of its own (`method_instance_fn`, spec §8.4).
                    if !f.generics.is_empty() {
                        continue;
                    }
                    let method = f.name.name.clone();
                    let owner = BodyOwner::Method { owner: sym, name: key };
                    let params = self.decl_params(&owner, true);
                    let ret = self.body_ret_ty(&owner);
                    // A function with no receiver is C++'s `static` member, and has no
                    // `this` to be `const` about (spec §7.3).
                    let cst = match f.self_param {
                        None => "",
                        Some(s) if s.is_mut => "",
                        Some(_) => " const",
                    };
                    let stat = if f.self_param.is_none() { "static " } else { "" };
                    // `virtual` / `override`, both spelled in the output because both were
                    // spelled in the source; a body-less `virtual` is C++'s `= 0`
                    // (spec §7.3, 18.32).
                    let (pre, post) = match f.dispatch {
                        Dispatch::Static => ("", ""),
                        Dispatch::Virtual if f.body.is_none() => ("virtual ", " = 0"),
                        Dispatch::Virtual => ("virtual ", ""),
                        Dispatch::Override => ("", " override"),
                    };
                    let _ = writeln!(
                        self.header,
                        "    {pre}{stat}{ret} {}({params}){cst}{post};",
                        sanitize(&method)
                    );
                }
                _ => {}
            }
        }

        // The special members of spec §6.6. Everything C++ needs is synthesised from the
        // at-most-three Dazzle declares, and a move-only type has its copy operations
        // deleted so that a hole in flow analysis is still caught by the C++ compiler.
        let copyable = self.table.copyable(&self.self_type(sym));
        let has_drop = sig.drop.is_some();
        // An instance whose arguments are not copyable has no `copy`, written or not
        // (spec §8.2, §18.26): its copy operations are deleted like any move-only type's.
        let has_copy = sig.copy.is_some() && copyable;
        // A type with a `virtual` method — its own or a base's — has its `drop` made
        // virtual too (spec §7.3, 18.32).
        let polymorphic = sig.polymorphic;
        let needs_special = has_drop || has_copy || !copyable || ctors > 0 || polymorphic;
        if needs_special {
            self.header.push('\n');
            // A user constructor suppresses C++'s implicit default one, and `let v: T`
            // has to keep working (spec §6.2) — unless a user constructor can already be
            // called with no arguments, which is then what `let v: T` runs, and a defaulted
            // one beside it would be a redeclaration.
            let callable_with_nothing = sig.ctors.iter().any(|c| c.required() == 0);
            if !callable_with_nothing {
                let _ = writeln!(self.header, "    {name}() = default;");
            }
        }
        // A polymorphic type with no `drop` of its own still needs the virtual destructor,
        // so that destroying a derived value through this type does the right thing. The
        // derived type's own `~T()` overrides it.
        if polymorphic && !has_drop && base.is_none() {
            let _ = writeln!(self.header, "    virtual ~{name}() = default;");
        }
        if has_drop {
            // `drop` is virtual once the type or a base has a `virtual` method, so a
            // value destroyed through a base reference runs the right one (spec §7.3).
            let v = if polymorphic { "virtual " } else { "" };
            let _ = writeln!(self.header, "    {v}~{name}();");
        }
        if has_copy {
            let _ = writeln!(self.header, "    {name}(const {name}& other);");
            let _ = writeln!(self.header, "    {name}& operator=(const {name}& other);");
        } else if !copyable {
            let _ = writeln!(self.header, "    {name}(const {name}&) = delete;");
            let _ = writeln!(self.header, "    {name}& operator=(const {name}&) = delete;");
        } else if needs_special {
            let _ = writeln!(self.header, "    {name}(const {name}&) = default;");
            let _ = writeln!(self.header, "    {name}& operator=(const {name}&) = default;");
        }
        if has_drop || !copyable {
            // A moved-from value is left at its defaults, never unspecified (§6.4), so
            // the steal is followed by a reset rather than by whatever C++ would leave.
            let _ = writeln!(self.header, "    {name}({name}&& other);");
            let _ = writeln!(self.header, "    {name}& operator=({name}&& other);");
            self.header.push_str(
                "\n    // Spec §6.4: the source of a move goes back to its defaults, so\n\
                 \x20   // running its `drop` afterwards is safe and does nothing.\n\
                 \x20   void reset_to_defaults__() {\n",
            );
            for (i, (fname, fty)) in fields.iter().enumerate() {
                // A move-only member was reset by its own move constructor, and its
                // assignment is deleted, so touching it again would not even compile.
                if !self.table.copyable(fty) && !matches!(fty, Ty::OwnPtr { .. }) {
                    continue;
                }
                let _ = writeln!(self.header, "        {fname} = {};", defaults[i]);
            }
            self.header.push_str("    }\n");
        } else if needs_special {
            let _ = writeln!(self.header, "    {name}({name}&&) = default;");
            let _ = writeln!(self.header, "    {name}& operator=({name}&&) = default;");
        }
        self.handle_upcast_ctor(sym, args, &name);

        self.header.push_str("};\n\n");
    }

    /// `Rc<Button>` → `Rc<UiElement>` (spec §14.5, 18.64): the checker allowed it, and this
    /// is the conversion, as a constructor template on the instance. It accepts another
    /// instance only when its value pointer converts *implicitly* to this one's — a derived
    /// type to its base, never the other way — and a lvalue `Box` never, since `O*` of a
    /// reference type does not exist, so a move-only box is not silently stolen from.
    fn handle_upcast_ctor(&mut self, sym: SymbolId, args: &[Ty], name: &str) {
        let symbol = self.res.symbol(sym);
        if symbol.module != self.res.prelude || args.len() != 1 {
            return;
        }
        let kind = symbol.name.clone();
        let base = self.cpp_ty(&args[0]);
        let h = &mut self.header;
        h.push_str(
            "\n    // Spec §14.5, 18.64: a handle to a derived struct is one to its base.\n",
        );
        match kind.as_str() {
            "Rc" | "Weak" => {
                let count = if kind == "Rc" { "strong" } else { "weak" };
                let _ = writeln!(
                    h,
                    "    template <class O, class = decltype(::dz::implicit_ptr<{base}>(static_cast<O*>(nullptr)->m_value))>\n    \
                     {name}(const O& o) : m_header(o.m_header), m_value(o.m_value) {{\n        \
                     if (m_header != nullptr) {{ m_header->{count} += 1; }}\n    }}"
                );
            }
            "Box" => {
                let _ = writeln!(
                    h,
                    "    template <class O, class = decltype(::dz::implicit_ptr<{base}>(static_cast<O*>(nullptr)->m_ptr))>\n    \
                     {name}(O&& o)\n        : m_ptr(o.m_ptr), m_allocator(o.m_allocator), m_size(o.m_size), m_align(o.m_align), m_drop(o.m_drop) {{\n        \
                     o.m_ptr = nullptr;\n        o.reset_to_defaults__();\n    }}"
                );
            }
            _ => {}
        }
    }

    fn struct_bodies(&mut self, s: &StructDecl, sym: SymbolId, args: &[Ty]) {
        let name = self.struct_cpp_name(s, sym, args);
        let inline = self.def_prefix;
        let mut ctors = 0usize;
        let keys = self.table.struct_sig(sym).map(|s| s.method_keys.clone()).unwrap_or_default();
        let mut fns = 0usize;
        for m in &s.members {
            match &m.kind {
                MemberKind::Const(c) => {
                    let owner = BodyOwner::AssocConst { owner: sym, name: c.name.name.clone() };
                    let Some(body) = self.find_body(&owner) else { continue };
                    let ty = self
                        .table
                        .struct_sig(sym)
                        .and_then(|st| st.consts.iter().find(|x| x.name == c.name.name))
                        .map(|x| x.ty.clone())
                        .unwrap_or(Ty::Error);
                    let cpp_ty = self.cpp_ty(&ty);
                    let (pre, value) = self.in_body(body, |e| {
                        let root = body.block(body.root);
                        match root.stmts.first().map(|st| body.stmt(*st)) {
                            Some(hir::Stmt { kind: hir::StmtKind::Expr(v), .. }) => {
                                e.stored(&ty, *v)
                            }
                            _ => "{}".to_owned(),
                        }
                    });
                    let cname = sanitize(&c.name.name);
                    self.current_fn = format!("{name}::{cname}");
                    let _ = writeln!(self.out, "{inline}{cpp_ty} {name}::{cname}() {{");
                    for l in pre.lines() {
                        let _ = writeln!(self.out, "    {l}");
                    }
                    let _ = writeln!(self.out, "    return {value};\n}}\n");
                }
                MemberKind::Ctor(_) => {
                    let owner = BodyOwner::Ctor { owner: sym, index: ctors };
                    ctors += 1;
                    let params = self.decl_params(&owner, false);
                    self.current_fn = format!("{name}::{name}");
                    // `: UiElement(w)` is a C++ member-initialiser list, which is the only
                    // place a base can be constructed (spec §7.3, 18.32).
                    let init = self.base_init(&owner);
                    let head = format!("{inline}{name}::{name}({params}){init} {{");
                    self.emit_body(&owner, &head);
                }
                MemberKind::Fn(f) => {
                    let key = keys.get(fns).cloned().unwrap_or_else(|| f.name.name.clone());
                    fns += 1;
                    if !f.generics.is_empty() {
                        continue;
                    }
                    let method = f.name.name.clone();
                    let owner = BodyOwner::Method { owner: sym, name: key };
                    // A body-less `virtual fn` is `= 0` in the declaration and has no
                    // definition at all — there is nothing to define (spec §7.3, 18.32).
                    if f.dispatch == Dispatch::Virtual && f.body.is_none() {
                        continue;
                    }
                    let params = self.decl_params(&owner, false);
                    let ret = self.body_ret_ty(&owner);
                    let cst = match f.self_param {
                        Some(s) if !s.is_mut => " const",
                        _ => "",
                    };
                    self.current_fn = format!("{name}::{method}");
                    let head =
                        format!("{inline}{ret} {name}::{}({params}){cst} {{", sanitize(&method));
                    self.emit_body(&owner, &head);
                }
                _ => {}
            }
        }
        // `impl Drop` and `impl Copy` are the destructor and the copy constructor (spec
        // §6.1, §6.6). Their bodies live with the implementation; they are emitted here,
        // as members, because that is what they are in C++.
        let sig = self.table.struct_sig(sym).cloned().unwrap_or_default();
        if sig.drop.is_some() {
            self.current_fn = format!("{name}::drop");
            let head = format!("{inline}{name}::~{name}() {{");
            self.emit_body(&BodyOwner::Drop(sym), &head);
        }
        if sig.copy.is_some() && self.table.copyable(&self.self_type(sym)) {
            // §6.6: the copy constructor's body runs on a defaults-initialised `*this`,
            // which is exactly what the member initialisers give it.
            let owner = BodyOwner::Copy(sym);
            let param = match self.find_body(&owner) {
                Some(b) => match b.params.get(1) {
                    Some(p) => local_names(b)[p.index()].clone(),
                    None => "other".to_owned(),
                },
                None => "other".to_owned(),
            };
            self.current_fn = format!("{name}::copy");
            let head = format!("{inline}{name}::{name}(const {name}& {param}) {{");
            self.emit_body(&owner, &head);
            self.copy_assignment(&name, &param);
        }
        if self.table.struct_sig(sym).is_some_and(|s| s.drop.is_some() || s.copy.is_some())
            || !self.table.copyable(&self.self_type(sym))
        {
            self.move_members(&name, sym);
        }
    }

    /// Copy assignment, lowered exactly as spec §6.6 writes it: destroy, put the object
    /// back at its defaults, then run the user's `copy`. The self-assignment guard is what
    /// keeps `x = x` from destroying `x` before reading it.
    fn copy_assignment(&mut self, name: &str, param: &str) {
        self.line(&format!(
            "{}{name}& {name}::operator=(const {name}& {param}) {{",
            self.def_prefix
        ));
        self.indent += 1;
        self.line(&format!("if (this == &{param}) {{ return *this; }}"));
        self.line(&format!("this->~{name}();"));
        self.line(&format!("new (static_cast<void*>(this)) {name}();"));
        self.line(&format!("new (static_cast<void*>(this)) {name}({param});"));
        self.line("return *this;");
        self.indent -= 1;
        self.line("}");
        self.out.push('\n');
    }

    /// The move constructor and move assignment of spec §6.6: steal each field, then put
    /// the source back at its defaults so running its `drop` is safe and does nothing.
    fn move_members(&mut self, name: &str, sym: SymbolId) {
        let fields = self.field_list(sym);
        self.line(&format!("{}{name}::{name}({name}&& other) {{", self.def_prefix));
        self.indent += 1;
        for (f, ty) in &fields {
            if self.table.copyable(ty) {
                self.line(&format!("{f} = other.{f};"));
            } else {
                self.line(&format!("{f} = static_cast<decltype({f})&&>(other.{f});"));
            }
        }
        self.line("other.reset_to_defaults__();");
        self.indent -= 1;
        self.line("}");
        self.out.push('\n');

        self.line(&format!("{}{name}& {name}::operator=({name}&& other) {{", self.def_prefix));
        self.indent += 1;
        self.line("if (this == &other) { return *this; }");
        self.line(&format!("this->~{name}();"));
        self.line(&format!("new (static_cast<void*>(this)) {name}(static_cast<{name}&&>(other));"));
        self.line("return *this;");
        self.indent -= 1;
        self.line("}");
        self.out.push('\n');
    }

    /// The C++ name and type of each field, in declaration order — read through the
    /// instance being emitted, so `Buffer<f32>`'s `m_data` is a `float*` (spec §8.4).
    fn field_list(&self, sym: SymbolId) -> Vec<(String, Ty)> {
        let Some(s) = self.table.struct_sig(sym) else { return Vec::new() };
        let tys = self.table.field_types(sym, &self.args_for(sym), &s.fields);
        s.fields.iter().zip(tys).map(|(f, t)| (sanitize(&f.name), t)).collect()
    }

    /// For each of `sym`'s fields, the type that declared it — `None` for its own, the
    /// base (or the base's base) for one it inherited (spec §7.3, 18.32).
    fn declaring_types(&self, sym: SymbolId) -> Vec<Option<SymbolId>> {
        match self.table.struct_sig(sym) {
            Some(s) => s.fields.iter().map(|f| f.declared_in).collect(),
            None => Vec::new(),
        }
    }

    /// A field's `= expr` default, rendered. Field defaults are ordinary expressions but
    /// live in a member initialiser, where nothing can be hoisted — so one that needs
    /// statements is built by a lambda called in place.
    fn field_default(&mut self, sym: SymbolId, index: usize) -> Option<String> {
        let owner = BodyOwner::FieldDefault { owner: sym, index };
        let body = self.find_body(&owner)?;
        let (pre, text) = self.in_body(body, |e| {
            let root = body.block(body.root);
            match root.stmts.first().map(|s| body.stmt(*s)) {
                Some(hir::Stmt { kind: hir::StmtKind::Expr(v), .. }) => e.expr(*v),
                _ => "0".to_owned(),
            }
        });
        if !pre.trim().is_empty() {
            return Some(Self::immediately_invoked(&pre, &text));
        }
        Some(text)
    }

    // --- unions (spec §7.5) ------------------------------------------------

    /// A tagged union, or with `args` one instance of a generic one (spec §7.5, §8.4).
    fn union_decl(&mut self, u: &UnionDecl, sym: SymbolId, args: &[Ty]) {
        let Some(ItemSig::Union { variants }) = self.table.get(sym) else { return };
        // Each variant's fields as this instance sees them: `value: T` is `value: i32` in
        // `Maybe<i32>`.
        let variants: Vec<dz_check::VariantSig> = variants
            .iter()
            .map(|v| {
                let types = self.table.field_types(sym, args, &v.fields);
                let fields = v
                    .fields
                    .iter()
                    .zip(types)
                    .map(|(f, ty)| dz_check::FieldSig { ty, ..f.clone() })
                    .collect();
                dz_check::VariantSig { fields, ..v.clone() }
            })
            .collect();
        // A union owns its active payload (spec §7.5, 18.52). When every payload is plain
        // data the C++ union needs nothing written; otherwise its special members switch on
        // the tag, so the payload that is there is the one copied, moved and destroyed.
        let trivial = self.table.union_is_trivial(sym, args);

        let name =
            if args.is_empty() { sanitize(&u.name.name) } else { self.instance_name(sym, args) };
        let _ = writeln!(self.header, "struct {name} {{");
        let _ = writeln!(self.header, "    enum class Kind : uint32_t {{");
        for v in &variants {
            let _ = writeln!(self.header, "        {},", sanitize(&v.name));
        }
        self.header.push_str("    };\n\n");

        for v in variants.iter().filter(|v| v.carries.is_none()) {
            let _ = writeln!(self.header, "    struct {}_t {{", sanitize(&v.name));
            for f in &v.fields {
                let ty = self.cpp_ty(&f.ty);
                let _ = writeln!(self.header, "        {ty} {}{{}};", sanitize(&f.name));
            }
            self.header.push_str("    };\n");
        }

        let first = variants.first().map(|v| sanitize(&v.name)).unwrap_or_default();
        let _ = writeln!(self.header, "\n    Kind kind{{Kind::{first}}};");
        self.header.push_str("    union {\n");
        for v in &variants {
            let n = sanitize(&v.name);
            // A carried struct by its *fully* qualified name, even from its own module: the
            // member has the struct's own name, and g++ 14 rejects a class member whose
            // declaration changes what an unqualified name used inside it meant
            // (`-Wchanges-meaning`, an error by default). g++ 13 and clang accept it.
            let ty = match v.carries {
                Some(c) => match self.res.cpp_path(c) {
                    Some(cpp) => cpp.to_owned(),
                    None => format!(
                        "{}::{}",
                        qualified_namespace(&self.ws.module(self.res.symbol(c).module).name),
                        sanitize(&self.res.symbol(c).name)
                    ),
                },
                None => format!("{n}_t"),
            };
            let _ = writeln!(self.header, "        {ty} {n};");
        }
        self.header.push_str("    };\n\n");
        // The first variant, at its defaults, is what `let s: Shape` holds (spec §6.2).
        let _ = writeln!(self.header, "    {name}() : {first}() {{}}");
        if trivial {
            self.header.push_str("};\n\n");
            return;
        }

        let copyable = self.table.copyable(&Ty::Named { sym, args: args.to_vec() });
        let names: Vec<String> = variants.iter().map(|v| sanitize(&v.name)).collect();
        let each = |body: &dyn Fn(&str) -> String| -> String {
            let mut s = String::from("        switch (kind) {\n");
            for n in &names {
                let _ = writeln!(s, "            case Kind::{n}: {} break;", body(n));
            }
            s.push_str("        }\n");
            s
        };
        let h = &mut self.header;
        // Destroying runs the active payload's destructor, and only that one.
        let _ = writeln!(h, "    ~{name}() {{ drop_active(); }}");
        if copyable {
            let _ = writeln!(h, "    {name}(const {name}& o) : kind(o.kind) {{ copy_from(o); }}");
            let _ = writeln!(
                h,
                "    {name}& operator=(const {name}& o) {{ if (this != &o) {{ drop_active(); \
                 kind = o.kind; copy_from(o); }} return *this; }}"
            );
        } else {
            let _ = writeln!(h, "    {name}(const {name}&) = delete;");
            let _ = writeln!(h, "    {name}& operator=(const {name}&) = delete;");
        }
        // A move takes the active payload and leaves the source at its defaults, as every
        // moved-from value is (spec §6.4).
        let _ = writeln!(
            h,
            "    {name}({name}&& o) : kind(o.kind) {{ move_from(o); o.become(Kind::{first}); }}"
        );
        let _ = writeln!(
            h,
            "    {name}& operator=({name}&& o) {{ if (this != &o) {{ drop_active(); kind = o.kind; \
             move_from(o); o.become(Kind::{first}); }} return *this; }}"
        );
        // Switching variant drops the old payload before the new one is built (18.52).
        h.push_str("    void become(Kind k) {\n        drop_active();\n        kind = k;\n");
        let construct = each(&|n| format!("::dz::construct({n});"));
        h.push_str(&construct);
        h.push_str("    }\n");
        let drop_body = each(&|n| format!("::dz::destroy({n});"));
        let _ = write!(h, "    void drop_active() {{\n{drop_body}    }}\n");
        if copyable {
            let copy_body = each(&|n| format!("::dz::construct({n}, o.{n});"));
            let _ = write!(h, "    void copy_from(const {name}& o) {{\n{copy_body}    }}\n");
        }
        let move_body = each(&|n| format!("::dz::construct({n}, ::dz::mv(o.{n}));"));
        let _ = write!(h, "    void move_from({name}& o) {{\n{move_body}    }}\n");
        h.push_str("};\n\n");
    }

    // --- function shells ---------------------------------------------------

    fn free_fn(&mut self, sym: SymbolId, name: &str) {
        let owner = BodyOwner::Fn(sym);
        if self.find_body(&owner).is_none() {
            return;
        }
        let ret = self.body_ret_ty(&owner);
        let decl = self.decl_params(&owner, true);
        // `current_fn` feeds `#function`, which names the Dazzle function, not the C++ one.
        let cpp = sanitize(name);
        let _ = writeln!(self.header, "{ret} {cpp}({decl});");

        let params = self.decl_params(&owner, false);
        self.current_fn = name.to_owned();
        let head = format!("{ret} {cpp}({params}) {{");
        self.emit_body(&owner, &head);
    }

    /// An `extern fn` of `std`: a forwarding shell to `::dz::native::<name>` in the
    /// runtime, which the C++ compiler inlines away (spec §7.2, §16.4). The shell keeps a
    /// call to it an ordinary call as far as the rest of codegen is concerned.
    fn extern_fn(&mut self, sym: SymbolId, name: &str) {
        let owner = BodyOwner::Fn(sym);
        let Some(body) = self.find_body(&owner) else { return };
        let ret = self.body_ret_ty(&owner);
        let decl = self.decl_params(&owner, true);
        let cpp = sanitize(name);
        let _ = writeln!(self.header, "{ret} {cpp}({decl});");
        let names = local_names(body);
        let mut params = Vec::new();
        let mut args = Vec::new();
        for p in &body.params {
            let local = body.local(*p);
            let ty = self.cpp_ty(&local.ty);
            params.push(format!("{ty} {}", names[p.index()]));
            args.push(names[p.index()].clone());
        }
        let ret_kw = if matches!(body.ret, Ty::Void) { "" } else { "return " };
        let _ = writeln!(
            self.out,
            "{ret} {cpp}({}) {{\n    {ret_kw}::dz::native::{name}({});\n}}\n",
            params.join(", "),
            args.join(", ")
        );
    }

    /// The C++ return type of a body. A `!->` signature returns `dz::Res<T>`, since the
    /// success type alone cannot carry the failure (spec §9.3).
    fn body_ret_ty(&mut self, owner: &BodyOwner) -> String {
        let Some(body) = self.find_body(owner) else { return "void".to_owned() };
        let ret = self.cpp_ty(&body.ret);
        if body.fallible {
            format!("::dz::Res<{ret}>")
        } else {
            ret
        }
    }

    /// Parameters as C++ writes them. `with_defaults` controls whether default arguments
    /// are printed, which C++ allows only on the declaration.
    fn decl_params(&mut self, owner: &BodyOwner, with_defaults: bool) -> String {
        let Some(body) = self.find_body(owner) else { return String::new() };
        let names = local_names(body);
        let uses = hir::local_uses(body);
        let has_self = body.params.first().is_some_and(|p| body.local(*p).name == "self");
        let start = usize::from(has_self);

        let mut parts: Vec<String> = Vec::new();
        for (i, p) in body.params.iter().enumerate().skip(start) {
            let local = body.local(*p);
            let ty = self.cpp_ty(&local.ty);
            let unused = if uses[p.index()].read { "" } else { "[[maybe_unused]] " };
            let mut text =
                format!("{unused}{ty} {}", incoming_name(&names[p.index()], self.table, &local.ty));
            if with_defaults {
                // An implementation's method takes its defaults from the trait when it does
                // not write its own: the trait's signature is what callers were checked
                // against (spec §7.6).
                let own = body.param_defaults.get(i - start).copied().flatten().map(|d| (body, d));
                let found = own.or_else(|| self.trait_default(owner, i - start));
                if let Some((from, default)) = found {
                    let v = self.render_default(from, default);
                    let _ = write!(text, " = {v}");
                }
            }
            parts.push(text);
        }
        parts.join(", ")
    }

    /// A default argument as its declaration writes it.
    fn render_default(&mut self, body: &'a hir::Body, default: ExprId) -> String {
        // `loc: SourceLocation = #location` is the *caller's* location (spec §12.7). A C++
        // default argument is evaluated at each call, and these builtins answer for that
        // call — through the `#line` directives, in Dazzle's file and line.
        if let hir::ExprKind::Intrinsic { name, .. } = &body.expr(default).kind {
            if let Some(v) = caller_intrinsic(name) {
                return v.to_owned();
            }
        }
        let (pre, value) = self.in_body(body, |e| e.expr(default));
        if pre.trim().is_empty() {
            value
        } else {
            Self::immediately_invoked(&pre, &value)
        }
    }

    /// The trait's default for parameter `index` of an implementation's method, from the
    /// trait method's body or, for one without a body, its defaults-only body.
    fn trait_default(&self, owner: &BodyOwner, index: usize) -> Option<(&'a hir::Body, ExprId)> {
        let BodyOwner::ImplMethod { imp, name } = owner else { return None };
        let tsym = self.table.impls().get(*imp as usize)?.trait_ref.trait_sym;
        let hir = self.hir;
        let body = hir
            .find(&BodyOwner::TraitDefaults { owner: tsym, name: name.clone() })
            .or_else(|| hir.find(&BodyOwner::Method { owner: tsym, name: name.clone() }))?;
        Some((body, body.param_defaults.get(index).copied().flatten()?))
    }

    /// Emit `head`, then the body's statements, then the closing brace.
    /// A constructor's `: UiElement(w)`, rendered as C++'s member-initialiser list.
    ///
    /// The base call lives in the constructor's own body arena but outside its root block
    /// (spec §7.3, 18.32), so it is rendered here rather than reached by `block_into`.
    /// An argument built by statements — `: Base(x or 0)` — has nowhere before the
    /// member-initialiser list to put them, so that argument alone becomes a lambda called in
    /// place. It captures by reference, since the constructor's parameters are what it reads;
    /// the checker has refused a `return`, the one statement a lambda would change the
    /// meaning of.
    fn base_init(&mut self, owner: &BodyOwner) -> String {
        let Some(body) = self.find_body(owner) else { return String::new() };
        let Some(init) = body.base_init else { return String::new() };
        let (pre, text) = self.in_body(body, |e| e.expr(init));
        if pre.trim().is_empty() {
            return format!(" : {text}");
        }
        let hir::ExprKind::Call { callee: hir::Callee::Ctor { owner: base, .. }, args } =
            &body.expr(init).kind
        else {
            return format!(" : {text}");
        };
        let target = self.qualify(*base);
        let mut rendered = Vec::new();
        for a in args {
            let moved = !self.table.copyable(body.ty(*a));
            let (pre, v) = self.in_body(body, |e| e.expr(*a));
            let v = if moved { format!("::dz::mv({v})") } else { v };
            if pre.trim().is_empty() {
                rendered.push(v);
            } else {
                let lambda = Self::immediately_invoked(&pre, &v);
                rendered.push(lambda.replacen("[]()", "[&]()", 1));
            }
        }
        format!(" : {target}({})", rendered.join(", "))
    }

    fn emit_body(&mut self, owner: &BodyOwner, head: &str) {
        let Some(body) = self.find_body(owner) else { return };
        self.line(head);
        self.indent += 1;
        self.last_line = 0;
        self.take_ownership_of_params(body);
        let (text, ()) = self.in_body(body, |e| e.block_into(body.root, Dest::None));
        self.write_captured(&text);
        // A `!-> void` still has to say it succeeded.
        if body.fallible && matches!(body.ret, Ty::Void) {
            self.line("return ::dz::Res<void>::good();");
        } else if !matches!(body.ret, Ty::Void | Ty::Error | Ty::Never) && !ends_in_return(body) {
            // `dz_flow` proved every Dazzle path returns, but C++ cannot see that through
            // an exhaustive `switch` with no `default` — and a `-Wreturn-type` warning in
            // generated code is a compiler bug, not a user error (invariant 7).
            let loc = self.source_location(body.span);
            self.line(&format!("::dz::unreachable({loc});"));
        }
        self.indent -= 1;
        self.line("}");
        self.out.push('\n');
    }

    /// Move each by-value parameter that owns something into a local of its own.
    ///
    /// Spec §6.5 says a value drops at the end of its enclosing scope, and for a parameter
    /// that scope is the function. C++ does not agree with itself about this: under the
    /// Itanium ABI the *caller* destroys a by-value parameter at the end of the full
    /// expression, under the MSVC ABI the *callee* destroys it before returning. The same
    /// program printed its drops in a different order under g++ and clang++.
    ///
    /// Moving the parameter into a local at entry settles it: the local dies at the end of
    /// the body on every ABI, and what the ABI eventually destroys is the moved-from
    /// husk, whose `drop` §6.4 already guarantees is a harmless no-op. The cost is a move
    /// per owning parameter, which is what the language promised anyway.
    fn take_ownership_of_params(&mut self, body: &'a hir::Body) {
        let names = local_names(body);
        let uses = hir::local_uses(body);
        for p in &body.params {
            let local = body.local(*p);
            if local.name == "self" || !self.table.needs_drop(&local.ty) {
                continue;
            }
            let ty = self.cpp_ty(&local.ty);
            let name = &names[p.index()];
            let arg = incoming_name(name, self.table, &local.ty);
            let unused = if uses[p.index()].read { "" } else { "[[maybe_unused]] " };
            self.line(&format!("{unused}{ty} {name} = ::dz::mv({arg});"));
        }
    }

    /// "[[maybe_unused]] " for a local nothing reads. The user has already had a DZ0406
    /// warning for it, and the C++ compiler warning about the same variable again would be
    /// a compiler bug (invariant 7). The one answer comes from dz_hir::local_uses, which is
    /// what dz_flow warns from.
    fn unused_attr(&self, index: usize) -> &'static str {
        let read = hir::local_uses(self.cur()).get(index).map_or(true, |u| u.read);
        if read {
            ""
        } else {
            "[[maybe_unused]] "
        }
    }

    /// Run `f` with `body` installed as the current body, capturing what it emits.
    fn in_body<R>(&mut self, body: &'a hir::Body, f: impl FnOnce(&mut Self) -> R) -> (String, R) {
        let saved_body = self.body.replace(body);
        let mut names = local_names(body);
        // A generic method's instance is a free function, so its receiver is the parameter
        // `self` — as an implementation's is — and not `*this` (spec §8.4).
        if self.method_instance.is_some_and(|m| std::ptr::eq(&m.body, body)) {
            for p in &body.params {
                if body.local(*p).name == "self" {
                    names[p.index()] = "self".to_owned();
                }
            }
        }
        let saved_names = std::mem::replace(&mut self.names, names);
        let saved_file = std::mem::replace(&mut self.file, body.file);
        let result = self.capture(f);
        self.body = saved_body;
        self.names = saved_names;
        self.file = saved_file;
        result
    }

    // --- types -------------------------------------------------------------

    /// `^mut T` is `T*` and `^T` is `const T*` (spec §15.2). After a pointee that is itself
    /// a pointer the `const` goes on the right, which is where C++ reads it as applying to
    /// that pointer: `^^mut i32` is `int32_t* const*`.
    fn ptr_ty(&mut self, mutable: bool, inner: &Ty) -> String {
        if let Some(t) = self.trait_object(inner) {
            return self.dyn_name(t);
        }
        let pointee = self.cpp_ty(inner);
        if mutable {
            format!("{pointee}*")
        } else if pointee.ends_with('*') {
            format!("{pointee} const*")
        } else {
            format!("const {pointee}*")
        }
    }

    fn cpp_ty(&mut self, t: &Ty) -> String {
        match t {
            Ty::Error | Ty::Never | Ty::Void => "void".to_owned(),
            Ty::Bool => "bool".to_owned(),
            Ty::Char => "char32_t".to_owned(),
            Ty::Int(i) => int_name(*i).to_owned(),
            Ty::Float(FloatTy::F32) => "float".to_owned(),
            Ty::Float(FloatTy::F64) => "double".to_owned(),
            Ty::Str => "::dz::str".to_owned(),
            Ty::CStr => "const char*".to_owned(),
            // `finish()` settles every literal, so these can only be a checker bug.
            Ty::IntLit => "int32_t".to_owned(),
            Ty::FloatLit => "double".to_owned(),
            Ty::Ptr { mutable, inner } | Ty::OwnPtr { mutable, inner } => {
                self.ptr_ty(*mutable, inner)
            }
            // A trait object is a two-word handle whatever points at it (spec §7.6).
            Ty::Ref { inner, .. } if self.trait_object(inner).is_some() => {
                self.trait_object(inner).map(|t| self.dyn_name(t)).unwrap_or_default()
            }
            Ty::Ref { mutable: true, inner } => format!("{}&", self.cpp_ty(inner)),
            Ty::Ref { mutable: false, inner } => format!("const {}&", self.cpp_ty(inner)),
            Ty::Optional(i) => format!("::dz::Opt<{}>", self.cpp_ty(i)),
            Ty::Fallible(i) => format!("::dz::Res<{}>", self.cpp_ty(i)),
            Ty::Array { len, elem } => format!("::dz::Array<{}, {len}>", self.cpp_ty(elem)),
            // A read-only slice becomes `Slice<const T>`, so C++ enforces the same rule
            // the checker does and a mistake that slipped past us is caught rather than
            // miscompiled (spec §5.3, §18.22).
            Ty::Slice { mutable: true, elem } => format!("::dz::Slice<{}>", self.cpp_ty(elem)),
            Ty::Slice { mutable: false, elem } => {
                format!("::dz::Slice<const {}>", self.cpp_ty(elem))
            }
            Ty::Tuple(parts) => {
                let ps: Vec<String> = parts.iter().map(|p| self.cpp_ty(p)).collect();
                format!("::dz::Tuple<{}>", ps.join(", "))
            }
            Ty::Range(elem) => format!("::dz::Range<{}>", self.cpp_ty(elem)),
            // Through the shim's alias: C++'s own `R (*)(A)` puts the name inside the
            // parentheses, so it cannot be written as `T name` the way every other type is.
            Ty::Fn { params, ret } => {
                let mut ps: Vec<String> = vec![self.cpp_ty(ret)];
                ps.extend(params.iter().map(|p| self.cpp_ty(p)));
                format!("::dz::fn<{}>", ps.join(", "))
            }
            // Both live in the shim, so they keep their `dz::` name (spec §9.2, §16.4).
            Ty::Builtin(b) => format!("::dz::{}", b.name()),
            // An instance has a name of its own (spec §8.4); a plain declaration keeps its.
            Ty::Named { sym, args } if !args.is_empty() => self.qualify_instance(*sym, args),
            Ty::Named { sym, .. } => self.qualify(*sym),
            Ty::Variant(sym, index, args) => {
                let name = self.variant_name(*sym, *index);
                let owner = self.cpp_ty(&Ty::Named { sym: *sym, args: args.clone() });
                format!("{owner}::{name}_t")
            }
            // `Self` is substituted away by the checker; reaching here is a bug there.
            Ty::SelfTy => "void".to_owned(),
            // So is a generic parameter, by monomorphization (spec §8.4). Nothing generic
            // reaches codegen: `dz_mono` has already replaced every `T` with a concrete
            // type, so this arm exists only to keep the match total.
            Ty::Param(_) => "void".to_owned(),
        }
    }

    /// `::dz::obj(x)` reaches through a pointer. A value or a reference already *is* the
    /// thing, and a C array member must not go through a template at all: it would decay to
    /// a pointer there and take its length with it (spec §15.2).
    fn through(&self, e: ExprId, rendered: String) -> String {
        if matches!(self.cur().ty(e).peeled_refs(), Ty::Ptr { .. } | Ty::OwnPtr { .. }) {
            format!("::dz::obj({rendered})")
        } else {
            rendered
        }
    }

    /// Does this build leave the item out? A `#[test]` belongs to the test binary and a
    /// `#[bench]` to the benchmark binary; the program carries neither, and neither
    /// carries the other (spec §19 M14, M15).
    fn excluded(&self, item: &Item) -> bool {
        let marked =
            ["test", "bench"].into_iter().find(|w| item.attrs.iter().any(|a| a.name.name == *w));
        match marked {
            Some(w) => self.mode.attribute() != Some(w),
            None => false,
        }
    }

    fn is_foreign(&self, sym: SymbolId) -> bool {
        self.ws.module(self.res.symbol(sym).module).foreign.is_some()
    }

    fn qualify(&self, sym: SymbolId) -> String {
        let s = self.res.symbol(sym);
        // An imported header's names are its own and global, spelled `::mini_add` so no
        // Dazzle name in the enclosing namespace can hide one — and `::JPH::Body` for one a
        // namespace holds. The importer settled the spelling; this only writes it (§15.2).
        if let Some(cpp) = self.res.cpp_path(sym) {
            return cpp.to_owned();
        }
        if s.module == self.module {
            sanitize(&s.name)
        } else {
            format!(
                "{}::{}",
                qualified_namespace(&self.ws.module(s.module).name),
                sanitize(&s.name)
            )
        }
    }

    // --- statements --------------------------------------------------------

    /// Emit a block's statements, sending the value of its final expression to `dest`.
    fn block_into(&mut self, id: BlockId, dest: Dest<'_>) {
        let body = self.cur();
        let stmts = &body.block(id).stmts;
        let last = stmts.len().saturating_sub(1);
        for (i, s) in stmts.iter().enumerate() {
            if i == last && !matches!(dest, Dest::None) {
                if let hir::StmtKind::Expr(e) = &body.stmt(*s).kind {
                    let ty = body.ty(*e);
                    if !matches!(ty, Ty::Void | Ty::Never) {
                        self.line_directive(body.stmt(*s).span);
                        self.assign_into(*e, dest);
                        continue;
                    }
                }
            }
            self.stmt(*s);
        }
    }

    /// Evaluate `e` and put it where `dest` says.
    fn assign_into(&mut self, e: ExprId, dest: Dest<'_>) {
        match dest {
            Dest::None => self.stmt_expr(e),
            Dest::Var(name) => {
                let moved = !self.table.copyable(self.cur().ty(e));
                let v = self.expr(e);
                if moved {
                    self.line(&format!("{name} = ::dz::mv({v});"));
                } else {
                    self.line(&format!("{name} = {v};"));
                }
            }
            Dest::Ptr(name) => {
                let v = self.ptr_expr(e);
                self.line(&format!("{name} = {v};"));
            }
        }
    }

    fn stmt(&mut self, id: StmtId) {
        let body = self.cur();
        let s = body.stmt(id);
        self.line_directive(s.span);
        match &s.kind {
            hir::StmtKind::Let { local, init } => match (local, init) {
                (Some(l), Some(init)) => {
                    let ty = self.cpp_ty(&body.local(*l).ty);
                    let name = self.names[l.index()].clone();
                    let local_ty = body.local(*l).ty.clone();
                    let v = self.stored(&local_ty, *init);
                    let unused = self.unused_attr(l.index());
                    self.line(&format!("{unused}{ty} {name} = {v};"));
                }
                // Declared with no value: default-initialise (spec §6.2).
                (Some(l), None) => {
                    let ty = self.cpp_ty(&body.local(*l).ty);
                    let name = self.names[l.index()].clone();
                    let unused = self.unused_attr(l.index());
                    self.line(&format!("{unused}{ty} {name}{{}};"));
                }
                // `let _ = f()` still evaluates.
                (None, Some(init)) => self.stmt_expr(*init),
                (None, None) => {}
            },
            hir::StmtKind::LetTuple { locals, init } => {
                let tmp = self.fresh("tup");
                let ty = self.cpp_ty(body.ty(*init));
                let v = self.expr(*init);
                self.line(&format!("{ty} {tmp} = {v};"));
                for (i, l) in locals.iter().enumerate() {
                    let Some(l) = l else { continue };
                    let lty = self.cpp_ty(&body.local(*l).ty);
                    let name = self.names[l.index()].clone();
                    let unused = self.unused_attr(l.index());
                    self.line(&format!("{unused}{lty} {name} = {tmp}.v{i};"));
                }
            }
            hir::StmtKind::Assign { target, op, value } => {
                let moved = *op == AssignOp::Assign
                    && !self.table.copyable(body.ty(*value))
                    && matches!(body.expr(*value).kind, hir::ExprKind::Move(_));
                // The value is evaluated before the place it goes to (spec §7.2, 18.49) —
                // which C++17 already does for `=` and `op=`, so only statements hoisted out
                // of either side need putting in order: the value's first, and the value
                // held in a temporary if the place's would otherwise run ahead of it.
                let target_ty = body.ty(*target).clone();
                let (pre_t, t) = self.capture(|e| e.place_expr(*target));
                let (pre_v, v) = self.capture(|e| e.stored(&target_ty, *value));
                self.write_captured(&pre_v);
                let v = if moved { format!("::dz::mv({v})") } else { v };
                let v = if !pre_t.trim().is_empty() && !self.order_stable(*value) {
                    let tmp = self.fresh("val");
                    let ty = self.cpp_ty(target_ty.peeled_refs());
                    self.line(&format!("{ty} {tmp} = {v};"));
                    format!("::dz::mv({tmp})")
                } else {
                    v
                };
                self.write_captured(&pre_t);
                self.line(&format!("{t} {} {v};", op.as_str()));
            }
            hir::StmtKind::Expr(e) => self.stmt_expr(*e),
            hir::StmtKind::While { cond, body: block, label } => {
                let (pre, cond_text) = self.capture(|e| e.expr(*cond));
                if pre.trim().is_empty() {
                    self.line(&format!("while ({}) {{", unwrap_parens(&cond_text)));
                    self.indent += 1;
                } else {
                    // The condition needs statements of its own, and they have to run on
                    // every iteration rather than once before the loop.
                    self.line("while (true) {");
                    self.indent += 1;
                    self.write_captured(&pre);
                    self.line(&format!("if (!({cond_text})) {{ break; }}"));
                }
                self.loop_body_into(*block, label);
                self.indent -= 1;
                self.line("}");
                self.loop_break_label(label);
            }
            hir::StmtKind::For { bindings, subject, body: block, shape, label } => {
                self.for_loop(bindings, *subject, *block, *shape, label, s.span)
            }
            hir::StmtKind::Defer(inner) => {
                // Captured by reference so the deferred code sees the final values.
                let name = self.fresh("defer");
                self.line(&format!("auto {name} = ::dz::make_defer([&] {{"));
                self.indent += 1;
                self.stmt(*inner);
                self.indent -= 1;
                self.line("});");
            }
            // Nested declarations are emitted at module scope by the outer walk.
            hir::StmtKind::Item(_) | hir::StmtKind::Error => {}
        }
    }

    /// An expression in statement position: the value, if any, is discarded.
    fn stmt_expr(&mut self, e: ExprId) {
        let body = self.cur();
        match &body.expr(e).kind {
            hir::ExprKind::If { .. }
            | hir::ExprKind::IfLet { .. }
            | hir::ExprKind::Switch { .. }
            | hir::ExprKind::Block(_) => self.control_flow(e, Dest::None),
            hir::ExprKind::Or { operand, clause } => {
                self.or_expr(e, *operand, clause, Dest::None);
            }
            _ => {
                let ty = body.ty(e).clone();
                let text = self.expr(e);
                if text.is_empty() {
                    return;
                }
                // A value thrown away needs the cast, or `-Wunused-value` fires on it.
                if matches!(ty, Ty::Void | Ty::Never | Ty::Error) {
                    self.line(&format!("{text};"));
                } else {
                    self.line(&format!("(void)({text});"));
                }
            }
        }
    }

    /// `for` (spec §10.4). The loop head depends on what is iterated — a range counts, a
    /// user `Iterate` steps an iterator, an array or slice is walked in place — and the
    /// bindings on the shape and on each binding's own type: a reference binding is a C++
    /// reference into the element, anything else a copy (or, consuming, a move).
    fn for_loop(
        &mut self,
        bindings: &[Option<LocalId>],
        subject: ExprId,
        block: BlockId,
        shape: hir::ForShape,
        label: &Option<hir::LoopLabel>,
        span: Span,
    ) {
        let body = self.cur();
        let subject_ty = body.ty(subject).clone();

        // `for i in a..b` is a counting loop; nothing is materialised.
        if let (hir::ExprKind::Range { start, end, inclusive }, hir::ForShape::Single) =
            (&body.expr(subject).kind, shape)
        {
            let elem = self.cpp_ty(subject_ty.element().unwrap_or(&Ty::Int(IntTy::I32)));
            let lo = start.map(|s| self.expr(s)).unwrap_or_else(|| "0".to_owned());
            let hi = end.map(|s| self.expr(s)).unwrap_or_else(|| "0".to_owned());
            let name = match bindings[0] {
                Some(l) => self.names[l.index()].clone(),
                None => self.fresh("i"),
            };
            let cmp = if *inclusive { "<=" } else { "<" };
            self.line(&format!("for ({elem} {name} = {lo}; {name} {cmp} {hi}; ++{name}) {{"));
            self.indent += 1;
            self.loop_body_into(block, label);
            self.indent -= 1;
            self.line("}");
            self.loop_break_label(label);
            return;
        }

        // `indexed()` counts passes (spec §10.4).
        let counter = (shape == hir::ForShape::Indexed).then(|| self.fresh("count"));
        let element_bindings: Vec<Option<LocalId>> = match shape {
            hir::ForShape::Indexed => bindings.iter().skip(1).copied().collect(),
            _ => bindings.to_vec(),
        };
        self.line("{");
        self.indent += 1;
        if let Some(i) = &counter {
            self.line(&format!("size_t {i} = 0;"));
        }

        let item = if let Ty::Range(elem) = subject_ty.peeled() {
            // A range held in a variable, or counted with `indexed()`.
            let elem = self.cpp_ty(elem);
            let subject_text = self.expr(subject);
            let r = self.fresh("r");
            let n = self.fresh("n");
            self.line(&format!("auto {r} = {subject_text};"));
            self.line(&format!(
                "for ({elem} {n} = {r}.start; {r}.inclusive ? {n} <= {r}.end : {n} < {r}.end; \
                 ++{n}) {{"
            ));
            n
        } else if let Ty::Named { .. } = subject_ty.peeled() {
            // A user `Iterate`: the loop's own copy (or move) of the iterator, since
            // `advance` mutates it.
            let iterated = subject_ty.peeled().clone();
            let calls = self
                .table
                .prelude_trait_named("Iterate")
                .and_then(|t| self.table.resolve_impl(t, &iterated, "valid", None))
                .map(|(idx, map)| {
                    (
                        self.qualify_impl_fn(idx, &map, "valid"),
                        self.qualify_impl_fn(idx, &map, "get"),
                        self.qualify_impl_fn(idx, &map, "advance"),
                    )
                });
            let Some((valid, get, advance)) = calls else {
                self.unsupported(span, "`for` over this type", "owed by M12");
                self.indent -= 1;
                self.line("}");
                return;
            };
            let subject_text = self.expr(subject);
            let it = self.fresh("it");
            self.line(&format!(
                "for (auto {it} = {subject_text}; {valid}({it}); {advance}({it})) {{"
            ));
            format!("{get}({it})")
        } else {
            // An array or slice, walked in place — or, consuming, moved from. `&xs` is
            // iterated as `xs` itself: a reference already is the sequence.
            let pointer_subject =
                matches!(subject_ty.peeled_refs(), Ty::Ptr { .. } | Ty::OwnPtr { .. });
            let subject_text = match &body.expr(subject).kind {
                hir::ExprKind::Ref { place, .. } => self.expr(*place),
                _ => self.expr(subject),
            };
            let seq = self.fresh("seq");
            self.line(&format!("auto&& {seq} = {subject_text};"));
            let seq_obj = if pointer_subject { format!("::dz::obj({seq})") } else { seq };
            let x = self.fresh("x");
            self.line(&format!("for (auto&& {x} : {seq_obj}) {{"));
            x
        };
        self.indent += 1;

        // The pass count is taken at the top of each pass, so a `continue` cannot skip it.
        if let (Some(c), Some(Some(l))) = (&counter, bindings.first()) {
            let name = self.names[l.index()].clone();
            let unused = self.unused_attr(l.index());
            self.line(&format!("{unused}size_t {name} = {c}++;"));
        } else if let Some(c) = &counter {
            self.line(&format!("++{c};"));
        }
        let consuming = shape == hir::ForShape::Consuming;
        if shape == hir::ForShape::Tuple {
            let t = self.fresh("t");
            self.line(&format!("auto&& {t} = {item};"));
            for (k, l) in element_bindings.iter().enumerate() {
                if let Some(l) = l {
                    self.for_binding(*l, &format!("{t}.v{k}"), false);
                }
            }
        } else if let Some(Some(l)) = element_bindings.first() {
            self.for_binding(*l, &item, consuming);
        }

        self.loop_body_into(block, label);
        self.indent -= 1;
        self.line("}");
        self.indent -= 1;
        self.line("}");
        self.loop_break_label(label);
    }

    /// A loop's body. When a nested loop `continue`s to this one, the body goes in a block of
    /// its own with the label after it: the jump then leaves every body local's scope
    /// rather than skipping past their declarations, which C++ forbids (spec §10.2).
    fn loop_body_into(&mut self, block: BlockId, label: &Option<hir::LoopLabel>) {
        match label {
            Some(l) if l.continues => {
                self.line("{");
                self.indent += 1;
                self.block_into(block, Dest::None);
                self.indent -= 1;
                self.line("}");
                self.line(&format!("{}:;", loop_label_name("continue", l.id)));
            }
            _ => self.block_into(block, Dest::None),
        }
    }

    /// The label a nested loop's `break outer` jumps to, straight after the loop.
    fn loop_break_label(&mut self, label: &Option<hir::LoopLabel>) {
        if let Some(l) = label.as_ref().filter(|l| l.breaks) {
            self.line(&format!("{}:;", loop_label_name("break", l.id)));
        }
    }

    /// Declare one `for` binding from its source. Its type says what it is: a `&T` or
    /// `&mut T` becomes a C++ reference into the element, anything else a copy — or a move,
    /// when the loop consumes what it iterates.
    fn for_binding(&mut self, l: LocalId, source: &str, consuming: bool) {
        let ty = self.cur().local(l).ty.clone();
        let cpp = self.cpp_ty(&ty);
        let name = self.names[l.index()].clone();
        let unused = self.unused_attr(l.index());
        let value = if consuming && !self.table.copyable(&ty) {
            format!("::dz::mv({source})")
        } else {
            source.to_owned()
        };
        self.line(&format!("{unused}{cpp} {name} = {value};"));
    }

    /// `if` / `switch` / a bare block, with its value going to `dest`.
    fn control_flow(&mut self, e: ExprId, dest: Dest<'_>) {
        let body = self.cur();
        match &body.expr(e).kind {
            // `if let x = v` (spec §9.4): the presence test `or` makes, with `x` declared
            // inside the `if` so it exists only where the value does.
            hir::ExprKind::IfLet { binding, operand, then, else_branch } => {
                let operand_ty = body.ty(*operand).clone();
                let name = self.names[binding.index()].clone();
                let unused = self.unused_attr(binding.index());
                // The binding's type says how it holds the value (18.61): a `&T` or `&mut T`
                // refers to it where it lives, anything else is a copy — or, out of an
                // optional the `if let` owns, a move.
                let binding_ty = body.local(*binding).ty.clone();
                let by_ref = matches!(binding_ty, Ty::Ref { .. });
                let binding_cpp = self.cpp_ty(&binding_ty);
                // A handle's guard is a line of the current block; a block of its own ends it
                // with the `if let`, which is as long as the binding can use it (spec §14.5).
                let guarded = matches!(body.expr(*operand).kind, hir::ExprKind::Guarded { .. });
                if guarded {
                    self.line("{");
                    self.indent += 1;
                }
                let v = self.expr(*operand);
                match operand_ty {
                    Ty::Optional(_) if by_ref => {
                        // Bound to the optional itself when it is a place, so a write through
                        // `&mut v` lands in it; a temporary lives as long as the `if`.
                        let tmp = self.fresh("opt");
                        self.line(&format!("auto&& {tmp} = {v};"));
                        self.line(&format!("if ({tmp}.has) {{"));
                        self.indent += 1;
                        self.line(&format!("{unused}{binding_cpp} {name} = {tmp}.value;"));
                    }
                    Ty::Optional(inner) => {
                        let cpp = self.cpp_ty(&inner);
                        let tmp = self.fresh("opt");
                        // A move-only optional is taken, as `or` takes one (spec §9.4).
                        let v =
                            if self.table.copyable(&inner) { v } else { format!("::dz::mv({v})") };
                        self.line(&format!("::dz::Opt<{cpp}> {tmp} = {v};"));
                        self.line(&format!("if ({tmp}.has) {{"));
                        self.indent += 1;
                        let value = if self.table.copyable(&inner) {
                            format!("{tmp}.value")
                        } else {
                            format!("::dz::mv({tmp}.value)")
                        };
                        self.line(&format!("{unused}{cpp} {name} = {value};"));
                    }
                    // A `&T?`: the checker allows a plain binding only for a copyable `T`.
                    Ty::Ref { inner, .. } if matches!(*inner, Ty::Optional(_)) => {
                        let tmp = self.fresh("opt");
                        self.line(&format!("auto&& {tmp} = {v};"));
                        self.line(&format!("if ({tmp}.has) {{"));
                        self.indent += 1;
                        self.line(&format!("{unused}{binding_cpp} {name} = {tmp}.value;"));
                    }
                    Ty::Ptr { mutable, inner } | Ty::OwnPtr { mutable, inner } => {
                        let ptr = self.ptr_ty(mutable, &inner);
                        let tmp = self.fresh("ptr");
                        self.line(&format!("{ptr} {tmp} = {v};"));
                        self.line(&format!("if ({tmp} != nullptr) {{"));
                        self.indent += 1;
                        self.line(&format!("{unused}{binding_cpp} {name} = *{tmp};"));
                    }
                    // The checker reported the operand; keep the braces balanced.
                    _ => {
                        self.line("if (false) {");
                        self.indent += 1;
                    }
                }
                self.block_into(*then, dest);
                self.indent -= 1;
                match else_branch {
                    Some(other) => {
                        self.line("} else {");
                        self.indent += 1;
                        self.block_into(*other, dest);
                        self.indent -= 1;
                        self.line("}");
                    }
                    None => self.line("}"),
                }
                if guarded {
                    self.indent -= 1;
                    self.line("}");
                }
            }
            hir::ExprKind::If { cond, then, else_branch } => {
                // §8.3: a condition made only of compile-time queries is decided here and
                // the dead branch is not emitted at all. It must not be: for this `T` it may
                // not even be valid C++ — the copying branch of `Buffer::copy` names a copy
                // constructor that a move-only `T` has deleted.
                if let Some(known) = self.fold_condition(*cond) {
                    let live = if known { Some(*then) } else { *else_branch };
                    if let Some(b) = live {
                        self.line("{");
                        self.indent += 1;
                        self.block_into(b, dest);
                        self.indent -= 1;
                        self.line("}");
                    }
                    return;
                }
                let c = self.expr(*cond);
                self.line(&format!("if ({}) {{", unwrap_parens(&c)));
                self.indent += 1;
                self.block_into(*then, dest);
                self.indent -= 1;
                match else_branch {
                    Some(other) => {
                        self.line("} else {");
                        self.indent += 1;
                        self.block_into(*other, dest);
                        self.indent -= 1;
                        self.line("}");
                    }
                    None => self.line("}"),
                }
            }
            hir::ExprKind::Block(b) => {
                self.line("{");
                self.indent += 1;
                self.block_into(*b, dest);
                self.indent -= 1;
                self.line("}");
            }
            hir::ExprKind::Switch { subject, arms, default } => {
                self.switch(*subject, arms, *default, dest)
            }
            _ => {}
        }
    }

    fn switch(
        &mut self,
        subject: ExprId,
        arms: &[hir::Arm],
        default: Option<BlockId>,
        dest: Dest<'_>,
    ) {
        let body = self.cur();
        let subject_ty = body.ty(subject).peeled().clone();
        // An enum or union is matched by variant; an integer, `char`, `bool` or `str` by
        // the constants in `arm.values` (spec §10.3).
        let named = subject_ty.named().map(|sym| {
            let is_union = matches!(self.table.get(sym), Some(ItemSig::Union { .. }));
            let variants: Vec<String> = match self.table.get(sym) {
                Some(ItemSig::Enum { variants, .. }) | Some(ItemSig::Union { variants }) => {
                    variants.iter().map(|v| sanitize(&v.name)).collect()
                }
                _ => Vec::new(),
            };
            // `cpp_ty`, not `qualify`: a `Maybe<i32>` subject compares against its instance.
            (self.cpp_ty(&subject_ty), is_union, variants)
        });
        let text = self.expr(subject);
        if arms.is_empty() {
            match default {
                Some(d) => {
                    self.line(&format!("(void)({text});"));
                    self.line("{");
                    self.indent += 1;
                    self.block_into(d, dest);
                    self.indent -= 1;
                    self.line("}");
                }
                None => self.line(&format!("(void)({text});")),
            }
            return;
        }

        // An `if` chain rather than a C++ `switch`: inside a C++ `switch`, `break` leaves
        // the switch, while a Dazzle `break` in an arm leaves the enclosing loop — there
        // is no fallthrough to break out of (§10.3). The subject is evaluated once, and
        // held by reference so a payload binding writes into the union itself.
        let subject_var = self.fresh("switch");
        // `obj` reaches through a pointer or a reference. A value is bound directly, so a
        // temporary — `switch next_state()` — has its lifetime extended rather than being
        // passed through `obj` and left dangling.
        let init = match self.cur().ty(subject) {
            Ty::Ptr { .. } | Ty::OwnPtr { .. } | Ty::Ref { .. } => format!("::dz::obj({text})"),
            _ => text,
        };
        self.line("{");
        self.indent += 1;
        self.line(&format!("auto&& {subject_var} = {init};"));
        for (i, arm) in arms.iter().enumerate() {
            let tests: Vec<String> = match &named {
                Some((owner, is_union, variants)) => arm
                    .variants
                    .iter()
                    .filter_map(|v| variants.get(*v))
                    .map(|name| {
                        // A union compares its generated tag; an enum is the value (§7.5).
                        if *is_union {
                            format!("{subject_var}.kind == {owner}::Kind::{name}")
                        } else {
                            format!("{subject_var} == {owner}::{name}")
                        }
                    })
                    .collect(),
                None => arm
                    .values
                    .iter()
                    .map(|v| {
                        let value = self.expr(*v);
                        format!("{subject_var} == {value}")
                    })
                    .collect(),
            };
            let cond = if tests.is_empty() { "false".to_owned() } else { tests.join(" || ") };
            let keyword = if i == 0 { "if" } else { "} else if" };
            self.line(&format!("{keyword} ({cond}) {{"));
            self.indent += 1;
            if let (Some(local), Some(v), Some((_, true, variants))) =
                (arm.binding, arm.variants.first(), &named)
            {
                if let Some(name) = variants.get(*v) {
                    // As a `for` binding: the local's type says copy (`c`), `const&` (`&c`)
                    // or `&` (`&mut c`), and nothing is turned into a reference behind the
                    // user's back (§10.3, 18.43).
                    let binding = self.names[local.index()].clone();
                    let unused = self.unused_attr(local.index());
                    let ty = self.cur().local(local).ty.clone();
                    let cpp = self.cpp_ty(&ty);
                    self.line(&format!("{unused}{cpp} {binding} = {subject_var}.{name};"));
                }
            }
            self.block_into(arm.body, dest);
            self.indent -= 1;
        }
        // Without a `default` the checker proved the arms exhaustive for an enum or union,
        // and a switch on constants simply does nothing when none matches.
        if let Some(d) = default {
            self.line("} else {");
            self.indent += 1;
            self.block_into(d, dest);
            self.indent -= 1;
        }
        self.line("}");
        self.indent -= 1;
        self.line("}");
    }

    // --- expressions -------------------------------------------------------

    /// A place expression, for the left of an assignment. Identical to [`Emitter::expr`]
    /// except that it must not be wrapped in anything that would make it an rvalue.
    fn place_expr(&mut self, e: ExprId) -> String {
        self.expr(e)
    }

    /// An expression of Dazzle type `&T` or `^T`, emitted as a C++ `T*`.
    ///
    /// Dazzle's `&T` is a C++ reference, which cannot be reseated, so anything that has to
    /// *store* one — the temporary behind `^T or …` — stores a pointer instead.
    fn ptr_expr(&mut self, e: ExprId) -> String {
        let body = self.cur();
        // A trait object is the same two-word handle as a reference or a pointer, so there
        // is nothing to take the address of (spec §7.6).
        if let Ty::Ref { inner, .. } | Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } = body.ty(e)
        {
            if self.trait_object(inner).is_some()
                && !matches!(body.expr(e).kind, hir::ExprKind::Ref { .. })
            {
                return self.expr(e);
            }
        }
        match &body.expr(e).kind {
            hir::ExprKind::Ref { place, .. } => {
                let p = self.expr(*place);
                format!("&({p})")
            }
            _ => match body.ty(e) {
                Ty::Ptr { .. } | Ty::OwnPtr { .. } => self.expr(e),
                _ => {
                    let v = self.expr(e);
                    format!("&({v})")
                }
            },
        }
    }

    /// A value stored where `target` is the destination's type. A `&T` built by a block, an
    /// `if` or an `or` is held as a C++ reference, which does not become the `T*` a `^T`
    /// destination is; a `&x` written directly is `::dz::addr`, which does (spec §5.2).
    fn stored(&mut self, target: &Ty, e: ExprId) -> String {
        let body = self.cur();
        let hoisted_ref = matches!(body.ty(e), Ty::Ref { .. })
            && !matches!(body.expr(e).kind, hir::ExprKind::Ref { .. });
        if hoisted_ref && matches!(target, Ty::Ptr { .. } | Ty::OwnPtr { .. }) {
            return self.ptr_expr(e);
        }
        self.expr(e)
    }

    fn expr(&mut self, e: ExprId) -> String {
        let body = self.cur();
        let node = body.expr(e);
        match &node.kind {
            // The checker reports every error node it makes, and codegen never runs after a
            // reported error, so reaching one here is a compiler bug. It used to become a
            // silent `0`, which is how a checker bug turned into bad C++ (invariant 7).
            hir::ExprKind::Error => {
                self.diags.push(
                    Diagnostic::error(
                        codes::INTERNAL_ERROR,
                        "the compiler lost track of this expression",
                    )
                    .at(self.file, node.span, "no C++ can be generated for this")
                    .with_help(
                        "this is a bug in the Dazzle compiler, not in your code: an earlier \
                         phase produced an error it did not report",
                    ),
                );
                "0".to_owned()
            }

            hir::ExprKind::Int(v) => int_literal(*v, body.ty(e)),
            hir::ExprKind::Float(v) => float_literal(*v, body.ty(e)),
            hir::ExprKind::Bool(b) => b.to_string(),
            hir::ExprKind::Char(c) => format!("U'\\U{:08X}'", *c as u32),
            // A literal typed `cstr` is the C string literal itself (spec §5.3, §18.39).
            hir::ExprKind::Str(s) if matches!(body.ty(e), Ty::CStr) => quote_cpp(s),
            hir::ExprKind::Str(s) => format!("::dz::str({}, {})", quote_cpp(s), s.len()),
            // `null` for an `Rc`, `Weak` or `Box` is the handle's default (spec §14.5).
            hir::ExprKind::Null => match body.ty(e).clone() {
                t @ Ty::Named { .. } => format!("{}{{}}", self.cpp_ty(&t)),
                _ => "nullptr".to_owned(),
            },
            hir::ExprKind::NoneOpt => {
                let ty = self.cpp_ty(body.ty(e));
                format!("{ty}()")
            }

            hir::ExprKind::Local(l) => self.names[l.index()].clone(),
            // An imported constant is an enumerator or a macro, whose C++ type is C's — an
            // enum, an `int` — rather than the one its Dazzle context settled (spec §15.2).
            hir::ExprKind::Const(sym) if self.is_foreign(*sym) => {
                let t = self.cpp_ty(body.ty(e));
                format!("static_cast<{t}>({})", self.qualify(*sym))
            }
            hir::ExprKind::FnItem { sym, type_args } if !type_args.is_empty() => {
                self.qualify_instance(*sym, type_args)
            }
            hir::ExprKind::Const(sym)
            | hir::ExprKind::Static(sym)
            | hir::ExprKind::FnItem { sym, .. } => self.qualify(*sym),
            hir::ExprKind::AssocConst { owner, name } => {
                format!("{}::{}()", self.qualify(*owner), sanitize(name))
            }
            hir::ExprKind::Field { base, index } => {
                let name = self.field_name(*base, *index);
                // `Error.location` is a `&SourceLocation` in Dazzle and a pointer into static
                // storage in the shim, so reading it yields the referent (spec §9.2).
                let deref =
                    matches!(body.ty(*base).peeled(), Ty::Builtin(dz_hir::ty::BuiltinTy::Error))
                        && name == "location";
                let b = self.expr(*base);
                if deref {
                    format!("(*::dz::obj({b}).{name})")
                } else {
                    format!("::dz::obj({b}).{name}")
                }
            }
            hir::ExprKind::TupleIndex { base, index } => {
                let b = self.expr(*base);
                format!("::dz::obj({b}).v{index}")
            }
            hir::ExprKind::SliceRange { base, start, end } => {
                let base_ty = self.cur().ty(*base).peeled_refs().clone();
                let slice_ty = self.cpp_ty(body.ty(e));
                let operands: Vec<ExprId> =
                    std::iter::once(*base).chain(*start).chain(*end).collect();
                let mut places = vec![false; operands.len()];
                places[0] = true;
                let mut v = self.ordered_operands(&operands, &places).into_iter();
                let b = v.next().unwrap_or_default();
                let lo = match start {
                    Some(_) => format!("static_cast<size_t>({})", v.next().unwrap_or_default()),
                    None => "0".to_owned(),
                };
                let hi =
                    end.map(|_| format!("static_cast<size_t>({})", v.next().unwrap_or_default()));
                if matches!(base_ty, Ty::Ptr { .. } | Ty::OwnPtr { .. }) {
                    // Unchecked, as indexing a pointer is (spec §5.3).
                    let hi = hi.unwrap_or_else(|| lo.clone());
                    return format!("{slice_ty}(({b}) + {lo}, {hi} - {lo})");
                }
                let loc = self.source_location(node.span);
                let base = self.through(*base, b);
                let hi = hi.unwrap_or_else(|| format!("::dz::len({base})"));
                format!("{slice_ty}(::dz::subslice({base}, {lo}, {hi}, {loc}))")
            }
            hir::ExprKind::Index { base, index } => {
                // Indexing a `^T` is unchecked pointer arithmetic, as in C: a pointer has no
                // length to check against, which is why the checker keeps it apart from
                // arrays and slices.
                let raw_pointer = matches!(
                    self.cur().ty(*base).peeled_refs(),
                    Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. }
                        if !matches!(**inner, Ty::Array { .. } | Ty::Slice { .. })
                );
                let v = self.ordered_operands(&[*base, *index], &[true, false]);
                let (b, i) = (v[0].clone(), v[1].clone());
                if raw_pointer {
                    return format!("({b})[static_cast<size_t>({i})]");
                }
                let loc = self.source_location(node.span);
                {
                    let base = self.through(*base, b);
                    format!("::dz::index({base}, static_cast<size_t>({i}), {loc})")
                }
            }
            hir::ExprKind::Deref(inner) => {
                let is_ref = matches!(body.ty(*inner), Ty::Ref { .. });
                let v = self.expr(*inner);
                // A `&T` read as a `T` (spec §18.35) is a C++ reference already; using it
                // where a value is wanted is the copy.
                if is_ref {
                    v
                } else {
                    format!("(*({v}))")
                }
            }
            // `&x` yields a proxy convertible to both `T&` and `T*` (spec §5.2).
            hir::ExprKind::Ref { place, .. } => {
                let v = self.expr(*place);
                // `map.get(&5)`: a reference to a value that is not a place refers to a
                // temporary holding it, for the rest of the block — C++ will not take the
                // address of a literal or of a call's result.
                let is_place = matches!(
                    body.expr(*place).kind,
                    hir::ExprKind::Local(_)
                        | hir::ExprKind::Static(_)
                        | hir::ExprKind::Field { .. }
                        | hir::ExprKind::TupleIndex { .. }
                        | hir::ExprKind::Index { .. }
                        | hir::ExprKind::Deref(_)
                ) || matches!(body.ty(*place), Ty::Ref { .. });
                if !is_place {
                    let tmp = self.fresh("ref");
                    let ty = self.cpp_ty(body.ty(*place));
                    self.line(&format!("{ty} {tmp} = {v};"));
                    return format!("::dz::addr({tmp})");
                }
                format!("::dz::addr({v})")
            }

            hir::ExprKind::Unary { op, operand } => {
                let o = self.expr(*operand);
                self.narrow_result(e, format!("{}({o})", unary_op(*op)))
            }
            hir::ExprKind::Binary { op, lhs, rhs } => {
                let text = self.binary(*op, *lhs, *rhs);
                self.narrow_result(e, text)
            }
            hir::ExprKind::Cast { operand } => {
                let t = self.cpp_ty(body.ty(e));
                let v = self.expr(*operand);
                format!("::dz::cast<{t}>({v})")
            }
            // A range held as a value (spec §10.4): the shim's `Range<T>`, which a `for` over
            // it then counts through. It takes both ends to have a value to hold.
            hir::ExprKind::Range { start: Some(lo), end: Some(hi), inclusive } => {
                let t = self.cpp_ty(body.ty(e).element().unwrap_or(&Ty::Int(IntTy::I32)));
                let v = self.sequenced_elements(&[*lo, *hi]);
                format!("::dz::Range<{t}>{{{}, {}, {inclusive}}}", v[0], v[1])
            }
            hir::ExprKind::Range { .. } => {
                self.unsupported(
                    node.span,
                    "an open-ended range outside a `for` loop",
                    "owed by M12",
                );
                "{}".to_owned()
            }

            hir::ExprKind::StructLit { ty, fields } => {
                self.struct_literal(*ty, fields, body.ty(e).clone())
            }
            hir::ExprKind::Tuple(parts) => {
                let t = self.cpp_ty(body.ty(e));
                let vs = self.sequenced_elements(parts);
                format!("{t}{{{}}}", vs.join(", "))
            }
            hir::ExprKind::TypeQuery { ty, query } => self.type_query(ty, *query),
            hir::ExprKind::Reflect { ty, info } => self.reflect_expr(ty, *info),
            hir::ExprKind::Len(base) => {
                let b = self.expr(*base);
                {
                    let base = self.through(*base, b);
                    format!("::dz::len({base})")
                }
            }
            hir::ExprKind::DataPtr(base) => {
                let b = self.expr(*base);
                {
                    let base = self.through(*base, b);
                    format!("::dz::data({base})")
                }
            }
            // A variadic call with no variadic arguments passes an empty slice (spec §7.2):
            // there is no `Array<T, 0>` to view.
            hir::ExprKind::ArrayLit(parts)
                if parts.is_empty() && matches!(body.ty(e), Ty::Slice { .. }) =>
            {
                format!("{}()", self.cpp_ty(body.ty(e)))
            }
            hir::ExprKind::ArrayLit(parts) => {
                let t = self.cpp_ty(body.ty(e));
                let vs = self.sequenced_elements(parts);
                format!("{t}{{{{{}}}}}", vs.join(", "))
            }
            hir::ExprKind::EnumVariant { owner, index } => {
                let name = self.variant_name(*owner, *index);
                format!("{}::{name}", self.qualify(*owner))
            }
            hir::ExprKind::UnionVariant { owner, index, payload } => {
                self.union_literal(*owner, *index, &payload.clone(), body.ty(e).clone())
            }

            hir::ExprKind::Call { callee, args } => self.call(callee, args, body.ty(e).clone()),
            hir::ExprKind::Move(inner) => {
                /// Something `take` can reset: a local, or a field of one.
                fn is_place(body: &hir::Body, e: hir::ExprId) -> bool {
                    match &body.expr(e).kind {
                        hir::ExprKind::Local(_) => true,
                        hir::ExprKind::Field { base, .. }
                        | hir::ExprKind::TupleIndex { base, .. } => is_place(body, *base),
                        _ => false,
                    }
                }
                let v = self.expr(*inner);
                if !self.table.copyable(body.ty(e)) {
                    // The type's move constructor resets the source (spec §6.4).
                    format!("::dz::mv({v})")
                } else if is_place(body, *inner) {
                    // A copyable type has no move constructor to do the resetting, so the
                    // shim does it: the source is left at its defaults like any other
                    // moved-from value (spec §6.4, §18.33).
                    format!("::dz::take({v})")
                } else {
                    // A temporary leaves nothing behind to reset.
                    v
                }
            }

            hir::ExprKind::Block(_)
            | hir::ExprKind::If { .. }
            | hir::ExprKind::IfLet { .. }
            | hir::ExprKind::Switch { .. } => {
                let ty = body.ty(e).clone();
                if matches!(ty, Ty::Void | Ty::Never | Ty::Error) {
                    self.control_flow(e, Dest::None);
                    return String::new();
                }
                let tmp = self.fresh("val");
                // A reference cannot be declared empty and assigned later, so a block that
                // yields one — `let p: &P = unsafe { &xs^[i] }` — is held as a pointer, as
                // `or` does below.
                if let Ty::Ref { mutable, inner } = &ty {
                    let ptr = self.ptr_ty(*mutable, inner);
                    self.line(&format!("{ptr} {tmp} = nullptr;"));
                    self.control_flow(e, Dest::Ptr(&tmp));
                    return format!("(*{tmp})");
                }
                let cpp = self.cpp_ty(&ty);
                self.line(&format!("{cpp} {tmp}{{}};"));
                let dest = Dest::Var(&tmp);
                self.control_flow(e, dest);
                // The temporary is only a way to get the value out of the statements, so a
                // move-only value leaves it by moving (spec §6.4).
                if self.table.copyable(&ty) {
                    tmp
                } else {
                    format!("::dz::mv({tmp})")
                }
            }
            // The guard is a line of the current C++ block, so it lives until that block
            // ends — past the `or` or `if let` that reads the pointer (spec §14.5).
            hir::ExprKind::Guarded { guard, init, value } => {
                let (guard, init, value) = (*guard, *init, *value);
                let cpp = self.cpp_ty(&body.local(guard).ty.clone());
                let name = self.names[guard.index()].clone();
                let v = self.expr(init);
                self.line(&format!("{cpp} {name} = {v};"));
                self.expr(value)
            }
            hir::ExprKind::Or { operand, clause } => {
                let ty = body.ty(e).clone();
                if matches!(ty, Ty::Void | Ty::Never | Ty::Error) {
                    self.or_expr(e, *operand, clause, Dest::None);
                    return String::new();
                }
                let cpp = self.cpp_ty(&ty);
                let is_ref = matches!(ty, Ty::Ref { .. });
                let tmp = self.fresh("ok");
                if is_ref {
                    // A reference cannot be reseated, so the result is held as a pointer
                    // of the same writability until the clause has run.
                    let ptr = match &ty {
                        Ty::Ref { mutable, inner } => self.ptr_ty(*mutable, inner),
                        _ => format!("{cpp}*"),
                    };
                    self.line(&format!("{ptr} {tmp} = nullptr;"));
                    self.or_expr(e, *operand, clause, Dest::Ptr(&tmp));
                    format!("(*{tmp})")
                } else {
                    self.line(&format!("{cpp} {tmp}{{}};"));
                    self.or_expr(e, *operand, clause, Dest::Var(&tmp));
                    if self.table.copyable(&ty) {
                        tmp
                    } else {
                        format!("::dz::mv({tmp})")
                    }
                }
            }

            hir::ExprKind::Return(v) => self.return_expr(*v),
            // To the innermost loop, C++'s own; further out, a jump to the label that
            // loop placed (spec §10.2).
            hir::ExprKind::Break(None) => "break".to_owned(),
            hir::ExprKind::Continue(None) => "continue".to_owned(),
            hir::ExprKind::Break(Some(id)) => format!("goto {}", loop_label_name("break", *id)),
            hir::ExprKind::Continue(Some(id)) => {
                format!("goto {}", loop_label_name("continue", *id))
            }

            hir::ExprKind::Builtin { name, args } => self.builtin(name, args, node.span),
            hir::ExprKind::Intrinsic { name, .. } => self.intrinsic(name, node.span),
        }
    }

    /// `&&` and `||` short-circuit, so a right operand that needs statements of its own
    /// cannot simply be hoisted — the statements would run unconditionally. It becomes an
    /// `if` instead, which keeps `f() or return` working inside a condition.
    /// An operator on an 8- or 16-bit integer, cast back to that type. C++ promotes both
    /// operands to `int` first, so `250u8 + 10` would be the `int` 260 wherever it is used
    /// before being stored; Dazzle's arithmetic wraps in its own type (spec §5.1, 18.63).
    /// `-fwrapv` already makes `int` and wider wrap, and a comparison is a `bool`.
    fn narrow_result(&self, e: ExprId, text: String) -> String {
        match self.cur().ty(e) {
            Ty::Int(i @ (IntTy::I8 | IntTy::U8 | IntTy::I16 | IntTy::U16)) => {
                format!("static_cast<{}>({text})", int_name(*i))
            }
            _ => text,
        }
    }

    fn binary(&mut self, op: BinaryOp, lhs: ExprId, rhs: ExprId) -> String {
        // `x == none` asks whether an optional is empty (spec §9): a test of its flag, since
        // `Opt` has no `==` — and `T` may not have one either. The checker allows no other
        // comparison on an optional.
        if matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
            let body = self.cur();
            let is_none = |x: ExprId| matches!(body.expr(x).kind, hir::ExprKind::NoneOpt);
            let subject = if is_none(rhs) {
                Some(lhs)
            } else if is_none(lhs) {
                Some(rhs)
            } else {
                None
            };
            if let Some(s) = subject {
                let v = self.expr(s);
                let not = if matches!(op, BinaryOp::Eq) { "!" } else { "" };
                return format!("({not}({v}).has)");
            }
        }
        let sym = match op {
            // `^^` is Dazzle's xor; C++ spells it `^` (spec §2.6).
            BinaryOp::BitXor => "^",
            other => other.as_str(),
        };
        if !matches!(op, BinaryOp::And | BinaryOp::Or) {
            // Left operand first (spec §7.2, 18.49): C++ fixes the order of neither.
            let v = self.ordered_operands(&[lhs, rhs], &[false, false]);
            return format!("({} {sym} {})", v[0], v[1]);
        }
        // `&&` and `||` are ordered by C++ itself; what is left is the right operand's own
        // statements, which must run only when it does.
        let a = self.expr(lhs);
        let (pre, b) = self.capture(|e| e.expr(rhs));
        if !pre.trim().is_empty() {
            let tmp = self.fresh("sc");
            self.line(&format!("bool {tmp} = false;"));
            let (open, other) = match op {
                // `a` is often parenthesised already; doubled parentheses around `==` are
                // what clang warns about as a mistyped assignment.
                BinaryOp::And => (format!("if ({}) {{", unwrap_parens(&a)), "true"),
                _ => (format!("if (!({a})) {{"), "false"),
            };
            let _ = other;
            self.line(&open);
            self.indent += 1;
            self.write_captured(&pre);
            self.line(&format!("{tmp} = {b};"));
            self.indent -= 1;
            self.line("} else {");
            self.indent += 1;
            self.line(&format!("{tmp} = {};", matches!(op, BinaryOp::Or)));
            self.indent -= 1;
            self.line("}");
            return tmp;
        }
        format!("({a} {sym} {b})")
    }

    fn field_name(&mut self, base: ExprId, index: usize) -> String {
        let body = self.cur();
        let unknown = || format!("_{index}");
        match body.ty(base).peeled() {
            Ty::Named { sym, .. } => self
                .table
                .struct_sig(*sym)
                .and_then(|s| s.fields.get(index))
                .map(|f| sanitize(&f.name))
                .unwrap_or_else(unknown),
            Ty::Builtin(b) => {
                b.fields().get(index).map(|(n, _)| (*n).to_owned()).unwrap_or_else(unknown)
            }
            // A `case Circle(c)` binding reaches into the variant's own field list.
            Ty::Variant(sym, v, _) => match self.table.get(*sym) {
                Some(ItemSig::Union { variants }) => variants
                    .get(*v)
                    .and_then(|x| x.fields.get(index))
                    .map(|f| sanitize(&f.name))
                    .unwrap_or_else(unknown),
                _ => unknown(),
            },
            _ => unknown(),
        }
    }

    fn variant_name(&self, owner: SymbolId, index: usize) -> String {
        match self.table.get(owner) {
            Some(ItemSig::Enum { variants, .. }) | Some(ItemSig::Union { variants }) => {
                variants.get(index).map(|v| sanitize(&v.name)).unwrap_or_default()
            }
            _ => String::new(),
        }
    }

    /// `node_ty` is the literal's own type, so `Pair<i32>{…}` names its instance.
    fn struct_literal(&mut self, sym: SymbolId, fields: &[Option<ExprId>], node_ty: Ty) -> String {
        let ty = self.cpp_ty(&node_ty);
        if fields.iter().all(Option::is_none) {
            return format!("{ty}{{}}");
        }
        let order = self.field_list(sym);
        let values = self.literal_fields(fields, |i| order.get(i).map(|f| f.1.clone()), true);

        // C++20 designated initialisers read far better, but they apply only to
        // aggregates — a type with a constructor or a user special member is not one, and
        // neither is one with a base or a `virtual` method (spec §7.3, 18.32): a base
        // subobject would have to be initialised first, and the field order written here
        // does not account for it.
        let aggregate = self.table.struct_sig(sym).is_some_and(|s| {
            s.ctors.is_empty()
                && s.copy.is_none()
                && s.drop.is_none()
                && s.base.is_none()
                && !s.polymorphic
        }) && self.table.copyable(&node_ty);
        if self.standard >= 20 && aggregate {
            let mut parts = Vec::new();
            for (i, v) in &values {
                parts.push(format!(".{} = {v}", order[*i].0));
            }
            return format!("{ty}{{ {} }}", parts.join(", "));
        }

        // C++17 has no designated initialisers, so build the value in a lambda. It costs
        // nothing after inlining and keeps field defaults working.
        let tmp = self.fresh("lit");
        let mut out = format!("[&] {{ {ty} {tmp}{{}}; ");
        for (i, v) in &values {
            let _ = write!(out, "{tmp}.{} = {v}; ", order[*i].0);
        }
        let _ = write!(out, "return {tmp}; }}()");
        out
    }

    fn union_literal(
        &mut self,
        owner: SymbolId,
        index: usize,
        payload: &[Option<ExprId>],
        // The literal's own type, so `Maybe<i32>::Some{…}` names its instance.
        node_ty: Ty,
    ) -> String {
        let ty = self.cpp_ty(&node_ty);
        let name = self.variant_name(owner, index);
        // The tag and the payload must be set together, and C++ has nowhere to write two
        // statements inside an expression, so this is the one place a lambda is right:
        // no `return` can appear inside a variant literal's field values.
        let tmp = self.fresh("var");
        // A payload with a destructor or a constructor must be built where it goes, after
        // the first variant's is gone: `become` does both (spec §7.5, 18.52). Plain data
        // needs only the tag set.
        let union_args = match &node_ty {
            Ty::Named { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        let switch_to = if self.table.union_is_trivial(owner, &union_args) {
            format!("{tmp}.kind = {ty}::Kind::{name};")
        } else {
            format!("{tmp}.become({ty}::Kind::{name});")
        };
        let mut out = format!("[&] {{ {ty} {tmp}{{}}; {switch_to} ");
        let carries = match self.table.get(owner) {
            Some(ItemSig::Union { variants }) => variants.get(index).and_then(|v| v.carries),
            _ => None,
        };
        // A carried struct is built whole, so its field defaults run: the union's own
        // constructor built only its first member, and this one may not be that.
        if let Some(carried) = carries {
            let value = self.struct_literal(carried, payload, Ty::plain(carried));
            let _ = write!(out, "{tmp}.{name} = {value}; return {tmp}; }}()");
            return out;
        }
        let fields = match self.table.get(owner) {
            Some(ItemSig::Union { variants }) => variants
                .get(index)
                .map(|v| v.fields.iter().map(|f| sanitize(&f.name)).collect::<Vec<_>>())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        for (i, text) in self.literal_fields(payload, |_| None, true) {
            let Some(f) = fields.get(i) else { continue };
            let _ = write!(out, "{tmp}.{name}.{f} = {text}; ");
        }
        let _ = write!(out, "return {tmp}; }}()");
        out
    }

    /// A literal's field values, in declaration order, each with its field's index — but
    /// **evaluated in the order they are written** (spec §7.2, 18.49). The HIR holds fields
    /// in declaration order, which is what C++ must build them in; `Point{y: f(), x: g()}`
    /// still runs `f()` first, found from where each value sits in the source.
    fn literal_fields(
        &mut self,
        fields: &[Option<ExprId>],
        target: impl Fn(usize) -> Option<Ty>,
        consume: bool,
    ) -> Vec<(usize, String)> {
        let body = self.cur();
        let mut written: Vec<(usize, ExprId)> =
            fields.iter().enumerate().filter_map(|(i, f)| Some((i, (*f)?))).collect();
        let declared: Vec<usize> = written.iter().map(|w| w.0).collect();
        written.sort_by_key(|(_, f)| body.expr(*f).span.start);
        let in_declared_order = written.iter().map(|w| w.0).eq(declared.iter().copied());
        let mut captured =
            self.capture_each(written.iter().map(|&(i, f)| (f, target(i))).collect(), consume);
        // C++ builds the fields in declaration order; where that is also the written order,
        // only hoisted statements can put a value out of turn.
        captured.sequenced = in_declared_order;
        let values = self.emit_args(captured);
        let mut out: Vec<(usize, String)> = written.iter().map(|w| w.0).zip(values).collect();
        out.sort_by_key(|(i, _)| *i);
        out
    }

    /// `node_ty` is the call's own type, which is what says *which* instance a generic
    /// struct's constructor builds.
    fn call(&mut self, callee: &hir::Callee, args: &[ExprId], node_ty: Ty) -> String {
        if let hir::Callee::Method { recv, owner, name, .. } = callee {
            if let Some(native) = self.native_trait_call(*recv, *owner, name, args) {
                return native;
            }
        }
        // The receiver, or the function value, is evaluated first (spec §7.2, 18.49). C++17
        // already sequences a call's postfix expression before its arguments, so this only
        // has to protect it from an argument's hoisted statements, or — for a trait method,
        // where it becomes an argument itself — from an argument's side effects.
        let head = match callee {
            hir::Callee::Method { recv, .. } | hir::Callee::BaseMethod { recv, .. } => Some(*recv),
            hir::Callee::Value(v) => Some(*v),
            _ => None,
        };
        let head_text = head.map(|h| self.expr(h));
        let captured = self.capture_args(args, true);
        let head_text = match (head, head_text) {
            (Some(h), Some(text)) if captured.any_effects() && !self.fixed_place(h) => {
                let tmp = self.fresh("recv");
                self.line(&format!("auto&& {tmp} = {text};"));
                Some(tmp)
            }
            (_, text) => text,
        };
        let recv_text = || head_text.clone().expect("a method call has a receiver");

        // A trait's method is an implementation's free function, and the receiver its
        // first argument (spec §7.6).
        let mut recv_arg: Option<String> = None;
        let target = match callee {
            hir::Callee::Fn { sym, type_args } if !type_args.is_empty() => {
                self.qualify_instance(*sym, type_args)
            }
            hir::Callee::Fn { sym, .. } => self.qualify(*sym),
            // A generic method's instance is a free function (spec §8.4).
            hir::Callee::Assoc { owner, type_args, name, method_args }
                if !method_args.is_empty() =>
            {
                self.qualify_method_instance(*owner, type_args, name, method_args)
            }
            // A C++ `static` member function (spec §7.3).
            hir::Callee::Assoc { owner, type_args, name, .. } => {
                let ty = if type_args.is_empty() {
                    self.qualify(*owner)
                } else {
                    self.qualify_instance(*owner, type_args)
                };
                format!("{ty}::{}", sanitize(name))
            }
            hir::Callee::Ctor { owner, .. } => match &node_ty {
                Ty::Named { args, .. } if !args.is_empty() => self.qualify_instance(*owner, args),
                _ => self.qualify(*owner),
            },
            // `UiElement::draw(self)` is C++'s `recv.UiElement::draw()`, which is exactly
            // the qualified call that suppresses virtual dispatch (spec §7.3, 18.32).
            hir::Callee::BaseMethod { owner, name, .. } => {
                let base = self.qualify(*owner);
                let r = recv_text();
                format!("::dz::obj({r}).{base}::{}", sanitize(name))
            }
            // The receiver becomes the first argument, as for an implementation's function.
            hir::Callee::Method { recv, owner, name, method_args } if !method_args.is_empty() => {
                let owner_args = match self.cur().ty(*recv).peeled() {
                    Ty::Named { sym, args } if sym == owner => args.clone(),
                    _ => Vec::new(),
                };
                recv_arg = Some(format!("::dz::obj({})", recv_text()));
                self.qualify_method_instance(*owner, &owner_args, name, method_args)
            }
            hir::Callee::Method { recv, owner, name, .. } => {
                let target = self.impl_target(*recv, *owner, name, args);
                let r = recv_text();
                match target {
                    Some(f) => {
                        recv_arg = Some(format!("::dz::obj({r})"));
                        f
                    }
                    None => format!("::dz::obj({r}).{}", sanitize(name)),
                }
            }
            hir::Callee::Value(_) => format!("({})", recv_text()),
        };
        // An imported function may want a C type Dazzle spells differently — `char *`
        // against `int8_t *` — so the crossing is cast, in both directions (spec §15.2).
        let imported = match callee {
            hir::Callee::Fn { sym, .. } if self.is_foreign(*sym) => match self.table.get(*sym) {
                Some(ItemSig::Fn(sig)) => Some(sig.clone()),
                _ => None,
            },
            // A member function of an imported C++ class crosses the same boundary as a
            // free one, and wants the same casts (spec §15.2). The call node carries the
            // name, not the overload key, so this applies only where the name names one
            // method — and an overload set's members differ in their parameter types, which
            // is exactly when guessing would cast against the wrong one.
            hir::Callee::Method { owner, name, .. } if self.is_foreign(*owner) => {
                match self.table.struct_sig(*owner) {
                    Some(s) => match s.overloads.get(name).map(Vec::as_slice) {
                        Some([key]) => s.methods.get(key).cloned(),
                        _ => None,
                    },
                    None => None,
                }
            }
            hir::Callee::Ctor { owner, index } if self.is_foreign(*owner) => {
                self.table.struct_sig(*owner).and_then(|s| s.ctors.get(*index)).cloned()
            }
            _ => None,
        };

        let mut rendered: Vec<String> = recv_arg.into_iter().collect();
        for (i, v) in self.emit_args(captured).into_iter().enumerate() {
            let cast =
                imported.as_ref().and_then(|s| s.params.get(i)).and_then(|p| p.c_cast.clone());
            rendered.push(match cast {
                Some(c) => format!("({c})({v})"),
                None => v,
            });
        }
        let call = format!("{target}({})", rendered.join(", "));
        match imported {
            Some(sig) if sig.ret_cast => {
                let t = self.cpp_ty(&node_ty);
                format!("({t})({call})")
            }
            _ => call,
        }
    }

    // --- argument order (spec §7.2, 18.49) -----------------------------------
    //
    // Dazzle evaluates arguments left to right; C++ leaves the order unspecified, and g++ and
    // clang++ targeting Windows both go right to left. Two things can make the order show:
    // an argument's side effects, and an argument's hoisted statements — an `or` clause or a
    // block value writes its work *before* the statement, ahead of every argument to its
    // left. So each argument is rendered on its own first, and then written out in order:
    // its hoisted statements, and — only where an earlier argument could see a later one's
    // effects, or the other way round — its value bound to a temporary. A call whose
    // arguments are plain reads, or that has one argument with effects and nothing else
    // that could observe them, comes out exactly as it did before.

    /// Render every argument without writing anything yet.
    fn capture_args(&mut self, args: &[ExprId], consume: bool) -> CapturedArgs {
        self.capture_each(args.iter().map(|&a| (a, None)).collect(), consume)
    }

    /// Operands C++ gives no order — `a() + b()`, `xs[i()]` — evaluated left to right
    /// (spec §7.2, 18.49). `places` marks the ones that are the place itself rather than a
    /// value, such as an indexed container: held by reference, not copied, if they must be
    /// held at all. An arithmetic operand is a value, so a later operand's effect must not
    /// show through it.
    fn ordered_operands(&mut self, operands: &[ExprId], places: &[bool]) -> Vec<String> {
        let mut captured = self.capture_args(operands, false);
        for (a, &place) in captured.args.iter_mut().zip(places) {
            a.place = place;
        }
        self.emit_args(captured)
    }

    /// Elements C++ already evaluates in order — a braced initialiser's — so only what
    /// they hoist can run out of turn (spec §7.2, 18.49).
    fn sequenced_elements(&mut self, elements: &[ExprId]) -> Vec<String> {
        let mut captured = self.capture_args(elements, false);
        captured.sequenced = true;
        self.emit_args(captured)
    }

    /// Each argument with, where it is stored into a declared type, that type — which
    /// decides how a `&T` built by a block is held (see [`Emitter::stored`]).
    fn capture_each(&mut self, args: Vec<(ExprId, Option<Ty>)>, consume: bool) -> CapturedArgs {
        let mut out = CapturedArgs::default();
        for (a, target) in args {
            let (pre, v) = self.capture(|e| match &target {
                Some(t) => e.stored(t, a),
                None => e.expr(a),
            });
            // A move-only value is handed over; a copyable one is copied by C++ itself.
            let v = if consume && !self.table.copyable(self.cur().ty(a)) {
                format!("::dz::mv({v})")
            } else {
                v
            };
            let effects = !pre.trim().is_empty() || self.has_effects(a);
            let stable = !effects && self.order_stable(a);
            out.args.push(CapturedArg { id: a, pre, value: v, effects, stable, place: false });
        }
        out
    }

    /// Write the captured arguments' hoisted work in order, binding a value to a temporary
    /// where the order could otherwise be observed, and return what each argument is.
    fn emit_args(&mut self, captured: CapturedArgs) -> Vec<String> {
        let args = &captured.args;
        let mut out = Vec::with_capacity(args.len());
        for (i, a) in args.iter().enumerate() {
            self.write_captured(&a.pre);
            let later = &args[i + 1..];
            let observable = if captured.sequenced {
                !a.stable && later.iter().any(|b| !b.pre.trim().is_empty())
            } else {
                later.iter().any(|b| (a.effects && !b.stable) || (!a.stable && b.effects))
            };
            if !observable {
                out.push(a.value.clone());
                continue;
            }
            let tmp = self.fresh("arg");
            let ty = self.cur().ty(a.id).clone();
            if a.place {
                // `construct(dst, move_out(src))` and `xs[i()]` work on the place itself:
                // bound as it is, and forwarded as the lvalue or rvalue it was.
                self.line(&format!("auto&& {tmp} = {};", a.value));
                out.push(format!("static_cast<decltype({tmp})>({tmp})"));
            } else if matches!(ty, Ty::Ref { .. }) {
                // A reference is evaluated once it names its referent; what the referent
                // holds may still change, as it would in any language.
                self.line(&format!("auto&& {tmp} = {};", a.value));
                out.push(tmp);
            } else {
                let t = self.cpp_ty(&ty);
                self.line(&format!("{t} {tmp} = {};", a.value));
                // The temporary is never read again, so it is handed over, not copied.
                out.push(format!("::dz::mv({tmp})"));
            }
        }
        out
    }

    /// Whether evaluating `e` can change what another expression reads, or reach outside
    /// the expression at all: a call, anything that writes statements of its own, a jump.
    /// A failed bounds check is not counted — it ends the program either way.
    fn has_effects(&self, e: ExprId) -> bool {
        let body = self.cur();
        let any = |xs: &[ExprId]| xs.iter().any(|x| self.has_effects(*x));
        let opt = |x: &Option<ExprId>| x.is_some_and(|x| self.has_effects(x));
        match &body.expr(e).kind {
            hir::ExprKind::Call { .. }
            | hir::ExprKind::Builtin { .. }
            | hir::ExprKind::Block(_)
            | hir::ExprKind::If { .. }
            | hir::ExprKind::IfLet { .. }
            | hir::ExprKind::Switch { .. }
            | hir::ExprKind::Or { .. }
            | hir::ExprKind::Guarded { .. }
            | hir::ExprKind::Return(_)
            | hir::ExprKind::Break(_)
            | hir::ExprKind::Continue(_)
            | hir::ExprKind::Error => true,
            hir::ExprKind::Int(_)
            | hir::ExprKind::Float(_)
            | hir::ExprKind::Bool(_)
            | hir::ExprKind::Char(_)
            | hir::ExprKind::Str(_)
            | hir::ExprKind::Null
            | hir::ExprKind::NoneOpt
            | hir::ExprKind::Local(_)
            | hir::ExprKind::Const(_)
            | hir::ExprKind::Static(_)
            | hir::ExprKind::AssocConst { .. }
            | hir::ExprKind::FnItem { .. }
            | hir::ExprKind::EnumVariant { .. }
            | hir::ExprKind::TypeQuery { .. }
            | hir::ExprKind::Reflect { .. }
            | hir::ExprKind::Intrinsic { .. } => false,
            hir::ExprKind::Field { base, .. }
            | hir::ExprKind::TupleIndex { base, .. }
            | hir::ExprKind::Len(base)
            | hir::ExprKind::DataPtr(base)
            | hir::ExprKind::Deref(base)
            | hir::ExprKind::Move(base)
            | hir::ExprKind::Ref { place: base, .. }
            | hir::ExprKind::Unary { operand: base, .. }
            | hir::ExprKind::Cast { operand: base } => self.has_effects(*base),
            hir::ExprKind::Index { base, index } => any(&[*base, *index]),
            hir::ExprKind::Binary { lhs, rhs, .. } => any(&[*lhs, *rhs]),
            hir::ExprKind::SliceRange { base, start, end } => {
                self.has_effects(*base) || opt(start) || opt(end)
            }
            hir::ExprKind::Range { start, end, .. } => opt(start) || opt(end),
            hir::ExprKind::StructLit { fields: xs, .. }
            | hir::ExprKind::UnionVariant { payload: xs, .. } => xs.iter().any(opt),
            hir::ExprKind::Tuple(xs) | hir::ExprKind::ArrayLit(xs) => any(xs),
        }
    }

    /// Whether `e` means the same thing wherever among the arguments it is evaluated: a
    /// constant, or the address of a place nothing can move.
    fn order_stable(&self, e: ExprId) -> bool {
        let body = self.cur();
        match &body.expr(e).kind {
            hir::ExprKind::Int(_)
            | hir::ExprKind::Float(_)
            | hir::ExprKind::Bool(_)
            | hir::ExprKind::Char(_)
            | hir::ExprKind::Str(_)
            | hir::ExprKind::Null
            | hir::ExprKind::NoneOpt
            | hir::ExprKind::Const(_)
            | hir::ExprKind::AssocConst { .. }
            | hir::ExprKind::FnItem { .. }
            | hir::ExprKind::EnumVariant { .. }
            | hir::ExprKind::TypeQuery { .. }
            | hir::ExprKind::Reflect { .. }
            | hir::ExprKind::Intrinsic { .. } => true,
            // A local reference cannot be re-seated, so it names the same thing throughout.
            hir::ExprKind::Local(_) => matches!(body.ty(e), Ty::Ref { .. }),
            hir::ExprKind::Ref { place, .. } => self.fixed_place(*place),
            hir::ExprKind::Cast { operand } | hir::ExprKind::Unary { operand, .. } => {
                self.order_stable(*operand)
            }
            hir::ExprKind::Binary { lhs, rhs, .. } => {
                self.order_stable(*lhs) && self.order_stable(*rhs)
            }
            _ => false,
        }
    }

    /// A place whose address no expression can change: a local or a static, or a field of
    /// one held by value. Reaching through a pointer or an index is not fixed, since either
    /// may be changed by an argument before it.
    fn fixed_place(&self, e: ExprId) -> bool {
        let body = self.cur();
        match &body.expr(e).kind {
            hir::ExprKind::Local(_) | hir::ExprKind::Static(_) => true,
            hir::ExprKind::Field { base, .. } | hir::ExprKind::TupleIndex { base, .. } => {
                matches!(body.ty(*base), Ty::Named { .. } | Ty::Tuple(_) | Ty::Ref { .. })
                    && self.fixed_place(*base)
            }
            _ => self.order_stable(e),
        }
    }

    /// `return`, in a body whose C++ return type may not be its Dazzle one (spec §9.3).
    fn return_expr(&mut self, value: Option<ExprId>) -> String {
        let body = self.cur();
        match value {
            Some(v) => {
                // `return error(...)` is the failure case, not a value of the success
                // type, so it takes the other constructor (spec §9.1).
                if matches!(body.ty(v), Ty::Fallible(_)) {
                    let ret = self.cpp_ty(&body.ret);
                    let e = self.expr(v);
                    return format!("return ::dz::Res<{ret}>::bad({e})");
                }
                let moved = !self.table.copyable(body.ty(v));
                let ret = body.ret.clone();
                let text = self.stored(&ret, v);
                let text = if moved { format!("::dz::mv({text})") } else { text };
                if body.fallible {
                    let ret = self.cpp_ty(&body.ret);
                    format!("return ::dz::Res<{ret}>::good({text})")
                } else {
                    format!("return {text}")
                }
            }
            None if body.fallible => "return ::dz::Res<void>::good()".to_owned(),
            None => "return".to_owned(),
        }
    }

    // --- the `or` clause (spec §9.4) ---------------------------------------

    /// Lower `operand or clause` into statements, leaving the success value in `dest`.
    fn or_expr(&mut self, e: ExprId, operand: ExprId, clause: &hir::OrClause, dest: Dest<'_>) {
        let body = self.cur();
        let span = body.expr(e).span;
        // A `&T?` is read as the optional it refers to, which the checker allows only for a
        // copyable one — so the `Opt` below copies it (18.35).
        let operand_ty = match body.ty(operand).clone() {
            Ty::Ref { inner, .. } if matches!(*inner, Ty::Optional(_)) => (*inner).clone(),
            t => t,
        };
        match operand_ty {
            Ty::Fallible(inner) => {
                let res_ty = self.cpp_ty(&inner);
                let tmp = self.fresh("res");
                let v = self.expr(operand);
                self.line(&format!("::dz::Res<{res_ty}> {tmp} = {v};"));
                self.line(&format!("if ({tmp}.ok) {{"));
                self.indent += 1;
                if !matches!(*inner, Ty::Void) {
                    self.move_or_copy_into(&format!("{tmp}.value"), &inner, dest);
                }
                self.indent -= 1;
                self.line("} else {");
                self.indent += 1;
                self.clause(clause, dest, Some(&format!("{tmp}.error")), span);
                self.indent -= 1;
                self.line("}");
            }
            Ty::Optional(inner) => {
                let opt_ty = self.cpp_ty(&inner);
                let tmp = self.fresh("opt");
                let v = self.expr(operand);
                // `t or …` on a move-only optional consumes `t`, as ownership analysis
                // already says it does (spec §6.4); a copy would not compile.
                let v = if self.table.copyable(&inner) { v } else { format!("::dz::mv({v})") };
                self.line(&format!("::dz::Opt<{opt_ty}> {tmp} = {v};"));
                self.line(&format!("if ({tmp}.has) {{"));
                self.indent += 1;
                self.move_or_copy_into(&format!("{tmp}.value"), &inner, dest);
                self.indent -= 1;
                self.line("} else {");
                self.indent += 1;
                self.clause(clause, dest, None, span);
                self.indent -= 1;
                self.line("}");
            }
            // `^T or …` is the checked pointer-to-reference conversion of §5.2.
            Ty::Ptr { mutable, inner } | Ty::OwnPtr { mutable, inner } => {
                let ptr = self.ptr_ty(mutable, &inner);
                let tmp = self.fresh("ptr");
                let v = self.expr(operand);
                self.line(&format!("{ptr} {tmp} = {v};"));
                self.line(&format!("if ({tmp} != nullptr) {{"));
                self.indent += 1;
                match dest {
                    Dest::Ptr(name) => self.line(&format!("{name} = {tmp};")),
                    Dest::Var(name) => self.line(&format!("{name} = (*{tmp});")),
                    Dest::None => {}
                }
                self.indent -= 1;
                self.line("} else {");
                self.indent += 1;
                self.clause(clause, dest, None, span);
                self.indent -= 1;
                self.line("}");
            }
            _ => {
                // The checker reported that this cannot fail; emit the operand so any
                // side effect it has still happens.
                self.stmt_expr(operand);
            }
        }
    }

    fn move_or_copy_into(&mut self, source: &str, ty: &Ty, dest: Dest<'_>) {
        match dest {
            Dest::None => {}
            Dest::Var(name) => {
                if self.table.copyable(ty) {
                    self.line(&format!("{name} = {source};"));
                } else {
                    self.line(&format!("{name} = ::dz::mv({source});"));
                }
            }
            Dest::Ptr(name) => self.line(&format!("{name} = &({source});")),
        }
    }

    /// The right-hand side of an `or`: a value, a block, `ignore` or `unreachable`.
    fn clause(&mut self, clause: &hir::OrClause, dest: Dest<'_>, error: Option<&str>, span: Span) {
        let body = self.cur();
        match clause {
            hir::OrClause::Value(v) => {
                // Bare `or return` propagates the failure rather than returning nothing
                // (spec §9.5), so it is not the `return` the node says it is.
                if matches!(body.expr(*v).kind, hir::ExprKind::Return(None)) {
                    self.propagate(error);
                    return;
                }
                if matches!(body.ty(*v), Ty::Never) {
                    self.stmt_expr(*v);
                } else {
                    self.assign_into(*v, dest);
                }
            }
            hir::OrClause::Block { binding, body: block } => {
                if let (Some(local), Some(err)) = (binding, error) {
                    let name = self.names[local.index()].clone();
                    let unused = self.unused_attr(local.index());
                    self.line(&format!("{unused}const ::dz::Error& {name} = {err};"));
                }
                self.block_into(*block, dest);
            }
            hir::OrClause::Ignore => {}
            // A debug build panics with the location; release and shipping assume the
            // success path, so the optimiser drops the check (spec §9.4, 18.20).
            hir::OrClause::Unreachable => {
                if self.ws.module(self.ws.root).build.optimize == dz_project::Optimize::Debug {
                    let loc = self.source_location(span);
                    self.line(&format!("::dz::unreachable({loc});"));
                } else {
                    self.line("__builtin_unreachable();");
                }
            }
        }
    }

    /// `or return`: hand the failure to the caller in whatever shape this signature has.
    fn propagate(&mut self, error: Option<&str>) {
        let body = self.cur();
        let ret = self.cpp_ty(&body.ret);
        match (body.fallible, error) {
            (true, Some(err)) => self.line(&format!("return ::dz::Res<{ret}>::bad({err});")),
            // A `T?` or `^T` failing inside a `!->` function is not an `Error`, so the
            // only thing to report is success-with-nothing (spec §9.5).
            (true, None) if matches!(body.ret, Ty::Void) => {
                self.line("return ::dz::Res<void>::good();")
            }
            (true, None) => self.line(&format!("return ::dz::Res<{ret}>();")),
            (false, _) => match &body.ret {
                Ty::Optional(_) => self.line(&format!("return {ret}();")),
                Ty::Void | Ty::Error => self.line("return;"),
                _ => self.line(&format!("return {ret}{{}};")),
            },
        }
    }

    // --- builtins and intrinsics -------------------------------------------

    fn builtin(&mut self, name: &str, args: &[ExprId], span: Span) -> String {
        // In order, as any call's are: `println("{} {}", touch(), COUNT.hits)` is §7.2's own
        // example. A builtin takes its arguments as they are, so nothing is moved into it.
        let mut captured = self.capture_args(args, false);
        if matches!(name, "construct" | "destroy" | "move_out") {
            for a in &mut captured.args {
                a.place = true;
            }
        }
        let rendered = self.emit_args(captured);
        match name {
            // The checker verified the argument counts of everything below (`builtin_arity`),
            // so indexing `rendered` cannot go out of range.
            "alloc" => {
                let loc = self.source_location(span);
                format!("::dz::alloc({}, {}, {loc})", rendered[0], rendered[1])
            }
            "free" => format!("::dz::dealloc({})", rendered[0]),
            "construct" => format!("::dz::construct({})", rendered.join(", ")),
            "destroy" => format!("::dz::destroy({})", rendered[0]),
            "move_out" => format!("::dz::move_out({})", rendered[0]),
            "str_from_utf8" => format!("::dz::str_from_utf8({})", rendered[0]),
            // `typeinfo(id)` reads the table codegen writes (spec §13.1).
            "typeinfo" => self.typeinfo_by_id(&rendered[0]),
            "memcpy" => format!("::memcpy({}, {}, {})", rendered[0], rendered[1], rendered[2]),
            // The optimiser barrier a benchmark needs (spec §19 M15).
            "blackbox" => format!("::dz::blackbox({})", rendered[0]),
            "println" | "print" | "eprintln" | "eprint" => {
                format!("::dz::{name}({})", rendered.join(", "))
            }
            "format" => format!("::dz::format({})", rendered.join(", ")),
            // `panic` formats exactly as `println` does (spec §9.8, §12.9), so `{}` and `{{`
            // mean the same thing in both; the checker guaranteed a format string is there.
            // A final `SourceLocation` argument is where the panic says it happened, in place
            // of its own line: `assert` passes its caller's (spec §9.8, §12.7).
            "panic" => {
                let given = args.last().is_some_and(|a| {
                    matches!(
                        self.cur().ty(*a).peeled_refs(),
                        Ty::Builtin(dz_hir::ty::BuiltinTy::SourceLocation)
                    )
                });
                let (values, loc) = match rendered.split_last() {
                    Some((last, rest)) if given && !rest.is_empty() => (rest, last.clone()),
                    _ => (&rendered[..], self.source_location(span)),
                };
                format!("::dz::panic_str(::dz::format({}), {loc})", values.join(", "))
            }
            // `error(code?, fmt, args…)` — the code is optional and comes first (§9.1).
            "error" => {
                let loc = self.source_location(span);
                let integral = args
                    .first()
                    .map(|a| self.cur().ty(*a))
                    .is_some_and(|t| t.is_integral() || matches!(t, Ty::Named { .. }));
                let (code, rest) = if integral && rendered.len() > 1 {
                    (format!("::dz::cast<uint32_t>({})", rendered[0]), &rendered[1..])
                } else {
                    ("0u".to_owned(), &rendered[..])
                };
                format!("::dz::make_error({code}, {loc}, {})", rest.join(", "))
            }
            _ => {
                // What reaches here is reflection (`typeinfo_of` and friends, spec §13), which
                // is M11's.
                self.unsupported(span, &format!("the builtin `{name}`"), "owed by M11");
                "0".to_owned()
            }
        }
    }

    /// A prelude operator-trait method called on a primitive, lowered back to the machine's
    /// operator (spec §11.2).
    ///
    /// Inside a generic body `a > b` on a `T: Ord` became `a.cmp(&b) == Greater`, because
    /// `T` might be a struct. In the instance for `i32` the receiver is an `int`, which has
    /// no `.cmp` — so the call is spelled as the operator it stands for.
    fn native_trait_call(
        &mut self,
        recv: ExprId,
        owner: SymbolId,
        name: &str,
        args: &[ExprId],
    ) -> Option<String> {
        let recv_ty = self.cur().ty(recv).peeled().clone();
        // An enum is a named integer, and has `Eq`, `Hash` and `Ord` as one (spec §7.4).
        let builtin = matches!(recv_ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str)
            || self.table.is_enum(&recv_ty);
        if !builtin || (self.table.is_enum(&recv_ty) && !matches!(name, "eq" | "cmp" | "hash")) {
            return None;
        }
        // The standard library's `Float` and `Integer` on a number: the shim's function of the
        // same name, receiver first (spec §14.4). `abs` and `pow` differ for integers.
        let owner_sym = self.res.symbol(owner);
        if matches!(owner_sym.name.as_str(), "Float" | "Integer" | "Number")
            && owner_sym.module == self.res.prelude
        {
            let f = match (&recv_ty, name) {
                (Ty::Int(_), "abs") => "abs_int",
                (Ty::Int(_), "pow") => "pow_int",
                _ => name,
            };
            let operands: Vec<ExprId> = std::iter::once(recv).chain(args.iter().copied()).collect();
            let places = vec![false; operands.len()];
            let mut values = self.ordered_operands(&operands, &places);
            // The receiver's type picks the shim's instance, and a literal is written without
            // one: `5u8` is `5`, an `int` to C++, which would make `5u8.wrapping_add(255)`
            // 260. So a literal receiver says its type.
            if matches!(self.cur().expr(recv).kind, hir::ExprKind::Int(_) | hir::ExprKind::Float(_))
            {
                let t = self.cpp_ty(&recv_ty);
                values[0] = format!("static_cast<{t}>({})", values[0]);
            }
            return Some(format!("::dz::num::{f}({})", values.join(", ")));
        }
        if self.res.symbol(owner).module != self.res.prelude {
            return None;
        }
        let op = match name {
            "eq" => "==",
            "add" => "+",
            "sub" => "-",
            "mul" => "*",
            "div" => "/",
            "rem" => "%",
            "cmp" | "neg" | "hash" => "",
            _ => return None,
        };
        if name == "neg" || name == "hash" {
            let a = self.expr(recv);
            // `hash` on a primitive is the shim's, which mixes the bits (spec §14.2).
            return Some(if name == "neg" {
                format!("(-{a})")
            } else {
                format!("::dz::hash({a})")
            });
        }
        // `eq` and `cmp` take `&Self`, so the operand arrives as `&b`; the operator wants `b`.
        let b = match args.first() {
            Some(&x) => match self.cur().expr(x).kind {
                hir::ExprKind::Ref { place, .. } => place,
                _ => x,
            },
            None => return None,
        };
        // In order, as the call it stands for would be (spec §7.2, 18.49).
        let v = self.ordered_operands(&[recv, b], &[false, false]);
        let (a, b) = (v[0].clone(), v[1].clone());
        if name == "cmp" {
            let ordering = self.res.lookup(self.res.prelude, "Ordering")?;
            let ord = self.qualify(ordering);
            return Some(format!("static_cast<{ord}>(::dz::compare({a}, {b}))"));
        }
        Some(format!("({a} {op} {b})"))
    }

    /// `Some(value)` when `e` is decided at compile time by the §5.5 queries it is made of,
    /// possibly combined with `!`, `&&` and `||` (spec §8.3). A condition with no query in
    /// it is left to C++ even when constant, so ordinary code is emitted as written.
    fn fold_condition(&self, e: ExprId) -> Option<bool> {
        let (value, has_query) = self.const_bool(e)?;
        has_query.then_some(value)
    }

    fn const_bool(&self, e: ExprId) -> Option<(bool, bool)> {
        let body = self.cur();
        match &body.expr(e).kind {
            hir::ExprKind::TypeQuery { ty, query } => {
                let v = match query {
                    hir::TypeQuery::IsTriviallyCopyable => self.table.trivially_copyable(ty),
                    hir::TypeQuery::IsCopyable => self.table.copyable(ty),
                    hir::TypeQuery::HasDrop => self.table.needs_drop(ty),
                    _ => return None,
                };
                Some((v, true))
            }
            hir::ExprKind::Bool(b) => Some((*b, false)),
            hir::ExprKind::Unary { op: UnaryOp::Not, operand } => {
                self.const_bool(*operand).map(|(v, q)| (!v, q))
            }
            hir::ExprKind::Binary { op, lhs, rhs }
                if matches!(op, BinaryOp::And | BinaryOp::Or) =>
            {
                let (a, qa) = self.const_bool(*lhs)?;
                let (b, qb) = self.const_bool(*rhs)?;
                let v = if matches!(op, BinaryOp::And) { a && b } else { a || b };
                Some((v, qa || qb))
            }
            _ => None,
        }
    }

    /// Answer a §5.5 query for a concrete type. Size and alignment are left to C++, which
    /// knows the layout exactly; the rest are properties the checker already computes.
    fn type_query(&mut self, ty: &Ty, query: hir::TypeQuery) -> String {
        use hir::TypeQuery::*;
        match query {
            Size => format!("sizeof({})", self.cpp_ty(ty)),
            Align => format!("alignof({})", self.cpp_ty(ty)),
            Name => {
                let n = dz_check::type_name(self.res, ty);
                format!("::dz::str({}, {})", quote_cpp(&n), n.len())
            }
            IsTriviallyCopyable => self.table.trivially_copyable(ty).to_string(),
            IsCopyable => self.table.copyable(ty).to_string(),
            HasDrop => self.table.needs_drop(ty).to_string(),
        }
    }

    fn intrinsic(&mut self, name: &str, span: Span) -> String {
        match name {
            "line" => {
                let f = self.map.get(self.file);
                format!("{}u", f.location(span.start as usize).line)
            }
            "column" => {
                let f = self.map.get(self.file);
                format!("{}u", f.location(span.start as usize).column)
            }
            "file" => {
                let f = self.map.get(self.file).name().replace('\\', "/");
                format!("::dz::str(\"{f}\", {})", f.len())
            }
            "function" => {
                let n = self.current_fn.clone();
                format!("::dz::str(\"{n}\", {})", n.len())
            }
            "location" => self.source_location(span),
            _ => {
                // `#module`, `#location` and `#expr_text` (spec §12.7) arrive with the rest of
                // metaprogramming.
                self.unsupported(span, "this intrinsic", "owed by M10");
                "0".to_owned()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// An intrinsic used as a default argument, spelled so C++ evaluates it at the call
/// (spec §12.7). GCC, Clang and MSVC all provide these builtins. The column is not the
/// Dazzle one — C++ only knows its own — so a default `#location` carries column 0.
fn caller_intrinsic(name: &str) -> Option<&'static str> {
    Some(match name {
        // Each is a shim function whose own default arguments are the builtins: g++ evaluates
        // a builtin at the call only when it is directly a default argument, and inside any
        // larger expression evaluates it where the function is declared.
        "location" => "::dz::caller_location()",
        "line" => "::dz::caller_line()",
        "column" => "0u",
        "file" => "::dz::caller_file()",
        "function" => "::dz::caller_function()",
        _ => return None,
    })
}

/// Does `Self` appear anywhere in this type?
fn mentions_self(t: &Ty) -> bool {
    match t {
        Ty::SelfTy => true,
        Ty::Named { args, .. } => args.iter().any(mentions_self),
        Ty::Optional(i) | Ty::Fallible(i) | Ty::Range(i) => mentions_self(i),
        Ty::Ref { inner, .. } | Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => {
            mentions_self(inner)
        }
        Ty::Array { elem, .. } | Ty::Slice { elem, .. } => mentions_self(elem),
        Ty::Tuple(parts) => parts.iter().any(mentions_self),
        Ty::Fn { params, ret } => params.iter().any(mentions_self) || mentions_self(ret),
        _ => false,
    }
}

/// The C++ name of the incoming parameter, which differs from the Dazzle binding's name
/// only where [`Emitter::take_ownership_of_params`] rebinds it.
fn incoming_name(name: &str, table: &TypeTable, ty: &Ty) -> String {
    if table.needs_drop(ty) {
        format!("{name}__arg")
    } else {
        name.to_owned()
    }
}

/// Does the body's last statement hand control back on its own?
///
/// Only the shallow case is recognised, because that is the only one C++ recognises too:
/// anything deeper (an exhaustive `switch`, an `if`/`else` where both sides return) needs
/// the trap the caller emits.
fn ends_in_return(body: &hir::Body) -> bool {
    let Some(last) = body.block(body.root).stmts.last() else { return false };
    match &body.stmt(*last).kind {
        hir::StmtKind::Expr(e) => matches!(body.expr(*e).kind, hir::ExprKind::Return(_)),
        _ => false,
    }
}

/// One C++ identifier per local.
///
/// Dazzle lets a `let` shadow an earlier one in the same block and C++ does not, so a name
/// that is *still in scope* when a second local claims it gets its arena index appended.
/// Two loops that both say `for i in …` keep their `i`, because the first one's is gone by
/// the time the second starts — the generated code is meant to be read (spec §16.2), and
/// renaming every repeat would be a steady tax for a rare collision.
fn local_names(body: &hir::Body) -> Vec<String> {
    let mut namer =
        Namer { body, names: vec![String::new(); body.locals.len()], scopes: vec![Vec::new()] };
    for p in &body.params {
        namer.declare(*p);
    }
    namer.block(body.root);
    // Anything the walk did not reach — a default-argument expression's own bindings,
    // say — still needs a name, and by then nothing it could collide with is in scope.
    for (i, name) in namer.names.iter_mut().enumerate() {
        if name.is_empty() {
            *name = format!("{}__{i}", sanitize(&body.locals[i].name));
        }
    }
    namer.names
}

/// A Dazzle identifier as a C++ one. `double`, `class` and `template` are ordinary names
/// in Dazzle and keywords in C++, so every identifier the generator writes goes through
/// here — a declaration and every reference to it alike, which is what keeps the two
/// spellings in step. Applied unconditionally, including to imported names: a C++ header
/// cannot declare something named after a C++ keyword, so there is nothing to special-case.
fn sanitize(name: &str) -> String {
    if is_cpp_reserved(name) {
        format!("{name}_")
    } else {
        name.to_owned()
    }
}

/// A module path's segments, each sanitized — a module may be named `class` too.
fn sanitize_path(module: &str) -> String {
    module_parts(module).join("::")
}

/// A module's name as C++ namespaces. The first part lives in `namespace dz` beside the
/// runtime's own declarations, and C++ refuses a namespace and a function of one name in
/// one scope: a module called `overflow` once broke against the shim's `dz::overflow`. So a
/// first part that is one of those names gains a `_`, as a C++ keyword does.
fn module_parts(module: &str) -> Vec<String> {
    module
        .split("::")
        .enumerate()
        .map(|(i, part)| {
            let name = sanitize(part);
            if i == 0 && runtime_names().contains(name.as_str()) {
                format!("{name}_")
            } else {
                name
            }
        })
        .collect()
}

/// Every name the runtime shim, the prelude and the native layer declare at the top of
/// their text: a type (`struct Opt`), a namespace (`namespace num`) or a function (`void
/// panic(`). Read from the text itself, so a function added there is reserved without
/// anyone remembering to list it. Over-reserving a name only renames a module that never
/// clashed, which costs nothing.
fn runtime_names() -> &'static std::collections::HashSet<String> {
    static NAMES: std::sync::OnceLock<std::collections::HashSet<String>> =
        std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = std::collections::HashSet::new();
        for text in [RUNTIME_H, RUNTIME_CPP, PRELUDE_H, NATIVE_H, NATIVE_CPP] {
            for line in text.lines() {
                if line.is_empty() || line.starts_with([' ', '\t', '/', '#', '}']) {
                    continue;
                }
                let words: Vec<&str> = line.split_whitespace().collect();
                if let [kind, name, ..] = words.as_slice() {
                    if matches!(*kind, "struct" | "class" | "using" | "namespace" | "enum") {
                        let ident: String =
                            name.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                        names.insert(ident);
                        continue;
                    }
                }
                let Some(paren) = line.find('(') else { continue };
                let ident: String = line[..paren]
                    .trim_end()
                    .chars()
                    .rev()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                if !ident.is_empty() {
                    names.insert(ident);
                }
            }
        }
        names
    })
}

/// Assigns C++ names by walking the body's scopes, so shadowing is detected where it
/// actually happens rather than assumed from a name being seen twice.
struct Namer<'a> {
    body: &'a hir::Body,
    names: Vec<String>,
    scopes: Vec<Vec<String>>,
}

impl Namer<'_> {
    fn declare(&mut self, l: LocalId) {
        if self.body.local(l).name == "self" {
            // A member's `self` is `*this`; an implementation's function takes it as an
            // ordinary first parameter (spec §7.6).
            self.names[l.index()] = match self.body.owner {
                BodyOwner::ImplMethod { .. } => "self".to_owned(),
                _ => "(*this)".to_owned(),
            };
            return;
        }
        let base = sanitize(&self.body.local(l).name);
        let taken = self.scopes.iter().flatten().any(|n| *n == base);
        let name = if taken { format!("{base}__{}", l.0) } else { base };
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(name.clone());
        }
        self.names[l.index()] = name;
    }

    fn scoped(&mut self, f: impl FnOnce(&mut Self)) {
        self.scopes.push(Vec::new());
        f(self);
        self.scopes.pop();
    }

    fn block(&mut self, id: BlockId) {
        let stmts = self.body.block(id).stmts.clone();
        self.scoped(|n| {
            for s in stmts {
                n.stmt(s);
            }
        });
    }

    fn stmt(&mut self, id: StmtId) {
        match &self.body.stmt(id).kind {
            hir::StmtKind::Let { local, init } => {
                if let Some(e) = init {
                    self.expr(*e);
                }
                if let Some(l) = local {
                    self.declare(*l);
                }
            }
            hir::StmtKind::LetTuple { locals, init } => {
                self.expr(*init);
                for l in locals.clone().into_iter().flatten() {
                    self.declare(l);
                }
            }
            hir::StmtKind::Assign { target, value, .. } => {
                self.expr(*target);
                self.expr(*value);
            }
            hir::StmtKind::Expr(e) => self.expr(*e),
            hir::StmtKind::While { cond, body, .. } => {
                self.expr(*cond);
                self.block(*body);
            }
            hir::StmtKind::For { bindings, subject, body, .. } => {
                self.expr(*subject);
                let (bindings, body) = (bindings.clone(), *body);
                self.scoped(|n| {
                    for l in bindings.into_iter().flatten() {
                        n.declare(l);
                    }
                    n.block(body);
                });
            }
            hir::StmtKind::Defer(inner) => self.stmt(*inner),
            hir::StmtKind::Item(_) | hir::StmtKind::Error => {}
        }
    }

    fn expr(&mut self, id: ExprId) {
        match self.body.expr(id).kind.clone() {
            hir::ExprKind::Block(b) => self.block(b),
            hir::ExprKind::If { cond, then, else_branch } => {
                self.expr(cond);
                self.block(then);
                if let Some(b) = else_branch {
                    self.block(b);
                }
            }
            // Declared in the enclosing scope, where its C++ line lands.
            hir::ExprKind::Guarded { guard, init, value } => {
                self.expr(init);
                self.declare(guard);
                self.expr(value);
            }
            hir::ExprKind::IfLet { binding, operand, then, else_branch } => {
                self.expr(operand);
                self.scoped(|n| {
                    n.declare(binding);
                    n.block(then);
                });
                if let Some(b) = else_branch {
                    self.block(b);
                }
            }
            hir::ExprKind::Switch { subject, arms, default } => {
                self.expr(subject);
                for arm in arms {
                    self.scoped(|n| {
                        if let Some(l) = arm.binding {
                            n.declare(l);
                        }
                        n.block(arm.body);
                    });
                }
                if let Some(b) = default {
                    self.block(b);
                }
            }
            hir::ExprKind::Or { operand, clause } => {
                self.expr(operand);
                match clause {
                    hir::OrClause::Value(v) => self.expr(v),
                    hir::OrClause::Block { binding, body } => self.scoped(|n| {
                        if let Some(l) = binding {
                            n.declare(l);
                        }
                        n.block(body);
                    }),
                    hir::OrClause::Ignore | hir::OrClause::Unreachable => {}
                }
            }
            hir::ExprKind::Field { base, .. }
            | hir::ExprKind::TupleIndex { base, .. }
            | hir::ExprKind::Deref(base)
            | hir::ExprKind::Ref { place: base, .. }
            | hir::ExprKind::Unary { operand: base, .. }
            | hir::ExprKind::Cast { operand: base }
            | hir::ExprKind::Move(base) => self.expr(base),
            hir::ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            hir::ExprKind::SliceRange { base, start, end } => {
                self.expr(base);
                for x in [start, end].into_iter().flatten() {
                    self.expr(x);
                }
            }
            hir::ExprKind::Len(base) | hir::ExprKind::DataPtr(base) => self.expr(base),
            hir::ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            hir::ExprKind::Range { start, end, .. } => {
                for e in [start, end].into_iter().flatten() {
                    self.expr(e);
                }
            }
            hir::ExprKind::StructLit { fields, .. } => {
                for f in fields.into_iter().flatten() {
                    self.expr(f);
                }
            }
            hir::ExprKind::UnionVariant { payload, .. } => {
                for f in payload.into_iter().flatten() {
                    self.expr(f);
                }
            }
            hir::ExprKind::Tuple(parts) | hir::ExprKind::ArrayLit(parts) => {
                for p in parts {
                    self.expr(p);
                }
            }
            hir::ExprKind::Call { callee, args } => {
                match callee {
                    hir::Callee::Method { recv, .. } | hir::Callee::BaseMethod { recv, .. } => {
                        self.expr(recv)
                    }
                    hir::Callee::Value(v) => self.expr(v),
                    _ => {}
                }
                for a in args {
                    self.expr(a);
                }
            }
            hir::ExprKind::Builtin { args, .. } | hir::ExprKind::Intrinsic { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
            hir::ExprKind::Return(Some(v)) => self.expr(v),
            _ => {}
        }
    }
}

/// Words that are keywords in C++ but ordinary identifiers in Dazzle.
fn is_cpp_reserved(name: &str) -> bool {
    matches!(
        name,
        "alignas"
            | "alignof"
            | "and"
            | "asm"
            | "auto"
            | "bitand"
            | "bitor"
            | "catch"
            | "class"
            | "compl"
            | "concept"
            | "constexpr"
            | "const_cast"
            | "co_await"
            | "co_return"
            | "co_yield"
            | "decltype"
            | "delete"
            | "double"
            | "dynamic_cast"
            | "explicit"
            | "export"
            | "extern"
            | "float"
            | "friend"
            | "goto"
            | "inline"
            | "int"
            | "long"
            | "mutable"
            | "namespace"
            | "new"
            | "noexcept"
            | "not"
            | "nullptr"
            | "operator"
            | "or"
            | "private"
            | "protected"
            | "public"
            | "register"
            | "reinterpret_cast"
            | "requires"
            | "short"
            | "signed"
            | "sizeof"
            | "static"
            // Not a keyword, but a module segment named `std` would open `dz::std`, and
            // every unqualified `std::` inside `namespace dz` would then find it rather than
            // the C++ standard library (spec §14.4).
            | "std"
            | "static_assert"
            | "static_cast"
            | "template"
            | "this"
            | "thread_local"
            | "throw"
            | "try"
            | "typedef"
            | "typeid"
            | "typename"
            | "unsigned"
            | "using"
            | "virtual"
            | "volatile"
            | "wchar_t"
            | "xor"
    )
}

fn int_name(i: IntTy) -> &'static str {
    match i {
        IntTy::I8 => "int8_t",
        IntTy::I16 => "int16_t",
        IntTy::I32 => "int32_t",
        IntTy::I64 => "int64_t",
        IntTy::U8 => "uint8_t",
        IntTy::U16 => "uint16_t",
        IntTy::U32 => "uint32_t",
        IntTy::U64 => "uint64_t",
        IntTy::Isize => "intptr_t",
        IntTy::Usize => "size_t",
    }
}

/// A literal, with the suffix its type needs so C++ does not narrow or wrap it.
fn int_literal(v: i128, ty: &Ty) -> String {
    match ty {
        Ty::Int(IntTy::U64 | IntTy::Usize) => format!("{v}ull"),
        Ty::Int(IntTy::I64) => format!("{v}ll"),
        Ty::Int(IntTy::U32) => format!("{v}u"),
        Ty::Float(FloatTy::F32) => format!("{v}.0f"),
        Ty::Float(FloatTy::F64) => format!("{v}.0"),
        _ => v.to_string(),
    }
}

fn float_literal(v: f64, ty: &Ty) -> String {
    let mut text = format!("{v:?}");
    if !text.contains('.') && !text.contains('e') && !text.contains("inf") && !text.contains("NaN")
    {
        text.push_str(".0");
    }
    if matches!(ty, Ty::Float(FloatTy::F32)) {
        text.push('f');
    }
    text
}

fn unary_op(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "!",
        UnaryOp::Neg => "-",
        UnaryOp::BitNot => "~",
    }
}

/// The C++ label `break outer` or `continue outer` jumps to, by the loop's id within its
/// body — ids, not names, since two sibling loops may carry the same Dazzle label.
fn loop_label_name(what: &str, id: u32) -> String {
    format!("_dz_{what}_{id}")
}

/// Drop one layer of parentheses when they wrap the whole expression, so conditions do
/// not come out as `if ((a == b))` and trip `-Wparentheses-equality`.
fn unwrap_parens(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'(') || bytes.last() != Some(&b')') {
        return text;
    }
    let mut depth = 0i32;
    for (i, b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                // The opening parenthesis closes before the end, so it does not wrap
                // the whole expression: `(a) && (b)` must be left alone.
                if depth == 0 && i + 1 != bytes.len() {
                    return text;
                }
            }
            _ => {}
        }
    }
    &text[1..text.len() - 1]
}

/// `"c++17"` → 17, `"gnu++20"` → 20, `"c++2b"` → 23. Unknown spellings fall back to 17,
/// which only costs readability, never correctness.
fn cpp_standard_version(text: &str) -> u32 {
    let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    match digits.as_str() {
        "20" | "2" => 20,
        "23" | "2b" => 23,
        "26" => 26,
        _ => match text.trim_end_matches(['a', 'b', 'c']).chars().last() {
            // `c++2a` is C++20 and `c++2b` is C++23 under their old names.
            Some('2') if text.ends_with('a') => 20,
            Some('2') if text.ends_with('b') => 23,
            _ => 17,
        },
    }
}

/// `engine::render` → `::dz::engine::render`. Absolute, so a module named `dz` (the
/// prelude, §18.23) cannot shadow the shim's namespace from inside another module.
fn qualified_namespace(module: &str) -> String {
    format!("::dz::{}", sanitize_path(module))
}

fn namespace_open(module: &str) -> String {
    let mut s = String::from("namespace dz {\n");
    for part in module_parts(module) {
        let _ = writeln!(s, "namespace {part} {{");
    }
    s
}

fn namespace_close(module: &str) -> String {
    let mut s = String::new();
    for part in module_parts(module).into_iter().rev() {
        let _ = writeln!(s, "}}  // namespace {part}");
    }
    s.push_str("}  // namespace dz\n");
    s
}

fn file_stem(module: &str) -> String {
    module.replace("::", "_")
}

/// Quote already-decoded bytes as a C++ string literal.
///
/// The HIR holds the string's *value*, escapes applied, so this escapes rather than
/// re-escapes: whatever the source spelled, the bytes here are what the program means.
fn quote_cpp(text: &str) -> String {
    let mut out = String::from("\"");
    for b in text.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(b as char),
            other => {
                // Hex escapes are greedy in C++, so close the literal after each one.
                let _ = write!(out, "\\x{other:02x}\"\"");
            }
        }
    }
    out.push('"');
    out
}

/// Order types so a struct is emitted after anything it stores by value.
/// Every named type `t` holds **by value**, with its arguments: what a declaration of `t`
/// needs declared first. A pointer or reference does not count — a forward declaration
/// would do — but an array, a tuple and an optional all store their element in place.
fn by_value_named(t: &Ty, out: &mut Vec<(SymbolId, Vec<Ty>)>) {
    match t {
        Ty::Named { sym, args } => out.push((*sym, args.clone())),
        Ty::Array { elem, .. } | Ty::Optional(elem) => by_value_named(elem, out),
        Ty::Tuple(parts) => {
            for p in parts {
                by_value_named(p, out);
            }
        }
        _ => {}
    }
}
