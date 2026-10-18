//! A character's task system (port_map `ai`): its goals, orders and jobs, and how it decides
//! what to do (AITaskSytem).
//!
//! Spec: F036926 decide, F036927 think, F036915 rankGoals, F036914 rankList, F036923
//! startBest, F036921 planScore, F036895 setTask, F036876 setDefaultGoal, F036879 isUrgent,
//! F039871 Goal::score.
//!
//! The decision is a cascade: orders and urgent goals first, then urgent jobs, all goals,
//! non-urgent and fluff jobs, then the default idle goal. Each pass ranks candidates into a map
//! (highest key first) and starts the first whose plan (`planner`) scores above zero; the plan's
//! running steps are queued to be carried out.

use crate::planner::{Agent, Planner, Repertoire, holds, preconditions_hold};
use crate::task_def::{TaskDef, def};

/// Idleness, the default goal (F036876).
pub const IDLE: u16 = 14;
/// Goals a character without free will still takes (F036879): attacking enemies, staying close,
/// bodyguard, robotics, medic, splint rigging.
pub const URGENT_GOALS: [u16; 6] = [16, 31, 45, 57, 58, 250];
/// Goal keys start at the goal count + 8 and drop by one per goal considered (F036915).
const GOAL_KEY_BASE: f32 = 8.0;
/// rankList: the running task's bonus, its committed floor, the cap otherwise, and the step
/// between equal keys (constants 0x14168ba14, 0x141696c90, 0x141696c74, 0x14169c3c4).
const SAME_TASK_BONUS: f32 = 1.25;
const COMMITTED_FLOOR: f32 = 0.4;
const UNCOMMITTED_CAP: f32 = 0.05;
const KEY_STEP: f32 = 1e-4;

/// Task classification (AI_TASK "classification", task +0x8).
pub const FLUFF: u8 = 1;
pub const NON_URGENT: u8 = 2;
pub const URGENT: u8 = 3;

/// A task or goal instance (TaskRepertoire::create F030716 fills these).
#[derive(Debug, Clone, PartialEq)]
pub struct Task<T> {
    pub task: u16,
    pub classification: u8,
    pub target: T,
    /// +0x38: the target the definition's finder gave.
    pub target2: T,
    /// +0x30
    pub priority: f32,
    /// +0x58
    pub pos: [f32; 3],
    /// +0x64 / +0x68: hours the task may run in (0..100 for package goals: always).
    pub hours: (i32, i32),
}

impl<T> Task<T> {
    pub fn def(&self) -> &'static TaskDef {
        def(self.task).expect("registered task")
    }
}

/// rankList's hour window: start <= hour < end, wrapping past midnight when start >= end.
/// @fn F036914
pub fn in_hours(hours: (i32, i32), hour: i32) -> bool {
    let (s, e) = hours;
    if s < e {
        s <= hour && hour < e
    } else {
        hour >= s || hour < e
    }
}

/// What the task system needs from its character beyond the planner's questions.
pub trait Mind: Agent {
    /// The game hour (0-23).
    fn hour(&self) -> i32;
    /// A random number in [0, 1).
    fn rand01(&mut self) -> f32;
    /// Game time in hours.
    fn now(&self) -> f64;
    /// +0x35: the character decides for itself (not a player's character waiting for orders).
    fn free_will(&self) -> bool;
    /// How many others already do this job (the job manager count, F036914).
    fn others_on_job(&self, _t: &Task<Self::Target>) -> usize {
        0
    }
    /// The task object's own check that it has finished (Task vfunc +0x20).
    fn finished(&mut self, _t: &Task<Self::Target>) -> bool {
        false
    }
    fn repertoire(&self) -> Repertoire {
        Repertoire::Human
    }
    /// The built-in goals the goal ranking adds for the player's own people (F036915).
    fn idle_goals(&mut self) -> IdleGoals {
        IdleGoals::default()
    }
}

/// F036915's built-in goals, each allowed or not: for one of the player's faction with free
/// will (+0x35) and not sneaking (F039177), by the player's AI options (+0x110.., F035130):
/// Ditching resources (242, "Ditch items", +0x117, (assumption) the task system's +0x60 taken
/// as clear); Getting out of bed and Going to bed (257 / 153, "Sleep when injured", +0x116) and
/// Sitting around (275, "Sit when idle", +0x118), neither on hold (F029593) nor carrying
/// someone (Character +0x348) nor while F045533 answers (the player object's +0x208 list,
/// (not ported) taken as not).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct IdleGoals {
    pub ditch: bool,
    pub bed: bool,
    pub sit: bool,
}

