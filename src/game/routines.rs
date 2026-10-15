//! The player's people on their own (port_map `ai`): the Medic and Rescue orders (the main
//! panel's OrdersMedicButton / OrdersRescueButton: "heal all wounded squad members within
//! range"; "find nearby unconscious allies and automatically carry them to a bed if there are
//! any available in the vicinity", the game's tutorials) and the squad's AI options (F035130):
//! - Heal Allies / Rescue Allies: the medic and rescue jobs take in allied factions' people too.
//! - Help Allies: the player's people's Protect allies job (`sim`, F050285) counts another
//!   squad's people only with it.
//! - Sleep when injured: an idle injured character at home goes to a free bed, and gets up once
//!   healed.
//! - Sit when idle: an idle character at home sits on a free chair or bench.
//!
//! (stand-in) Kenshi runs these through its AI: the jobs as player tasks, the idle ones as the
//! built-in goals its goal ranking adds when the options allow (F036915: tasks 242 ditching
//! resources, 153 / 257 going to and getting out of bed, 275 sitting around), whose conditions
//! are not ported. Here they are direct errands with the reaches and waits below assumed.
//! - Ditch items: "If they have nothing else to do, characters will automatically put any
//!   excess materials they are carrying into storage": an idle character at home carrying
//!   what one of the player's resource storages keeps ("limit inventory") takes it there
//!   (task 242, Ditching resources). (assumption) The excess is all of it; storages without a
//!   limit are not ditched into.
//! "Feed animals" is the hunger step's (`sim`).
use crate::sim::Sim;
use kenshi_world::furniture::{BF_BED, BF_CHAIR, BF_THRONE};

/// A player character's standing job from the orders panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Medic,
    Rescue,
}

/// (assumption) How far a medic or rescuer looks for patients, and how far a bed may be.
const JOB_RANGE: f32 = 1500.0;
const BED_RANGE: f32 = 3000.0;
/// (assumption) How near the player's buildings counts as home, and how long a character
/// stands idle before sitting.
const HOME_RANGE: f32 = 2500.0;
const SIT_AFTER: f32 = 10.0;

