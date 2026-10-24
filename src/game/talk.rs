//! Talk-on-arrival (port_map `ai` / `dialogue`): the tasks whose definition carries a dialogue
//! event (TaskDef +0x130): the character walks up to its target and raises the event at it.
//! - The planner: F036914 ranks such a task only while the character's dialogue has lines for
//!   its event (F040846) and makes the event the one isDialogDelivered asks of (+0x268, also
//!   F036920 as it plans); the plan queued makes it the plan's (+0x264, F036921)
//!   (`kenshi_ai::planner::TalkEvents`).
//! - Conditions: isDialogDelivered (109 / 110, 0x140792140 -> F040852): no dialogue, or the
//!   event (+0x268 while deciding, else +0x264) delivered (the Dialogue's +0xf8 map, filled as a
//!   conversation ends, F040866) and no conversation running (its +0x148 set, no partner
//!   +0x2c8). "strangers in house" (108, F039856): Shooing's finder finds someone.
//! - Shooing (158 Const 1, 283 by F038597: 1 while the home is not public, F037699), "Shoo
//!   strangers out of my building": inside its home (107) with no alarm about (130), its finder
//!   (F047581) a stranger in the home; its target condition (110) is solved by Talking (161),
//!   Task_TalktoPlayer on the stranger with EV_SHOO_FROM_MY_BUILDING.
//! - The slavers' (squads whose home object, F044526, holds slaves: its +0x10 list, +0x18 their
//!   number; (assumption) the slaves its squad owns): Working the slaves (188, F038608: 0.25
//!   a slave), EV_SHOUT_AT_SLAVE_WORKER at one of its slaves (F047572) once at it (Talking 159);
//!   Selling slaves (194 F038608, 218 F038546: only in a slave camp or prison town), done once
//!   it has none (155, F038609), EV_SLAVE_DELIVERY at the buyer F047544 finds in its town; the
//!   delivery line's DA_GIVE_TARGET_MY_SLAVES hands them over (crate::dialogue).
//! - Task_TalktoPlayer (factory case 0x2b: Talking 159 / 161; F031276 start, F031277 update):
//!   the event the plan's (+0x264, 1 when none); at the start it is raised at the target (an
//!   intruder is EV_INTRUDER_FOUND instead, F030859), failing which the plan ends (+0x26d);
//!   EV_PLAYER_TALK_TO_ME (1) is done at once (+0x26c). Each update the talker walks to the
//!   target (Character vtable +0x3b0, F039192); EV_SHOO is raised again each update, any other
//!   event ends the plan once the conversation is over (F040828). Past 25 game minutes with the
//!   target running off (F030728) the plan ends with EV_GIVE_UP_CHASE.
use crate::sim::{Sim, Target};
use kenshi_ai::task_def::TaskDef;
use std::collections::{HashMap, HashSet};

/// The AI_TASK enums.
pub const SHOOING: u16 = 158;
pub const SHOOING_PRIVATE: u16 = 283;
pub const WORKING_SLAVES: u16 = 188;
pub const SELLING_SLAVES: u16 = 194;
pub const SELLING_SLAVES_CAMP: u16 = 218;
/// "Talking" (Task_TalktoPlayer): 159 at the target first (condition 1), done when 109; 161
/// done when 110.
pub const TALKING_AT: u16 = 159;
pub const TALKING: u16 = 161;
pub const TALK_RUNNABLE: [u16; 2] = [TALKING_AT, TALKING];
/// The finders (the +0x160 hooks) and score methods (AI vtable slots) ported here.
pub const TALK_FINDERS: [u16; 5] = [SHOOING, SHOOING_PRIVATE, WORKING_SLAVES, SELLING_SLAVES, SELLING_SLAVES_CAMP];
pub const TALK_SCORES: [u8; 3] = [65, 66, 71];
/// F038608: a slave's worth to the slavers' scores (0x141696c80).
const PER_SLAVE: f32 = 0.25;
/// TownType TOWN_SLAVE_CAMP, TOWN_PRISON (F038546's towns).
const TOWN_SLAVE_CAMP: i32 = 5;
const TOWN_PRISON: i32 = 7;
/// BuildingDesignation BD_SLAVE_STORAGE (F047544's query, +0x40).
const BD_SLAVE_STORAGE: i32 = 10;
/// EventTriggerEnum.
pub const EV_PLAYER_TALK_TO_ME: i32 = 1;
pub const EV_GIVE_UP_CHASE: i32 = 60;
pub const EV_SHOUT_AT_SLAVE_WORKER: i32 = 38;
/// Task_TalktoPlayer's chase is given up past 25 game minutes (elapsedHours x 60, 0x1416ae8b0,
/// against 0x1416c0ea0).
const GIVE_UP_MINUTES: f64 = 25.0;
/// The tasks F030728 takes for the target running off: Running away (35), Retreating fast
/// (127), and a Move order (29) to a place farther from the talker than it is.
const RUNNING_AWAY: u16 = 35;
const RETREATING_FAST: u16 = 127;
const MOVE_ORDER: u16 = 29;
/// (stand-in) The walk to the target is ordered again once the target is this far from the
/// point ordered (units): the game orders it every update.
const REPATH: f32 = 50.0;
/// CharacterTypeEnum OT_LAW_ENFORCEMENT / OT_MILITARY: the sensor's knowledge flag 0x80
/// (F046540), which Shooing's finder passes over.
const OT_LAW_ENFORCEMENT: i32 = 1;
const OT_MILITARY: i32 = 2;

