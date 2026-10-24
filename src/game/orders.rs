//! The player's right-click orders (the menu F044603 offers, labelled by F044568) that need
//! their own errand: sleeping and putting someone in a bed, splinting, the shackles (picked,
//! cut, broken), cages (putting someone in, getting in), attacking a whole squad, hauling a
//! machine's output, throwing someone out, and disposing of a corpse. (Picking up, kidnapping,
//! first aid, training and the rest live with their systems.)

use crate::sim::{BF_BED, PLAYER_FACTION, Sim};
use kenshi_char::StatsEnumerated;
use kenshi_char::movement::stop;

/// A bed errand: walking to bed `bed` to sleep in it, or (`put`) to put the one carried in it;
/// `age` the seconds since the order (a moment to set off). Once in the bed the sleeper stays
/// until given another order.
#[derive(Debug, Clone)]
pub struct BedErrand {
    pub id: usize,
    pub bed: usize,
    pub put: bool,
    pub lying: bool,
    pub age: f32,
}

/// Work on someone's shackles (Task_PickLock_Shackles, Task_CutShackles,
/// Task_BruteForceShackles): who works, whose shackles, how, and how far it has gone.
#[derive(Debug, Clone)]
pub struct ShackleWork {
    pub worker: usize,
    pub target: usize,
    pub how: ShackleHow,
    /// Picking: the time to the next attempt and its progress; cutting: the hardness left.
    pub progress: f32,
    pub next: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShackleHow {
    /// "Unlock Shackles": picked, attempt by attempt (lockpicking).
    Pick,
    /// "Use Tools": cut through with a tool (a Hacksaw: "item function" 9), used up at the end.
    Cut,
    /// "Use Strength": broken at once if strong enough.
    Strength,
}

/// A cage errand: carrying someone to cage `cage` to lock them in ("Put in"), or getting in it
/// oneself ("Get in").
#[derive(Debug, Clone)]
pub struct CageErrand {
    pub id: usize,
    pub cage: usize,
    pub get_in: bool,
    pub age: f32,
}

/// A splint errand: `medic` going to splint `patient`'s broken limbs.
#[derive(Debug, Clone)]
pub struct SplintErrand {
    pub medic: usize,
    pub patient: usize,
    pub work: f32,
}

/// The item function of tools (the Hacksaw) and of splint kits.
pub const ITEM_TOOLS: i32 = 9;
pub const ITEM_SPLINT: i32 = 2;
/// How near a worker must be to someone it works on (units).
const REACH: f32 = 25.0;

impl Sim {
    fn dist(&self, a: usize, b: [f32; 3]) -> f32 {
        let p = self.folk[a].mover.position_units();
        (p[0] - b[0]).hypot(p[2] - b[2])
    }

    fn still(&self, id: usize) -> bool {
        let v = self.folk[id].mover.velocity;
        v[0].hypot(v[2]) < 0.5
    }

    /// @fn F044547
    ///
    /// Whether taking up someone unconscious is a kidnapping ("Kidnap", order 0xf6) rather than
    /// "Pick Up" (0xe1): it is, unless they are of the leader's faction, an ally of it, wanted
    /// (a bounty), a prisoner, or dead.
    pub fn is_kidnap(&self, leader: usize, target: usize) -> bool {
        let (l, t) = (&self.folk[leader], &self.folk[target]);
        if t.dead || t.prison.is_some() || !t.bounties.is_empty() || t.player {
            return false;
        }
        if l.faction.is_some() && l.faction == t.faction {
            return false;
        }
        let ally = t.faction.as_deref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_ally(l.faction.as_deref()));
        !ally
    }

    // ---- beds -------------------------------------------------------------------------------

    /// Whether bed `b` (furniture) is free.
    pub fn bed_free(&self, b: usize) -> bool {
        self.bed_taken.get(b).is_some_and(|t| t.is_none()) && !self.bed_errands.iter().any(|e| e.bed == b && e.lying)
    }

    /// "Sleep" (order 0x102, a bed's own order): the first of `ids` free walks to bed `b` and
    /// lies in it until given another order.
    pub fn order_sleep(&mut self, ids: &[usize], b: usize) {
        if self.furn.get(b).is_none_or(|f| f.function != BF_BED) || !self.bed_free(b) {
            return;
        }
        let Some(&id) = ids.iter().find(|&&i| !self.folk[i].down && self.folk[i].carrying.is_none()) else { return };
        let at = self.furn[b].position;
        self.order_move(&[id], at);
        self.bed_errands.retain(|e| e.id != id);
        self.bed_errands.push(BedErrand { id, bed: b, put: false, lying: false, age: 0.0 });
    }

