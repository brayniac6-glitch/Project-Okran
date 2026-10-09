//! Slavery (port_map `law`): the slaves a squad owns (SQUAD_TEMPLATE "slaves": the slave shops'
//! "slaves 3-7", the slave camps' "slaves 1-3"), what they do (the owner's AI package "Slave AI
//! Goals": "Slave get in Cage" (172), "Auto worker mining machines" (189, 190 pretending),
//! "Follow slavemaster" (171), idle), the player's people enslaved ("Obedient Slave", F039272:
//! working for their master until the player gives them an order of their own), and selling and
//! buying slaves (F041437 "Selling Slaves", F041440 "Buying Slaves").
//!
//! Specs:
//! - F040876 (a character's value): (strength x its stat multiplier + melee attack + dexterity x
//!   the multiplier) x the race's "buy value", kept within 1000 .. 20000.
//! - F041437: a slave sells for half its value, kept within 200 .. 2500 (unless the trader fixes a
//!   price); F041440: a slave is bought for its value (or the fixed price), twice that for one of
//!   the player's own.
//! - A goal of "Slave AI Goals" runs within its hours (val0 .. val1), the highest priority
//!   (val2 / 100, 1 when 0) of those running.
//!
//! (assumption) The handlers of the slave goals are not decompiled: a slave in its cage stays in
//! it; one sent to work walks from its cage to the town's nearest mining machine (BF_MINE,
//! BF_MINE_NATURAL, BF_REFINERY), else a general node, and stands at it; one following walks with
//! its squad's leader. Slaves take their owners' faction. A bought slave joins the squad freed of
//! cage and shackles.

use crate::sim::Sim;
use kenshi_world::furniture::BF_CAGE;

/// "Follow slavemaster", "Slave get in Cage", "Auto worker mining machines" (and pretending),
/// idle.
pub const SLAVE_FOLLOW: u16 = 171;
pub const SLAVE_CAGE: u16 = 172;
pub const SLAVE_WORK: u16 = 189;
pub const SLAVE_WORK_PRETEND: u16 = 190;

/// A slave's master.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Slavery {
    /// The owning squad (platoon) and faction.
    pub owner: Option<usize>,
    pub faction: String,
    /// The player's people: obeying ("Obedient Slave") until given an order of their own.
    pub obedient: bool,
    /// What it is doing (a slave goal) and where its work is (units).
    pub doing: u16,
    pub work_at: Option<[f32; 3]>,
    /// Where the routine last sent it: an order elsewhere is the player's own (the job given up).
    pub sent: Option<[f32; 3]>,
    /// Escaping has been made a crime once.
    pub fled: bool,
    /// The machine (furniture) it works, its operator slot taken.
    pub machine: Option<usize>,
}

impl Sim {
    /// @fn F040876
    pub fn character_value(&self, id: usize) -> i32 {
        use kenshi_char::StatsEnumerated::*;
        let f = &self.arena.f[id];
        let race = self.folk[id].race.clone().unwrap_or_else(|| self.race_record.clone());
        let buy = self.data.get(&race).and_then(|r| r.ints.get("buy value").copied()).unwrap_or(100);
        let v = (f.level(StatStrength) + f.level(StatMeleeAttack) + f.level(StatDexterity)) as i32 * buy;
        v.clamp(1000, 20000)
    }

    /// @fn F041437
    pub fn slave_sell_price(&self, id: usize) -> i32 {
        (self.character_value(id) / 2).clamp(200, 2500)
    }

    /// @fn F041440
    pub fn slave_buy_price(&self, id: usize) -> i32 {
        let v = self.character_value(id);
        if self.folk[id].player { v * 2 } else { v }
    }