/// What the AI's view (crate::sim View) reads of the talk tasks, made once a second for those
/// with a talk task among their goals.
#[derive(Debug, Clone, Default)]
pub struct TalkView {
    /// (person, event): its dialogue has lines for the event (F040846).
    pub has_event: HashSet<(usize, i32)>,
    /// (person, event): isDialogDelivered answers yes (F040852).
    pub delivered: HashSet<(usize, i32)>,
    /// person -> the stranger Shooing's finder (F047581) found in its home.
    pub shoo: HashMap<usize, usize>,
    /// Those whose home is not public (F038597's Building::isPublic, F037699).
    pub home_private: HashSet<usize>,
    /// Each one's home and the building each one is in.
    pub home: HashMap<usize, usize>,
    pub inside: Vec<Option<usize>>,
    /// person -> the slaves its squad owns (F044526's +0x18), those in a slave camp or prison
    /// town (F038546), the slave Working the slaves' finder (F047572) picked and the buyer
    /// Selling slaves' (F047544) found.
    pub slaves: HashMap<usize, usize>,
    pub slave_town: HashSet<usize>,
    pub work_slave: HashMap<usize, usize>,
    pub buyer: HashMap<usize, usize>,
    /// (dialogue, character, faction, event) -> F040846's answer.
    events: HashMap<(Option<String>, Option<String>, Option<String>, i32), bool>,
}

impl TalkView {
    /// The talk tasks' conditions for `me` (`asked`: the event isDialogDelivered asks of).
    pub fn check(&self, me: usize, c: u16, asked: i32) -> Option<bool> {
        Some(match c {
            // F039856: Shooing's finder, from no target, scores above 0
            108 => self.shoo.contains_key(&me),
            // isDialogDelivered (0x140792140)
            109 | 110 => self.delivered.contains(&(me, asked)),
            // iHaveSomeSlaves (F038609): the squad home's slaves (F044526 +0x18) not none
            155 => self.slaves.get(&me).is_some_and(|&n| n > 0),
            _ => return None,
        })
    }

    /// The talk tasks' scores (AI methods by vtable slot).
    pub fn score(&self, me: usize, method: u8) -> f32 {
        match method {
            // F038597 (Shooing, 283): 1 while the home (F044526 +0x38) is not public
            71 => {
                if self.home_private.contains(&me) { 1.0 } else { 0.0 }
            }
            // F038608 (Working the slaves, Selling slaves 194): 0.25 a slave its squad home holds
            65 => self.slaves.get(&me).copied().unwrap_or(0) as f32 * PER_SLAVE,
            // F038546 (Selling slaves 218): in a town (Character +0x88) that is a slave camp or a
            // prison (+0xd8 5 / 7), F038608's (AI vtable +0x208); else none
            66 => {
                if self.slave_town.contains(&me) { self.score(me, 65) } else { 0.0 }
            }
            _ => 0.0,
        }
    }

