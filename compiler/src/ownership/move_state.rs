use crate::resolve::FieldId;
use crate::source::Span;

/// An empty path means the whole local moved and never coexists with other entries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct MovedSet {
    entries: Vec<(Vec<FieldId>, Span)>,
}

impl MovedSet {
    pub(super) fn moved_or_ancestor_moved(&self, fields: &[FieldId]) -> Option<Span> {
        self.entries
            .iter()
            .find(|(path, _)| {
                path.len() <= fields.len() && path.as_slice() == &fields[..path.len()]
            })
            .map(|&(_, span)| span)
    }

    pub(super) fn moved_descendant(&self, fields: &[FieldId]) -> Option<Span> {
        self.entries
            .iter()
            .find(|(path, _)| path.len() > fields.len() && &path[..fields.len()] == fields)
            .map(|&(_, span)| span)
    }

    pub(super) fn moved_strict_ancestor(&self, fields: &[FieldId]) -> Option<Span> {
        self.entries
            .iter()
            .find(|(path, _)| path.len() < fields.len() && path.as_slice() == &fields[..path.len()])
            .map(|&(_, span)| span)
    }

    pub(super) fn record_move(&mut self, fields: Vec<FieldId>, span: Span) {
        self.entries.push((fields, span));
    }

    pub(super) fn reinitialize(&mut self, fields: &[FieldId]) {
        self.entries
            .retain(|(path, _)| !(path.len() >= fields.len() && &path[..fields.len()] == fields));
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }

    /// A path moved on either side counts as moved.
    pub(super) fn join(&self, other: &Self) -> Self {
        let mut entries: Vec<(Vec<FieldId>, Span)> =
            self.entries.iter().chain(&other.entries).cloned().collect();
        entries.sort_by_key(|(path, span)| (path.len(), path.clone(), span.start(), span.end()));
        let mut reduced: Vec<(Vec<FieldId>, Span)> = Vec::new();
        for (path, span) in entries {
            let covered = reduced.iter().any(|(kept, _)| {
                kept.len() <= path.len() && kept.as_slice() == &path[..kept.len()]
            });
            if !covered {
                reduced.push((path, span));
            }
        }
        Self { entries: reduced }
    }
}

#[cfg(test)]
mod tests {
    use super::MovedSet;
    use crate::resolve::FieldId;
    use crate::source::{SourceMap, Span};

    fn two_spans() -> (Span, Span) {
        let mut sources = SourceMap::new();
        let file = sources.add("test.ore", "0123456789".to_string()).unwrap();
        (
            sources.span(file, 0, 1).unwrap(),
            sources.span(file, 2, 3).unwrap(),
        )
    }

    #[test]
    fn whole_local_move_absorbs_any_other_entry_on_join() {
        let (whole_span, field_span) = two_spans();
        let mut whole = MovedSet::default();
        whole.record_move(Vec::new(), whole_span);

        let mut field = MovedSet::default();
        field.record_move(vec![FieldId(0)], field_span);

        let joined = whole.join(&field);
        assert_eq!(joined.entries, vec![(Vec::new(), whole_span)]);

        let joined = field.join(&whole);
        assert_eq!(joined.entries, vec![(Vec::new(), whole_span)]);
    }
}
