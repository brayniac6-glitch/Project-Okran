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

/// SlaveStateEnum (the state object +0, F039148 / F038704): a slave; one seen escaping (for 12
/// hours, F038706); an ex-slave (for 4 days). NOT_SLAVE (0) is no `Slavery`.
pub const IS_SLAVE: u8 = 1;
pub const ESCAPING_SLAVE: u8 = 2;
pub const EX_SLAVE: u8 = 3;
/// F038706: hours escaping before an ex-slave, days an ex-slave before free.
const ESCAPING_HOURS: f64 = 12.0;
const EX_SLAVE_DAYS: f64 = 4.0;
/// F038706: a slave outside any town farther than this from its owners is escaping (units;
/// 12500000 squared).
const OWNER_REACH: f32 = 3535.534;
/// F046490: how far from its owners a slave may be sent (units; (assumption) the game's is set
/// at run time).
const SLAVE_LEASH: f32 = 250.0;
/// F038705: seconds without owners before a slave makes off.
const OWNERLESS_SECONDS: f32 = 45.0;
/// EventTriggerEnum: EV_SLAVE_ESCAPE_OPPORTUNITY_SAVIOR / _ALONE, EV_ESCAPED_PRISONER_SPOTTED.
pub const EV_SLAVE_ESCAPE_OPPORTUNITY_SAVIOR: i32 = 54;
pub const EV_SLAVE_ESCAPE_OPPORTUNITY_ALONE: i32 = 55;
pub const EV_ESCAPED_PRISONER_SPOTTED: i32 = 45;
pub const EV_BOUGHT_ME_FROM_SLAVERY: i32 = 51;
pub const EV_INTRODUCING_NEW_SLAVE: i32 = 35;
pub const EV_RECAPTURED_A_SLAVE: i32 = 37;
/// F031348: how long the processor works at a new slave before it is processed (seconds).
const PROCESSING_SECONDS: f32 = 3.0;

fn is_slave() -> u8 {
    IS_SLAVE
}

/// A slave's master.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Slavery {
    /// The owning squad (platoon, the owner handle Character +0x328) and faction (the state
    /// object +0xe0).
    pub owner: Option<usize>,
    pub faction: String,
    /// The player's people: obeying ("Obedient Slave") until given an order of their own.
    pub obedient: bool,
    /// What it is doing (a slave goal) and where its work is (units).
    pub doing: u16,
    pub work_at: Option<[f32; 3]>,
    /// Where the routine last sent it: an order elsewhere is the player's own (the job given up).
    pub sent: Option<[f32; 3]>,
    /// The machine (furniture) it works, its operator slot taken.
    pub machine: Option<usize>,
    /// Where a slave with no cage or work of its own belongs (a start's slaves: where the game
    /// began), for when it counts as away.
    #[serde(default)]
    pub home: Option<[f32; 3]>,
    /// SlaveStateEnum (`IS_SLAVE`, `ESCAPING_SLAVE`, `EX_SLAVE`), and when it last became
    /// escaping or an ex-slave (+0x8 "ssct", game hours).
    #[serde(default = "is_slave")]
    pub state: u8,
    #[serde(default)]
    pub since: f64,
    /// How long its owners have been nowhere (F038705's timer, seconds).
    #[serde(default)]
    pub ownerless: f32,
}

impl Slavery {
    /// A slave of squad `owner` of `faction`, obeying, in its cage.
    pub fn new(owner: Option<usize>, faction: &str) -> Slavery {
        Slavery { owner, faction: faction.to_string(), obedient: true, doing: SLAVE_CAGE, work_at: None, sent: None, machine: None, home: None, state: IS_SLAVE, since: 0.0, ownerless: 0.0 }
    }
}

