//! An edit re-runs ownership, instantiation and emission only for the modules it can affect
//! (T-0197), stage 2a of the incremental build.
//!
//! The front end still reads the whole program every build: manifests, parsing, macros,
//! names, types, and placement, whose whole-program passes read every function's body. What
//! is left per module is checking its bodies for ownership, instantiating the generics it
//! uses, and writing its C++, which was 1.2 s of every build of a 100-module project. A
//! module none of whose inputs changed would come out the same, so its files in `build/cpp`
//! are kept as they are, and its ownership warnings are printed from its record.
//!
//! **A record per module**, in `build/obj/dz/modules-<key>/`, written after a build that
//! succeeded, under the same key as the build stamp (`fresh::key`): another `dz`, option or
//! setting starts with none. It holds a hash of the module's sources and manifest (an
//! imported header's declarations, for one of those), the files it wrote with their sizes
//! and times, the instance bodies it placed and which it defines, whether it read the type
//! table, and its ownership warnings.
//!
//! **A module is affected** when its own record is missing or differs, or one of its files
//! was changed or removed, or what placement decided for its bodies changed, or what it was
//! decided for any generic body; and when the *interface* of any module it depends on,
//! however indirectly, changed (T-0198). Everything else keeps its files.
//!
//! **The interface** is what of a module other modules' C++ can be made from: its
//! declarations and signatures, constants, statics, macros, field defaults and default
//! arguments, and the whole of every generic item and trait, whose bodies are copied into
//! the modules that use them. The body of a plain function or method is not in it, so an
//! edit inside one leaves the modules above as they were, as a `.cpp` edit does in C++.
//! What a body's placement depends on in its callees' bodies (whether a call keeps its
//! argument, whether a function builds its result whole) is caught by comparing every
//! body's placement instead. Where an item's position can reach another module's C++ (a
//! generic's `#line`s, a default's), its line is part of the interface too.
//!
//! **Instance bodies** are defined by the first module in order to need them (T-0185), so a
//! module emitted now can take over a body a kept module defined, or leave one to it. After
//! emitting, the owner of each body is worked out again from every module's needs, the
//! emitted ones' new and the kept ones' recorded; a kept module whose bodies changed hands is
//! emitted too. Needs do not depend on ownership, so that settles at once.
//!
//! **Reflection** numbers types in the order modules first name them, across the program, so
//! a program that reads the type table is emitted whole.
//!
//! `DZ_INCREMENTAL=0` turns this off. `DZ_INCREMENTAL=verify` also emits the whole program
//! and fails the build if any file differs from what was kept and written, which is how the
//! edit corpus (`crates/dz/tests/incremental.rs`) holds this to a clean build.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use dz_project::{ModuleId, Workspace};
use dz_resolve::ParsedFile;
use dz_span::SourceMap;

use crate::native::{fnv1a, nanos};

const FORMAT: &str = "dz module record 1";

/// What one module's last build left.
#[derive(Default)]
struct Record {
    source: u64,
    interface: u64,
    /// What placement decided for the module's bodies, and for every generic body.
    placement: u64,
    templates: u64,
    /// Each file it wrote in `build/cpp`: name, size, time.
    outputs: Vec<(String, u64, u128)>,
    /// Each instance body it placed, by `_DEF` guard, and whether it defines it.
    claims: Vec<(String, bool)>,
    reflects: bool,
    warnings: String,
}

/// Every module's record, by module name.
pub struct Records {
    dir: PathBuf,
    by_name: HashMap<String, Record>,
}

/// Whether this build may reuse, and whether it also checks itself.
pub fn mode() -> Option<bool> {
    match std::env::var("DZ_INCREMENTAL").as_deref() {
        Ok("0") => None,
        Ok("verify") => Some(true),
        _ => Some(false),
    }
}

