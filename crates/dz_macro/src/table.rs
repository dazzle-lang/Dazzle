//! Every macro in the workspace, and the checks a declaration must pass before it is used.
//!
//! A declaration is checked once, where it is written, rather than at each invocation: a
//! `$x` that nothing captured is the macro author's mistake whether or not anyone has
//! invoked the macro yet, and reporting it per invocation would point users at code that is
//! not theirs (spec §12.6).

use std::collections::{HashMap, HashSet};

use dz_ast::*;
use dz_diag::{codes, Diagnostic};
use dz_parse::Fragment;
use dz_project::ModuleId;
use dz_span::{FileId, Span};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MacroKind {
    Decl,
    Stmt,
    Expr,
}

impl MacroKind {
    pub fn name(self) -> &'static str {
        match self {
            MacroKind::Decl => "decl",
            MacroKind::Stmt => "stmt",
            MacroKind::Expr => "expr",
        }
    }
}

/// What a capture's kind names: one of the parser's fragments, or a `syntax` rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CapKind {
    Fragment(Fragment),
    Rule(String),
}

pub(crate) struct MacroEntry {
    pub decl: MacroDecl,
    pub module: ModuleId,
    pub file: FileId,
    pub vis: Vis,
    /// `None` when the declaration was rejected; invoking it then reports nothing more,
    /// since what is wrong has been said where the macro is declared.
    pub kind: Option<MacroKind>,
}

#[derive(Default)]
pub(crate) struct Table {
    pub macros: HashMap<(ModuleId, String), MacroEntry>,
    /// Each module's dependencies, by the alias it names them with (spec §3.2).
    pub deps: HashMap<ModuleId, HashMap<String, ModuleId>>,
    /// Each module's top-level declaration names: what a name in a macro's `expand`
    /// resolves to when it is invoked from another module (spec §12.4).
    pub top_names: HashMap<ModuleId, HashSet<String>>,
}

/// The kind a capture names, if it names one.
pub(crate) fn cap_kind(kind: &Ident, rules: &[SyntaxRule]) -> Option<CapKind> {
    if let Some(f) = Fragment::from_name(&kind.name) {
        return Some(CapKind::Fragment(f));
    }
    rules.iter().any(|r| r.name.name == kind.name).then(|| CapKind::Rule(kind.name.clone()))
}

/// How a capture appears to `expand`: its shape, and what it holds.
#[derive(Clone, Debug)]
struct Cap {
    /// `[]K`, which `#for` iterates.
    many: bool,
    kind: Option<CapKind>,
}

/// Check one declaration, returning what is wrong with it. An empty result means every
/// invocation can be matched and expanded without the macro itself being at fault.
pub(crate) fn validate(decl: &MacroDecl, file: FileId) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let name = decl.name.name.as_str();
    if INTRINSICS.contains(&name) || name == "error" {
        out.push(
            Diagnostic::error(
                codes::RESERVED_MACRO_NAME,
                format!("`{name}` is the name of one of the compiler's own macros"),
            )
            .at(file, decl.name.span, "reserved")
            .with_help("intrinsic names and `error` are reserved (spec §12.8); choose another"),
        );
    }

    let mut rule_names = HashSet::new();
    for r in &decl.rules {
        if Fragment::from_name(&r.name.name).is_some() {
            out.push(
                Diagnostic::error(
                    codes::BAD_MACRO,
                    format!("`{}` is a built-in fragment kind", r.name.name),
                )
                .at(file, r.name.span, "cannot be a rule's name")
                .with_help("name a `syntax` rule in PascalCase, like a type (spec §12.1)"),
            );
        } else if !rule_names.insert(r.name.name.as_str()) {
            out.push(
                Diagnostic::error(
                    codes::BAD_MACRO,
                    format!("`syntax {}` is declared twice", r.name.name),
                )
                .at(file, r.name.span, "a second rule of this name"),
            );
        }
    }

    // Each rule's captures, for `$s.field` through a capture of that rule.
    let mut schemas: HashMap<String, HashMap<String, Cap>> = HashMap::new();
    for r in &decl.rules {
        let mut caps = HashMap::new();
        collect_caps(&r.pattern, &decl.rules, file, &mut caps, &mut out);
        schemas.insert(r.name.name.clone(), caps);
    }
    let mut top = HashMap::new();
    if let Some(p) = &decl.pattern {
        collect_caps(p, &decl.rules, file, &mut top, &mut out);
    }
    if let Some(t) = &decl.expand {
        let mut env: Vec<(String, Cap)> = Vec::new();
        template(t, &top, &schemas, &mut env, file, &mut out);
    }
    out
}

fn collect_caps(
    elems: &[PatElem],
    rules: &[SyntaxRule],
    file: FileId,
    caps: &mut HashMap<String, Cap>,
    out: &mut Vec<Diagnostic>,
) {
    for e in elems {
        match e {
            PatElem::Token(t) if t.kind == dz_lex::TokenKind::Dollar => {
                out.push(Diagnostic::error(codes::BAD_MACRO, "a `$` that begins no capture").at(
                    file,
                    t.span,
                    "expected `$name: kind` or `$( … )?`",
                ));
            }
            PatElem::Token(_) => {}
            PatElem::Capture { name, shape, kind, .. } => {
                let k = cap_kind(kind, rules);
                if k.is_none() && kind.name != "<error>" {
                    out.push(
                        Diagnostic::error(
                            codes::BAD_MACRO,
                            format!("`{}` is not a fragment kind or a `syntax` rule", kind.name),
                        )
                        .at(file, kind.span, "unknown kind")
                        .with_help(
                            "a capture is `ident`, `literal`, `type`, `expr`, `stmt`, `block`, \
                             `code`, `decl`, or a `syntax` rule of this macro (spec §12.2)",
                        ),
                    );
                }
                let cap = Cap { many: matches!(shape, CaptureShape::Many { .. }), kind: k };
                if caps.insert(name.name.clone(), cap).is_some() {
                    out.push(
                        Diagnostic::error(
                            codes::BAD_MACRO,
                            format!("`${}` is captured twice", name.name),
                        )
                        .at(
                            file,
                            name.span,
                            "a second capture of this name",
                        ),
                    );
                }
            }
            PatElem::Group { elems, .. } => collect_caps(elems, rules, file, caps, out),
        }
    }
}

