//! A campaign's victory (port_map `world`): once none of the player's people stand, a hostile
//! campaign's squads play out its "victory AI leader" package (FACTION_CAMPAIGN, through
//! "inherits from") instead of all going home. By the package's tasks:
//! - capturing ("Capture Slaves" 167, "Find and kidnap (with cages) urgent" 254, "Find cage and
//!   put someone in it" 112: the slavers' "Slaver deliver to nearest depot", the cannibals'
//!   "Cannibal Raid Retreat"): its people pick up the player's knocked out (`capture_tick`) and
//!   carry them off home, where they are kept shackled (`law`: a cage of the faction's on
//!   the way takes them first); slavers shipping their catch (SlaverPrisonerShipping, F028970 /
//!   F029037) take it to the nearest slave camp, or a nearer town with a slave shop, and hand
//!   it over there (EV_SLAVE_DELIVERY);
//! - occupying ("Static guard- town" 157, "Man the turrets" 149: "campaign victory occupy
//!   town"): it stays at the base, fighting whoever of the player's comes back; its people run
//!   the package's jobs, its "get food" (Getting food, 243) taking the base's food (`sim`); its
//!   package starting, each says so (EV_TAKEN_OVER_PLAYER_TOWN, F029010);
//! - looting food ("Looting food" 131, in the victory package or the assault's, "pt2 AI
//!   leader": "Assault Town and loot food"): its people run the package's jobs, Looting food
//!   taking the food from the player's storage near them (`sim`); once they carry it (or none
//!   is left) it leaves; an assault package that does not fight ("Loot food then run away (non
//!   violent)", the Shek's tributes) does so as soon as it is there, its campaign won;
//! - else ("Retreat and refresh", "Casual Raid Retreat") it goes home.
//!
//! (stand-in) The capturing packages' task handlers are not run: these are their outcomes. A captive
//! carried by a squad that goes off-screen goes with it (hidden) and comes back carried when the
//! squad is met again; reaching home (or its depot) with them, the squad leaves them as its
//! faction's (the depot's) shackled slaves at that town's centre (its cages are not loaded
//! there). (assumption) The cannibals' captives are kept as slaves too (the pot is not ported).
use crate::sim::Sim;
use crate::world::{Goal, campaign_refs};
use kenshi_data::GameData;

/// What a won campaign's squads do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Victory {
    #[default]
    Leave,
    Capture,
    Occupy,
    LootFood,
}

/// The victory package tasks (AI_TASK "enum").
const CAPTURE_SLAVES: i32 = 167;
const KIDNAP_WITH_CAGES: i32 = 254;
const PUT_IN_CAGE: i32 = 112;
const STATIC_GUARD_TOWN: i32 = 157;
const MAN_TURRETS: i32 = 149;
const LOOTING_FOOD: i32 = 131;
const ATTACK_ENEMIES: i32 = 16;
/// AI_PACKAGE "signal func" SIG_OCCUPY_CONQUERED_TOWN (Package_OccupyTown).
const SIG_OCCUPY_CONQUERED_TOWN: i32 = 30;
/// EventTriggerEnum EV_TAKEN_OVER_PLAYER_TOWN.
pub const EV_TAKEN_OVER_PLAYER_TOWN: i32 = 73;
/// AI_PACKAGE "signal func" SIG_SLAVER_SHIP_TO_WORKCAMPS (SlaverPrisonerShipping_Camps) and
/// SIG_SLAVER_SHIP_TO_ANYWHERE_CLOSE (SlaverPrisonerShipping_AnywhereTownsIncluded: "Slaver
/// deliver to nearest depot").
const SIG_SLAVER_SHIP_TO_WORKCAMPS: i32 = 11;
const SIG_SLAVER_SHIP_TO_ANYWHERE_CLOSE: i32 = 28;
/// TownType TOWN_TOWN, TOWN_SLAVE_CAMP; BuildingDesignation BD_SLAVE_STORAGE.
const TOWN_TOWN: i32 = 2;
const TOWN_SLAVE_CAMP: i32 = 5;
const BD_SLAVE_STORAGE: i32 = 10;
/// F029037: a town taken over the camp only nearer than 0.7 of the camp's distance squared
/// (0x1416ae650).
const DEPOT_NEARER: f32 = 0.7;
/// EventTriggerEnum EV_SLAVE_DELIVERY (Selling slaves, 194 / 218: TaskDef +0x130).
pub const EV_SLAVE_DELIVERY: i32 = 39;
/// (stand-in) How near the depot's centre its receiver is looked for (the finder F047544 is
/// not ported): its radius as a town (F048768).
const RECEIVER_RANGE: f32 = 2500.0;
/// (assumption) How far from the target the player's storage is looted.
const LOOT_RANGE: f32 = 3000.0;
/// (assumption) How far from its home town's centre a cage takes a raid's captives.
const HOME_CAGE_RANGE: f32 = 3000.0;

