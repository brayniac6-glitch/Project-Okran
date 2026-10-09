//! The AI's task planner (port_map `ai`): how a chosen goal becomes a chain of tasks.
//!
//! Spec: F036920 TaskPlanner::plan, F037121 getNode, F037117 Node::Node, F037120 Node::expand,
//! F037118 Node::branch, F037066 Node::isRedundant, F036919 TaskPlanner::takePlan, F037114
//! TaskPlanner::prune, F037078 popBest, F039872 TaskRepertoire::index, F039873 solvers,
//! F039870 preconditionsHold, F039867 checkCondition, F036921 AITaskSytem::planScore.
//!
//! It is a regressive best-first search over task definitions (`task_def`): a node is a task
//! with a target and a place. Expanding a node finds its preconditions that fail now and, for
//! each, the tasks whose completion conditions make it hold (the repertoire's solver index); those
//! become child nodes. The node with the highest score (+0x54 + +0x58; a child keeps the lowest
//! score along its chain) is taken next; one whose preconditions all hold can run now, and its
//! chain up to the goal becomes the plan (first task first).

use crate::task_def::{DEFS, TaskDef, def};
use std::collections::HashMap;

/// Task types the redundancy check never drops (F037066): moving and running.
const ALWAYS_NEW: [u16; 2] = [1, 0x43];
/// Placeholder task a precondition gets when nothing in the repertoire solves it (F037119).
pub const NO_SOLUTION: u16 = 0x68;
/// Nodes with the same task, target and place are shared after this many (F037121, F036964).
const SHARE_AFTER: usize = 3;
/// Ancestors the cycle check walks (F037066).
const CYCLE_DEPTH: usize = 50;

/// The six global TaskRepertoires and their registration mask bits (F162958..F162963).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Repertoire {
    Human = 1,
    Animal = 2,
    Chained = 4,
    Platoon = 8,
    Imprisoned = 0x10,
    Crippled = 0x20,
}

impl Repertoire {
    /// F039861/F039862: the repertoire an AI uses from its state flags (+0x200): imprisoned (4),
    /// crippled (0x40), chained (0x10), human (1), then platoon (2) or animal.
    /// @fn F039861
    pub fn for_flags(flags: u32) -> Self {
        if flags & 4 != 0 {
            Repertoire::Imprisoned
        } else if flags & 0x40 != 0 {
            Repertoire::Crippled
        } else if flags & 0x10 != 0 {
            Repertoire::Chained
        } else if flags & 1 != 0 {
            Repertoire::Human
        } else if flags & 2 != 0 {
            Repertoire::Platoon
        } else {
            Repertoire::Animal
        }
    }

    /// F039864 adds a type to a repertoire whose mask bit is set; TaskRepertoire vfunc 8 asks it.
    pub fn offers(self, d: &TaskDef) -> bool {
        d.repertoires & self as u8 != 0
    }
}

/// F039872: (condition, expected) -> the tasks of a repertoire that make it so, in type order.
/// Tasks with +0x40 set are never solvers.
/// @fn F039872
pub fn solver_index(rep: Repertoire) -> HashMap<(u16, bool), Vec<u16>> {
    let mut m: HashMap<(u16, bool), Vec<u16>> = HashMap::new();
    for d in DEFS.iter().filter(|d| rep.offers(d) && d.f40 == 0) {
        for &(c, expected, _) in d.done_when {
            m.entry((c, expected)).or_default().push(d.task);
        }
    }
    m
}