    /// @fn F047581
    ///
    /// Shooing's finder: none without a home; a character target still inside the home keeps;
    /// else the first the sensor knows as neutral (flag 2; not down 8, law or military 0x80,
    /// caged 0x200 nor 0x10) inside the home, not incapacitated (vtable +0x30, F039191) nor
    /// `me`'s faction's prisoner (F030709); else, for a faction not the player's (+0x250), the
    /// first of the player's characters inside the home, likewise and not caged: one sneaking
    /// (F039177) only when `me`'s sensor knows it (F046525). The search is made once a second
    /// (`talk_view_tick`).
    pub fn find_target(&self, me: usize, task: u16, t: &Target) -> Option<Target> {
        match task {
            SHOOING | SHOOING_PRIVATE => {
                let home = *self.home.get(&me)?;
                if let Target::Person(x) = t
                    && self.inside.get(*x).copied().flatten() == Some(home)
                {
                    return Some(*t);
                }
                self.shoo.get(&me).map(|&x| Target::Person(x))
            }
            // F047572: a target keeps; else one of its squad's slaves (`work_slave`)
            WORKING_SLAVES => match t {
                Target::None => self.work_slave.get(&me).map(|&x| Target::Person(x)),
                _ => Some(*t),
            },
            // F047544: a target keeps; else the buyer in its town (`buyer`)
            SELLING_SLAVES | SELLING_SLAVES_CAMP => match t {
                Target::None => self.buyer.get(&me).map(|&x| Target::Person(x)),
                _ => Some(*t),
            },
            _ => None,
        }
    }
}

/// Task_TalktoPlayer's own state (+0x78 its start, +0x80 its event) and the point its walk was
/// last ordered to.
#[derive(Debug, Clone)]
pub struct Talk {
    pub since: f64,
    pub event: i32,
    pub ordered: Option<[f32; 3]>,
}

/// The talk tasks running, by talker.
#[derive(Debug, Clone, Default)]
pub struct TalkState {
    pub running: HashMap<usize, Talk>,
}

/// The dialogue events of the talk tasks among `p`'s goals, orders and jobs.
fn talk_events_of(p: &crate::sim::Person) -> Vec<i32> {
    let mut v: Vec<i32> = Vec::new();
    let ai = &p.ai;
    for t in ai.goals.iter().chain(ai.orders.iter()).chain(ai.jobs.iter().flatten()).chain(ai.classified.iter().flatten()) {
        let e = kenshi_ai::task_def::def(t.task).map_or(0, |d: &TaskDef| d.f130);
        if e != 0 && !v.contains(&e) {
            v.push(e);
        }
    }
    v
}

impl Sim {
    /// Every tick the talk tasks run (`talk_tasks_tick`); once a second what the planner reads
    /// of them is made (`talk_view_tick`).
    pub fn talk_tick(&mut self, dt: f32) {
        if self.tick.is_multiple_of(60) {
            self.talk_view_tick();
        }
        self.talk_tasks_tick(dt);
    }

    /// Whether `npc` is in a conversation: its dialogue not free (F040828: +0x148, +0x228,
    /// +0x2c8, crate::conversation), or in the player's window (F040918: a conversation
    /// waiting for the player).
    pub fn in_conversation(&self, npc: usize) -> bool {
        !self.run_free(npc) || self.npc_talks.iter().any(|(c, _)| c.npc == npc && !c.choices.is_empty())
    }

    /// @fn F030709
    ///
    /// Whether `other` is `me`'s faction's prisoner: held (+0x330 / +0x320: (assumption) a
    /// slave in its shackles, state 1) and owned by `me`'s faction (its squad's +0xe0).
    fn prisoner_of(&self, me: usize, other: usize) -> bool {
        self.slave_state(other) == crate::slavery::IS_SLAVE && self.folk[other].slave.as_ref().is_some_and(|s| Some(&s.faction) == self.folk[me].faction.as_ref())
    }

    /// F039191 (Character vtable +0x30): dead, down, (not ported) its +0x3d4 and a medical state
    /// past 2.
    fn incapacitated(&self, id: usize) -> bool {
        self.folk[id].dead || self.folk[id].down
    }

    fn caged(&self, id: usize) -> bool {
        self.folk[id].prison.as_ref().is_some_and(|pr| !pr.out)
    }

