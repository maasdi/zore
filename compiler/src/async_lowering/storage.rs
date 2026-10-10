use crate::hir;
use crate::mir::{self, Operand, Place, Projection, Rvalue, Statement, Terminator};

use super::LocalStorage;

pub(super) fn classify(
    body: &mir::Body,
    suspensions: &[(mir::BlockId, super::Suspension)],
    budget_blocks: &[mir::BlockId],
) -> Vec<LocalStorage> {
    let mut frame = vec![false; body.locals.len()];
    for &local in body
        .params
        .iter()
        .chain(&body.captures)
        .chain(&body.returns)
    {
        frame[local.0 as usize] = true;
    }
    for (index, local) in body.locals.iter().enumerate() {
        if local.by_reference {
            frame[index] = true;
        }
    }
    for block in &body.blocks {
        for statement in &block.statements {
            if let Statement::Assign { rvalue, .. } = statement {
                stable_rvalue(rvalue, &mut frame);
            }
        }
        if let Terminator::Call { args, .. } = &block.terminator {
            for argument in args {
                stable_operand(argument, &mut frame);
            }
        } else if let Terminator::Assert { rvalue, .. } = &block.terminator {
            stable_rvalue(rvalue, &mut frame);
        }
    }

    let mut reachable = vec![false; body.blocks.len()];
    let mut pending = Vec::new();
    for (block, _) in suspensions {
        if let Terminator::Call {
            target,
            unwind,
            destinations,
            ..
        } = &body.blocks[block.0 as usize].terminator
        {
            pending.push(*target);
            pending.extend(*unwind);
            for destination in destinations.iter().flatten() {
                visit_place(destination, &mut |local| frame[local] = true);
            }
        }
    }
    while let Some(block) = pending.pop() {
        let index = block.0 as usize;
        if reachable[index] {
            continue;
        }
        reachable[index] = true;
        let terminator = &body.blocks[index].terminator;
        pending.extend(terminator.successors());
        if let Terminator::Call {
            unwind: Some(unwind),
            ..
        }
        | Terminator::Assert {
            unwind: Some(unwind),
            ..
        } = terminator
        {
            pending.push(*unwind);
        }
    }
    for (index, block) in body.blocks.iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        for statement in &block.statements {
            visit_statement(statement, &mut |local| frame[local] = true);
        }
        visit_terminator(&block.terminator, &mut |local| frame[local] = true);
    }

    if !budget_blocks.is_empty() {
        let live_at_entry = live_at_entry(body);
        for block in budget_blocks {
            for (index, live) in live_at_entry[block.0 as usize].iter().enumerate() {
                frame[index] |= *live;
            }
        }
    }

    frame
        .into_iter()
        .map(|needed| {
            if needed {
                LocalStorage::Frame
            } else {
                LocalStorage::Poll
            }
        })
        .collect()
}