/// What the planner asks of the character it plans for.
pub trait Agent {
    type Target: Clone + PartialEq + std::fmt::Debug;
    /// The empty target (a handle of type 0xb).
    fn no_target(&self) -> Self::Target;
    /// The check behind a condition (F039867), `None` when the condition has none.
    fn check(&mut self, condition: u16, target: &Self::Target, pos: [f32; 3]) -> Option<bool>;
    /// A task's score method (F037110 / F039871).
    fn score(&mut self, d: &TaskDef, target: &Self::Target, pos: [f32; 3]) -> f32;
    /// The task's target finder (+0x160 hook); `None` when it finds nothing.
    fn find_target(
        &mut self,
        d: &TaskDef,
        target: &Self::Target,
        pos: [f32; 3],
    ) -> Option<Self::Target>;
    /// Tasks marked +0x41 are skipped unless the plan is forced (the character +0x250 check).
    fn restricted(&self) -> bool {
        false
    }
}

/// F039867: the entry holds when its check answers `expected`; a condition without a check holds.
/// @fn F039867
pub fn holds<A: Agent>(a: &mut A, entry: (u16, bool), target: &A::Target, pos: [f32; 3]) -> bool {
    a.check(entry.0, target, pos).is_none_or(|v| v == entry.1)
}

/// F039870: the task's preconditions in order, then its target condition on `target2` (the
/// target the finder gave, run here when `search` is set). Returns the first failing entry.
/// @fn F039870
pub fn preconditions_hold<A: Agent>(
    a: &mut A,
    d: &TaskDef,
    target: &A::Target,
    pos: [f32; 3],
    target2: &mut A::Target,
    search: bool,
) -> Result<(), (u16, bool)> {
    for &(c, e, _) in d.preconditions {
        if !holds(a, (c, e), target, pos) {
            return Err((c, e));
        }
    }
    if let Some(tc) = d.target_condition {
        if *target2 == a.no_target()
            && search
            && d.hook.is_some()
            && let Some(t) = a.find_target(d, target, pos)
        {
            *target2 = t;
        }
        if *target2 == a.no_target() || !holds(a, tc, &target2.clone(), pos) {
            return Err(tc);
        }
    }
    Ok(())
}

/// A planning node (F037117, 0xb0 bytes).
#[derive(Debug, Clone)]
pub struct Node<T> {
    pub task: u16,
    /// +0x4: the task runs as a step of the plan (the definition's +0x134 flag or a target
    /// condition); otherwise it only carries conditions.
    pub runs: bool,
    pub target: T,
    /// +0x28: the target the finder gave.
    pub target2: T,
    pub pos: [f32; 3],
    /// +0x54 and +0x58: both start as the task's score; +0x54 drops to the lowest along a chain.
    pub chain_score: f32,
    pub score: f32,
    /// +0x60
    pub parent: Option<usize>,
    /// +0x68..: failing precondition -> child nodes that solve it, in the order found.
    pub branches: Vec<((u16, bool), Vec<usize>)>,
}

/// The search state (TaskPlanner, AITaskSytem +0x?).
pub struct Planner<'a, A: Agent> {
    pub agent: &'a mut A,
    pub rep: Repertoire,
    solvers: HashMap<(u16, bool), Vec<u16>>,
    /// +0x20: plan even with restricted (+0x41) tasks.
    pub forced: bool,
    pub nodes: Vec<Node<A::Target>>,
    open: Vec<usize>,
    closed: Vec<usize>,
}

impl<'a, A: Agent> Planner<'a, A> {
    pub fn new(agent: &'a mut A, rep: Repertoire) -> Self {
        Planner {
            agent,
            rep,
            solvers: solver_index(rep),
            forced: false,
            nodes: vec![],
            open: vec![],
            closed: vec![],
        }
    }