impl Records {
    /// The records of the last build with `key` under `build_root`; none if there are none.
    pub fn load(build_root: &Path, key: &str) -> Records {
        let dir = build_root
            .join("obj")
            .join("dz")
            .join(format!("modules-{:016x}", fnv1a(key.as_bytes())));
        let mut by_name = HashMap::new();
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
            if let Some((name, record)) = parse(&text) {
                by_name.insert(name, record);
            }
        }
        Records { dir, by_name }
    }

    /// The modules whose ownership, instances and C++ this build must work out again, or
    /// `None` for every module.
    pub fn select(&self, cpp_dir: &Path, c: &dz_driver::Checked<'_>) -> Option<HashSet<ModuleId>> {
        let (ws, map, files) = (c.ws, c.map, c.files);
        if self.by_name.is_empty() || self.by_name.values().any(|r| r.reflects) {
            return None;
        }
        let facts = Facts::of(c.ws, c.map, c.files, c.hir, c.table, c.placements);
        // The modules whose interface changed, which every module above them must follow.
        let mut reached: HashSet<ModuleId> = HashSet::new();
        let mut changed: HashSet<ModuleId> = HashSet::new();
        for m in ws.modules().filter(|m| m.id != ws.prelude) {
            let Some(record) = self.by_name.get(&m.name) else {
                changed.insert(m.id);
                continue;
            };
            let outputs_hold = record.outputs.iter().all(|(name, len, time)| {
                std::fs::metadata(cpp_dir.join(name)).is_ok_and(|meta| {
                    meta.len() == *len && meta.modified().ok().and_then(nanos) == Some(*time)
                })
            });
            if record.interface != facts.interface(ws, map, files, m.id) {
                reached.insert(m.id);
            }
            if record.source != source_hash(ws, map, files, m.id)
                || !outputs_hold
                || record.placement != facts.placement_of(m.id)
                || record.templates != facts.templates
            {
                changed.insert(m.id);
            }
        }
        // A module is affected when it changed, or the interface of anything beneath it did.
        let mut affected = HashSet::new();
        for m in ws.modules().filter(|m| m.foreign.is_none() && m.id != ws.prelude) {
            let beneath: Vec<ModuleId> = ws.module(m.id).deps.iter().map(|d| d.module).collect();
            if changed.contains(&m.id) || beneath.iter().any(|&d| depends_on_any(ws, d, &reached)) {
                affected.insert(m.id);
            }
        }
        let all = ws.modules().filter(|m| m.foreign.is_none() && m.id != ws.prelude).count();
        (affected.len() < all).then_some(affected)
    }

    /// What `select`'s modules emit around: the instance bodies the kept modules define.
    pub fn selection(&self, ws: &Workspace, modules: &HashSet<ModuleId>) -> dz_cpp::Selection {
        let mut emit = modules.clone();
        emit.insert(ws.prelude);
        let mut claimed = HashMap::new();
        for m in ws.modules().filter(|m| m.foreign.is_none() && !emit.contains(&m.id)) {
            if let Some(record) = self.by_name.get(&m.name) {
                for (guard, defines) in &record.claims {
                    if *defines {
                        claimed.insert(guard.clone(), m.id);
                    }
                }
            }
        }
        dz_cpp::Selection { modules: emit, claimed }
    }

    /// After emitting `selection`: the kept modules whose instance bodies changed hands, which
    /// must be emitted too. `None` when even the emitted ones disagree with the rule, or the
    /// program reads the type table: emit the whole program.
    pub fn reconcile(
        &self,
        ws: &Workspace,
        selection: &dz_cpp::Selection,
        claims: &HashMap<ModuleId, Vec<(String, bool)>>,
        reflects: &HashSet<ModuleId>,
    ) -> Option<HashSet<ModuleId>> {
        if !reflects.is_empty() {
            return None;
        }
        let modules: Vec<ModuleId> =
            ws.modules().filter(|m| m.foreign.is_none()).map(|m| m.id).collect();
        let claims_of = |m: ModuleId| -> Option<&Vec<(String, bool)>> {
            if selection.modules.contains(&m) {
                claims.get(&m)
            } else {
                self.by_name.get(&ws.module(m).name).map(|r| &r.claims)
            }
        };
        // Each body's owner: the first module in order that needs it.
        let mut owner: HashMap<&str, ModuleId> = HashMap::new();
        for &m in &modules {
            for (guard, _) in claims_of(m).into_iter().flatten() {
                owner.entry(guard.as_str()).or_insert(m);
            }
        }
        let mut extra = HashSet::new();
        for &m in &modules {
            let Some(placed) = claims_of(m) else { continue };
            let want: BTreeSet<&str> = placed
                .iter()
                .filter(|(g, _)| owner[g.as_str()] == m)
                .map(|(g, _)| g.as_str())
                .collect();
            let has: BTreeSet<&str> =
                placed.iter().filter(|(_, d)| *d).map(|(g, _)| g.as_str()).collect();
            if want != has {
                if selection.modules.contains(&m) {
                    return None;
                }
                extra.insert(m);
            }
        }
        Some(extra)
    }

    /// The files the kept modules wrote, which this build compiles as they are.
    pub fn kept_files(&self, ws: &Workspace, selection: &dz_cpp::Selection) -> Vec<String> {
        ws.modules()
            .filter(|m| m.foreign.is_none() && !selection.modules.contains(&m.id))
            .filter_map(|m| self.by_name.get(&m.name))
            .flat_map(|r| r.outputs.iter().map(|(name, _, _)| name.clone()))
            .collect()
    }

    /// The ownership warnings the kept modules' last build printed, to print again.
    pub fn kept_warnings(&self, ws: &Workspace, selection: Option<&HashSet<ModuleId>>) -> String {
        let Some(selection) = selection else { return String::new() };
        ws.modules()
            .filter(|m| m.foreign.is_none() && m.id != ws.prelude && !selection.contains(&m.id))
            .filter_map(|m| self.by_name.get(&m.name))
            .map(|r| r.warnings.as_str())
            .collect()
    }

    /// Record what this build did, once it has succeeded: every module emitted, and every
    /// imported header's declarations.
    pub fn write(
        &self,
        analysis: &crate::build::Analysis,
        out: &crate::build::BuildOutput,
    ) -> Option<()> {
        let ws = analysis.ws.as_ref()?;
        let (hir, table, placements) =
            (analysis.hir.as_ref()?, analysis.table.as_ref()?, analysis.placements.as_ref()?);
        let facts = Facts::of(ws, &analysis.map, &analysis.files, hir, table, placements);
        std::fs::create_dir_all(&self.dir).ok()?;
        for m in ws.modules().filter(|m| m.id != ws.prelude) {
            let emitted = out.claims.get(&m.id);
            if m.foreign.is_none() && emitted.is_none() {
                continue;
            }
            let mut text = format!("{FORMAT}\nname {}\n", m.name);
            text.push_str(&format!(
                "source {:016x}\n",
                source_hash(ws, &analysis.map, &analysis.files, m.id)
            ));
            let interface = facts.interface(ws, &analysis.map, &analysis.files, m.id);
            text.push_str(&format!("interface {interface:016x}\n"));
            text.push_str(&format!("placement {:016x}\n", facts.placement_of(m.id)));
            text.push_str(&format!("templates {:016x}\n", facts.templates));
            text.push_str(&format!("reflects {}\n", out.reflects.contains(&m.id)));
            for name in out.module_files.get(&m.id).into_iter().flatten() {
                let meta = std::fs::metadata(out.cpp_dir.join(name)).ok()?;
                let time = meta.modified().ok().and_then(nanos)?;
                text.push_str(&format!("output {} {time} {name}\n", meta.len()));
            }
            for (guard, defines) in emitted.into_iter().flatten() {
                text.push_str(&format!("claim {} {guard}\n", u8::from(*defines)));
            }
            text.push_str("warnings\n");
            // A module emitted again only because its instance bodies changed hands was not
            // checked again: what it warned about is what it warned about before.
            let checked = analysis.finished.as_ref().map_or(true, |f| f.contains(&m.id));
            if checked {
                let own: HashSet<dz_span::FileId> =
                    analysis.files.get(&m.id).into_iter().flatten().map(|f| f.file).collect();
                let mut warnings = dz_diag::Diagnostics::new();
                for d in analysis.flow_diags.iter() {
                    if d.labels.first().is_some_and(|l| own.contains(&l.file)) {
                        warnings.push(d.clone());
                    }
                }
                if !warnings.is_empty() {
                    text.push_str(&dz_diag::render_to_string(&analysis.map, &warnings));
                }
            } else if let Some(old) = self.by_name.get(&m.name) {
                text.push_str(&old.warnings);
            }
            let path = self.dir.join(format!("{:016x}.rec", fnv1a(m.name.as_bytes())));
            crate::shared::write(&path, text.as_bytes()).ok()?;
        }
        Some(())
    }
}

