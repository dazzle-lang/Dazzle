//! The diagnostic model and its renderer.
//!
//! The model is ours; the terminal rendering is delegated to `codespan-reporting`. That
//! split is deliberate: every phase of the compiler builds [`Diagnostic`] values, and the
//! renderer can be replaced (or joined by an LSP serialiser) without any of them changing.

use dz_span::{FileId, SourceMap, Span};

pub mod codes;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

/// A span with a message attached. The *primary* label is the one the caret points at.
#[derive(Clone, Debug)]
pub struct Label {
    pub file: FileId,
    pub span: Span,
    pub message: String,
    pub primary: bool,
}

impl Label {
    pub fn primary(file: FileId, span: Span, message: impl Into<String>) -> Label {
        Label { file, span, message: message.into(), primary: true }
    }

    pub fn secondary(file: FileId, span: Span, message: impl Into<String>) -> Label {
        Label { file, span, message: message.into(), primary: false }
    }
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable identifier such as `DZ0412`, so errors can be looked up and tested by name.
    pub code: Option<&'static str>,
    pub message: String,
    pub labels: Vec<Label>,
    /// Trailing `= note:` / `= help:` lines.
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code: Some(code),
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn warning(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            code: Some(code),
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn with_label(mut self, label: Label) -> Diagnostic {
        self.labels.push(label);
        self
    }

    /// Attach the primary span. Most diagnostics only need this.
    pub fn at(self, file: FileId, span: Span, message: impl Into<String>) -> Diagnostic {
        self.with_label(Label::primary(file, span, message))
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Diagnostic {
        self.notes.push(format!("note: {}", note.into()));
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Diagnostic {
        self.notes.push(format!("help: {}", help.into()));
        self
    }

    /// The span a caller should treat as "where this happened".
    pub fn primary_span(&self) -> Option<(FileId, Span)> {
        self.labels
            .iter()
            .find(|l| l.primary)
            .or_else(|| self.labels.first())
            .map(|l| (l.file, l.span))
    }
}

/// A collection of diagnostics, in the order they were reported.
#[derive(Default, Debug, Clone)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Diagnostics {
        Diagnostics::default()
    }

    pub fn push(&mut self, d: Diagnostic) {
        self.items.push(d);
    }

    pub fn extend(&mut self, other: impl IntoIterator<Item = Diagnostic>) {
        self.items.extend(other);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Drop everything after `len`, used to undo a speculative parse.
    pub fn truncate(&mut self, len: usize) {
        self.items.truncate(len);
    }

    pub fn error_count(&self) -> usize {
        self.items.iter().filter(|d| d.severity == Severity::Error).count()
    }

    pub fn warning_count(&self) -> usize {
        self.items.iter().filter(|d| d.severity == Severity::Warning).count()
    }

    pub fn has_errors(&self) -> bool {
        self.error_count() > 0
    }

    /// Sort by source position so output order does not depend on which phase ran first.
    pub fn sort_by_position(&mut self) {
        self.items.sort_by_key(|d| {
            d.primary_span().map(|(f, s)| (f.0, s.start, s.end)).unwrap_or((u32::MAX, 0, 0))
        });
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Adapter letting `codespan-reporting` read a [`SourceMap`].
struct MapFiles<'a>(&'a SourceMap);

impl<'a> codespan_reporting::files::Files<'a> for MapFiles<'a> {
    type FileId = FileId;
    type Name = String;
    type Source = &'a str;

    fn name(&'a self, id: FileId) -> Result<String, codespan_reporting::files::Error> {
        self.0
            .try_get(id)
            .map(|f| f.name())
            .ok_or(codespan_reporting::files::Error::FileMissing)
    }

    fn source(&'a self, id: FileId) -> Result<&'a str, codespan_reporting::files::Error> {
        self.0
            .try_get(id)
            .map(|f| f.text())
            .ok_or(codespan_reporting::files::Error::FileMissing)
    }

    fn line_index(
        &'a self,
        id: FileId,
        byte_index: usize,
    ) -> Result<usize, codespan_reporting::files::Error> {
        self.0
            .try_get(id)
            .map(|f| f.line_index(byte_index))
            .ok_or(codespan_reporting::files::Error::FileMissing)
    }

    fn line_range(
        &'a self,
        id: FileId,
        line_index: usize,
    ) -> Result<std::ops::Range<usize>, codespan_reporting::files::Error> {
        self.0
            .try_get(id)
            .ok_or(codespan_reporting::files::Error::FileMissing)?
            .line_range(line_index)
            .ok_or(codespan_reporting::files::Error::LineTooLarge {
                given: line_index,
                max: self.0.get(id).line_count(),
            })
    }
}

fn to_codespan(d: &Diagnostic) -> codespan_reporting::diagnostic::Diagnostic<FileId> {
    use codespan_reporting::diagnostic as cs;

    let mut out = match d.severity {
        Severity::Error => cs::Diagnostic::error(),
        Severity::Warning => cs::Diagnostic::warning(),
        Severity::Note => cs::Diagnostic::note(),
        Severity::Help => cs::Diagnostic::help(),
    };
    out = out.with_message(&d.message);
    if let Some(code) = d.code {
        out = out.with_code(code);
    }
    out = out.with_labels(
        d.labels
            .iter()
            .map(|l| {
                let style = if l.primary {
                    cs::LabelStyle::Primary
                } else {
                    cs::LabelStyle::Secondary
                };
                cs::Label::new(style, l.file, l.span.range()).with_message(&l.message)
            })
            .collect(),
    );
    out.with_notes(d.notes.clone())
}

/// Render diagnostics to a plain string with no ANSI colour — used by snapshot tests.
pub fn render_to_string(map: &SourceMap, diags: &Diagnostics) -> String {
    use codespan_reporting::term::{self, termcolor::NoColor};

    let files = MapFiles(map);
    let config = term::Config::default();
    let mut buf = NoColor::new(Vec::new());
    for d in diags.iter() {
        // Writing into an in-memory buffer cannot fail for well-formed spans; if it ever
        // does, surfacing it as text beats silently dropping the diagnostic.
        if let Err(e) = term::emit(&mut buf, &config, &files, &to_codespan(d)) {
            let _ = std::io::Write::write_fmt(
                &mut buf,
                format_args!("<failed to render diagnostic: {e}>\n"),
            );
        }
    }
    String::from_utf8_lossy(&buf.into_inner()).into_owned()
}

/// Print diagnostics to stderr, in colour when stderr is a terminal.
pub fn emit_to_stderr(map: &SourceMap, diags: &Diagnostics) {
    use codespan_reporting::term::{
        self,
        termcolor::{ColorChoice, StandardStream},
    };

    let files = MapFiles(map);
    let config = term::Config::default();
    let mut stream = StandardStream::stderr(ColorChoice::Auto);
    for d in diags.iter() {
        let _ = term::emit(&mut stream, &config, &files, &to_codespan(d));
    }
}