    /// "Put in Bed" (order 99, Task "Put in bed"): the one of `ids` carrying someone walks to
    /// bed `b` and lays them in it.
    pub fn order_put_in_bed(&mut self, ids: &[usize], b: usize) {
        if !self.bed_free(b) {
            return;
        }
        let Some(&id) = ids.iter().find(|&&i| self.folk[i].carrying.is_some()) else { return };
        let at = self.furn[b].position;
        self.order_move(&[id], at);
        self.bed_errands.retain(|e| e.id != id);
        self.bed_errands.push(BedErrand { id, bed: b, put: true, lying: false, age: 0.0 });
    }

    /// Lay person `w` in bed `b`.
    fn lay_in_bed(&mut self, w: usize, b: usize) {
        let at = self.furn[b].position.map(|v| v * kenshi_world::navmesh::METRES);
        let p = &mut self.folk[w];
        p.carried_by = None;
        p.bed = Some(b);
        p.mover.position = at;
        stop(&mut p.mover, &mut p.pathing);
        self.arena.f[w].mover.position = at;
        if let Some(t) = self.bed_taken.get_mut(b) {
            *t = Some(w);
        }
    }

    /// (testing) `get_out_of_bed`.
    pub fn get_out_of_bed_test(&mut self, id: usize) {
        self.get_out_of_bed(id);
    }

    pub(crate) fn get_out_of_bed(&mut self, id: usize) {
        if let Some(b) = self.folk[id].bed.take()
            && let Some(t) = self.bed_taken.get_mut(b)
            && *t == Some(id)
        {
            *t = None;
        }
    }

    fn bed_tick(&mut self, dt: f32) {
        let mut keep = Vec::new();
        for mut e in std::mem::take(&mut self.bed_errands) {
            e.age += dt;
            let Some(furn) = self.furn.get(e.bed) else { continue };
            let pos = furn.position;
            let id = e.id;
            if self.folk[id].dead {
                continue;
            }
            if e.lying {
                // in the bed: kept there until given another order (or, someone else's, until up
                // and about again)
                let p = &self.folk[id];
                let ordered = !p.ai.orders.is_empty() || (!p.player && !p.down && !p.ai.queue.is_empty());
                if ordered || p.carried_by.is_some() {
                    if std::env::var("KENSHI_ORDER_TEST").is_ok() {
                        eprintln!("order test: {id} leaves bed {}: orders {} queue {} carried {:?}", e.bed, p.ai.orders.len(), p.ai.queue.len(), p.carried_by);
                    }
                    self.get_out_of_bed(id);
                    continue;
                }
                let at = pos.map(|v| v * kenshi_world::navmesh::METRES);
                let p = &mut self.folk[id];
                p.mover.position = at;
                p.mover.velocity = [0.0; 3];
                self.arena.f[id].mover.position = at;
                keep.push(e);
                continue;
            }
            let near = self.dist(id, pos) <= furn.use_range.max(10.0) + 40.0;
            if self.folk[id].down || (self.still(id) && !near && e.age > 2.0) {
                if std::env::var("KENSHI_ORDER_TEST").is_ok() {
                    eprintln!("order test: {id} gives up bed {}: {:.0} away (reach {:.0}), down {}", e.bed, self.dist(id, pos), furn.use_range, self.folk[id].down);
                }
                continue;
            }
            if !(near && self.still(id)) {
                keep.push(e);
                continue;
            }
            if e.put {
                let Some(w) = self.folk[id].carrying.take() else { continue };
                self.lay_in_bed(w, e.bed);
                let name = self.display_name(w);
                self.messages.push(format!("{name} has been put in bed."));
                keep.push(BedErrand { id: w, bed: e.bed, put: false, lying: true, age: 0.0 });
            } else {
                self.folk[id].ai.orders.clear();
                self.lay_in_bed(id, e.bed);
                e.lying = true;
                keep.push(e);
            }
        }
        self.bed_errands = keep;
    }

    // ---- splints ----------------------------------------------------------------------------

    /// The best splint kit `id` carries ("item function" 2): its section, index and "quality".
    pub fn splint_kit(&self, id: usize) -> Option<(usize, usize, f32)> {
        self.folk[id]
            .inventory
            .sections
            .iter()
            .enumerate()
            .flat_map(|(s, sec)| sec.items.iter().enumerate().map(move |(i, p)| (s, i, p)))
            .filter_map(|(s, i, p)| {
                let r = self.data.get(&p.item.record)?;
                (r.ints.get("item function").copied() == Some(ITEM_SPLINT)).then(|| (s, i, r.floats.get("quality").copied().unwrap_or(25.0)))
            })
            .max_by(|a, b| a.2.total_cmp(&b.2))
    }

    /// @fn F040439
    ///
    /// The limbs of `id` a splint of `quality` helps ("needsSplint", F050290): not robotic,
    /// their health with the splint they have x 1.05 under 50, and the kit better than that
    /// splint by more than 1.
    pub fn limbs_to_splint(&self, id: usize, quality: f32) -> Vec<usize> {
        self.arena.f[id]
            .medical
            .parts
            .iter()
            .enumerate()
            .filter(|(_, q)| !q.vital && q.treatment > q.bonus)
            .filter(|(_, q)| (q.bonus + q.flesh - q.stun) * 1.05 < 50.0 && (quality as i32) as f32 > q.bonus + 1.0)
            .map(|(k, _)| k)
            .collect()
    }