fn live_at_entry(body: &mir::Body) -> Vec<Vec<bool>> {
    let block_count = body.blocks.len();
    let local_count = body.locals.len();
    let mut used_before_definition = vec![vec![false; local_count]; block_count];
    let mut defined = vec![vec![false; local_count]; block_count];
    for (index, block) in body.blocks.iter().enumerate() {
        for statement in &block.statements {
            match statement {
                Statement::Assign { place, rvalue, .. } => {
                    visit_rvalue(rvalue, &mut |local| {
                        if !defined[index][local] {
                            used_before_definition[index][local] = true;
                        }
                    });
                    if place.projections.is_empty() {
                        defined[index][place.local.0 as usize] = true;
                    } else {
                        visit_place(place, &mut |local| {
                            if !defined[index][local] {
                                used_before_definition[index][local] = true;
                            }
                        });
                    }
                }
                Statement::Drop { place, .. } => visit_place(place, &mut |local| {
                    if !defined[index][local] {
                        used_before_definition[index][local] = true;
                    }
                }),
                Statement::EndScope(locals) => {
                    for local in locals {
                        let local = local.0 as usize;
                        if !defined[index][local] {
                            used_before_definition[index][local] = true;
                        }
                    }
                }
                Statement::SetGlobal { value, .. } => visit_operand(value, &mut |local| {
                    if !defined[index][local] {
                        used_before_definition[index][local] = true;
                    }
                }),
            }
        }
        visit_terminator(&block.terminator, &mut |local| {
            if !defined[index][local] {
                used_before_definition[index][local] = true;
            }
        });
    }
    let mut live = vec![vec![false; local_count]; block_count];
    loop {
        let mut changed = false;
        for index in (0..block_count).rev() {
            let mut next = used_before_definition[index].clone();
            let terminator = &body.blocks[index].terminator;
            let mut successors = terminator.successors();
            match terminator {
                Terminator::Call {
                    unwind: Some(unwind),
                    ..
                }
                | Terminator::Assert {
                    unwind: Some(unwind),
                    ..
                } => successors.push(*unwind),
                _ => {}
            }
            for successor in successors {
                for (local, needed) in live[successor.0 as usize].iter().enumerate() {
                    if !defined[index][local] {
                        next[local] |= *needed;
                    }
                }
            }
            if next != live[index] {
                live[index] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    live
}

/// Same-type slot sharing for pinned locals that are never live together, and the work it took.
pub(super) fn reuse(
    body: &mir::Body,
    package: &hir::Package,
    storage: &[LocalStorage],
) -> (Vec<usize>, usize) {
    let count = body.locals.len();
    let mut eligible: Vec<bool> = body
        .locals
        .iter()
        .enumerate()
        .map(|(index, local)| {
            storage[index] == LocalStorage::Frame
                && !local.by_reference
                && !package.needs_drop(local.ty)
                && !package.contains_view(local.ty)
        })
        .collect();
    for local in body
        .params
        .iter()
        .chain(&body.captures)
        .chain(&body.returns)
    {
        eligible[local.0 as usize] = false;
    }
    let mut stable = vec![false; count];
    for block in &body.blocks {
        for statement in &block.statements {
            if let Statement::Assign { rvalue, .. } = statement {
                stable_rvalue(rvalue, &mut stable);
            }
        }
        match &block.terminator {
            Terminator::Call { args, .. } => {
                for arg in args {
                    stable_operand(arg, &mut stable);
                }
            }
            Terminator::Assert { rvalue, .. } => stable_rvalue(rvalue, &mut stable),
            _ => {}
        }
    }
    for (index, is_stable) in stable.into_iter().enumerate() {
        eligible[index] &= !is_stable;
    }

    let entry = block_entry_liveness(body);
    let mut eligible_bits = bits(count);
    for (index, &is_eligible) in eligible.iter().enumerate() {
        if is_eligible {
            insert(&mut eligible_bits, index);
        }
    }
    let mut interference = Interference::new(count);
    let reachable = reachable_from_entry(body);
    for (index, block) in body.blocks.iter().enumerate() {
        if reachable[index] {
            interference.pair_mentions(body, &entry, block, &eligible_bits);
        } else {
            interference.pair_every_point(body, &entry, block, &eligible_bits);
        }
    }
    if let Some(start) = entry.first() {
        interference.clique(&intersection(start, &eligible_bits));
    }
    interference.symmetrize();

    let mut reuse: Vec<usize> = (0..count).collect();
    let mut owners: Vec<usize> = Vec::new();
    // `conflicts[local]` holds the groups that already contain one of its neighbors.
    let mut conflicts = vec![Vec::new(); count];
    let mut work = 0;
    for index in 0..count {
        if !eligible[index] {
            continue;
        }
        let ty = body.locals[index].ty;
        let group = owners.iter().enumerate().position(|(group, &owner)| {
            work += 1;
            body.locals[owner].ty == ty && !has_bit(&conflicts[index], group)
        });
        let group = match group {
            Some(group) => {
                reuse[index] = owners[group];
                group
            }
            None => {
                owners.push(index);
                owners.len() - 1
            }
        };
        for neighbor in ones(&interference.rows[index]) {
            work += 1;
            set_bit(&mut conflicts[neighbor], group);
        }
    }
    interference.work += work;
    (reuse, interference.work)
}

/// Locals live at each block's entry.
fn block_entry_liveness(body: &mir::Body) -> Vec<Bits> {
    let count = body.locals.len();
    let mut entry = vec![bits(count); body.blocks.len()];
    loop {
        let mut changed = false;
        for (index, block) in body.blocks.iter().enumerate().rev() {
            let mut live = successor_liveness(body, &entry, &block.terminator);
            transfer_terminator(&block.terminator, &mut live);
            for statement in block.statements.iter().rev() {
                transfer_statement(statement, &mut live);
            }
            if live != entry[index] {
                entry[index] = live;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    entry
}

fn reachable_from_entry(body: &mir::Body) -> Vec<bool> {
    let mut reachable = vec![false; body.blocks.len()];
    let mut pending = vec![mir::BlockId(0)];
    while let Some(block) = pending.pop() {
        let index = block.0 as usize;
        if index >= reachable.len() || reachable[index] {
            continue;
        }
        reachable[index] = true;
        pending.extend(successors_with_unwind(&body.blocks[index].terminator));
    }
    reachable
}

fn successors_with_unwind(terminator: &Terminator) -> Vec<mir::BlockId> {
    let mut successors = terminator.successors();
    match terminator {
        Terminator::Call {
            unwind: Some(unwind),
            ..
        }
        | Terminator::Assert {
            unwind: Some(unwind),
            ..
        } => successors.push(*unwind),
        _ => {}
    }
    successors
}

fn successor_liveness(body: &mir::Body, entry: &[Bits], terminator: &Terminator) -> Bits {
    let mut live = bits(body.locals.len());
    for successor in successors_with_unwind(terminator) {
        union_into(&mut live, &entry[successor.0 as usize]);
    }
    live
}

fn transfer_terminator(terminator: &Terminator, live: &mut Bits) {
    match terminator {
        Terminator::Call {
            callee,
            args,
            destinations,
            ..
        } => {
            for place in destinations.iter().flatten() {
                visit_place(place, &mut |local| insert(live, local));
            }
            if let mir::Callee::Value(place) = callee {
                visit_place(place, &mut |local| insert(live, local));
            }
            for arg in args {
                visit_operand(arg, &mut |local| insert(live, local));
            }
        }
        _ => visit_terminator(terminator, &mut |local| insert(live, local)),
    }
}

fn transfer_statement(statement: &Statement, live: &mut Bits) {
    if let Statement::Assign { place, rvalue, .. } = statement {
        if place.projections.is_empty() {
            remove(live, place.local.0 as usize);
        } else {
            visit_place(place, &mut |local| insert(live, local));
        }
        visit_rvalue(rvalue, &mut |local| insert(live, local));
    } else {
        visit_statement(statement, &mut |local| insert(live, local));
    }
}

/// Eligible locals that are live together, or live while an instruction mentions the other.
struct Interference {
    rows: Vec<Bits>,
    /// Bitset words and single bits read or written; a deterministic measure of the analysis cost.
    work: usize,
}

impl Interference {
    fn new(count: usize) -> Self {
        Interference {
            rows: vec![bits(count); count],
            work: 0,
        }
    }

    /// Co-live locals stay co-live backward until one is defined, so mentions plus the entry find every pair.
    fn pair_mentions(
        &mut self,
        body: &mir::Body,
        entry: &[Bits],
        block: &mir::BasicBlock,
        eligible: &Bits,
    ) {
        let mut live = successor_liveness(body, entry, &block.terminator);
        let mut mentioned = Vec::new();
        visit_terminator(&block.terminator, &mut |local| mentioned.push(local));
        self.pair_instruction(&mentioned, &live, eligible);
        transfer_terminator(&block.terminator, &mut live);
        for statement in block.statements.iter().rev() {
            mentioned.clear();
            visit_statement(statement, &mut |local| mentioned.push(local));
            self.pair_instruction(&mentioned, &live, eligible);
            transfer_statement(statement, &mut live);
        }
    }

    fn pair_instruction(&mut self, mentioned: &[usize], live: &Bits, eligible: &Bits) {
        let mut touched = live.clone();
        for &local in mentioned {
            insert(&mut touched, local);
        }
        let touched = intersection(&touched, eligible);
        for &local in mentioned {
            if has(eligible, local) {
                union_into(&mut self.rows[local], &touched);
                self.work += touched.len();
            }
        }
    }

    /// Code that never runs keeps the exhaustive marking at every point.
    fn pair_every_point(
        &mut self,
        body: &mir::Body,
        entry: &[Bits],
        block: &mir::BasicBlock,
        eligible: &Bits,
    ) {
        let mut live = successor_liveness(body, entry, &block.terminator);
        self.clique(&intersection(&live, eligible));
        let mut touched = live.clone();
        visit_terminator(&block.terminator, &mut |local| insert(&mut touched, local));
        self.clique(&intersection(&touched, eligible));
        transfer_terminator(&block.terminator, &mut live);
        self.clique(&intersection(&live, eligible));
        for statement in block.statements.iter().rev() {
            let mut touched = live.clone();
            visit_statement(statement, &mut |local| insert(&mut touched, local));
            self.clique(&intersection(&touched, eligible));
            transfer_statement(statement, &mut live);
            self.clique(&intersection(&live, eligible));
        }
    }

    fn clique(&mut self, members: &Bits) {
        for local in ones(members) {
            union_into(&mut self.rows[local], members);
            self.work += members.len();
        }
    }

    fn symmetrize(&mut self) {
        for left in 0..self.rows.len() {
            let right: Vec<usize> = ones(&self.rows[left]).collect();
            self.work += self.rows[left].len() + right.len();
            for right in right {
                insert(&mut self.rows[right], left);
            }
        }
    }
}

type Bits = Vec<u64>;

fn bits(count: usize) -> Bits {
    vec![0; count.div_ceil(64)]
}

fn has(set: &[u64], index: usize) -> bool {
    set[index / 64] >> (index % 64) & 1 == 1
}

fn insert(set: &mut [u64], index: usize) {
    set[index / 64] |= 1 << (index % 64);
}

fn remove(set: &mut [u64], index: usize) {
    set[index / 64] &= !(1 << (index % 64));
}

fn union_into(target: &mut [u64], source: &[u64]) {
    for (target, source) in target.iter_mut().zip(source) {
        *target |= source;
    }
}

fn intersection(left: &[u64], right: &[u64]) -> Bits {
    left.iter()
        .zip(right)
        .map(|(left, right)| left & right)
        .collect()
}

fn has_bit(set: &[u64], index: usize) -> bool {
    set.get(index / 64)
        .is_some_and(|word| word >> (index % 64) & 1 == 1)
}

fn set_bit(set: &mut Vec<u64>, index: usize) {
    if set.len() <= index / 64 {
        set.resize(index / 64 + 1, 0);
    }
    insert(set, index);
}

fn ones(set: &[u64]) -> impl Iterator<Item = usize> + '_ {
    set.iter().enumerate().flat_map(|(word_index, &word)| {
        let mut word = word;
        std::iter::from_fn(move || {
            if word == 0 {
                return None;
            }
            let bit = word.trailing_zeros() as usize;
            word &= word - 1;
            Some(word_index * 64 + bit)
        })
    })
}

fn stable_rvalue(rvalue: &Rvalue, stable: &mut [bool]) {
    match rvalue {
        Rvalue::Ref(place)
        | Rvalue::Slice { place, .. }
        | Rvalue::MapValueRef(place, _)
        | Rvalue::InterfaceView { place, .. } => {
            visit_place(place, &mut |local| stable[local] = true);
        }
        Rvalue::Closure { captures, .. } => {
            for (place, _) in captures {
                visit_place(place, &mut |local| stable[local] = true);
            }
        }
        _ => {}
    }
}

fn stable_operand(operand: &Operand, stable: &mut [bool]) {
    if let Operand::Ref(place) = operand {
        visit_place(place, &mut |local| stable[local] = true);
    }
}

fn visit_place(place: &Place, visit: &mut impl FnMut(usize)) {
    visit(place.local.0 as usize);
    for projection in &place.projections {
        if let Projection::Index(index) = projection {
            visit_operand(index, visit);
        }
    }
}

fn visit_operand(operand: &Operand, visit: &mut impl FnMut(usize)) {
    if let Operand::Copy(place) | Operand::Move(place) | Operand::Ref(place) = operand {
        visit_place(place, visit);
    }
}

fn visit_rvalue(rvalue: &Rvalue, visit: &mut impl FnMut(usize)) {
    match rvalue {
        Rvalue::Use(value)
        | Rvalue::Unary(_, value)
        | Rvalue::Convert(value, _)
        | Rvalue::Error(value)
        | Rvalue::Spawn(value)
        | Rvalue::InterfaceBox(value) => visit_operand(value, visit),
        Rvalue::InterfaceView { place, .. } => visit_place(place, visit),
        Rvalue::Binary(_, left, right)
        | Rvalue::BoundsCheck(left, right)
        | Rvalue::StringChar(left, right)
        | Rvalue::StringAdvance(left, right) => {
            visit_operand(left, visit);
            visit_operand(right, visit);
        }
        Rvalue::Length(place) | Rvalue::Ref(place) => visit_place(place, visit),
        Rvalue::MapKeyAt(place, index) | Rvalue::MapValueRef(place, index) => {
            visit_place(place, visit);
            visit_operand(index, visit);
        }
        Rvalue::StringSlice { source, low, high } => {
            visit_operand(source, visit);
            for bound in low.iter().chain(high) {
                visit_operand(bound, visit);
            }
        }
        Rvalue::Slice {
            place, low, high, ..
        } => {
            visit_place(place, visit);
            for bound in low.iter().chain(high) {
                visit_operand(bound, visit);
            }
        }
        Rvalue::Aggregate(_, values) => {
            for value in values {
                visit_operand(value, visit);
            }
        }
        Rvalue::Closure { captures, .. } => {
            for (place, _) in captures {
                visit_place(place, visit);
            }
        }
        Rvalue::Zero | Rvalue::Global(_) => {}
    }
}

fn visit_statement(statement: &Statement, visit: &mut impl FnMut(usize)) {
    match statement {
        Statement::Assign { place, rvalue, .. } => {
            visit_place(place, visit);
            visit_rvalue(rvalue, visit);
        }
        Statement::Drop { place, .. } => visit_place(place, visit),
        Statement::EndScope(locals) => {
            for local in locals {
                visit(local.0 as usize);
            }
        }
        Statement::SetGlobal { value, .. } => visit_operand(value, visit),
    }
}

fn visit_terminator(terminator: &Terminator, visit: &mut impl FnMut(usize)) {
    match terminator {
        Terminator::Branch { condition, .. } => visit_operand(condition, visit),
        Terminator::Call {
            callee,
            args,
            destinations,
            ..
        } => {
            if let mir::Callee::Value(place) = callee {
                visit_place(place, visit);
            }
            for argument in args {
                visit_operand(argument, visit);
            }
            for destination in destinations.iter().flatten() {
                visit_place(destination, visit);
            }
        }
        Terminator::Assert { place, rvalue, .. } => {
            visit_place(place, visit);
            visit_rvalue(rvalue, visit);
        }
        Terminator::Goto(_)
        | Terminator::Return
        | Terminator::PanicReturn
        | Terminator::Unreachable => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::Path;

    use super::{
        LocalStorage, stable_operand, stable_rvalue, visit_operand, visit_place, visit_rvalue,
        visit_statement, visit_terminator,
    };
    use crate::driver::project::{Disk, load_project};
    use crate::hir;
    use crate::mir::{self, Statement, Terminator};
    use crate::source::SourceMap;

    // The previous all-points construction, kept as the reference the faster analysis must match.
    fn reference_reuse(
        body: &mir::Body,
        package: &hir::Package,
        storage: &[LocalStorage],
    ) -> Vec<usize> {
        let count = body.locals.len();
        let mut eligible: Vec<bool> = body
            .locals
            .iter()
            .enumerate()
            .map(|(index, local)| {
                storage[index] == LocalStorage::Frame
                    && !local.by_reference
                    && !package.needs_drop(local.ty)
                    && !package.contains_view(local.ty)
            })
            .collect();
        for local in body
            .params
            .iter()
            .chain(&body.captures)
            .chain(&body.returns)
        {
            eligible[local.0 as usize] = false;
        }
        let mut stable = vec![false; count];
        for block in &body.blocks {
            for statement in &block.statements {
                if let Statement::Assign { rvalue, .. } = statement {
                    stable_rvalue(rvalue, &mut stable);
                }
            }
            match &block.terminator {
                Terminator::Call { args, .. } => {
                    for arg in args {
                        stable_operand(arg, &mut stable);
                    }
                }
                Terminator::Assert { rvalue, .. } => stable_rvalue(rvalue, &mut stable),
                _ => {}
            }
        }
        for (index, is_stable) in stable.into_iter().enumerate() {
            eligible[index] &= !is_stable;
        }

        let mut entry = vec![vec![false; count]; body.blocks.len()];
        loop {
            let mut changed = false;
            for (index, block) in body.blocks.iter().enumerate().rev() {
                let mut live = successor_liveness(body, &entry, &block.terminator);
                transfer_terminator(&block.terminator, &mut live);
                for statement in block.statements.iter().rev() {
                    transfer_statement(statement, &mut live);
                }
                if live != entry[index] {
                    entry[index] = live;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        let mut interference = vec![HashSet::new(); count];
        for block in &body.blocks {
            let mut live = successor_liveness(body, &entry, &block.terminator);
            mark_overlap(&live, &eligible, &mut interference);
            let mut touched = live.clone();
            visit_terminator(&block.terminator, &mut |local| touched[local] = true);
            mark_overlap(&touched, &eligible, &mut interference);
            transfer_terminator(&block.terminator, &mut live);
            mark_overlap(&live, &eligible, &mut interference);
            for statement in block.statements.iter().rev() {
                let mut touched = live.clone();
                visit_statement(statement, &mut |local| touched[local] = true);
                mark_overlap(&touched, &eligible, &mut interference);
                transfer_statement(statement, &mut live);
                mark_overlap(&live, &eligible, &mut interference);
            }
        }

        let mut reuse: Vec<usize> = (0..count).collect();
        for index in 0..count {
            if !eligible[index] {
                continue;
            }
            for owner in 0..index {
                if !eligible[owner]
                    || reuse[owner] != owner
                    || body.locals[owner].ty != body.locals[index].ty
                {
                    continue;
                }
                if (0..index)
                    .any(|member| reuse[member] == owner && interference[index].contains(&member))
                {
                    continue;
                }
                reuse[index] = owner;
                break;
            }
        }
        reuse
    }

    fn successor_liveness(
        body: &mir::Body,
        entry: &[Vec<bool>],
        terminator: &Terminator,
    ) -> Vec<bool> {
        let mut live = vec![false; body.locals.len()];
        let mut successors = terminator.successors();
        match terminator {
            Terminator::Call {
                unwind: Some(unwind),
                ..
            }
            | Terminator::Assert {
                unwind: Some(unwind),
                ..
            } => successors.push(*unwind),
            _ => {}
        }
        for successor in successors {
            for (index, used) in entry[successor.0 as usize].iter().enumerate() {
                live[index] |= used;
            }
        }
        live
    }

    fn transfer_terminator(terminator: &Terminator, live: &mut [bool]) {
        match terminator {
            Terminator::Call {
                callee,
                args,
                destinations,
                ..
            } => {
                for place in destinations.iter().flatten() {
                    visit_place(place, &mut |local| live[local] = true);
                }
                if let mir::Callee::Value(place) = callee {
                    visit_place(place, &mut |local| live[local] = true);
                }
                for arg in args {
                    visit_operand(arg, &mut |local| live[local] = true);
                }
            }
            _ => visit_terminator(terminator, &mut |local| live[local] = true),
        }
    }

    fn transfer_statement(statement: &Statement, live: &mut [bool]) {
        if let Statement::Assign { place, rvalue, .. } = statement {
            if place.projections.is_empty() {
                live[place.local.0 as usize] = false;
            } else {
                visit_place(place, &mut |local| live[local] = true);
            }
            visit_rvalue(rvalue, &mut |local| live[local] = true);
        } else {
            visit_statement(statement, &mut |local| live[local] = true);
        }
    }

    fn mark_overlap(live: &[bool], eligible: &[bool], interference: &mut [HashSet<usize>]) {
        let active: Vec<_> = live
            .iter()
            .enumerate()
            .filter_map(|(index, used)| (*used && eligible[index]).then_some(index))
            .collect();
        for (position, &left) in active.iter().enumerate() {
            for &right in &active[position + 1..] {
                interference[left].insert(right);
                interference[right].insert(left);
            }
        }
    }

    fn compare(checked: crate::check::Checked) -> (usize, usize) {
        assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
        let package = checked.package.unwrap();
        let mut program = mir::lower::lower(&package);
        crate::dropck::insert(&package, &mut program);
        let plan = crate::async_lowering::lower(&package, &program);
        let mut compared = 0;
        let mut shared = 0;
        for (function, machine) in &plan.machines {
            let body = &program.bodies[function.0 as usize];
            let expected = reference_reuse(body, &package, &machine.storage);
            assert_eq!(machine.frame_reuse, expected, "{}", body.name);
            compared += 1;
            shared += expected
                .iter()
                .enumerate()
                .filter(|(index, owner)| index != *owner)
                .count();
        }
        (compared, shared)
    }

    const CASES: &[&str] = &[
        "package main
async func mix(ch channel<int>, n int) int {
    var total = 0
    for var i = 0; i < n; i += 1 {
        let a = i * 2
        ch.send(a)
        let b = a + 1
        if b > 3 {
            let c = b * 2
            ch.send(c)
            total += c
        } else {
            let d = b
            total += d
        }
    }
    return total
}
func main() {}
",
        "package main
async func spin(ch channel<int>) {
    var a = 1
    ch.send(a)
    let b = a + 1
    ch.send(b)
    for {
        let c = b * 3
        ch.send(c)
    }
}
func main() {}
",
        "package main
async func views(ch channel<int>) int {
    var values = [int; 4]{1, 2, 3, 4}
    let first = values[0]
    ch.send(first)
    let second = values[1]
    ch.send(second)
    var other = [int; 4]{5, 6, 7, 8}
    ch.send(other[2])
    return first + second
}
func main() {}
",
    ];

    fn scaling(locals: usize) -> String {
        let mut source = String::from(
            "package main\nasync func run(ch channel<int>) int {\nch.send(0)\nvar sum = 0\n",
        );
        for i in 0..locals {
            source.push_str(&format!("let v{i} = {i}\nch.send(v{i})\n"));
        }
        for i in 0..locals {
            source.push_str(&format!("sum += v{i}\n"));
        }
        source.push_str("return sum\n}\nfunc main() {}\n");
        source
    }

    #[test]
    fn interference_matches_the_all_points_construction() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .canonicalize()
            .unwrap();
        let mut files = Vec::new();
        for folder in ["examples", "benchmarks"] {
            let mut pending = vec![root.join(folder)];
            while let Some(directory) = pending.pop() {
                for entry in std::fs::read_dir(directory).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        pending.push(path);
                    } else if path
                        .file_name()
                        .is_some_and(|name| name == "main.ore" || name == "workload.ore")
                    {
                        files.push(path);
                    }
                }
            }
        }
        files.sort();
        assert!(files.len() >= 20, "{files:?}");
        let (mut compared, mut shared) = (0, 0);
        for path in &files {
            let mut sources = SourceMap::new();
            let Ok(project) = load_project(&mut sources, &Disk, path) else {
                panic!("cannot load {}", path.display());
            };
            let (machines, slots) = compare(crate::check::check_project(&project, &sources));
            compared += machines;
            shared += slots;
        }
        for (index, case) in CASES
            .iter()
            .map(|case| case.to_string())
            .chain([scaling(24)])
            .enumerate()
        {
            let mut sources = SourceMap::new();
            let id = sources.add(format!("case{index}.ore"), case).unwrap();
            let (machines, slots) = compare(crate::check::check_file(sources.file(id).unwrap()));
            compared += machines;
            shared += slots;
        }
        assert!(compared >= 10, "{compared} state machines compared");
        assert!(shared > 0, "no compared function shares a slot");
    }
}