impl Sim {
    /// SURRENDER_NON_HUMANS (see crate::dialogue): `speaker` takes the non-humans of `target`'s
    /// squad. Returns how many gave themselves up.
    pub fn surrender_non_humans(&mut self, speaker: usize, target: usize) -> usize {
        let Some(faction) = self.folk[speaker].faction.clone() else { return 0 };
        let Some(own) = self.race_of(speaker) else { return 0 };
        let at = self.folk[speaker].mover.position_units();
        let squad: Vec<usize> = self.squad_of(target).to_vec();
        let mut n = 0;
        for m in squad {
            let p = &self.folk[m];
            if p.dead || p.race.is_some() || p.slave.is_some() || p.prison.is_some() {
                continue;
            }
            let q = p.mover.position_units();
            if (q[0] - at[0]).hypot(q[2] - at[2]) > 100.0 || (p.sneaking && !self.can_see(speaker, m)) {
                continue;
            }
            if self.race_of(m).is_some_and(|r| self.same_race_group(&r, &own)) {
                continue;
            }
            let owner = self.folk[speaker].platoon;
            self.folk[m].slave = Some(Slavery::new(owner, &faction));
            self.begin_processing(m, &faction, speaker);
            self.folk[m].ai.queue.clear();
            self.folk[m].ai.orders.clear();
            self.folk[m].escort = Some((speaker, false));
            let name = self.display_name(m);
            self.messages.push(format!("{name} was handed over as a slave."));
            n += 1;
        }
        n
    }

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
        self.make_slave(id, owner, faction);
        match cage {
            Some(c) => self.imprison(id, c, faction, true),
            None => self.folk[id].shackled = true,
        }
    }

    /// Person `id` made a slave of squad `owner` of `faction` where it is (no cage).
    pub fn make_slave(&mut self, id: usize, owner: Option<usize>, faction: &str) {
        let player = self.folk[id].player;
        if !player {
            self.folk[id].faction = Some(faction.to_string());
            let side = self.side(&Some(faction.to_string()));
            self.arena.f[id].team = side;
            // its own goals give way to its master's (the slave goals)
            self.folk[id].ai.jobs.iter_mut().for_each(|j| j.clear());
            self.folk[id].ai.goals.clear();
        }
        // processed by the nearest of its owners (within 300 units)
        let at = self.folk[id].mover.position_units();
        let taker = owner.and_then(|o| {
            self.folk
                .iter()
                .filter(|q| q.platoon == Some(o) && q.id != id && !q.dead && !q.down && q.slave.is_none())
                .map(|q| {
                    let r = q.mover.position_units();
                    (q.id, (r[0] - at[0]).hypot(r[2] - at[2]))
                })
                .filter(|x| x.1 < 300.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|x| x.0)
        });
        self.folk[id].slave = Some(Slavery::new(owner, faction));
        match taker {
            Some(t) => self.begin_processing(id, faction, t),
            None => self.dress_as_slave(id, faction),
        }
    }

    /// F030712's shackles (put on as `id` is enslaved, when it wears none) and Task_ProcessNewSlave
    /// begun by `taker`: it stands at it for 3 s before the processing (`processing_tick`).
    pub(crate) fn begin_processing(&mut self, id: usize, faction: &str, taker: usize) {
        self.put_on_shackles(id);
        self.order_stop(&[taker]);
        self.law_state.processing.push(crate::law::NewSlave { taker, slave: id, faction: faction.to_string(), left: PROCESSING_SECONDS });
    }

    /// @fn F031348
    ///
    /// Task_ProcessNewSlave's time (its first 3 s, the processor in the "medic" pose): the new
    /// slave processed when it is up (`process_new_slave`) and the processor's line,
    /// EV_INTRODUCING_NEW_SLAVE, or EV_RECAPTURED_A_SLAVE for one already shaved (Character
    /// vtable +0x3e0); the processing goes on without its processor when it is down or gone.
    /// The processor plays "medic" meanwhile (`law_task_clip`).
    pub fn processing_tick(&mut self, dt: f32) {
        let mut k = 0;
        while k < self.law_state.processing.len() {
            let e = self.law_state.processing[k].clone();
            let gone = self.folk[e.taker].dead || self.folk[e.taker].down || self.folk[e.slave].dead;
            let left = e.left - dt;
            if !gone && left > 0.0 {
                self.law_state.processing[k].left = left;
                k += 1;
                continue;
            }
            self.law_state.processing.remove(k);
            // (one dead, or no longer a slave held, has got away from it)
            if self.folk[e.slave].dead || self.slave_state(e.slave) != IS_SLAVE {
                continue;
            }
            let recaptured = self.folk[e.slave].shaved;
            let taker = (!gone).then_some(e.taker);
            self.process_new_slave(e.slave, &e.faction, taker);
            if let Some(t) = taker {
                self.bark(t, e.slave, if recaptured { EV_RECAPTURED_A_SLAVE } else { EV_INTRODUCING_NEW_SLAVE });
            }
        }
    }

    /// F039148: `id`'s SlaveStateEnum (0 not a slave).
    pub fn slave_state(&self, id: usize) -> u8 {
        self.folk.get(id).and_then(|p| p.slave.as_ref()).map_or(0, |s| s.state)
    }

    /// @fn F038704
    ///
    /// `id`'s slave state set: escaping blows its disguise for 250 s ("Discovered!"); a change to
    /// escaping or ex-slave stamps the time; 0 frees it (no master).
    pub fn set_slave_state(&mut self, id: usize, state: u8) {
        if state == ESCAPING_SLAVE {
            self.folk[id].disguise_blown = crate::disguise::BLOWN_SECONDS;
            self.folk[id].disguised = None;
        }
        let now = self.now_hours();
        if state == 0 {
            self.folk[id].slave = None;
            return;
        }
        let Some(s) = self.folk[id].slave.as_mut() else { return };
        if s.state != state && state > IS_SLAVE {
            s.since = now;
        }
        s.state = state;
        if state > IS_SLAVE {
            s.obedient = false;
        }
        if std::env::var("KENSHI_SLAVE_DEBUG").is_ok() {
            eprintln!("slave {id}: state {state}");
        }
    }

    /// DA_TAG_ESCAPED_SLAVE (F040912 0x19: setSlaveState 2): `id` taken for an escaping slave;
    /// (assumption) one that was no slave becomes the speaker's (`faction`, squad `owner`), as
    /// the slave state needs a master here.
    pub fn tag_escaped_slave(&mut self, id: usize, faction: &str, owner: Option<usize>) {
        if self.folk[id].slave.is_none() {
            self.folk[id].slave = Some(Slavery { obedient: false, ..Slavery::new(owner, faction) });
        }
        self.set_slave_state(id, ESCAPING_SLAVE);
    }

    /// A new slave's clothes with no one processing it (`process_new_slave`, what is taken
    /// left on the ground).
    pub fn dress_as_slave(&mut self, id: usize, faction: &str) {
        self.put_on_shackles(id);
        self.process_new_slave(id, faction, None);
    }

    /// @fn F031348
    /// @fn F031166
    /// @fn F031332
    ///
    /// A new slave of `faction` processed (Task_ProcessNewSlave) by `taker`: unless the faction
    /// "allow slaves weapons" (Faction +0x30) its weapons and items are taken (F031348's first
    /// and third pass), and its armour that is not the faction's "slave clothing" and its
    /// containers (F031166, item types 3 and 46), each into the taker's inventory (its
    /// backpack first); locked things (the shackles) and a slave's hat (the hat slot, "stigma"
    /// 6) stay. With no trousers on (the "legs" section empty) it is given the slave clothing,
    /// each piece in its slot (F031332). Its head is shaved. (The shackles went on as it was
    /// enslaved, `put_on_shackles`.)
    /// (assumption) Crossbows go with the weapons; what the taker has no room for, or with no
    /// taker, is left on the ground; the shaving (the appearance's +0x168) is done here.
    /// The processor's 3 s and its line are `processing_tick`'s.
    pub fn process_new_slave(&mut self, id: usize, faction: &str, taker: Option<usize>) {
        use kenshi_data::ItemType;
        let data = self.data.clone();
        let clothing: Vec<String> = data.get(faction).map(|f| f.refs("slave clothing").iter().map(|(s, _)| s.clone()).collect()).unwrap_or_default();
        let weapons_allowed = data.get(faction).is_some_and(|f| f.bools.get("allow slaves weapons").copied().unwrap_or(false));
        let locked = |i: &kenshi_char::inventory::Item| data.get(&i.record).is_some_and(|r| r.bools.get("is locked").copied().unwrap_or(false));
        let slave_hat = |i: &kenshi_char::inventory::Item| i.slot == 3 && data.get(&i.record).and_then(|r| r.ints.get("stigma").copied()) == Some(6);
        let taken_kind = |i: &kenshi_char::inventory::Item| match i.kind {
            Some(ItemType::Weapon) | Some(ItemType::Crossbow) | Some(ItemType::Item) => !weapons_allowed,
            Some(ItemType::Armour) => !clothing.contains(&i.record),
            Some(ItemType::Container) => true,
            _ => false,
        };
        let mut taken = Vec::new();
        for sec in self.folk[id].inventory.sections.iter_mut() {
            let mut k = 0;
            while k < sec.items.len() {
                let i = &sec.items[k].item;
                if taken_kind(i) && !locked(i) && !slave_hat(i) {
                    taken.push(sec.items.remove(k).item);
                } else {
                    k += 1;
                }
            }
        }
        for item in taken {
            let kept = taker.is_some_and(|t| self.folk[t].inventory.add(item.clone(), false));
            if !kept {
                self.drop_item(id, item);
            }
        }
        let bare_legs = self.folk[id].inventory.sections.iter().find(|s| s.slot == 6 && s.is_equipment()).is_none_or(|s| s.items.is_empty());
        if bare_legs {
            for sid in &clothing {
                let Some(r) = data.get(sid) else { continue };
                let item = if r.item_type() == Some(ItemType::Armour) { kenshi_char::inventory::Item::armour(&data, r, 1) } else { kenshi_char::inventory::Item::from_record(r, 1) };
                let worn = self.folk[id].inventory.sections.iter().any(|s| s.is_equipment() && s.slot == item.slot && !s.items.is_empty());
                if !worn {
                    let _ = self.folk[id].inventory.add(item, true);
                }
            }
        }
        self.folk[id].shaved = true;
    }

    /// @fn F030712
    ///
    /// Shackles put on `id` when it wears none: "SHACKLES" while the boots section is enabled
    /// (F040014), else "SHACKLES-BODY"; whatever is worn in their place is taken off and left
    /// on the ground.
    pub fn put_on_shackles(&mut self, id: usize) {
        if self.shackles_lock(id).is_none() {
            let shackles = if self.boots_enabled(id) { "SHACKLES" } else { "SHACKLES-BODY" };
            if let Some(r) = self.data.get(shackles) {
                let item = kenshi_char::inventory::Item::armour(&self.data, r, 1);
                let worn: Vec<kenshi_char::inventory::Item> = self.folk[id].inventory.sections.iter_mut().filter(|s| s.is_equipment() && s.slot == item.slot).flat_map(|s| s.items.drain(..).map(|p| p.item)).collect();
                for w in worn {
                    self.drop_item(id, w);
                }
                let _ = self.folk[id].inventory.add(item, true);
            }
        }
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

    /// "Looks like slave" (the state object +0xf4, F038707: crate::disguise's `outfit`), 0 .. 1.
    pub fn looks_like_slave(&self, id: usize) -> f32 {
        self.outfit(id).looks
    }

    /// Whether `faction` keeps slaves: it has "slave clothing" for them and is not "anti slavery".
    pub fn slaving_faction(&self, faction: &str) -> bool {
        self.data.get(faction).is_some_and(|f| !f.refs("slave clothing").is_empty() && !f.bools.get("anti slavery").copied().unwrap_or(false))
    }

    /// Where `id`'s owners are (F038706's owner object: the owning squad's first standing member,
    /// else where it belongs), units.
    fn owner_position(&self, id: usize) -> Option<[f32; 3]> {
        let s = self.folk[id].slave.as_ref()?;
        let owner = s.owner.and_then(|o| self.folk.iter().find(|q| q.platoon == Some(o) && q.slave.is_none() && !q.dead && q.id != id));
        owner.map(|q| q.mover.position_units()).or(s.home)
    }

    /// The members of `id`'s owning squad (living, not slaves).
    fn owners_of(&self, id: usize) -> Vec<usize> {
        let Some(o) = self.folk[id].slave.as_ref().and_then(|s| s.owner) else { return Vec::new() };
        self.folk.iter().filter(|q| q.platoon == Some(o) && q.slave.is_none() && !q.dead && q.id != id).map(|q| q.id).collect()
    }

    /// @fn F038706
    /// @fn F038705
    ///
    /// Every second (each think): the slave states run on. Escaping turns ex-slave after 12
    /// hours and an ex-slave is free after 4 days (F038706). A slave outside any town (the
    /// state object +0x88) farther than 3536 units from its owners is escaping (F038706). A
    /// slave, awake and not held in a cage, whose owners are nowhere for 45 s makes off
    /// (escaping, EV_SLAVE_ESCAPE_OPPORTUNITY_ALONE); one whose owners are all down makes off
    /// with EV_SLAVE_ESCAPE_OPPORTUNITY_SAVIOR said to whoever downed them, or alone when no one
    /// did (F038705, F044493).
    /// (assumption) The owner object is the owning squad (Kenshi's owner handle, Character
    /// +0x328, names a character, platoon or building); the savior is whoever has one of the
    /// fallen as its combat target.
    pub fn slave_states_tick(&mut self, dt: f32) {
        let now = self.now_hours();
        for id in 0..self.folk.len() {
            let Some(s) = self.folk[id].slave.clone() else { continue };
            if self.folk[id].dead {
                continue;
            }
            if s.state > IS_SLAVE {
                let gone = now - s.since;
                if gone < 0.0 {
                    self.set_slave_state(id, 0);
                } else if s.state == EX_SLAVE && gone / 24.0 > EX_SLAVE_DAYS {
                    self.set_slave_state(id, 0);
                } else if s.state == ESCAPING_SLAVE && gone > ESCAPING_HOURS {
                    if let Some(sl) = self.folk[id].slave.as_mut() {
                        sl.state = EX_SLAVE;
                        sl.since = now;
                    }
                }
                continue;
            }
            // F038706: away from its owners, outside any town
            let at = self.folk[id].mover.position_units();
            if self.folk[id].town.is_none()
                && self.folk[id].carried_by.is_none()
                && let Some(o) = self.owner_position(id)
                && OWNER_REACH < (o[0] - at[0]).hypot(o[2] - at[2])
            {
                self.set_slave_state(id, ESCAPING_SLAVE);
                continue;
            }
            // F038705: the owners gone, or all down
            let p = &self.folk[id];
            if p.down || p.prison.as_ref().is_some_and(|pr| !pr.out) || s.owner.is_none() {
                continue;
            }
            let owners = self.owners_of(id);
            if owners.is_empty() {
                let left = s.ownerless + dt;
                if let Some(sl) = self.folk[id].slave.as_mut() {
                    sl.ownerless = left;
                }
                if left > OWNERLESS_SECONDS {
                    self.set_slave_state(id, ESCAPING_SLAVE);
                    self.bark(id, id, EV_SLAVE_ESCAPE_OPPORTUNITY_ALONE);
                }
                continue;
            }
            if let Some(sl) = self.folk[id].slave.as_mut() {
                sl.ownerless = 0.0;
            }
            if owners.iter().any(|&o| !self.folk[o].down && !self.folk[o].away) {
                continue;
            }
            let savior = (0..self.folk.len()).find(|&k| {
                let q = &self.folk[k];
                !q.dead && !q.down && self.arena.f[k].focus.is_some_and(|f| owners.contains(&f)) && !q.faction.as_deref().and_then(|f| self.data.get(f)).is_some_and(|f| f.bools.get("not real").copied().unwrap_or(false))
            });
            self.set_slave_state(id, ESCAPING_SLAVE);
            match savior {
                Some(k) => self.bark(id, k, EV_SLAVE_ESCAPE_OPPORTUNITY_SAVIOR),
                None => self.bark(id, id, EV_SLAVE_ESCAPE_OPPORTUNITY_ALONE),
            };
        }
    }

    /// @fn F046526
    /// @fn F046490
    ///
    /// Whether slave `id` behaves like one getting away as `observer` sees it (F046534 asks it
    /// of a slave with an owner, for an observer of a faction neither the player's nor "anti
    /// slavery"): escaping; sneaking (F039177, not in a fight) out of a cage; out of its
    /// shackles (F039147: no locked thing worn on the feet or the body); walking away from its
    /// owners while not doing its obedient slave work (F046489, job 187): running away
    /// ("Running away" 35, "Retreating fast" 127), or walking to an order beyond `SLAVE_LEASH`
    /// units from its owners and farther than it is, not heading to them, unless indoors near
    /// them (F046490); fighting one of the observer's faction or of one it holds an ally
    /// (F050246: relation 50 or better).
    /// (assumption) F046490's leash is a value set at run time (0x141fedd00): 250 units here; an
    /// NPC's slave counts as obeying. (not ported) F046526's tests for an owner that is no
    /// character or squad (a building).
    pub(crate) fn slave_out_of_place(&self, observer: usize, id: usize) -> bool {
        let p = &self.folk[id];
        let caged = p.prison.as_ref().is_some_and(|pr| !pr.out);
        let fighting = self.arena.f[id].active;
        if self.slave_state(id) == ESCAPING_SLAVE || (p.sneaking && !fighting && !caged) {
            return true;
        }
        if self.shackles_lock(id).is_none() {
            return true;
        }
        let obeying = p.slave.as_ref().is_some_and(|s| s.obedient);
        if !obeying && self.slave_walking_away(id) {
            return true;
        }
        if fighting {
            let theirs = self.arena.f[id].focus.and_then(|f| self.faction_id(f));
            let mine = self.folk[observer].faction.as_deref();
            return theirs.is_some_and(|t| Some(t.as_str()) == mine || mine.and_then(|m| self.relations.get(m)).is_some_and(|r| r.is_ally(Some(&t))));
        }
        false
    }

    /// F046490: slave `id` walking away from its owners (see `slave_out_of_place`).
    fn slave_walking_away(&self, id: usize) -> bool {
        let p = &self.folk[id];
        if p.ai.current.as_ref().is_some_and(|t| t.task == 35 || t.task == 127) {
            return true;
        }
        let (Some(o), Some(order)) = (self.owner_position(id), p.ai.orders.first()) else { return false };
        let at = p.mover.position_units();
        let d2 = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2);
        let (to_dest, to_me) = (d2(order.pos, o), d2(at, o));
        if to_dest <= SLAVE_LEASH * SLAVE_LEASH || to_dest <= to_me {
            return false;
        }
        let heading = p.facing[0] * (o[0] - at[0]) + p.facing[2] * (o[2] - at[2]) > 0.0;
        !heading && (p.inside.is_none() || 3.0 * SLAVE_LEASH * SLAVE_LEASH < to_me)
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

    /// @fn F040051
    ///
    /// A character made from a CHARACTER with a "slave" state (the Slaves starts' "start slave")
    /// is a slave from the start: Kenshi sets its slave state (F040051 -> setSlaveState,
    /// F038704: 1 a slave, 2 escaped) as the character is made, without dressing
    /// it (its record already wears the shackles) and, before the game has begun, without the
    /// "{1} has been enslaved" message. They are obedient slaves of the nearest squad of a
    /// slaving faction (Rebirth's for the Slaves start), working for it until given an order of
    /// their own.
    /// (assumption: Kenshi's slave state names no master; the port's slaves need one for their
    /// work, so the nearest slaving faction's squad within 3000 units is taken, one whose AI
    /// package has "Slave AI Goals" first)
    pub fn start_slaves(&mut self) {
        let data = self.data.clone();
        // (harness: the town's people are made over the first seconds) tried each second until
        // every one has a master, for a minute at most
        let mut waiting = false;
        for id in 0..self.folk.len() {
            let p = &self.folk[id];
            // (a Kenshi save's slave comes with its state and no master: crate::kenshi_save)
            let imported = p.slave.as_ref().filter(|s| s.faction.is_empty()).map(|s| s.state as i32);
            if (p.slave.is_some() && imported.is_none()) || p.dead {
                continue;
            }
            let record = p.character.as_ref().and_then(|c| data.get(c)).map_or(0, |c| c.ints.get("slave").copied().unwrap_or(0));
            let state = imported.unwrap_or(record);
            if state <= 0 {
                continue;
            }
            let at = p.mover.position_units();
            let has_goals = |m: &crate::sim::Person| m.package.as_ref().and_then(|k| data.get(k)).is_some_and(|k| !k.refs("Slave AI Goals").is_empty());
            let master = self
                .folk
                .iter()
                .filter(|m| !m.player && !m.dead && m.slave.is_none() && m.prison.is_none())
                .filter(|m| m.faction.as_deref().is_some_and(|f| self.slaving_faction(f)))
                .map(|m| {
                    let q = m.mover.position_units();
                    (m.id, (q[0] - at[0]).hypot(q[2] - at[2]), has_goals(m))
                })
                .filter(|m| m.1 < 3000.0)
                .min_by(|a, b| b.2.cmp(&a.2).then(a.1.total_cmp(&b.1)));
            let Some((m, _, _)) = master else {
                waiting = true;
                continue;
            };
            let faction = self.folk[m].faction.clone().unwrap_or_default();
            let owner = self.folk[m].platoon;
            if std::env::var("KENSHI_SLAVE_DEBUG").is_ok() {
                eprintln!("start slave {id}: of {} ({faction}), squad {owner:?}, package {:?}", self.display_name(m), self.folk[m].package.as_ref().and_then(|k| data.get(k)).map(|k| k.name.clone()));
            }
            // (an imported one keeps the save's disguise blown time and its stamp, "ssct")
            let blown = self.folk[id].disguise_blown;
            let since = self.folk[id].slave.as_ref().map_or(0.0, |s| s.since);
            self.folk[id].slave = Some(Slavery { home: Some(at), ..Slavery::new(owner, &faction) });
            if state > 1 {
                self.set_slave_state(id, state.min(3) as u8);
            }
            if imported.is_some() {
                self.folk[id].disguise_blown = blown;
                if let Some(s) = self.folk[id].slave.as_mut() {
                    s.since = since;
                }
            }
            self.folk[id].shackled = self.shackles_lock(id).is_some();
            // (the squad's arrival order is not an order of the player's: obeying from the start)
            self.folk[id].ai.orders.clear();
            self.folk[id].ai.queue.clear();
        }
        self.start_slaves = waiting && self.tick < 3600;
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
        // (processed already: at once, with no processing time or lines)
        for e in std::mem::take(&mut self.law_state.processing) {
            self.process_new_slave(e.slave, &e.faction, Some(e.taker));
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
        self.processing_tick(dt);
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
        if self.start_slaves && self.tick.is_multiple_of(60) {
            self.start_slaves();
        }
        if self.tick.is_multiple_of(60) {
            self.slave_states_tick(1.0);
            self.catch_loose_slaves();
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
                if std::env::var("KENSHI_SLAVE_DEBUG").is_ok() {
                    eprintln!("slave {id} disobeys: order {:?} at {:?}, sent {:?}", o.task, o.pos, s.sent);
                }
                if let Some(sl) = self.folk[id].slave.as_mut() {
                    sl.obedient = false;
                }
                continue;
            }
            // one that gave up the job, or no longer a slave (escaping, an ex-slave), works no more
            // (getting away is `slave_states_tick`'s and the NPCs' look's, crate::intruders)
            if !s.obedient || s.state != IS_SLAVE {
                self.slave_stop_work(id);
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
                    // "Getting in my cage" (172) with no cage yet (a start's slaves, F031221
                    // Task_UseCage): the nearest free cage of its masters, walked to (its door)
                    // and got into (F031340: the cage used, its occupant held)
                    if self.folk[id].prison.is_none()
                        && let Some(k) = self.free_slave_cage(&s.faction, at)
                    {
                        let door = self.cage_exit(k);
                        if (door[0] - at[0]).hypot(door[2] - at[2]) < 40.0 {
                            self.slave_into_cage(id, k, &s.faction);
                        } else if self.folk[id].ai.orders.first().is_none_or(|o| 30.0 < (o.pos[0] - door[0]).hypot(o.pos[2] - door[2])) {
                            self.order_move(&[id], door);
                            if let Some(sl) = self.folk[id].slave.as_mut() {
                                sl.sent = Some(door);
                            }
                        }
                    }
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

/// A slave getting into cage `cage` of its masters (`faction`) of its own accord (Task_UseCage):
    /// held in it as a slave is, the cage's lock its faction's; no message (it was a slave
    /// already).
    pub fn slave_into_cage(&mut self, id: usize, cage: usize, faction: &str) {
        let rec = self.data.get(faction);
        let level = rec.and_then(|r| r.ints.get("cages lock level").copied()).unwrap_or(30) as f32;
        let random = rec.and_then(|r| r.ints.get("lock level random").copied()).unwrap_or(0) as f32;
        let lock = level + (self.rng.uniform() * 2.0 - 1.0) * random;
        let at = self.furn[cage].position;
        let p = &mut self.folk[id];
        p.mover.position = at.map(|v| v * 0.1);
        self.arena.f[id].mover.position = p.mover.position;
        p.ai.orders.clear();
        p.ai.queue.clear();
        p.prison = Some(crate::law::Prison { cage, faction: faction.to_string(), until: f64::MAX, slave: true, lock: crate::law::Lock { level: lock, progress: 0.0, next: 3.0 }, out: false });
        if std::env::var("KENSHI_SLAVE_DEBUG").is_ok() {
            eprintln!("slave {id} gets into cage {cage}");
        }
    }

    /// "Capture Slaves" (167, its target isChained, F038662: shackled): the masters' cage
    /// wardens (catchers who watch slaves) beat a slave of theirs loose outside a cage while its
    /// masters want it caged and it is not on its way there (not an obedient slave: the start's
    /// "Get into a cage straight away to avoid a beating"), within 1500 units, then carry it to a
    /// cage (the arrest's slaver catch, crate::law).
    pub fn catch_loose_slaves(&mut self) {
        for w in 0..self.folk.len() {
            let p = &self.folk[w];
            if !p.captures || p.duties & crate::sim::DUTY_WATCHES_SLAVES == 0 || p.player || p.down || p.dead || p.slave.is_some() || p.prison.is_some() {
                continue;
            }
            if p.arrest.is_some() || p.carrying.is_some() || p.pickup.is_some() {
                continue;
            }
            let Some(f) = p.faction.clone() else { continue };
            let at = p.mover.position_units();
            let loose = self
                .folk
                .iter()
                .filter(|q| !q.dead && q.shackled && q.carried_by.is_none())
                .filter(|q| q.prison.as_ref().is_none_or(|pr| pr.out))
                .filter(|q| q.slave.as_ref().is_some_and(|s| s.faction == f && !s.obedient && self.slave_goal(s.owner) == SLAVE_CAGE))
                .filter(|q| !self.folk.iter().any(|o| o.arrest.as_ref().is_some_and(|a| a.target == q.id)))
                .map(|q| (q.id, q.mover.position_units()))
                .map(|(id, q)| (id, (q[0] - at[0]).hypot(q[2] - at[2])))
                .filter(|(_, d)| *d < 1500.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id);
            if let Some(c) = loose {
                if std::env::var("KENSHI_SLAVE_DEBUG").is_ok() {
                    eprintln!("warden {w} beats loose slave {c}");
                }
                self.folk[w].arrest = Some(crate::law::Arrest { target: c, slaver: true, to: None });
                if !self.folk[c].down {
                    self.order_attack(&[w], c);
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

    /// @fn F041439
    ///
    /// The player sells their character `id` to `trader` (F041437, "WARNING: Character will
    /// become a slave!"): the price paid, `id` enslaved to the trader (F030712), the trader's
    /// faction's relation with the player's up 4.
    /// (stand-in) The new slave is caged and dressed at once (Kenshi's slavers do it after, by
    /// Task_ProcessNewSlave and Task_UseCage); the sold one's squad's tag of the trader
    /// (F040853, 5) is not kept.
    pub fn sell_slave(&mut self, id: usize, trader: usize) -> i32 {
        let price = self.slave_sell_price(id);
        self.money += price;
        let faction = self.folk[trader].faction.clone().unwrap_or_default();
        let owner = self.folk[trader].platoon;
        let p = &mut self.folk[id];
        p.player = false;
        p.carried_by = None;
        self.enslave(id, owner, &faction);
        self.affect_relation(&faction, 4.0);
        price
    }

    /// @fn F041463
    ///
    /// The player buys `id` from its owners (CharacterTrading_SlaveBuying): the price (F041440)
    /// paid; it has no owner and is no slave (setSlaveState 0), out of its cage and released
    /// (F030719, the state object +0xea), its cage's and its shackles' locks open (it keeps the
    /// shackles on, no longer held by them); it thanks the buyer (EV_BOUGHT_ME_FROM_SLAVERY);
    /// the sellers' relation with the player's up 1 and its own faction's up 4.
    /// (assumption) It joins the player's squad (F040016 / the order 0x85 that follow are not
    /// decompiled); the buyer is the player's character nearest it. (port) An NPC slave here
    /// has taken its owners' faction: its own is not raised twice.
    pub fn buy_slave(&mut self, id: usize) -> Option<i32> {
        let price = self.slave_buy_price(id);
        if self.money < price {
            return None;
        }
        self.money -= price;
        self.slave_stop_work(id);
        let cage = self.folk[id].prison.as_ref().map(|pr| pr.cage);
        if let Some(c) = cage {
            self.leave_cage(id, c);
        }
        let sellers = self.folk[id].slave.as_ref().map(|s| s.faction.clone());
        let own = self.faction_id(id);
        let at = self.folk[id].mover.position_units();
        let buyer = self
            .folk
            .iter()
            .filter(|q| q.player && !q.dead && q.id != id)
            .min_by(|a, b| {
                let d = |q: &crate::sim::Person| {
                    let r = q.mover.position_units();
                    (r[0] - at[0]).hypot(r[2] - at[2])
                };
                d(a).total_cmp(&d(b))
            })
            .map(|q| q.id);
        let p = &mut self.folk[id];
        p.slave = None;
        p.prison = None;
        p.shackled = false;
        p.escaped = None;
        if let Some(b) = buyer {
            self.bark(id, b, EV_BOUGHT_ME_FROM_SLAVERY);
        }
        if let Some(f) = &sellers {
            self.affect_relation(f, 1.0);
        }
        if let Some(f) = own.filter(|f| Some(f) != sellers.as_ref()) {
            self.affect_relation(&f, 4.0);
        }
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