    /// Whether `id` has a limb a splint would help (any kit: the best, 50).
    pub fn needs_splint(&self, id: usize) -> bool {
        !self.folk[id].dead && !self.limbs_to_splint(id, 50.0).is_empty()
    }

    /// "Splint Injuries" (order 0xf9, Task "Splinting"): the first of `ids` with a splint kit
    /// goes to `patient` and splints their broken limbs; none with a kit says so ("I don't have
    /// a splint kit.").
    pub fn order_splint(&mut self, ids: &[usize], patient: usize) {
        let Some(&medic) = ids.iter().find(|&&i| !self.folk[i].down && self.splint_kit(i).is_some()) else {
            if let Some(&i) = ids.first() {
                self.said.push((i, "I don't have a splint kit.".to_string()));
            }
            return;
        };
        if medic != patient {
            let at = self.folk[patient].mover.position_units();
            self.order_move(&[medic], at);
        }
        self.splinters.retain(|e| e.medic != medic);
        self.splinters.push(SplintErrand { medic, patient, work: 0.0 });
    }

    fn splint_tick(&mut self, dt: f32) {
        let mut keep = Vec::new();
        for mut e in std::mem::take(&mut self.splinters) {
            if self.folk[e.medic].down || self.folk[e.patient].dead {
                continue;
            }
            let at = self.folk[e.patient].mover.position_units();
            if e.medic != e.patient && self.dist(e.medic, at) > REACH {
                if self.still(e.medic) {
                    self.order_move(&[e.medic], at);
                }
                keep.push(e);
                continue;
            }
            // Task_FirstAidRig (F031288): each update the splint (F040455, the part's +0x4c) on
            // the first limb that wants one grows by (1 + 14 x level / 100) x "medic speed mult"
            // a second, the level the lesser of the medic's skill and the kit's quality, up to
            // the level + 1 and to the limb's stun and treatment; the kit's charges drain as
            // first aid's do (F040413); done when no limb wants more
            let Some((s, i, quality)) = self.splint_kit(e.medic) else { continue };
            let skill = self.arena.f[e.medic].level(StatsEnumerated::StatMedic);
            let level = skill.min(quality);
            let cap = 1.0 + level.floor();
            // (F040455: a part not vital (+0x32), under 50 with its splint, the splint short of
            // its stun and treatment and of the cap)
            let want = self.arena.f[e.patient].medical.parts.iter().position(|q| !q.vital && (q.flesh - q.stun) + q.bonus < 50.0 && q.bonus < q.stun + q.treatment && q.bonus < cap);
            let Some(k) = want else {
                self.arena.f[e.medic].skills.train(StatsEnumerated::StatMedic, 1.0, 1.0);
                let name = self.display_name(e.patient);
                self.messages.push(format!("{name}'s injuries have been splinted."));
                continue;
            };
            let c = self.first_aid_c;
            let rate = (1.0 + 14.0 * level * 0.01) * c.speed;
            let part = &mut self.arena.f[e.patient].medical.parts[k];
            part.bonus = (part.bonus + rate * dt).min(part.stun + part.treatment).min(cap);
            part.update_frac();
            e.work += dt;
            let drained = kenshi_char::medical::kit_drain(skill, kenshi_char::medical::Kit { quality, robot: false }, &c, dt);
            let item = &mut self.folk[e.medic].inventory.sections[s].items[i].item;
            if let Some(kit) = item.medkit.as_mut() {
                kit.charges -= drained;
                if kit.charges <= 0.0 {
                    if item.quantity > 1 {
                        item.quantity -= 1;
                        kit.charges = kit.full;
                    } else {
                        self.folk[e.medic].inventory.sections[s].items.remove(i);
                    }
                }
            }
            keep.push(e);
        }
        self.splinters = keep;
    }

    // ---- shackles -----------------------------------------------------------------------------

    /// The first carried tool (a Hacksaw, "item function" 9) of `id`: its section and index.
    fn tool(&self, id: usize) -> Option<(usize, usize)> {
        self.folk[id].inventory.sections.iter().enumerate().find_map(|(s, sec)| {
            sec.items.iter().position(|p| self.data.get(&p.item.record).is_some_and(|r| r.ints.get("item function").copied() == Some(ITEM_TOOLS))).map(|i| (s, i))
        })
    }

