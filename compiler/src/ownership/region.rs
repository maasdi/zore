//! Region analysis: which borrows each local's views hold, where those views
//! are live, and the exclusivity conflicts and escapes that follow (§11.3–11.7,
//! §12.3, §12.5). A loan stays live exactly while some live local holds it.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::borrow::{
    Access, AccessKind, Action, Depth, Loan, LoanKind, LoanTarget, Path, conflicts,
};
use super::checker::describe_place;
use crate::ast::ParamMode;
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir;
use crate::mir::{
    BasicBlock, BlockId, Body, Callee, Local, Operand, Place, Program, Projection, Rvalue,
    Statement, Terminator, place_type,
};
use crate::resolve::LocalKind;
use crate::source::Span;
use crate::types::{TypeId, TypeKind};

/// For one result of a function, how each parameter can back it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Origin {
    /// The result views the argument's own storage (a by-reference parameter).
    storage: bool,
    /// The result forwards views the argument already holds.
    contents: bool,
}

/// A function's return-borrow contract (§11.7): `results[r][p]`.
#[derive(Clone, Debug, PartialEq)]
struct Contract {
    results: Vec<Vec<Origin>>,
}

pub(super) fn check(package: &hir::Package, program: &Program, diagnostics: &mut Vec<Diagnostic>) {
    let mut contracts: Vec<Contract> = program
        .bodies
        .iter()
        .map(|body| Contract {
            results: vec![vec![Origin::default(); body.params.len()]; body.returns.len()],
        })
        .collect();
    // Least fixpoint, so mutually recursive functions get consistent contracts.
    loop {
        let mut changed = false;
        for (index, body) in program.bodies.iter().enumerate() {
            let (contract, _) = Analysis::new(package, body, &contracts).run();
            if contract != contracts[index] {
                contracts[index] = contract;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for body in &program.bodies {
        diagnostics.extend(Analysis::new(package, body, &contracts).run().1);
    }
}

type LoanId = usize;
type Holdings = Vec<BTreeSet<LoanId>>;
type LiveSet = Vec<bool>;

/// Identifies where a loan is created, so repeated passes reuse its ID.
type LoanKey = (u32, usize, usize);

const ENTRY_BLOCK_KEY: u32 = u32::MAX;

struct Analysis<'a> {
    package: &'a hir::Package,
    body: &'a Body,
    contracts: &'a [Contract],
    loans: Vec<Loan>,
    interned: HashMap<LoanKey, LoanId>,
}

/// Where loans created by the current statement are keyed.
struct Site {
    block: u32,
    position: usize,
    ordinal: usize,
    span: Span,
}

impl Site {
    fn next_key(&mut self) -> LoanKey {
        self.ordinal += 1;
        (self.block, self.position, self.ordinal)
    }
}

/// Diagnostics and contract facts gathered on the final pass over a body.
struct Findings<'l> {
    live: &'l Liveness,
    contract: Contract,
    diagnostics: Vec<Diagnostic>,
    reported: HashSet<Span>,
}

impl<'a> Analysis<'a> {
    fn new(package: &'a hir::Package, body: &'a Body, contracts: &'a [Contract]) -> Self {
        Self {
            package,
            body,
            contracts,
            loans: Vec::new(),
            interned: HashMap::new(),
        }
    }

    fn run(mut self) -> (Contract, Vec<Diagnostic>) {
        let body = self.body;
        let empty_contract = Contract {
            results: vec![vec![Origin::default(); body.params.len()]; body.returns.len()],
        };
        if body.blocks.is_empty() {
            return (empty_contract, Vec::new());
        }
        let mut entry: Holdings = vec![BTreeSet::new(); body.locals.len()];
        for (index, &param) in body.params.iter().enumerate() {
            if self.carries_views(param) {
                let id = self.intern(
                    (ENTRY_BLOCK_KEY, index, 0),
                    Loan {
                        kind: LoanKind::Shared,
                        target: LoanTarget::Param(index),
                        span: self.package.function(body.function).span,
                        name: String::new(),
                        from_slicing: false,
                    },
                );
                entry[param.0 as usize].insert(id);
            }
        }
        let mut incoming: Vec<Option<Holdings>> = vec![None; body.blocks.len()];
        incoming[0] = Some(entry);
        let mut changed = true;
        while changed {
            changed = false;
            for (index, block) in body.blocks.iter().enumerate() {
                let Some(mut state) = incoming[index].clone() else {
                    continue;
                };
                self.transfer(BlockId(index as u32), block, &mut state, None);
                for successor in block.terminator.successors() {
                    let slot = &mut incoming[successor.0 as usize];
                    let joined = match slot {
                        Some(previous) => previous
                            .iter()
                            .zip(&state)
                            .map(|(a, b)| a.union(b).copied().collect())
                            .collect(),
                        None => state.clone(),
                    };
                    if slot.as_ref() != Some(&joined) {
                        *slot = Some(joined);
                        changed = true;
                    }
                }
            }
        }
        let live = Liveness::compute(body);
        let mut findings = Findings {
            live: &live,
            contract: empty_contract,
            diagnostics: Vec::new(),
            reported: HashSet::new(),
        };
        for (index, (block, state)) in body.blocks.iter().zip(incoming).enumerate() {
            if let Some(mut state) = state {
                self.transfer(
                    BlockId(index as u32),
                    block,
                    &mut state,
                    Some(&mut findings),
                );
            }
        }
        (findings.contract, findings.diagnostics)
    }

    fn intern(&mut self, key: LoanKey, loan: Loan) -> LoanId {
        if let Some(&id) = self.interned.get(&key) {
            return id;
        }
        self.loans.push(loan);
        let id = self.loans.len() - 1;
        self.interned.insert(key, id);
        id
    }

    fn local_ty(&self, local: Local) -> TypeId {
        self.body.locals[local.0 as usize].ty
    }

    fn place_ty(&self, place: &Place) -> TypeId {
        place_type(self.package, &self.body.locals, place)
    }

    fn carries_views(&self, local: Local) -> bool {
        self.package.contains_view(self.local_ty(local))
    }

    fn is_mut_slice(&self, ty: TypeId) -> bool {
        matches!(
            self.package.types.kind(ty),
            TypeKind::Slice { mutable: true, .. }
        )
    }

    fn describe(&self, place: &Place) -> String {
        describe_place(self.package, self.body, place)
    }

    fn transfer(
        &mut self,
        block_id: BlockId,
        block: &BasicBlock,
        state: &mut Holdings,
        mut findings: Option<&mut Findings>,
    ) {
        for (position, statement) in block.statements.iter().enumerate() {
            match statement {
                Statement::Assign {
                    place,
                    rvalue,
                    span,
                } => {
                    let mut site = Site {
                        block: block_id.0,
                        position,
                        ordinal: 0,
                        span: *span,
                    };
                    self.assign_statement(place, rvalue, &mut site, state, findings.as_deref_mut());
                }
                Statement::EndScope(locals) => {
                    if let Some(findings) = findings.as_deref_mut() {
                        let live_after = findings.live.before(block_id, position + 1);
                        for &local in locals {
                            let place = Place::local(local);
                            let access = Access {
                                path: Path::of(self.package, self.body, &place),
                                kind: AccessKind::Write,
                                depth: Depth::Shallow,
                                action: Action::StorageDead,
                                span: self.scope_end_span(local),
                                name: self.describe(&place),
                                from_slicing: false,
                            };
                            self.check_access(&access, live_after, state, findings);
                        }
                    }
                    for local in locals {
                        state[local.0 as usize].clear();
                    }
                }
                Statement::Drop { .. } => {}
            }
        }
        let position = block.statements.len();
        let mut site = Site {
            block: block_id.0,
            position,
            ordinal: 0,
            span: self.package.function(self.body.function).span,
        };
        match &block.terminator {
            Terminator::Assert {
                place,
                rvalue,
                span,
                ..
            } => {
                site.span = *span;
                self.assign_statement(place, rvalue, &mut site, state, findings);
            }
            Terminator::Branch {
                condition, span, ..
            } => {
                if let Some(findings) = findings {
                    let mut accesses = Vec::new();
                    self.operand_accesses(condition, None, *span, &mut accesses);
                    let live = findings.live.before(block_id, position);
                    for access in &accesses {
                        self.check_access(access, live, state, findings);
                    }
                }
            }
            Terminator::Call {
                callee,
                args,
                destinations,
                span,
                ..
            } => {
                site.span = *span;
                self.call(callee, args, destinations, &mut site, state, findings);
            }
            Terminator::Return => {
                if let Some(findings) = findings {
                    self.return_origins(state, findings);
                }
            }
            Terminator::Goto(_) | Terminator::PanicReturn | Terminator::Unreachable => {}
        }
    }

    /// The span a scope-end access is reported at: the binding's declaration.
    fn scope_end_span(&self, local: Local) -> Span {
        let function = self.package.function(self.body.function);
        function
            .locals
            .get(local.0 as usize)
            .map_or(function.span, |decl| decl.span)
    }

    fn assign_statement(
        &mut self,
        place: &Place,
        rvalue: &Rvalue,
        site: &mut Site,
        state: &mut Holdings,
        findings: Option<&mut Findings>,
    ) {
        if let Some(findings) = findings {
            let mut accesses = Vec::new();
            self.rvalue_accesses(rvalue, site.span, &mut accesses);
            self.target_accesses(place, site.span, &mut accesses);
            let live = findings.live.before(BlockId(site.block), site.position);
            for access in &accesses {
                self.check_access(access, live, state, findings);
            }
            self.check_view_store(place, site.span, findings);
        }
        let loans = self.rvalue_loans(rvalue, site, state);
        self.assign(place, loans, state);
    }

    fn call(
        &mut self,
        callee: &Callee,
        args: &[Operand],
        destinations: &[Option<Place>],
        site: &mut Site,
        state: &mut Holdings,
        findings: Option<&mut Findings>,
    ) {
        let modes: Vec<Option<ParamMode>> = match callee {
            Callee::Function(id) => {
                let function = self.package.function(*id);
                function
                    .params
                    .iter()
                    .map(|param| match function.locals[param.0 as usize].kind {
                        LocalKind::Param(mode) => Some(mode),
                        _ => None,
                    })
                    .collect()
            }
            Callee::Println | Callee::Drop => vec![None; args.len()],
        };
        if let Some(findings) = findings {
            let mut accesses = Vec::new();
            for (arg, mode) in args.iter().zip(&modes) {
                self.operand_accesses(arg, *mode, site.span, &mut accesses);
            }
            for destination in destinations.iter().flatten() {
                self.target_accesses(destination, site.span, &mut accesses);
                self.check_view_store(destination, site.span, findings);
            }
            let live = findings.live.before(BlockId(site.block), site.position);
            for access in &accesses {
                self.check_access(access, live, state, findings);
            }
        }
        let Callee::Function(id) = callee else {
            return;
        };
        let results = &self.package.function(*id).results;
        for (index, destination) in destinations.iter().enumerate() {
            let Some(destination) = destination else {
                continue;
            };
            let mut loans = BTreeSet::new();
            if self.package.contains_view(results[index]) {
                let kind = if self.package.contains_mut_view(results[index]) {
                    LoanKind::Exclusive
                } else {
                    LoanKind::Shared
                };
                let origins = self.contracts[id.0 as usize].results[index].clone();
                for (arg, origin) in args.iter().zip(origins) {
                    let (Operand::Copy(place) | Operand::Move(place) | Operand::Ref(place)) = arg
                    else {
                        continue;
                    };
                    if origin.storage && matches!(arg, Operand::Ref(_)) {
                        let path = Path::of(self.package, self.body, place);
                        loans.extend(self.inherited_loans(place, &path, state));
                        let loan = Loan {
                            kind,
                            target: LoanTarget::Place(path),
                            span: site.span,
                            name: self.describe(place),
                            from_slicing: false,
                        };
                        loans.insert(self.intern(site.next_key(), loan));
                    }
                    if origin.contents && self.package.contains_view(self.place_ty(place)) {
                        loans.extend(state[place.local.0 as usize].iter().copied());
                    }
                }
            }
            self.assign(destination, loans, state);
        }
    }

    /// Stores `loans` as what `target` now holds, then ends every loan that
    /// borrowed through the descriptor the assignment replaced.
    fn assign(&mut self, target: &Place, loans: BTreeSet<LoanId>, state: &mut Holdings) {
        let root = target.local.0 as usize;
        if self.carries_views(target.local) {
            if target.projections.is_empty() {
                state[root] = loans;
            } else {
                state[root].extend(loans);
            }
        }
        let path = Path::of(self.package, self.body, target);
        for held in state.iter_mut() {
            held.retain(|&id| !self.loans[id].ended_by_assignment_to(&path));
        }
    }

    /// The loans a new borrow of `place` (covering `path`) must carry over
    /// from its root: storage reached through a view stays borrowed from that
    /// view's backing, and a value holding views keeps their provenance.
    fn inherited_loans(&self, place: &Place, path: &Path, state: &Holdings) -> BTreeSet<LoanId> {
        if path.goes_through_deref() || self.package.contains_view(self.place_ty(place)) {
            state[place.local.0 as usize].clone()
        } else {
            BTreeSet::new()
        }
    }

    fn operand_loans(
        &mut self,
        operand: &Operand,
        site: &mut Site,
        state: &Holdings,
    ) -> BTreeSet<LoanId> {
        let (Operand::Copy(place) | Operand::Move(place)) = operand else {
            return BTreeSet::new();
        };
        let ty = self.place_ty(place);
        if !self.package.contains_view(ty) {
            return BTreeSet::new();
        }
        let mut loans = state[place.local.0 as usize].clone();
        if matches!(operand, Operand::Copy(_)) && self.is_mut_slice(ty) {
            // §12.3: copying a mutable view is an exclusive reborrow.
            let loan = Loan {
                kind: LoanKind::Exclusive,
                target: LoanTarget::Place(Path::of(self.package, self.body, place).deref()),
                span: site.span,
                name: self.describe(place),
                from_slicing: false,
            };
            loans.insert(self.intern(site.next_key(), loan));
        }
        loans
    }

    fn rvalue_loans(
        &mut self,
        rvalue: &Rvalue,
        site: &mut Site,
        state: &Holdings,
    ) -> BTreeSet<LoanId> {
        match rvalue {
            Rvalue::Use(operand) => self.operand_loans(operand, site, state),
            Rvalue::Aggregate(_, operands) => {
                let mut loans = BTreeSet::new();
                for operand in operands {
                    loans.extend(self.operand_loans(operand, site, state));
                }
                loans
            }
            Rvalue::Slice { place, mutable, .. } => {
                let base_is_slice = matches!(
                    self.package.types.kind(self.place_ty(place)),
                    TypeKind::Slice { .. }
                );
                let path = Path::of(self.package, self.body, place);
                let path = if base_is_slice { path.deref() } else { path };
                let inherited = self.inherited_loans(place, &path, state);
                let loan = Loan {
                    kind: if *mutable {
                        LoanKind::Exclusive
                    } else {
                        LoanKind::Shared
                    },
                    target: LoanTarget::Place(path),
                    span: site.span,
                    name: self.describe(place),
                    from_slicing: true,
                };
                let mut loans = BTreeSet::from([self.intern(site.next_key(), loan)]);
                loans.extend(inherited);
                loans
            }
            Rvalue::Zero
            | Rvalue::Binary(..)
            | Rvalue::Unary(..)
            | Rvalue::Convert(..)
            | Rvalue::Error(_)
            | Rvalue::BoundsCheck(..)
            | Rvalue::Length(_) => BTreeSet::new(),
        }
    }

    fn operand_accesses(
        &self,
        operand: &Operand,
        mode: Option<ParamMode>,
        span: Span,
        out: &mut Vec<Access>,
    ) {
        let (place, kind, action) = match operand {
            Operand::Const(..) => return,
            Operand::Copy(place) if self.is_mut_slice(self.place_ty(place)) => {
                (place, AccessKind::Write, Action::MutBorrow)
            }
            Operand::Copy(place) => (place, AccessKind::Read, Action::Use),
            Operand::Move(place) => (place, AccessKind::Write, Action::Move),
            Operand::Ref(place) if mode == Some(ParamMode::Mut) => {
                (place, AccessKind::Write, Action::MutBorrow)
            }
            Operand::Ref(place) => (place, AccessKind::Read, Action::Borrow),
        };
        self.index_accesses(place, span, out);
        out.push(self.access(place, kind, Depth::Deep, action, span, false));
    }

    fn index_accesses(&self, place: &Place, span: Span, out: &mut Vec<Access>) {
        for projection in &place.projections {
            if let Projection::Index(index) = projection {
                self.operand_accesses(index, None, span, out);
            }
        }
    }

    fn rvalue_accesses(&self, rvalue: &Rvalue, span: Span, out: &mut Vec<Access>) {
        match rvalue {
            Rvalue::Zero => {}
            Rvalue::Use(operand)
            | Rvalue::Unary(_, operand)
            | Rvalue::Convert(operand, _)
            | Rvalue::Error(operand) => self.operand_accesses(operand, None, span, out),
            Rvalue::Binary(_, left, right) | Rvalue::BoundsCheck(left, right) => {
                self.operand_accesses(left, None, span, out);
                self.operand_accesses(right, None, span, out);
            }
            Rvalue::Aggregate(_, operands) => {
                for operand in operands {
                    self.operand_accesses(operand, None, span, out);
                }
            }
            Rvalue::Length(place) => {
                self.index_accesses(place, span, out);
                out.push(self.access(
                    place,
                    AccessKind::Read,
                    Depth::Shallow,
                    Action::Use,
                    span,
                    false,
                ));
            }
            Rvalue::Slice {
                place,
                low,
                high,
                mutable,
            } => {
                self.index_accesses(place, span, out);
                let (kind, action) = if *mutable {
                    (AccessKind::Write, Action::MutBorrow)
                } else {
                    (AccessKind::Read, Action::Borrow)
                };
                out.push(self.access(place, kind, Depth::Deep, action, span, true));
                for bound in [low, high].into_iter().flatten() {
                    self.operand_accesses(bound, None, span, out);
                }
            }
        }
    }

    fn target_accesses(&self, target: &Place, span: Span, out: &mut Vec<Access>) {
        self.index_accesses(target, span, out);
        out.push(self.access(
            target,
            AccessKind::Write,
            Depth::Shallow,
            Action::Assign,
            span,
            false,
        ));
    }

    fn access(
        &self,
        place: &Place,
        kind: AccessKind,
        depth: Depth,
        action: Action,
        span: Span,
        from_slicing: bool,
    ) -> Access {
        Access {
            path: Path::of(self.package, self.body, place),
            kind,
            depth,
            action,
            span,
            name: self.describe(place),
            from_slicing,
        }
    }

    fn check_access(
        &self,
        access: &Access,
        live: &LiveSet,
        state: &Holdings,
        findings: &mut Findings,
    ) {
        for (holder, held) in state.iter().enumerate() {
            if !live[holder] {
                continue;
            }
            for &id in held {
                let loan = &self.loans[id];
                if !conflicts(loan, access) {
                    continue;
                }
                let diagnostic = self.conflict_diagnostic(loan, access, Local(holder as u32));
                if findings.reported.insert(diagnostic.span()) {
                    findings.diagnostics.push(diagnostic);
                }
                return;
            }
        }
    }

    fn conflict_diagnostic(&self, loan: &Loan, access: &Access, holder: Local) -> Diagnostic {
        let name = &access.name;
        let exclusive = loan.kind == LoanKind::Exclusive;
        let message = match access.action {
            Action::Use if exclusive => format!("cannot use `{name}` while it is mutably borrowed"),
            Action::Use => format!("cannot use `{name}` while it is borrowed"),
            Action::Assign => format!("cannot assign to `{name}` while it is borrowed"),
            Action::Borrow => format!("cannot borrow `{name}` because it is mutably borrowed"),
            Action::MutBorrow => {
                format!("cannot borrow `{name}` as mutable because it is already borrowed")
            }
            Action::Move => format!("cannot move `{name}` while it is borrowed"),
            Action::StorageDead
                if self.body.locals[access.path.local.0 as usize]
                    .name
                    .is_some() =>
            {
                format!("`{name}` does not live long enough")
            }
            Action::StorageDead => "temporary value does not live long enough".to_string(),
        };
        let created = if access.action == Action::StorageDead {
            format!("`{}` borrowed here", loan.name)
        } else if exclusive {
            format!("mutable borrow of `{}` created here", loan.name)
        } else {
            format!("borrow of `{}` created here", loan.name)
        };
        let later = match &self.body.locals[holder.0 as usize].name {
            Some(view) => format!("the view `{view}` is used later (§11.3)"),
            None => "a later use keeps this borrow live (§11.3)".to_string(),
        };
        let temporary_scope_end = access.action == Action::StorageDead
            && self.body.locals[access.path.local.0 as usize]
                .name
                .is_none();
        let mut diagnostic = if temporary_scope_end {
            Diagnostic::new(Severity::Error, message, loan.span)
        } else {
            Diagnostic::new(Severity::Error, message, access.span).related(loan.span, created)
        }
        .note(later);
        if access.from_slicing && loan.from_slicing {
            diagnostic = diagnostic.note(
                "slice borrows are checked against the whole originating place, not index ranges (§12.5)",
            );
        }
        diagnostic
    }

    /// Storing a view through a slice element or a by-reference parameter
    /// would need output-provenance contracts, which do not exist yet.
    fn check_view_store(&self, target: &Place, span: Span, findings: &mut Findings) {
        if !self.package.contains_view(self.place_ty(target)) {
            return;
        }
        let path = Path::of(self.package, self.body, target);
        if !path.goes_through_deref() && !self.body.locals[target.local.0 as usize].by_reference {
            return;
        }
        if findings.reported.insert(span) {
            findings.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!(
                        "storing a borrowed view through `{}` is not supported yet",
                        self.describe(target)
                    ),
                    span,
                )
                .note("views can only be stored in local variables for now; storing through a parameter or slice element needs output provenance contracts (§11.7)"),
            );
        }
    }

    /// Records which parameters each returned view borrows from, rejecting
    /// views of storage that does not outlive the call (§11.7).
    fn return_origins(&self, state: &Holdings, findings: &mut Findings) {
        for (result, ret) in self.body.returns.iter().enumerate() {
            for &id in &state[ret.0 as usize] {
                let loan = &self.loans[id];
                let path = match &loan.target {
                    LoanTarget::Param(index) => {
                        findings.contract.results[result][*index].contents = true;
                        continue;
                    }
                    LoanTarget::Place(path) if path.goes_through_deref() => continue,
                    LoanTarget::Place(path) => path,
                };
                let root = &self.body.locals[path.local.0 as usize];
                if let Some(index) = self.body.params.iter().position(|&p| p == path.local)
                    && root.by_reference
                {
                    findings.contract.results[result][index].storage = true;
                    continue;
                }
                if !findings.reported.insert(loan.span) {
                    continue;
                }
                let owner = match (&root.name, self.body.params.contains(&path.local)) {
                    (Some(name), true) => format!("`own` parameter `{name}`"),
                    (Some(name), false) => format!("local `{name}`"),
                    (None, _) => "a temporary value".to_string(),
                };
                findings.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!("cannot return a view of {owner}"),
                        loan.span,
                    )
                    .note(
                        "a returned view must refer to storage borrowed from a parameter (§11.7)",
                    ),
                );
            }
        }
    }
}