/// The built-in goals' tasks.
pub const DITCHING_RESOURCES: u16 = 242;
pub const GETTING_OUT_OF_BED_INJURED: u16 = 257;
pub const GOING_TO_BED: u16 = 153;
pub const SITTING_AROUND: u16 = 275;
/// Sitting around's priority (0x14167e8d8).
const SITTING_PRIORITY: f32 = 0.5;

/// AITaskSytem.
#[derive(Debug, Clone)]
pub struct TaskSystem<T> {
    /// +0x90/+0x98
    pub goals: Vec<Task<T>>,
    /// +0x70: player and dialogue orders.
    pub orders: Vec<Task<T>>,
    /// +0x118 + 0x18 * classification: the character's own jobs.
    pub jobs: [Vec<Task<T>>; 4],
    /// +0xa0 + 0x18 * classification: goals added by classification (F036811: the hold
    /// stance's Attacking enemies), ranked with the jobs of their classification (F036925).
    pub classified: [Vec<Task<T>>; 5],
    /// +0x190
    pub default_goal: Task<T>,
    /// +0x1c0: the goal being followed, its key (+0x208) and classification (+0x20c).
    pub current: Option<Task<T>>,
    pub key: i32,
    pub classification: u8,
    /// +0x280: until when the current goal holds (game hours).
    pub until: f64,
    /// +0x340: the plan's tasks to run, first first.
    pub queue: Vec<Task<T>>,
}

/// Candidates ranked by key, highest first (the std::map the passes fill).
type Candidates<T> = Vec<(f32, Task<T>)>;

fn insert<T>(c: &mut Candidates<T>, mut key: f32, t: Task<T>) {
    while c.iter().any(|(k, _)| *k == key) {
        key += KEY_STEP;
    }
    let at = c.iter().position(|(k, _)| *k < key).unwrap_or(c.len());
    c.insert(at, (key, t));
}

impl<T: Clone + PartialEq + std::fmt::Debug> TaskSystem<T> {
    /// A task system with the idle default goal (F036876: classification 1, priority 1.0).
    /// @fn F036876
    pub fn new(none: T, pos: [f32; 3]) -> Self {
        TaskSystem {
            goals: vec![],
            orders: vec![],
            jobs: Default::default(),
            classified: Default::default(),
            default_goal: Task {
                task: IDLE,
                classification: FLUFF,
                target: none.clone(),
                target2: none,
                priority: 1.0,
                pos,
                hours: (0, 100),
            },
            current: None,
            key: 0,
            classification: 0,
            until: 0.0,
            queue: vec![],
        }
    }

    /// Whether a goal is already achieved or not worth considering (F036915/F036914): its
    /// preconditions hold but it has nothing to run (definition +0x14 clear), or the task is
    /// finished (Task vfunc +0x20: the base answers definition +0x15 == 0, F030701), done (its
    /// completion conditions hold, F039869) and its target condition met (F036892).
    fn achieved<M: Mind<Target = T>>(m: &mut M, t: &Task<T>) -> bool {
        let d = t.def();
        if d.f14 == 0 {
            let mut t2 = t.target2.clone();
            if preconditions_hold(m, d, &t.target, t.pos, &mut t2, true).is_ok()
                && d.f134 == 0
                && d.target_condition.is_none()
            {
                return true;
            }
        }
        (d.f15 == 0 || m.finished(t)) && is_done(m, d, t) && target_condition_holds(m, d, t)
    }