    /// "Unlock Shackles" (0xc9), "Use Tools" (0xb9), "Use Strength" (0xba) on `target`'s locked
    /// shackles: the target itself does it if selected, else the first of `ids`.
    pub fn order_shackles(&mut self, ids: &[usize], target: usize, how: ShackleHow) {
        let Some(level) = self.shackles_lock(target) else { return };
        let worker = if ids.contains(&target) { target } else { ids.iter().copied().find(|&i| !self.folk[i].down).unwrap_or(target) };
        if how == ShackleHow::Cut && self.tool(worker).is_none() {
            self.said.push((worker, "I need tools to cut these shackles.".to_string()));
            return;
        }
        if worker != target {
            let at = self.folk[target].mover.position_units();
            self.order_move(&[worker], at);
        }
        let (progress, next) = match how {
            ShackleHow::Pick => (0.0, 3.0),
            ShackleHow::Cut => (level, 0.0),
            ShackleHow::Strength => (0.0, 0.0),
        };
        self.shackle_work.retain(|w| w.worker != worker);
        self.shackle_work.push(ShackleWork { worker, target, how, progress, next });
    }

    /// The bar over someone working on shackles: its caption and how full it is.
    pub fn shackle_bar(&self, id: usize) -> Option<(String, f32)> {
        let w = self.shackle_work.iter().find(|w| w.worker == id)?;
        let at = self.folk[w.target].mover.position_units();
        if w.worker != w.target && self.dist(w.worker, at) > REACH {
            return None;
        }
        let level = self.shackles_lock(w.target)?;
        match w.how {
            ShackleHow::Pick => {
                let skill = self.arena.f[w.worker].level(StatsEnumerated::StatLockpicking);
                let chance = crate::law::lockpick_chance(level, skill);
                Some((format!("Lockpicking (Chance {}%)", (chance * 100.0).round() as i32), (w.progress / w.next.max(0.01)).clamp(0.0, 1.0)))
            }
            // F031346: "Cutting: {1,num}" (the hardness left x 10)
            ShackleHow::Cut => Some((format!("Cutting: {}", (w.progress * 10.0) as i32), 1.0 - (w.progress / level.max(0.01)).clamp(0.0, 1.0))),
            ShackleHow::Strength => None,
        }
    }

    fn break_shackles(&mut self, worker: usize, target: usize) {
        self.remove_shackles(target);
        let name = self.display_name(target);
        self.messages.push(format!("{name}: Shackles broken!"));
        self.pick_events.push((worker, true));
    }

    fn shackle_tick(&mut self, dt: f32) {
        let mut keep = Vec::new();
        for mut w in std::mem::take(&mut self.shackle_work) {
            let Some(level) = self.shackles_lock(w.target) else { continue };
            if self.folk[w.worker].dead || (self.folk[w.worker].down && w.worker != w.target) {
                continue;
            }
            let at = self.folk[w.target].mover.position_units();
            if w.worker != w.target && self.dist(w.worker, at) > REACH {
                if self.still(w.worker) {
                    self.order_move(&[w.worker], at);
                }
                keep.push(w);
                continue;
            }
            // (crate::law) each update a crime
            self.shackle_crime(w.worker, w.target);
            match w.how {
                // F031308: broken at once if the strength is over 3 x the lock's level
                ShackleHow::Strength => {
                    let strength = self.shackle_strength(w.worker);
                    if strength > level * 3.0 {
                        self.said.push((w.worker, "*Crunck*".to_string()));
                        self.break_shackles(w.worker, w.target);
                    } else {
                        self.said.push((w.worker, "I'm not strong enough to do that".to_string()));
                    }
                    continue;
                }
                // Task_PickLock_Shackles: lockpicking attempts, as a cage's (law)
                ShackleHow::Pick => {
                    let skill = self.arena.f[w.worker].level(StatsEnumerated::StatLockpicking);
                    w.progress += (0.55 + (2.0 - 0.55) * (skill * 0.01).clamp(0.0, 1.0)) * dt;
                    self.arena.f[w.worker].skills.train(StatsEnumerated::StatLockpicking, dt * 0.05, 1.0);
                    if w.progress >= w.next {
                        w.progress = 0.0;
                        w.next = 2.0 + self.rng.uniform() * 2.0;
                        let ok = self.rng.uniform() < crate::law::lockpick_chance(level, skill);
                        if ok {
                            self.break_shackles(w.worker, w.target);
                            continue;
                        }
                        self.pick_events.push((w.worker, false));
                    }
                }
                // F031346: the hardness worn down by lerp(strength x 0.01, 0.05, 0.2) x 2 a
                // second (F030932's strength); the tool used up when it is through
                ShackleHow::Cut => {
                    let strength = self.shackle_strength(w.worker);
                    w.progress -= (0.05 + 0.15 * strength * 0.01) * 2.0 * dt;
                    if w.progress <= 0.0 {
                        if let Some((s, i)) = self.tool(w.worker) {
                            let item = &mut self.folk[w.worker].inventory.sections[s].items[i].item;
                            if item.quantity > 1 {
                                item.quantity -= 1;
                            } else {
                                self.folk[w.worker].inventory.sections[s].items.remove(i);
                            }
                            self.break_shackles(w.worker, w.target);
                        }
                        continue;
                    }
                }
            }
            keep.push(w);
        }
        self.shackle_work = keep;
    }