/// Backward liveness: a local is live where its current value may still be
/// read, which is exactly where the loans it holds stay in force.
struct Liveness {
    /// `before[block][k]`: live locals before statement `k`; index
    /// `statements.len()` is before the terminator, one more is after it.
    before: Vec<Vec<LiveSet>>,
}

impl Liveness {
    fn compute(body: &Body) -> Self {
        let count = body.locals.len();
        let mut live_in: Vec<LiveSet> = vec![vec![false; count]; body.blocks.len()];
        let mut changed = true;
        while changed {
            changed = false;
            for (index, block) in body.blocks.iter().enumerate().rev() {
                let mut live = Self::live_out(body, block, &live_in);
                Self::step_terminator(body, &block.terminator, &mut live);
                for statement in block.statements.iter().rev() {
                    Self::step_statement(body, statement, &mut live);
                }
                if live != live_in[index] {
                    live_in[index] = live;
                    changed = true;
                }
            }
        }
        let before = body
            .blocks
            .iter()
            .map(|block| {
                let mut points = vec![Self::live_out(body, block, &live_in)];
                let mut live = points[0].clone();
                Self::step_terminator(body, &block.terminator, &mut live);
                points.push(live.clone());
                for statement in block.statements.iter().rev() {
                    Self::step_statement(body, statement, &mut live);
                    points.push(live.clone());
                }
                points.reverse();
                points
            })
            .collect();
        Self { before }
    }