    /// Make person `id` a slave of squad `owner` of `faction`, shackled in one of the faction's
    /// free cages (any cage near when it has none).
    pub fn enslave(&mut self, id: usize, owner: Option<usize>, faction: &str) {
        let at = self.folk[id].mover.position_units();
        let cage = self.free_slave_cage(faction, at);
        let player = self.folk[id].player;
        if !player {
            self.folk[id].faction = Some(faction.to_string());
            let side = self.side(&Some(faction.to_string()));
            self.arena.f[id].team = side;
            // its own goals give way to its master's (the slave goals)
            self.folk[id].ai.jobs.iter_mut().for_each(|j| j.clear());
            self.folk[id].ai.goals.clear();
        }
        self.dress_as_slave(id, faction);
        self.folk[id].slave = Some(Slavery { owner, faction: faction.to_string(), obedient: true, doing: SLAVE_CAGE, work_at: None, sent: None, fled: false, machine: None });
        match cage {
            Some(c) => self.imprison(id, c, faction, true),
            None => self.folk[id].shackled = true,
        }
    }

    /// A new slave's clothes (Task_ProcessNewSlave, F031348 "slave clothing"): what it wore and
    /// carried taken (left on the ground where it is), its master's faction's "slave clothing"
    /// put on with a pair of "Prisoner Shackles" ("is locked"; the Hive's on the body), its head
    /// shaved. (assumption) The taken things are left where it was taken, not stored.
    pub fn dress_as_slave(&mut self, id: usize, faction: &str) {
        let data = self.data.clone();
        let taken: Vec<kenshi_char::inventory::Item> = self.folk[id].inventory.sections.iter_mut().flat_map(|s| s.items.drain(..).map(|p| p.item)).collect();
        for item in taken {
            self.drop_item(id, item);
        }
        let clothing: Vec<String> = data.get(faction).map(|f| f.refs("slave clothing").iter().map(|(s, _)| s.clone()).collect()).unwrap_or_default();
        let race_name = self.folk[id].race.clone().and_then(|r| data.get(&r).map(|x| x.name.clone())).unwrap_or_default();
        let shackles = if race_name.contains("Hive") { "SHACKLES-BODY" } else { "SHACKLES" };
        for sid in clothing.iter().map(String::as_str).chain([shackles]) {
            if let Some(r) = data.get(sid) {
                let item = if r.item_type() == Some(kenshi_data::ItemType::Armour) { kenshi_char::inventory::Item::armour(&data, r, 1) } else { kenshi_char::inventory::Item::from_record(r, 1) };
                let _ = self.folk[id].inventory.add(item, true);
            }
        }
        self.folk[id].shaved = true;
        self.folk[id].shackled = true;
    }

    /// The lock of the shackles `id` wears ("is locked" armour, its "lock level"), if any.
    pub fn shackles_lock(&self, id: usize) -> Option<f32> {
        self.folk[id].inventory.equipped().find_map(|i| {
            let r = self.data.get(&i.record)?;
            r.bools.get("is locked").copied().unwrap_or(false).then(|| r.ints.get("lock level").copied().unwrap_or(20) as f32)
        })
    }

    /// The shackles `id` wears taken off (broken, or unlocked by its owner).
    pub fn remove_shackles(&mut self, id: usize) {
        let data = self.data.clone();
        for s in self.folk[id].inventory.sections.iter_mut() {
            s.items.retain(|p| !data.get(&p.item.record).is_some_and(|r| r.bools.get("is locked").copied().unwrap_or(false)));
        }
        self.folk[id].shackled = false;
    }

    /// F039272 "Looks like slave": how much `id` looks like an escaped slave, 0 .. 1.
    /// (stand-in: the game's own measure, a slave-state field it shows x 100, is not decompiled;
    /// its tooltip names what counts) "Wearing shackles and rags is a big giveaway": shackles on
    /// 1; else each piece of a slaving faction's "slave clothing" worn 0.3, no shirt or body
    /// armour 0.2, no trousers 0.2, a shaved head 0.25 (until the hair is restored); at least
    /// 0.5 while escaping ("Minimum (Escaping)") and 0.1 as an ex-slave ("Minimum (Ex-Slave)").
    pub fn looks_like_slave(&self, id: usize) -> f32 {
        let p = &self.folk[id];
        if self.shackles_lock(id).is_some() {
            return 1.0;
        }
        let slave_clothes: std::collections::HashSet<&String> = self
            .data
            .of_type(kenshi_data::ItemType::Faction)
            .flat_map(|f| f.refs("slave clothing").iter().map(|(s, _)| s))
            .collect();
        let worn: Vec<&kenshi_char::inventory::Item> = p.inventory.equipped().collect();
        let mut l: f32 = worn.iter().filter(|i| slave_clothes.contains(&i.record)).count() as f32 * 0.3;
        if !worn.iter().any(|i| i.slot == 8 || i.slot == 5) {
            l += 0.2;
        }
        if !worn.iter().any(|i| i.slot == 6) {
            l += 0.2;
        }
        if p.shaved {
            l += 0.25;
        }
        let floor = match &p.slave {
            Some(s) if s.fled || p.escaped.is_some() => 0.5,
            Some(_) => 0.1,
            None => 0.0,
        };
        l.max(floor).min(1.0)
    }

