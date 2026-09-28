//! Human-readable diagnostics tied to validated source spans.

use crate::source::{SourceMap, Span};
use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug)]
pub struct Label {
    span: Span,
    message: String,
}

#[derive(Debug)]
pub struct Diagnostic {
    severity: Severity,
    message: String,
    primary: Label,
    related: Vec<Label>,
    notes: Vec<String>,
}

#[derive(Debug)]
pub struct RenderError;

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("diagnostic span is not in this source manager")
    }
}

impl Error for RenderError {}

impl Diagnostic {
    pub fn new(severity: Severity, message: impl Into<String>, span: Span) -> Self {
        Self {
            severity,
            message: message.into(),
            primary: Label {
                span,
                message: String::new(),
            },
            related: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn primary_message(mut self, message: impl Into<String>) -> Self {
        self.primary.message = message.into();
        self
    }

    pub fn related(mut self, span: Span, message: impl Into<String>) -> Self {
        self.related.push(Label {
            span,
            message: message.into(),
        });
        self
    }

    pub fn note(mut self, message: impl Into<String>) -> Self {
        self.notes.push(message.into());
        self
    }

    pub fn severity(&self) -> Severity {
        self.severity
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn span(&self) -> Span {
        self.primary.span
    }

    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    pub fn render(&self, sources: &SourceMap) -> Result<String, RenderError> {
        let mut output = format!("{}: {}\n", self.severity.name(), self.message);
        render_label(&mut output, sources, &self.primary, "-->")?;
        for label in &self.related {
            render_label(&mut output, sources, label, ":::")?;
        }
        for note in &self.notes {
            output.push_str(&format!("  = note: {note}\n"));
        }
        Ok(output)
    }
}

fn render_label(
    output: &mut String,
    sources: &SourceMap,
    label: &Label,
    arrow: &str,
) -> Result<(), RenderError> {
    let file = sources.file(label.span.file()).ok_or(RenderError)?;
    // A span from another source map must not resolve against this one.
    if sources.slice(label.span).is_none() {
        return Err(RenderError);
    }
    let start = file.location(label.span.start()).ok_or(RenderError)?;
    let end = file.location(label.span.end()).ok_or(RenderError)?;
    let line = file.line(start.line - 1).ok_or(RenderError)?;
    let (display, boundaries) = display_line(line);
    let start_scalar = (start.column - 1).min(boundaries.len() - 1);
    let visible_end = if start.line == end.line {
        (end.column - 1).min(boundaries.len() - 1)
    } else {
        boundaries.len() - 1
    };
    let marker_start = boundaries[start_scalar];
    let marker_end = boundaries[visible_end].max(marker_start + 1);
    let gutter = start.line.to_string().len();

    output.push_str(&format!(
        " {arrow} {}:{}:{}\n",
        file.path().display(),
        start.line,
        start.column
    ));
    output.push_str(&format!("{:gutter$} |\n", ""));
    output.push_str(&format!("{} | {display}\n", start.line));
    output.push_str(&format!(
        "{:gutter$} | {}{}",
        "",
        " ".repeat(marker_start),
        "^".repeat(marker_end - marker_start)
    ));
    if !label.message.is_empty() {
        output.push(' ');
        output.push_str(&label.message);
    }
    output.push('\n');
    if end.line > start.line {
        output.push_str(&format!(
            "{:gutter$} = span continues through line {}\n",
            "", end.line
        ));
    }
    Ok(())
}

fn display_line(line: &str) -> (String, Vec<usize>) {
    let mut display = String::new();
    let mut boundaries = vec![0];
    let mut columns = 0;
    for ch in line.chars() {
        let piece = if ch == '\t' {
            " ".repeat(4 - columns % 4)
        } else if ch.is_control() {
            ch.escape_default().to_string()
        } else {
            ch.to_string()
        };
        columns += piece.chars().count();
        display.push_str(&piece);
        boundaries.push(columns);
    }
    (display, boundaries)
}