    // ---- cages --------------------------------------------------------------------------------

    /// Whether cage `c` is empty.
    pub fn cage_free(&self, c: usize) -> bool {
        !self.folk.iter().any(|p| p.prison.as_ref().is_some_and(|q| q.cage == c && !q.out))
    }

    /// "Put in" (order 0x6c, Task "Put in prison"): the one of `ids` carrying someone takes them
    /// to empty cage `c` and locks them in. "Get in" (0x6b): the first of `ids` gets in itself.
    pub fn order_cage(&mut self, ids: &[usize], c: usize, get_in: bool) {
        if !self.cage_free(c) {
            return;
        }
        let id = if get_in { ids.iter().copied().find(|&i| !self.folk[i].down && self.folk[i].carrying.is_none()) } else { ids.iter().copied().find(|&i| self.folk[i].carrying.is_some()) };
        let Some(id) = id else { return };
        let door = self.cage_exit(c);
        self.order_move(&[id], door);
        self.cage_errands.retain(|e| e.id != id);
        self.cage_errands.push(CageErrand { id, cage: c, get_in, age: 0.0 });
    }

    fn cage_tick(&mut self, dt: f32) {
        let mut keep = Vec::new();
        for mut e in std::mem::take(&mut self.cage_errands) {
            e.age += dt;
            if self.folk[e.id].down || !self.cage_free(e.cage) {
                continue;
            }
            let door = self.cage_exit(e.cage);
            let near = self.dist(e.id, door) <= 30.0;
            if !near {
                if !(self.still(e.id) && e.age > 2.0) {
                    keep.push(e);
                }
                continue;
            }
            let who = if e.get_in { e.id } else { match self.folk[e.id].carrying.take() { Some(w) => w, None => continue } };
            // (assumption) locked in for the player's faction with no sentence; one getting in
            // of their own will finds the cage unlocked (lock 0: "The cage you are in is
            // UNLOCKED... you can escape any time you want.")
            self.imprison(who, e.cage, PLAYER_FACTION, false);
            if let Some(p) = self.folk[who].prison.as_mut() {
                p.until = f64::MAX;
                if e.get_in {
                    p.lock.level = 0.0;
                }
            }
        }
        self.cage_errands = keep;
    }

    // ---- standing up --------------------------------------------------------------------------

    /// "Stand Up" (order 0x1c): one of the player's playing dead gets up.
    pub fn order_stand_up(&mut self, id: usize) {
        if let Some(p) = self.folk.get_mut(id) {
            p.stand_up = true;
        }
    }

    // ---- attack all ---------------------------------------------------------------------------

    /// "Attack All" (order 0x10): `ids` attack `target`, then the rest of its squad one by one
    /// (the nearest standing), until none stands.
    pub fn order_attack_all(&mut self, ids: &[usize], target: usize) {
        self.order_attack(ids, target);
        let platoon = self.folk[target].platoon;
        for &id in ids {
            self.attack_all.retain(|(a, _)| *a != id);
            self.attack_all.push((id, platoon.map_or(usize::MAX, |p| p)));
        }
    }

    fn attack_all_tick(&mut self) {
        if !self.tick.is_multiple_of(30) {
            return;
        }
        let mut keep = Vec::new();
        for (id, platoon) in std::mem::take(&mut self.attack_all) {
            if self.folk[id].down || self.folk[id].dead || platoon == usize::MAX {
                continue;
            }
            let busy = self.arena.f[id].focus.is_some_and(|f| !self.folk[f].down && !self.folk[f].dead);
            if busy {
                keep.push((id, platoon));
                continue;
            }
            let at = self.folk[id].mover.position_units();
            let next = (0..self.folk.len())
                .filter(|&o| self.folk[o].platoon == Some(platoon) && !self.folk[o].down && !self.folk[o].dead && !self.folk[o].player)
                .min_by(|&a, &b| {
                    let d = |k: usize| {
                        let q = self.folk[k].mover.position_units();
                        (q[0] - at[0]).hypot(q[2] - at[2])
                    };
                    d(a).total_cmp(&d(b))
                });
            if let Some(n) = next {
                self.order_attack(&[id], n);
                keep.push((id, platoon));
            }
        }
        self.attack_all = keep;
    }

    // ---- auto-haul ----------------------------------------------------------------------------