    /// Whether `faction` keeps slaves: it has "slave clothing" for them and is not "anti slavery".
    pub fn slaving_faction(&self, faction: &str) -> bool {
        self.data.get(faction).is_some_and(|f| !f.refs("slave clothing").is_empty() && !f.bools.get("anti slavery").copied().unwrap_or(false))
    }

    /// Escaped slaves recognised (once a second each; the stealth panel's "Discovered!", F047087:
    /// "You've been discovered, somebody has noticed that you are an escaped slave."): a slave out
    /// of its cage and away from its work, seen within 500 units by someone of its masters'
    /// faction or another slaving one, is found out on a roll against how much it looks like a
    /// slave (`looks_like_slave`); found out, it is wanted by its masters (the crime of escaping)
    /// and one who takes prisoners (a guard, a slaver) goes to take it in.
    /// (stand-in: the game's discovery test, behind the disguise state the panel reads, is not
    /// decompiled) Each look: looks x 0.5 for its masters, x 0.25 for other slavers; nothing below
    /// 0.2; not again while it is wanted or being taken.
    pub fn slave_recognition_tick(&mut self) {
        let tick = self.tick;
        for c in 0..self.folk.len() {
            if tick % 60 != (c as u64 * 11) % 60 {
                continue;
            }
            let p = &self.folk[c];
            let Some(s) = p.slave.clone() else { continue };
            // (crate::disguise) a disguised escapee is not taken for a slave
            if p.dead || p.carried_by.is_some() || p.prison.is_some() || p.disguised.is_some() || (s.obedient && p.escaped.is_none() && !s.fled) {
                continue;
            }
            if 0 < self.bounty_with(c, &s.faction) || self.folk.iter().any(|o| o.arrest.as_ref().is_some_and(|a| a.target == c)) {
                continue;
            }
            let looks = self.looks_like_slave(c);
            if looks < 0.2 {
                continue;
            }
            let at = p.mover.position_units();
            let seer = (0..self.folk.len()).find(|&w| {
                let q = &self.folk[w];
                if w == c || q.player || q.down || q.dead || q.slave.is_some() || q.prison.is_some() {
                    return false;
                }
                let Some(f) = q.faction.as_deref() else { return false };
                let r = q.mover.position_units();
                (f == s.faction || self.slaving_faction(f)) && (r[0] - at[0]).hypot(r[2] - at[2]) < 500.0 && self.can_see(w, c)
            });
            let Some(w) = seer else { continue };
            let masters = self.folk[w].faction.as_deref() == Some(s.faction.as_str());
            let roll = self.rng.uniform();
            if std::env::var("KENSHI_SLAVE_DEBUG").is_ok() {
                eprintln!("slave {c} seen by {w}: looks {looks:.2}, masters {masters}, roll {roll:.2}");
            }
            if looks * if masters { 0.5 } else { 0.25 } <= roll {
                continue;
            }
            if self.folk[c].player {
                self.messages.push("You've been discovered, somebody has noticed that you are an escaped slave.".to_string());
            }
            self.add_bounty(c, &s.faction, kenshi_ai::CrimeEnum::CrimeEscapePrison);
            if self.folk[w].arrest.is_none() && (self.folk[w].captures || self.is_guard(w)) {
                self.folk[w].arrest = Some(crate::law::Arrest { target: c, slaver: true });
                if !self.folk[c].down && !self.folk[c].surrendered {
                    self.order_attack(&[w], c);
                }
            }
        }
    }