/// The tasks (AI_TASK "enum") of campaign `c`'s package of phase `phase` ("pt2 AI leader",
/// "victory AI leader").
fn phase_tasks(data: &kenshi_data::GameDataContainer, c: &GameData, phase: &str) -> Vec<i32> {
    campaign_refs(data, c, phase)
        .iter()
        .filter_map(|(p, _)| data.get(p))
        .flat_map(|p| p.refs("Leader AI Goals").iter().chain(p.refs("Squad AI Goals")).cloned().collect::<Vec<_>>())
        .filter_map(|(t, _)| data.get(&t).and_then(|t| t.ints.get("enum").copied()))
        .collect()
}

/// The victory of campaign `c`: by its "victory AI leader" package's tasks; one that only goes
/// home loots food first when its assault ("pt2 AI leader") loots food ("Assault Town and loot
/// food").
pub fn victory_of(data: &kenshi_data::GameDataContainer, c: &GameData) -> Victory {
    let tasks = phase_tasks(data, c, "victory AI leader");
    // (an occupation's package also cages people: it stays)
    if tasks.iter().any(|t| [STATIC_GUARD_TOWN, MAN_TURRETS].contains(t)) {
        Victory::Occupy
    } else if tasks.iter().any(|t| [CAPTURE_SLAVES, KIDNAP_WITH_CAGES, PUT_IN_CAGE].contains(t)) {
        Victory::Capture
    } else if tasks.contains(&LOOTING_FOOD) || phase_tasks(data, c, "pt2 AI leader").contains(&LOOTING_FOOD) {
        Victory::LootFood
    } else {
        Victory::Leave
    }
}

/// Whether campaign `c`'s assault ("pt2 AI leader") loots food without fighting ("Attack
/// enemies", 16): "Loot food then run away (non violent)", the Shek's tributes; it takes what
/// it came for as soon as it is there.
pub fn loots_without_fighting(data: &kenshi_data::GameDataContainer, c: &GameData) -> bool {
    let tasks = phase_tasks(data, c, "pt2 AI leader");
    tasks.contains(&LOOTING_FOOD) && !tasks.contains(&ATTACK_ENEMIES)
}