    /// F036915: the goals in list order, keys from count + 8 down; without free will only the
    /// urgent goals; on the urgent pass only turret goals (definition +0x04 == 2). On the other
    /// pass, the built-in goals the character may have (`IdleGoals`) below them: Ditching
    /// resources (class 2, priority 1), Getting out of bed (3, 1) and Going to bed (2, 1), one
    /// key apart, and Sitting around (1, 0.5).
    /// @fn F036915
    fn rank_goals<M: Mind<Target = T>>(&self, m: &mut M, c: &mut Candidates<T>, urgent_pass: bool) {
        let mut key = self.goals.len() as f32 + GOAL_KEY_BASE;
        for g in &self.goals {
            if !m.free_will() && !URGENT_GOALS.contains(&g.task) {
                continue;
            }
            if urgent_pass && g.def().f04 != 2 {
                continue;
            }
            if Self::achieved(m, g) {
                continue;
            }
            key -= 1.0;
            if goal_score(m, g) > 0.0 {
                insert(c, key, g.clone());
            }
        }
        if urgent_pass || !m.free_will() {
            return;
        }
        let idle = m.idle_goals();
        let none = self.default_goal.target.clone();
        let goal = |task: u16, classification: u8, priority: f32| Task {
            task,
            classification,
            target: none.clone(),
            target2: none.clone(),
            priority,
            pos: [0.0; 3],
            hours: (0, 100),
        };
        let mut add = |m: &mut M, key: f32, g: Task<T>| {
            if goal_score(m, &g) > 0.0 {
                insert(c, key, g);
            }
        };
        if idle.ditch {
            key -= 1.0;
            add(m, key, goal(DITCHING_RESOURCES, NON_URGENT, 1.0));
        }
        if idle.bed {
            add(m, key - 1.0, goal(GETTING_OUT_OF_BED_INJURED, URGENT, 1.0));
            key -= 2.0;
            add(m, key, goal(GOING_TO_BED, NON_URGENT, 1.0));
        }
        if idle.sit {
            add(m, key - 1.0, goal(SITTING_AROUND, FLUFF, SITTING_PRIORITY));
        }
    }

    /// F036914: orders and jobs by score * priority, crowded jobs scaled by the definition's
    /// +0x34 per other worker, the running task favoured.
    /// @fn F036914
    fn rank_list<M: Mind<Target = T>>(&self, m: &mut M, c: &mut Candidates<T>, list: &[Task<T>]) {
        let hour = m.hour();
        for t in list {
            let d = t.def();
            if !in_hours(t.hours, hour) || d.f130 != 0 {
                continue;
            }
            let mut s = goal_score(m, t) * t.priority;
            for _ in 0..m.others_on_job(t) {
                s *= d.f34;
            }
            let is_current = self
                .current
                .as_ref()
                .is_some_and(|c| c.task == t.task && c.target == t.target);
            if is_current {
                s *= SAME_TASK_BONUS;
                s = if m.now() < self.until {
                    s.max(COMMITTED_FLOOR)
                } else if s > UNCOMMITTED_CAP {
                    UNCOMMITTED_CAP
                } else {
                    s
                };
            } else if Self::achieved(m, t) {
                continue;
            }
            if s > 0.0 {
                insert(c, s, t.clone());
            }
        }
    }

    /// F036923 with F036921: from the highest key, the first candidate whose plan scores above
    /// zero becomes the current goal and its plan's running steps are queued.
    /// @fn F036923
    fn start_best<M: Mind<Target = T>>(&mut self, m: &mut M, c: Candidates<T>) -> bool {
        for (key, t) in c {
            if key <= 0.0 {
                continue;
            }
            if self.current.as_ref() == Some(&t) && !self.queue.is_empty() {
                return true;
            }
            let rep = m.repertoire();
            let mut p = Planner::new(m, rep);
            let Some(plan) = p.plan(t.task, t.target.clone(), t.pos) else {
                continue;
            };
            let (steps, score) = p.steps(&plan);
            if score <= 0.0 {
                continue;
            }
            let queue: Vec<Task<T>> = steps
                .iter()
                .map(|n| Task {
                    task: n.task,
                    classification: t.classification,
                    target: n.target.clone(),
                    target2: n.target2.clone(),
                    priority: t.priority,
                    pos: n.pos,
                    hours: t.hours,
                })
                .collect();
            self.queue = queue;
            self.set_task(m, t, key as i32);
            return true;
        }
        false
    }

    /// F036895: follow a goal for at least its definition's duration (+0x18 + rand * +0x1c).
    /// @fn F036895
    fn set_task<M: Mind<Target = T>>(&mut self, m: &mut M, t: Task<T>, key: i32) {
        let d = t.def();
        self.until = m.now() + (m.rand01() * d.duration.1 + d.duration.0) as f64;
        self.classification = t.classification;
        self.key = key;
        self.current = Some(t);
    }