    /// F047581's search for `me` (see `TalkView::find_target`).
    fn shoo_target(&self, me: usize) -> Option<usize> {
        let home = self.folk[me].home?;
        let inside = |c: usize| self.folk[c].inside == Some(home);
        // the sensor's entries flagged neutral and none of 0x298 (F046505 with 2, 0x298;
        // F046540's flags: neutral neither an enemy nor a friend; (not ported) the enemies it
        // still flags neutral, F047fb4, and flag 0x10)
        for &(c, _, _) in &self.folk[me].sensed {
            if c == me || c >= self.folk.len() || self.folk[c].away {
                continue;
            }
            let class = self.npc_class(c);
            let neutral = !self.takes_for_enemy(me, c, true) && !self.takes_for_friend(me, c, true);
            if !neutral || self.folk[c].down || self.folk[c].dead || class == OT_LAW_ENFORCEMENT || class == OT_MILITARY || self.caged(c) {
                continue;
            }
            if inside(c) && !self.incapacitated(c) && !self.prisoner_of(me, c) {
                return Some(c);
            }
        }
        if self.folk[me].faction.as_deref() == Some(crate::sim::PLAYER_FACTION) {
            return None;
        }
        (0..self.folk.len()).find(|&c| {
            let q = &self.folk[c];
            if !q.player || q.away || !inside(c) || self.incapacitated(c) || self.prisoner_of(me, c) || self.caged(c) {
                return false;
            }
            !q.sneaking || self.folk[me].sensed.iter().any(|e| e.0 == c) || self.talking_to(me) == Some(c)
        })
    }

    /// The slaves `me`'s squad home holds (F044526's +0x10 list): (assumption) the living
    /// whose slavery's owner is its squad.
    fn squad_slaves(&self, me: usize) -> Vec<usize> {
        let Some(pl) = self.folk[me].platoon else { return Vec::new() };
        self.folk.iter().filter(|q| !q.dead && !q.away && q.slave.as_ref().is_some_and(|s| s.owner == Some(pl))).map(|q| q.id).collect()
    }

    /// @fn F047572
    ///
    /// Working the slaves' finder: of its squad's slaves, those held (+0x330: (assumption) in
    /// their shackles, state 1), not busy (F00d5f8: (assumption) down or in a conversation),
    /// not carried (vtable +0x248), in no bed or cage (+0x2f8 0) and reserved by no one else
    /// (F052a3b: (assumption) no other talk task on it); one at random (F0336b8) when more
    /// than one. (not ported) Its +0x3d4 test.
    fn slave_to_work(&mut self, me: usize, owned: &[usize]) -> Option<usize> {
        let others: HashSet<usize> = self
            .talk_state
            .running
            .keys()
            .filter(|&&k| k != me)
            .filter_map(|&k| match self.folk[k].ai.queue.first().map(|s| s.target) {
                Some(Target::Person(x)) => Some(x),
                _ => None,
            })
            .collect();
        let c: Vec<usize> = owned
            .iter()
            .copied()
            .filter(|&s| {
                let q = &self.folk[s];
                self.slave_state(s) == crate::slavery::IS_SLAVE && !q.down && !self.in_conversation(s) && q.carried_by.is_none() && q.bed.is_none() && !self.caged(s) && !others.contains(&s)
            })
            .collect();
        match c.len() {
            0 => None,
            1 => Some(c[0]),
            n => {
                let k = ((self.rng.uniform() * n as f32) as usize).min(n - 1);
                Some(c[k])
            }
        }
    }

