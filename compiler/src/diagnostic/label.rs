use crate::source::Span;

#[derive(Debug)]
pub struct Label {
    pub(super) span: Span,
    pub(super) message: String,
}