    /// "Auto-Haul": Operating automatic machine (order 0x98, 152; F044603's menu gives it for a
    /// machine of no operators, Operate for the others, Hauling to (0x7c, 124) for a storage) on
    /// machine `b`, an order on the task system's list for each of `ids`, planned (crate::jobs):
    /// its output taken to the storages that keep it.
    pub fn order_auto_haul(&mut self, ids: &[usize], b: usize) {
        if !self.is_machine(b) {
            return;
        }
        let at = self.buildings[b].at;
        let ids: Vec<usize> = ids.iter().copied().filter(|&id| self.folk.get(id).is_some_and(|p| !p.down)).collect();
        for &id in &ids {
            self.folk[id].operate = None;
        }
        self.order_task(&ids, crate::jobs::OPERATING_AUTOMATIC, crate::sim::Target::Building(b), at);
    }

    /// "Auto-Haul" on a storage (order 0x7c, Hauling to, 124; F044603's menu gives it for a
    /// StorageBuilding): an order on the task system's list for each of `ids` on the storage,
    /// planned (crate::jobs): what it keeps fetched (Try filling machine's finder, F038633) and
    /// put in (Task_FillMachine, F031179), again while there is any.
    pub fn order_haul_to(&mut self, ids: &[usize], b: usize) {
        if !self.is_storage_building(b) {
            return;
        }
        let at = self.buildings[b].at;
        let ids: Vec<usize> = ids.iter().copied().filter(|&id| self.folk.get(id).is_some_and(|p| !p.down)).collect();
        self.order_task(&ids, crate::jobs::HAULING_TO, crate::sim::Target::Store(crate::base::BUILDING_OWNER + b), at);
    }

    // ---- throwing out -------------------------------------------------------------------------

    /// "Throw out of home" (order 0xdf, F044505: someone in a building of the player's) and
    /// "Throw out of town" (0x111: someone in the player's outpost): they walk out of it.
    /// (assumption) Out: past the building's edge, or 300 units past the outpost's, the way
    /// they are from its middle.
    pub fn order_throw_out(&mut self, target: usize, town: bool) {
        let at = self.folk[target].mover.position_units();
        let (centre, reach) = if town {
            let built: Vec<[f32; 3]> = self.buildings.iter().filter(|b| b.done).map(|b| b.at).collect();
            if built.is_empty() {
                return;
            }
            let n = built.len() as f32;
            let c = [built.iter().map(|b| b[0]).sum::<f32>() / n, at[1], built.iter().map(|b| b[2]).sum::<f32>() / n];
            let r = built.iter().map(|b| (b[0] - c[0]).hypot(b[2] - c[2])).fold(0.0, f32::max) + 1300.0;
            (c, r)
        } else {
            let Some(b) = self.building_at(at) else { return };
            (self.buildings[b].at, self.buildings[b].radius + 80.0)
        };
        let (dx, dz) = (at[0] - centre[0], at[2] - centre[2]);
        let d = dx.hypot(dz).max(1.0);
        let to = [centre[0] + dx / d * reach, at[1], centre[2] + dz / d * reach];
        self.folk[target].ai.queue.clear();
        self.order_move(&[target], to);
        let name = self.display_name(target);
        self.messages.push(format!("{name} has been thrown out."));
    }

    // ---- corpses ------------------------------------------------------------------------------

    /// "Corpse Disposal" (Task "Corpse disposal"): the first of `ids` free picks up the body and
    /// carries it away from the nearest town (or the outpost), 1500 units further out, and lays
    /// it down there. (assumption: the game's job finds corpses itself and takes them to its own
    /// spots)
    pub fn order_dispose(&mut self, ids: &[usize], body: usize) {
        if !self.folk[body].dead {
            return;
        }
        let Some(&id) = ids.iter().find(|&&i| !self.folk[i].down && self.folk[i].carrying.is_none()) else { return };
        self.order_pickup(&[id], body);
        self.disposers.retain(|(c, ..)| *c != id);
        let at = self.folk[body].mover.position_units();
        let town = self.towns.iter().map(|t| t.position).min_by(|a, b| ((a[0] - at[0]).hypot(a[2] - at[2])).total_cmp(&(b[0] - at[0]).hypot(b[2] - at[2])));
        let from = town.unwrap_or([at[0] - 1.0, at[1], at[2]]);
        let (dx, dz) = (at[0] - from[0], at[2] - from[2]);
        let d = dx.hypot(dz).max(1.0);
        // a spot on the navmesh that way (nearer in when there is none that far: water, a cliff)
        let m = kenshi_world::navmesh::METRES;
        let to = [1500.0f32, 1000.0, 600.0, 300.0]
            .iter()
            .find_map(|&r| {
                let want = [at[0] + dx / d * r, at[1], at[2] + dz / d * r];
                self.nav.closest_point(want.map(|v| v * m), 40.0).map(|(_, q)| q.map(|v| v / m))
            })
            .unwrap_or(at);
        self.disposers.push((id, body, to));
    }