    /// @fn F047544
    /// @fn F047538
    /// @fn F044565
    /// @fn F044487
    ///
    /// Selling slaves' finder (BuildingFinderNearestAllies_Designated): a target held is kept;
    /// else of the buildings of the town `me` is in (Character +0x88; in none, those of the nine
    /// zones about it, F051406) with an interior (the query's mask 0x80000:
    /// BF_SHELL_WITH_INTERIOR) and the designation BD_SLAVE_STORAGE (10, +0xc4: its residents'
    /// "building designation", F048905), owned by `me`'s faction or by one its squad home may
    /// use (F047538 -> F03bd0e: an ally's), one at random (F051388); its owner (F044565): the
    /// leader of the squad living there (F044487: its leader while up and free, else the first
    /// of it up and free).
    fn slave_buyer(&mut self, me: usize, kept: Option<usize>) -> Option<usize> {
        let free = |q: &crate::sim::Person| !q.dead && !q.down && !q.away && q.prison.is_none() && q.bed.is_none() && q.slave.is_none();
        if let Some(k) = kept
            && self.folk.get(k).is_some_and(free)
        {
            return Some(k);
        }
        let mine = self.faction_id(me)?;
        let town = self.folk[me].town;
        let at = self.folk[me].mover.position_units();
        let zone = kenshi_world::zone_of(at[0], at[2]);
        let shells: std::collections::HashSet<usize> = self.inner.iter().map(|v| v.placement).collect();
        let candidates: Vec<usize> = (0..self.placed.len())
            .filter(|&b| shells.contains(&b))
            .filter(|&b| match town {
                Some(t) => self.placed_town.get(b).copied().flatten() == Some(t),
                None => {
                    let q = self.placed_at[b];
                    let z = kenshi_world::zone_of(q[0], q[2]);
                    (z.0 - zone.0).abs() <= 1 && (z.1 - zone.1).abs() <= 1
                }
            })
            .filter(|&b| {
                let p = &self.placed[b];
                [&p.residents, &p.squad].into_iter().flatten().any(|t| self.data.get(t).and_then(|r| r.ints.get("building designation")) == Some(&BD_SLAVE_STORAGE))
            })
            .filter(|&b| {
                let owner = self.placed_town.get(b).copied().flatten().and_then(|t| self.towns[t].faction.clone());
                owner.as_deref() == Some(mine.as_str()) || self.relations.get(&mine).is_some_and(|r| r.is_ally(owner.as_deref()))
            })
            .filter(|&b| self.folk.iter().any(|q| q.home == Some(b) && free(q)))
            .collect();
        if std::env::var("KENSHI_TALK_DEBUG").is_ok() && candidates.is_empty() {
            let dbg: Vec<(usize, bool, Option<usize>, Option<String>)> = (0..self.placed.len())
                .filter(|&b| {
                    let p = &self.placed[b];
                    [&p.residents, &p.squad].into_iter().flatten().any(|t| self.data.get(t).and_then(|r| r.ints.get("building designation")) == Some(&BD_SLAVE_STORAGE))
                })
                .map(|b| (b, shells.contains(&b), self.placed_town.get(b).copied().flatten(), self.folk.iter().find(|q| q.home == Some(b)).map(|q| format!("{} free {}", q.id, free(q)))))
                .collect();
            eprintln!("talk: {me} (town {town:?}) finds no slave buyer; slave storages {dbg:?}");
        }
        let b = match candidates.len() {
            0 => return None,
            1 => candidates[0],
            n => candidates[((self.rng.uniform() * n as f32) as usize).min(n - 1)],
        };
        let living: Vec<usize> = self.folk.iter().filter(|q| q.home == Some(b) && free(q)).map(|q| q.id).collect();
        living.iter().copied().find(|&q| self.folk[q].role == 5).or(living.first().copied())
    }

