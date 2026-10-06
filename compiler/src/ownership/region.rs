//! A loan stays live exactly while some live local holds it.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::borrow::{
    Access, AccessKind, Action, Depth, Loan, LoanKind, LoanTarget, Path, PathElem, conflicts,
};
use super::checker::describe_place;
use crate::ast::ParamMode;
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir;
use crate::mir::{
    BasicBlock, BlockId, Body, Callee, Local, Operand, Place, Program, Projection, Rvalue,
    Statement, Terminator, place_type,
};
use crate::resolve::{FieldId, LocalKind};
use crate::source::Span;
use crate::types::{TypeId, TypeKind};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Origin {
    /// The result views the argument's own storage (a by-reference parameter).
    storage: bool,
    /// The result forwards views the argument already holds.
    contents: bool,
}

/// Inputs are the parameters, then a closure's captures.
#[derive(Clone, Debug, PartialEq)]
struct Contract {
    /// Indexed `results[result][input]`.
    results: Vec<Vec<Origin>>,
    /// Indexed `outputs[input][input]`: views a `mut` parameter or capture may receive.
    outputs: Vec<Vec<Origin>>,
}

impl Contract {
    fn empty(body: &Body) -> Self {
        let inputs = body.params.len() + body.captures.len();
        Self {
            results: vec![vec![Origin::default(); inputs]; body.returns.len()],
            outputs: vec![vec![Origin::default(); inputs]; inputs],
        }
    }
}

pub(super) fn check(package: &hir::Package, program: &Program, diagnostics: &mut Vec<Diagnostic>) {
    let mut contracts: Vec<Contract> = program.bodies.iter().map(Contract::empty).collect();
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
    /// Locals whose custom `drop` can read the borrows they hold.
    observing: Vec<bool>,
    loans: Vec<Loan>,
    interned: HashMap<LoanKey, LoanId>,
}

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

struct Findings<'l> {
    live: &'l Liveness,
    contract: Contract,
    diagnostics: Vec<Diagnostic>,
    reported: HashSet<Span>,
    drop_order_reported: HashSet<(Local, Local)>,
}

impl Findings<'_> {
    fn report_once(&mut self, span: Span, diagnostic: Diagnostic) {
        if self.reported.insert(span) {
            self.diagnostics.push(diagnostic);
        }
    }
}

impl<'a> Analysis<'a> {
    fn new(package: &'a hir::Package, body: &'a Body, contracts: &'a [Contract]) -> Self {
        let observing = body
            .locals
            .iter()
            .map(|local| !local.by_reference && package.drop_observes_view(local.ty))
            .collect();
        Self {
            package,
            body,
            contracts,
            observing,
            loans: Vec::new(),
            interned: HashMap::new(),
        }
    }