    fn dispose_tick(&mut self) {
        let mut keep = Vec::new();
        for (id, body, to) in std::mem::take(&mut self.disposers) {
            if self.folk[id].down {
                continue;
            }
            match self.folk[id].carrying {
                Some(w) if w == body => {
                    if std::env::var("KENSHI_ORDER_TEST").is_ok() && self.tick.is_multiple_of(300) {
                        eprintln!("order test: disposing: {:.0} from the spot, still {}", self.dist(id, to), self.still(id));
                    }
                    if self.dist(id, to) < 60.0 {
                        self.order_drop(&[id]);
                        continue;
                    }
                    // (ordered again every two seconds while it stands: a move ordered every
                    // tick never gets its path)
                    if self.still(id) && self.tick.is_multiple_of(120) {
                        self.order_move(&[id], to);
                    }
                    keep.push((id, body, to));
                }
                // still on the way to pick it up
                None if self.folk[id].pickup == Some(body) => keep.push((id, body, to)),
                _ => {}
            }
        }
        self.disposers = keep;
    }

    /// The player's errands each tick.
    pub fn orders_tick(&mut self, dt: f32) {
        self.bed_tick(dt);
        self.splint_tick(dt);
        self.shackle_tick(dt);
        self.cage_tick(dt);
        self.attack_all_tick();
        self.dispose_tick();
        self.door_order_tick();
    }
}

/// @fn F036821
///
/// A job of one of the player's people: an entry of its AI's goal list (AITaskSytem +0x88), which
/// the JOBS toggle (its free will, +0x35, F036819) lets the AI take up. Shift with an order
/// (F039153) makes the order's task a job when its definition allows one (+0x04): first turned
/// into its job form (+0x0c: Building and Repairing -> Engineer, Attacking -> Attacking
/// enemies, ...), its follow-up (+0x10) added after it, the JOBS toggle turned on; any other
/// order is just given. A kind whose definition's +0x08 is clear is kept once, whatever its
/// target. Its target is the clicked thing as the right click names it: a person, furniture
/// (`production::FURN_TARGET` + index) or a building (`base::BUILDING_OWNER` + index). The
/// job list is the AI's goals list, ranked in its order and planned (crate::jobs).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Job {
    /// The AI_TASK the job runs (0 in saves from before: taken from the target).
    #[serde(default)]
    pub task: u16,
    pub target: usize,
    /// What the jobs panel shows.
    pub label: String,
}

/// The job form of an order's task and its follow-up (0 for none), when it can be a job
/// (F036821: the definition's +0x04, +0x0c and +0x10). A Bodyguard's follow-up, Staying close
/// (31), is the player's own kind (289, the character's vtable +0x248 being set for them).
pub fn job_form(order: u16) -> Option<(u16, u16)> {
    let d = kenshi_ai::task_def::def(order)?;
    let task = if d.f04 != 0 && d.alias != 0 { d.alias as u16 } else { order };
    let follow = if d.follow_up == 31 { 289 } else { d.follow_up as u16 };
    (kenshi_ai::task_def::def(task)?.f04 != 0).then_some((task, follow))
}

/// AI_TASK enums of the orders a quick right click gives and of their jobs.
pub const BUILDING: u16 = 2;
pub const ATTACKING_TARGET: u16 = 5;
pub const OPERATING_MACHINE: u16 = 87;
pub const REPAIRING: u16 = 95;
pub const TRAINING: u16 = 97;
pub const ENGINEER: u16 = 125;

/// One of the stance orders (the main panel's buttons; F039205's codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StanceOrder {
    Block,
    Hold,
    Passive,
    Taunt,
    Ranged,
}

impl Sim {
    /// The order a quick right click on `target` gives one of the player's people (its AI_TASK):
    /// someone standing attacked, someone down lifted (68), training equipment (a town's or the
    /// player's) trained on, a building built (repaired when damaged) or used.
    pub fn order_task_of(&self, target: usize) -> Option<u16> {
        if let Some(f) = target.checked_sub(crate::production::FURN_TARGET) {
            return self.furn.get(f).is_some_and(|k| k.function == crate::production::FUNCTION_TRAINING).then_some(TRAINING);
        }
        if target >= crate::doors::DOOR_TARGET {
            return None;
        }
        if let Some(b) = target.checked_sub(crate::base::BUILDING_OWNER) {
            let pb = self.buildings.get(b)?;
            // F044603's: built, a storage Hauling to (0x7c), a machine of no operators (+0x3ac)
            // Operating automatic machine (0x98), another Operate (0x57)
            return if !pb.done {
                Some(BUILDING)
            } else if self.needs_repair(b) {
                Some(REPAIRING)
            } else if self.training(b).is_some() {
                Some(TRAINING)
            } else if self.is_storage_building(b) {
                Some(crate::jobs::HAULING_TO)
            } else if self.is_machine(b) && self.max_operators_of(b) == 0 {
                Some(crate::jobs::OPERATING_AUTOMATIC)
            } else if self.is_operated(b) {
                Some(OPERATING_MACHINE)
            } else {
                None
            };
        }
        let p = self.folk.get(target)?;
        if p.dead || p.player {
            return None;
        }
        Some(if p.down { 68 } else { ATTACKING_TARGET })
    }