    fn before(&self, block: BlockId, position: usize) -> &LiveSet {
        &self.before[block.0 as usize][position]
    }

    fn live_out(body: &Body, block: &BasicBlock, live_in: &[LiveSet]) -> LiveSet {
        let mut live = vec![false; body.locals.len()];
        for successor in block.terminator.successors() {
            for (slot, &incoming) in live.iter_mut().zip(&live_in[successor.0 as usize]) {
                *slot |= incoming;
            }
        }
        live
    }

    fn step_statement(body: &Body, statement: &Statement, live: &mut LiveSet) {
        if let Statement::Assign { place, rvalue, .. } = statement {
            Self::define(place, live);
            Self::use_target(body, place, live);
            Self::use_rvalue(rvalue, live);
        }
    }

    fn step_terminator(body: &Body, terminator: &Terminator, live: &mut LiveSet) {
        match terminator {
            Terminator::Assert { place, rvalue, .. } => {
                Self::define(place, live);
                Self::use_target(body, place, live);
                Self::use_rvalue(rvalue, live);
            }
            Terminator::Branch { condition, .. } => Self::use_operand(condition, live),
            Terminator::Call {
                args, destinations, ..
            } => {
                for destination in destinations.iter().flatten() {
                    Self::define(destination, live);
                    Self::use_target(body, destination, live);
                }
                for arg in args {
                    Self::use_operand(arg, live);
                }
            }
            Terminator::Return => {
                for ret in &body.returns {
                    live[ret.0 as usize] = true;
                }
            }
            Terminator::Goto(_) | Terminator::PanicReturn | Terminator::Unreachable => {}
        }
    }

