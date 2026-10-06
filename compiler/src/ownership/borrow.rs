//! Loans, the storage paths they cover, and when an access conflicts with one.

use crate::hir;
use crate::mir::{Body, Local, Place, Projection, projection_type};
use crate::resolve::FieldId;
use crate::source::Span;
use crate::types::TypeKind;

/// A step along a storage path. Unlike a MIR projection, indexing a slice
/// first leaves the descriptor for the storage it views (`Deref`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PathElem {
    Field(FieldId),
    /// Any element; index values are never proof of disjointness (§12.6).
    Index,
    Deref,
}

/// A storage location, as a root local plus the steps taken from it.
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
                    // An `Array<T>` owns its elements like a fixed array, so
                    // replacing it must conflict with views of them.
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

    /// The storage a slice descriptor at this path views.
    pub(super) fn deref(mut self) -> Self {
        self.elems.push(PathElem::Deref);
        self
    }

    pub(super) fn goes_through_deref(&self) -> bool {
        self.elems.contains(&PathElem::Deref)
    }

    /// Whether one path is a prefix of the other, so they share storage.
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

/// One borrow, created at a slicing expression, a mutable-view copy, or a
/// call whose result borrows from an argument.
#[derive(Debug)]
pub(super) struct Loan {
    pub(super) kind: LoanKind,
    pub(super) target: LoanTarget,
    pub(super) span: Span,
    /// The borrowed place as written, for diagnostics.
    pub(super) name: String,
    pub(super) from_slicing: bool,
    /// A closure literal's capture created the loan.
    pub(super) captured: bool,
}

impl Loan {
    /// Whether assigning to `path` ends this loan: the loan borrows through a
    /// descriptor stored at `path`, which the assignment replaces.
    pub(super) fn ended_by_assignment_to(&self, path: &Path) -> bool {
        let LoanTarget::Place(borrowed) = &self.target else {
            return false;
        };
        borrowed.local == path.local
            && borrowed.elems.len() > path.elems.len()
            && borrowed.elems[path.elems.len()] == PathElem::Deref
            && borrowed.elems.iter().zip(&path.elems).all(|(a, b)| a == b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum AccessKind {
    Read,
    Write,
}

/// A shallow access touches a place's own storage but not what a slice
/// descriptor inside it views.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Depth {
    Shallow,
    Deep,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Use,
    /// Calling a closure, which uses it exclusively.
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

/// Whether performing `access` while `loan` is live violates exclusivity
/// (§11.3): reads conflict only with exclusive loans, writes with any.
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