impl Sim {
    /// Squad `k`'s campaign has won: its victory begins (or it goes home).
    pub(crate) fn begin_victory(&mut self, k: usize, v: Victory) {
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: squad {k} victory {v:?}");
        }
        match v {
            Victory::Leave => self.go_home(k),
            Victory::Capture => {
                for &i in &self.world.squads[k].members.clone() {
                    if self.folk[i].race.is_none() {
                        self.folk[i].captures = true;
                    }
                }
                self.world.squads[k].won = Some(v);
            }
            Victory::Occupy | Victory::LootFood => {
                // its people take up the package's jobs: the victory's, else (looting food) the
                // assault's
                let tasks_in = |phase: &str| -> Option<String> {
                    let Goal::Campaign { campaign, .. } = &self.world.squads[k].goal else { return None };
                    let c = self.data.get(campaign)?;
                    let p = campaign_refs(&self.data, c, phase).first()?.0.clone();
                    let rec = self.data.get(&p)?;
                    let all: Vec<i32> = rec.refs("Leader AI Goals").iter().chain(rec.refs("Squad AI Goals")).filter_map(|(t, _)| self.data.get(t).and_then(|t| t.ints.get("enum").copied())).collect();
                    (v == Victory::Occupy || all.contains(&LOOTING_FOOD)).then_some(p)
                };
                let package = tasks_in("victory AI leader").or_else(|| tasks_in("pt2 AI leader"));
                if let Some(p) = package.and_then(|p| self.data.get(&p).cloned()) {
                    let members = self.world.squads[k].members.clone();
                    let leader = members.first().copied();
                    for (j, &m) in members.iter().enumerate() {
                        let role = if j == 0 { kenshi_ai::package::Role::Leader } else { kenshi_ai::package::Role::Squad };
                        let goals = kenshi_ai::package::package_goals(&self.data, &p, role);
                        self.folk[m].ai.jobs = crate::sim::jobs_of(goals, leader.filter(|&l| l != m), None);
                    }
                    // @fn F029010: an occupation's package (SIG_OCCUPY_CONQUERED_TOWN) starting:
                    // each of the squad says so (EV_TAKEN_OVER_PLAYER_TOWN, no target: here
                    // itself)
                    if p.ints.get("signal func").copied() == Some(SIG_OCCUPY_CONQUERED_TOWN) {
                        for &m in &members {
                            self.bark(m, m, EV_TAKEN_OVER_PLAYER_TOWN);
                        }
                    }
                }
                self.world.squads[k].won = Some(v);
            }
        }
    }

    /// One step of squad `k`'s victory at its target (`standing`: its people up).
    pub(crate) fn victory_step(&mut self, k: usize, standing: &[usize], at: [f32; 3]) {
        let Some(v) = self.world.squads[k].won else { return };
        let near = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
        match v {
            Victory::Capture => {
                // the player's knocked out not yet taken
                let left: Vec<usize> = self
                    .folk
                    .iter()
                    .filter(|p| p.player && p.down && !p.dead && !p.away && p.carried_by.is_none() && p.prison.is_none())
                    .filter(|p| !self.folk.iter().any(|o| o.pickup == Some(p.id)))
                    .map(|p| p.id)
                    .collect();
                let free: Vec<usize> = standing.iter().copied().filter(|&i| self.folk[i].carrying.is_none() && self.folk[i].pickup.is_none()).collect();
                if left.is_empty() || free.is_empty() {
                    // all taken (or no hands left): home with them
                    self.go_home(k);
                    return;
                }
                // the free go to the nearest of them (the pick-up itself is `capture_tick`'s)
                let target = left.iter().copied().min_by(|&a, &b| near(self.folk[a].mover.position_units(), at).total_cmp(&near(self.folk[b].mover.position_units(), at)));
                if let Some(v) = target {
                    let q = self.folk[v].mover.position_units();
                    if self.world.squads[k].order_timer <= 0.0 {
                        self.world.squads[k].order_timer = 3.0;
                        let far: Vec<usize> = free.iter().copied().filter(|&i| near(self.folk[i].mover.position_units(), q) > 300.0).collect();
                        if !far.is_empty() {
                            self.order_move(&far, q);
                        }
                    }
                }
            }
            // the occupation: its people's jobs run (`sim`), the squad stays
            Victory::Occupy => {}
            // looting food: its people's Looting food job takes it (`sim`); home once they carry
            // food, or none is left about the target
            Victory::LootFood => {
                let target = match &self.world.squads[k].goal {
                    Goal::Campaign { target, .. } => *target,
                    _ => at,
                };
                let carries = standing.iter().any(|&m| {
                    let inv = &self.folk[m].inventory;
                    inv.sections.iter().chain(inv.bag().into_iter().flat_map(|b| b.sections.iter())).any(|s| s.items.iter().any(|i| i.item.food.is_some()))
                });
                let left = (0..self.buildings.len())
                    .filter(|&b| near(self.buildings[b].at, target) < LOOT_RANGE)
                    .any(|b| self.buildings[b].storage.as_ref().is_some_and(|s| s.sections.iter().any(|sec| sec.items.iter().any(|i| i.item.food.is_some()))));
                if carries || !left {
                    self.go_home(k);
                }
            }
            Victory::Leave => self.go_home(k),
        }
    }

    /// Squad `k` goes off-screen: whom its people carry goes with it.
    pub(crate) fn captives_away(&mut self, k: usize) {
        for &m in &self.world.squads[k].members.clone() {
            if let Some(c) = self.folk[m].carrying {
                self.folk[c].away = true;
                if !self.world.squads[k].captives.contains(&c) {
                    self.world.squads[k].captives.push(c);
                }
            }
        }
    }

    /// Squad `k` is met again: its captives back in its people's arms (the carriers' still
    /// holding them).
    pub(crate) fn captives_back(&mut self, k: usize) {
        for c in self.world.squads[k].captives.clone() {
            self.folk[c].away = false;
            if let Some(m) = self.folk[c].carried_by {
                self.folk[c].mover.position = self.folk[m].mover.position;
                self.arena.f[c].mover.position = self.folk[c].mover.position;
            }
        }
        self.world.squads[k].captives.clear();
    }

    /// @fn F048910
    ///
    /// The nearest town to `at` (across) of type `kind`, whose faction is no enemy of `friend`
    /// (F042efb), nearer than `max` (squared; `f32::MAX`: any), with a building designated
    /// `designation` (its TOWN record's "residents", their "building designation": the town's
    /// TagsClass<BuildingDesignation> +0x498, F048905); `None`: any (F048796).
    fn nearest_town_for(&self, at: [f32; 3], kind: i32, friend: &str, max: f32, designation: Option<i32>) -> Option<usize> {
        let enemy = |f: Option<&str>| f.and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_enemy(Some(friend)));
        let designated = |t: usize| {
            designation.is_none_or(|d| {
                let rec = self.town_record(t);
                self.data.get(&rec).is_some_and(|r| r.refs("residents").iter().any(|(q, _)| self.data.get(q).and_then(|q| q.ints.get("building designation")) == Some(&d)))
            })
        };
        (0..self.towns.len())
            .filter(|&t| self.towns[t].kind == kind && !enemy(self.towns[t].faction.as_deref()))
            .map(|t| (t, (self.towns[t].position[0] - at[0]).powi(2) + (self.towns[t].position[2] - at[2]).powi(2)))
            .filter(|&(t, d)| (max == f32::MAX || d <= max) && designated(t))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(t, _)| t)
    }

    /// Whether squad `k` ships its catch to a depot: won by capturing, its package (its
    /// leader's) a slaver shipping one (SIG_SLAVER_SHIP_TO_WORKCAMPS, _ANYWHERE_CLOSE).
    fn shipping_signal(&self, k: usize) -> Option<i32> {
        if self.world.squads[k].won != Some(Victory::Capture) {
            return None;
        }
        let leader = *self.world.squads[k].members.first()?;
        let sig = self.folk[leader].package.as_deref().and_then(|p| self.data.get(p)).and_then(|p| p.ints.get("signal func").copied())?;
        [SIG_SLAVER_SHIP_TO_WORKCAMPS, SIG_SLAVER_SHIP_TO_ANYWHERE_CLOSE].contains(&sig).then_some(sig)
    }

    /// @fn F028970
    /// @fn F029037
    ///
    /// The town squad `k` ships its catch to (SlaverPrisonerShipping's target): the nearest
    /// slave camp (TOWN_SLAVE_CAMP) of a faction no enemy of the squad's (the nearest of its own
    /// faction's is looked up too and not used, as in the game); shipping anywhere close, the
    /// nearest town (TOWN_TOWN) with a slave storage (BD_SLAVE_STORAGE), no enemy's, nearer than
    /// 0.7 of the camp's distance squared (any distance with no camp), over the camp.
    pub(crate) fn slave_depot(&self, k: usize) -> Option<usize> {
        let sig = self.shipping_signal(k)?;
        let s = &self.world.squads[k];
        let faction = s.faction.clone().unwrap_or_default();
        let camp = self.nearest_town_for(s.pos, TOWN_SLAVE_CAMP, &faction, f32::MAX, None);
        if sig != SIG_SLAVER_SHIP_TO_ANYWHERE_CLOSE {
            return camp;
        }
        let max = camp.map_or(f32::MAX, |c| ((self.towns[c].position[0] - s.pos[0]).powi(2) + (self.towns[c].position[2] - s.pos[2]).powi(2)) * DEPOT_NEARER);
        self.nearest_town_for(s.pos, TOWN_TOWN, &faction, max, Some(BD_SLAVE_STORAGE)).or(camp)
    }

    /// Squad `k` is home (`to`) with its captives: left there caged, shackled, its faction's
    /// slaves. A squad shipping its catch (`slave_depot`) hands it over there: its captives its
    /// own slaves, then (Selling slaves, 194: Task_TalktoPlayer's EV_SLAVE_DELIVERY, its line's
    /// DA_GIVE_TARGET_MY_SLAVES) given to whoever of the depot's faction its leader finds there
    /// ((stand-in) the nearest within `RECEIVER_RANGE`); none there, the depot's faction's.
    pub(crate) fn captives_home(&mut self, k: usize, to: [f32; 3]) {
        let mut faction = self.world.squads[k].faction.clone().unwrap_or_default();
        // shipping: the depot's faction's (its town at `to`)
        if self.shipping_signal(k).is_some()
            && let Some(f) = self.towns.iter().find(|t| t.position == to).and_then(|t| t.faction.clone())
        {
            if self.deliver_slaves(k, to, &f) {
                self.world.squads[k].captives.clear();
                return;
            }
            faction = f;
        }
        let mut captives = self.world.squads[k].captives.clone();
        for &m in &self.world.squads[k].members {
            if let Some(c) = self.folk[m].carrying
                && !captives.contains(&c)
            {
                captives.push(c);
            }
        }
        for (j, c) in captives.into_iter().enumerate() {
            if let Some(m) = self.folk[c].carried_by.take() {
                self.folk[m].carrying = None;
                self.folk[m].arrest = None;
            }
            let spot = [to[0] + (j % 3) as f32 * 20.0, to[1], to[2] + (j / 3) as f32 * 20.0];
            self.folk[c].away = false;
            self.folk[c].mover.position = spot.map(|v| v * 0.1);
            self.arena.f[c].mover.position = self.folk[c].mover.position;
            // a cage of theirs at home takes them, else they are kept shackled where left
            let cage = self.free_cage(&faction, to, false).filter(|&q| {
                let p = self.furn[q].position;
                (p[0] - to[0]).hypot(p[2] - to[2]) < HOME_CAGE_RANGE
            });
            match cage {
                Some(q) => {
                    self.make_slave(c, None, &faction);
                    self.imprison(c, q, &faction, true);
                }
                None => {
                    self.make_slave(c, None, &faction);
                    self.folk[c].shackled = true;
                }
            }
            let name = self.display_name(c);
            let town = self.data.get(&faction).map_or(faction.clone(), |f| f.name.clone());
            self.messages.push(format!("{name} has been taken away as a slave by {town}."));
            eprintln!("world: squad {k} home with captive {c}");
        }
        self.world.squads[k].captives.clear();
    }

    /// Squad `k`'s leader hands its captives to someone of faction `f` near `to`: it says
    /// EV_SLAVE_DELIVERY to the receiver, and they are the receiver's squad's slaves (its
    /// owners then process them, crate::slavery). (stand-in) The hand-over is done here, as the
    /// delivery lines' DA_GIVE_TARGET_MY_SLAVES would: their lines past the first are not
    /// played out. Whether it did.
    fn deliver_slaves(&mut self, k: usize, to: [f32; 3], f: &str) -> bool {
        let members = self.world.squads[k].members.clone();
        let Some(&leader) = members.iter().find(|&&m| !self.folk[m].dead && !self.folk[m].down && !self.folk[m].away) else { return false };
        let receiver = self
            .folk
            .iter()
            .filter(|p| !p.player && !p.dead && !p.down && !p.away && p.slave.is_none() && p.faction.as_deref() == Some(f) && !members.contains(&p.id))
            .map(|p| (p.id, (p.mover.position_units()[0] - to[0]).hypot(p.mover.position_units()[2] - to[2])))
            .filter(|&(_, d)| d < RECEIVER_RANGE)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        let Some(r) = receiver else { return false };
        let owner = self.folk[r].platoon;
        let mut captives = self.world.squads[k].captives.clone();
        for &m in &members {
            if let Some(c) = self.folk[m].carrying
                && !captives.contains(&c)
            {
                captives.push(c);
            }
        }
        let said = self.bark(leader, r, EV_SLAVE_DELIVERY);
        let at = self.folk[r].mover.position_units();
        for (j, &c) in captives.iter().enumerate() {
            if let Some(m) = self.folk[c].carried_by.take() {
                self.folk[m].carrying = None;
                self.folk[m].arrest = None;
            }
            // (put down beside the receiver)
            let spot = [at[0] + 30.0 + (j % 3) as f32 * 20.0, at[1], at[2] + (j / 3) as f32 * 20.0];
            self.folk[c].away = false;
            self.folk[c].mover.position = spot.map(|v| v * 0.1);
            self.arena.f[c].mover.position = self.folk[c].mover.position;
            self.make_slave(c, owner, f);
            self.folk[c].shackled = true;
        }
        if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: squad {k}'s {leader} delivers {captives:?} to {r} ({f}): said {said}");
        }
        for &c in &captives {
            let name = self.display_name(c);
            let town = self.data.get(f).map_or(f.to_string(), |r| r.name.clone());
            self.messages.push(format!("{name} has been taken away as a slave by {town}."));
        }
        true
    }
}