    fn define(place: &Place, live: &mut LiveSet) {
        if place.projections.is_empty() {
            live[place.local.0 as usize] = false;
        }
    }

    /// A projected target reads its index operands, and writing through a
    /// slice element reads the descriptor that locates it.
    fn use_target(body: &Body, place: &Place, live: &mut LiveSet) {
        Self::use_indices(place, live);
        if !place.projections.is_empty() && !body.locals[place.local.0 as usize].by_reference {
            live[place.local.0 as usize] = true;
        }
    }

    fn use_place(place: &Place, live: &mut LiveSet) {
        live[place.local.0 as usize] = true;
        Self::use_indices(place, live);
    }

    fn use_indices(place: &Place, live: &mut LiveSet) {
        for projection in &place.projections {
            if let Projection::Index(index) = projection {
                Self::use_operand(index, live);
            }
        }
    }

    fn use_operand(operand: &Operand, live: &mut LiveSet) {
        if let Operand::Copy(place) | Operand::Move(place) | Operand::Ref(place) = operand {
            Self::use_place(place, live);
        }
    }

    fn use_rvalue(rvalue: &Rvalue, live: &mut LiveSet) {
        match rvalue {
            Rvalue::Zero => {}
            Rvalue::Use(operand)
            | Rvalue::Unary(_, operand)
            | Rvalue::Convert(operand, _)
            | Rvalue::Error(operand) => Self::use_operand(operand, live),
            Rvalue::Binary(_, left, right) | Rvalue::BoundsCheck(left, right) => {
                Self::use_operand(left, live);
                Self::use_operand(right, live);
            }
            Rvalue::Aggregate(_, operands) => {
                for operand in operands {
                    Self::use_operand(operand, live);
                }
            }
            Rvalue::Length(place) => Self::use_place(place, live),
            Rvalue::Slice {
                place, low, high, ..
            } => {
                Self::use_place(place, live);
                for bound in [low, high].into_iter().flatten() {
                    Self::use_operand(bound, live);
                }
            }
        }
    }
}