    fn free_slave_cage(&self, faction: &str, at: [f32; 3]) -> Option<usize> {
        let taken: Vec<usize> = self.folk.iter().filter_map(|p| p.prison.as_ref().map(|q| q.cage)).collect();
        let cages: Vec<usize> = (0..self.furn.len()).filter(|&k| self.furn[k].function == BF_CAGE && !taken.contains(&k)).collect();
        let d = |k: &usize| (self.furn[*k].position[0] - at[0]).hypot(self.furn[*k].position[2] - at[2]);
        cages
            .iter()
            .filter(|&&k| self.cage_faction(k).as_deref() == Some(faction))
            .min_by(|a, b| d(a).total_cmp(&d(b)))
            .or_else(|| cages.iter().filter(|k| d(k) < 3000.0).min_by(|a, b| d(a).total_cmp(&d(b))))
            .copied()
    }

    /// The squads loaded with slaves to own (SQUAD_TEMPLATE "slaves"), their slaves made at the
    /// first tick: each slave template's people beside the squad, enslaved to it.
    pub fn spawn_owned_slaves(&mut self) {
        let pending = std::mem::take(&mut self.slave_squads);
        let data = self.data.clone();
        // they were slaves before the game began: no "has been enslaved" messages
        let said = self.messages.len();
        for (first, template) in pending {
            let Some(t) = data.get(&template) else { continue };
            let Some(owner) = self.folk.get(first).map(|p| p.platoon) else { continue };
            let faction = self.folk[first].faction.clone().or_else(|| t.refs("faction").first().map(|(f, _)| f.clone()));
            let Some(faction) = faction else { continue };
            let at = self.folk[first].mover.position_units();
            for (sid, _) in t.refs("slaves") {
                let Some(members) = self.spawn_template(sid, at) else { continue };
                for m in members {
                    self.enslave(m, owner, &faction);
                }
            }
        }
        self.messages.truncate(said);
    }