    /// What the jobs panel calls a job: the AI_TASK's name, and what it is on.
    pub fn job_label(&self, task: u16, target: usize) -> String {
        let name = kenshi_ai::task_def::def(task).map_or("Job", |d| d.name);
        let on = if let Some(f) = target.checked_sub(crate::production::FURN_TARGET) {
            self.furn.get(f).and_then(|k| self.placed.get(k.placement)).and_then(|p| self.data.get(&p.building)).map(|r| r.name.clone())
        } else if target >= crate::doors::DOOR_TARGET {
            None
        } else if let Some(b) = target.checked_sub(crate::base::BUILDING_OWNER) {
            self.buildings.get(b).and_then(|pb| self.data.get(&pb.record)).map(|r| r.name.clone())
        } else {
            self.folk.get(target).map(|_| self.display_name(target))
        };
        match on {
            Some(n) => format!("{name}: {n}"),
            None => name.to_string(),
        }
    }

    /// Shift with an order on `target` (F039153 -> F036821): a job for each of the player's
    /// people of `ids` when the order's task can be one (once; a kind without +0x08 once at
    /// all), its follow-up after it, the JOBS toggle turned on. False when it is no job: the
    /// order is then given as usual.
    pub fn add_job(&mut self, ids: &[usize], target: usize) -> bool {
        let Some((task, follow)) = self.order_task_of(target).and_then(job_form) else { return false };
        let mut jobs = vec![Job { task, target, label: self.job_label(task, target) }];
        if follow != 0 {
            jobs.push(Job { task: follow, target, label: self.job_label(follow, target) });
        }
        for &id in ids {
            if let Some(p) = self.folk.get_mut(id)
                && p.player
            {
                let (first, rest) = jobs.split_first().expect("a job");
                let once = kenshi_ai::task_def::def(first.task).is_some_and(|d| d.f08 == 0);
                if p.jobs.iter().any(|j| j.task == first.task && (once || j.target == first.target)) {
                    continue;
                }
                p.jobs.push(first.clone());
                p.jobs.extend(rest.iter().cloned());
                p.jobs_on = true;
            }
        }
        true
    }

    /// The JOBS button (F042966 -> order 0xf, F039205 -> F036819): on for all the selection
    /// unless the first already has it, then off.
    pub fn toggle_jobs(&mut self, ids: &[usize]) {
        let Some(&first) = ids.iter().find(|&&i| self.folk.get(i).is_some_and(|p| p.player)) else { return };
        let on = !self.folk[first].jobs_on;
        for &id in ids {
            if let Some(p) = self.folk.get_mut(id)
                && p.player
            {
                p.jobs_on = on;
            }
        }
    }

    /// The job list cleared.
    pub fn clear_jobs(&mut self, ids: &[usize]) {
        for &id in ids {
            if let Some(p) = self.folk.get_mut(id) {
                p.jobs.clear();
                // the permanent "Obedient Slave" job goes with them (F039272: "You can remove
                // this job, but beware of the consequences...")
                if p.player
                    && let Some(s) = p.slave.as_mut()
                {
                    s.obedient = false;
                }
            }
        }
    }

    pub fn has_stance(&self, id: usize, which: StanceOrder) -> bool {
        let s = &self.folk[id].stance;
        match which {
            StanceOrder::Block => s.block,
            StanceOrder::Hold => s.hold,
            StanceOrder::Passive => s.passive,
            StanceOrder::Taunt => s.taunt,
            StanceOrder::Ranged => s.ranged,
        }
    }

    /// @fn F045513
    ///
    /// A stance order to the selection: on for all of them unless the first already has it,
    /// then off (F042962..F042968 toggle the button, F045513 tests the selection); each of the
    /// player's people takes it (F039205). Hold keeps the spot the order was given at.
    pub fn toggle_stance(&mut self, ids: &[usize], which: StanceOrder) {
        let Some(&first) = ids.iter().find(|&&i| self.folk.get(i).is_some_and(|p| p.player)) else { return };
        let on = !self.has_stance(first, which);
        for &id in ids {
            if !self.folk.get(id).is_some_and(|p| p.player) {
                continue;
            }
            let at = self.folk[id].mover.position_units();
            let p = &mut self.folk[id];
            match which {
                StanceOrder::Block => p.stance.block = on,
                StanceOrder::Hold => {
                    p.stance.hold = on;
                    p.hold_at = on.then_some(at);
                }
                StanceOrder::Passive => p.stance.passive = on,
                StanceOrder::Taunt => p.stance.taunt = on,
                StanceOrder::Ranged => p.stance.ranged = on,
            }
        }
    }
}