/// A module's record, and its name.
fn parse(text: &str) -> Option<(String, Record)> {
    let (head, warnings) = text.split_once("\nwarnings\n")?;
    let mut lines = head.lines();
    if lines.next()? != FORMAT {
        return None;
    }
    let name = lines.next()?.strip_prefix("name ")?.to_owned();
    let mut r = Record { warnings: warnings.to_owned(), ..Record::default() };
    for line in lines {
        let (kind, rest) = line.split_once(' ')?;
        match kind {
            "source" => r.source = u64::from_str_radix(rest, 16).ok()?,
            "interface" => r.interface = u64::from_str_radix(rest, 16).ok()?,
            "placement" => r.placement = u64::from_str_radix(rest, 16).ok()?,
            "templates" => r.templates = u64::from_str_radix(rest, 16).ok()?,
            "reflects" => r.reflects = rest == "true",
            "output" => {
                let mut parts = rest.splitn(3, ' ');
                let len = parts.next()?.parse().ok()?;
                let time = parts.next()?.parse().ok()?;
                r.outputs.push((parts.next()?.to_owned(), len, time));
            }
            "claim" => {
                let (defines, guard) = rest.split_once(' ')?;
                r.claims.push((guard.to_owned(), defines == "1"));
            }
            _ => return None,
        }
    }
    Some((name, r))
}