    fn run(mut self) -> (Contract, Vec<Diagnostic>) {
        let body = self.body;
        let empty_contract = Contract::empty(body);
        if body.blocks.is_empty() {
            return (empty_contract, Vec::new());
        }
        let mut entry: Holdings = vec![BTreeSet::new(); body.locals.len()];
        for (index, param) in self.inputs().into_iter().enumerate() {
            if self.carries_views(param) {
                let id = self.intern(
                    (ENTRY_BLOCK_KEY, index, 0),
                    Loan {
                        kind: LoanKind::Shared,
                        target: LoanTarget::Param(index),
                        span: self.package.function(body.function).span,
                        name: String::new(),
                        from_slicing: false,
                        captured: false,
                        binding: false,
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
        let inputs = self.inputs();
        let kept_at_return: Vec<bool> = body
            .locals
            .iter()
            .zip(&self.observing)
            .enumerate()
            .map(|(index, (local, &observing))| {
                observing
                    || local.by_reference
                        && inputs.contains(&Local(index as u32))
                        && self.package.contains_view(local.ty)
            })
            .collect();
        let live = Liveness::compute(body, &self.observing, &kept_at_return);
        let mut findings = Findings {
            live: &live,
            contract: empty_contract,
            diagnostics: Vec::new(),
            reported: HashSet::new(),
            drop_order_reported: HashSet::new(),
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
        self.package.contains_view(self.local_ty(local)) || self.binds_reference(local)
    }

    /// A loop's borrow of its collection or of the current element.
    fn binds_reference(&self, local: Local) -> bool {
        self.body.locals[local.0 as usize].by_reference && !self.inputs().contains(&local)
    }

    /// Holds a `mut []T` whose elements can hold views.
    fn stores_through_views(&self, ty: TypeId) -> bool {
        self.package.contains_mut_slice_of_views(ty)
    }

    /// `local` and every owner whose storage a write through its mutable views reaches.
    fn store_holders(&self, local: Local, state: &Holdings) -> Vec<Local> {
        let inputs = self.inputs();
        let mut holders = vec![local];
        let mut next = 0;
        while let Some(&holder) = holders.get(next) {
            next += 1;
            for &id in &state[holder.0 as usize] {
                let loan = &self.loans[id];
                let owner = match (&loan.target, loan.kind) {
                    (LoanTarget::Place(path), LoanKind::Exclusive) => path.local,
                    (LoanTarget::Param(index), _) => inputs[*index],
                    (LoanTarget::Place(_), LoanKind::Shared) => continue,
                };
                if !holders.contains(&owner) {
                    holders.push(owner);
                }
            }
        }
        holders
    }

    /// Views stored into `target`, or written by a callee through the views it holds.
    fn store_views(&self, target: &Place, loans: &BTreeSet<LoanId>, state: &mut Holdings) {
        let holders = self.receivers(target, state);
        self.hold(&holders, loans, state);
    }

    fn receivers(&self, target: &Place, state: &Holdings) -> Vec<Local> {
        let through_views = Path::of(self.package, self.body, target).goes_through_deref()
            || self.stores_through_views(self.place_ty(target));
        if through_views {
            self.store_holders(target.local, state)
        } else {
            vec![target.local]
        }
    }

    fn hold(&self, holders: &[Local], loans: &BTreeSet<LoanId>, state: &mut Holdings) {
        for &holder in holders {
            if self.carries_views(holder) {
                state[holder.0 as usize].extend(loans.iter().copied());
            }
        }
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
                        for (dropped, &local) in locals.iter().enumerate() {
                            // Locals drop newest first; only those dropped later still need their borrows.
                            let mut live = live_after.clone();
                            for (order, other) in locals.iter().enumerate() {
                                live[other.0 as usize] =
                                    order < dropped && self.observing[other.0 as usize];
                            }
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
                            self.check_access(&access, &live, state, findings);
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
                    self.output_origins(state, findings);
                }
            }
            Terminator::Goto(_) | Terminator::PanicReturn | Terminator::Unreachable => {}
        }
    }

    /// Reported at the binding's declaration.
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
        mut findings: Option<&mut Findings>,
    ) {
        if let Some(findings) = findings.as_deref_mut() {
            let mut accesses = Vec::new();
            self.rvalue_accesses(rvalue, site.span, &mut accesses);
            self.target_accesses(place, site.span, &mut accesses);
            let live = findings.live.before(BlockId(site.block), site.position);
            for access in &accesses {
                self.check_access(access, live, state, findings);
            }
            self.check_view_store(place, state, site.span, findings);
        }
        let loans = self.rvalue_loans(rvalue, site, state);
        let moved: Vec<&Operand> = match rvalue {
            Rvalue::Use(operand) => vec![operand],
            Rvalue::Aggregate(_, operands) => operands.iter().collect(),
            _ => Vec::new(),
        };
        self.clear_moved(moved, state);
        self.assign(place, loans, state);
        if let Rvalue::Closure { function, captures } = rvalue {
            self.apply_capture_outputs(*function, captures, site, state);
        }
        if let Some(findings) = findings {
            self.check_drop_order(state, site.span, findings);
        }
    }

    fn inputs(&self) -> Vec<Local> {
        self.body
            .params
            .iter()
            .chain(&self.body.captures)
            .copied()
            .collect()
    }

    /// Whether `local` can receive views for the caller: a `mut` parameter or a capture.
    fn is_output(&self, local: Local) -> bool {
        if self.stores_through_views(self.local_ty(local)) {
            return self.inputs().contains(&local);
        }
        self.body.locals[local.0 as usize].by_reference
            && matches!(
                self.package
                    .function(self.body.function)
                    .locals
                    .get(local.0 as usize)
                    .map(|local| local.kind),
                Some(LocalKind::Param(ParamMode::Mut) | LocalKind::Capture(_))
            )
    }

    fn loan_kind_for(&self, ty: TypeId) -> LoanKind {
        if self.package.contains_mut_view(ty) {
            LoanKind::Exclusive
        } else {
            LoanKind::Shared
        }
    }

    /// Loans on each source's storage, its existing views, or both, as its origin says.
    fn origin_loans<'p>(
        &mut self,
        sources: impl IntoIterator<Item = (&'p Place, Origin, bool)>,
        kind: LoanKind,
        site: &mut Site,
        state: &Holdings,
    ) -> BTreeSet<LoanId> {
        let mut loans = BTreeSet::new();
        for (place, origin, borrowable) in sources {
            if origin.storage && borrowable {
                let path = Path::of(self.package, self.body, place);
                loans.extend(self.inherited_loans(place, &path, state));
                let loan = Loan {
                    kind,
                    target: LoanTarget::Place(path),
                    span: site.span,
                    name: self.describe(place),
                    from_slicing: false,
                    captured: false,
                    binding: false,
                };
                loans.insert(self.intern(site.next_key(), loan));
            }
            if origin.contents && self.package.contains_view(self.place_ty(place)) {
                loans.extend(state[place.local.0 as usize].iter().copied());
            }
        }
        loans
    }

    /// A closure that stores views into captured variables may do so whenever it runs.
    fn apply_capture_outputs(
        &mut self,
        function: crate::resolve::FunctionId,
        captures: &[(Place, bool)],
        site: &mut Site,
        state: &mut Holdings,
    ) {
        let contract = self.contracts[function.0 as usize].clone();
        let skipped = contract.outputs.len() - captures.len();
        for (index, (target, _)) in captures.iter().enumerate() {
            let origins = &contract.outputs[skipped + index][skipped..];
            if origins.iter().all(|origin| *origin == Origin::default()) {
                continue;
            }
            let kind = self.loan_kind_for(self.place_ty(target));
            let sources: Vec<(&Place, Origin, bool)> = captures
                .iter()
                .zip(origins)
                .map(|((place, _), &origin)| (place, origin, true))
                .collect();
            let loans = self.origin_loans(sources, kind, site, state);
            self.store_views(target, &loans, state);
        }
    }

    /// A value moved out whole no longer holds borrows; its new owner does.
    fn clear_moved<'o>(
        &self,
        operands: impl IntoIterator<Item = &'o Operand>,
        state: &mut Holdings,
    ) {
        for operand in operands {
            if let Operand::Move(place) = operand
                && place.projections.is_empty()
            {
                state[place.local.0 as usize].clear();
            }
        }
    }

    /// Locals drop in reverse declaration order, so a `drop` reading a borrow needs older storage.
    fn check_drop_order(&self, state: &Holdings, span: Span, findings: &mut Findings) {
        let named = self.package.function(self.body.function).locals.len();
        for (index, held) in state.iter().enumerate() {
            if !self.observing[index] {
                continue;
            }
            let holder = Local(index as u32);
            for &id in held {
                let LoanTarget::Place(path) = &self.loans[id].target else {
                    continue;
                };
                let owner = path.local;
                if path.goes_through_deref()
                    || owner.0 <= holder.0
                    || owner.0 as usize >= named
                    || self.body.locals[owner.0 as usize].by_reference
                    || !findings.drop_order_reported.insert((holder, owner))
                {
                    continue;
                }
                let holder_name = self.describe(&Place::local(holder));
                let owner_name = self.describe(&Place::local(owner));
                findings.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "`{holder_name}` borrows `{owner_name}`, which is dropped before `{holder_name}`'s custom `drop` runs"
                        ),
                        span,
                    )
                    .related(self.scope_end_span(owner), "declared after the value that borrows it")
                    .note("declare the borrowed storage before the value whose `drop` reads it"),
                );
            }
        }
    }

    fn call(
        &mut self,
        callee: &Callee,
        args: &[Operand],
        destinations: &[Option<Place>],
        site: &mut Site,
        state: &mut Holdings,
        mut findings: Option<&mut Findings>,
    ) {
        let moved: Vec<Local> = args
            .iter()
            .filter_map(|arg| match arg {
                Operand::Move(place) if place.projections.is_empty() => Some(place.local),
                _ => None,
            })
            .collect();
        self.call_effects(
            callee,
            args,
            destinations,
            &moved,
            site,
            state,
            findings.as_deref_mut(),
        );
        if let Some(findings) = findings {
            self.check_drop_order(state, site.span, findings);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_effects(
        &mut self,
        callee: &Callee,
        args: &[Operand],
        destinations: &[Option<Place>],
        moved: &[Local],
        site: &mut Site,
        state: &mut Holdings,
        findings: Option<&mut Findings>,
    ) {
        // A function value argument may be called, so it is borrowed exclusively.
        let exclusive_if_func = |mode: ParamMode, ty: TypeId| {
            if matches!(self.package.types.kind(ty), TypeKind::Func(_)) {
                ParamMode::Mut
            } else {
                mode
            }
        };
        let modes: Vec<Option<ParamMode>> = match callee {
            Callee::Function(id) => {
                let function = self.package.function(*id);
                function
                    .params
                    .iter()
                    .map(|param| {
                        let local = &function.locals[param.0 as usize];
                        match local.kind {
                            LocalKind::Param(mode) => Some(exclusive_if_func(mode, local.ty)),
                            _ => None,
                        }
                    })
                    .collect()
            }
            Callee::Value(place) => self
                .package
                .types
                .func_signature(self.place_ty(place))
                .expect("a function-typed callee")
                .params
                .iter()
                .map(|&(mode, ty)| Some(exclusive_if_func(mode, ty)))
                .collect(),
            Callee::Println | Callee::Drop | Callee::Clone(_) => vec![None; args.len()],
            Callee::MapInsertNew | Callee::MapAssign | Callee::MapLookup | Callee::MapRemove => {
                let mut modes = vec![None; args.len()];
                modes[0] = callee.map_access();
                modes
            }
            Callee::ArrayPush | Callee::ArrayPop => {
                let mut modes = vec![None; args.len()];
                modes[0] = Some(ParamMode::Mut);
                modes
            }
        };
        if let Some(findings) = findings {
            let mut accesses = Vec::new();
            if let Callee::Value(place) = callee {
                self.index_accesses(place, site.span, &mut accesses);
                accesses.push(self.access(
                    place,
                    AccessKind::Write,
                    Depth::Deep,
                    Action::Call,
                    site.span,
                    false,
                ));
            }
            for (arg, mode) in args.iter().zip(&modes) {
                self.operand_accesses(arg, *mode, site.span, &mut accesses);
            }
            for (index, arg) in args.iter().enumerate() {
                let (Operand::Ref(target) | Operand::Copy(target) | Operand::Move(target)) = arg
                else {
                    continue;
                };
                let receives_views = match callee {
                    Callee::Function(id) => self.contracts[id.0 as usize].outputs[index]
                        .iter()
                        .any(|origin| *origin != Origin::default()),
                    Callee::Value(place) => self
                        .package
                        .types
                        .func_signature(self.place_ty(place))
                        .is_some_and(|signature| {
                            let (mode, ty) = signature.params[index];
                            mode == ParamMode::Mut || self.stores_through_views(ty)
                        }),
                    _ => false,
                };
                if receives_views {
                    self.check_view_store(target, state, site.span, findings);
                }
            }
            for destination in destinations.iter().flatten() {
                self.target_accesses(destination, site.span, &mut accesses);
                self.check_view_store(destination, state, site.span, findings);
            }
            if let (Some(ParamMode::Mut), [Operand::Ref(map), _, _]) = (callee.map_access(), args) {
                self.check_view_store(map, state, site.span, findings);
            }
            if let (Callee::ArrayPush, [Operand::Ref(array), _]) = (callee, args) {
                self.check_view_store(array, state, site.span, findings);
            }
            let live = findings.live.before(BlockId(site.block), site.position);
            for access in &accesses {
                self.check_access(access, live, state, findings);
            }
        }
        let clear = |state: &mut Holdings| {
            for local in moved {
                state[local.0 as usize].clear();
            }
        };
        if callee.map_access().is_some() {
            self.map_call(args, destinations, site, state);
            clear(state);
            return;
        }
        if let (Callee::ArrayPush, [Operand::Ref(array), value]) = (callee, args) {
            let loans = self.operand_loans(value, site, state);
            self.store_views(array, &loans, state);
            clear(state);
            return;
        }
        if let (Callee::ArrayPop, [Operand::Ref(array)]) = (callee, args) {
            let held = state[array.local.0 as usize].clone();
            for (index, destination) in destinations.iter().enumerate() {
                if let Some(destination) = destination {
                    let carries =
                        index == 1 && self.package.contains_view(self.place_ty(destination));
                    let loans = if carries {
                        held.clone()
                    } else {
                        BTreeSet::new()
                    };
                    self.assign(destination, loans, state);
                }
            }
            clear(state);
            return;
        }
        if let Callee::Clone(ty) = callee {
            let loans = match (args, self.package.contains_view(*ty)) {
                ([Operand::Ref(source)], true) => state[source.local.0 as usize].clone(),
                _ => BTreeSet::new(),
            };
            clear(state);
            for destination in destinations.iter().flatten() {
                self.assign(destination, loans.clone(), state);
            }
            return;
        }
        if let Callee::Value(callee_place) = callee {
            self.value_call_effects(callee_place, args, destinations, site, state);
            clear(state);
            return;
        }
        let Callee::Function(id) = callee else {
            clear(state);
            return;
        };
        let results = self.package.function(*id).results.clone();
        let contract = self.contracts[id.0 as usize].clone();
        let sources = |args: &'_ [Operand], origins: &[Origin]| -> Vec<(Place, Origin, bool)> {
            args.iter()
                .zip(origins)
                .filter_map(|(arg, &origin)| match arg {
                    Operand::Copy(place) | Operand::Move(place) => {
                        Some((place.clone(), origin, false))
                    }
                    Operand::Ref(place) => Some((place.clone(), origin, true)),
                    Operand::Const(..) => None,
                })
                .collect()
        };
        let mut stores = Vec::new();
        for (index, destination) in destinations.iter().enumerate() {
            let Some(destination) = destination else {
                continue;
            };
            let loans = if self.package.contains_view(results[index]) {
                let kind = self.loan_kind_for(results[index]);
                let found = sources(args, &contract.results[index]);
                self.origin_loans(found.iter().map(|(p, o, b)| (p, *o, *b)), kind, site, state)
            } else {
                BTreeSet::new()
            };
            stores.push((destination, loans));
        }
        let mut outputs = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            let (Operand::Ref(target) | Operand::Copy(target) | Operand::Move(target)) = arg else {
                continue;
            };
            let origins = &contract.outputs[index];
            if origins.iter().all(|origin| *origin == Origin::default()) {
                continue;
            }
            let kind = self.loan_kind_for(self.place_ty(target));
            let found = sources(args, origins);
            let loans =
                self.origin_loans(found.iter().map(|(p, o, b)| (p, *o, *b)), kind, site, state);
            outputs.push((self.receivers(target, state), loans));
        }
        clear(state);
        for (destination, loans) in stores {
            self.assign(destination, loans, state);
        }
        for (holders, loans) in outputs {
            self.hold(&holders, &loans, state);
        }
    }

    /// An unknown callee may pass any argument's or capture's views to its results and `mut` arguments.
    fn value_call_effects(
        &mut self,
        callee: &Place,
        args: &[Operand],
        destinations: &[Option<Place>],
        site: &mut Site,
        state: &mut Holdings,
    ) {
        let everything = Origin {
            storage: true,
            contents: true,
        };
        let signature = self
            .package
            .types
            .func_signature(self.place_ty(callee))
            .expect("a function-typed callee")
            .clone();
        let mut sources: Vec<(Place, Origin, bool)> = args
            .iter()
            .filter_map(|arg| match arg {
                Operand::Copy(place) | Operand::Move(place) => {
                    Some((place.clone(), everything, false))
                }
                Operand::Ref(place) => Some((place.clone(), everything, true)),
                Operand::Const(..) => None,
            })
            .collect();
        let closure_views = Origin {
            storage: false,
            contents: true,
        };
        sources.push((callee.clone(), closure_views, false));
        let mut stores = Vec::new();
        for (index, destination) in destinations.iter().enumerate() {
            let Some(destination) = destination else {
                continue;
            };
            let ty = signature.results[index];
            let loans = if self.package.contains_view(ty) {
                let kind = self.loan_kind_for(ty);
                self.origin_loans(
                    sources.iter().map(|(p, o, b)| (p, *o, *b)),
                    kind,
                    site,
                    state,
                )
            } else {
                BTreeSet::new()
            };
            stores.push((destination, loans));
        }
        let mut outputs = Vec::new();
        for (arg, &(mode, ty)) in args.iter().zip(&signature.params) {
            if let Operand::Ref(target) | Operand::Copy(target) | Operand::Move(target) = arg
                && (mode == ParamMode::Mut && self.package.contains_view(ty)
                    || self.stores_through_views(ty))
            {
                let kind = self.loan_kind_for(ty);
                let loans = self.origin_loans(
                    sources.iter().map(|(p, o, b)| (p, *o, *b)),
                    kind,
                    site,
                    state,
                );
                outputs.push((self.receivers(target, state), loans));
            }
        }
        for (destination, loans) in stores {
            self.assign(destination, loans, state);
        }
        for (holders, loans) in outputs {
            self.hold(&holders, &loans, state);
        }
    }

    /// Stored values' loans join the map's; values taken out carry them.
    fn map_call(
        &mut self,
        args: &[Operand],
        destinations: &[Option<Place>],
        site: &mut Site,
        state: &mut Holdings,
    ) {
        let Operand::Ref(map) = &args[0] else {
            unreachable!("map operations borrow their map")
        };
        if let [_, _, value] = args {
            let loans = self.operand_loans(value, site, state);
            self.store_views(map, &loans, state);
            return;
        }
        let held = state[map.local.0 as usize].clone();
        for (index, destination) in destinations.iter().enumerate() {
            let Some(destination) = destination else {
                continue;
            };
            let carries = index == 1 && self.package.contains_view(self.place_ty(destination));
            let loans = if carries {
                held.clone()
            } else {
                BTreeSet::new()
            };
            self.assign(destination, loans, state);
        }
    }

    /// Also ends loans through the descriptor the assignment replaces.
    fn assign(&mut self, target: &Place, loans: BTreeSet<LoanId>, state: &mut Holdings) {
        if target.projections.is_empty() {
            if self.carries_views(target.local) {
                state[target.local.0 as usize] = loans;
            }
        } else {
            self.store_views(target, &loans, state);
        }
        let path = Path::of(self.package, self.body, target);
        for held in state.iter_mut() {
            held.retain(|&id| !self.loans[id].ended_by_assignment_to(&path));
        }
    }

    /// Storage behind a view stays borrowed from its backing; held views keep their provenance.
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
        if self.binds_reference(place.local) {
            loans.retain(|&id| !self.loans[id].binding);
        }
        if matches!(operand, Operand::Copy(_)) {
            // Copying a mutable view, alone or inside a composite, is an exclusive reborrow.
            for elems in self.mut_view_paths(ty) {
                let mut path = Path::of(self.package, self.body, place);
                path.elems.extend(elems);
                let loan = Loan {
                    kind: LoanKind::Exclusive,
                    target: LoanTarget::Place(path.deref()),
                    span: site.span,
                    name: self.describe(place),
                    from_slicing: false,
                    captured: false,
                    binding: false,
                };
                loans.insert(self.intern(site.next_key(), loan));
            }
        }
        loans
    }

    fn mut_view_paths(&self, ty: TypeId) -> Vec<Vec<PathElem>> {
        let prefixed = |prefix: PathElem, paths: Vec<Vec<PathElem>>| {
            paths
                .into_iter()
                .map(|path| std::iter::once(prefix).chain(path).collect())
                .collect::<Vec<_>>()
        };
        match self.package.types.kind(ty) {
            TypeKind::Slice { mutable: true, .. } => vec![Vec::new()],
            TypeKind::Struct(id) => self
                .package
                .strukt(id)
                .fields
                .iter()
                .enumerate()
                .flat_map(|(index, field)| {
                    prefixed(
                        PathElem::Field(FieldId(index as u32)),
                        self.mut_view_paths(field.ty),
                    )
                })
                .collect(),
            TypeKind::Array { element, .. } => {
                prefixed(PathElem::Index, self.mut_view_paths(element))
            }
            _ => Vec::new(),
        }
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
                    captured: false,
                    binding: false,
                };
                let mut loans = BTreeSet::from([self.intern(site.next_key(), loan)]);
                loans.extend(inherited);
                loans
            }
            Rvalue::Ref(place) | Rvalue::MapValueRef(place, _) => {
                let mut path = Path::of(self.package, self.body, place);
                if matches!(rvalue, Rvalue::MapValueRef(..)) {
                    path.elems.push(PathElem::Index);
                }
                let loan = Loan {
                    kind: LoanKind::Shared,
                    target: LoanTarget::Place(path),
                    span: site.span,
                    name: self.describe(place),
                    from_slicing: false,
                    captured: false,
                    binding: true,
                };
                let mut loans = BTreeSet::from([self.intern(site.next_key(), loan)]);
                loans.extend(state[place.local.0 as usize].iter().copied());
                loans
            }
            Rvalue::Closure { captures, .. } => {
                let mut loans = BTreeSet::new();
                for (place, exclusive) in captures {
                    let path = Path::of(self.package, self.body, place);
                    loans.extend(self.inherited_loans(place, &path, state));
                    let loan = Loan {
                        kind: if *exclusive {
                            LoanKind::Exclusive
                        } else {
                            LoanKind::Shared
                        },
                        target: LoanTarget::Place(path),
                        span: site.span,
                        name: self.describe(place),
                        from_slicing: false,
                        captured: true,
                        binding: false,
                    };
                    loans.insert(self.intern(site.next_key(), loan));
                }
                loans
            }
            Rvalue::Zero
            | Rvalue::Binary(..)
            | Rvalue::Unary(..)
            | Rvalue::Convert(..)
            | Rvalue::Error(_)
            | Rvalue::BoundsCheck(..)
            | Rvalue::Length(_)
            | Rvalue::MapKeyAt(..) => BTreeSet::new(),
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
            Operand::Copy(place) if !self.mut_view_paths(self.place_ty(place)).is_empty() => {
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
            Rvalue::Ref(place) => {
                self.index_accesses(place, span, out);
                out.push(self.access(
                    place,
                    AccessKind::Read,
                    Depth::Deep,
                    Action::Borrow,
                    span,
                    false,
                ));
            }
            Rvalue::MapKeyAt(place, position) | Rvalue::MapValueRef(place, position) => {
                out.push(self.access(
                    place,
                    AccessKind::Read,
                    Depth::Deep,
                    Action::Borrow,
                    span,
                    false,
                ));
                self.operand_accesses(position, None, span, out);
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
            Rvalue::Closure { captures, .. } => {
                for (place, exclusive) in captures {
                    let (kind, action) = if *exclusive {
                        (AccessKind::Write, Action::MutBorrow)
                    } else {
                        (AccessKind::Read, Action::Borrow)
                    };
                    out.push(self.access(place, kind, Depth::Deep, action, span, false));
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
            Action::Call => format!("cannot call `{name}` while it is borrowed"),
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
        let created = if loan.captured && exclusive {
            format!("`{}` captured mutably by this function literal", loan.name)
        } else if loan.captured {
            format!("`{}` captured by this function literal", loan.name)
        } else if access.action == Action::StorageDead {
            format!("`{}` borrowed here", loan.name)
        } else if exclusive {
            format!("mutable borrow of `{}` created here", loan.name)
        } else {
            format!("borrow of `{}` created here", loan.name)
        };
        let holder_decl = &self.body.locals[holder.0 as usize];
        let holder_is_closure =
            matches!(self.package.types.kind(holder_decl.ty), TypeKind::Func(_));
        let later = match &holder_decl.name {
            Some(value) if self.observing[holder.0 as usize] => {
                format!("`{value}`'s custom `drop` can read this borrow until `{value}` is dropped")
            }
            Some(closure) if holder_is_closure => {
                format!("the closure `{closure}` is used later")
            }
            _ if self.binds_reference(holder) => {
                format!(
                    "the loop over `{}` keeps it borrowed until the loop ends",
                    loan.name
                )
            }
            Some(view) => format!("the view `{view}` is used later"),
            None => "a later use keeps this borrow live".to_string(),
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
                "slice borrows are checked against the whole originating place, not index ranges",
            );
        }
        diagnostic
    }

    /// A view written into storage reached through an input must land in an output.
    fn check_view_store(
        &self,
        target: &Place,
        state: &Holdings,
        span: Span,
        findings: &mut Findings,
    ) {
        if !self.package.contains_view(self.place_ty(target)) {
            return;
        }
        let inputs = self.inputs();
        let Some(&input) = self
            .receivers(target, state)
            .iter()
            .find(|&&holder| inputs.contains(&holder) && !self.is_output(holder))
        else {
            return;
        };
        findings.report_once(
            span,
            Diagnostic::new(
                Severity::Error,
                format!(
                    "cannot store a view through `{}`, which belongs to shared parameter `{}`",
                    self.describe(target),
                    self.describe(&Place::local(input))
                ),
                span,
            )
            .note("a function can store views only through `mut` parameters, mutable slices, and captured variables"),
        );
    }

    /// Which input a held loan comes from, or the description of local storage it escapes.
    fn loan_origin(&self, id: LoanId) -> Option<Result<(usize, bool), String>> {
        let path = match &self.loans[id].target {
            LoanTarget::Param(index) => return Some(Ok((*index, false))),
            LoanTarget::Place(path) if path.goes_through_deref() => return None,
            LoanTarget::Place(path) => path,
        };
        let root = &self.body.locals[path.local.0 as usize];
        if root.by_reference
            && let Some(index) = self.inputs().iter().position(|&input| input == path.local)
        {
            return Some(Ok((index, true)));
        }
        Some(Err(
            match (&root.name, self.body.params.contains(&path.local)) {
                (Some(name), true) => format!("`own` parameter `{name}`"),
                (Some(name), false) => format!("local `{name}`"),
                (None, _) => "a temporary value".to_string(),
            },
        ))
    }

    /// Rejects returned views of storage that does not outlive the call.
    fn return_origins(&self, state: &Holdings, findings: &mut Findings) {
        for (result, ret) in self.body.returns.iter().enumerate() {
            for &id in &state[ret.0 as usize] {
                match self.loan_origin(id) {
                    None => {}
                    Some(Ok((index, true))) => {
                        findings.contract.results[result][index].storage = true
                    }
                    Some(Ok((index, false))) => {
                        findings.contract.results[result][index].contents = true
                    }
                    Some(Err(owner)) => {
                        let span = self.loans[id].span;
                        findings.report_once(
                            span,
                            Diagnostic::new(
                                Severity::Error,
                                format!("cannot return a view of {owner}"),
                                span,
                            )
                            .note(
                                "a returned view must refer to storage borrowed from a parameter",
                            ),
                        );
                    }
                }
            }
        }
    }

    /// Records which inputs' views each `mut` parameter or capture may now hold.
    fn output_origins(&self, state: &Holdings, findings: &mut Findings) {
        let params = self.body.params.len();
        for (slot, local) in self.inputs().into_iter().enumerate() {
            if !self.is_output(local) || !self.carries_views(local) {
                continue;
            }
            let name = self.describe(&Place::local(local));
            for &id in &state[local.0 as usize] {
                let span = self.loans[id].span;
                match self.loan_origin(id) {
                    None => {}
                    Some(Ok((index, _))) if index == slot => {}
                    Some(Ok((index, _))) if slot >= params && index < params => {
                        findings.report_once(span, Diagnostic::new(
                                    Severity::Error,
                                    format!(
                                        "storing a view from a function literal's parameter into captured `{name}` is not supported yet"
                                    ),
                                    span,
                                )
                                .note("a closure can store views of other captured variables into a captured variable"));
                    }
                    Some(Ok((index, storage))) => {
                        let origin = &mut findings.contract.outputs[slot][index];
                        if storage {
                            origin.storage = true;
                        } else {
                            origin.contents = true;
                        }
                    }
                    Some(Err(owner)) => {
                        findings.report_once(span, Diagnostic::new(
                                    Severity::Error,
                                    format!("cannot store a view of {owner} into `{name}`"),
                                    span,
                                )
                                .note("a view stored through a `mut` parameter, mutable slice, or captured variable must borrow storage from outside the function"));
                    }
                }
            }
        }
    }
}

/// Backward liveness: a loan stays in force while its holder is live.
struct Liveness {
    /// Index `statements.len()` is before the terminator, one more is after it.
    before: Vec<Vec<LiveSet>>,
}

impl Liveness {
    fn compute(body: &Body, observing: &[bool], kept_at_return: &[bool]) -> Self {
        let count = body.locals.len();
        let mut live_in: Vec<LiveSet> = vec![vec![false; count]; body.blocks.len()];
        let mut changed = true;
        while changed {
            changed = false;
            for (index, block) in body.blocks.iter().enumerate().rev() {
                let mut live = Self::live_out(body, block, &live_in);
                Self::step_terminator(
                    body,
                    observing,
                    kept_at_return,
                    &block.terminator,
                    &mut live,
                );
                for statement in block.statements.iter().rev() {
                    Self::step_statement(body, observing, statement, &mut live);
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
                Self::step_terminator(
                    body,
                    observing,
                    kept_at_return,
                    &block.terminator,
                    &mut live,
                );
                points.push(live.clone());
                for statement in block.statements.iter().rev() {
                    Self::step_statement(body, observing, statement, &mut live);
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

    fn step_statement(body: &Body, observing: &[bool], statement: &Statement, live: &mut LiveSet) {
        match statement {
            Statement::Assign { place, rvalue, .. } => {
                Self::define_dropping(place, observing, live);
                Self::use_target(body, place, live);
                Self::use_rvalue(rvalue, live);
            }
            Statement::EndScope(locals) => {
                for local in locals {
                    if observing[local.0 as usize] {
                        live[local.0 as usize] = true;
                    }
                }
            }
            Statement::Drop { .. } => {}
        }
    }

    /// Replacing a value whose `drop` reads borrows is a use of the old value.
    fn define_dropping(place: &Place, observing: &[bool], live: &mut LiveSet) {
        Self::define(place, live);
        if observing[place.local.0 as usize] {
            live[place.local.0 as usize] = true;
        }
    }

    fn step_terminator(
        body: &Body,
        observing: &[bool],
        kept_at_return: &[bool],
        terminator: &Terminator,
        live: &mut LiveSet,
    ) {
        match terminator {
            Terminator::Assert { place, rvalue, .. } => {
                Self::define_dropping(place, observing, live);
                Self::use_target(body, place, live);
                Self::use_rvalue(rvalue, live);
            }
            Terminator::Branch { condition, .. } => Self::use_operand(condition, live),
            Terminator::Call {
                callee,
                args,
                destinations,
                ..
            } => {
                for destination in destinations.iter().flatten() {
                    Self::define_dropping(destination, observing, live);
                    Self::use_target(body, destination, live);
                }
                for arg in args {
                    Self::use_operand(arg, live);
                }
                if let Callee::Value(place) = callee {
                    Self::use_place(place, live);
                }
            }
            Terminator::Return => {
                for ret in &body.returns {
                    live[ret.0 as usize] = true;
                }
                for (index, &kept) in kept_at_return.iter().enumerate() {
                    live[index] |= kept;
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

    /// Writing through a slice element reads the descriptor that locates it.
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
            Rvalue::Length(place) | Rvalue::Ref(place) => Self::use_place(place, live),
            Rvalue::MapKeyAt(place, position) | Rvalue::MapValueRef(place, position) => {
                Self::use_place(place, live);
                Self::use_operand(position, live);
            }
            Rvalue::Slice {
                place, low, high, ..
            } => {
                Self::use_place(place, live);
                for bound in [low, high].into_iter().flatten() {
                    Self::use_operand(bound, live);
                }
            }
            Rvalue::Closure { captures, .. } => {
                for (place, _) in captures {
                    Self::use_place(place, live);
                }
            }
        }
    }
}
