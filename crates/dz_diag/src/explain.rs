//! `dz explain <code>`: what a diagnostic's rule is, why, and a program that breaks it
//! beside one that does not (T-0062).
//!
//! Two sources, so neither can drift from the compiler:
//!
//! - **the rule** is the code's own doc comment in `codes.rs`, read from that file as it is
//!   compiled — the same sentence a reader of the compiler sees, spec section included;
//! - **the examples and the longer why** are in `explain/*.md`, one file per range, one
//!   `## DZNNNN` section per code: prose, then a ```` ```dz ```` block that is wrong and one
//!   that is right. `dz_driver`'s `explain` test compiles every block, and fails unless the
//!   wrong one reports exactly that code and the right one reports nothing.
//!
//! A code that no single-file program can produce — a manifest mistake, an encoding, a
//! C++ library failing to build — says so in a line beginning `No example:` instead.

/// Everything `dz explain` prints about one code.
#[derive(Clone, Debug)]
pub struct Explanation {
    pub code: &'static str,
    /// The constant's name, `USE_AFTER_MOVE`.
    pub name: &'static str,
    /// The doc comment on the constant, joined into paragraphs.
    pub rule: String,
    /// The section's prose, when it has any.
    pub why: String,
    /// A program that reports this code, and the same program put right.
    pub wrong: Option<String>,
    pub right: Option<String>,
    /// Why there are no examples, for a code no single-file program can produce.
    pub no_example: Option<String>,
}

/// The explanation files, embedded in the compiler.
const SOURCES: &[(&str, &str)] = &[
    ("project.md", include_str!("../explain/project.md")),
    ("lexer.md", include_str!("../explain/lexer.md")),
    ("parser.md", include_str!("../explain/parser.md")),
    ("resolve.md", include_str!("../explain/resolve.md")),
    ("flow.md", include_str!("../explain/flow.md")),
    ("check.md", include_str!("../explain/check.md")),
    ("generics.md", include_str!("../explain/generics.md")),
    ("macros.md", include_str!("../explain/macros.md")),
    ("interop.md", include_str!("../explain/interop.md")),
    ("codegen.md", include_str!("../explain/codegen.md")),
];

/// Every code `codes.rs` defines, in its order, with its constant's name and doc comment.
/// A retired number has no constant, so it is not here.
pub fn all_codes() -> Vec<(&'static str, &'static str, String)> {
    let text = include_str!("codes.rs");
    let mut out = Vec::new();
    let mut doc: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(d) = line.strip_prefix("///") {
            doc.push(d.trim());
            continue;
        }
        if let Some(rest) = line.strip_prefix("pub const ") {
            let name = rest.split(':').next().unwrap_or("").trim();
            let code = rest.split('"').nth(1).unwrap_or("");
            if let Some(code) = known(code) {
                out.push((code, static_name(text, name), doc.join(" ")));
            }
        }
        doc.clear();
    }
    out
}

/// `code` as the `&'static str` the compiler reports, if it is one.
fn known(code: &str) -> Option<&'static str> {
    let text = include_str!("codes.rs");
    let at = text.find(&format!("\"{code}\""))?;
    Some(&text[at + 1..at + 1 + code.len()])
}

/// The constant's name as a slice of the embedded file, so it lives as long as the file.
fn static_name(text: &'static str, name: &str) -> &'static str {
    let at = text.find(&format!("pub const {name}:")).map_or(0, |a| a + "pub const ".len());
    &text[at..at + name.len()]
}

/// Read `dz explain`'s argument: `DZ0401`, `dz0401`, `0401` and `401` all name one code.
pub fn normalise(arg: &str) -> Option<String> {
    let digits = arg.trim().trim_start_matches(['D', 'd']).trim_start_matches(['Z', 'z']);
    let n: u32 = digits.parse().ok()?;
    Some(format!("DZ{n:04}"))
}

/// The explanation of `code`, or `None` if no diagnostic has that code.
pub fn explain(code: &str) -> Option<Explanation> {
    let (code, name, rule) = all_codes().into_iter().find(|(c, _, _)| *c == code)?;
    let mut e = Explanation {
        code,
        name,
        rule,
        why: String::new(),
        wrong: None,
        right: None,
        no_example: None,
    };
    if let Some(section) = section(code) {
        let (why, blocks, no_example) = parse_section(section);
        e.why = why;
        let mut blocks = blocks.into_iter();
        e.wrong = blocks.next();
        e.right = blocks.next();
        e.no_example = no_example;
    }
    Some(e)
}

/// The text under `## code`, up to the next section, from whichever file has it.
pub fn section(code: &str) -> Option<&'static str> {
    let head = format!("## {code}");
    for (_, text) in SOURCES {
        let Some(at) = text.find(&head) else { continue };
        let body = &text[at + head.len()..];
        let end = body.find("\n## ").map_or(body.len(), |e| e + 1);
        return Some(&body[..end]);
    }
    None
}

/// Every `## DZNNNN` heading in the explanation files, with the file it is in — so a test
/// can find a section for a code that does not exist, or one written twice.
pub fn sections() -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for (file, text) in SOURCES {
        for line in text.lines() {
            if let Some(code) = line.strip_prefix("## ") {
                out.push((*file, code.trim().to_owned()));
            }
        }
    }
    out
}

/// A section's prose, its ```` ```dz ```` blocks in order, and its `No example:` line.
fn parse_section(section: &str) -> (String, Vec<String>, Option<String>) {
    let mut prose: Vec<&str> = Vec::new();
    let mut blocks = Vec::new();
    let mut no_example = None;
    let mut block: Option<Vec<&str>> = None;
    for line in section.lines() {
        match &mut block {
            Some(b) if line.trim_end() == "```" => {
                blocks.push(b.join("\n") + "\n");
                block = None;
            }
            Some(b) => b.push(line),
            None if line.trim_end() == "```dz" => block = Some(Vec::new()),
            None => {
                if let Some(why) = line.strip_prefix("No example:") {
                    no_example = Some(why.trim().to_owned());
                } else {
                    prose.push(line);
                }
            }
        }
    }
    let why = prose.join("\n").trim().to_owned();
    (why, blocks, no_example)
}

/// The explanation as `dz explain` prints it.
pub fn render(e: &Explanation) -> String {
    let mut out = format!("{}: {}\n\n", e.code, words(e.name));
    out.push_str(&wrap(&e.rule, 88));
    if !e.why.is_empty() {
        out.push('\n');
        out.push_str(&e.why);
        out.push('\n');
    }
    if let Some(w) = &e.wrong {
        out.push_str("\nThis is reported:\n\n");
        out.push_str(&indent(w));
    }
    if let Some(r) = &e.right {
        out.push_str("\nThis is not:\n\n");
        out.push_str(&indent(r));
    }
    if let Some(n) = &e.no_example {
        out.push_str(&format!("\nNo example: {n}\n"));
    }
    out
}

/// `USE_AFTER_MOVE` as "use after move".
fn words(name: &str) -> String {
    name.to_lowercase().replace('_', " ")
}

fn indent(code: &str) -> String {
    code.lines()
        .map(|l| if l.is_empty() { "\n".to_owned() } else { format!("    {l}\n") })
        .collect()
}

/// Fill `text` to `width` columns, at spaces.
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// One line per code — `DZ0401  use after move` — for `dz explain` with no argument.
pub fn index() -> String {
    let mut out = String::new();
    for (code, name, _) in all_codes() {
        out.push_str(&format!("{code}  {}\n", words(name)));
    }
    out
}
