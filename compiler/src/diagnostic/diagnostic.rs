use super::label::Label;
use crate::source::Span;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug)]
pub struct Diagnostic {
    pub(super) severity: Severity,
    pub(super) message: String,
    pub(super) primary: Label,
    pub(super) related: Vec<Label>,
    pub(super) notes: Vec<String>,
}

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

    pub fn primary_label(&self) -> &str {
        &self.primary.message
    }

    pub fn related_labels(&self) -> impl Iterator<Item = (Span, &str)> {
        self.related
            .iter()
            .map(|label| (label.span, label.message.as_str()))
    }
}
