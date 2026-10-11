use crate::source::Span;

#[derive(Debug, PartialEq)]
pub struct Label {
    pub(super) span: Span,
    pub(super) message: String,
}