/// What `$a.b.c` names, if it names a capture: the capture reached through the path, and
/// how many of the path's names that took. Names past those are Dazzle's own `.field`.
fn resolve<'a>(
    v: &VarRef,
    top: &'a HashMap<String, Cap>,
    schemas: &'a HashMap<String, HashMap<String, Cap>>,
    env: &'a [(String, Cap)],
) -> Option<(Cap, usize)> {
    let first = &v.path[0].name;
    let mut cap = env
        .iter()
        .rev()
        .find(|(n, _)| n == first)
        .map(|(_, c)| c.clone())
        .or_else(|| top.get(first).cloned())?;
    let mut used = 1;
    while used < v.path.len() {
        let Some(CapKind::Rule(r)) = &cap.kind else { break };
        if cap.many {
            break;
        }
        let Some(next) = schemas.get(r).and_then(|s| s.get(&v.path[used].name)) else { break };
        cap = next.clone();
        used += 1;
    }
    Some((cap, used))
}

fn template(
    nodes: &[TemplateNode],
    top: &HashMap<String, Cap>,
    schemas: &HashMap<String, HashMap<String, Cap>>,
    env: &mut Vec<(String, Cap)>,
    file: FileId,
    out: &mut Vec<Diagnostic>,
) {
    let unknown = |v: &VarRef, out: &mut Vec<Diagnostic>| {
        out.push(
            Diagnostic::error(
                codes::BAD_MACRO,
                format!("`${}` is not captured by this macro's pattern", v.path[0].name),
            )
            .at(file, v.span, "nothing captured by this name")
            .with_help("every `$name` in `expand` is a capture of `pattern`, or a `#for` variable"),
        );
    };
    for n in nodes {
        match n {
            TemplateNode::Token(_) => {}
            TemplateNode::Var(v) => {
                if resolve(v, top, schemas, env).is_none() {
                    unknown(v, out);
                }
            }
            TemplateNode::For { var, list, body, .. } => {
                let element = match resolve(list, top, schemas, env) {
                    None => {
                        unknown(list, out);
                        Cap { many: false, kind: None }
                    }
                    Some((cap, used)) if used < list.path.len() || !cap.many => {
                        out.push(
                            Diagnostic::error(
                                codes::BAD_MACRO,
                                "`#for` iterates a repeated capture",
                            )
                            .at(file, list.span, "not captured as `[]Kind` or `[,]Kind`")
                            .with_help(
                                "`#if $x { … }` is for something that is there or not \
                                 (spec §12.3)",
                            ),
                        );
                        Cap { many: false, kind: None }
                    }
                    Some((cap, _)) => Cap { many: false, kind: cap.kind },
                };
                env.push((var.name.clone(), element));
                template(body, top, schemas, env, file, out);
                env.pop();
            }
            TemplateNode::If { cond, then, otherwise, .. } => {
                if resolve(cond, top, schemas, env).is_none() {
                    unknown(cond, out);
                }
                template(then, top, schemas, env, file, out);
                if let Some(o) = otherwise {
                    template(o, top, schemas, env, file, out);
                }
            }
            TemplateNode::Error { at, .. } => {
                if let Some(v) = at {
                    if resolve(v, top, schemas, env).is_none() {
                        unknown(v, out);
                    }
                }
            }
        }
    }
}

/// The macro kind a declaration names, or what is wrong with it.
pub(crate) fn macro_kind(decl: &MacroDecl, file: FileId) -> Result<MacroKind, Box<Diagnostic>> {
    match decl.kind.name.as_str() {
        "decl" => Ok(MacroKind::Decl),
        "stmt" => Ok(MacroKind::Stmt),
        "expr" => Ok(MacroKind::Expr),
        "type" => Err(Box::new(
            Diagnostic::error(codes::BAD_MACRO, "a `type` macro is reserved and not yet available")
                .at(file, decl.kind.span, "not available yet")
                .with_help("a macro is `-> decl`, `-> stmt` or `-> expr` (spec §12.1)"),
        )),
        other => Err(Box::new(
            Diagnostic::error(codes::BAD_MACRO, format!("`{other}` is not a kind of macro"))
                .at(file, decl.kind.span, "unknown kind")
                .with_help(match other {
                    "declaration" => "write `-> decl` (spec §12.1)",
                    "statement" => "write `-> stmt` (spec §12.1)",
                    "expression" => "write `-> expr` (spec §12.1)",
                    _ => "a macro is `-> decl`, `-> stmt` or `-> expr` (spec §12.1)",
                }),
        )),
    }
}

/// The span a token run covers, from its first token to its last.
pub(crate) fn run_span(tokens: &[MacroToken]) -> Option<Span> {
    let first = tokens.first()?;
    let last = tokens.last()?;
    Some(first.span.to(last.span))
}