    /// Once a second, for each one with a talk task among its goals: the events its dialogue
    /// has lines for (F040846, `has_event`), those delivered (F040852), its home's being private
    /// and Shooing's stranger.
    pub fn talk_view_tick(&mut self) {
        let mut v = std::mem::take(&mut self.talk_view);
        v.has_event.clear();
        v.delivered.clear();
        v.shoo.clear();
        v.home_private.clear();
        v.home.clear();
        v.slaves.clear();
        v.slave_town.clear();
        v.work_slave.clear();
        // (F047544 keeps a target it holds)
        let kept_buyers = std::mem::take(&mut v.buyer);
        v.inside = self.folk.iter().map(|p| p.inside).collect();
        for me in 0..self.folk.len() {
            let p = &self.folk[me];
            if p.player || p.dead || p.away {
                continue;
            }
            let events = talk_events_of(p);
            if events.is_empty() {
                continue;
            }
            let speaking = self.in_conversation(me);
            for &e in &events {
                let key = (p.dialogue.clone(), p.character.clone(), p.faction.clone(), e);
                let has = match v.events.get(&key) {
                    Some(&h) => h,
                    None => {
                        let h = self.has_event(me, e);
                        v.events.insert(key, h);
                        h
                    }
                };
                if has {
                    v.has_event.insert((me, e));
                }
            }
            // every event delivered, as +0x264 may hold any
            if !speaking {
                for &(n, e) in self.dialog_memory.delivered.iter().filter(|(n, _)| *n == me) {
                    v.delivered.insert((n, e));
                }
            }
            // the slavers': their squad's slaves, the town they are in, the slave to work and
            // the buyer
            let slaving = events.iter().any(|&e| e == EV_SHOUT_AT_SLAVE_WORKER || e == crate::raid::EV_SLAVE_DELIVERY);
            if slaving {
                let owned = self.squad_slaves(me);
                v.slaves.insert(me, owned.len());
                if self.folk[me].town.is_some_and(|t| matches!(self.towns[t].kind, TOWN_SLAVE_CAMP | TOWN_PRISON)) {
                    v.slave_town.insert(me);
                }
                if events.contains(&EV_SHOUT_AT_SLAVE_WORKER)
                    && let Some(x) = self.slave_to_work(me, &owned)
                {
                    v.work_slave.insert(me, x);
                }
                if events.contains(&crate::raid::EV_SLAVE_DELIVERY)
                    && let Some(x) = self.slave_buyer(me, kept_buyers.get(&me).copied())
                {
                    v.buyer.insert(me, x);
                }
            }
            let p = &self.folk[me];
            if let Some(h) = p.home {
                v.home.insert(me, h);
                if !self.building_public(h) {
                    v.home_private.insert(me);
                }
                if events.contains(&crate::intruders::EV_SHOO_FROM_MY_BUILDING)
                    && let Some(x) = self.shoo_target(me)
                {
                    v.shoo.insert(me, x);
                }
            }
        }
        self.talk_view = v;
    }

    /// Task_TalktoPlayer raises `event` at `target` (F040920, `Sim::trigger_event`: the
    /// conversation played out line by line, crate::conversation; so the delivery lines'
    /// DA_GIVE_TARGET_MY_SLAVES hands the slaves over as they are said). Whether it spoke.
    fn raise(&mut self, who: usize, target: usize, event: i32) -> bool {
        if who >= self.folk.len() || target >= self.folk.len() || self.folk[who].away {
            return false;
        }
        self.trigger_event(who, Some(target), event)
    }

    /// @fn F030859
    ///
    /// The event Task_TalktoPlayer raises: EV_SHOO_FROM_MY_BUILDING becomes EV_INTRUDER_FOUND
    /// at one `me` takes for an enemy (vtable +1000, F044572) or, while its squad has locked
    /// its home up (+0xe8, F044498), at one it takes for no friend (vtable +0x3f0, F044470).
    fn raised_event(&self, me: usize, target: usize, event: i32) -> i32 {
        use crate::intruders::{EV_INTRUDER_FOUND, EV_SHOO_FROM_MY_BUILDING};
        if event == EV_SHOO_FROM_MY_BUILDING {
            if self.takes_for_enemy(me, target, true) {
                return EV_INTRUDER_FOUND;
            }
            if self.folk[me].home.is_some_and(|h| self.locked_up(h)) && !self.takes_for_friend(me, target, true) {
                return EV_INTRUDER_FOUND;
            }
        }
        event
    }