    /// F036926: the cascade. `only_higher` (a goal is held, F036927) skips passes at or below
    /// the current classification. Returns whether a goal was started or kept.
    /// @fn F036926
    pub fn decide<M: Mind<Target = T>>(&mut self, m: &mut M, only_higher: bool) -> bool {
        let mut c: Candidates<T> = vec![];
        if only_higher && self.classification > URGENT - 1 {
            return false;
        }
        let orders = self.orders.clone();
        self.rank_list(m, &mut c, &orders);
        if !self.goals.is_empty() {
            self.rank_goals(m, &mut c, true);
            if self.start_best(m, std::mem::take(&mut c)) {
                return true;
            }
        }
        if self.try_jobs(m, URGENT) {
            return true;
        }
        if only_higher && self.classification > NON_URGENT - 1 {
            return false;
        }
        self.rank_goals(m, &mut c, false);
        if self.start_best(m, std::mem::take(&mut c)) {
            return true;
        }
        if self.try_jobs(m, NON_URGENT) {
            return true;
        }
        if only_higher && self.classification > FLUFF - 1 {
            return false;
        }
        if self.try_jobs(m, FLUFF) {
            return true;
        }
        let idle = self.default_goal.clone();
        self.start_best(m, vec![(1.0, idle)])
    }

    /// F036925: the jobs of one classification (+0x118) and the goals added at it (+0xa0), ranked
    /// together. (not ported) The job manager's list (+0x2e8, F041092).
    /// @fn F036925
    fn try_jobs<M: Mind<Target = T>>(&mut self, m: &mut M, class: u8) -> bool {
        let jobs = self.jobs[class as usize].clone();
        let classified = self.classified[class as usize].clone();
        if jobs.is_empty() && classified.is_empty() {
            return false;
        }
        let mut c = vec![];
        self.rank_list(m, &mut c, &jobs);
        self.rank_list(m, &mut c, &classified);
        self.start_best(m, c)
    }

    /// @fn F036811
    ///
    /// A goal added by its classification: one of the same task already among the classified
    /// lists, on the same target (with no target, for a definition that wants one (+0x48), at
    /// the same place too), takes its priority; otherwise it goes on its classification's list.
    pub fn add_classified(&mut self, t: Task<T>, no_target: &T) {
        for list in self.classified.iter_mut() {
            if let Some(g) = list.iter_mut().find(|g| {
                g.task == t.task && g.target == t.target && (g.target != *no_target || g.def().f48 == 0 || g.pos == t.pos)
            }) {
                g.priority = t.priority;
                return;
            }
        }
        let c = (t.classification as usize).min(4);
        self.classified[c].push(t);
    }

    /// @fn F036801
    ///
    /// The classified goals of a task taken away (each list walked once: of two in a row the
    /// second stays, as the original's index moves on past the one shifted down).
    pub fn remove_classified(&mut self, task: u16) {
        for list in self.classified.iter_mut() {
            let mut k = 0;
            while k < list.len() {
                if list[k].task == task {
                    list.remove(k);
                }
                k += 1;
            }
        }
    }

    /// @fn F036802
    ///
    /// Whether a classified goal of the task is held.
    pub fn has_classified(&self, task: u16) -> bool {
        self.classified.iter().flatten().any(|g| g.task == task)
    }

    /// F036927: think: a goal whose time (+0x280) has run out is dropped (+0x1c0 and its
    /// classification cleared) and planned afresh; while one is held only higher classes may
    /// take over.
    /// @fn F036927
    pub fn think<M: Mind<Target = T>>(&mut self, m: &mut M) -> bool {
        let held = self.current.is_some() && m.now() < self.until;
        if !held {
            self.current = None;
            self.classification = 0;
        }
        self.decide(m, held)
    }
}

/// @fn F039869
///
/// A task is done when its completion conditions all hold and, with a target condition, the
/// target the finder gives meets it; a task with neither is never done.
pub fn is_done<A: Agent>(a: &mut A, d: &TaskDef, t: &Task<A::Target>) -> bool {
    if d.done_when.is_empty() && d.target_condition.is_none() {
        return false;
    }
    if !d
        .done_when
        .iter()
        .all(|&(c, e, _)| holds(a, (c, e), &t.target, t.pos))
    {
        return false;
    }
    match d.target_condition {
        None => true,
        Some(tc) => match a.find_target(d, &t.target, t.pos) {
            Some(found) => holds(a, tc, &found, t.pos),
            None => false,
        },
    }
}