impl Sim {
    /// The Medic order: each of `ids` treats the wounded about it (`routines_tick`).
    pub fn order_medic(&mut self, ids: &[usize]) {
        for &id in ids {
            if self.folk.get(id).is_some_and(|p| p.player && !p.down) {
                self.folk[id].job = Some(Job::Medic);
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

    /// Ditch items: the nearest built storage of the player's that keeps ("limit inventory")
    /// something `id` carries (its main grid or worn backpack), and those records.
    pub(crate) fn ditch_target(&self, id: usize) -> Option<(usize, Vec<String>)> {
        let inv = &self.folk[id].inventory;
        let carried: Vec<String> = inv
            .sections
            .iter()
            .filter(|s| !s.is_equipment())
            .chain(inv.bag().into_iter().flat_map(|b| b.sections.iter()))
            .flat_map(|s| s.items.iter().map(|p| p.item.record.clone()))
            .collect();
        if carried.is_empty() {
            return None;
        }
        let at = self.folk[id].mover.position_units();
        (0..self.buildings.len())
            .filter(|&k| self.buildings[k].done && self.buildings[k].storage.is_some())
            .filter_map(|k| {
                let keeps: Vec<String> = self.data.get(&self.buildings[k].record)?.refs("limit inventory").iter().map(|(s, _)| s.clone()).collect();
                let items: Vec<String> = carried.iter().filter(|c| keeps.contains(c)).cloned().collect();
                (!items.is_empty()).then_some((k, items))
            })
            .min_by(|a, b| {
                let d = |k: usize| (self.buildings[k].at[0] - at[0]).hypot(self.buildings[k].at[2] - at[2]);
                d(a.0).total_cmp(&d(b.0))
            })
    }

    fn at_home(&self, at: [f32; 3]) -> bool {
        self.buildings.iter().any(|b| b.confirmed && (b.at[0] - at[0]).hypot(b.at[2] - at[2]) < HOME_RANGE)
    }

    /// Whether `id` stands about with nothing to do.
    fn is_idle(&self, id: usize) -> bool {
        let p = &self.folk[id];
        p.ai.orders.is_empty()
            && p.first_aid.is_none()
            && p.carrying.is_none()
            && p.bed.is_none()
            && p.operating.is_none()
            && p.fetch.is_none()
            && p.pickup.is_none()
            && !self.arena.f[id].active
            && !self.bed_errands.iter().any(|e| e.id == id)
    }

    /// Every second: the player's people's jobs and idle habits.
    pub fn routines_tick(&mut self, dt: f32) {
        self.routine_timer += dt;
        if self.routine_timer < 1.0 {
            return;
        }
        let step = std::mem::take(&mut self.routine_timer);
        let opts = self.ai_options.clone();
        let players: Vec<usize> = self.folk.iter().filter(|p| p.player && !p.dead && !p.down && !p.away && p.prison.is_none() && p.carried_by.is_none()).map(|p| p.id).collect();
        for id in players {
            match self.folk[id].job {
                Some(Job::Medic) => {
                    if self.folk[id].first_aid.is_some() {
                        continue;
                    }
                    let patient = (0..self.folk.len())
                        .filter(|&k| !self.folk[k].dead && self.folk[k].race.is_none() && self.looked_after(k, opts.heal_allies) && self.apart(id, k) < JOB_RANGE)
                        .map(|k| (k, self.arena.f[k].medical.aid_need(false).flesh))
                        .filter(|(_, n)| *n > 0.0)
                        .max_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(k, _)| k);
                    match patient {
                        Some(k) if self.folk[id].inventory.sections.iter().any(|s| s.items.iter().any(|i| i.item.medkit.is_some_and(|m| 0.0 < m.charges))) => self.order_first_aid(&[id], k),
                        Some(_) => {
                            self.messages.push(format!("{} has no medical kit", self.display_name(id)));
                            self.folk[id].job = None;
                        }
                        None => self.folk[id].job = None,
                    }
                    continue;
                }
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
            let idle = self.is_idle(id);
            self.folk[id].idle_for = if idle { self.folk[id].idle_for + step } else { 0.0 };
            let at = self.folk[id].mover.position_units();
            // Sleep when injured: home, hurt, to bed; up again once healed
            if opts.sleep_injured {
                let need = self.arena.f[id].medical.aid_need(false).flesh;
                if self.folk[id].rest_injured && self.folk[id].bed.is_some() && need <= 0.0 && self.arena.f[id].medical.parts.iter().all(|q| q.health() >= q.max_health() - 0.5) {
                    self.folk[id].rest_injured = false;
                    self.get_out_of_bed(id);
                    continue;
                }
                if idle && need > 0.0 && self.at_home(at)
                    && let Some(b) = self.free_bed_near(at, HOME_RANGE)
                {
                    self.order_sleep(&[id], b);
                    self.folk[id].rest_injured = true;
                    continue;
                }
            }
            // Ditch items: what a resource storage keeps, into it
            if opts.ditch_items && idle && self.folk[id].haul.is_none() && self.at_home(at)
                && let Some((to, items)) = self.ditch_target(id)
            {
                let dest = self.buildings[to].at;
                self.order_move(&[id], dest);
                self.folk[id].haul = Some(crate::production::Haul { from: crate::production::DITCH, to, items });
                continue;
            }
            // Sit when idle: home, a while idle, a free chair
            if opts.sit_idle && idle && self.folk[id].idle_for > SIT_AFTER && self.at_home(at) {
                let taken: Vec<usize> = self.folk.iter().filter_map(|p| p.operating).collect();
                let chair = (0..self.furn.len())
                    .filter(|&k| matches!(self.furn[k].function, BF_CHAIR | BF_THRONE) && !taken.contains(&k) && self.operators.get(k).is_none_or(|o| o.is_none()))
                    .map(|k| (k, (self.furn[k].position[0] - at[0]).hypot(self.furn[k].position[2] - at[2])))
                    .filter(|(_, d)| *d < HOME_RANGE)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((k, _)) = chair {
                    let p = self.furn[k].position;
                    self.order_operate_furniture(id, k, p);
                    self.folk[id].idle_for = 0.0;
                }
            }
        }
    }
}