    /// @fn F030728
    ///
    /// Whether `target` runs off from `me`: its goal Running away (35) or Retreating fast
    /// (127), or a Move order (29) to a place farther from `me` than it is.
    /// (assumption) Its task (+0x648 +0x68) taken as its current goal.
    fn running_off(&self, me: usize, target: usize) -> bool {
        let Some(c) = self.folk[target].ai.current.as_ref() else { return false };
        match c.task {
            RUNNING_AWAY | RETREATING_FAST => true,
            MOVE_ORDER if c.pos != [0.0; 3] => {
                let (a, b) = (self.folk[me].mover.position_units(), self.folk[target].mover.position_units());
                let d2 = |p: [f32; 3], q: [f32; 3]| (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
                d2(b, a) < d2(c.pos, a)
            }
            _ => false,
        }
    }

    /// @fn F031276
    /// @fn F031277
    /// @fn F040828
    ///
    /// Task_TalktoPlayer for whoever has a Talking step in hand (see the module's notes). Its
    /// start (F031276): the event the plan's (+0x264, 1 when none) raised at the target
    /// (F030859's), failing which the plan ends (+0x26d, F036890); EV_PLAYER_TALK_TO_ME is
    /// done at once (+0x26c, F036886). Its update (F031277): with no target the plan ends;
    /// past 25 game minutes with the target running off (F030728) the plan is dropped
    /// (F036803) with EV_GIVE_UP_CHASE; the talker walks to the target (F039192); then
    /// EV_SHOO is raised again, or another event's plan ends (F036803, F036886) once the
    /// conversation is over (F040828, `in_conversation`).
    /// (not ported) The start's AI vtable +0x98 call (param_2[0xb]) and the give-up's +0x28.
    pub fn talk_tasks_tick(&mut self, _dt: f32) {
        let ids: Vec<usize> = self.folk.iter().filter(|p| !p.dead && !p.down && !p.away && p.ai.queue.first().is_some_and(|s| TALK_RUNNABLE.contains(&s.task))).map(|p| p.id).collect();
        self.talk_state.running.retain(|id, _| ids.contains(id));
        for me in ids {
            let Some(step) = self.folk[me].ai.queue.first().cloned() else { continue };
            // the plan ended (+0x26d, F036890 / F036910 -> F036902; or dropped, F036803): the
            // next decision (`Person::think`, once a second) plans afresh
            let end = |sim: &mut Sim| {
                let p = &mut sim.folk[me];
                p.ai.queue.clear();
                p.started = false;
                sim.talk_state.running.remove(&me);
            };
            let x = match step.target {
                Target::Person(x) if x < self.folk.len() => x,
                // a target of no kind (0xb): the plan ends (F036890)
                Target::None => {
                    end(self);
                    continue;
                }
                // (a target not a character: nothing to talk to)
                _ => continue,
            };
            let now = self.hours_gone();
            // a new plan (crate::sim clears `started`) starts the task afresh
            if !self.folk[me].started || !self.talk_state.running.contains_key(&me) {
                let plan = self.folk[me].ai.talk_event;
                let event = if plan == 0 { EV_PLAYER_TALK_TO_ME } else { plan };
                self.talk_state.running.insert(me, Talk { since: now, event, ordered: None });
                self.folk[me].started = true;
                let e = self.raised_event(me, x, event);
                let spoke = self.raise(me, x, e);
                if std::env::var("KENSHI_TALK_DEBUG").is_ok() {
                    eprintln!("talk: {me} starts Talking ({}) to {x}: event {event} -> {e}, spoke {spoke} (t {:.2}, delivered {} / view {}, plan event {})", step.task, self.t, self.dialog_memory.delivered.contains(&(me, event)), self.talk_view.delivered.contains(&(me, event)), self.folk[me].ai.talk_event);
                }
                if !spoke {
                    end(self);
                } else if event == EV_PLAYER_TALK_TO_ME {
                    let p = &mut self.folk[me];
                    p.ai.queue.remove(0);
                    p.started = false;
                    self.talk_state.running.remove(&me);
                }
                continue;
            }
            let talk = self.talk_state.running[&me].clone();
            if (now - talk.since) * 60.0 > GIVE_UP_MINUTES && self.running_off(me, x) {
                end(self);
                self.raise(me, x, EV_GIVE_UP_CHASE);
                continue;
            }
            // the walk to the target
            let goal = self.folk[x].mover.position_units();
            let moved = talk.ordered.is_none_or(|o| (o[0] - goal[0]).hypot(o[2] - goal[2]) > REPATH);
            if moved && !self.folk[me].pathing.pending {
                let Sim { folk, manager, .. } = self;
                let p = &mut folk[me];
                p.pathing.go_to(p.id, &p.mover, goal, false, manager);
                if let Some(t) = self.talk_state.running.get_mut(&me) {
                    t.ordered = Some(goal);
                }
            }
            if talk.event == crate::intruders::EV_SHOO_FROM_MY_BUILDING {
                let e = self.raised_event(me, x, talk.event);
                let spoke = self.raise(me, x, e);
                if spoke && std::env::var("KENSHI_TALK_DEBUG").is_ok() {
                    eprintln!("talk: {me} says event {e} to {x} again");
                }
            } else if !self.in_conversation(me) {
                end(self);
            }
        }
    }
}