/// What a module's C++ is made from that is its own: its manifest and the text of each of
/// its sources, or for an imported header, what libclang declared.
fn source_hash(
    ws: &Workspace,
    map: &SourceMap,
    files: &HashMap<ModuleId, Vec<ParsedFile>>,
    m: ModuleId,
) -> u64 {
    let module = ws.module(m);
    let mut all = Vec::new();
    if let Some(foreign) = &module.foreign {
        all.extend_from_slice(format!("{:?}\n{:?}", foreign.header, foreign.trust).as_bytes());
        return fnv1a(&all);
    }
    for id in
        std::iter::once(module.manifest).chain(files.get(&m).into_iter().flatten().map(|f| f.file))
    {
        let file = map.get(id);
        all.extend_from_slice(file.path().to_string_lossy().as_bytes());
        all.push(0);
        all.extend_from_slice(file.text().as_bytes());
        all.push(0);
    }
    fnv1a(&all)
}

/// What placement decided, by module, and for every generic body; and what makes up each
/// module's interface.
struct Facts {
    placement: HashMap<ModuleId, u64>,
    templates: u64,
}

impl Facts {
    fn of(
        ws: &Workspace,
        _map: &SourceMap,
        files: &HashMap<ModuleId, Vec<ParsedFile>>,
        hir: &dz_hir::Hir,
        table: &dz_check::TypeTable,
        placements: &dz_flow::placement::Placements,
    ) -> Facts {
        let module_of: HashMap<dz_span::FileId, ModuleId> =
            files.iter().flat_map(|(m, fs)| fs.iter().map(move |f| (f.file, *m))).collect();
        let mut by_module: HashMap<ModuleId, Vec<u8>> = HashMap::new();
        let mut templates = Vec::new();
        for (_, body) in hir.bodies() {
            let placed = placements.body(&body.owner).map(|p| p.fingerprint()).unwrap_or_default();
            // A generic body's placement is what every instance of it is emitted with, in
            // whichever module uses it.
            let into = if dz_mono::is_template(table, &body.owner) {
                &mut templates
            } else {
                match module_of.get(&body.file) {
                    Some(m) if *m != ws.prelude => by_module.entry(*m).or_default(),
                    _ => continue,
                }
            };
            into.extend_from_slice(placed.as_bytes());
            into.push(0);
        }
        Facts {
            placement: by_module.into_iter().map(|(m, bytes)| (m, fnv1a(&bytes))).collect(),
            templates: fnv1a(&templates),
        }
    }

    fn placement_of(&self, m: ModuleId) -> u64 {
        self.placement.get(&m).copied().unwrap_or(0)
    }

    /// What of module `m` other modules' C++ can be made from (see the module's comment).
    fn interface(
        &self,
        ws: &Workspace,
        map: &SourceMap,
        files: &HashMap<ModuleId, Vec<ParsedFile>>,
        m: ModuleId,
    ) -> u64 {
        let module = ws.module(m);
        if module.foreign.is_some() {
            return source_hash(ws, map, files, m);
        }
        let mut out = Interface { bytes: Vec::new(), file: map.get(module.manifest) };
        out.whole(dz_span::Span::new(0, out.file.text().len()), false);
        for pf in files.get(&m).into_iter().flatten() {
            out.file = map.get(pf.file);
            out.bytes.extend_from_slice(out.file.path().to_string_lossy().as_bytes());
            out.bytes.push(0);
            for import in &pf.ast.imports {
                out.whole(import.span, false);
            }
            for item in &pf.ast.items {
                out.item(item);
            }
        }
        fnv1a(&out.bytes)
    }
}