/// @fn F036892
///
/// No target condition, or one not searched for (+0x120): true; else true when the finder
/// finds nothing.
pub fn target_condition_holds<A: Agent>(a: &mut A, d: &TaskDef, t: &Task<A::Target>) -> bool {
    if d.target_condition.is_none() || d.f120 != 0 {
        return true;
    }
    a.find_target(d, &t.target, t.pos).is_none()
}

/// F039871: the definition's score for the goal's target.
/// @fn F039871
pub fn goal_score<A: Agent>(a: &mut A, t: &Task<A::Target>) -> f32 {
    a.score(t.def(), &t.target, t.pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Toy {
        conditions: HashMap<u16, bool>,
        free: bool,
    }
    impl Agent for Toy {
        type Target = u32;
        fn no_target(&self) -> u32 {
            0
        }
        fn check(&mut self, c: u16, _: &u32, _: [f32; 3]) -> Option<bool> {
            self.conditions.get(&c).copied()
        }
        fn score(&mut self, _: &TaskDef, _: &u32, _: [f32; 3]) -> f32 {
            1.0
        }
        fn find_target(&mut self, _: &TaskDef, t: &u32, _: [f32; 3]) -> Option<u32> {
            Some(*t)
        }
    }
    impl Mind for Toy {
        fn hour(&self) -> i32 {
            12
        }
        fn rand01(&mut self) -> f32 {
            0.5
        }
        fn now(&self) -> f64 {
            0.0
        }
        fn free_will(&self) -> bool {
            self.free
        }
    }

    fn goal(task: u16, class: u8) -> Task<u32> {
        Task {
            task,
            classification: class,
            target: 7,
            target2: 0,
            priority: 1.0,
            pos: [0.0; 3],
            hours: (0, 100),
        }
    }

    #[test]
    fn hour_windows_wrap() {
        assert!(in_hours((8, 18), 12) && !in_hours((8, 18), 20));
        assert!(in_hours((20, 6), 23) && in_hours((20, 6), 3) && !in_hours((20, 6), 12));
    }

    #[test]
    fn earlier_goals_rank_first_and_idle_is_the_fallback() {
        // not at the place the move order is for: it is not done yet
        let mut m = Toy {
            conditions: HashMap::from([(1, false)]),
            free: true,
        };
        let mut s = TaskSystem::new(0u32, [0.0; 3]);
        assert!(s.decide(&mut m, false));
        assert_eq!(s.current.as_ref().unwrap().task, IDLE);
        s.goals = vec![goal(29, NON_URGENT), goal(24, NON_URGENT)];
        assert!(s.decide(&mut m, false));
        assert_eq!(s.current.as_ref().unwrap().task, 29);
    }

    #[test]
    fn classified_goals_rank_with_their_jobs() {
        let mut m = Toy {
            conditions: HashMap::from([(1, false)]),
            free: false,
        };
        let mut s = TaskSystem::new(0u32, [0.0; 3]);
        s.add_classified(goal(29, URGENT), &0);
        s.add_classified(goal(29, URGENT), &0);
        assert_eq!(s.classified[URGENT as usize].len(), 1);
        assert!(s.has_classified(29));
        assert!(s.decide(&mut m, false));
        assert_eq!(s.current.as_ref().unwrap().task, 29);
        // two in a row: the second stays (F036801's walk)
        s.classified[URGENT as usize].push(goal(29, URGENT));
        s.remove_classified(29);
        assert_eq!(s.classified[URGENT as usize].len(), 1);
    }

    #[test]
    fn without_free_will_only_urgent_goals() {
        let mut m = Toy {
            conditions: HashMap::new(),
            free: false,
        };
        let mut s = TaskSystem::new(0u32, [0.0; 3]);
        s.goals = vec![goal(29, NON_URGENT)];
        assert!(s.decide(&mut m, false));
        assert_eq!(s.current.as_ref().unwrap().task, IDLE);
    }
}
