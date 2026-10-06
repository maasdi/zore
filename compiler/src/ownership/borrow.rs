use crate::hir;
use crate::mir::{Body, Local, Place, Projection, projection_type};
use crate::resolve::FieldId;
use crate::source::Span;
use crate::types::TypeKind;

/// Indexing a slice first steps through its descriptor (`Deref`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PathElem {
    Field(FieldId),
    /// Index values never prove disjointness.
    Index,
    Deref,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Path {
    pub(super) local: Local,
    pub(super) elems: Vec<PathElem>,
}

impl Path {
    pub(super) fn of(package: &hir::Package, body: &Body, place: &Place) -> Self {
        let mut ty = body.locals[place.local.0 as usize].ty;
        let mut elems = Vec::with_capacity(place.projections.len());
        for projection in &place.projections {
            match projection {
                Projection::Field(field) => elems.push(PathElem::Field(*field)),
                Projection::Index(_) => {
                    // `Array<T>` owns its elements, so replacing it conflicts with views of them.
                    if matches!(package.types.kind(ty), TypeKind::Slice { .. }) {
                        elems.push(PathElem::Deref);
                    }
                    elems.push(PathElem::Index);
                }
            }
            ty = projection_type(package, ty, projection);
        }
        Self {
            local: place.local,
            elems,
        }
    }

    pub(super) fn deref(mut self) -> Self {
        self.elems.push(PathElem::Deref);
        self
    }

    pub(super) fn goes_through_deref(&self) -> bool {
        self.elems.contains(&PathElem::Deref)
    }

    fn overlaps(&self, other: &Path) -> bool {
        self.local == other.local && self.elems.iter().zip(&other.elems).all(|(a, b)| a == b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum LoanKind {
    Shared,
    Exclusive,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum LoanTarget {
    Place(Path),
    /// Storage outside the function that parameter `i`'s views already borrow.
    Param(usize),
}

#[derive(Debug)]
pub(super) struct Loan {
    pub(super) kind: LoanKind,
    pub(super) target: LoanTarget,
    pub(super) span: Span,
    /// As written, for diagnostics.
    pub(super) name: String,
    pub(super) from_slicing: bool,
    pub(super) captured: bool,
}

impl Loan {
    /// The loan borrows through a descriptor stored at `path`.
    pub(super) fn ended_by_assignment_to(&self, path: &Path) -> bool {
        let LoanTarget::Place(borrowed) = &self.target else {
            return false;
        };
        borrowed.local == path.local
            && borrowed.elems.len() > path.elems.len()
            && borrowed.elems[path.elems.len()..].contains(&PathElem::Deref)
            && borrowed.elems.iter().zip(&path.elems).all(|(a, b)| a == b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum AccessKind {
    Read,
    Write,
}

/// Shallow accesses skip what a slice descriptor inside the place views.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Depth {
    Shallow,
    Deep,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Use,
    Call,
    Assign,
    Borrow,
    MutBorrow,
    Move,
    StorageDead,
}

#[derive(Debug)]
pub(super) struct Access {
    pub(super) path: Path,
    pub(super) kind: AccessKind,
    pub(super) depth: Depth,
    pub(super) action: Action,
    pub(super) span: Span,
    pub(super) name: String,
    pub(super) from_slicing: bool,
}

/// Reads conflict only with exclusive loans, writes with any.
pub(super) fn conflicts(loan: &Loan, access: &Access) -> bool {
    let LoanTarget::Place(borrowed) = &loan.target else {
        return false;
    };
    if !borrowed.overlaps(&access.path) {
        return false;
    }
    let beyond_access = &borrowed.elems[borrowed.elems.len().min(access.path.elems.len())..];
    if access.depth == Depth::Shallow && beyond_access.contains(&PathElem::Deref) {
        return false;
    }
    !(access.kind == AccessKind::Read && loan.kind == LoanKind::Shared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceMap;

    fn span() -> Span {
        let mut sources = SourceMap::new();
        let file = sources.add("test.ore", "x".to_string()).unwrap();
        sources.span(file, 0, 1).unwrap()
    }

    fn path(elems: &[PathElem]) -> Path {
        Path {
            local: Local(0),
            elems: elems.to_vec(),
        }
    }

    fn loan(kind: LoanKind, elems: &[PathElem]) -> Loan {
        Loan {
            kind,
            target: LoanTarget::Place(path(elems)),
            span: span(),
            name: "s".into(),
            from_slicing: true,
            captured: false,
        }
    }

    fn access(kind: AccessKind, depth: Depth, elems: &[PathElem]) -> Access {
        Access {
            path: path(elems),
            kind,
            depth,
            action: Action::Use,
            span: span(),
            name: "s".into(),
            from_slicing: false,
        }
    }

    #[test]
    fn reads_conflict_only_with_exclusive_loans() {
        let read = access(AccessKind::Read, Depth::Deep, &[PathElem::Index]);
        assert!(!conflicts(&loan(LoanKind::Shared, &[]), &read));
        assert!(conflicts(&loan(LoanKind::Exclusive, &[]), &read));
        let write = access(AccessKind::Write, Depth::Deep, &[PathElem::Index]);
        assert!(conflicts(&loan(LoanKind::Shared, &[]), &write));
    }

    #[test]
    fn shallow_accesses_skip_storage_behind_a_descriptor() {
        let reborrow = loan(LoanKind::Exclusive, &[PathElem::Deref]);
        let overwrite = access(AccessKind::Write, Depth::Shallow, &[]);
        assert!(!conflicts(&reborrow, &overwrite));
        let element_write = access(
            AccessKind::Write,
            Depth::Shallow,
            &[PathElem::Deref, PathElem::Index],
        );
        assert!(conflicts(&reborrow, &element_write));
        let copy = access(AccessKind::Read, Depth::Deep, &[]);
        assert!(conflicts(&reborrow, &copy));
    }

    #[test]
    fn assigning_a_descriptor_ends_loans_through_it() {
        let reborrow = loan(LoanKind::Exclusive, &[PathElem::Deref, PathElem::Index]);
        assert!(reborrow.ended_by_assignment_to(&path(&[])));
        assert!(!loan(LoanKind::Shared, &[PathElem::Index]).ended_by_assignment_to(&path(&[])));
    }
}