/// A module's interface being written down, one piece of source text at a time.
struct Interface<'a> {
    bytes: Vec<u8>,
    file: &'a dz_span::SourceFile,
}

impl Interface<'_> {
    /// The text at `span`, and with `positioned` the line it starts on. A span that is not
    /// in the file, as an expansion's may not be, is written as its numbers, so anything
    /// about it that changes still counts.
    fn whole(&mut self, span: dz_span::Span, positioned: bool) {
        match self.file.text().get(span.start as usize..span.end as usize) {
            Some(text) => self.bytes.extend_from_slice(text.as_bytes()),
            None => self.bytes.extend_from_slice(format!("{span:?}").as_bytes()),
        }
        if positioned {
            let line = self.file.line_index(span.start as usize);
            self.bytes.extend_from_slice(format!("@{line}").as_bytes());
        }
        self.bytes.push(0);
    }

    fn item(&mut self, item: &dz_ast::Item) {
        use dz_ast::ItemKind;
        self.bytes.extend_from_slice(format!("{:?}", item.vis).as_bytes());
        for attr in &item.attrs {
            self.whole(attr.span, false);
        }
        match &item.kind {
            ItemKind::Fn(f) if f.generics.is_empty() && f.body.is_some() => {
                self.signature(item.span, f)
            }
            ItemKind::Struct(s) if s.generics.is_empty() => self.members(item.span, &s.members),
            ItemKind::Union(u) if u.generics.is_empty() => self.members(item.span, &u.methods),
            ItemKind::Impl(i) if i.generics.is_empty() => self.members(item.span, &i.items),
            ItemKind::Enum(_) | ItemKind::Const(_) | ItemKind::Static(_) => {
                self.whole(item.span, false)
            }
            // Generic items and traits, whose bodies other modules instantiate or inherit,
            // and macros: all of it, where it is.
            _ => self.whole(item.span, true),
        }
    }

    /// A function whose body only its own module's C++ holds: everything before the body.
    /// A default argument is evaluated where the function is called, so with one, where it
    /// is counts too.
    fn signature(&mut self, outer: dz_span::Span, f: &dz_ast::FnDecl) {
        let Some(body) = &f.body else { return self.whole(outer, true) };
        if body.span.start < outer.start || body.span.start > outer.end {
            return self.whole(outer, true);
        }
        let defaults = f.params.iter().any(|p| p.default.is_some());
        self.whole(dz_span::Span::new(outer.start as usize, body.span.start as usize), defaults);
    }

    /// A struct, union or implementation that is not generic: what is around its members,
    /// and each member.
    fn members(&mut self, outer: dz_span::Span, members: &[dz_ast::Member]) {
        use dz_ast::MemberKind;
        let mut cursor = outer.start;
        for m in members {
            if m.span.start < cursor || m.span.end > outer.end {
                return self.whole(outer, true);
            }
            self.whole(dz_span::Span::new(cursor as usize, m.span.start as usize), false);
            cursor = m.span.end;
            for attr in &m.attrs {
                self.whole(attr.span, false);
            }
            self.bytes.extend_from_slice(format!("{:?}", m.vis).as_bytes());
            match &m.kind {
                MemberKind::Fn(f) | MemberKind::Ctor(f) if f.generics.is_empty() => {
                    self.signature(m.span, f)
                }
                // A field's default is written into the C++ of whatever builds the struct.
                MemberKind::Field(field) => self.whole(m.span, field.default.is_some()),
                _ => self.whole(m.span, true),
            }
        }
        self.whole(dz_span::Span::new(cursor as usize, outer.end as usize), false);
    }
}

/// Whether `m`, or anything it depends on however indirectly, is in `changed`.
fn depends_on_any(ws: &Workspace, m: ModuleId, changed: &HashSet<ModuleId>) -> bool {
    let mut seen = HashSet::new();
    let mut pending = vec![m];
    while let Some(m) = pending.pop() {
        if !seen.insert(m) {
            continue;
        }
        if changed.contains(&m) {
            return true;
        }
        pending.extend(ws.module(m).deps.iter().map(|d| d.module));
    }
    false
}