    fn def(&self, n: usize) -> &'static TaskDef {
        def(self.nodes[n].task).expect("registered task")
    }

    fn same(&self, a: usize, b: usize) -> bool {
        let (x, y) = (&self.nodes[a], &self.nodes[b]);
        x.task == y.task && x.target == y.target && x.pos == y.pos
    }

    /// F037121: a node for (task, target, place): an existing one once more than three share
    /// them, else a new one, expanded at once (F037120).
    /// @fn F037121
    fn node(&mut self, d: &'static TaskDef, target: A::Target, pos: [f32; 3]) -> Option<usize> {
        let alike: Vec<usize> = (0..self.nodes.len())
            .filter(|&i| {
                self.nodes[i].task == d.task
                    && self.nodes[i].target == target
                    && self.nodes[i].pos == pos
            })
            .collect();
        if alike.len() > SHARE_AFTER {
            return alike.first().copied();
        }
        let s = self.agent.score(d, &target, pos);
        let none = self.agent.no_target();
        self.nodes.push(Node {
            task: d.task,
            runs: d.f134 != 0 || d.target_condition.is_some(),
            target,
            target2: none,
            pos,
            chain_score: s,
            score: s,
            parent: None,
            branches: vec![],
        });
        let n = self.nodes.len() - 1;
        self.expand(n).then_some(n)
    }

    /// F037119: the placeholder child of a precondition nothing solves.
    /// @fn F037119
    fn dead_end(&mut self) -> usize {
        let none = self.agent.no_target();
        self.nodes.push(Node {
            task: NO_SOLUTION,
            runs: false,
            target: none.clone(),
            target2: none,
            pos: [0.0; 3],
            chain_score: 0.0,
            score: 0.0,
            parent: None,
            branches: vec![],
        });
        self.nodes.len() - 1
    }

    /// F037120: for each failing precondition (last first; the target condition first of all,
    /// its target found by the hook), the solver tasks as children; none -> a dead end.
    /// @fn F037120
    fn expand(&mut self, n: usize) -> bool {
        let d = self.def(n);
        if self.nodes[n].score <= 0.0 {
            return true;
        }
        let (target, pos) = (self.nodes[n].target.clone(), self.nodes[n].pos);
        let mut needed: Vec<((u16, bool), bool)> = vec![];
        if let Some(tc) = d.target_condition {
            needed.push((tc, true));
        }
        for &(c, e, _) in d.preconditions.iter().rev() {
            if !holds(self.agent, (c, e), &target, pos) {
                needed.push(((c, e), false));
            }
        }
        for (entry, is_target) in needed {
            let mut child_target = target.clone();
            if is_target && let Some(t) = self.agent.find_target(d, &target, pos) {
                child_target = t.clone();
                self.nodes[n].target2 = t;
            }
            let mut kids = vec![];
            let solvers = self.solvers.get(&entry).cloned().unwrap_or_default();
            for s in solvers {
                let sd = def(s).expect("registered task");
                if sd.f41 != 0 && !self.forced && self.agent.restricted() {
                    continue;
                }
                if child_target == self.agent.no_target() && sd.f48 != 0 {
                    continue;
                }
                if let Some(c) = self.node(sd, child_target.clone(), pos) {
                    kids.push(c);
                }
            }
            if kids.is_empty() {
                kids.push(self.dead_end());
            }
            self.nodes[n].branches.push((entry, kids));
        }
        true
    }

    /// F037118: the children of the node's first precondition (in definition order) still
    /// waiting, else of its target condition.
    /// @fn F037118
    fn branch(&self, n: usize) -> Option<Vec<usize>> {
        let d = self.def(n);
        let node = &self.nodes[n];
        let find = |e: (u16, bool)| node.branches.iter().find(|b| b.0 == e).map(|b| b.1.clone());
        d.preconditions
            .iter()
            .find_map(|&(c, e, _)| find((c, e)))
            .or_else(|| d.target_condition.and_then(find))
    }

    /// F037066: whether the node repeats a planned step or one of its own ancestors.
    /// @fn F037066
    fn redundant(&self, n: usize, plan: &[usize]) -> bool {
        if ALWAYS_NEW.contains(&self.nodes[n].task) {
            return false;
        }
        if plan.iter().any(|&p| self.same(p, n)) {
            return true;
        }
        let mut a = self.nodes[n].parent;
        for _ in 0..=CYCLE_DEPTH {
            match a {
                Some(p) if p != n => {
                    if self.same(p, n) {
                        return true;
                    }
                    a = self.nodes[p].parent;
                }
                _ => return false,
            }
        }
        false
    }

    /// F037114: drop from every ancestor the branches that a node of this chain answers.
    /// @fn F037114
    fn prune(&mut self, from: usize) {
        let mut chain = vec![from];
        let mut a = self.nodes[from].parent;
        while let Some(p) = a {
            if chain.contains(&p) {
                break;
            }
            chain.push(p);
            a = self.nodes[p].parent;
        }
        for &p in &chain {
            let keep: Vec<((u16, bool), Vec<usize>)> = self.nodes[p]
                .branches
                .iter()
                .filter(|(_, kids)| !kids.iter().any(|k| chain.contains(k)))
                .cloned()
                .collect();
            self.nodes[p].branches = keep;
        }
    }

    /// F036919: the chain from `n` up to the first node already planned goes into the plan
    /// (first to run first). Unless redundant, the search goes on from the nearest ancestor that
    /// still has branches (`Err` with it); otherwise it is done when the plan is not empty.
    /// @fn F036919
    fn take(&mut self, n: usize, plan: &mut Vec<usize>, redundant: bool) -> Result<bool, usize> {
        let mut v = n;
        loop {
            if plan.iter().any(|&p| self.same(p, v)) {
                break;
            }
            plan.push(v);
            match self.nodes[v].parent {
                Some(p) => v = p,
                None => break,
            }
        }
        self.prune(n);
        if !redundant {
            let mut u = Some(n);
            while let Some(x) = u {
                if !self.nodes[x].branches.is_empty() {
                    return Err(x);
                }
                u = self.nodes[x].parent;
            }
        }
        Ok(!plan.is_empty())
    }

    /// F037078: the open node with the highest +0x54 + +0x58 (the first such).
    /// @fn F037078
    fn pop_best(&mut self) -> usize {
        let mut best = 0;
        for i in 1..self.open.len() {
            let (b, c) = (&self.nodes[self.open[best]], &self.nodes[self.open[i]]);
            if b.chain_score + b.score < c.chain_score + c.score {
                best = i;
            }
        }
        self.open.remove(best)
    }

    /// F036920: plan for the goal task `task` on `target` at `pos`. Returns the plan (node ids,
    /// first to run first) or `None`.
    /// @fn F036920
    pub fn plan(&mut self, task: u16, target: A::Target, pos: [f32; 3]) -> Option<Vec<usize>> {
        let gd = def(task)?;
        let root = self.node(gd, target, pos)?;
        let mut plan = vec![];
        if gd.hook.is_some() {
            let (t, p) = (self.nodes[root].target.clone(), self.nodes[root].pos);
            self.agent.find_target(gd, &t, p)?;
        } else {
            let (t, p) = (self.nodes[root].target.clone(), self.nodes[root].pos);
            let mut t2 = self.nodes[root].target2.clone();
            if preconditions_hold(self.agent, gd, &t, p, &mut t2, false).is_ok() {
                plan.push(root);
                return Some(plan);
            }
        }
        self.open.push(root);
        let (mut found, mut partial) = (false, false);
        while !self.open.is_empty() {
            let mut n = self.pop_best();
            let d = self.def(n);
            let redundant = self.redundant(n, &plan);
            let runnable = d.hook.is_none() && d.f134 != 0 && {
                let (t, p) = (self.nodes[n].target.clone(), self.nodes[n].pos);
                let mut t2 = self.nodes[n].target.clone();
                preconditions_hold(self.agent, d, &t, p, &mut t2, false).is_ok()
            };
            if runnable || redundant {
                found = true;
                match self.take(n, &mut plan, redundant) {
                    Ok(true) => return Some(self.finish(plan)),
                    Ok(false) => {}
                    Err(ancestor) => n = ancestor,
                }
            }
            if let Some(kids) = self.branch(n) {
                for c in kids {
                    if self.open.contains(&c) {
                        continue;
                    }
                    if self.closed.contains(&c) {
                        match self.take(n, &mut plan, redundant) {
                            Ok(true) => return Some(self.finish(plan)),
                            _ => partial = true,
                        }
                        continue;
                    }
                    if self.nodes[n].chain_score < self.nodes[c].chain_score {
                        self.nodes[c].chain_score = self.nodes[n].chain_score;
                    }
                    self.nodes[c].parent = Some(n);
                    self.open.retain(|&o| o != c);
                    self.closed.retain(|&o| o != c);
                    self.open.push(c);
                }
            }
            self.close(n);
        }
        if (found && partial) || (found && gd.f23 != 0 && !plan.is_empty()) {
            return Some(self.finish(plan));
        }
        None
    }

    /// F036992: off the open list, onto the closed one.
    /// @fn F036992
    fn close(&mut self, n: usize) {
        self.open.retain(|&o| o != n);
        if !self.closed.contains(&n) {
            self.closed.push(n);
        }
    }

    fn finish(&self, plan: Vec<usize>) -> Vec<usize> {
        plan
    }

    /// F036921: the steps of a plan that run (+0x4 set), first to run first, and the plan's
    /// start score: the lowest +0x58 along it (0 when the plan is empty).
    /// @fn F036921
    pub fn steps(&self, plan: &[usize]) -> (Vec<&Node<A::Target>>, f32) {
        let score = plan
            .iter()
            .map(|&n| self.nodes[n].score)
            .fold(f32::INFINITY, f32::min);
        let steps = plan
            .iter()
            .map(|&n| &self.nodes[n])
            .filter(|n| n.runs)
            .collect();
        (steps, if plan.is_empty() { 0.0 } else { score })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A character whose conditions answer from a table (missing ones: no check).
    struct Toy(HashMap<u16, bool>);
    impl Agent for Toy {
        type Target = u32;
        fn no_target(&self) -> u32 {
            0
        }
        fn check(&mut self, c: u16, _: &u32, _: [f32; 3]) -> Option<bool> {
            self.0.get(&c).copied()
        }
        fn score(&mut self, _: &TaskDef, _: &u32, _: [f32; 3]) -> f32 {
            1.0
        }
        fn find_target(&mut self, _: &TaskDef, t: &u32, _: [f32; 3]) -> Option<u32> {
            Some(*t)
        }
    }

    #[test]
    fn repertoire_from_flags() {
        assert_eq!(Repertoire::for_flags(1), Repertoire::Human);
        assert_eq!(Repertoire::for_flags(1 | 4), Repertoire::Imprisoned);
        assert_eq!(Repertoire::for_flags(0), Repertoire::Animal);
    }

    #[test]
    fn solvers_come_from_completion_conditions() {
        let s = solver_index(Repertoire::Human);
        // Moving (1) is done when at its location: it solves isAtLocation
        assert!(s[&(1, true)].contains(&1));
        assert!(s.values().flatten().all(|&t| def(t).unwrap().f40 == 0));
    }

    #[test]
    fn a_goal_whose_preconditions_hold_plans_itself() {
        let mut a = Toy(HashMap::new());
        let mut p = Planner::new(&mut a, Repertoire::Human);
        let plan = p.plan(29, 7, [0.0; 3]).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(p.nodes[plan[0]].task, 29);
    }

    #[test]
    fn a_failing_precondition_is_solved_first() {
        // Move order (29) needs not crouching (24 false): crouching now
        let mut a = Toy(HashMap::from([(24, true)]));
        let mut p = Planner::new(&mut a, Repertoire::Human);
        let solvers = solver_index(Repertoire::Human);
        let plan = p.plan(29, 7, [0.0; 3]).expect("a plan");
        let tasks: Vec<u16> = plan.iter().map(|&n| p.nodes[n].task).collect();
        assert_eq!(*tasks.last().unwrap(), 29, "{tasks:?}");
        assert!(solvers[&(24, false)].contains(&tasks[0]), "{tasks:?}");
    }
}