    /// The slave goal that runs now for a slave of squad `owner`: the owner's package's "Slave
    /// AI Goals" within their hours, the highest priority.
    fn slave_goal(&self, owner: Option<usize>) -> u16 {
        let Some(pkg) = owner.and_then(|o| self.folk.iter().find(|p| p.platoon == Some(o) && p.slave.is_none())).and_then(|p| p.package.clone()) else {
            return SLAVE_CAGE;
        };
        let Some(pkg) = self.data.get(&pkg) else { return SLAVE_CAGE };
        let hour = self.hour() as i32;
        let within = |a: i32, b: i32| if a <= b { a <= hour && hour < b } else { hour >= a || hour < b };
        pkg.refs("Slave AI Goals")
            .iter()
            .filter_map(|(sid, v)| {
                let task = self.data.get(sid)?.ints.get("enum").copied()? as u16;
                within(v[0], v[1]).then_some((task, if v[2] == 0 { 1.0 } else { v[2] as f32 * 0.01 }))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(SLAVE_CAGE, |(t, _)| t)
    }

    /// The town's nearest work for slave `id` at `at`: a mining machine or refinery no one else
    /// operates (the machine and where it is worked from), else a general node.
    fn slave_work_spot(&self, id: usize, at: [f32; 3]) -> Option<(Option<usize>, [f32; 3])> {
        // BuildingFunction: BF_MINE 1, BF_REFINERY 4, BF_MINE_NATURAL 27
        const BF_MINE: i32 = 1;
        const BF_REFINERY: i32 = 4;
        const BF_MINE_NATURAL: i32 = 27;
        let d = |p: [f32; 3]| (p[0] - at[0]).hypot(p[2] - at[2]);
        let taken = |k: usize| {
            self.operators.get(k).copied().flatten().is_some_and(|o| o != id)
                || self.folk.iter().any(|q| q.id != id && q.slave.as_ref().is_some_and(|s| s.machine == Some(k)))
        };
        self.furn
            .iter()
            .enumerate()
            .filter(|(k, f)| [BF_MINE, BF_MINE_NATURAL, BF_REFINERY].contains(&f.function) && d(f.position) < 3000.0 && !taken(*k))
            .map(|(k, f)| (Some(k), self.furniture_approach.get(k).copied().unwrap_or(f.position)))
            .min_by(|a, b| d(a.1).total_cmp(&d(b.1)))
            .or_else(|| {
                self.general
                    .iter()
                    .filter(|n| n.kind == kenshi_world::nodes::NODE_GENERAL && d(n.position) < 3000.0)
                    .map(|n| (None, n.position))
                    .min_by(|a, b| d(a.1).total_cmp(&d(b.1)))
            })
    }

    /// Slave `id` stops working its machine (Task_OperateMachine end: the operator slot let go).
    fn slave_stop_work(&mut self, id: usize) {
        let Some(k) = self.folk[id].slave.as_mut().and_then(|s| s.machine.take()) else { return };
        if self.folk[id].operating == Some(k) {
            self.folk[id].operating = None;
        }
        if self.operators.get(k).copied().flatten() == Some(id) {
            self.operators[k] = None;
        }
    }

    /// Slaves at work (Task_OperateMachine, F031350, for "auto worker mining"): each operating
    /// its machine makes the machine's products as an operator does (F029326: rate x production
    /// speed x skill, `operator_rate` without the player's power, resource and research) into the
    /// machine's storage, training the skill it uses.
    /// (assumption) The town's machines need no power or ingredients (a refinery's "consumes" is
    /// not taken) and keep what they make until full; a
    /// slave on "auto worker pretend" works the same machine without making anything.
    pub fn slave_work_tick(&mut self, dt: f32) {
        use crate::production::{building_rate, function_of, skill_mult, PRODUCTION_SPEED};
        let data = self.data.clone();
        for id in 0..self.folk.len() {
            let Some(s) = self.folk[id].slave.as_ref() else { continue };
            let (Some(k), SLAVE_WORK) = (s.machine, s.doing) else { continue };
            if self.folk[id].operating != Some(k) || self.folk[id].down || self.folk[id].dead {
                continue;
            }
            let Some(rec) = self.furn.get(k).and_then(|f| data.get(&f.record)) else { continue };
            let Some((f, _)) = function_of(&data, rec) else { continue };
            let stat = f.ints.get("stat used").copied().filter(|&v| v > 0).and_then(kenshi_char::StatsEnumerated::from_i32);
            let level = stat.map(|s| self.arena.f[id].level(s));
            let amount = building_rate(rec, f) * PRODUCTION_SPEED * skill_mult(level) * dt;
            if amount <= 0.0 {
                continue;
            }
            let products: Vec<(String, f32)> = f.refs("produces").iter().map(|(s, v)| (s.clone(), if v[1] > 0 { v[1] as f32 * 0.01 } else { 1.0 })).collect();
            let (w, h) = (rec.ints.get("storage size width").copied().unwrap_or(8).max(1) as u32, rec.ints.get("storage size height").copied().unwrap_or(8).max(1) as u32);
            let (made, store) = self.machine_stock.entry(k).or_insert_with(|| {
                (Vec::new(), kenshi_char::inventory::Inventory { sections: vec![kenshi_char::inventory::Section::new("main", w, h, kenshi_char::inventory::ANY)] })
            });
            if made.len() < products.len() {
                made.resize(products.len(), 0.0);
            }
            for (n, (item, per)) in products.iter().enumerate() {
                made[n] += amount * per;
                while made[n] >= 0.999 {
                    let Some(r) = data.get(item) else { break };
                    if !store.add(kenshi_char::inventory::Item::from_record(r, 1), false) {
                        made[n] = 0.999;
                        break;
                    }
                    made[n] -= 1.0;
                    if std::env::var("KENSHI_SLAVE_DEBUG").is_ok() {
                        eprintln!("slave {id} made {} at {}", r.name, rec.name);
                    }
                }
            }
            if let Some(s) = stat {
                self.arena.f[id].skills.train(s, dt * 0.05, 1.0);
            }
        }
    }

    /// Every second: each slave does its master's slave goal (see the module notes); the
    /// player's people only while obedient.
    pub fn slavery_tick(&mut self) {
        if !self.slave_squads.is_empty() {
            self.spawn_owned_slaves();
        }
        if !self.tick.is_multiple_of(60) {
            return;
        }
        for id in 0..self.folk.len() {
            let Some(s) = self.folk[id].slave.clone() else { continue };
            let p = &self.folk[id];
            if p.dead || p.down || p.carried_by.is_some() {
                continue;
            }
            // the player's slave told to go somewhere of the player's choosing gives up the job;
            // once away from its cage and its work it is escaping (the crime of escaping prison)
            if p.player && s.obedient
                && let Some(o) = p.ai.orders.first()
                && s.sent.is_none_or(|w| 5.0 < (w[0] - o.pos[0]).hypot(w[2] - o.pos[2]))
            {
                if let Some(sl) = self.folk[id].slave.as_mut() {
                    sl.obedient = false;
                }
                continue;
            }
            if !s.obedient {
                self.slave_stop_work(id);
                let p = &self.folk[id];
                let at = p.mover.position_units();
                let home = s.work_at.or_else(|| p.prison.as_ref().and_then(|pr| self.furn.get(pr.cage).map(|f| f.position)));
                if !s.fled && home.is_none_or(|h| 400.0 < (h[0] - at[0]).hypot(h[2] - at[2])) {
                    if let Some(sl) = self.folk[id].slave.as_mut() {
                        sl.fled = true;
                    }
                    self.commit_crime(id, kenshi_ai::CrimeEnum::CrimeEscapePrison, Some(&s.faction));
                }
                continue;
            }
            let goal = self.slave_goal(s.owner);
            let at = p.mover.position_units();
            if std::env::var("KENSHI_SLAVE_DEBUG").is_ok() && self.tick.is_multiple_of(600) {
                eprintln!("slave {id} of {:?}: goal {goal}, machine {:?} operating {:?}, at {:?}, work {:?}, out {:?}", s.owner, s.machine.map(|k| (k, self.furn[k].name.clone(), self.furn[k].function)), p.operating, at.map(|v| v as i32), s.work_at.map(|w| w.map(|v| v as i32)), self.folk[id].prison.as_ref().map(|p| p.out));
            }
            match goal {
                SLAVE_WORK | SLAVE_WORK_PRETEND => {
                    let (machine, spot) = match s.work_at {
                        Some(w) => (s.machine, Some(w)),
                        None => self.slave_work_spot(id, at).map_or((None, None), |(m, w)| (m, Some(w))),
                    };
                    self.let_out(id);
                    // at its machine: operating it (its operator slot, facing it)
                    if let (Some(k), Some(w)) = (machine, spot) {
                        if (w[0] - at[0]).hypot(w[2] - at[2]) < 25.0 {
                            if self.folk[id].operating != Some(k) {
                                self.order_stop(&[id]);
                                let m = self.furn[k].position;
                                let (dx, dz) = (m[0] - at[0], m[2] - at[2]);
                                let l = dx.hypot(dz);
                                if 0.1 < l {
                                    self.folk[id].facing = [dx / l, 0.0, dz / l];
                                }
                                self.folk[id].operating = Some(k);
                                self.operators[k] = Some(id);
                            }
                        } else if self.folk[id].operating == Some(k) {
                            self.folk[id].operating = None;
                        }
                    }
                    if let Some(sl) = self.folk[id].slave.as_mut() {
                        sl.machine = machine;
                    }
                    if let Some(w) = spot
                        && 40.0 < (w[0] - at[0]).hypot(w[2] - at[2])
                        && self.folk[id].ai.orders.first().is_none_or(|o| 30.0 < (o.pos[0] - w[0]).hypot(o.pos[2] - w[2]))
                    {
                        self.order_move(&[id], w);
                        if let Some(sl) = self.folk[id].slave.as_mut() {
                            sl.sent = Some(w);
                        }
                    }
                    if let Some(sl) = self.folk[id].slave.as_mut() {
                        sl.doing = goal;
                        sl.work_at = spot;
                    }
                }
                SLAVE_FOLLOW => {
                    self.slave_stop_work(id);
                    self.let_out(id);
                    let leader = s.owner.and_then(|o| self.folk.iter().find(|q| q.platoon == Some(o) && q.slave.is_none() && !q.dead)).map(|q| q.id);
                    if let Some(l) = leader
                        && self.folk[id].escort.is_none()
                    {
                        self.order_follow(&[id], l, false);
                    }
                    if let Some(sl) = self.folk[id].slave.as_mut() {
                        sl.doing = goal;
                    }
                }
                _ => {
                    self.slave_stop_work(id);
                    // back to the cage: walked there, then kept in it
                    let cage = self.folk[id].prison.as_ref().and_then(|pr| self.furn.get(pr.cage).map(|f| f.position));
                    if let Some(c) = cage {
                        if (c[0] - at[0]).hypot(c[2] - at[2]) < 40.0 {
                            if let Some(pr) = self.folk[id].prison.as_mut() {
                                pr.out = false;
                            }
                        } else if self.folk[id].prison.as_ref().is_some_and(|pr| pr.out)
                            && self.folk[id].ai.orders.first().is_none_or(|o| 30.0 < (o.pos[0] - c[0]).hypot(o.pos[2] - c[2]))
                        {
                            self.order_move(&[id], c);
                            if let Some(sl) = self.folk[id].slave.as_mut() {
                                sl.sent = Some(c);
                            }
                        }
                    }
                    if let Some(sl) = self.folk[id].slave.as_mut() {
                        sl.doing = SLAVE_CAGE;
                        sl.work_at = None;
                    }
                }
            }
        }
    }

    /// A caged slave let out (its door, `leave_cage`).
    fn let_out(&mut self, id: usize) {
        if let Some(pr) = self.folk[id].prison.clone()
            && !pr.out
        {
            self.leave_cage(id, pr.cage);
            if let Some(p) = self.folk[id].prison.as_mut() {
                p.out = true;
            }
        }
    }

    /// The player sells their character `id` to `trader`: the price paid, `id` the trader's
    /// slave (F041437, "WARNING: Character will become a slave!").
    pub fn sell_slave(&mut self, id: usize, trader: usize) -> i32 {
        let price = self.slave_sell_price(id);
        self.money += price;
        let faction = self.folk[trader].faction.clone().unwrap_or_default();
        let owner = self.folk[trader].platoon;
        let p = &mut self.folk[id];
        p.player = false;
        p.carried_by = None;
        self.enslave(id, owner, &faction);
        price
    }

    /// The player buys `id` from its owners: the price (F041440) paid, `id` joins the squad.
    pub fn buy_slave(&mut self, id: usize) -> Option<i32> {
        let price = self.slave_buy_price(id);
        if self.money < price {
            return None;
        }
        self.money -= price;
        // (assumption) a bought slave comes in its shackles and rags; its new owner can take the
        // shackles off (they hold it no longer: not `shackled`)
        self.slave_stop_work(id);
        let cage = self.folk[id].prison.as_ref().map(|pr| pr.cage);
        if let Some(c) = cage {
            self.leave_cage(id, c);
        }
        let p = &mut self.folk[id];
        p.slave = None;
        p.prison = None;
        p.shackled = false;
        self.recruit(id);
        Some(price)
    }

    /// The slaves `trader`'s faction keeps in the zone, for sale.
    pub fn slaves_for_sale(&self, trader: usize) -> Vec<usize> {
        let f = self.folk[trader].faction.clone();
        self.folk.iter().filter(|p| !p.dead && p.slave.as_ref().is_some_and(|s| Some(&s.faction) == f.as_ref())).map(|p| p.id).collect()
    }

    /// The player's people near `trader` (within 1000 units) that could be sold.
    pub fn sellable(&self, trader: usize) -> Vec<usize> {
        let at = self.folk[trader].mover.position_units();
        self.folk
            .iter()
            .filter(|p| p.player && !p.dead && {
                let q = p.mover.position_units();
                (q[0] - at[0]).hypot(q[2] - at[2]) < 1000.0
            })
            .map(|p| p.id)
            .collect()
    }
}
