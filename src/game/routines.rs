//! The player's people on their own (port_map `ai`): the Medic and Rescue orders (the main
//! panel's OrdersMedicButton / OrdersRescueButton: "heal all wounded squad members within
//! range"; "find nearby unconscious allies and automatically carry them to a bed if there are
//! any available in the vicinity", the game's tutorials) and the squad's AI options (F035130):
//! - Heal Allies / Rescue Allies: the medic and rescue jobs take in allied factions' people too.
//! - Help Allies: the player's people's Protect allies job (`sim`, F050285) counts another
//!   squad's people only with it.
//! - Sleep when injured, Sit when idle, Ditch items: the built-in goals the goal ranking adds
//!   (F036915), planned (crate::jobs).
//!
//! The Medic order is the Medic job on the planner (crate::law's Medic goal). (stand-in) Kenshi
//! runs the rescue job through its AI as a player task (Find and rescue, 148); here it is a
//! direct errand with the reaches below assumed.
//! "Feed animals" is the hunger step's (`sim`).
use crate::sim::Sim;
use kenshi_world::furniture::BF_BED;

/// A player character's standing job from the orders panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Rescue,
}

/// (assumption) How far a medic or rescuer looks for patients, and how far a bed may be.
const JOB_RANGE: f32 = 1500.0;
const BED_RANGE: f32 = 3000.0;

impl Sim {
    /// The Medic order: each of `ids` given the Medic job (58, F036821; an urgent goal, so taken
    /// with JOBS off too), its JOBS turned on: planned (crate::law's Medic finder, F050304 ->
    /// First aid, 25) as the NPC medics' goal is.
    pub fn order_medic(&mut self, ids: &[usize]) {
        for &id in ids {
            if self.folk.get(id).is_some_and(|p| p.player && !p.down) && !self.folk[id].jobs.iter().any(|j| j.task == crate::law::MEDIC) {
                let label = self.job_label(crate::law::MEDIC, id);
                self.folk[id].jobs.push(crate::orders::Job { task: crate::law::MEDIC, target: id, label });
                self.folk[id].jobs_on = true;
            }
        }
    }

    /// The Rescue order: each of `ids` carries the unconscious about it to beds.
    pub fn order_rescue(&mut self, ids: &[usize]) {
        for &id in ids {
            if self.folk.get(id).is_some_and(|p| p.player && !p.down) {
                self.folk[id].job = Some(Job::Rescue);
            }
        }
    }

    /// Whether `who` is one the player's people look after: one of theirs, or (with the option)
    /// of an allied faction.
    fn looked_after(&self, who: usize, allies: bool) -> bool {
        let q = &self.folk[who];
        if q.player {
            return true;
        }
        allies && q.faction.as_ref().is_some_and(|f| self.relations.get(f).is_some_and(|r| r.is_ally(Some(crate::sim::PLAYER_FACTION))))
    }

    fn apart(&self, a: usize, b: usize) -> f32 {
        let (p, q) = (self.folk[a].mover.position_units(), self.folk[b].mover.position_units());
        (p[0] - q[0]).hypot(p[2] - q[2])
    }

    /// A free bed (furniture) nearest `at` within `range`.
    fn free_bed_near(&self, at: [f32; 3], range: f32) -> Option<usize> {
        (0..self.furn.len())
            .filter(|&k| self.furn[k].function == BF_BED && self.bed_free(k))
            .map(|k| (k, (self.furn[k].position[0] - at[0]).hypot(self.furn[k].position[2] - at[2])))
            .filter(|(_, d)| *d < range)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    }

    /// Every second: the player's people's rescue job.
    pub fn routines_tick(&mut self, dt: f32) {
        self.routine_timer += dt;
        if self.routine_timer < 1.0 {
            return;
        }
        self.routine_timer = 0.0;
        let opts = self.ai_options.clone();
        let players: Vec<usize> = self.folk.iter().filter(|p| p.player && !p.dead && !p.down && !p.away && p.prison.is_none() && p.carried_by.is_none()).map(|p| p.id).collect();
        for id in players {
            match self.folk[id].job {
                Some(Job::Rescue) => {
                    if self.folk[id].carrying.is_some() {
                        if !self.bed_errands.iter().any(|e| e.id == id) {
                            let at = self.folk[id].mover.position_units();
                            match self.free_bed_near(at, BED_RANGE) {
                                Some(b) => self.order_put_in_bed(&[id], b),
                                None => {
                                    self.order_drop(&[id]);
                                    self.folk[id].job = None;
                                }
                            }
                        }
                        continue;
                    }
                    if self.folk[id].pickup.is_some() {
                        continue;
                    }
                    let at = self.folk[id].mover.position_units();
                    let downed = (0..self.folk.len())
                        .filter(|&k| k != id && self.folk[k].down && !self.folk[k].dead && self.folk[k].bed.is_none() && self.folk[k].carried_by.is_none() && self.folk[k].prison.is_none())
                        .filter(|&k| self.looked_after(k, opts.rescue_allies) && self.apart(id, k) < JOB_RANGE)
                        .min_by(|&a, &b| self.apart(id, a).total_cmp(&self.apart(id, b)));
                    match downed {
                        Some(k) if self.free_bed_near(at, BED_RANGE).is_some() => self.order_pickup(&[id], k),
                        _ => self.folk[id].job = None,
                    }
                    continue;
                }
                None => {}
            }
        }
    }
}
