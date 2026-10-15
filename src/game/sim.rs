//! Townsfolk on Kenshi's own AI: residents of a zone wander between its general nodes the way
//! the "Wandering town" goal (37) makes them.
//!
//! Driven by `cargo run --release -p kenshi-game --example town -- <install> X Y [people] [seconds] [squads] > town.csv`
//!
//! Extra squads stand at the town centre: `player:<n>` the player's squad of `n` (faction
//! Nameless, no free will: they take orders, `Sim::order_move` / `Sim::order_attack`, through the
//! task system's orders list), `squad:<squad template>` as the data has it, or
//! (harness) `demo:<faction>:<count>`, greenlanders in that faction's formation on the
//! "wander town as a squad" package.
//!
//! Ported: the task system's decision (`task_system`), the planner (`planner`) on the extracted
//! task definitions, the node pick (F031809: a random free general node of the town), the
//! conditions the chain needs (isAtLocation F038667, atNode F038684), the moving and standing
//! steps (Task_Move: go-to through the path queue, finished when at the location;
//! Task_StandAtNode: claims the node, F031320/F031321), the navmesh, path requests and the mover.
//!
//! Town patrol: Task_PatrolTown (F030869 start, F030870 update, F030871 end) through the move
//! helper F039100: an area order round the destination town (half its radius, F048768) that
//! CharMovement turns into a random point (F040635) and a goTo, ordered again whenever the mover
//! is idle. The current town comes from F038707's town part, atDestinationTown (F036806) compares
//! it with the destination.
//!
//! Harness (not Kenshi's): a squad's destination town (the platoon's +0x78 +0xf8) is the town it
//! lives in; the conditions about the body (crouching, in bed, carrying, weapon)
//! answer for an unarmed townsperson standing up; goals whose score or target finder is not
//! ported score 0; the AI thinks once a real second. One game hour lasts `MINUTE_HOURS` real
//! seconds: the clock is SkyX's, advanced by the frame time x its time multiplier, which F040805
//! sets to the game speed (F044304, 1 at normal speed) x 0.55 / 60 hours a second.
//!
//! Squads: Task_FollowLeader (F031009) keeps members to their formation spot (F028591 layout,
//! F030760 spot), stepping straight (CharMovement mode 2) when near and pathing when far.
//!
//! Enemies: each think the character's sensor tests everyone in range (F046551 / F046539, the
//! daylight from the sim clock); the known characters of an enemy faction (F041494) give the
//! "enemies in vicinity" count (AI +0xbc, condition 6 isNoEnemiesInVicinity: (assumption) the
//! count of seen, standing enemies - the code that keeps it is not found yet) and the
//! "Attacking enemies" goal (16, score F038552) finds its target with AI::findEnemy (F038641).
//! The planner reaches it through Attacking (4, score F038700): Moving until within 500 units
//! (condition 41, F038683), then the fight runs on the combat arena (`kenshi_combat::arena`).
//! Harness: fighters carry a katana and a plain outfit at skill 30; the one attacked fights
//! back at once and counts its attacker's side as enemies (the temporary-enemy tag, F040863);
//! characters light the same as the daylight; the arena moves fighters without the navmesh.
//!
//! Prints `t,id,x,y,z,speed,task,role,state` rows (units; role 0 wanderer, 1 shopkeeper, 2 guard,
//! 3 other, 4 town patrol, 5 visiting squad leader, 6 its follower, 7 bar patron; state 0, 1
//! fighting, 2 knocked out) at 20 Hz for `town_map`.
use kenshi_ai::bar::{BD_BAR, Chair, choose_bar, choose_chair};
use kenshi_ai::enemy::{Area, Known, LeashTarget, find_enemy, nearest_score};
use kenshi_ai::faction::FactionRelations;
use kenshi_ai::follow::{Body, FollowLeader, FollowMove};
use kenshi_ai::formation::{FormationKind, Member, layout};
use kenshi_ai::package::{PackageGoal, Role, package_goals};
use kenshi_ai::perception::{Observer, Stance, Target as Seen, daylight, sees, vision_range};
use kenshi_ai::planner::Agent;
use kenshi_ai::task_def::{Score, TaskDef};
use kenshi_ai::task_system::{Mind, NON_URGENT, Task, TaskSystem};
use kenshi_char::encumbrance::run_speed;
use kenshi_char::movement::{
    MoveSpeed, Mover, NO_GROUND, Pathing, RaceMovement, area_point, direct, measured_velocity,
    order_radius, speed_limit, stop,
};
use kenshi_combat::arena::{Arena, Fighter, Kit, QUIET, Shared, opponent, skeleton_clips};
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use kenshi_world::biome::{Biome, BiomeMap, biome_of_zone, biomes, zone_at, zone_centre};
use kenshi_world::furniture::{BF_CHAIR, Furniture, furniture};
use kenshi_world::interior::{Door, building_at, doors, interiors};
use kenshi_world::navmesh::{METRES, NavWorld, PathManager, tile_of};
use kenshi_world::nodes::{Layouts, NODE_GENERAL, NODE_GUARDPOST, NODE_SHOPKEEPER, Node, nodes};
use kenshi_world::town::{Town, current_town, towns};
use kenshi_world::{Terrain, placements};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;

/// Real seconds per game hour at normal speed (F040805: SkyX time multiplier 0.55 / 60).
pub const MINUTE_HOURS: f64 = 1.0 / (0.55f32 / 60.0) as f64;
/// isAtLocation (F038667): within 12 units across, 20 up or down.
pub const AT_LOCATION_SQ: f32 = 144.0;
pub const AT_LOCATION_DY: f32 = 20.0;
/// atNode (F038684): within 0.95 of the node's radius, 50 up or down. The node radius
/// (func 0x14002b175) is not read yet: 20 units.
pub const NODE_RADIUS: f32 = 20.0;
pub const AT_NODE_DY: f32 = 50.0;

pub const WANDERING_TOWN: u16 = 37;
pub const MOVING: u16 = 1;
pub const STAND_AT_NODE: u16 = 51;
/// AI_TASK "Shopping" (civilians', farmers', bar-goers' packages; a generic task: crate::shopping).
pub const SHOPPING: u16 = 118;
/// Its solver, the other "Shopping" (119: Task_Shopping, buying at the counter).
pub const SHOPPING_VISIT: u16 = 119;
/// AI_TASK "Home territory aggression" (144: guards' "Attack enemies- but don't leave town").
pub const HOME_TERRITORY_AGGRESSION: u16 = 144;
/// AI_TASK "Self preservation" (32): fighting back whoever attacks me.
pub const SELF_PRESERVATION: u16 = 32;
/// The goals of using furniture for show (IS_WORKING_PRETEND, condition 136): "Staying home"
/// (43: "Stay home, sit around, use stuff INDOORS"), "Reigning" (255: throne sitting), "Sitting
/// around" (275), each planned into "Operating machine" (221, Task_OperateMachine for show).
pub const STAYING_HOME: u16 = 43;
pub const REIGNING: u16 = 255;
pub const SITTING_AROUND: u16 = 275;
pub const OPERATING_PRETEND: u16 = 221;
/// AI_TASK "Fixing Legs" (269: "Heal my legs when crippled"; Task_FirstAid on oneself).
pub const FIXING_LEGS: u16 = 269;
/// AI_TASK "Static town guard" (157: "Static guard- town") and "Static guard indoors" (268).
pub const STATIC_TOWN_GUARD: u16 = 157;
pub const STATIC_GUARD_INDOORS: u16 = 268;
pub const SHOPKEEPER: u16 = 20;
pub const STATIC_GUARD: u16 = 38;
pub const PATROL_TOWN: u16 = 36;
pub const PATROLLING: u16 = 22;
pub const FOLLOWING_LEADER: u16 = 141;
/// (assumption) How far behind the one followed (Follow, Bodyguard) a character lets itself get.
pub const ESCORT_GAP: f32 = 40.0;
pub const RELAXING: u16 = 214;
pub const OPERATING_MACHINE: u16 = 87;
pub const SLEEPING: u16 = 98;
pub const GETTING_OUT_OF_BED: u16 = 116;
pub const GOING_TO_BED: u16 = 153;
pub const GOING_TO_BED_SECURE: u16 = 239;
pub const LOCKING_UP: u16 = 155;
pub const ENTERING_BUILDING: u16 = 156;
pub const LOCKING_DOOR: u16 = 162;
pub const TO_DOOR_INSIDE: u16 = 163;
/// (assumption) how near a door's inside point counts as at the door (the building's door
/// radius, vtable +0x350, is not read): units.
pub const DOOR_REACH: f32 = 20.0;
/// BuildingFunction BF_BED.
pub const BF_BED: i32 = 6;
/// atDestinationTown (F036806).
pub const AT_DESTINATION_TOWN: u16 = 94;
pub const ATTACKING: u16 = 4;
/// The player's faction, Nameless.
pub const PLAYER_FACTION: &str = kenshi_ai::faction::PLAYER_FACTION;
pub const RUNNING: u16 = 67;
/// (harness) How near an attacker walks before the fight starts (units).
pub const ENGAGE_RANGE: f32 = 60.0;
pub const ATTACKING_ENEMIES: u16 = 16;
/// "Attacking attackers of" (AI_TASK 13), "Protect allies" (15), "Protect close allies" (143).
pub const ATTACKING_ATTACKERS_OF: u16 = 13;
pub const PROTECT_ALLIES: u16 = 15;
pub const PROTECT_CLOSE_ALLIES: u16 = 143;
/// "Turret gunner" (146: Task_UseTurret, runnable), "Turret guard" (149, the packages' "Man the
/// turrets") and "Turret home guard" (222, "Turret Gunner Homebuilding").
pub const TURRET_GUNNER: u16 = 146;
pub const MAN_THE_TURRETS: u16 = 149;
pub const TURRET_HOME_GUARD: u16 = 222;
/// "Looting food" (131, the packages' "Looting food from base(desperate)"), its step "Looting"
/// (284), and "Grabbing food" (244), Getting food's step.
pub const LOOTING_FOOD: u16 = 131;
pub const LOOTING: u16 = 284;
pub const GRABBING_FOOD: u16 = 244;
/// (assumption) How far about a looter F047552's search for food reaches (units).
pub const FOOD_SEARCH: f32 = 3000.0;
/// (assumption) How near a container one must stand to take from it (units).
pub const STORE_REACH: f32 = 60.0;
/// F050284: how near (squared units) to the ally its attackers must be (0x1416e345c).
const ATTACKERS_OF_RANGE_SQ: f32 = 22500.0;
/// "attack enemies and neutrals" (AI_TASK enum 21): anyone not an ally is fought.
pub const ATTACKING_NEUTRALS: u16 = 21;
/// "Wandering trader" (enum 39): travelling town to town.
pub const WANDERING_TRADER: u16 = 39;
/// "Capture Slaves" (enum 167) and "Carry wounded slaves" (168).
pub const CAPTURE_SLAVES: u16 = 167;
pub const CARRY_SLAVES: u16 = 168;
/// Watching slaves ("Slave watching/stripping", 183), Bounty hunting ("Arrest bounties", 204),
/// Getting food ("get food", 243), Kidnapper ("Find and kidnap (with cages)", 254).
pub const WATCHING_SLAVES: u16 = 183;
pub const BOUNTY_HUNTING: u16 = 204;
pub const GETTING_FOOD: u16 = 243;
pub const KIDNAPPER: u16 = 254;
/// `Person::duties` bits: the goals of its squad's packages that run as routines here.
pub const DUTY_ARRESTS: u8 = 1;
pub const DUTY_WATCHES_SLAVES: u8 = 2;
pub const DUTY_GETS_FOOD: u8 = 4;
pub const DUTY_KIDNAPS: u8 = 8;
/// "Law enforcer": Attacking trouble makers (34), the town guards' (their target down, F target
/// condition 7).
pub const DUTY_ENFORCES: u8 = 16;
pub const ATTACKING_TROUBLE_MAKERS: u16 = 34;

/// A squad template's packages (all of them, an "addon-" one too): whether its people capture
/// (tasks 167, 168, 254) and the goals that run as routines (`duties_of`).
pub fn template_duties(data: &GameDataContainer, t: &kenshi_data::GameData) -> (bool, u8) {
    let mut goals: Vec<u16> = Vec::new();
    for pk in t.refs("AI packages").iter().filter_map(|(pk, _)| data.get(pk)) {
        for r in [Role::Leader, Role::Squad] {
            goals.extend(package_goals(data, pk, r).iter().map(|g| g.task));
        }
    }
    (goals.iter().any(|&g| g == CAPTURE_SLAVES || g == CARRY_SLAVES || g == KIDNAPPER), duties_of(goals))
}

/// The `Person::duties` of goals `tasks`.
pub fn duties_of(tasks: impl IntoIterator<Item = u16>) -> u8 {
    tasks.into_iter().fold(0, |d, t| {
        d | match t {
            BOUNTY_HUNTING => DUTY_ARRESTS,
            WATCHING_SLAVES => DUTY_WATCHES_SLAVES,
            GETTING_FOOD => DUTY_GETS_FOOD,
            KIDNAPPER => DUTY_KIDNAPS,
            ATTACKING_TROUBLE_MAKERS => DUTY_ENFORCES,
            _ => 0,
        }
    })
}
/// (harness) the townsfolk's fighting kit and skill.
pub const KIT_WEAPON: &str = "Katana";
pub const KIT_OUTFIT: [&str; 3] = ["Leather Shirt", "Cargopants", "Drifter's Boots"];
pub const KIT_SKILL: f32 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    None,
    Node(usize),
    /// Another character (by index).
    Person(usize),
    /// A piece of furniture (by index).
    Furniture(usize),
    /// A building's door (by index).
    Door(usize),
    /// A container: a town's (`loot::BOX_OWNER` + index) or a storage building of the
    /// player's (`base::BUILDING_OWNER` + index).
    Store(usize),
}

/// A door and its lock (the building state record's "locked{n}", "lock lvl{n}").
#[derive(Debug, Clone)]
pub struct DoorState {
    pub door: Door,
    pub locked: bool,
    pub level: i32,
    /// The point just inside the door the character locks it from (units).
    pub inside: [f32; 3],
    /// DoorStuff +0x380: DOOR_CLOSED, DOOR_OPEN, DOOR_OPENING, DOOR_CLOSING.
    pub phase: i32,
    /// How far open, 0 .. 1 (+0x37c).
    pub open: f32,
    /// Broken in (bashed): it stays open and takes no lock.
    pub broken: bool,
    /// Locks itself once shut ("always locks").
    pub wants_lock: bool,
    /// Its health, out of `full`: its record's "build materials" worth (the door's construction,
    /// Building +0x164, which blows take down, F029386); `paid`: the materials put into
    /// repairing it so far (crate::doors).
    pub health: f32,
    pub full: f32,
    pub paid: f32,
    /// The door of the player's building `b` (built by the player, crate::doors), and whether
    /// it is gone (its building dismantled or destroyed).
    pub player_building: Option<usize>,
    pub gone: bool,
}

pub const DOOR_CLOSED: i32 = 0;
pub const DOOR_OPEN: i32 = 1;
pub const DOOR_OPENING: i32 = 2;
pub const DOOR_CLOSING: i32 = 3;

/// How near a closed door someone walking has to come to open it (units).
/// (assumption) Kenshi opens a door for a path that crosses it (Task_MoveToDoor, Task_OpenDoor,
/// F030902 / F030903); here whoever walks within this of a closed door opens it.
pub const DOOR_OPEN_REACH: f32 = 40.0;

impl DoorState {
    /// A door as its zone left it (F029398: its saved "state", "doa", "broken", "wants lock"),
    /// else as its building's squad has it (F037791: the SQUAD_TEMPLATE's "initial door state",
    /// DoorState CLOSED, OPEN, LOCKED, BROKEN; the building's own squad, else its residents').
    pub fn new(data: &GameDataContainer, place: &kenshi_world::Placement, door: Door, locked: bool, level: i32, inside: [f32; 3]) -> DoorState {
        let full = data.get(&door.record).and_then(|d| d.ints.get("build materials").copied()).unwrap_or(1).max(1) as f32;
        let mut s = DoorState { wants_lock: door.always_locks, door, locked, level, inside, phase: DOOR_CLOSED, open: 0.0, broken: false, health: full, full, paid: full, player_building: None, gone: false };
        if let Some(Some(saved)) = place.door_saved.get(s.door.index) {
            s.phase = saved.state.clamp(DOOR_CLOSED, DOOR_CLOSING);
            s.open = saved.open.clamp(0.0, 1.0);
            s.broken = saved.broken;
            s.wants_lock = saved.wants_lock;
            return s;
        }
        let initial = [&place.squad, &place.residents]
            .into_iter()
            .flatten()
            .find_map(|t| data.get(t)?.ints.get("initial door state").copied())
            .unwrap_or(0);
        match initial {
            1 => {
                s.phase = DOOR_OPEN;
                s.open = 1.0;
                s.locked = false;
            }
            2 => s.locked = true,
            3 => {
                s.broken = true;
                s.phase = DOOR_OPEN;
                s.open = 1.0;
                s.locked = false;
            }
            _ => {}
        }
        s
    }

    /// F029271: a closed door starts opening ("Door_Open").
    pub fn start_open(&mut self) -> bool {
        if self.phase == DOOR_CLOSED && !(self.locked && self.level >= 1) {
            self.phase = DOOR_OPENING;
            return true;
        }
        false
    }

    /// F029334: an open door (not broken) starts closing ("Door_Close").
    pub fn start_close(&mut self) -> bool {
        if !self.broken && self.phase == DOOR_OPEN {
            self.phase = DOOR_CLOSING;
            return true;
        }
        false
    }

    /// @fn F029337
    ///
    /// A door moving: opening by its "door move speed" a second until open, closing until shut
    /// ("Door_Shut"), then locking itself if it "wants lock". Returns whether it just shut.
    pub fn step(&mut self, dt: f32) -> bool {
        match self.phase {
            DOOR_CLOSING => {
                self.open -= dt * self.door.speed;
                if self.open <= 0.0 {
                    self.open = 0.0;
                    self.phase = DOOR_CLOSED;
                    if self.wants_lock {
                        self.locked = true;
                    }
                    return true;
                }
            }
            DOOR_OPENING => {
                self.open += dt * self.door.speed;
                if self.open >= 1.0 {
                    self.open = 1.0;
                    self.phase = DOOR_OPEN;
                }
            }
            _ => {}
        }
        false
    }

    /// Whether people can pass (F029335: the navmesh takes a door 0.6 open as open).
    pub fn passable(&self) -> bool {
        self.gone || self.broken || self.open >= 0.6
    }
}

/// A squad's Package_HangAroundBar state: its bar and each member's chair.
#[derive(Debug, Default)]
pub struct BarState {
    pub bar: Option<usize>,
    pub held: std::collections::HashMap<usize, usize>,
}

pub struct Lcg(pub u64);
impl Lcg {
    pub(crate) fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A uniform number in [0, 1).
    pub fn uniform(&mut self) -> f32 {
        self.next()
    }
}

/// One resident: the character side the AI asks about.
pub struct Person {
    pub id: usize,
    /// The character's name (the player's from the character editor).
    pub name: String,
    /// What the character carries and wears.
    pub inventory: kenshi_char::inventory::Inventory,
    /// The CHARACTER record it was made from (its stats and kit), if any.
    pub character: Option<String>,
    /// Seconds until it may eat again.
    pub eat_timer: f32,
    /// The shop it keeps, if it is a trader.
    pub shop: Option<crate::trade::Shop>,
    /// The DIALOGUE_PACKAGE it talks with.
    pub dialogue: Option<String>,
    /// Its RACE when not the world's people race (an animal's).
    pub race: Option<String>,
    /// Its goals fight neutrals too (task 21).
    pub attacks_neutrals: bool,
    /// Its squad's AI_PACKAGE (its "dialogs" are spoken too).
    pub package: Option<String>,
    /// Its goals take the knocked out captive (tasks 167 / 168, and the Kidnapper's 254).
    pub captures: bool,
    /// The goals of its squad's packages run as routines (DUTY_*): arresting bounties,
    /// watching slaves, getting food, kidnapping.
    pub duties: u8,
    /// A builder fetching materials (`base`, Task "Finding building mats"): the storage it
    /// walks to (true) or, carrying them, back to its site (false).
    pub build_fetch: Option<(usize, bool)>,
    /// Its combat stance (the main panel's orders, kenshi_char::stance) and where it holds
    /// (CharStats +0x130, set with the hold order).
    pub stance: kenshi_char::stance::Stance,
    pub hold_at: Option<[f32; 3]>,
    /// The player's job list (orders given with shift, kenshi_game::orders::Job) and the JOBS
    /// toggle (the task system's +0x35, F036819: with it off only urgent goals are weighed,
    /// F036915).
    pub jobs: Vec<crate::orders::Job>,
    pub jobs_on: bool,
    /// Its weapon in hand (Character +0x6d8, set by drawWeapon F039274, cleared by the sheathe,
    /// vtable +0x2d0), and how long it has had no fight to keep it out.
    pub weapon_out: bool,
    pub weapon_idle: f32,
    /// The wandering trader package's trip (its leader only).
    pub caravan: Option<kenshi_ai::caravan::Caravan>,
    /// Whom it is giving first aid (Task_FirstAid).
    pub first_aid: Option<usize>,
    /// Whom it is looting (Task_Loot_AI) and for how long it has been at it.
    pub loot: Option<(usize, f32)>,
    /// The player's "Follow" (order 0x1f) or "Bodyguard" (0x2d, true) order: whom it keeps up
    /// with, and whether it fights those who fight them.
    pub escort: Option<(usize, bool)>,
    /// The player's building it is building (Task_Build).
    pub build: Option<usize>,
    /// The player's production building it operates (Task_OperateMachine).
    pub operate: Option<usize>,
    /// Its errand emptying a full machine (Task_EmptyMachine).
    pub haul: Option<crate::production::Haul>,
    /// Crime and law (`law`): the crime it is committing, its bounties, its heat, its cage, its
    /// picking of its locks, its shackles, its escape (faction, until), the one it arrests or
    /// enslaves, whether it gives itself up, the one it creeps up on to knock out.
    pub crime: Option<crate::law::Crime>,
    pub bounties: Vec<crate::law::Bounty>,
    pub heat: f32,
    pub prison: Option<crate::law::Prison>,
    pub picking: bool,
    pub shackled: bool,
    /// Head shaved as a slave's (CHARACTER "shaved", or enslaved): no hair (crate::slavery).
    pub shaved: bool,
    /// The faction it is disguised as (its uniform, not blown) and how long its blown
    /// disguise stays blown (crate::disguise).
    pub disguised: Option<String>,
    pub disguise_blown: f32,
    /// Its sex when set by the character editor (else as its look rolls it, crate::looks_roll),
    /// and its personality (PersonalityTags, rolled from its faction's PERSONALITY, F040031).
    pub female: Option<bool>,
    pub personality: i32,
    /// Its race (RACE id) as the character editor set it (else as its look draws it).
    pub look_race: Option<String>,
    pub escaped: Option<(String, f64)>,
    /// Picking a cage's lock from outside to free whoever is in it: (the prisoner, the lock).
    pub freeing: Option<(usize, crate::law::Lock)>,
    pub arrest: Option<crate::law::Arrest>,
    pub surrendered: bool,
    /// Come round but staying down while enemies fight near ("Playing dead...": F047087's
    /// "This person has regained consciousness but is staying down, waiting for all the
    /// enemies to leave the area."); told to stand up ("Stand Up", order 0x1c).
    pub playing_dead: bool,
    pub stand_up: bool,
    /// The machine it keeps emptied into storage ("Auto-Haul", `orders`).
    pub auto_haul: Option<usize>,
    /// Where it is in the water (F039118, `water_state`: 0 dry, 1 shallow, 2 wading, 3 swimming)
    /// and, swimming at the surface, the height its body floats at (units).
    pub water: i32,
    pub float_y: Option<f32>,
    /// The player's standing job (crate::routines), how long it has stood idle (seconds) and
    /// whether it went to bed to heal ("sleep when injured").
    pub job: Option<crate::routines::Job>,
    pub idle_for: f32,
    pub rest_injured: bool,
    /// An animal's age, 0 (newborn) .. 1 (grown): its squad's "animals" third value (100 =
    /// adult), randomised by "animal age random". People are grown.
    pub age: f32,
    /// The dust in its eyes (crate::hazards, 0..1: a dust storm's strength less its gear's
    /// protection; F040451) and the weather or ground hurting it now.
    pub dust: f32,
    pub hazard: Option<crate::hazards::Hazard>,
    pub ko_target: Option<usize>,
    /// The ground item (`loot`) they are going to pick up.
    pub fetch: Option<u64>,
    /// The world squad it belongs to (`world`), and whether it is away (its squad travelling
    /// off-screen: not simulated, not drawn).
    pub world_squad: Option<usize>,
    pub away: bool,
    /// Robotic limbs in its limb slots (`limbs`: left arm, right arm, left leg, right leg).
    pub limbs: [Option<kenshi_char::inventory::Item>; 4],
    /// Dead (blood or a vital part lost, F040477): down for good.
    pub dead: bool,
    pub mover: Mover,
    pub pathing: Pathing,
    pub ai: TaskSystem<Target>,
    /// The node this person has claimed (node +0x100).
    pub claimed: Option<usize>,
    /// Its shopping list (crate::shopping; made on its first tick) and Task_Shopping's state.
    pub shopping: Option<crate::shopping::ShoppingList>,
    pub shop_visit: Option<crate::shopping::ShopVisit>,
    /// Their master, when a slave (crate::slavery).
    pub slave: Option<crate::slavery::Slavery>,
    /// Running step: started yet?
    pub started: bool,
    /// The building (placement) the person's squad lives in.
    pub home: Option<usize>,
    /// For the map: 0 wanderer, 1 shopkeeper, 2 guard, 3 other.
    pub role: u8,
    /// The town the character is in (+0x88, F038707) and the squad's destination town.
    pub town: Option<usize>,
    pub destination: Option<usize>,
    /// The platoon and this member's formation offset (F028591).
    pub platoon: Option<usize>,
    pub offset: [f32; 3],
    /// Task_FollowLeader's state.
    pub follow: FollowLeader,
    /// CharMovement mode 2 this tick: straight along a direction for at most a distance.
    pub straight: Option<([f32; 3], f32)>,
    /// CharMovement +0xd0 / +0xb8: the direction last moved in and the measured speed (units).
    pub facing: [f32; 3],
    pub speed: f32,
    /// The speed cap the player's last move order set (units / s), kept while that order runs.
    pub ordered_cap: Option<f32>,
    /// Sneaking (stealth mode, "toggle_sneak").
    pub sneaking: bool,
    /// Whom this character carries over its shoulder, and who carries this one.
    pub carrying: Option<usize>,
    pub carried_by: Option<usize>,
    /// Going to pick someone up (they are lifted once within reach).
    pub pickup: Option<usize>,
    /// Its move speed this tick (CharStats::updateMoveSpeed, F047064, units / s).
    pub own_speed: f32,
    /// Seconds left getting up after coming round (Task_GetUp: "standing up 3", F037164), held
    /// in place meanwhile.
    pub getting_up: f32,
    /// The furniture this person operates (Task_OperateMachine).
    pub operating: Option<usize>,
    /// The building (placement) the person is inside (F039242 / interior volumes).
    pub inside: Option<usize>,
    /// The bed the person lies in, and when they last got up (game hours).
    pub bed: Option<usize>,
    pub woke_at: f64,
    /// The bed this person's plan is heading for (the package runtime's reservation, F052a3b).
    pub reserved_bed: Option<usize>,
    /// The squad's faction (string id).
    pub faction: Option<String>,
    /// Knocked out.
    pub down: bool,
    /// Where Task_Run last ordered the mover to.
    pub run_goal: Option<[f32; 3]>,
    /// The player's character: no free will (AITaskSystem +0x35 off), orders only.
    pub player: bool,
    /// Think on the next tick (a new order).
    pub rethink: bool,
    /// A shut gate (door) that stopped this person's last path, and their bashing at it
    /// (Task_BashDoor, F031240): seconds to the next blow.
    pub gate_in_way: Option<usize>,
    /// The AI's ignore map (AI +0x218, F044546 / F038789 / F036917): people it leaves alone,
    /// each with seconds left.
    pub ignored: Vec<(usize, f32)>,
    /// (metres) The path to my fight's target while the straight way there is blocked
    /// (CombatClass's path list, +0x200); not saved.
    pub fight_path: Vec<[f32; 3]>,
    /// The player's order to bash a door (BASH_DOOR_PLAYER_ORDER): the door.
    pub bash_order: Option<usize>,
    /// The player's order to open or shut a door (task 72, "Open"): the door, toggled on
    /// reaching it.
    pub door_order: Option<usize>,
    /// The player's building this one is ordered to dismantle (Task_Dismantle, F031298).
    pub dismantle: Option<usize>,
    pub bash_timer: f32,
    /// DA_THUG_HUNTER (F040912 case 0x11): the one it stalks (task 165, "Stalking").
    pub stalk: Option<usize>,
    /// CharMovement's road way (+0xf8) and road factor (+0x100, from the faction's "road
    /// preference", F040657).
    pub roads: kenshi_world::roads::CharRoads,
}

/// What the AI sees while it plans for one person.
pub struct View<'a> {
    me: usize,
    home: Option<usize>,
    pos: [f32; 3],
    nodes: &'a [Node],
    /// Each node's approach point: its position moved onto the navmesh (units), as the location
    /// check measures to an object's reachable point (F038630, func 0x140037ffb).
    approach: &'a [[f32; 3]],
    owners: &'a [Option<usize>],
    rng: &'a mut Lcg,
    now: f64,
    town: Option<usize>,
    destination: Option<usize>,
    /// Everyone's position (units).
    positions: &'a [[f32; 3]],
    /// The zone's furniture, its approach points (units) and who operates each.
    furniture: &'a [Furniture],
    furniture_approach: &'a [[f32; 3]],
    operators: &'a [Option<usize>],
    /// The town's bars (placement indices) and the placements' positions.
    bars: &'a [usize],
    placed_at: &'a [[f32; 3]],
    placed_town: &'a [Option<usize>],
    /// This person's squad's bar handler, if its package has one.
    bar_state: Option<&'a mut BarState>,
    /// The building the person is inside, their bed, when they last got up, the clock's start.
    inside: Option<usize>,
    in_bed: Option<usize>,
    woke_at: f64,
    start_hour: f64,
    doors: &'a [DoorState],
    /// Beds already chosen by others (F052a3b's reservations).
    bed_taken: &'a [Option<usize>],
    /// The characters this one knows of (its sensor's entries) and how many are enemies about
    /// (AI +0xbc).
    known: &'a [Known<usize>],
    enemies: usize,
    /// The character decides for itself (not a player's character).
    free_will: bool,
    /// The passive stance (CharStats +0x12c) and the hold point of the hold stance (+0x12b,
    /// +0x130): one of the player's people goes for enemies only within its leash.
    passive: bool,
    hold: Option<[f32; 3]>,
    /// Those its AI leaves alone (`Person::ignored`).
    ignored: Vec<usize>,
    /// In a fight or someone's target in one (F038651's "attacked").
    attacked: bool,
    /// Its shopping list done (condition 85, F050255) and the item functions still wanted.
    shop_done: bool,
    shop_wants: Vec<i32>,
    /// The towns' shop counters and the public buildings Shopping falls back on.
    counters: &'a [crate::shopping::Counter],
    /// The home town's centre and radius (the AI area's town, +0x30: (assumption) the town of
    /// the home building).
    home_area: Option<([f32; 3], f32)>,
    /// Its squad's leader (made with the leader role, 5; ActivePlatoon +0xa0).
    leader: bool,
    /// A leg hurt (F050238; (assumption) its health under its most) and a first aid kit carried
    /// (condition 23).
    legs_hurt: bool,
    has_medkit: bool,
    public: &'a [crate::shopping::PublicBuilding],
    /// Who is knocked out.
    downs: &'a [bool],
    /// The ally Protect allies (15) and Protect close allies (143) would help, with the
    /// finder's score (F050285 / F050287), and whether a friendly known needs help (F050269).
    protect: [Option<(usize, f32)>; 2],
    allies_fighting: bool,
    /// An obedient slave (job 187), and sneaking out of a fight (F039177): F038601 scores
    /// Protect allies 0.
    obedient: bool,
    sneaking: bool,
    /// Carrying someone (Character +0x348).
    carrying: bool,
    /// Each one's attackers, who is in a fight and whom each goes for in it.
    attackers_of: &'a [Vec<usize>],
    in_fight: &'a [bool],
    fight_target: &'a [Option<usize>],
    /// The zone's turrets (furniture) with the town each stands in, and my hunger level and the
    /// food queued to digest (Character +0x4b8 / +0x4bc, F038553 reads them).
    turrets: &'a [(usize, Option<usize>)],
    turret_taken: &'a [Option<usize>],
    /// The containers about (owner, the navmesh point by it, whether it holds food) and
    /// whether I carry food (gotAFoodItem, F038549).
    stores: &'a [(usize, [f32; 3], bool)],
    has_food: bool,
    hunger: (f32, f32),
    /// Each one's faction (as it passes), mine and my faction's relations.
    factions: &'a [Option<String>],
    my_faction: Option<String>,
    mine: Option<&'a FactionRelations>,
}

impl View<'_> {
    fn target_pos(&self, t: &Target, pos: [f32; 3]) -> Option<[f32; 3]> {
        match t {
            Target::Node(n) => Some(self.approach[*n]),
            Target::Person(i) => Some(self.positions[*i]),
            Target::Furniture(f) => Some(self.furniture_approach[*f]),
            Target::Door(d) => Some(self.doors[*d].inside),
            Target::Store(o) => self.stores.iter().find(|s| s.0 == *o).map(|s| s.1),
            Target::None => (pos != [0.0; 3]).then_some(pos),
        }
    }
}

impl Agent for View<'_> {
    type Target = Target;
    fn no_target(&self) -> Target {
        Target::None
    }
    fn check(&mut self, c: u16, t: &Target, pos: [f32; 3]) -> Option<bool> {
        let me = self.pos;
        let near = |p: [f32; 3], r2: f32, dy: f32| {
            (p[0] - me[0]).powi(2) + (p[2] - me[2]).powi(2) < r2 && (p[1] - me[1]).abs() < dy
        };
        Some(match c {
            1 | 46 => self
                .target_pos(t, pos)
                .is_some_and(|p| near(p, AT_LOCATION_SQ, AT_LOCATION_DY)),
            // isWithin10Meters (F038683 with 100)
            40 => self
                .target_pos(t, pos)
                .is_some_and(|p| (p[0] - me[0]).hypot(p[2] - me[2]) < 100.0),
            31 => match t {
                Target::Node(n) => near(
                    self.nodes[*n].position,
                    (NODE_RADIUS * 0.95).powi(2),
                    AT_NODE_DY,
                ),
                _ => false,
            },
            // the body: standing, not in bed, unarmed, empty-handed, free to move
            5 | 24 | 25 | 26 | 47 | 162 => false,
            27 | 59 | 60 | 90 => true,
            // the town: no alarms, standing in a town
            130 => false,
            // atATown (F036791): in a town (Character +0x88)
            86 => self.town.is_some(),
            // the shopping list done (F050255; the target is not looked at)
            85 => self.shop_done,
            // IS_WORKING / IS_WORKING_PRETEND: the check (0x1405974d0) answers false
            123 | 136 => false,
            // machineHasFreeOperatorSlot (F038577): a building target whose functionality
            // takes this operator
            73 => match t {
                Target::Furniture(f) => self.operators[*f].is_none_or(|o| o == self.me),
                _ => false,
            },
            // isInBed; isFullyRested (F038648: only ever for the player's own); canGetUp (F038571)
            83 => self.in_bed.is_some(),
            72 => false,
            45 => true,
            // allDoorsLocked (F038525): the home has no unlocked door (the package runtime's
            // +0xe8 flag, read while inside the home, is taken to say the same)
            87 => self.home_unlocked_door().is_none(),
            // I_AM_INSIDE_TARGET_BUILDING (F038614): a door's building, else the home
            107 => match t {
                Target::Door(d) => self.inside == Some(self.doors[*d].door.placement),
                _ => self.inside.is_some() && self.inside == self.home,
            },
            // It's locked (F038646): a lock of level 1 or more, locked (and not broken)
            112 => match t {
                Target::Door(d) => self.doors[*d].locked && self.doors[*d].level >= 1,
                _ => false,
            },
            // AT_LOCATION_BUILDING_DOOR_INSIDE (F038526): near the door's inside point
            111 => match t {
                Target::Door(d) => {
                    let q = self.doors[*d].inside;
                    (q[0] - me[0]).hypot(q[2] - me[2]) < DOOR_REACH
                }
                _ => true,
            },
            // DOOR_IS_OPEN (F038692): doors stand closed here
            52 => false,
            // isObjectMine (F038695): (assumption) the home building's doors are the squad's
            70 => match t {
                Target::Door(d) => Some(self.doors[*d].door.placement) == self.home,
                _ => true,
            },
            // strangers in the house (F039856 / P): perception is not ported
            108 | 141 => false,
            // arms usable (F050241), no electricity wanted (F038522: a chair needs none)
            38 | 93 => true,
            // a chair has no inventory: no empty inputs (F at 0x1405974e0 answers false without
            // one), not full, no unwanted output, no invalid input
            62 | 63 | 89 | 169 => false,
            // F036806: no destination, or the current town is the destination
            AT_DESTINATION_TOWN => self.destination.is_none() || self.town == self.destination,
            // isNoEnemiesInVicinity: AI +0xbc == 0
            6 => self.enemies == 0,
            // isAlliesFighting (F050269): a friendly known in need of help (F050268)
            12 => self.allies_fighting,
            // gotAFoodItem (F038549: food among what it carries, its backpack's too);
            // HAVE_SOME_LOOT_FROM (F038535): (assumption) the same, food taken
            149 => self.has_food,
            68 => self.has_food,
            // using target turret (0x140997d30) / using any turret
            104 => matches!(t, Target::Furniture(f) if self.operators[*f] == Some(self.me)),
            145 => self.turrets.iter().any(|&(f, _)| self.operators[f] == Some(self.me)),
            // isAggressionTowards (F038635): the target character's flag at +0x2b0
            // ((assumption) not identified: taken as its being in a fight)
            16 => match t {
                Target::Person(x) => self.in_fight[*x],
                _ => return None,
            },
            // isAggressionTowardsMe (F038686): condition 16's test on myself
            39 => self.in_fight[self.me],
            // isAtHome (F050262): in the home building
            18 => self.home.is_some() && self.inside == self.home,
            // "I need a medkit." (a first aid kit carried); "My legs are messed up." (F050238)
            23 => self.has_medkit,
            103 => self.legs_hurt,
            // isTargetDown (F at 0x14099a5a0): the target character is knocked out
            7 => match t {
                Target::Person(e) => self.downs[*e],
                _ => false,
            },
            // (harness) isReadyForAction (F050243): the fighter holds its weapon from the start
            37 => true,
            // isWithin50Meters (F038683 with 500): the target's point within 500 units across
            41 => self
                .target_pos(t, pos)
                .is_some_and(|p| (p[0] - me[0]).hypot(p[2] - me[2]) < 500.0),
            // not in water, not swimming, no route trace blocked
            139 | 140 | 69 => false,
            _ => return None,
        })
    }
    fn score(&mut self, d: &TaskDef, t: &Target, _: [f32; 3]) -> f32 {
        // runnable tasks whose Task class is not ported yet cannot be carried out here
        if d.f134 != 0
            && ![
                MOVING,
                RUNNING,
                ATTACKING,
                PATROLLING,
                STAND_AT_NODE,
                PATROL_TOWN,
                FOLLOWING_LEADER,
                OPERATING_MACHINE,
                TURRET_GUNNER,
                OPERATING_PRETEND,
                FIXING_LEGS,
                SHOPPING_VISIT,
                GRABBING_FOOD,
                LOOTING,
                SLEEPING,
                GETTING_OUT_OF_BED,
                ENTERING_BUILDING,
                LOCKING_DOOR,
                TO_DOOR_INSIDE,
            ]
            .contains(&d.task)
        {
            return 0.0;
        }
        match d.score {
            Score::Const(v) => v,
            Score::Random => self.rng.next() * 0.4 + 0.01,
            // F038686 (the leader's factor object is not ported: 1)
            Score::Method(15) => kenshi_ai::follow::score(None),
            // F038650, a townsperson's: half-hearted for 20 game minutes after getting up
            // (the trouble score F038651 and the own-bed-in-home test are not ported)
            Score::Method(30) => {
                if (self.now - self.woke_at) * 60.0 < 20.0 {
                    0.3
                } else {
                    1.0
                }
            }
            // F044530: locking up wanted while the home is not all locked
            Score::Method(31) => {
                if self.home_unlocked_door().is_some() {
                    1.0
                } else {
                    0.0
                }
            }
            // F038651 (AI::scoreGetUpTrouble, "Getting out of bed!"): 2 attacked, 1.5 with
            // enemies about; (not ported) 1 for an intruder in the home and the player check
            Score::Method(28) => {
                if self.attacked {
                    2.0
                } else if self.enemies > 0 {
                    1.5
                } else {
                    0.0
                }
            }
            // F038552: attacking enemies while standing, not passive (CharStats +0x12c) and not
            // F047db1 (not traced)
            Score::Method(52) => {
                if self.passive {
                    0.0
                } else {
                    1.0
                }
            }
            // F038700: 1 (0.1 with the AI +0x310 +0x129 flag, halved without F044612: (harness)
            // neither); with a hold position none for a target outside its leash ((assumption)
            // F038700's tests of it taken as F038560's)
            Score::Method(53) => match (self.hold, t) {
                (Some(h), Target::Person(e)) => {
                    let at = self.positions[*e];
                    let leash = LeashTarget { position: at, helpless: self.downs[*e], fighting_toward_me: self.in_fight[*e] };
                    if kenshi_ai::enemy::in_leash(Some(&kenshi_ai::enemy::Hold { position: h }), self.pos, &leash) { 1.0 } else { 0.0 }
                }
                _ => 1.0,
            },
            // F038547, Getting food: the AI's own score, 1, for any but the player's
            Score::Method(26) => {
                if self.free_will { 1.0 } else { 0.0 }
            }
            // F038553: manning a turret: none passive out of a fight, nor beyond 1.2 x the AI's
            // +0x28 (between 100 and 1000 units; (assumption) +0x28 taken as the squared vision
            // range, so 1000) of it; 2 with enemies about ((assumption) Character +0x260, its
            // target, taken as set), 0.4 when hungry (under 2.1, nothing queued) with none, else
            // 1; none while carrying someone (Character +0x348). (not ported) The player's branch
            // (their Man Turret job).
            Score::Method(35) => {
                if self.carrying || (self.passive && !self.in_fight[self.me]) {
                    return 0.0;
                }
                if let Some(q) = self.target_pos(t, [0.0; 3]) {
                    let me = self.pos;
                    let d2 = (q[0] - me[0]).powi(2) + (q[1] - me[1]).powi(2) + (q[2] - me[2]).powi(2);
                    if d2 > 1_000_000.0 {
                        return 0.0;
                    }
                }
                if self.enemies > 0 {
                    2.0
                } else if self.hunger.0 < 2.1 && self.hunger.1 <= 0.0 {
                    0.4
                } else {
                    1.0
                }
            }
            // F038601: Protect allies' and Protect close allies' finder scores (F050285 /
            // F050287), none while passive or an obedient slave
            Score::Method(14) => {
                if self.passive || self.obedient || self.sneaking {
                    0.0
                } else {
                    let k = if d.task == PROTECT_CLOSE_ALLIES { 1 } else { 0 };
                    self.protect[k].map_or(0.0, |(_, s)| s) * if self.carrying { 0.5 } else { 1.0 }
                }
            }
            // Fixing Legs (F050240): 1 with a leg hurt, else none. (not ported) Its 0.4 while
            // AI +0xb4 runs
            Score::Function(0x1409983b0) => {
                if self.legs_hurt {
                    1.0
                } else {
                    0.0
                }
            }
            // Reigning (F038545): 1 for the squad's leader (ActivePlatoon +0xa0), else none
            Score::Method(67) => {
                if self.leader {
                    1.0
                } else {
                    0.0
                }
            }
            // Self preservation (F038603): 1 while I am fought (+0x2b0, isAggressionTowards);
            // else 0.7 with one known within 181 units whose squad fights ((assumption)
            // Character +0x1a0's +0xa0 taken as its being in a fight); else none
            Score::Method(17) => {
                if self.in_fight[self.me] {
                    1.0
                } else if self.known.iter().any(|k| self.in_fight[k.id] && (k.position[0] - self.pos[0]).powi(2) + (k.position[1] - self.pos[1]).powi(2) + (k.position[2] - self.pos[2]).powi(2) < 33_000.0) {
                    0.7
                } else {
                    0.0
                }
            }
            // Shopping (F050261): 1 while the list wants anything (its daily reset, F050260,
            // runs as the view is made)
            Score::Function(0x140999620) => {
                if self.shop_done {
                    0.0
                } else {
                    1.0
                }
            }
            // F030643: entering a building wanted unless already in it
            Score::Function(0x140317c00) => match t {
                Target::Door(d) if self.inside == Some(self.doors[*d].door.placement) => 0.0,
                _ => 1.0,
            },
            _ => 0.0,
        }
    }
    fn find_target(&mut self, d: &TaskDef, t: &Target, _: [f32; 3]) -> Option<Target> {
        let free = |v: &Self, i: usize| v.owners[i].is_none_or(|o| o == v.me);
        // a node still free or ours of the kind asked keeps (F050264 / F050298)
        let keep = |v: &Self, kind: i32| match t {
            Target::Node(n) if v.nodes[*n].kind == kind && free(v, *n) => Some(*t),
            _ => None,
        };
        match d.task {
            // Wandering town: a random general node of the town (F050264 -> F031809)
            WANDERING_TOWN => keep(self, NODE_GENERAL).or_else(|| self.pick(NODE_GENERAL, false)),
            // Shopkeeper: a shopkeeper node, the home building's first (F050298 -> F050297)
            SHOPKEEPER => keep(self, NODE_SHOPKEEPER).or_else(|| self.pick(NODE_SHOPKEEPER, true)),
            // Static guard: a guard post, the home building's first, else a general node
            // (F050297's fallback for guard posts)
            STATIC_GUARD => keep(self, NODE_GUARDPOST)
                .or_else(|| self.pick(NODE_GUARDPOST, true))
                .or_else(|| self.pick(NODE_GENERAL, true)),
            // Static guard indoors: Static guard's finder (F050300 with its indoors flag;
            // (not ported) the flag's own test)
            STATIC_GUARD_INDOORS => keep(self, NODE_GUARDPOST)
                .or_else(|| self.pick(NODE_GUARDPOST, true))
                .or_else(|| self.pick(NODE_GENERAL, true)),
            // Static town guard (F050301): the guard post it holds, else a random one of the
            // town (F01596a, node kind 2)
            STATIC_TOWN_GUARD => keep(self, NODE_GUARDPOST).or_else(|| self.pick(NODE_GUARDPOST, false)),
            // Shopping (F050263): in the town the shopper is in, for each line still wanted in
            // turn, a random shop counter of the town whose stock has that item function
            // (F051403 -> F051401, F037733); none for any: (a town not the player's) a random
            // building of the town open by day (F051404), its shop counter, else (stand-in for
            // the building itself) a door of it. (not ported) The finder's score: 1 at a counter,
            // 0.4 at a building.
            SHOPPING => {
                let town = self.town?;
                for f in &self.shop_wants {
                    let shops: Vec<usize> = self.counters.iter().filter(|c| c.town == Some(town) && c.functions.contains(f)).map(|c| c.k).collect();
                    if !shops.is_empty() {
                        let k = ((self.rng.next() * shops.len() as f32) as usize).min(shops.len() - 1);
                        return Some(Target::Store(crate::loot::BOX_OWNER + shops[k]));
                    }
                }
                let open: Vec<&crate::shopping::PublicBuilding> = self.public.iter().filter(|b| b.town == Some(town)).collect();
                if open.is_empty() {
                    return None;
                }
                let b = open[((self.rng.next() * open.len() as f32) as usize).min(open.len() - 1)];
                b.counter.map(|k| Target::Store(crate::loot::BOX_OWNER + k)).or(b.door.map(Target::Door))
            }
            // Relaxing: the squad's package handler's chair (F050252 -> F029228)
            RELAXING => self.relax_chair().map(Target::Furniture),
            // Going to bed: the bed lain in, else a free bed of the home (F047587; the town's
            // other beds through the building finder are not ported)
            GOING_TO_BED | GOING_TO_BED_SECURE => self.home_bed().map(Target::Furniture),
            // Locking up: the first unlocked door of the home (F047549)
            LOCKING_UP => self.home_unlocked_door().map(Target::Door),
            // Attacking (its finder F050285 is not ported): (assumption) an order's own target
            // while it stands
            ATTACKING => match t {
                Target::Person(e) if !self.downs[*e] => Some(*t),
                _ => None,
            },
            // Getting food / Looting food (F047552): a search about the package's area for a
            // container with food: (assumption) the nearest within FOOD_SEARCH of me
            GETTING_FOOD | LOOTING_FOOD => {
                let me = self.pos;
                if let Target::Store(o) = *t
                    && self.stores.iter().any(|s| s.0 == o && s.2)
                {
                    return Some(*t);
                }
                self.stores
                    .iter()
                    .filter(|s| s.2 && (s.1[0] - me[0]).hypot(s.1[2] - me[2]) < FOOD_SEARCH)
                    .min_by(|a, b| (a.1[0] - me[0]).hypot(a.1[2] - me[2]).total_cmp(&(b.1[0] - me[0]).hypot(b.1[2] - me[2])))
                    .map(|s| Target::Store(s.0))
            }
            // Turret guard (F047566): with a town to be in, the turret it has if free and in that
            // town, else (F047713's search, (assumption) the nearest) a free one of the town;
            // Turret home guard (F047567): a free turret of the home building
            MAN_THE_TURRETS | TURRET_HOME_GUARD => {
                let free = |f: usize| self.operators[f].is_none_or(|o| o == self.me) && self.turret_taken[f].is_none_or(|o| o == self.me);
                let fits = |f: usize, town: Option<usize>| {
                    free(f)
                        && if d.task == MAN_THE_TURRETS {
                            self.town.is_some() && town == self.town
                        } else {
                            self.home == Some(self.furniture[f].placement)
                        }
                };
                if d.task == MAN_THE_TURRETS && self.town.is_none() {
                    return None;
                }
                if let Target::Furniture(f) = *t
                    && let Some(&(_, town)) = self.turrets.iter().find(|(k, _)| *k == f)
                    && fits(f, town)
                {
                    return Some(*t);
                }
                let me = self.pos;
                self.turrets
                    .iter()
                    .filter(|&&(f, town)| fits(f, town))
                    .min_by(|a, b| {
                        let d = |f: usize| {
                            let q = self.furniture_approach[f];
                            (q[0] - me[0]).hypot(q[2] - me[2])
                        };
                        d(a.0).total_cmp(&d(b.0))
                    })
                    .map(|&(f, _)| Target::Furniture(f))
            }
            // Protect allies / Protect close allies: the ally most in need (F050285 / F050287)
            PROTECT_ALLIES => self.protect[0].map(|(x, _)| Target::Person(x)),
            PROTECT_CLOSE_ALLIES => self.protect[1].map(|(x, _)| Target::Person(x)),
            // Attacking attackers of (F050284): of the ally's attackers within 150 units of it
            // (any distance when it is me) those not friendly nor left alone, AI::findEnemy's
            // pick. (assumption) Without attackers, the platoon's list (+0x50) is taken as the
            // one the ally fights; (not ported) the test that the ally or I know of each.
            ATTACKING_ATTACKERS_OF => {
                let Target::Person(x) = *t else {
                    return None;
                };
                let mut list = self.attackers_of[x].clone();
                if list.is_empty()
                    && let Some(v) = self.fight_target[x]
                {
                    list.push(v);
                }
                let at = self.positions[x];
                let foes: Vec<Known<usize>> = list
                    .into_iter()
                    .filter(|&a| a != self.me && !self.downs[a] && !self.ignored.contains(&a) && !self.friendly(a))
                    .filter(|&a| {
                        let q = self.positions[a];
                        x == self.me || (q[0] - at[0]).powi(2) + (q[1] - at[1]).powi(2) + (q[2] - at[2]).powi(2) <= ATTACKERS_OF_RANGE_SQ
                    })
                    .map(|a| {
                        let q = self.positions[a];
                        Known {
                            id: a,
                            position: q,
                            seen: true,
                            carried: false,
                            lying: false,
                            ally: false,
                            squad_target: true,
                            swinging_at_me: false,
                            leash: LeashTarget { position: q, helpless: false, fighting_toward_me: false },
                        }
                    })
                    .collect();
                find_enemy(&foes, self.pos, &Area::Me, None, false, nearest_score(self.pos)).map(Target::Person)
            }
            // Staying home (F047556): furniture of the home building used for show, of the
            // functions mine, research, refinery, training, crafting and chair (the search's
            // 0x2049a); Reigning (F047557): its throne (1 << 24); Sitting around (F047548): of
            // the building I am in, a training dummy, chair or throne (0x1020080) no one uses.
            // (assumption) The search's pick (F01e1e6) taken as the nearest free.
            STAYING_HOME => self.furniture_for_show(0x2049a, self.home),
            REIGNING => self.furniture_for_show(1 << 24, self.home),
            SITTING_AROUND => self.furniture_for_show(0x1020080, self.inside),
            // Self preservation's finder (0x1405985e0): myself, so the plan makes the aggression
            // towards me stop (condition 39: Attacking attackers of me)
            SELF_PRESERVATION => Some(Target::Person(self.me)),
            // Home territory aggression (F038644): with a home town, of the known enemies those
            // within its radius x 1.3 (F048764) or fighting me (their target, +0x1d8), the best
            // by findEnemy's scoring (F038638); without one, Attacking enemies' finder
            // (F038643). (not ported) Its pass over some of the player's people (F015f82 on
            // their squad).
            HOME_TERRITORY_AGGRESSION => {
                if self.enemies == 0 {
                    return None;
                }
                let me = self.me;
                let near: Vec<Known<usize>> = self
                    .known
                    .iter()
                    .copied()
                    .filter(|k| !self.ignored.contains(&k.id))
                    .filter(|k| match self.home_area {
                        Some((c, r)) => (k.position[0] - c[0]).hypot(k.position[2] - c[2]) < r * 1.3 || self.fight_target.get(k.id).copied().flatten() == Some(me),
                        None => true,
                    })
                    .collect();
                find_enemy(&near, self.pos, &Area::Me, None, false, nearest_score(self.pos)).map(Target::Person)
            }
            // Attacking enemies: AI::findEnemy (F038641) over the known characters, the area
            // around me (mode 2), no hold position; the squad's scoring by nearness (harness).
            // Attacking others (21, F038642): the same over the enemies and neutrals known (one
            // with the task counts the neutrals as enemies, `Person::attacks_neutrals`)
            ATTACKING_ENEMIES | ATTACKING_NEUTRALS => {
                if self.enemies == 0 {
                    return None;
                }
                // (those the AI ignores, F038789, are passed over; the player's leash, F038638
                // with its flag, is the fight's retargeting only, F040734)
                let near: Vec<Known<usize>> = self
                    .known
                    .iter()
                    .copied()
                    .filter(|k| !self.ignored.contains(&k.id))
                    .collect();
                let known = &near;
                let hold = self.hold.map(|position| kenshi_ai::enemy::Hold { position });
                find_enemy(
                    known,
                    self.pos,
                    &Area::Me,
                    hold.as_ref(),
                    false,
                    nearest_score(self.pos),
                )
                .map(Target::Person)
            }
            _ => None,
        }
    }
}

impl View<'_> {
    /// The nearest free furniture of building `at` (a placement) whose BuildingFunction is in
    /// `mask` (1 << function).
    fn furniture_for_show(&self, mask: u32, at: Option<usize>) -> Option<Target> {
        let at = at?;
        let me = self.pos;
        (0..self.furniture.len())
            .filter(|&f| self.furniture[f].placement == at)
            .filter(|&f| (0..32).contains(&self.furniture[f].function) && mask & (1 << self.furniture[f].function) != 0)
            .filter(|&f| self.operators[f].is_none_or(|o| o == self.me))
            .min_by(|&a, &b| {
                let d = |f: usize| (self.furniture[f].position[0] - me[0]).hypot(self.furniture[f].position[2] - me[2]);
                d(a).total_cmp(&d(b))
            })
            .map(Target::Furniture)
    }

    /// One of my faction or an allied faction's (as it passes, disguised).
    fn friendly(&self, x: usize) -> bool {
        self.factions[x] == self.my_faction || self.mine.is_some_and(|r| r.is_ally(self.factions[x].as_deref()))
    }

    /// F047549's door: of the home, the first door not locked.
    fn home_unlocked_door(&self) -> Option<usize> {
        let home = self.home?;
        (0..self.doors.len()).find(|&d| {
            let s = &self.doors[d];
            s.door.placement == home && !(s.locked && s.level >= 1)
        })
    }

    /// F047587 for a townsperson: the bed already lain in, else the home's first bed nobody uses
    /// or has chosen.
    fn home_bed(&self) -> Option<usize> {
        if let Some(b) = self.in_bed {
            return Some(b);
        }
        let home = self.home?;
        self.furniture.iter().enumerate().find_map(|(i, f)| {
            (f.function == BF_BED
                && f.placement == home
                && self.operators[i].is_none_or(|o| o == self.me)
                && self.bed_taken[i].is_none_or(|o| o == self.me))
            .then_some(i)
        })
    }

    /// Package_HangAroundBar: the squad's bar (F029219, once) and this member's chair (F029228).
    fn relax_chair(&mut self) -> Option<usize> {
        let r = self.rng.next();
        let (me, bars) = (self.me, self.bars);
        // F029227 picks among the town's bars: the bar the squad was put down at, else one of
        // the bars of the town it is in
        let (home, town, placed_town) = (self.home, self.town, self.placed_town);
        let state = self.bar_state.as_deref_mut()?;
        if state.bar.is_none() {
            let own = home.filter(|h| bars.contains(h));
            let near: Vec<usize> = bars.iter().copied().filter(|&b| town.is_some() && placed_town.get(b).copied().flatten() == town).collect();
            state.bar = own.or_else(|| choose_bar(&near, r));
        }
        let bar = state.bar?;
        let chairs: Vec<Chair> = self
            .furniture
            .iter()
            .enumerate()
            .filter(|(_, f)| f.function == BF_CHAIR)
            .map(|(i, f)| Chair {
                id: i,
                position: f.position,
                building: f.placement,
                taken: self.operators[i].is_some_and(|o| o != me),
            })
            .collect();
        let others: Vec<usize> = state
            .held
            .iter()
            .filter(|(m, _)| **m != me)
            .map(|(_, c)| *c)
            .collect();
        let held = state.held.get(&me).copied();
        let chair = choose_chair(held, &chairs, bar, self.placed_at[bar], &others)?;
        if std::env::var("KENSHI_BAR_DEBUG").is_ok() {
            eprintln!("relax {me}: bar {bar} at {:?} (bars {:?}), chair {chair} at {:?} of building {}", self.placed_at[bar], bars, self.furniture[chair].position, self.furniture[chair].placement);
        }
        state.held.insert(me, chair);
        Some(chair)
    }

    /// F050297 / F031809: the home building's free node of a kind (when asked), else a random
    /// free one of the town (within 300 m).
    fn pick(&mut self, kind: i32, home_first: bool) -> Option<Target> {
        let free =
            |i: usize| self.owners[i].is_none_or(|o| o == self.me) && self.nodes[i].kind == kind;
        // (assumption) a node this person already holds stays theirs: the static guard finder
        // (0x14099f650) is not ported
        if home_first
            && let Some(i) = (0..self.nodes.len())
                .find(|&i| self.owners[i] == Some(self.me) && self.nodes[i].kind == kind)
        {
            return Some(Target::Node(i));
        }
        if home_first
            && let Some(h) = self.home
            && let Some(i) =
                (0..self.nodes.len()).find(|&i| free(i) && self.nodes[i].placement == h)
        {
            return Some(Target::Node(i));
        }
        // of the town: the nodes of the buildings of the home's town (else the town it is in)
        let town = self.home.and_then(|h| self.placed_town.get(h).copied().flatten()).or(self.town);
        let near = |i: usize| town.is_some() && self.placed_town.get(self.nodes[i].placement).copied().flatten() == town;
        let all: Vec<usize> = (0..self.nodes.len()).filter(|&i| free(i) && near(i)).collect();
        if all.is_empty() {
            return None;
        }
        let k = ((self.rng.next() * all.len() as f32) as usize).min(all.len() - 1);
        Some(Target::Node(all[k]))
    }
}

impl Mind for View<'_> {
    fn hour(&self) -> i32 {
        (self.start_hour + self.now).rem_euclid(24.0) as i32
    }
    fn rand01(&mut self) -> f32 {
        self.rng.next()
    }
    fn now(&self) -> f64 {
        self.now
    }
    fn free_will(&self) -> bool {
        self.free_will
    }
}

/// The player's AI options (the overview window's AI tab, F035130: the player's flags at
/// +0x110 .. +0x117 and the player faction's "squad formation"), each with the game's tooltip:
/// - ditch items: "If they have nothing else to do, characters will automatically put any excess
///   materials they are carrying into storage.";
/// - sleep when injured: "Characters will automatically go to bed if they are injured and have
///   nothing else to do";
/// - sit when idle: "Idle characters at home will sit down on chairs and benches, if you have
///   any";
/// - heal, help, rescue allies: the medic, any, the rescue job, for allied factions' people too;
/// - share food: "Characters in a squad will automatically share any of their food that is in a
///   backpack (if in range).";
/// - feed animals: "If food is running low, you can elect not to feed your animals.";
/// - shoot first: "Your turret gunners will shoot at any potentially dangerous but neutral
///   targets, such as wild animals or bandits, without waiting for them to attack the base first.";
/// - formation: "Formation used when squad members are following a leader" (0 Random, 1 Caravan,
///   2 Military).
///
/// The defaults are the player object's (F045553: +0x110 .. +0x113 off, +0x114 .. +0x119 on,
/// +0x11a off): heal, help and rescue allies off; feed animals, share food, sleep when injured,
/// ditch items and sit when idle on; shoot first off; (assumption) the formation Random.
/// Sharing food and feeding animals act in the hunger step; sleeping, sitting, ditching and
/// healing / rescuing allies in crate::routines; helping allies is the Protect allies job's
/// (F050285); shooting first in crate::turrets (F033850); the formation in group moves
/// (`squad_spots`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AiOptions {
    pub ditch_items: bool,
    pub sleep_injured: bool,
    pub sit_idle: bool,
    pub heal_allies: bool,
    pub help_allies: bool,
    pub rescue_allies: bool,
    pub share_food: bool,
    pub feed_animals: bool,
    pub shoot_first: bool,
    pub formation: i32,
}

impl Default for AiOptions {
    fn default() -> AiOptions {
        AiOptions {
            ditch_items: true,
            sleep_injured: true,
            sit_idle: true,
            heal_allies: false,
            help_allies: false,
            rescue_allies: false,
            share_food: true,
            feed_animals: true,
            shoot_first: false,
            formation: 0,
        }
    }
}

/// How near a squad mate must be to share its food (units, F039268).
pub const SHARE_FOOD_RANGE: f32 = 200.0;

/// The faction's "squad formation" of a squad template (loose when it has none).
/// A squad template's faction (string id).
/// (assumption: from F040415's part states and the game's own help text) whether a knocked-out
/// character comes round: the knockout clock run out, no blood-loss knockout (cleared above 25
/// blood), and every vital part above the KO point (F040402). Characters get up with vital
/// parts still below zero, as in the game.
pub fn wake_ready(m: &kenshi_char::medical::Medical, stats: &kenshi_char::medical::Stats, c: &kenshi_char::medical::Constants) -> bool {
    if m.dead || m.knockout_time > 0.0 || m.blood_ko {
        return false;
    }
    let ko = kenshi_char::medical::Medical::ko_point(stats, c);
    m.parts.iter().filter(|p| p.vital).all(|p| p.health() >= ko)
}

impl Sim {
    /// @fn F036917
    ///
    /// The AI's ignore map run down: each entry's seconds fall; one run out is let go once its
    /// one can be reached again (F040601: the navmesh's reachability from the character's face,
    /// F032192), else held 20 s more; one let go a tick (as the original breaks off after it).
    /// Gone (dead) ones are let go.
    pub fn ignore_tick(&mut self, dt: f32) {
        for id in 0..self.folk.len() {
            if self.folk[id].ignored.is_empty() {
                continue;
            }
            let me = self.folk[id].mover.position_units();
            let mut drop = None;
            for k in 0..self.folk[id].ignored.len() {
                let (who, left) = self.folk[id].ignored[k];
                let left = left - dt;
                self.folk[id].ignored[k].1 = left;
                if left >= 0.0 {
                    continue;
                }
                let gone = self.folk.get(who).is_none_or(|w| w.dead);
                let there = self.folk.get(who).map(|w| w.mover.position_units());
                let reach = |p: [f32; 3]| self.nav.closest_point(p.map(|v| v * METRES), 50.0).map(|f| f.0);
                let reachable = gone || there.and_then(|t| Some((reach(me)?, reach(t)?))).is_some_and(|(a, b)| self.nav.reachable(a, b));
                if reachable {
                    drop = Some(k);
                    break;
                }
                self.folk[id].ignored[k].1 = IGNORE_SECONDS;
            }
            if let Some(k) = drop {
                self.folk[id].ignored.remove(k);
            }
        }
    }
}

/// @fn F040393
///
/// The name lists: men's from data/namesM.txt and data/namesMF.txt, women's from namesF.txt
/// and namesMF.txt, a name a line.
pub fn name_lists(install: &std::path::Path) -> [Vec<String>; 2] {
    let read = |f: &str| -> Vec<String> {
        std::fs::read_to_string(install.join("data").join(f)).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
    };
    let both = read("namesMF.txt");
    let mut men = read("namesM.txt");
    men.extend(both.iter().cloned());
    let mut women = read("namesF.txt");
    women.extend(both);
    [men, women]
}

impl Sim {
    /// @fn F040051
    ///
    /// Names the unnamed: a character whose CHARACTER is "named" gets a name picked at random
    /// from its sex's list (F040051 with the NameGenerator, F040393); one that is not keeps its
    /// record's name (`display_name`). (assumption: the player's people made without a CHARACTER,
    /// the harness's, are named too)
    pub fn name_people(&mut self) {
        for id in 0..self.folk.len() {
            let p = &self.folk[id];
            if !p.name.is_empty() || p.race.is_some() {
                continue;
            }
            // (assumption) the player's people are named in the character editor, which offers a
            // random name: so they always get one
            let named = p.player || p.character.as_deref().and_then(|c| self.data.get(c)).is_some_and(|r| r.bools.get("named").copied().unwrap_or(false));
            if !named {
                continue;
            }
            let list = &self.names[usize::from(self.is_female(id))];
            if list.is_empty() {
                continue;
            }
            let k = ((self.rng.next() * list.len() as f32) as usize).min(list.len() - 1);
            self.folk[id].name = list[k].clone();
        }
    }
}

/// How long the AI leaves alone one it could not get to (F044546: 20 s, renewed by F036917
/// while it still can't be reached).
pub const IGNORE_SECONDS: f32 = 20.0;


/// (harness for the Havok character's physics) an outdoor body (`pos` in metres) stands on the
/// terrain itself: the navmesh steers but is only a rough copy of the ground (up to a metre off
/// on slopes). A building's own navmesh (its floors, however near the ground: a bar's floor a
/// metre up) is stood on; (assumption) so is any navmesh more than 2 m above the ground (a
/// bridge, a platform).
fn stand_on_ground(pos: &mut [f32; 3], terrain: &mut Terrain, nav: &NavWorld) {
    // on a building's own navmesh (a floor, a porch, stairs, however near the ground): on it
    if let Some((f, q)) = nav.closest_point(*pos, 2.0)
        && nav.on_building(f)
        && (q[0] - pos[0]).hypot(q[2] - pos[2]) < 0.3
    {
        pos[1] = q[1];
        return;
    }
    if let Ok(g) = terrain.height_at(pos[0] / METRES, pos[2] / METRES) {
        let ground = g * METRES;
        if pos[1] - ground < 2.0 {
            pos[1] = ground;
        }
    }
}

/// A fighter keeps shooting at its target while that one stands in its crossbow's range, even
/// if the target is not (yet) in the fight itself: one running in to attack is shot at on the way.
fn shooting_at(f: &[kenshi_combat::arena::Fighter], k: usize) -> bool {
    let me = &f[k];
    me.focus.is_some_and(|o| {
        let t = &f[o];
        !t.down
            && t.team != me.team
            && me.foes.as_ref().is_none_or(|v| v.contains(&t.team))
            && kenshi_combat::arena::ranged_usable(me)
            && me.gun.as_ref().is_some_and(|g| g.in_range(me.pos(), t.pos()))
    })
}

/// The AI package of a squad template that runs: the template's "AI packages" value is a
/// priority, the highest overriding the rest while it is active (fcs.def). (assumption) The
/// town assault packages (signal func 20 war assault, 29 passing town assault, 30 occupy town,
/// 31 protect player base, 32 cannibal raid) only become active against a player's town, which
/// the rewrite has none of, so the highest of the others runs.
pub(crate) fn running_package<'a>(data: &'a GameDataContainer, t: &kenshi_data::GameData) -> Option<&'a kenshi_data::GameData> {
    let packs: Vec<(&kenshi_data::GameData, i32)> =
        t.refs("AI packages").iter().filter_map(|(s, v)| Some((data.get(s)?, v[0]))).collect();
    let player_town_only = |p: &kenshi_data::GameData| {
        [20, 29, 30, 31, 32].contains(p.ints.get("signal func").unwrap_or(&0))
    };
    packs
        .iter()
        .filter(|(p, _)| !player_town_only(p))
        .max_by_key(|(_, v)| *v)
        .or_else(|| packs.iter().max_by_key(|(_, v)| *v))
        .map(|(p, _)| *p)
}

pub fn faction_of(t: &kenshi_data::GameData) -> Option<String> {
    t.refs("faction").first().map(|(s, _)| s.clone())
}

pub fn formation_of(data: &GameDataContainer, t: &kenshi_data::GameData) -> FormationKind {
    t.refs("faction")
        .first()
        .and_then(|(s, _)| data.get(s))
        .and_then(|f| f.ints.get("squad formation"))
        .and_then(|&v| FormationKind::from_int(v))
        .unwrap_or(FormationKind::Loose)
}

/// A zone of townsfolk on Kenshi's AI: everything one tick reads and changes.
pub struct Sim {
    pub data: std::sync::Arc<GameDataContainer>,
    /// The install's files (animals' skeletons).
    pub res: std::sync::Arc<kenshi_io::resources::Resources>,
    /// Each animal race's skeleton clip lengths, loaded once.
    pub animal_clips: HashMap<String, std::sync::Arc<HashMap<String, f32>>>,
    pub general: Vec<Node>,
    pub approach: Vec<[f32; 3]>,
    pub arena: Arena,
    pub bar_states: Vec<Option<BarState>>,
    pub bars: Vec<usize>,
    pub bed_taken: Vec<Option<usize>>,
    pub door_states: Vec<DoorState>,
    pub folk: Vec<Person>,
    pub furn: Vec<Furniture>,
    pub furniture_approach: Vec<[f32; 3]>,
    pub inner: Vec<kenshi_world::interior::Interior>,
    pub inside_point: Vec<Option<[f32; 3]>>,
    pub manager: PathManager<usize>,
    pub nav: NavWorld,
    pub operators: Vec<Option<usize>>,
    pub owners: Vec<Option<usize>>,
    pub placed: Vec<kenshi_world::Placement>,
    pub placed_at: Vec<[f32; 3]>,
    /// The town each placement stands in (`current_town` of its place), kept to `placed_at`.
    pub placed_town: Vec<Option<usize>>,
    pub relations: HashMap<String, FactionRelations>,
    pub rng: Lcg,
    pub terrain: Terrain,
    pub towns: Vec<Town>,
    pub dt: f32,
    pub run: f32,
    pub walk: f32,
    pub start_hour: f64,
    pub hour_secs: f64,
    pub vision_mult: f32,
    pub sunrise: f32,
    pub sunset: f32,
    /// Messages for the player (the law's), taken by the game to show.
    pub messages: Vec<String>,
    /// The world beyond the squad (`world`).
    pub world: crate::world::World,
    pub t: f32,
    pub tick: u64,
    /// The player's characters run when ordered (Kenshi's default; R toggles in the game).
    pub player_run: bool,
    /// The install, for loading navmesh tiles as the player's characters travel.
    pub install: std::path::PathBuf,
    /// Each arena side's faction (side 0: none).
    pub sides: Vec<Option<String>>,
    /// What spawning characters needs: the fighting kit, the race record and its movement.
    pub kit: Kit,
    pub race_record: String,
    pub race: RaceMovement,
    /// The roaming population (biomes and the squads spawned from them).
    pub population: Option<Population>,
    /// The wilds' camps and dens (`crate::nests`).
    pub nests: Vec<crate::nests::Nest>,
    /// The level's nest markers (kenshi_world::town::nest_markers), read when first wanted.
    pub nest_markers: Option<Vec<[f32; 3]>>,
    /// The zones whose buildings and residents are in (the first one's from `load`), in the
    /// order they came in (their placements', furniture's and nodes' indices follow it).
    pub zones: std::collections::HashSet<(i32, i32)>,
    pub zone_order: Vec<(i32, i32)>,
    /// Where each zone's placements start among `placed` (a zone's building n is placement
    /// offset + n).
    pub zone_offsets: std::collections::HashMap<(i32, i32), usize>,
    /// The barriers' owners numbered (crate::doors), each door's barrier (gates only) and each
    /// barrier's door.
    pub faction_numbers: std::collections::HashMap<String, u32>,
    pub barrier_of: Vec<Option<usize>>,
    pub barrier_door: Vec<usize>,
    /// Buildings dismantled so far (for the sound), and whether the player's buildings changed
    /// in a way the navmesh must follow (crate::upkeep).
    pub dismantled: u32,
    pub buildings_changed: bool,
    /// Zones come in without their people (a save's people are put back instead).
    pub quiet_zones: bool,
    /// A loaded save's world (squads, towns, relations), put back on the world's first tick.
    pub pending_world: Option<crate::save::SavedWorld>,
    /// Read once for zones added later: the building layouts.
    pub(crate) layouts: Option<Layouts>,
    /// What hunger reads: GLOBAL CONSTANTS "starvation time" (game hours, F032933), "fed
    /// recovery rate mult", "bed hunger rate", "encumbrance hunger rate", and the race's
    /// "hunger rate" (CharStats +0x178).
    pub hunger_c: [f32; 5],
    /// The name lists (NameGenerator, F040393): men's (namesM.txt and namesMF.txt), women's
    /// (namesF.txt and namesMF.txt).
    pub names: [Vec<String>; 2],
    /// The GLOBAL CONSTANTS first aid reads.
    pub first_aid_c: kenshi_char::medical::FirstAidConstants,
    /// The new game's advanced options (`crate::advanced`; "Bandits loot the player": the
    /// world's looters strip the player's knocked-out characters too).
    pub advanced: crate::advanced::Advanced,
    /// (looter, target) pairs done: the looter's memory of having looted them (tag 6,
    /// TARGET_HAS_BEEN_LOOTED).
    pub looted_by: std::collections::HashSet<(usize, usize)>,
    /// The player's buildings (`base`).
    pub buildings: Vec<crate::base::PlayerBuilding>,
    /// The RESEARCH records done.
    pub research_done: Vec<String>,
    /// The research queued (RESEARCH ids), the first being researched.
    pub research_queue: Vec<String>,
    /// The first queued research: its hours of research done, and whether its cost is paid.
    pub research_progress: f32,
    pub research_paid: bool,
    /// Why research can't go on (F035132's messages), cleared when it can.
    pub research_message: Option<String>,
    /// The research rate now (the window's "CURRENT RATE"): the operators' sum.
    pub research_rate: f32,
    /// The base's power: supplied and drawn (units), the batteries' charge and capacity (unit
    /// hours), and whether the draw is met.
    pub power_supply: f32,
    pub power_demand: f32,
    pub battery: f32,
    pub battery_capacity: f32,
    /// What is supplied over what is drawn, at most 1 (F037662).
    pub power_eff: f32,
    /// The base's name and whether it is open to the public (the HUD's town panel, F043042).
    /// "Outpost" until named. Open to the public: Kenshi reads the flag only in the wandering
    /// traders' town finder (F048860, town types 2 3 5 6), which never picks a player town (type
    /// 1); its visitors are the factions' visit campaigns (crate::world).
    pub outpost_name: String,
    pub outpost_public: bool,
    /// The player's outposts' centres (`base::assign_outposts`, F036212).
    pub outposts: Vec<[f32; 3]>,
    /// The player's squads (crate::squads), in the order they are listed.
    pub player_squads: Vec<crate::squads::PlayerSquad>,
    /// The player's faction's name as typed in the FACTION tab ("" its record's).
    pub faction_name: String,
    /// The player's AI options (the overview window's AI tab).
    pub ai_options: AiOptions,
    /// The weather at the base: wind (0 to 1) and rain (set by the game from its weather).
    pub wind: f32,
    pub rain: f32,
    /// A material a builder found none of (for the message), cleared when shown.
    pub missing_material: Option<String>,
    /// The player's money (cats).
    pub money: i32,
    /// When each NPC may next meet the player's squad (game hours), and the conversations NPCs
    /// started, waiting to be shown.
    pub met: HashMap<usize, f64>,
    /// Squads that own slaves still to be made: (first member, SQUAD_TEMPLATE), crate::slavery.
    pub slave_squads: Vec<(usize, String)>,
    /// The squads loaded with prisoners to keep (SQUAD_TEMPLATE "prisoners"): their first member
    /// and template, the prisoners made at the first tick (`spawn_prisoners`).
    pub prisoner_squads: Vec<(usize, String)>,
    /// A new game's people of a CHARACTER with a "slave" state still to be made slaves (the
    /// first tick, `crate::slavery::Sim::start_slaves`).
    pub start_slaves: bool,
    /// Lock attempts this tick (who, whether it opened), for the "Pick failed" / "Pick success!"
    /// floaters (Task_PickLock::update, F031344).
    pub pick_events: Vec<(usize, bool)>,
    /// Those prospecting and how far (`prospect`), and the last results not yet shown.
    pub prospectors: Vec<(usize, f32)>,
    /// Those ordered to train on town training equipment (person, furniture, seconds since the
    /// order; `production`).
    pub trainees: Vec<(usize, usize, f32)>,
    /// The player's errands (`orders`): beds, splints, shackles, cages, attacking whole squads,
    /// disposing of corpses (carrier, body, where to).
    pub bed_errands: Vec<crate::orders::BedErrand>,
    pub splinters: Vec<crate::orders::SplintErrand>,
    pub shackle_work: Vec<crate::orders::ShackleWork>,
    pub cage_errands: Vec<crate::orders::CageErrand>,
    pub attack_all: Vec<(usize, usize)>,
    pub disposers: Vec<(usize, usize, [f32; 3])>,
    pub prospect_result: Option<crate::prospect::ProspectResult>,
    /// Each cage's half size across (units, from its mesh): where its door lets out.
    pub cage_reach: HashMap<usize, f32>,
    /// What the town's machines worked by slaves have made: each furniture's part-made products
    /// and its storage (crate::slavery::slave_work_tick).
    pub machine_stock: HashMap<usize, (Vec<f32>, kenshi_char::inventory::Inventory)>,
    pub dialog_memory: crate::dialogue::DialogMemory,
    /// The weather and ground of the zones round the player's characters, and the effects that
    /// hurt, as the game tells them each frame (crate::hazards); the acid shelters' records.
    pub climate: crate::hazards::Climates,
    pub harms: Vec<crate::hazards::Harm>,
    pub shelters: Option<std::collections::HashSet<String>>,
    /// Where the placed acid shelters stand (units), and how many placements there were when
    /// that was taken (zones loaded since: taken again).
    pub shelter_spots: (usize, Vec<[f32; 3]>),
    /// Each record's weather types (an item's "weather protection" with its amount, a race's
    /// "weather immunity"), read once (`hazards`).
    pub weather_kinds: std::sync::Mutex<HashMap<String, (Vec<i32>, f32)>>,
    /// `race_of`'s drawn races (a look rolled from the character's template and faction race
    /// lists, asked every tick by the weather), kept by person, template and faction.
    pub race_rolls: std::sync::Mutex<HashMap<(usize, Option<String>, Option<String>), Option<String>>>,
    /// Each RACE's "swims" and "swim speed mult" (F033689 +0x79, +0x60), as asked for.
    pub race_water: HashMap<String, (bool, f32)>,
    /// The squads hired on contracts (crate::dialogue::Contract).
    pub contracts: Vec<crate::dialogue::Contract>,
    /// The Kenshi save the game was imported from (crate::save::SavedWorld::kenshi_base).
    pub kenshi_base: Option<String>,
    /// What each squad thinks of others for now (PlatoonMemory short-term tags, T00046), by
    /// `Sim::memory_key`: the player's squad remembered as a whole (`Subject::Squad(0)`).
    pub platoon_memory: HashMap<u64, kenshi_ai::tags::PlatoonMemory<usize, u8>>,
    /// Who was down and dead at the last memory tick (crate::dialogue::memory_tick).
    pub mem_down: Vec<(bool, bool)>,
    pub npc_talks: Vec<(crate::dialogue::Conversation, Vec<crate::dialogue::Effect>)>,
    /// Lines said aloud (speaker, text), for the game to show over their heads (`speech`).
    pub said: Vec<(usize, String)>,
    /// Items on the ground (`loot`), the next one's number, and the towns whose loot is out.
    pub ground: Vec<crate::loot::GroundItem>,
    /// (testing) Plan steps of each task dropped for want of an executor, counted.
    pub dropped_steps: HashMap<u16, u32>,
    /// Each squad's (platoon's) SQUAD_TEMPLATE, and when a "regenerates" one last got a member
    /// back (game hours; `regenerate_tick`).
    pub squad_template: HashMap<usize, String>,
    /// Each squad's hunger (`npc_hunger`, F045433), and the NPCs whose hunger has been set.
    pub squad_hunger: HashMap<usize, f32>,
    pub hunger_started: std::collections::HashSet<usize>,
    pub regenerated: HashMap<usize, f64>,
    pub regen_check: f64,
    /// crate::routines's once-a-second clock.
    pub routine_timer: f32,
    /// Knocked-out intruders being carried out of homes (crate::intruders).
    pub ejecting: Vec<crate::intruders::Eject>,
    /// The observers' notice meters (crate::disguise).
    pub recognition: crate::disguise::Recognition,
    /// What the law reads of each step (crate::law).
    pub law_state: crate::law::LawState,
    /// The buildings' item spots (crate::loot::ItemSpot).
    pub spots: Vec<crate::loot::ItemSpot>,
    /// A loaded game's empty item spots and when each fills (`loot::apply_spot_dues`).
    pub saved_spot_dues: Vec<([f32; 3], f64)>,
    pub next_ground: u64,
    pub looted: std::collections::HashSet<String>,
    /// The containers of the towns and ruins loaded, and what is in them (`loot`).
    pub boxes: Vec<crate::loot::WorldBox>,
    /// Saved containers' contents waiting for their zone (`loot::refill_boxes`).
    pub saved_boxes: Vec<(String, [f32; 3], Vec<crate::save::SavedItem>)>,
    /// The world's artifacts handed out to the towns (`artifacts`, made once).
    pub artifacts: Option<crate::artifacts::Handout>,
    /// When each person may next speak, and next greet the player's characters (game hours);
    /// who was down at the last speech tick.
    pub bark_next: HashMap<usize, f64>,
    pub seen_player: HashMap<usize, f64>,
    pub was_down: std::collections::HashSet<usize>,
    /// Effects of NPC lines run on inside a conversation step.
    pub pending_effects: Vec<crate::dialogue::Effect>,
    pub price_c: crate::trade::PriceConstants,
    /// Each town's prices for trade goods (F048904).
    pub trade_prices: crate::trade::TradePrices,
}

/// The world's roaming population (port_map `zones`): F048140 ZoneManager's 2 s tick keeps the
/// zones round the player topped up with homeless squads from their biome (F048137).
pub struct Population {
    pub map: BiomeMap,
    pub biomes: Vec<Biome>,
    /// Seconds since the last tick (ZoneManager +0xc0).
    pub timer: f32,
    /// The squads spawned: (members, zone spawned in, template).
    pub squads: Vec<(Vec<usize>, (i32, i32), String)>,
    /// GLOBAL population multiplier (options, "Global population multiplier": default 1).
    pub mult: f32,
    /// Each biome's zones (worked out when first needed).
    pub zones: Vec<Vec<(i32, i32)>>,
    /// The town each squad from a town's "roaming squads" came from (by its first member).
    pub homes: HashMap<usize, usize>,
}

impl Sim {
    /// Load zone `zx`.`zy` of an install: its residents, `people` wanderers and the `extras`
    /// squads (`squad:<template>[@slot]` or `demo:<faction>:<count>`).
    pub fn load(install: &Path, zx: i32, zy: i32, people: usize, extras: &[String]) -> Sim {
        // the game's data with the player's mods over it (MODDING.md)
        let order = Install::new(install).game_order().unwrap();
        let data = GameDataContainer::load(&order).unwrap();
        // (a zone without a file: open ground, no buildings)
        let placed = kenshi_world::read_zone(install, zx, zy)
            .map(|z| placements(&z))
            .unwrap_or_default();
        let layouts = Layouts::load(install);
        let all = nodes(&data, &layouts, &placed);
        let mut general: Vec<Node> = all
            .into_iter()
            .filter(|n| n.kind <= NODE_GUARDPOST)
            .collect();
        // (harness) a zone without town nodes (wilderness, or buildings whose layouts are not
        // read): one general node at its first placement, else its centre, so the shell loads
        if general.is_empty() {
            let c = |i: i32| (i as f32 + 0.5) * kenshi_world::ZONE_SIZE - kenshi_world::WORLD_SIZE / 2.0;
            let ground = Terrain::open(install).ok().and_then(|mut t| t.height_at(c(zx), c(zy)).ok()).unwrap_or(0.0);
            let (position, building) = placed
                .first()
                .map_or(([c(zx), ground, c(zy)], String::new()), |p| (p.position, p.building.clone()));
            general.push(Node {
                kind: NODE_GENERAL,
                position,
                rotation: [1.0, 0.0, 0.0, 0.0],
                building,
                placement: 0,
            });
        }
        eprintln!(
            "{} town nodes ({} general)",
            general.len(),
            general.iter().filter(|n| n.kind == NODE_GENERAL).count()
        );
        let towns: Vec<Town> = towns(install, &data);
        let home_town = current_town(&towns, general[0].position, None);
        if let Some(t) = home_town {
            eprintln!(
                "town {} at {:?}, type {}, radius {}",
                towns[t].name,
                towns[t].position,
                towns[t].kind,
                towns[t].radius()
            );
        }
        let mut terrain = Terrain::open(install).unwrap();
        // furniture and the town's bars: buildings whose residents template is designated BD_BAR
        // (F048905)
        let furn = furniture(&data, &layouts, &placed);
        let placed_at: Vec<[f32; 3]> = placed
            .iter()
            .map(|p| {
                [
                    p.position[0],
                    p.world_y.unwrap_or(p.position[1]),
                    p.position[2],
                ]
            })
            .collect();
        let bars: Vec<usize> = (0..placed.len())
            .filter(|&i| {
                placed[i]
                    .residents
                    .as_deref()
                    .and_then(|r| data.get(r))
                    .and_then(|r| r.ints.get("building designation"))
                    == Some(&BD_BAR)
                    && home_town.is_none_or(|t| towns[t].contains(placed_at[i], 1.0))
            })
            .collect();
        eprintln!(
            "{} pieces of furniture, bars: {:?}",
            furn.len(),
            bars.iter()
                .map(|&b| data
                    .get(&placed[b].building)
                    .map_or("?", |b| b.name.as_str()))
                .collect::<Vec<_>>()
        );
        let operators: Vec<Option<usize>> = vec![None; furn.len()];
        // interiors and doors
        let res = kenshi_io::resources::Resources::index(install);
        let inner = interiors(&data, &res, &placed);
        let door_list = doors(&data, &res, &placed);
        eprintln!(
            "{} interior volumes, {} doors",
            inner.len(),
            door_list.len()
        );
        let bed_taken: Vec<Option<usize>> = vec![None; furn.len()];
        let start_hour: f64 = std::env::var("TOWN_START_HOUR")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(12.0);
        let hour_secs: f64 = std::env::var("TOWN_HOUR_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(MINUTE_HOURS);

        // navmesh round the zone
        let mid = general[0].position.map(|v| v * METRES);
        let (tx, ty) = tile_of(mid);
        let nav = NavWorld::load_around(install, tx, ty, 1).unwrap();
        let manager: PathManager<usize> = PathManager::default();

        let race = RaceMovement::from_record(
            data.of_type(ItemType::Race)
                .find(|r| r.name == "Greenlander")
                .unwrap(),
        );
        // the character's own speed (CharMovement +0xb4) and the walking cap
        let run = run_speed(30.0, 70.0, 120.0, 1.0);
        let walk = speed_limit(run, MoveSpeed::Walk.cap(15.0).unwrap(), None);
        let mut rng = Lcg(0x4b656e736869);
        let owners: Vec<Option<usize>> = vec![None; general.len()];
        let furniture_approach: Vec<[f32; 3]> = furn
            .iter()
            .map(|f| {
                let p = f.position.map(|v| v * METRES);
                nav.closest_point(p, 50.0)
                    .map_or(f.position, |(_, q)| q.map(|v| v / METRES))
            })
            .collect();
        // (harness) a building's inside point: the first of its furniture standing inside its
        // interior volume, on the navmesh; a door's inside point: 25 units from the door toward it
        let inside_point: Vec<Option<[f32; 3]>> = (0..placed.len())
            .map(|i| {
                furn.iter()
                    .zip(&furniture_approach)
                    .find(|(f, q)| f.placement == i && building_at(&inner, **q) == Some(i))
                    .map(|(_, q)| *q)
            })
            .collect();
        let door_states: Vec<DoorState> = door_list
            .into_iter()
            .map(|door: Door| {
                let (locked, level) = placed[door.placement]
                    .locks
                    .get(door.index)
                    .copied()
                    .unwrap_or((false, 0));
                let inside = match inside_point[door.placement] {
                    Some(q) => {
                        let (dx, dz) = (q[0] - door.position[0], q[2] - door.position[2]);
                        let l = dx.hypot(dz).max(1e-3);
                        let p = [
                            door.position[0] + dx / l * 25.0,
                            door.position[1],
                            door.position[2] + dz / l * 25.0,
                        ]
                        .map(|v| v * METRES);
                        nav.closest_point(p, 10.0)
                            .map_or(q, |(_, r)| r.map(|v| v / METRES))
                    }
                    None => door.position,
                };
                let place = &placed[door.placement];
                DoorState::new(&data, place, door, locked, level, inside)
            })
            .collect();
        let approach: Vec<[f32; 3]> = general
            .iter()
            .map(|n| {
                let p = n.position.map(|v| v * METRES);
                nav.closest_point(p, 50.0)
                    .map_or(n.position, |(_, q)| q.map(|v| v / METRES))
            })
            .collect();
        // the zone's resident squads (building state "squad template"): the leader takes the
        // package's leader goals, the others its squad goals, as jobs (F028583)
        let mut folk: Vec<Person> = vec![];
        let spawn = |folk: &mut Vec<Person>,
                     at: [f32; 3],
                     home: Option<usize>,
                     jobs: Vec<PackageGoal>,
                     rng: &mut Lcg,
                     leader: Option<usize>,
                     platoon: Option<usize>|
         -> usize {
            let id = folk.len();
            let p = at.map(|v| v * METRES);
            let jitter = [rng.next() * 6.0 - 3.0, 0.0, rng.next() * 6.0 - 3.0];
            let start = nav
                .closest_point([p[0] + jitter[0], p[1], p[2] + jitter[2]], 50.0)
                .map_or(p, |(_, q)| q);
            let mut mover = Mover::for_character(&race, start.map(|v| v / METRES), 1.0, 1.0, false);
            mover.position = start;
            mover.set_max_speed(walk);
            let mut ai = TaskSystem::new(Target::None, start);
            for g in jobs {
                let class = (g.classification as usize).min(3);
                // targeting 2: the squad leader (F028593)
                let target = match (g.targeting, leader) {
                    (2, Some(l)) => Target::Person(l),
                    _ => Target::None,
                };
                ai.jobs[class].push(Task {
                    task: g.task,
                    classification: g.classification,
                    target,
                    target2: Target::None,
                    priority: g.priority,
                    pos: [0.0; 3],
                    hours: g.hours,
                });
            }
            let role = if platoon.is_some() && home.is_none() {
                if leader.is_none() { 5 } else { 6 }
            } else if home.is_none() {
                0
            } else if ai.jobs.iter().flatten().any(|j| j.task == SHOPKEEPER) {
                1
            } else if ai.jobs.iter().flatten().any(|j| j.task == STATIC_GUARD) {
                2
            } else if ai.jobs.iter().flatten().any(|j| j.task == RELAXING) {
                7
            } else if ai.jobs.iter().flatten().any(|j| j.task == PATROL_TOWN) {
                4
            } else {
                3
            };
            folk.push(Person {
                id,
                mover,
                pathing: Pathing::default(),
                ai,
                claimed: None,
                shopping: None,
                shop_visit: None,
                slave: None,
                started: false,
                home,
                role,
                town: None,
                destination: home_town,
                platoon,
                offset: [0.0; 3],
                follow: FollowLeader::default(),
                straight: None,
                // (harness) the facing a character starts with: a random direction
                facing: {
                    let a = rng.next() * std::f32::consts::TAU;
                    [a.sin(), 0.0, a.cos()]
                },
                speed: 0.0,
                ordered_cap: None,
                sneaking: false,
                carrying: None,
                carried_by: None,
                pickup: None,
                own_speed: 0.0,
                name: String::new(),
                inventory: kenshi_char::inventory::Inventory::character(),
                dead: false,
                character: None,
                eat_timer: 0.0,
                shop: None,
                dialogue: None,
                race: None,
                attacks_neutrals: false,
                package: None,
                captures: false,
                duties: 0,
                build_fetch: None,
                stance: Default::default(),
                hold_at: None,
                jobs: Vec::new(),
                jobs_on: false,
                weapon_out: false,
                weapon_idle: 0.0,
                caravan: None,
                first_aid: None,
                loot: None,
                escort: None,
                build: None,
                operate: None,
                haul: None,
                crime: None,
                bounties: Vec::new(),
                heat: 0.0,
                prison: None,
                picking: false,
                shackled: false,
                shaved: false,
                disguised: None,
                disguise_blown: 0.0,
                female: None,
                personality: 0,
                look_race: None,
                escaped: None,
                freeing: None,
                arrest: None,
                surrendered: false,
                playing_dead: false,
                stand_up: false,
                auto_haul: None,
                dust: 0.0,
                hazard: None,
                water: 0,
                float_y: None,
                job: None,
                idle_for: 0.0,
                rest_injured: false,
                age: 1.0,
                ko_target: None,
                fetch: None,
                world_squad: None,
                away: false,
                limbs: Default::default(),
                getting_up: 0.0,
                operating: None,
                inside: None,
                bed: None,
                woke_at: -100.0,
                reserved_bed: None,
                faction: None,
                down: false,
                run_goal: None,
                player: false,
                rethink: false,
                gate_in_way: None,
                ignored: Vec::new(),
                fight_path: Vec::new(),
                bash_order: None,
                door_order: None,
                dismantle: None,
                bash_timer: 0.0,
                stalk: None,
                roads: Default::default(),
            });
            id
        };
        // the squad's members: the leader takes the package's leader goals, the others its squad
        // goals; a squad without a leader entry is led by its first member
        // each squad: its members, formation and its package's "signal func" (which handler the
        // package runtime gets, F028935)
        let mut platoons: Vec<(Vec<usize>, FormationKind, i32)> = vec![];
        #[allow(clippy::too_many_arguments)]
        // which squad template each spawned squad came from: (first member, template), for kit
        let mut kits: Vec<(usize, String)> = Vec::new();
        // the people each spawned squad was made of (`template_roster`), by its first member
        let mut rosters: HashMap<usize, Vec<String>> = HashMap::new();
        let spawn_squad = |folk: &mut Vec<Person>,
                           platoons: &mut Vec<(Vec<usize>, FormationKind, i32)>,
                           rng: &mut Lcg,
                           at: [f32; 3],
                           home: Option<usize>,
                           package: &kenshi_data::GameData,
                           leaders: i32,
                           squad: i32,
                           kind: FormationKind,
                           faction: Option<String>| {
            let (leaders, squad) = if leaders == 0 && squad > 0 {
                (1, squad - 1)
            } else {
                (leaders, squad)
            };
            let pid = platoons.len();
            let mut members = vec![];
            for _ in 0..leaders {
                members.push(spawn(
                    folk,
                    at,
                    home,
                    package_goals(&data, package, Role::Leader),
                    rng,
                    None,
                    Some(pid),
                ));
            }
            let leader = members.first().copied();
            for _ in 0..squad {
                members.push(spawn(
                    folk,
                    at,
                    home,
                    package_goals(&data, package, Role::Squad),
                    rng,
                    leader,
                    Some(pid),
                ));
            }
            for &m in &members {
                folk[m].faction = faction.clone();
                folk[m].package = Some(package.string_id.clone());
            }
            platoons.push((
                members,
                kind,
                *package.ints.get("signal func").unwrap_or(&0),
            ));
        };
        for (pi, pl) in placed.iter().enumerate() {
            let Some(t) = pl.squad.as_deref().and_then(|s| data.get(s)) else {
                continue;
            };
            let package = t
                .refs("AI packages")
                .iter()
                .max_by_key(|(_, v)| v[0])
                .and_then(|(s, _)| data.get(s));
            let Some(package) = package else { continue };
            let at = [
                pl.position[0],
                pl.world_y.unwrap_or(pl.position[1]),
                pl.position[2],
            ];
            let roster = template_roster(t, 1.0, &mut || rng.next());
            let (leaders, squad) = (i32::from(!roster.is_empty()), roster.len().saturating_sub(1) as i32);
            rosters.insert(folk.len(), roster);
            eprintln!(
                "{} at {}: {} leader(s), {} others, package {}",
                t.name,
                data.get(&pl.building).map_or("?", |b| &b.name),
                leaders,
                squad,
                package.name
            );
            let kind = formation_of(&data, t);
            kits.push((folk.len(), t.string_id.clone()));
            spawn_squad(
                &mut folk,
                &mut platoons,
                &mut rng,
                at,
                Some(pi),
                package,
                leaders,
                squad,
                kind,
                // F038271: the template's "faction", else its owner's (the town it lives in)
                faction_of(t).or_else(|| kenshi_world::town::current_town(&towns, at, None).or(home_town).and_then(|k| towns[k].faction.clone())),
            );
        }
        // the town's bar squads (TOWN "bar squads": the ref's first value how many, its second
        // the % chance of each) at its bar. (stand-in) Rolled once as the zone first loads: the
        // town population manager that times them is not ported
        if let (Some(t), Some(&bar)) = (home_town, bars.first())
            && let Some(town_rec) = data.get(&towns[t].record)
        {
            let mut wanted: Vec<String> = Vec::new();
            for (sid, v) in town_rec.refs("bar squads") {
                let chance = if v[1] > 0 { v[1] as f32 } else { 100.0 };
                for _ in 0..v[0].max(1) {
                    if rng.next() * 100.0 < chance {
                        wanted.push(sid.clone());
                    }
                }
            }
            for sid in &wanted {
                let Some(t) = data.get(sid) else { continue };
                let Some(package) = t
                    .refs("AI packages")
                    .iter()
                    .max_by_key(|(_, v)| v[0])
                    .and_then(|(s, _)| data.get(s))
                else {
                    continue;
                };
                let roster = template_roster(t, 1.0, &mut || rng.next());
                let (leaders, squad) = (i32::from(!roster.is_empty()), roster.len().saturating_sub(1) as i32);
                rosters.insert(folk.len(), roster);
                eprintln!(
                    "bar squad {}: {leaders} leader(s), {squad} others, package {}",
                    t.name, package.name
                );
                kits.push((folk.len(), t.string_id.clone()));
                spawn_squad(
                    &mut folk,
                    &mut platoons,
                    &mut rng,
                    placed_at[bar],
                    Some(bar),
                    package,
                    leaders,
                    squad,
                    formation_of(&data, t),
                    // F038271: the template's "faction", else the town's
                    faction_of(t).or_else(|| home_town.and_then(|k| towns[k].faction.clone())),
                );
            }
        }
        // squads asked for on the command line, at the town's centre; (harness) they arrive facing
        // the residents' middle
        let centre = home_town.map_or(general[0].position, |t| towns[t].position);
        let first_extra = folk.len();
        let residents: Vec<[f32; 3]> = folk.iter().map(|p| p.mover.position_units()).collect();
        // (each "kit:" one of the game start's squad templates, in turn)
        let mut player_kits: Vec<String> = Vec::new();
        // a game start's faction relations (faction, value) and research done
        let mut start_relations: Vec<(String, f32)> = Vec::new();
        let mut start_research: Vec<String> = Vec::new();
        // the player's money: the game start's "money", (harness) 1000 cats when launched with no
        // start
        let mut money = 1000;
        // the new game's advanced options (crate::advanced)
        let mut advanced = crate::advanced::Advanced::default();
        for extra in extras {
            if let Some(a) = extra.strip_prefix("advanced:") {
                advanced = crate::advanced::Advanced::from_extra(a);
                continue;
            }
            if let Some(m) = extra.strip_prefix("money:") {
                money = m.parse().unwrap_or(0);
                continue;
            }
            if let Some(r) = extra.strip_prefix("relation:")
                && let Some((f, v)) = r.rsplit_once('=')
            {
                start_relations.push((f.to_string(), v.parse().unwrap_or(0.0)));
                continue;
            }
            if let Some(r) = extra.strip_prefix("research:") {
                start_research.push(r.to_string());
                continue;
            }
            if let Some(t) = extra.strip_prefix("kit:") {
                // the player's squad equipped as this squad template's characters (a game start)
                player_kits.push(t.to_string());
                continue;
            }
            if extra.starts_with("spawn:") {
                continue;
            }
            if let Some(n) = extra.strip_prefix("player:") {
                // the player's squad: Nameless, no goals of its own; it starts where the game
                // start puts it ("spawn:x,z", F046699: the start's town or position, on the
                // navmesh there), else (harness) at the town's first general node outdoors
                let n: usize = n.parse().expect("player:<count>");
                let spawn_at = extras.iter().find_map(|e| e.strip_prefix("spawn:")).and_then(|v| {
                    let (x, z) = v.split_once(',')?;
                    let (x, z): (f32, f32) = (x.trim().parse().ok()?, z.trim().parse().ok()?);
                    let y = terrain.height_at(x, z).unwrap_or(0.0);
                    nav.closest_point([x, y, z].map(|v| v * METRES), 300.0).map(|(_, q)| q.map(|v| v / METRES))
                });
                let centre = spawn_at.unwrap_or_else(|| {
                    approach
                        .iter()
                        .copied()
                        .find(|q| building_at(&inner, *q).is_none())
                        .unwrap_or(centre)
                });
                let pid = platoons.len();
                let mut members = vec![];
                for _ in 0..n {
                    let id = spawn(&mut folk, centre, None, vec![], &mut rng, None, Some(pid));
                    folk[id].player = true;
                    folk[id].role = 8;
                    folk[id].faction = Some(PLAYER_FACTION.to_string());
                    members.push(id);
                }
                eprintln!("player squad: {n}");
                platoons.push((members, FormationKind::Loose, 0));
                continue;
            }
            let (package, leaders, squad, kind, label, faction) =
                if let Some(name) = extra.strip_prefix("squad:") {
                    // a squad template as the data has it; `@n` picks its package of slot n (the
                    // "AI packages" value: 0 at home, 1 roaming, 2 retreating), else the highest
                    let (name, slot) = match name.rsplit_once('@') {
                        Some((n, k)) => (n, k.parse::<i32>().ok()),
                        None => (name, None),
                    };
                    let t = data
                        .of_type(ItemType::SquadTemplate)
                        .find(|t| t.name == name)
                        .unwrap_or_else(|| panic!("no squad template {name}"));
                    let package = t
                        .refs("AI packages")
                        .iter()
                        .filter(|(_, v)| slot.is_none_or(|k| v[0] == k))
                        .max_by_key(|(_, v)| v[0])
                        .and_then(|(s, _)| data.get(s))
                        .expect("package");
                    let leaders: i32 = t.refs("leader").iter().map(|(_, v)| v[0].max(1)).sum();
                    let squad: i32 = t.refs("squad").iter().map(|(_, v)| v[0]).sum();
                    (
                        package,
                        leaders,
                        squad,
                        formation_of(&data, t),
                        t.name.clone(),
                        faction_of(t),
                    )
                } else if let Some(rest) = extra.strip_prefix("demo:") {
                    // (harness) a squad of greenlanders of a faction's formation on the
                    // "wander town as a squad" package
                    let (faction, n) = rest.rsplit_once(':').expect("demo:<faction>:<count>");
                    let f = data
                        .of_type(ItemType::Faction)
                        .find(|f| f.name == faction)
                        .unwrap_or_else(|| panic!("no faction {faction}"));
                    let package = data
                        .of_type(ItemType::AiPackage)
                        .find(|p| p.name == "wander town as a squad")
                        .expect("package");
                    let kind = f
                        .ints
                        .get("squad formation")
                        .and_then(|&v| FormationKind::from_int(v))
                        .unwrap_or(FormationKind::Loose);
                    (
                        package,
                        1,
                        n.parse::<i32>().unwrap() - 1,
                        kind,
                        format!("{faction} (demo)"),
                        Some(f.string_id.clone()),
                    )
                } else {
                    panic!("extra squads are squad:<template> or demo:<faction>:<count>");
                };
            eprintln!(
                "{label}: {leaders} leader(s), {squad} others, package {}, formation {kind:?}",
                package.name
            );
            if let Some(name) = extra.strip_prefix("squad:") {
                let name = name.rsplit_once('@').map_or(name, |(n, _)| n);
                if let Some(t) = data.of_type(ItemType::SquadTemplate).find(|t| t.name == name) {
                    kits.push((folk.len(), t.string_id.clone()));
                }
            }
            spawn_squad(
                &mut folk,
                &mut platoons,
                &mut rng,
                centre,
                None,
                package,
                leaders,
                squad,
                kind,
                faction,
            );
        }
        if !residents.is_empty() {
            let n = residents.len() as f32;
            let mid = residents
                .iter()
                .fold([0.0f32; 3], |m, q| [m[0] + q[0] / n, 0.0, m[2] + q[2] / n]);
            for p in folk.iter_mut().skip(first_extra) {
                let me = p.mover.position_units();
                let (dx, dz) = (mid[0] - me[0], mid[2] - me[2]);
                let l = dx.hypot(dz).max(1e-3);
                p.facing = [dx / l, 0.0, dz / l];
            }
        }
        // and some wanderers
        for _ in 0..people {
            let n = &general[(rng.next() * general.len() as f32) as usize % general.len()];
            let wander = PackageGoal {
                task: WANDERING_TOWN,
                classification: NON_URGENT,
                priority: 1.0,
                hours: (0, 100),
                targeting: 0,
                ending: 0,
                name: "Wandering town".into(),
            };
            spawn(
                &mut folk,
                n.position,
                None,
                vec![wander],
                &mut rng,
                None,
                None,
            );
        }
        // everyone's kit: their squad template's characters' clothing, weapons and inventory
        // (leaders first, then the squad, then the choose-from list, in turn); the player's squad
        // from the start's template; anyone else the harness kit they are drawn in
        {
            let mut roll_seed = 0x9e37_79b9_u64;
            let mut roll = move || {
                roll_seed ^= roll_seed << 13;
                roll_seed ^= roll_seed >> 7;
                roll_seed ^= roll_seed << 17;
                (roll_seed % 10_000) as f32 / 100.0
            };
            let chars_of = |sid: &str| -> Vec<String> {
                let Some(t) = data.get(sid) else { return Vec::new() };
                // a game start's "squad" may name a CHARACTER itself (Son of a Captain's
                // Citizen, the Cannibal Hunters' Adventurers): that one person
                if t.item_type() == Some(ItemType::Character) {
                    return vec![sid.to_string()];
                }
                let mut v = Vec::new();
                for cat in ["leader", "squad", "choosefrom list"] {
                    for (c, n) in t.refs(cat) {
                        for _ in 0..n.first().copied().unwrap_or(1).max(1) {
                            v.push(c.clone());
                        }
                    }
                }
                v
            };
            let mut ranges: Vec<(usize, usize, String)> = Vec::new();
            for (k, (start, sid)) in kits.iter().enumerate() {
                let end = kits.get(k + 1).map_or(folk.len(), |n| n.0);
                ranges.push((*start, end, sid.clone()));
            }
            // the game start's squads (F046699 spawns each of its "squad" refs): each template
            // its own people, its leaders' and squad's counts (at least one), the last the rest
            if !player_kits.is_empty() {
                let players: Vec<usize> = folk.iter().filter(|p| p.player).map(|p| p.id).collect();
                if let (Some(&first), Some(&last)) = (players.first(), players.last()) {
                    let mut a = first;
                    for (k, t) in player_kits.iter().enumerate() {
                        let n = data.get(t).map_or(1, |r| r.refs("leader").iter().chain(r.refs("squad")).map(|(_, v)| v[0].max(1) as usize).sum::<usize>().max(1));
                        let b = if k + 1 == player_kits.len() { last + 1 } else { (a + n).min(last + 1) };
                        if a < b {
                            ranges.push((a, b, t.clone()));
                        }
                        a = b;
                    }
                }
            }
            for p in folk.iter_mut() {
                let template = ranges.iter().rev().find(|(a, b, _)| (*a..*b).contains(&p.id)).and_then(|(a, _, sid)| {
                    let chars = rosters.get(a).cloned().filter(|r| !r.is_empty()).unwrap_or_else(|| chars_of(sid));
                    (!chars.is_empty()).then(|| chars[(p.id - a) % chars.len()].clone())
                });
                p.character = template.clone();
                p.inventory = match template.as_deref().and_then(|c| data.get(c)) {
                    Some(c) => kenshi_char::inventory::Inventory::from_template(&data, c, &mut roll),
                    None => {
                        let mut inv = kenshi_char::inventory::Inventory::character();
                        for name in std::iter::once(KIT_WEAPON).chain(KIT_OUTFIT) {
                            if let Some(r) = data.records.values().find(|r| r.name == name && r.item_type().is_some()) {
                                inv.add(kenshi_char::inventory::Item::from_record(r, 1), true);
                            }
                        }
                        inv
                    }
                };
            }
        }
        // each squad's packages' goals that run as routines (capturing, kidnapping, arresting,
        // watching slaves, getting food), from any of its template's packages
        for (first, sid) in &kits {
            let Some(t) = data.get(sid) else { continue };
            let mut goals: Vec<u16> = Vec::new();
            for pk in t.refs("AI packages").iter().filter_map(|(pk, _)| data.get(pk)) {
                for r in [Role::Leader, Role::Squad] {
                    goals.extend(package_goals(&data, pk, r).iter().map(|g| g.task));
                }
            }
            let captures = goals.iter().any(|&g| g == CAPTURE_SLAVES || g == CARRY_SLAVES || g == KIDNAPPER);
            let duties = duties_of(goals);
            let Some(pid) = folk.get(*first).and_then(|p| p.platoon) else { continue };
            for p in folk.iter_mut().filter(|p| p.platoon == Some(pid) && !p.player && p.race.is_none()) {
                p.captures |= captures;
                p.duties |= duties;
            }
        }
        eprintln!(
            "{} people (arresting {}, watching slaves {}, getting food {}, kidnapping {}, capturing {})",
            folk.len(),
            folk.iter().filter(|p| p.duties & DUTY_ARRESTS != 0).count(),
            folk.iter().filter(|p| p.duties & DUTY_WATCHES_SLAVES != 0).count(),
            folk.iter().filter(|p| p.duties & DUTY_GETS_FOOD != 0).count(),
            folk.iter().filter(|p| p.duties & DUTY_KIDNAPS != 0).count(),
            folk.iter().filter(|p| p.captures).count()
        );
        // faction relations of the factions present (F041513) and each one's side in the arena
        let mut sides: Vec<Option<String>> = vec![None];
        for p in &folk {
            if !sides.contains(&p.faction) {
                sides.push(p.faction.clone());
            }
        }
        let relations: HashMap<String, FactionRelations> = sides
            .iter()
            .flatten()
            .filter_map(|f| Some((f.clone(), FactionRelations::from_data(data.get(f)?, &data))))
            .collect();
        let side_of = |f: &Option<String>| sides.iter().position(|s| s == f).unwrap_or(0) as u16;
        let side_of: Vec<u16> = folk.iter().map(|p| side_of(&p.faction)).collect();
        for f in sides.iter().flatten() {
            let r = &relations[f];
            let enemies: Vec<&str> = sides
                .iter()
                .flatten()
                .filter(|o| r.is_enemy(Some(o)))
                .map(|o| data.get(o).map_or(o.as_str(), |d| d.name.as_str()))
                .collect();
            eprintln!(
                "faction {}: enemies here {:?}",
                data.get(f).map_or("?", |d| d.name.as_str()),
                enemies
            );
        }
        // the combat arena: one fighter per person, out of the fight until it starts one
        QUIET.store(std::env::var("TOWN_COMBAT_LOG").is_err(), Ordering::Relaxed);
        let race_rec = data
            .of_type(ItemType::Race)
            .find(|r| r.name == "Greenlander")
            .unwrap();
        let vision_mult = race_rec
            .floats
            .get("vision range mult")
            .copied()
            .unwrap_or(1.0);
        let (lengths, roots) = skeleton_clips(&res, race_rec).expect("skeleton");
        let kit = Kit::new(&data, KIT_WEAPON, &KIT_OUTFIT).expect("kit");
        let mut arena = Arena::new(
            vec![],
            Shared::load(&data, race_rec, lengths, 7),
            roots,
            data.get("110-gamedata.quack")
                .and_then(|g| g.floats.get("animation blend rate").copied())
                .unwrap_or(4.0),
        );
        // (testing) KENSHI_ARENA_ROWS: the arena prints its fighters' clips ("f,..." rows)
        arena.rows = std::env::var("KENSHI_ARENA_ROWS").is_ok();
        // the options' "Harpoon limit" (settings.cfg "harpoonLimit"; (assumption) 60 without it)
        if let Some(v) = kenshi_io::cfg::load(install, "settings.cfg").get("harpoonLimit").and_then(|v| v.trim().parse::<f32>().ok()) {
            arena.sh.harpoon_limit = v.clamp(10.0, 500.0) as usize;
        }
        for p in &folk {
            let side = side_of[p.id];
            let mut f = Fighter::new(
                &data,
                race_rec,
                &kit,
                &p.id.to_string(),
                side,
                KIT_SKILL,
                p.mover.position_units(),
            );
            f.mover.position = p.mover.position;
            f.active = false;
            f.foes = Some(
                sides
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| {
                        p.faction
                            .as_ref()
                            .is_some_and(|f| relations[f].is_enemy(o.as_deref()))
                    })
                    .map(|(k, _)| k as u16)
                    .collect(),
            );
            arena.add(f);
        }
        let gc = data.get("110-gamedata.quack");
        let sun = |k: &str, d: f32| gc.and_then(|g| g.floats.get(k).copied()).unwrap_or(d);
        let (sunrise, sunset) = (sun("sunrise", 5.0), sun("sunset", 23.0));
        // each platoon's formation (F028591): the leader first, every member a squad role (the
        // leader 2) and its mover radius
        for (members, kind, _) in platoons.iter().filter(|(m, _, _)| !m.is_empty()) {
            let ms: Vec<Member> = members
                .iter()
                .enumerate()
                .map(|(k, &i)| Member {
                    role: if k == 0 { 2 } else { 0 },
                    radius: folk[i].mover.radius * 10.0,
                    animal: false,
                })
                .collect();
            let off = layout(*kind, &ms, ms[0].radius, &mut || rng.next());
            for (k, &i) in members.iter().enumerate() {
                folk[i].offset = off[k];
            }
        }

        // Package_HangAroundBar for the packages whose signal makes one (17, 26, 27: F028935)
        let bar_states: Vec<Option<BarState>> = platoons
            .iter()
            .map(|(_, _, sig)| [17, 26, 27].contains(sig).then(BarState::default))
            .collect();

        let dt = 1.0 / 60.0;
        let race_record = race_rec.string_id.clone();
        let race_record_id = race_record.clone();
        let data_arc = std::sync::Arc::new(data);
        let shop_kits = kits.clone();
        let mut sim = Sim {
            dropped_steps: HashMap::new(),
            data: data_arc.clone(),
            res: std::sync::Arc::new(res),
            animal_clips: HashMap::new(),
            install: install.to_path_buf(),
            general,
            approach,
            arena,
            bar_states,
            bars,
            bed_taken,
            door_states,
            folk,
            furn,
            furniture_approach,
            inner,
            inside_point,
            manager,
            nav,
            operators,
            owners,
            placed,
            placed_at,
            placed_town: Vec::new(),
            relations,
            rng,
            terrain,
            towns,
            dt,
            run,
            walk,
            start_hour,
            hour_secs,
            vision_mult,
            sunrise,
            sunset,
            messages: Vec::new(),
            world: Default::default(),
            t: 0.0,
            tick: 0,
            player_run: true,
            sides,
            kit,
            race_record,
            race,
            population: BiomeMap::load(install).map(|map| Population {
                map,
                biomes: biomes(&data_arc),
                timer: 0.0,
                squads: vec![],
                // the install's settings.cfg "Global population multiplier" (F032960)
                mult: std::fs::read_to_string(install.join("settings.cfg"))
                    .ok()
                    .and_then(|c| c.lines().find_map(|l| l.strip_prefix("Global population multiplier=").and_then(|v| v.trim().parse::<f32>().ok())))
                    .unwrap_or(1.0),
                zones: vec![],
                homes: HashMap::new(),
            }),
            nests: Vec::new(),
            nest_markers: None,
            zones: [(zx, zy)].into_iter().collect(),
            zone_offsets: [((zx, zy), 0)].into_iter().collect(),
            faction_numbers: Default::default(),
            barrier_of: Vec::new(),
            barrier_door: Vec::new(),
            dismantled: 0,
            buildings_changed: false,
            zone_order: vec![(zx, zy)],
            quiet_zones: false,
            pending_world: None,
            layouts: None,
            money,
            met: HashMap::new(),
            pick_events: Vec::new(),
            prospectors: Vec::new(),
            trainees: Vec::new(),
            bed_errands: Vec::new(),
            splinters: Vec::new(),
            shackle_work: Vec::new(),
            cage_errands: Vec::new(),
            attack_all: Vec::new(),
            disposers: Vec::new(),
            prospect_result: None,
            cage_reach: HashMap::new(),
            machine_stock: HashMap::new(),
            start_slaves: true,
            slave_squads: kits.iter().filter(|(_, t)| data_arc.get(t).is_some_and(|r| !r.refs("slaves").is_empty())).cloned().collect(),
            prisoner_squads: kits.iter().filter(|(_, t)| data_arc.get(t).is_some_and(|r| !r.refs("prisoners").is_empty())).cloned().collect(),
            dialog_memory: Default::default(),
            contracts: Vec::new(),
            kenshi_base: None,
            squad_template: HashMap::new(),
            squad_hunger: HashMap::new(),
            hunger_started: std::collections::HashSet::new(),
            regenerated: HashMap::new(),
            regen_check: 0.0,
            routine_timer: 0.0,
            ejecting: Vec::new(),
            recognition: Default::default(),
            law_state: Default::default(),
            spots: Vec::new(),
            saved_spot_dues: Vec::new(),
            race_water: HashMap::new(),
            climate: Default::default(),
            harms: Vec::new(),
            shelters: None,
            shelter_spots: (usize::MAX, Vec::new()),
            weather_kinds: std::sync::Mutex::new(HashMap::new()),
            race_rolls: std::sync::Mutex::new(HashMap::new()),
            platoon_memory: HashMap::new(),
            mem_down: Vec::new(),
            npc_talks: Vec::new(),
            said: Vec::new(),
            ground: Vec::new(),
            next_ground: 0,
            looted: Default::default(),
            boxes: Vec::new(),
            saved_boxes: Vec::new(),
            artifacts: None,
            bark_next: HashMap::new(),
            seen_player: HashMap::new(),
            was_down: Default::default(),
            pending_effects: Vec::new(),
            price_c: crate::trade::PriceConstants::load(&data_arc),
            trade_prices: crate::trade::TradePrices::default(),
            buildings: Vec::new(),
            research_done: Vec::new(),
            research_queue: Vec::new(),
            research_progress: 0.0,
            research_paid: false,
            research_message: None,
            research_rate: 0.0,
            power_supply: 0.0,
            power_demand: 0.0,
            battery: 0.0,
            battery_capacity: 0.0,
            power_eff: 1.0,
            outpost_name: "Outpost".into(),
            player_squads: Vec::new(),
            faction_name: String::new(),
            outpost_public: false,
            outposts: Vec::new(),
            ai_options: AiOptions::default(),
            wind: 0.0,
            rain: 0.0,
            missing_material: None,
            advanced: advanced.clone(),
            looted_by: Default::default(),
            first_aid_c: {
                let g = |k: &str| data_arc.get("110-gamedata.quack").and_then(|r| r.floats.get(k).copied());
                let d = kenshi_char::medical::FirstAidConstants::default();
                kenshi_char::medical::FirstAidConstants {
                    speed: g("medic speed mult").unwrap_or(d.speed),
                    robot_speed: g("robot medic speed mult").unwrap_or(d.robot_speed),
                    drain: [g("medkit drain 1").unwrap_or(d.drain[0]), g("medkit drain 99").unwrap_or(d.drain[1])],
                }
            },
            names: name_lists(install),
            hunger_c: {
                let g = |k: &str, d: f32| data_arc.get("110-gamedata.quack").and_then(|r| r.floats.get(k).copied()).unwrap_or(d);
                let race = data_arc.get(&race_record_id).and_then(|r| r.floats.get("hunger rate").copied()).unwrap_or(1.0);
                [g("starvation time", 60.0), g("fed recovery rate mult", 3.0), g("bed hunger rate", 0.33), g("encumbrance hunger rate", 0.7), race]
            },
        };
        sim.arena.sh.damage_mult = sim.advanced.damage;
        for id in 0..sim.folk.len() {
            sim.apply_stats(id);
        }
        // trade: the towns' prices for trade goods, then the shops of the squads whose
        // templates sell (their leader the trader)
        let towns: Vec<[f32; 3]> = sim.towns.iter().map(|t| t.position).collect();
        let margins = sim.price_c.margins;
        let rng = &mut sim.rng;
        sim.trade_prices = crate::trade::TradePrices::roll(&sim.data, &towns, margins, &mut || rng.next());
        // the first zone's containers (`loot`)
        let first: Vec<kenshi_world::Placement> = sim.placed.clone();
        sim.add_containers(&first, 0);
        // the game start's "research" done and "faction relations" set (both ways: the player's
        // faction toward them and theirs toward it)
        sim.research_done.extend(start_research);
        for (f, v) in start_relations {
            for (a, b) in [(PLAYER_FACTION.to_string(), f.clone()), (f.clone(), PLAYER_FACTION.to_string())] {
                if !sim.relations.contains_key(&a)
                    && let Some(r) = sim.data.get(&a)
                {
                    let rel = FactionRelations::from_data(r, &sim.data);
                    sim.relations.insert(a.clone(), rel);
                }
                if let Some(r) = sim.relations.get_mut(&a) {
                    r.relations.entry(b).or_insert(kenshi_ai::faction::Relation { forced_ally: false, relation: v }).relation = v;
                }
            }
        }
        for (start, t) in &shop_kits {
            if let Some(pl) = sim.folk.get(*start).and_then(|p| p.platoon) {
                sim.squad_template.insert(pl, t.clone());
                if let Some(r) = data_arc.get(t) {
                    sim.note_squad_hunger(pl, r);
                }
            }
        }
        // F045433: a malnourished game start (Rock Bottom) starts the player's squad hungry
        if let Some(r) = player_kits.first().and_then(|t| data_arc.get(t)) {
            if let Some(pl) = sim.folk.iter().find(|p| p.player).and_then(|p| p.platoon) {
                sim.note_squad_hunger(pl, r);
                let level = sim.squad_hunger[&pl];
                for p in sim.folk.iter().filter(|p| p.player) {
                    sim.arena.f[p.id].medical.hunger.level = level;
                }
            }
        }
        for (k, (start, t)) in shop_kits.iter().enumerate() {
            let end = shop_kits.get(k + 1).map_or(sim.folk.len(), |n| n.0);
            for id in *start..end.min(sim.folk.len()) {
                if !sim.folk[id].player {
                    sim.assign_dialogue(id, Some(t), id == *start);
                }
            }
            sim.open_shop(*start, t);
        }
        sim
    }

    /// Slavers (Capture Slaves, 167; Carry wounded slaves, 168) pick up the knocked out who are
    /// not theirs (F047573's test), within 400 units, one each, and carry them on with the squad
    /// (cages and shackles are `slavery`'s). (stand-in) run as a routine, not planned: the
    /// finder reads the known characters, nearest, where this looks 400 units about.
    fn capture_tick(&mut self) {
        let catchers: Vec<usize> = self
            .folk
            .iter()
            .filter(|p| p.captures && !p.down && !p.dead && p.carrying.is_none() && p.pickup.is_none() && !p.player)
            .map(|p| p.id)
            .collect();
        if std::env::var("KENSHI_CAPTURE_DEBUG").is_ok() {
            let downs: Vec<usize> = self.folk.iter().filter(|p| p.down).map(|p| p.id).collect();
            eprintln!("capture: catchers {catchers:?}, down {downs:?}");
        }
        for c in catchers {
            let at = self.folk[c].mover.position_units();
            let faction = self.folk[c].faction.clone();
            // F038655, the Kidnapper's score: none while two of the squad already carry someone,
            // and only with a cage to take them to (F047589, the building finder of
            // functionality 8, BF_CAGE: any within reach)
            if self.folk[c].duties & DUTY_KIDNAPS != 0 {
                let platoon = self.folk[c].platoon;
                let carrying = self.folk.iter().filter(|p| p.platoon == platoon && p.carrying.is_some()).count();
                let caged = faction.as_deref().is_some_and(|f| self.free_cage(f, at, true).is_some());
                if carrying > 1 || !caged {
                    continue;
                }
            }
            let taken: Vec<usize> = self.folk.iter().filter_map(|p| p.pickup).collect();
            // F047573 (Capturing slave's finder): one lying down, not held, not an animal, not
            // a lawman, trader or diplomat (NPC class 1, 3, 5), not a slave (state 1), not
            // crippled, the nearest
            let class = |p: &Person| p.character.as_deref().and_then(|c| self.data.get(c)).and_then(|c| c.ints.get("NPC class").copied()).unwrap_or(0);
            let prey = self
                .folk
                .iter()
                .filter(|p| p.down && !p.dead && p.carried_by.is_none() && p.prison.is_none() && p.race.is_none() && p.faction != faction && !taken.contains(&p.id))
                .filter(|p| !matches!(class(p), 1 | 3 | 5) && !p.slave.as_ref().is_some_and(|s| s.state == crate::slavery::IS_SLAVE))
                .filter(|p| !(self.limb_lost(p.id, 2) || self.limb_lost(p.id, 3) || (self.limb_lost(p.id, 0) && self.limb_lost(p.id, 1))))
                .map(|p| (p.id, p.mover.position_units()))
                .filter(|(_, q)| (q[0] - at[0]).hypot(q[2] - at[2]) < 400.0)
                .min_by(|a, b| {
                    let d = |q: &[f32; 3]| (q[0] - at[0]).hypot(q[2] - at[2]);
                    d(&a.1).total_cmp(&d(&b.1))
                })
                .map(|(id, _)| id);
            if let Some(v) = prey {
                eprintln!("t {:.1} person {c} goes to capture {v}", self.t);
                self.order_pickup(&[c], v);
            }
        }
    }

    /// Person `id` becomes the ANIMAL_CHARACTER `animal`: its race's body, size and movement, a
    /// fighter of that race with its natural weapon ("weapon") and its stats, grown to its age
    /// (`Fighter::set_animal`, F040054).
    pub fn make_animal(&mut self, id: usize, animal: &str) {
        let data = self.data.clone();
        let Some(a) = data.get(animal) else { return };
        let Some(race) = a.refs("race").first().and_then(|(r, _)| data.get(r)) else { return };
        let rm = kenshi_char::movement::RaceMovement::from_record(race);
        let pos = self.folk[id].mover.position;
        let mut mover = Mover::for_character(&rm, pos.map(|v| v / METRES), 1.0, 1.0, false);
        mover.position = pos;
        mover.set_max_speed(self.walk);
        let p = &mut self.folk[id];
        p.mover = mover;
        p.race = Some(race.string_id.clone());
        p.character = Some(a.string_id.clone());
        p.name = a.name.clone();
        // its inventory, wearing its "backpack" (a pack bull's bags)
        p.inventory = animal_inventory(&data, a, true);
        let old = &self.arena.f[id];
        let (team, foes) = (old.team, old.foes.clone());
        let mut f = Fighter::new(&data, race, &self.kit, &id.to_string(), team, KIT_SKILL, pos.map(|v| v / METRES));
        f.mover.position = pos;
        f.active = false;
        f.foes = foes;
        // (assumption) an animal's natural weapon at quality 50, as the harness had it
        let weapon = a.refs("weapon").first().and_then(|(w, _)| data.get(w)).map(|r| kenshi_char::inventory::Item::from_record(r, 1).with_quality(&data, 50.0));
        // (its bags' modifiers, as a person's backpack's)
        let bags: Vec<kenshi_char::inventory::Item> = self.folk[id].inventory.equipped().filter(|i| i.kind == Some(kenshi_data::ItemType::Container)).cloned().collect();
        f.equip(&data, race, weapon.as_ref(), &bags);
        // its skeleton's clips: it fights with its "attack type" techniques (F040738 "animal")
        let clips = match self.animal_clips.get(&race.string_id) {
            Some(c) => Some(c.clone()),
            None => skeleton_clips(&self.res, race).map(|(l, r)| {
                let c = std::sync::Arc::new(l);
                self.animal_clips.insert(race.string_id.clone(), c.clone());
                self.arena.animal_roots.insert(race.string_id.clone(), std::sync::Arc::new(r));
                c
            }),
        };
        f.clips = Some(clips.unwrap_or_default());
        // its moves' root tracks: the lunges drive it (arena `record`)
        f.roots = self.arena.animal_roots.get(&race.string_id).cloned();
        let mult = kenshi_char::stats::race_multipliers(race);
        let rng = &mut self.rng;
        let mut skills = f.skills.clone();
        skills.levels = kenshi_char::stats::character_levels(&data, a, &mult, &mut || rng.next());
        skills.race_xp = mult;
        f.set_skills(&data, race, skills);
        f.set_animal(&data, race, a, self.folk[id].age);
        f.body.aim = kenshi_combat::arena::bone_height(&self.res, race, "Bip01 Spine1").unwrap_or(f.body.aim);
        self.arena.f[id] = f;
    }

    /// Person `id`'s dialogue package: its squad template's "dialog leader" (the leader) or
    /// "dialog squad" (the rest), else its character's "dialogue package".
    pub fn assign_dialogue(&mut self, id: usize, template: Option<&str>, leader: bool) {
        let data = self.data.clone();
        let from_template = template.and_then(|t| data.get(t)).and_then(|t| {
            let cat = if leader { "dialog leader" } else { "dialog squad" };
            t.refs(cat).first().map(|(s, _)| s.clone())
        });
        let from_character = self.folk[id]
            .character
            .as_deref()
            .and_then(|c| data.get(c))
            .and_then(|c| c.refs("dialogue package").first().map(|(s, _)| s.clone()));
        self.folk[id].dialogue = from_template.or(from_character);
        // (crate::dialogue) its CHARACTER's bounty rolled as it is made
        self.roll_bounty(id);
    }

    /// Person `id` keeps the shop of SQUAD_TEMPLATE `template` when the template has vendor
    /// lists: stocked now (F049418), restocked every "vendors refresh time" game hours.
    pub fn open_shop(&mut self, id: usize, template: &str) {
        let data = self.data.clone();
        let Some(t) = data.get(template) else { return };
        if t.refs("vendors").is_empty() || id >= self.folk.len() {
            return;
        }
        let town = kenshi_world::town::current_town(&self.towns, self.folk[id].mover.position_units(), None);
        let rng = &mut self.rng;
        let (stock, money) = crate::trade::restock(&data, t, &self.price_c, &self.trade_prices, town, &mut || rng.next());
        let every = t.ints.get("vendors refresh time").copied().unwrap_or(24).max(1) as f64;
        let now = self.hours_gone();
        if std::env::var("KENSHI_SHOP_DEBUG").is_ok() {
            let items: Vec<String> = stock.sections[0].items.iter().take(6).map(|p| format!("{} x{}", p.item.name, p.item.quantity)).collect();
            eprintln!(
                "shop {} ({}): {} items, {money} cats, town {:?}: {}",
                id,
                t.name,
                stock.sections[0].items.len(),
                town.map(|k| self.towns[k].name.clone()),
                items.join(", ")
            );
        }
        self.folk[id].shop = Some(crate::trade::Shop { stock, money, restock_at: now + every, template: template.to_string(), town });
    }

    /// What one stack of `item` costs at a shop in `town`: the trader's asking price when the
    /// player is `buying`, else what the trader pays (crate::trade).
    pub fn price(&self, item: &kenshi_char::inventory::Item, town: Option<usize>, buying: bool) -> i32 {
        if buying {
            crate::trade::buy_price(item, &self.data, &self.price_c, &self.trade_prices, town)
        } else {
            crate::trade::sell_price(item, &self.data, &self.price_c, &self.trade_prices, town)
        }
    }

    /// The player buys stock item `idx` of `trader`'s shop for `person`, into section `to` at
    /// (x, y) (None: the first place it fits): the trader's asking price, if the player has it.
    pub fn buy(&mut self, trader: usize, idx: usize, person: usize, to: usize, at: Option<(u32, u32)>) -> Result<i32, String> {
        let Some(shop) = self.folk[trader].shop.as_ref() else { return Err("not a trader".into()) };
        let Some(item) = shop.stock.sections[0].items.get(idx).map(|p| p.item.clone()) else { return Err("no such item".into()) };
        let price = self.price(&item, shop.town, true) * item.quantity as i32;
        if self.money < price {
            return Err(format!("can't afford {} (c.{price}, have c.{})", item.name, self.money));
        }
        let inv = &self.folk[person].inventory;
        let Some(sec) = inv.sections.get(to) else { return Err("no such section".into()) };
        let (x, y) = match at {
            Some(c) => c,
            None => sec.first_fit(&item).ok_or("no room")?,
        };
        if !sec.takes(&item) || !sec.fits(&item, x, y, None) {
            return Err("doesn't fit there".into());
        }
        let shop = self.folk[trader].shop.as_mut().expect("shop");
        let item = shop.stock.take(0, idx).expect("item");
        shop.money += price;
        self.money -= price;
        let _ = self.folk[person].inventory.put(to, item, x, y);
        Ok(price)
    }

    /// Stealing ("Hold down the ALT key to steal items", F024551): `person` takes the trader's
    /// item `idx` without paying, a crime against the trader's faction while it is seen
    /// (`law`); trains thievery.
    pub fn steal(&mut self, trader: usize, idx: usize, person: usize, to: usize, at: Option<(u32, u32)>) -> Result<(), String> {
        let Some(shop) = self.folk[trader].shop.as_ref() else { return Err("not a trader".into()) };
        let Some(item) = shop.stock.sections[0].items.get(idx).map(|p| p.item.clone()) else { return Err("no such item".into()) };
        let inv = &self.folk[person].inventory;
        let Some(sec) = inv.sections.get(to) else { return Err("no such section".into()) };
        let (x, y) = match at {
            Some(c) => c,
            None => sec.first_fit(&item).ok_or("no room")?,
        };
        if !sec.takes(&item) || !sec.fits(&item, x, y, None) {
            return Err("doesn't fit there".into());
        }
        let shop = self.folk[trader].shop.as_mut().expect("shop");
        let item = shop.stock.take(0, idx).expect("item");
        let _ = self.folk[person].inventory.put(to, item, x, y);
        let faction = self.folk[trader].faction.clone();
        self.commit_crime(person, kenshi_ai::CrimeEnum::CrimeStealing, faction.as_deref());
        self.arena.f[person].skills.train(kenshi_char::StatsEnumerated::StatThieving, 10.0, 1.0);
        Ok(())
    }

    /// The player sells `person`'s item `idx` of section `from` to `trader`, into its stock at
    /// (x, y) (None: the first place it fits): what the trader pays, if it has the money.
    pub fn sell(&mut self, trader: usize, person: usize, from: usize, idx: usize, at: Option<(u32, u32)>) -> Result<i32, String> {
        let Some(item) = self.folk[person].inventory.sections.get(from).and_then(|s| s.items.get(idx)).map(|p| p.item.clone()) else {
            return Err("no such item".into());
        };
        let Some(shop) = self.folk[trader].shop.as_ref() else { return Err("not a trader".into()) };
        let price = self.price(&item, shop.town, false) * item.quantity as i32;
        if shop.money < price {
            return Err(format!("the trader can't afford {} (c.{price}, has c.{})", item.name, shop.money));
        }
        // a full shop grows a row band for it ((stand-in) the game's trader grid scrolls)
        let sec = &shop.stock.sections[0];
        let (x, y) = match at.filter(|&(x, y)| sec.fits(&item, x, y, None)) {
            Some(c) => c,
            None => match sec.first_fit(&item) {
                Some(c) => c,
                None => {
                    let h = sec.h;
                    let shop = self.folk[trader].shop.as_mut().expect("shop");
                    shop.stock.sections[0].h += item.h;
                    (0, h)
                }
            },
        };
        let item = self.folk[person].inventory.take(from, idx).expect("item");
        let shop = self.folk[trader].shop.as_mut().expect("shop");
        shop.money -= price;
        self.money += price;
        let _ = shop.stock.put(0, item, x, y);
        Ok(price)
    }

    /// Shops past their restock time stock up again.
    fn restock_shops(&mut self) {
        let now = self.hours_gone();
        let due: Vec<(usize, String)> = self
            .folk
            .iter()
            .filter_map(|p| p.shop.as_ref().filter(|s| s.restock_at <= now).map(|s| (p.id, s.template.clone())))
            .collect();
        for (id, t) in due {
            self.open_shop(id, &t);
        }
    }

    /// F040461: person `id`'s fighter takes the stats of the CHARACTER record it was made
    /// from, scaled by its race's multipliers (F033689, which also scale its XP). People made
    /// from no record keep the harness's flat level.
    pub fn apply_stats(&mut self, id: usize) {
        let Some(c) = self.folk[id].character.as_deref().and_then(|c| self.data.get(c)) else { return };
        let Some(race) = self.data.get(&self.race_record) else { return };
        let mult = kenshi_char::stats::race_multipliers(race);
        let rng = &mut self.rng;
        let levels = kenshi_char::stats::character_levels(&self.data, c, &mult, &mut || rng.next());
        let mut skills = self.arena.f[id].skills.clone();
        skills.levels = levels;
        skills.race_xp = mult;
        let race = race.clone();
        self.arena.f[id].set_skills(&self.data, &race, skills);
        self.brand_uniform(id);
        self.folk[id].personality = self.roll_personality(id);
        self.equip(id);
    }

    /// Person `id`'s fighter takes up what its inventory has equipped: the first weapon in a
    /// weapon slot (none: unarmed) and the armour worn.
    pub fn equip(&mut self, id: usize) {
        let inv = &self.folk[id].inventory;
        let weapon = inv.equipped().find(|i| i.kind == Some(kenshi_data::ItemType::Weapon)).cloned();
        // (the worn backpack too: its modifiers, kenshi_combat::armour::BagMods)
        let armour: Vec<kenshi_char::inventory::Item> = inv.equipped().filter(|i| matches!(i.kind, Some(kenshi_data::ItemType::Armour | kenshi_data::ItemType::Container))).cloned().collect();
        let Some(race) = self.data.get(&self.race_record).cloned() else { return };
        let data = self.data.clone();
        self.arena.f[id].equip(&data, &race, weapon.as_ref(), &armour);
        // an equipped crossbow (GunClassPersonal, F033879), its loaded shots kept while it stays
        // the same one; (assumption) the global damage factor is 1, as hit resolution takes it
        let bow = inv.equipped().find(|i| i.kind == Some(kenshi_data::ItemType::Crossbow)).cloned();
        let f = &mut self.arena.f[id];
        f.gun = bow.and_then(|b| {
            let r = data.get(&b.record)?;
            let mut g = kenshi_combat::ranged::Gun::personal(&data, r, b.quality, self.advanced.damage);
            if let Some(old) = f.gun.as_ref().filter(|o| o.record == g.record) {
                g.shots = old.shots;
            }
            Some(g)
        });
        self.sync_ammo(id);
    }

    /// The ammo charges a fighter carries for its crossbow, from its inventory.
    pub fn sync_ammo(&mut self, id: usize) {
        let f = &mut self.arena.f[id];
        f.ammo = match f.gun.as_ref().and_then(|g| g.ammo.as_ref()) {
            Some(a) => self.folk[id].inventory.ammo_charges(a),
            None => 0,
        };
    }

    /// Give characters an order (the task system's orders list, +0x70): what they did is
    /// dropped and they think again at once; a fighter leaves the fight.
    pub(crate) fn order_task(&mut self, ids: &[usize], task: u16, target: Target, pos: [f32; 3]) {
        self.order(ids, task, target, pos);
    }

    fn order(&mut self, ids: &[usize], task: u16, target: Target, pos: [f32; 3]) {
        for &id in ids {
            let Some(p) = self.folk.get_mut(id) else {
                continue;
            };            // the downed, and prisoners in their cage, take no orders (a slave let out does)
            if p.down || p.prison.as_ref().is_some_and(|pr| !pr.out) {
                continue;
            }
            p.ai.orders = vec![Task {
                task,
                classification: 3,
                target,
                target2: Target::None,
                priority: 1.0,
                pos,
                hours: (0, 100),
            }];
            p.ai.current = None;
            p.ai.queue.clear();
            p.started = false;
            stop(&mut p.mover, &mut p.pathing);
            p.rethink = true;
            p.build = None;
            p.build_fetch = None;
            p.operate = None;
            p.escort = None;
            p.haul = None;
            let f = &mut self.arena.f[id];
            if f.active && std::env::var("KENSHI_RANGED_DEBUG").is_ok() {
                eprintln!("{} ordered out of the fight (task {task})", f.name);
            }
            f.active = false;
            f.focus = None;
        }
    }

    /// "stop_movement": the characters drop what they were ordered and stand, leaving their
    /// own AI to choose again. (assumption) As an order with no task: everything `order` clears.
    pub fn order_stop(&mut self, ids: &[usize]) {
        for &id in ids {
            let Some(at) = self.folk.get(id).map(|p| p.mover.position) else { continue };
            self.order(&[id], MOVING, Target::None, at);
            self.folk[id].ai.orders.clear();
        }
    }

    /// Move an item from one character's inventory to another's (looting, or handing over):
    /// section `from` item `idx` of `giver` into section `to` of `taker` at cell `at` (None: the
    /// first place it fits).
    pub fn transfer(&mut self, giver: usize, from: usize, idx: usize, taker: usize, to: usize, at: Option<(u32, u32)>) -> Result<(), String> {
        // (law) the player's people taking from a town's or a shop's container: stealing
        if self.folk.get(taker).is_some_and(|p| p.player)
            && giver < crate::loot::BAG_OWNER
            && let Some(k) = giver.checked_sub(crate::loot::BOX_OWNER)
            && let Some(f) = self.box_faction(k)
        {
            self.commit_crime(taker, kenshi_ai::CrimeEnum::CrimeStealing, Some(&f));
        }
        // (law) the player's people taking from someone not theirs: looting; and they remember
        // it (LT_STOLE_FROM_ME)
        if self.folk.get(taker).is_some_and(|p| p.player) && self.folk.get(giver).is_some_and(|p| !p.player) {
            self.remember(giver, taker, crate::dialogue::LT_STOLE_FROM_ME);
            let faction = self.folk[giver].faction.clone();
            self.commit_crime(taker, kenshi_ai::CrimeEnum::CrimeLooting, faction.as_deref());
        }
        let item = self.inv(giver).and_then(|i| i.sections.get(from)).and_then(|s| s.items.get(idx)).map(|p| p.item.clone()).ok_or("no such item")?;
        let sec = self.inv(taker).and_then(|i| i.sections.get(to)).ok_or("no such section")?;
        let (x, y) = match at {
            Some(c) => c,
            None => sec.first_fit(&item).ok_or("no room")?,
        };
        if !sec.takes(&item) || !sec.fits(&item, x, y, None) {
            return Err("doesn't fit there".into());
        }
        let item = self.inv_mut(giver).and_then(|i| i.take(from, idx)).expect("item");
        if let Some(i) = self.inv_mut(taker) {
            let _ = i.put(to, item, x, y);
        }
        Ok(())
    }

    /// An inventory by owner: a person, or (from [`crate::base::BUILDING_OWNER`]) a building of
    /// the player's storage.
    pub fn inv(&self, owner: usize) -> Option<&kenshi_char::inventory::Inventory> {
        if let Some(p) = owner.checked_sub(crate::loot::BAG_OWNER) {
            return self.folk.get(p)?.inventory.bag();
        }
        if let Some(k) = owner.checked_sub(crate::loot::BOX_OWNER) {
            // a shop's counter holds its trader's stock
            if let Some(t) = self.counter_trader(k) {
                return self.folk[t].shop.as_ref().map(|s| &s.stock);
            }
            return self.boxes.get(k).map(|b| &b.inventory);
        }
        match owner.checked_sub(crate::base::BUILDING_OWNER) {
            Some(b) => self.buildings.get(b).and_then(|b| b.storage.as_ref()),
            None => self.folk.get(owner).map(|p| &p.inventory),
        }
    }

    pub fn inv_mut(&mut self, owner: usize) -> Option<&mut kenshi_char::inventory::Inventory> {
        if let Some(p) = owner.checked_sub(crate::loot::BAG_OWNER) {
            return self.folk.get_mut(p)?.inventory.bag_mut();
        }
        if let Some(k) = owner.checked_sub(crate::loot::BOX_OWNER) {
            if let Some(t) = self.counter_trader(k) {
                return self.folk[t].shop.as_mut().map(|s| &mut s.stock);
            }
            return self.boxes.get_mut(k).map(|b| &mut b.inventory);
        }
        match owner.checked_sub(crate::base::BUILDING_OWNER) {
            Some(b) => self.buildings.get_mut(b).and_then(|b| b.storage.as_mut()),
            None => self.folk.get_mut(owner).map(|p| &mut p.inventory),
        }
    }

    /// Order characters to a point (units): Moving (1).
    pub fn order_move(&mut self, ids: &[usize], to: [f32; 3]) {
        self.order(ids, MOVING, Target::None, to);
        // the mode's cap (CharMovement +0xbc): run uncapped, walk 15; the character's own move
        // speed limits it each tick
        let mode = if self.player_run {
            MoveSpeed::Run.cap(15.0).unwrap()
        } else {
            MoveSpeed::Walk.cap(15.0).unwrap()
        };
        for &id in ids {
            if let Some(p) = self.folk.get_mut(id) {
                let own = if p.own_speed > 0.0 { p.own_speed } else { self.run };
                p.mover.set_max_speed(speed_limit(own, mode, None));
                p.ordered_cap = Some(mode);
                p.pickup = None;
            }
        }
    }

    /// "toggle_sneak": the characters sneak, or stop (all stop if any sneaks... as Kenshi's
    /// button: on unless every one already sneaks).
    pub fn toggle_sneak(&mut self, ids: &[usize]) {
        let on = !ids.iter().all(|&i| self.folk.get(i).is_some_and(|p| p.sneaking));
        for &i in ids {
            if let Some(p) = self.folk.get_mut(i) {
                p.sneaking = on;
            }
        }
    }

    /// Pick someone up: the first character goes to them and lifts them over its left
    /// shoulder (a knocked-out person not already carried; the carrier's hands not full).
    pub fn order_pickup(&mut self, ids: &[usize], whom: usize) {
        let Some(&id) = ids.iter().find(|&&i| self.folk.get(i).is_some_and(|p| p.carrying.is_none() && !p.down)) else {
            return;
        };
        let ok = self.folk.get(whom).is_some_and(|w| w.down && w.carried_by.is_none()) && id != whom;
        if !ok {
            return;
        }
        let at = self.folk[whom].mover.position_units();
        self.order_move(&[id], at);
        self.folk[id].pickup = Some(whom);
    }

    /// Order the first of `ids` free to give `patient` first aid (Task_FirstAid): it walks
    /// there and treats them ([`Sim::first_aid_tick`]).
    pub fn order_first_aid(&mut self, ids: &[usize], patient: usize) {
        let Some(&id) = ids.iter().find(|&&i| self.folk.get(i).is_some_and(|p| !p.down && p.carrying.is_none())) else {
            return;
        };
        if self.folk.get(patient).is_none_or(|p| p.dead) {
            return;
        }
        if id != patient {
            let at = self.folk[patient].mover.position_units();
            self.order_move(&[id], at);
        }
        self.folk[id].first_aid = Some(patient);
    }

    /// Kidnapping (Task_Kidnapping, F031292): the victim is carried off as when picked up, a
    /// crime against their faction: (assumption, for the stealth roll F031292 makes) their
    /// people standing within 1000 units see it, turn on the kidnappers' side and go for the
    /// kidnapper.
    pub fn order_kidnap(&mut self, ids: &[usize], victim: usize) {
        self.order_pickup(ids, victim);
        let Some(&me) = ids.first() else { return };
        let side = self.arena.f[me].team;
        let (faction, at) = (self.folk[victim].faction.clone(), self.folk[victim].mover.position_units());
        if faction.is_none() {
            return;
        }
        if self.folk[me].player {
            self.commit_crime(me, kenshi_ai::CrimeEnum::CrimeKidnapping, faction.as_deref());
        }
        let mut witnesses = Vec::new();
        for k in 0..self.folk.len() {
            let w = &self.folk[k];
            if w.player || w.down || w.dead || k == victim || w.faction != faction {
                continue;
            }
            let q = w.mover.position_units();
            if (q[0] - at[0]).hypot(q[2] - at[2]) < 1000.0 {
                if let Some(foes) = &mut self.arena.f[k].foes
                    && !foes.contains(&side)
                {
                    foes.push(side);
                }
                witnesses.push(k);
            }
        }
        // the victim's squad goes for the kidnapper (F031292: its platoon told of them)
        if !witnesses.is_empty() {
            self.order_attack(&witnesses, me);
        }
    }

    /// @fn F031333
    ///
    /// Task_FirstAid each tick: a medic beside its patient (within 30 units, (assumption) the
    /// reach) treats them with the kit in its inventory with the fewest charges left (F031039;
    /// a robot repair kit for a robot) at its medic skill (x0.8 on itself) (F031038); the
    /// kit's charges run down, a used-up kit leaves the stack. Done when everything is treated.
    pub fn first_aid_tick(&mut self, dt: f32) {
        for id in 0..self.folk.len() {
            let Some(patient) = self.folk[id].first_aid else { continue };
            if self.folk[id].down || self.folk[patient].dead {
                self.folk[id].first_aid = None;
                continue;
            }
            let (a, b) = (self.folk[id].mover.position_units(), self.folk[patient].mover.position_units());
            if id != patient && 30.0 < (a[0] - b[0]).hypot(a[2] - b[2]) {
                // on its way: (again) toward where the patient is now
                let ordered = self.folk[id].ai.orders.first().map(|o| o.pos);
                if ordered.is_none_or(|o| 30.0 < (o[0] - b[0]).hypot(o[2] - b[2])) {
                    self.order_move(&[id], b);
                }
                continue;
            }
            if !self.folk[id].ai.orders.is_empty() {
                self.folk[id].ai.orders.clear();
                self.folk[id].ai.queue.clear();
                let p = &mut self.folk[id];
                stop(&mut p.mover, &mut p.pathing);
            }
            // the patient remembers who saved it (LT_MY_LIFESAVER)
            self.remember(patient, id, crate::dialogue::LT_MY_LIFESAVER);
            // the kit: the medical kit with the fewest charges left (F031039)
            let robot = false;
            let kit_at = self.folk[id]
                .inventory
                .sections
                .iter()
                .enumerate()
                .flat_map(|(s, sec)| sec.items.iter().enumerate().map(move |(i, p)| (s, i, p)))
                .filter_map(|(s, i, p)| p.item.medkit.filter(|k| k.robot == robot && 0.0 < k.charges).map(|k| (s, i, k)))
                .min_by(|x, y| x.2.charges.total_cmp(&y.2.charges));
            let kit = kit_at.map(|(_, _, k)| kenshi_char::medical::Kit { quality: k.quality, robot: k.robot });
            let mut skill = self.arena.f[id].level(kenshi_char::StatsEnumerated::StatMedic);
            if id == patient {
                skill *= 0.8;
            }
            let player = self.folk[id].player;
            let c = self.first_aid_c;
            let r = self.arena.f[patient].medical.first_aid(skill, kit, player, robot, &c, dt);
            if let Some((s, i, _)) = kit_at
                && 0.0 < r.drained
            {
                let item = &mut self.folk[id].inventory.sections[s].items[i].item;
                if let Some(k) = item.medkit.as_mut() {
                    k.charges -= r.drained;
                    if k.charges <= 0.0 {
                        if item.quantity > 1 {
                            item.quantity -= 1;
                            k.charges = k.full;
                        } else {
                            self.folk[id].inventory.sections[s].items.remove(i);
                        }
                    }
                }
            }
            if r.done {
                self.folk[id].first_aid = None;
                // a medic of the world goes back to what its package has it do
                if !self.folk[id].player {
                    self.folk[id].ai.orders.clear();
                    self.folk[id].rethink = true;
                }
                if std::env::var("KENSHI_FIRST_AID_DEBUG").is_ok() {
                    eprintln!("first aid: {id} finished treating {patient}");
                }
            }
        }
    }

    /// @fn F050304
    ///
    /// The "Medic" goal (AI_TASK 58, in most squads' packages) for the world's people: every 2
    /// seconds one out of a fight and free looks for whom to patch up, and goes to treat them
    /// (Task_FirstAid, [`Sim::first_aid_tick`]). Each person in reach not an enemy scores its
    /// first aid need (F040427, [`kenshi_char::medical::Medical::aid_need`]) x how near
    /// (F050251: 1 - distance^2 / 3397390, so 1843 units' reach) x, outside the medic's squad,
    /// 0.8 for an ally else the relation / 100, at least 0.05 (F050239); x 0.05 when someone
    /// else treats them already. Its own need, unscaled, wins when greater.
    /// (assumption) A stranger needs a score of 0.1 to be treated, after the 0.05 for one
    /// already seen to (F050304's threshold constant is not read; its "already treated" test is
    /// its squad's job list, here anyone's); the goal's own ranking against the package's others (its score function
    /// 0x140998410) is not ported: it runs whenever the medic is free and not fighting.
    pub fn medic_tick(&mut self, dt: f32) {
        const REACH2: f32 = 3_397_390.0;
        const SEARCH_EVERY: f32 = 2.0;
        let slot = (self.t / SEARCH_EVERY) as i64;
        let before = ((self.t - dt) / SEARCH_EVERY) as i64;
        if slot == before {
            return;
        }
        let mut treated: Vec<Option<usize>> = self.folk.iter().map(|p| p.first_aid).collect();
        for id in 0..self.folk.len() {
            let p = &self.folk[id];
            if p.player || p.dead || p.down || p.first_aid.is_some() || p.carrying.is_some() || p.prison.is_some() || p.carried_by.is_some() || p.race.is_some() {
                continue;
            }
            if self.arena.f[id].active || !p.ai.jobs.iter().flatten().any(|g| g.task == 58) {
                continue;
            }
            let me = p.mover.position_units();
            let rel = p.faction.as_ref().and_then(|f| self.relations.get(f));
            let mut best: Option<(usize, f32)> = None;
            for (k, q) in self.folk.iter().enumerate() {
                if k == id || q.dead || q.carried_by.is_some() || q.race.is_some() {
                    continue;
                }
                let at = q.mover.position_units();
                let d2 = (at[0] - me[0]).powi(2) + (at[2] - me[2]).powi(2);
                if REACH2 <= d2 || rel.is_some_and(|r| r.is_enemy(q.faction.as_deref())) {
                    continue;
                }
                let need = self.arena.f[k].medical.aid_need(false);
                let mut score = need.flesh + need.robotic;
                if score <= 0.0 {
                    continue;
                }
                let same_squad = p.platoon.is_some() && p.platoon == q.platoon;
                if !same_squad {
                    let f = match (rel, q.faction.as_deref()) {
                        (Some(r), Some(o)) if r.is_ally(Some(o)) => 0.8,
                        (Some(r), Some(o)) => r.relation(o) / 100.0,
                        _ => 0.0,
                    };
                    score *= if f < 0.0 { 0.05 } else { f.max(0.05) };
                }
                score *= (1.0 - d2 / REACH2).clamp(0.0, 1.0);
                if treated.iter().enumerate().any(|(m, t)| m != id && *t == Some(k)) {
                    score *= 0.05;
                }
                if !same_squad && score < 0.1 {
                    continue;
                }
                if best.is_none_or(|(_, b)| b < score) {
                    best = Some((k, score));
                }
            }
            let own = self.arena.f[id].medical.aid_need(false);
            let own = own.flesh + own.robotic;
            let patient = match best {
                Some((_, b)) if b < own => Some(id),
                Some((k, _)) => Some(k),
                None if 0.0 < own => Some(id),
                None => None,
            };
            if let Some(patient) = patient {
                if std::env::var("KENSHI_FIRST_AID_DEBUG").is_ok() {
                    eprintln!("medic: {} ({id}) goes to treat {} ({patient})", self.folk[id].name, self.folk[patient].name);
                }
                self.order_first_aid(&[id], patient);
                treated[id] = Some(patient);
            }
        }
    }

    /// @fn F050277
    ///
    /// The "Looting the dead" goal (AI_TASK 177: bandits' "Patrol map ... LOOTING", slavers'
    /// guards, occupiers) for the world's people: every 2 seconds one out of a fight and free
    /// picks whom to loot within 100 units: the dead first, then the knocked out (the player's
    /// only with "Bandits loot the player"), none it has looted already, none a squadmate is
    /// looting; it goes there and loots them ([`Sim::looting_tick`]).
    /// F050277's tests: within 100 units (squared 10000), and (F038528) not shut in a building
    /// of another's whose doors are all locked; (assumption) one so near stands on the same
    /// navmesh island (F038529's connectivity test not ported).
    pub fn looter_tick(&mut self, dt: f32) {
        const REACH2: f32 = 10_000.0;
        const SEARCH_EVERY: f32 = 2.0;
        if ((self.t / SEARCH_EVERY) as i64) == (((self.t - dt) / SEARCH_EVERY) as i64) {
            return;
        }
        let mut claimed: Vec<Option<usize>> = self.folk.iter().map(|p| p.loot.map(|l| l.0)).collect();
        for id in 0..self.folk.len() {
            let p = &self.folk[id];
            if p.player || p.dead || p.down || p.loot.is_some() || p.first_aid.is_some() || p.carrying.is_some() || p.prison.is_some() || p.carried_by.is_some() {
                continue;
            }
            if self.arena.f[id].active || !p.ai.jobs.iter().flatten().any(|g| g.task == 177) {
                continue;
            }
            let me = p.mover.position_units();
            let pick = |dead: bool| {
                (0..self.folk.len()).find(|&k| {
                    let q = &self.folk[k];
                    if k == id || q.carried_by.is_some() || self.looted_by.contains(&(id, k)) || q.inventory.sections.iter().all(|s| s.items.is_empty()) {
                        return false;
                    }
                    if dead != q.dead || (!dead && !q.down) || (!dead && q.player && !self.advanced.bandits_loot) {
                        return false;
                    }
                    let at = q.mover.position_units();
                    if REACH2 < (at[0] - me[0]).powi(2) + (at[2] - me[2]).powi(2) {
                        return false;
                    }
                    // F038528: shut in a building I am not in, its doors all locked
                    if let Some(b) = q.inside
                        && p.inside != Some(b)
                    {
                        let doors: Vec<&DoorState> = self.door_states.iter().filter(|d| d.door.placement == b).collect();
                        if !doors.is_empty() && doors.iter().all(|d| d.locked && d.level >= 1 && !d.broken) {
                            return false;
                        }
                    }
                    // a squadmate on them already
                    !claimed.iter().enumerate().any(|(m, c)| *c == Some(k) && m != id && self.folk[m].platoon == self.folk[id].platoon)
                })
            };
            let Some(target) = pick(true).or_else(|| pick(false)) else { continue };
            if std::env::var("KENSHI_LOOT_DEBUG").is_ok() {
                eprintln!("looter: {} ({id}) goes to loot {} ({target})", self.folk[id].name, self.folk[target].name);
            }
            let at = self.folk[target].mover.position_units();
            self.order_move(&[id], at);
            self.folk[id].loot = Some((target, 0.0));
            claimed[id] = Some(target);
        }
    }

    /// @fn F031281
    ///
    /// Task_Loot_AI each tick: beside the body (within 30 units) the looter's clock runs; at 0.8
    /// seconds it takes a weapon (the "hip" and "back" slots, F043569); at 8 (an animal's body,
    /// +0x84: 20, its butchering, and no weapon first) the rest of the weapons, then everything
    /// else that fits (F031165 modes 2 and 4), the backpack's contents (F039211, mode 4); and
    /// when it strips the body (F031331: +0x81, a body not of the player's faction; +0x83 clear,
    /// the looter's hunger cap, F039188, 2.8 or more) the rest again, the armour worn (mode 3)
    /// and the backpack again; then it is done.
    /// (not ported) F031331's other tests of +0x81 (Character +0x5bc set: strips; the
    /// looter's AI +0x28 object's +0x90 above 0: does not) and of +0x83 (the looter's squad of
    /// type 8); the crime of looting when seen and the "is butchering" message for the player's
    /// animals; the end's F040863 (looter, body, 6).
    pub fn looting_tick(&mut self, dt: f32) {
        for id in 0..self.folk.len() {
            let Some((target, t)) = self.folk[id].loot else { continue };
            if self.folk[id].down || self.folk[id].dead || self.arena.f[id].active {
                self.folk[id].loot = None;
                continue;
            }
            let (a, b) = (self.folk[id].mover.position_units(), self.folk[target].mover.position_units());
            if 30.0 < (a[0] - b[0]).hypot(a[2] - b[2]) {
                let ordered = self.folk[id].ai.orders.first().map(|o| o.pos);
                if ordered.is_none_or(|o| 30.0 < (o[0] - b[0]).hypot(o[2] - b[2])) {
                    self.order_move(&[id], b);
                }
                continue;
            }
            if !self.folk[id].ai.orders.is_empty() {
                self.folk[id].ai.orders.clear();
                self.folk[id].ai.queue.clear();
                let p = &mut self.folk[id];
                stop(&mut p.mover, &mut p.pathing);
            }
            let now = t + dt;
            self.folk[id].loot = Some((target, now));
            let weapons = |inv: &kenshi_char::inventory::Inventory| ["hip", "back"].iter().filter_map(|n| inv.section(n)).collect::<Vec<_>>();
            let animal = self.folk[target].race.is_some();
            if !animal && t < 0.8 && 0.8 <= now {
                for s in weapons(&self.folk[target].inventory) {
                    if self.loot_one(target, s, id) {
                        break;
                    }
                }
            }
            if (if animal { 20.0 } else { 8.0 }) <= now {
                let inv = &self.folk[target].inventory;
                let mut order = weapons(inv);
                let rest: Vec<usize> = (0..inv.sections.len()).filter(|&s| !inv.sections[s].is_equipment()).collect();
                order.extend(rest.iter().copied());
                for s in order {
                    while self.loot_one(target, s, id) {}
                }
                self.loot_bag(target, id);
                let theirs = self.folk[target].faction.as_deref() != Some(PLAYER_FACTION);
                let cap = self.folk[id].platoon.and_then(|pl| self.squad_hunger.get(&pl)).copied().unwrap_or(crate::npc_hunger::FULL);
                if std::env::var("KENSHI_LOOT_DEBUG").is_ok() {
                    eprintln!("looter: {id} strips {target}: body not the player's {theirs}, hunger cap {cap}");
                }
                if theirs && 2.8 <= cap {
                    for &s in &rest {
                        while self.loot_one(target, s, id) {}
                    }
                    let inv = &self.folk[target].inventory;
                    let worn: Vec<usize> = (0..inv.sections.len()).filter(|&s| inv.sections[s].is_equipment() && !["hip", "back", "backpack_attach"].contains(&inv.sections[s].name)).collect();
                    for s in worn {
                        while self.loot_one(target, s, id) {}
                    }
                    self.loot_bag(target, id);
                }
                self.looted_by.insert((id, target));
                self.folk[id].loot = None;
                self.folk[id].rethink = true;
                if std::env::var("KENSHI_LOOT_DEBUG").is_ok() {
                    let names = |i: usize| self.folk[i].inventory.sections.iter().flat_map(|s| s.items.iter().map(|p| p.item.name.clone())).collect::<Vec<_>>();
                    eprintln!("looter: {id} looted {target}: left {:?}, looter has {:?}", names(target), names(id));
                }
            }
        }
    }

    /// The contents of `giver`'s backpack (F039211) into `taker`'s, as far as they fit.
    fn loot_bag(&mut self, giver: usize, taker: usize) {
        loop {
            let Some(bag) = self.folk[giver].inventory.bag_mut() else { return };
            let Some(s) = (0..bag.sections.len()).find(|&s| !bag.sections[s].items.is_empty()) else { return };
            let Some(item) = bag.take(s, 0) else { return };
            if !self.folk[taker].inventory.add(item.clone(), false) {
                if let Some(bag) = self.folk[giver].inventory.bag_mut()
                    && let Some((x, y)) = bag.sections[s].first_fit(&item)
                {
                    let _ = bag.put(s, item, x, y);
                }
                return;
            }
        }
    }

    /// The first item of `giver`'s section `s` into `taker`'s main grid; false when there is
    /// none or it does not fit.
    fn loot_one(&mut self, giver: usize, s: usize, taker: usize) -> bool {
        if self.folk[giver].inventory.sections[s].items.is_empty() {
            return false;
        }
        let Some(item) = self.folk[giver].inventory.take(s, 0) else { return false };
        if self.folk[taker].inventory.add(item.clone(), false) {
            return true;
        }
        let back = &mut self.folk[giver].inventory;
        if let Some((x, y)) = back.sections[s].first_fit(&item) {
            let _ = back.put(s, item, x, y);
        }
        false
    }

    /// (testing) Knock someone out where they stand.
    pub fn knock_out(&mut self, id: usize) {
        let t = self.t;
        let (Some(p), Some(f)) = (self.folk.get_mut(id), self.arena.f.get_mut(id)) else { return };
        p.down = true;
        p.ai.queue.clear();
        stop(&mut p.mover, &mut p.pathing);
        f.down = true;
        f.down_at = t;
        // a medical knockout, so the medical tick keeps them down a while
        f.medical.knocked_out = true;
        f.medical.knockout_time = f.medical.knockout_time.max(120.0);
    }

    /// Bring a knocked-out person round at once: they get up (held for the "standing up 3" clip,
    /// F037164) and carry on. (The knockout's own end is the medical step's, "comes round";
    /// this is for callers that end it themselves.)
    pub fn wake(&mut self, id: usize) {
        let len = self.arena.sh.lengths.get("standing up 3").copied().unwrap_or(0.0);
        let (Some(p), Some(f)) = (self.folk.get_mut(id), self.arena.f.get_mut(id)) else { return };
        if !p.down || p.carried_by.is_some() {
            return;
        }
        p.down = false;
        f.down = false;
        p.getting_up = len;
        p.rethink = true;
    }

    /// Put down whoever the characters carry, at their feet.
    pub fn order_drop(&mut self, ids: &[usize]) {
        for &id in ids {
            let Some(w) = self.folk.get_mut(id).and_then(|p| p.carrying.take()) else { continue };
            let p = &self.folk[id];
            let at = p.mover.position;
            let side = [p.facing[2] * 0.6, 0.0, -p.facing[0] * 0.6];
            let drop = [at[0] + side[0], at[1], at[2] + side[2]];
            let spot = self.nav.closest_point(drop, 3.0).map_or(at, |(_, q)| q);
            let c = &mut self.folk[w];
            c.carried_by = None;
            c.mover.position = spot;
            self.arena.f[w].mover.position = spot;
        }
    }

    /// Order a group to a point in formation: (assumption: the player's order formation is not
    /// identified) the platoon layout (F028591) of the player faction's "squad formation" (the AI
    /// tab's formation option), its
    /// first member leading, turned from -x to the way from the group's middle to the point as
    /// F030760 turns a platoon's; each member's spot goes onto the navmesh.
    pub fn order_move_squad(&mut self, ids: &[usize], to: [f32; 3]) {
        for (i, spot) in self.squad_spots(ids, to) {
            self.order_move(&[i], spot);
        }
    }

    /// The move order held on the cursor: each character's standing move order (and the step
    /// under way) takes its new spot in the formation without stopping; one with no move order
    /// is ordered afresh.
    pub fn steer_move_squad(&mut self, ids: &[usize], to: [f32; 3]) {
        for (i, spot) in self.squad_spots(ids, to) {
            let p = &mut self.folk[i];
            let ordered = p.ai.orders.first().is_some_and(|o| o.task == MOVING && o.target == Target::None);
            if !ordered || p.down {
                self.order_move(&[i], spot);
                continue;
            }
            p.ai.orders[0].pos = spot;
            if let Some(q) = p.ai.queue.first_mut()
                && q.task == MOVING
                && q.target == Target::None
            {
                q.pos = spot;
                if p.started {
                    p.pathing.go_to(i, &p.mover, spot, false, &mut self.manager);
                }
            } else {
                p.rethink = true;
            }
        }
    }

    /// Each character's spot when a group is ordered to `to` (units).
    fn squad_spots(&mut self, ids: &[usize], to: [f32; 3]) -> Vec<(usize, [f32; 3])> {
        if ids.len() < 2 {
            return ids.iter().map(|&i| (i, to)).collect();
        }
        // the player faction's "squad formation", which the AI tab's formation sets (F035130)
        let kind = FormationKind::from_int(self.ai_options.formation).unwrap_or(FormationKind::Loose);
        let members: Vec<Member> = ids
            .iter()
            .enumerate()
            .map(|(k, &i)| Member {
                role: if k == 0 { 2 } else { 0 },
                radius: self.folk[i].mover.radius * 10.0,
                animal: false,
            })
            .collect();
        let rng = &mut self.rng;
        let offsets = layout(kind, &members, members[0].radius, &mut || rng.next());
        let n = ids.len() as f32;
        let mid = ids.iter().fold([0.0f32; 3], |m, &i| {
            let q = self.folk[i].mover.position_units();
            [m[0] + q[0] / n, 0.0, m[2] + q[2] / n]
        });
        let dir = kenshi_ai::formation::normalise([to[0] - mid[0], 0.0, to[2] - mid[2]]);
        let rot = kenshi_ai::formation::Quat::rotation_to([-1.0, 0.0, 0.0], dir);
        let mut out = Vec::new();
        for (k, &i) in ids.iter().enumerate() {
            let o = rot.rotate(offsets[k]);
            let spot = [to[0] + o[0], to[1], to[2] + o[2]].map(|v| v * METRES);
            let spot = self
                .nav
                .closest_point(spot, 30.0)
                .map_or(spot, |(_, q)| q)
                .map(|v| v / METRES);
            out.push((i, spot));
        }
        out
    }

    /// "Follow" (order 0x1f) and "Bodyguard" (0x2d, `guard`): the characters go after `whom`
    /// and keep up with them; a bodyguard also fights whoever fights them ([`Sim::escort_tick`]).
    pub fn order_follow(&mut self, ids: &[usize], whom: usize, guard: bool) {
        let ids: Vec<usize> = ids.iter().copied().filter(|&i| i != whom).collect();
        if self.folk.get(whom).is_none_or(|w| w.dead) || ids.is_empty() {
            return;
        }
        // Task_Follow (its update and ends Task_FollowLeader's, F030761 / F031009 / F030762;
        // its target the one followed, F031269): the order to follow them
        let at = self.folk[whom].mover.position_units();
        self.order(&ids, FOLLOWING_LEADER, Target::Person(whom), at);
        for &id in &ids {
            // Task_Follow's distance (F030758: a random 20 .. 40, the factory's randRange)
            let d = 20.0 + self.rng.uniform() * 20.0;
            if let Some(p) = self.folk.get_mut(id)
                && !p.down
                && p.prison.is_none()
            {
                p.escort = Some((whom, guard));
                p.follow.distance = d;
            }
        }
    }

    /// The Follow and Bodyguard orders, twice a second: the follow task does the following; a
    /// bodyguard goes for anyone fighting the one it guards and, its fight over, takes up the
    /// follow again; (stand-in) one whose follow task is gone walks after them once 40 units
    /// behind; the order ends with them dead.
    pub fn escort_tick(&mut self) {
        if !self.tick.is_multiple_of(30) {
            return;
        }
        for id in 0..self.folk.len() {
            let Some((whom, guard)) = self.folk[id].escort else { continue };
            let gone = self.folk.get(whom).is_none_or(|w| w.dead);
            if gone {
                self.folk[id].escort = None;
                continue;
            }
            if self.folk[id].down || self.arena.f[id].active {
                continue;
            }
            // a bodyguard: someone fighting the one guarded
            if guard {
                let foe = (0..self.folk.len()).find(|&o| {
                    o != id && o != whom && !self.folk[o].down && !self.folk[o].dead && {
                        let f = &self.arena.f[o];
                        let ai = &self.folk[o].ai;
                        // fighting them, or set on them (an attack task on them)
                        let set_on = ai.current.iter().chain(ai.orders.iter()).any(|t| {
                            matches!(t.task, ATTACKING | ATTACKING_ENEMIES | ATTACKING_NEUTRALS) && t.target == Target::Person(whom)
                        });
                        (f.active && (f.focus == Some(whom) || f.combat.target == Some(whom))) || set_on
                    }
                });
                if let Some(foe) = foe {
                    if std::env::var("KENSHI_ESCORT_DEBUG").is_ok() {
                        eprintln!("escort: {id} guarding {whom} goes for {foe}");
                    }
                    self.order_attack(&[id], foe);
                    self.folk[id].escort = Some((whom, guard));
                    continue;
                }
            }
            let (a, b) = (self.folk[id].mover.position_units(), self.folk[whom].mover.position_units());
            let d = ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
            if std::env::var("KENSHI_ESCORT_DEBUG").is_ok() {
                eprintln!("escort: {id} after {whom} (guard {guard}) {d:.0} away");
            }
            // the follow task again once nothing else is ordered (after a bodyguard's fight)
            let following = self.folk[id].ai.orders.iter().any(|o| o.task == FOLLOWING_LEADER && o.target == Target::Person(whom));
            if !following && self.folk[id].ai.orders.is_empty() {
                self.order(&[id], FOLLOWING_LEADER, Target::Person(whom), b);
                self.folk[id].escort = Some((whom, guard));
            } else if following && d > ESCORT_GAP * 8.0 && self.folk[id].ai.queue.is_empty() {
                // (stand-in) the task not taken up (its plan failed): walk after them
                let k = (ESCORT_GAP * 0.5) / d.max(1e-3);
                let to = [b[0] + (a[0] - b[0]) * k, b[1], b[2] + (a[2] - b[2]) * k];
                self.order_move(&[id], to);
                self.folk[id].escort = Some((whom, guard));
            }
        }
    }

    /// Order characters to attack someone: Attacking (4), Running there first.
    pub fn order_attack(&mut self, ids: &[usize], whom: usize) {
        for &id in ids {
            if let Some(p) = self.folk.get_mut(id) {
                p.ordered_cap = None;
            }
        }
        // (crate::law, F030782) the player's attack orders go unprovoked: an assault on one not
        // wanted, fighting or committing a crime
        for &id in ids {
            if self.folk[id].player && !self.folk[whom].player {
                self.unprovoked_attack(id, whom);
            }
        }
        let at = self.folk[whom].mover.position_units();
        // (assumption) attacking someone makes their side an enemy of the attackers
        let side = self.arena.f[whom].team;
        for &id in ids {
            // (None already fights every side)
            if let Some(foes) = &mut self.arena.f[id].foes
                && !foes.contains(&side)
            {
                foes.push(side);
            }
        }
        self.order(ids, ATTACKING, Target::Person(whom), at);
    }

    /// The game hour the clock started at.
    pub fn start_hour(&self) -> f64 {
        self.start_hour
    }

    /// Real seconds in a game hour.
    pub fn hour_secs(&self) -> f64 {
        self.hour_secs
    }

    /// The game hours gone since the start.
    pub fn hours_gone(&self) -> f64 {
        self.t as f64 / self.hour_secs
    }

    /// The game hour (0..24).
    pub fn hour(&self) -> f64 {
        (self.start_hour + self.t as f64 / self.hour_secs).rem_euclid(24.0)
    }

    /// One tick (1/60 s): thinking, the plans' steps, path answers, fights and movement.
    pub fn step(&mut self) {
        // the turrets and who mans them (crate::turrets)
        self.sync_turrets();
        let hunger_c = self.hunger_c;
        let hunger_caps = self.hunger_caps();
        // what target scoring reads of each (kenshi_combat::arena::Targeting)
        for id in 0..self.folk.len() {
            let crippled = self.limb_lost(id, 2) || self.limb_lost(id, 3) || (self.limb_lost(id, 0) && self.limb_lost(id, 1));
            let p = &self.folk[id];
            // (its AI's task target: the attack it was set on)
            let task_target = p.ai.current.iter().chain(p.ai.orders.iter()).find_map(|t| match t.target {
                Target::Person(v) if matches!(t.task, ATTACKING | ATTACKING_ENEMIES | ATTACKING_NEUTRALS) => Some(v),
                _ => None,
            });
            self.arena.f[id].targeting = kenshi_combat::arena::Targeting { stance: p.stance, player: p.player, knocked: false, getting_up: p.getting_up > 0.0, task_target, crippled, hold_at: if p.stance.hold { p.hold_at } else { None } };
            let ignored: Vec<usize> = p.ignored.iter().map(|i| i.0).collect();
            if self.arena.f[id].ignored != ignored {
                self.arena.f[id].ignored = ignored;
            }
        }
        let (dt, run, walk, start_hour, hour_secs, vision_mult, sunrise, sunset, t, tick) = (
            self.dt,
            self.run,
            self.walk,
            self.start_hour,
            self.hour_secs,
            self.vision_mult,
            self.sunrise,
            self.sunset,
            self.t,
            self.tick,
        );
        // each one's race's swimming (crate::sim::water_state)
        let swimmers: Vec<(bool, f32)> = (0..self.folk.len())
            .map(|i| {
                let race = self.folk[i].race.clone().or_else(|| self.folk[i].look_race.clone());
                let Some(r) = race else { return (true, 1.0) };
                if let Some(&v) = self.race_water.get(&r) {
                    return v;
                }
                let rec = self.data.get(&r);
                let v = (
                    rec.and_then(|x| x.bools.get("swims").copied()).unwrap_or(true),
                    rec.and_then(|x| x.floats.get("swim speed mult").copied()).unwrap_or(1.0),
                );
                self.race_water.insert(r, v);
                v
            })
            .collect();
        // the containers about: the towns' and the player's storage buildings, each with the
        // navmesh point by it and whether it holds food (Getting food and Looting food look for
        // them)
        let has_food = |inv: &kenshi_char::inventory::Inventory| inv.sections.iter().any(|s| s.items.iter().any(|i| i.item.food.is_some()));
        let by = |at: [f32; 3]| {
            self.nav
                .closest_point(at.map(|v| v * METRES), 300.0 * METRES)
                .map_or(at, |(_, q)| q.map(|v| v / METRES))
        };
        let stores: Vec<(usize, [f32; 3], bool)> = self
            .boxes
            .iter()
            .enumerate()
            .map(|(k, b)| (crate::loot::BOX_OWNER + k, by(b.at.position), has_food(&b.inventory)))
            .chain(self.buildings.iter().enumerate().filter_map(|(k, b)| {
                let s = b.storage.as_ref().filter(|_| b.done)?;
                Some((crate::base::BUILDING_OWNER + k, by(b.at), has_food(s)))
            }))
            .collect();
        // F040015: the player's people get their faction's "AI Goals" as jobs (F036828 ->
        // F036811, by classification): Nameless's Protect allies (urgent, so planned without
        // free will too)
        if self.placed_town.len() != self.placed_at.len() {
            let towns = &self.towns;
            self.placed_town = self.placed_at.iter().map(|&q| current_town(towns, q, None)).collect();
        }
        let placed_town = self.placed_town.clone();
        // the shops Shopping looks for (F050263)
        let counters = self.counters();
        let public = self.public_buildings();
        let (day, hour_now) = self.day_hour();
        // F040015: everyone else's built-in goals: Self preservation (urgent, priority 1) and
        // its race's "AI Goals" (F036828: "Heal my legs when crippled", animals' feeding, ...),
        // by race. (not ported) Idleness's own entry (the task system idles anyway) and the
        // faction's "AI Goals" for a character without its own (Character +0x210 unidentified).
        let mut race_goals: HashMap<String, [Vec<Task<Target>>; 4]> = HashMap::new();
        // (each one's race as it is drawn, `race_of`)
        let person_race: Vec<Option<String>> = (0..self.folk.len()).map(|i| if self.folk[i].player { None } else { self.race_of(i) }).collect();
        for r in person_race.iter().flatten() {
            let r = r.clone();
            if race_goals.contains_key(&r) {
                continue;
            }
            let goals = self.data.get(&r).map(|rec| jobs_of(kenshi_ai::package::record_goals(&self.data, rec), None, None)).unwrap_or_default();
            race_goals.insert(r, goals);
        }
        let self_preservation = Task {
            task: SELF_PRESERVATION,
            classification: 3,
            target: Target::None,
            target2: Target::None,
            priority: 1.0,
            pos: [0.0; 3],
            hours: (0, 100),
        };
        let player_goals: [Vec<Task<Target>>; 4] = self
            .data
            .get(PLAYER_FACTION)
            .map(|f| jobs_of(kenshi_ai::package::record_goals(&self.data, f), None, None))
            .unwrap_or_default();
        let Sim {
            data: _,
            general,
            approach,
            arena,
            bar_states,
            bars,
            bed_taken,
            door_states,
            folk,
            furn,
            furniture_approach,
            inner,
            inside_point,
            manager,
            nav,
            operators,
            owners,
            placed: _,
            placed_at,
            relations,
            world,
            rng,
            terrain,
            towns,
            population,
            ai_options,
            advanced,
            law_state,
            ..
        } = self;
        let mut prof = kenshi_render::prof::Marks::start();
        let now = t as f64 / hour_secs;
        let positions: Vec<[f32; 3]> = folk.iter().map(|p| p.mover.position_units()).collect();
        let downs: Vec<bool> = folk.iter().map(|p| p.down).collect();
        // what the sensor reads of each (F046498's target, F046538's noise)
        let noises: Vec<crate::law::Noise> = folk
            .iter()
            .map(|p| crate::law::Noise {
                sneaking: p.sneaking,
                fighting: arena.f[p.id].active,
                speed: p.speed,
                stealth: (arena.f[p.id].level(kenshi_char::StatsEnumerated::StatStealth) * 0.01).clamp(0.0, 1.0),
                inside: p.inside,
            })
            .collect();
        // (crate::disguise, F046494) one disguised passes as its disguise's faction, a slave as
        // its owners'
        let factions: Vec<Option<String>> = folk
            .iter()
            .map(|p| p.disguised.clone().or_else(|| p.slave.as_ref().filter(|s| s.state == crate::slavery::IS_SLAVE).map(|s| s.faction.clone())).or_else(|| p.faction.clone()))
            .collect();
        let internal: Vec<[f32; 3]> = folk.iter().map(|p| p.mover.position).collect();
        // (stand-in for the squad's target squad, F038606's first exception, not ported) each
        // squad's fights: those its people fight and those fighting them; a squad mate knows of
        // them too
        let mut squad_fights: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, q) in folk.iter().enumerate() {
            let f = &arena.f[i];
            if !f.active || q.down {
                continue;
            }
            if let Some(foe) = f.focus.or(f.combat.target)
                && !downs[foe]
            {
                if let Some(pl) = q.platoon {
                    squad_fights.entry(pl).or_default().push(foe);
                }
                if let Some(pl) = folk[foe].platoon {
                    squad_fights.entry(pl).or_default().push(i);
                }
            }
        }
        // who attacks whom: each one's attackers (those whose swing or target is on it, the
        // combat AI's +0x200 count / +0x208 list)
        let mut attackers_of: Vec<Vec<usize>> = vec![Vec::new(); folk.len()];
        for i in 0..folk.len() {
            let f = &arena.f[i];
            if f.active
                && !folk[i].down
                && let Some(v) = f.focus.or(f.combat.target)
                && v < attackers_of.len()
            {
                attackers_of[v].push(i);
            }
        }
        // @fn F050268
        //
        // How much each one needs help (the Protect allies finder's score of an ally): none
        // dead; 0.01 when its one attacker is itself fought by others; else 0.25 for each
        // attacker less 0.2 (one 0.05, two 0.3, ...); 0.01 at least while it fights.
        // (assumption) The other term, 0.1 x a value of its combat state (+0x178 +0xb0), is
        // not identified: taken as 0.
        let need: Vec<f32> = (0..folk.len())
            .map(|x| {
                if folk[x].dead {
                    return 0.0;
                }
                let on = &attackers_of[x];
                if on.len() == 1 && !attackers_of[on[0]].is_empty() {
                    return 0.01;
                }
                let n = on.len() as f32;
                let v = if on.is_empty() { 0.0 } else { n * 0.25 - 0.2 };
                if v <= 0.0 && arena.f[x].active { 0.01 } else { v }
            })
            .collect();
        let platoons: Vec<Option<usize>> = folk.iter().map(|p| p.platoon).collect();
        // the zone's turrets and the town each stands in, and whose plan already has each (the
        // finder F047566 passes over one another's task system holds, F023bf5)
        let turrets: Vec<(usize, Option<usize>)> = furn
            .iter()
            .enumerate()
            .filter(|(_, f)| f.function == crate::turrets::BF_TURRET)
            .map(|(k, f)| (k, current_town(towns, f.position, None)))
            .collect();
        let mut turret_taken: Vec<Option<usize>> = vec![None; furn.len()];
        for p in folk.iter() {
            for st in &p.ai.queue {
                if st.task == TURRET_GUNNER
                    && let Target::Furniture(f) = st.target
                    && f < turret_taken.len()
                {
                    turret_taken[f] = Some(p.id);
                }
            }
        }
        // who is in a fight and whom each one goes for in it
        let in_fight: Vec<bool> = (0..folk.len()).map(|i| arena.f[i].active).collect();
        let fight_target: Vec<Option<usize>> = (0..folk.len()).map(|i| arena.f[i].focus.or(arena.f[i].combat.target).filter(|_| arena.f[i].active)).collect();
        let no_protect = std::env::var("KENSHI_NO_PROTECT").is_ok();
        // who is someone's target in a fight
        let mut targeted = vec![false; folk.len()];
        for i in 0..folk.len() {
            if arena.f[i].active
                && let Some(v) = arena.f[i].focus.or(arena.f[i].combat.target)
                && v < targeted.len()
            {
                targeted[v] = true;
            }
        }
        prof.mark("sim: setup");
        // think once a second, staggered by person
        for p in folk.iter_mut() {
            if tick % 60 != (p.id as u64 * 7) % 60 && !p.rethink {
                continue;
            }
            p.rethink = false;
            if p.down || p.getting_up > 0.0 || p.away {
                // still somewhere: the knocked out carried out of a building are out of it
                if p.down && !p.away {
                    p.inside = building_at(inner, p.mover.position_units());
                }
                continue;
            }
            let before = (p.ai.current.clone(), p.ai.queue.clone());
            // the sensor (F046551): everyone within the vision range is tested (F046539)
            let me = p.mover.position_units();
            let hour = ((start_hour + now).rem_euclid(24.0)) as f32;
            let dl = daylight(hour, sunrise, sunset);
            let observer = Observer {
                position: me,
                facing: if p.facing == [0.0; 3] {
                    [0.0, 0.0, 1.0]
                } else {
                    p.facing
                },
                vision_mult,
                in_combat: arena.f[p.id].active,
                indoors: p.inside.is_some(),
                flag_1e0: false,
                out: false,
                light: dl,
                has_78: false,
            };
            let range = vision_range(vision_mult, false, dl);
            let mine = p.faction.as_ref().map(|f| &relations[f]);
            let mut known: Vec<Known<usize>> = vec![];
            let mut enemies = 0;
            for (q, &at) in positions.iter().enumerate() {
                if q == p.id || (at[0] - me[0]).hypot(at[2] - me[2]) > range {
                    if q != p.id && std::env::var("TOWN_FIGHT_DEBUG").is_ok_and(|w| w == p.id.to_string()) && factions[q] != p.faction {
                        eprintln!("  {} out of range of {q}: {:.0} > {range:.0}, me {me:?} at {at:?}", p.id, (at[0] - me[0]).hypot(at[2] - me[2]));
                    }
                    continue;
                }
                // a sneaking target (F039177: not while fighting) with its stealth; one not seen
                // may yet be heard (F046538)
                let n = &noises[q];
                let target = Seen {
                    position: at,
                    stance: if downs[q] {
                        Stance::Prone
                    } else {
                        Stance::Standing
                    },
                    sneaking: n.sneaking && !n.fighting,
                    moving: n.speed > 1.0,
                    stealth: n.stealth,
                    light: dl,
                };
                let d2 = (at[0] - me[0]).powi(2) + (at[1] - me[1]).powi(2) + (at[2] - me[2]).powi(2);
                if !sees(&observer, &target, dl) && !(p.bed.is_none() && crate::law::hear_roll(p.id, q, t, walk, p.inside, d2, n)) {
                    if std::env::var("TOWN_FIGHT_DEBUG").is_ok_and(|w| w == p.id.to_string())
                        && factions[q] != p.faction
                    {
                        eprintln!(
                            "  {} misses {q} at {:.0}: vis {:.3} range {range:.0} dl {dl:.2} face {:?} me {me:?} at {at:?}",
                            p.id,
                            (at[0] - me[0]).hypot(at[2] - me[2]),
                            kenshi_ai::perception::visibility(&observer, &target, dl),
                            observer.facing
                        );
                    }
                    continue;
                }
                // an enemy: of an enemy faction, or one fighting my side (the temporary-enemy
                // tag of the attacked, F040863)
                let hostile = mine.is_some_and(|r| r.is_enemy(factions[q].as_deref()))
                    || (p.attacks_neutrals && factions[q] != p.faction && !mine.is_some_and(|r| r.is_ally(factions[q].as_deref())))
                    || arena.f[p.id]
                        .foes
                        .as_ref()
                        .is_some_and(|v| v.contains(&arena.f[q].team));
                if hostile && !downs[q] {
                    enemies += 1;
                }
                if std::env::var("TOWN_FIGHT_DEBUG").is_ok_and(|w| w == p.id.to_string()) && factions[q] != p.faction {
                    eprintln!("  {} knows {q} ({:?}) hostile {hostile} down {}", p.id, factions[q], downs[q]);
                }
                known.push(Known {
                    id: q,
                    position: at,
                    seen: true,
                    carried: false,
                    lying: downs[q],
                    ally: !hostile,
                    squad_target: false,
                    swinging_at_me: false,
                    leash: LeashTarget {
                        position: at,
                        helpless: downs[q],
                        fighting_toward_me: false,
                    },
                });
            }
            // the hold stance gives one of the player's people the goal Attacking enemies
            // (F039205 order 0xc: added at classification 3, priority 1, hours 0-100; taken away
            // with the stance), so it goes for enemies within its hold leash; its score is off
            // while passive (F038552). An Attacking enemies job (a shift attack order) is the
            // same goal. Without either the player's people fight when attacked, and for their
            // allies by their faction's Protect allies job (below).
            let has_goal = p.ai.goals.iter().any(|g| g.task == ATTACKING_ENEMIES);
            let wants = p.stance.hold || p.jobs.iter().any(|j| j.task == ATTACKING_ENEMIES);
            if p.player && p.faction.as_deref() == Some(PLAYER_FACTION) {
                for (class, goals) in player_goals.iter().enumerate() {
                    for g in goals {
                        if !p.ai.jobs[class].iter().any(|j| j.task == g.task) {
                            p.ai.jobs[class].push(g.clone());
                        }
                    }
                }
            }
            if !p.player {
                if !p.ai.jobs[3].iter().any(|j| j.task == SELF_PRESERVATION) {
                    p.ai.jobs[3].push(self_preservation.clone());
                }
                if let Some(goals) = person_race.get(p.id).cloned().flatten().and_then(|r| race_goals.get(&r)) {
                    for (class, goals) in goals.iter().enumerate() {
                        for g in goals {
                            if !p.ai.jobs[class].iter().any(|j| j.task == g.task) {
                                p.ai.jobs[class].push(g.clone());
                            }
                        }
                    }
                }
            }
            if p.player && !wants && has_goal {
                p.ai.goals.retain(|g| g.task != ATTACKING_ENEMIES);
            }
            if p.player && wants && !has_goal {
                p.ai.goals.push(Task {
                    task: ATTACKING_ENEMIES,
                    classification: 3,
                    target: Target::None,
                    target2: Target::None,
                    priority: 1.0,
                    pos: [0.0; 3],
                    hours: (0, 100),
                });
            }
            // a squad mate's fight (not for the player's people: their stances decide)
            if !p.player
                && let Some(foes) = p.platoon.and_then(|pl| squad_fights.get(&pl))
            {
                for &e in foes {
                    if e == p.id || downs[e] || known.iter().any(|k| k.id == e && !k.ally) {
                        continue;
                    }
                    let at = positions[e];
                    if (at[0] - me[0]).hypot(at[2] - me[2]) > range * 2.0 {
                        continue;
                    }
                    known.retain(|k| k.id != e);
                    enemies += 1;
                    known.push(Known {
                        id: e,
                        position: at,
                        seen: true,
                        carried: false,
                        lying: false,
                        ally: false,
                        squad_target: true,
                        swinging_at_me: false,
                        leash: LeashTarget { position: at, helpless: false, fighting_toward_me: false },
                    });
                }
            }
            // @fn F050285
            //
            // Protect allies' finder (task 15): of the friendly known (seen; of my faction or an
            // allied one), the one most in need of help (F050268). For one of the player's
            // people its score falls off with the distance (F050517 over the squared distance:
            // 1 within 31.6 units, down to 0.1), and one of another squad counts only with the
            // AI option Help Allies, its score squared. (assumption) A slave or escaping slave
            // (F039148's slave state 1, 2) counts only with F050234's leave, taken as granted;
            // (not ported) the second pass over the
            // squad's protect list (x 0.1). The score is at most 1.2.
            //
            // @fn F050287
            //
            // Protect close allies' (143): the same, of my own squad's people only.
            //
            // @fn F050269
            //
            // isAlliesFighting (condition 12): a friendly known in need.
            let friendly = |x: usize| factions[x] == p.faction || mine.is_some_and(|r| r.is_ally(factions[x].as_deref()));
            let mut protect_any: Option<(usize, f32)> = None;
            let mut protect_close: Option<(usize, f32)> = None;
            let mut allies_fighting = false;
            for k in &known {
                let x = k.id;
                if !k.seen || x == p.id || need[x] <= 0.0 || !friendly(x) {
                    continue;
                }
                allies_fighting = true;
                let mut s = need[x];
                if p.player {
                    let at = positions[x];
                    let d2 = (at[0] - me[0]).powi(2) + (at[1] - me[1]).powi(2) + (at[2] - me[2]).powi(2);
                    s *= ((d2 - 1000.0) / (10.0 - 1000.0)).min(1.0).max(0.1);
                    if platoons[x] != p.platoon {
                        if !ai_options.help_allies {
                            continue;
                        }
                        s *= s;
                    }
                }
                if protect_any.is_none_or(|(_, b)| s > b) {
                    protect_any = Some((x, s.min(1.2)));
                }
                if p.platoon.is_some() && platoons[x] == p.platoon && protect_close.is_none_or(|(_, b)| need[x] > b) {
                    protect_close = Some((x, need[x].min(1.2)));
                }
            }
            // F038601 scores it 0 for an obedient slave (job 187) and while passive (CharStats
            // +0x12c); halved while it carries someone (Character +0x348)
            let obedient = p.slave.as_ref().is_some_and(|s| s.obedient);
            // ((testing) KENSHI_NO_PROTECT turns Protect allies off)
            if no_protect {
                protect_any = None;
                protect_close = None;
            }
            p.town = current_town(towns, p.mover.position_units(), p.town);
            p.inside = building_at(inner, p.mover.position_units());
            // the shopping list: its daily reset (F050260, from Shopping's score) and condition
            // 85 (F050255)
            let (shop_done, shop_wants) = match p.shopping.as_mut() {
                Some(l) => {
                    l.daily(day, hour_now);
                    (l.done(p.town, day), l.wants())
                }
                None => (true, vec![]),
            };
            let mut v = View {
                me: p.id,
                home: p.home,
                pos: p.mover.position_units(),
                nodes: general,
                approach,
                owners,
                rng,
                now,
                town: p.town,
                destination: p.destination,
                positions: &positions,
                furniture: furn,
                furniture_approach,
                operators,
                bars,
                placed_at,
                placed_town: &placed_town,
                bar_state: p.platoon.and_then(|pl| bar_states[pl].as_mut()),
                inside: p.inside,
                shop_done,
                shop_wants,
                counters: &counters,
                public: &public,
                home_area: p.home.and_then(|h| placed_town.get(h).copied().flatten()).map(|t| (towns[t].position, towns[t].radius())),
                leader: p.role == 5,
                legs_hurt: arena.f[p.id].medical.parts.iter().any(|q| q.kind == 1 && q.health() < q.max_health()),
                has_medkit: p.inventory.sections.iter().any(|s| s.items.iter().any(|i| i.item.medkit.is_some())),
                in_bed: p.bed,
                woke_at: p.woke_at,
                start_hour,
                doors: door_states,
                bed_taken,
                known: &known,
                enemies,
                free_will: !p.player,
                passive: p.stance.passive,
                attacked: arena.f[p.id].active || targeted[p.id],
                hold: if p.stance.hold { p.hold_at } else { None },
                turrets: &turrets,
                turret_taken: &turret_taken,
                stores: &stores,
                has_food: p.inventory.sections.iter().chain(p.inventory.bag().into_iter().flat_map(|b| b.sections.iter())).any(|s| s.items.iter().any(|i| i.item.food.is_some())),
                hunger: (arena.f[p.id].medical.hunger.level, arena.f[p.id].medical.hunger.queued),
                ignored: p.ignored.iter().map(|i| i.0).collect(),
                downs: &downs,
                protect: [protect_any, protect_close],
                allies_fighting,
                obedient,
                sneaking: p.sneaking && !arena.f[p.id].active,
                carrying: p.carrying.is_some(),
                attackers_of: &attackers_of,
                in_fight: &in_fight,
                fight_target: &fight_target,
                factions: &factions,
                my_faction: p.faction.clone(),
                mine,
            };
            if std::env::var("TOWN_FIGHT_DEBUG").is_ok_and(|w| w == p.id.to_string()) {
                let pos = v.pos;
                let mut pl =
                    kenshi_ai::planner::Planner::new(&mut v, kenshi_ai::planner::Repertoire::Human);
                let r = pl.plan(std::env::var("TOWN_FIGHT_PLAN").ok().and_then(|v| v.parse().ok()).unwrap_or(ATTACKING_ENEMIES), Target::None, pos);
                if let Some(plan) = &r {
                    let (steps, score) = pl.steps(plan);
                    eprintln!(
                        "  plan 16: {:?} steps {:?} score {score}",
                        plan.iter()
                            .map(|&n| (pl.nodes[n].task, pl.nodes[n].target, pl.nodes[n].score))
                            .collect::<Vec<_>>(),
                        steps.iter().map(|n| n.task).collect::<Vec<_>>()
                    );
                }
                eprintln!(
                    "  jobs {:?}",
                    p.ai.jobs
                        .iter()
                        .map(|l| l.iter().map(|j| j.task).collect::<Vec<_>>())
                        .collect::<Vec<_>>()
                );
            }
            p.ai.think(&mut v);
            if std::env::var("TOWN_FIGHT_DEBUG").is_ok_and(|w| w == p.id.to_string()) {
                eprintln!(
                    "t {t:.1} person {}: knows {}, enemies {enemies}, goal {:?}, queue {:?}",
                    p.id,
                    known.len(),
                    p.ai.current.as_ref().map(|c| c.task),
                    p.ai.queue
                        .iter()
                        .map(|s| (s.task, s.target))
                        .collect::<Vec<_>>()
                );
            }
            if (p.ai.current.clone(), p.ai.queue.clone()) != before {
                // the hold point (+0x130 of the AI settings) moves to where the character stands
                // as a move or follow ends (Task_FollowLeader::end, F030762) or a looting order
                // (Task_Loot_Order's end, at 0x140346748): Kenshi never walks a holder back, it
                // holds where its last order left it
                if p.stance.hold
                    && before.0.as_ref().is_some_and(|t| matches!(t.task, MOVING | FOLLOWING_LEADER | 26))
                    && p.ai.current.as_ref().is_none_or(|t| t.task != before.0.as_ref().map_or(0, |b| b.task))
                {
                    p.hold_at = Some(p.mover.position_units());
                }
                // a new plan: let go of the old node (Task_StandAtNode end, F031321) unless the
                // plan goes back to it; (assumption) the node the plan stands at is reserved at
                // once, so the next decision finds it again
                let next = p.ai.queue.iter().find_map(|s| match (s.task, s.target) {
                    (STAND_AT_NODE, Target::Node(n)) => Some(n),
                    _ => None,
                });
                if p.claimed != next {
                    if let Some(n) = p.claimed.take() {
                        owners[n] = None;
                    }
                    if let Some(n) = next
                        && owners[n].is_none()
                    {
                        owners[n] = Some(p.id);
                        p.claimed = Some(n);
                    }
                }
                p.started = false;
                // a plan to sleep reserves its bed so others choose another (F052a3b counts the
                // squad's reservations); a plan without one lets the old reservation go
                let next_bed = p.ai.queue.iter().find_map(|s| match (s.task, s.target) {
                    (SLEEPING, Target::Furniture(b)) => Some(b),
                    _ => None,
                });
                if p.reserved_bed != next_bed {
                    if let Some(b) = p.reserved_bed.take()
                        && bed_taken[b] == Some(p.id)
                        && p.bed != Some(b)
                    {
                        bed_taken[b] = None;
                    }
                    if let Some(b) = next_bed
                        && bed_taken[b].is_none()
                    {
                        bed_taken[b] = Some(p.id);
                        p.reserved_bed = Some(b);
                    }
                }
                // Task_OperateMachine end: the operator slot is let go (a slave's machine is the
                // slave routine's, crate::slavery)
                if let Some(f) = p.operating
                    && !p.slave.as_ref().is_some_and(|s| s.machine == Some(f))
                {
                    p.operating = None;
                    if operators[f] == Some(p.id) {
                        operators[f] = None;
                    }
                }
                // Task_FollowLeader end (F030762): the move speed's own cap again; a player's
                // move order keeps the speed it was given (walk / run)
                p.mover.set_max_speed(p.ordered_cap.unwrap_or(walk));
                p.straight = None;
            }
        }
        prof.mark("sim: think");
        // each character's move speed (F047064): its run speed (F047018, athletics), its legs
        // (F047037, the worse leg's health x 1.8), hunger (F040424), its load (F047034: what it carries against its strength;
        // carrying someone, "carry person weight" more, F039146), sneaking (F047035's stealth level
        // with its buffs, below); an ordered move keeps its
        // mode's cap over it
        // each one's load, legs and dust for its fighter (CharStats +0x190, legFitness, the dust
        // storm: F047048 and F047029 read them)
        let mut fight_inputs: Vec<(usize, f32, f32, f32)> = Vec::new();
        for p in folk.iter_mut() {
            let f = &arena.f[p.id];
            let ec = kenshi_char::encumbrance::EncumbranceConstants::default();
            let strength = kenshi_char::encumbrance::strength_level(f.level(kenshi_char::StatsEnumerated::StatStrength), 1.0);
            let load = kenshi_char::encumbrance::load(kenshi_char::encumbrance::carried_weight(p.inventory.weight(), p.carrying.is_some(), &ec), 1.0, strength, &ec);
            let hunger = kenshi_char::encumbrance::hunger_speed(f.medical.hunger.level);
            // F047035 with its buffs: the stealth level x 0.75 carrying someone (Character
            // +0x348) x the load factor, x 0.01, within 0 .. 1 (x 2 crawling, stance 1: none
            // crawl here); (not ported) its medical +0xb4, CharStats +0x3c and activity-0x10
            // fitness factors
            let stealth = (f.level(kenshi_char::StatsEnumerated::StatStealth) * if p.carrying.is_some() { 0.75 } else { 1.0 } * load * 0.01).clamp(0.0, 1.0);
            let leg = |side: i32| {
                f.medical
                    .parts
                    .iter()
                    .find(|q| q.kind == 1 && q.side == side)
                    // (a splint adds to what the leg can bear, `orders`)
                    .map_or(1.0, |q| (q.health() / q.max_health()).max(0.0))
            };
            // in the water (F039118, from the terrain under it: the walking mesh lies on the
            // water's surface); swimming at the surface its body floats at 90 (F040642)
            let (swims, swim_mult) = swimmers.get(p.id).copied().unwrap_or((true, 1.0));
            let q = p.mover.position_units();
            p.water = if q[1] < WATER_LEVEL + 2.0 { water_state(terrain.height_at(q[0], q[2]).unwrap_or(q[1]), 1.0, swims) } else { 0 };
            p.float_y = (p.water == 3 && swims).then_some(SWIM_HEIGHT);
            let legs_fit = kenshi_char::encumbrance::leg_fitness(leg(1), leg(2));
            fight_inputs.push((p.id, kenshi_char::encumbrance::load(kenshi_char::encumbrance::carried_weight(p.inventory.weight(), p.carrying.is_some(), &ec), 1.0, strength, &ec), legs_fit, p.dust));
            let swim_speed = kenshi_char::encumbrance::swim_speed(
                swim_mult,
                f.level(kenshi_char::StatsEnumerated::StatSwimming),
                legs_fit,
                leg(1) > 0.0,
                leg(2) > 0.0,
                1.0,
                load,
                hunger,
            );
            let m = kenshi_char::encumbrance::MoveInputs {
                run_speed: if f.run > 0.0 { f.run } else { run },
                legs: legs_fit,
                hunger,
                equipment: 1.0,
                load,
                mult: 1.0,
                sneaking: p.sneaking.then_some(stealth),
                water: match p.water {
                    1 => kenshi_char::encumbrance::Water::Shallow,
                    2 => kenshi_char::encumbrance::Water::Wading,
                    3 => kenshi_char::encumbrance::Water::Deep,
                    _ => kenshi_char::encumbrance::Water::Dry,
                },
                swimmer: swims,
                walk_speed: walk,
                swim_speed,
                // the legs collapsed (F040452's +0x160): prone, crawling (`Fighter::prone`)
                prone: f.prone(),
                slowed: f.prone(),
            };
            p.own_speed = kenshi_char::encumbrance::move_speed(&m);
            if p.getting_up > 0.0 {
                let was = p.getting_up;
                p.getting_up = (p.getting_up - dt).max(0.0);
                // up again (Task_GetUp's end, F031044): one on hold holds where it got up
                if was > 0.0 && p.getting_up == 0.0 && p.stance.hold {
                    p.hold_at = Some(p.mover.position_units());
                }
                stop(&mut p.mover, &mut p.pathing);
            }
            if let Some(mode) = p.ordered_cap {
                p.mover.set_max_speed(speed_limit(p.own_speed, mode, None));
            }
        }
        for (id, load, legs, dust) in fight_inputs {
            let f = &mut arena.f[id];
            f.load = load;
            f.legs = legs;
            f.dust = dust;
            f.deep_water = folk[id].water == 3;
        }
        // a pickup within reach lifts the person
        let reach = 30.0f32;
        let lifts: Vec<(usize, usize)> = folk
            .iter()
            .filter_map(|p| {
                let w = p.pickup?;
                let a = p.mover.position_units();
                let b = folk[w].mover.position_units();
                ((a[0] - b[0]).hypot(a[2] - b[2]) < reach && folk[w].carried_by.is_none()).then_some((p.id, w))
            })
            .collect();
        for (c, w) in lifts {
            if !folk[c].player {
                eprintln!("t {t:.1} person {c} picks up {w}");
            }
            folk[c].pickup = None;
            folk[c].carrying = Some(w);
            folk[w].carried_by = Some(c);
            folk[c].ai.queue.clear();
            folk[c].ordered_cap = None;
            let q = &mut folk[c];
            stop(&mut q.mover, &mut q.pathing);
        }
        // the carried go where their carrier goes
        for i in 0..folk.len() {
            if let Some(c) = folk[i].carried_by {
                let at = folk[c].mover.position;
                folk[i].mover.position = at;
                arena.f[i].mover.position = at;
                folk[i].facing = folk[c].facing;
            }
        }
        prof.mark("sim: speeds, carry");
        // carry out each person's plan; the task updates see everyone's CharMovement figures
        // as they were at the start of the tick
        let bodies: Vec<Body> = folk
            .iter()
            .map(|p| Body {
                position: p.mover.position_units(),
                facing: p.facing,
                speed: p.speed,
            })
            .collect();
        let platoon_of: Vec<Option<usize>> = folk.iter().map(|p| p.platoon).collect();
        let mut dropped: HashMap<u16, u32> = HashMap::new();
        for p in folk.iter_mut() {
            let Some(step) = p.ai.queue.first().cloned() else {
                continue;
            };
            if p.down || p.getting_up > 0.0 || p.away {
                continue;
            }
            match step.task {
                MOVING => {
                    let goal = match step.target {
                        Target::Node(n) => approach[n],
                        Target::Furniture(f) => furniture_approach[f],
                        Target::Door(d) => door_states[d].inside,
                        Target::Store(o) => match stores.iter().find(|s| s.0 == o) {
                            Some(s) => s.1,
                            None => {
                                p.ai.queue.remove(0);
                                continue;
                            }
                        },
                        // an ordered point (the task's position)
                        Target::None if step.pos != [0.0; 3] => step.pos,
                        _ => {
                            p.ai.queue.remove(0);
                            continue;
                        }
                    };
                    let me = p.mover.position_units();
                    let there =
                        (goal[0] - me[0]).powi(2) + (goal[2] - me[2]).powi(2) < AT_LOCATION_SQ;
                    if there {
                        p.ai.queue.remove(0);
                        p.started = false;
                        // the ordered move is done: its speed goes with it
                        if step.target == Target::None {
                            p.ordered_cap = None;
                            p.mover.set_max_speed(walk);
                        }
                    } else if !p.started {
                        let ok = p.pathing.go_to(p.id, &p.mover, goal, false, manager);
                        if std::env::var("TOWN_DEBUG").is_ok() {
                            eprintln!(
                                "t {t:.1} person {} go_to {:?} {:?} accepted {ok} (task {:?}, queue {:?}, orders {:?})",
                                p.id,
                                step.target,
                                goal.map(|v| v.round()),
                                p.ai.current.as_ref().map(|c| c.task),
                                p.ai.queue.iter().map(|s| (s.task, s.target)).collect::<Vec<_>>(),
                                p.ai.orders.iter().map(|s| s.task).collect::<Vec<_>>()
                            );
                        }
                        p.started = true;
                    } else if !p.pathing.pending && p.pathing.follow.is_none() {
                        // the path ended short of the location, or none was found: stop here
                        if std::env::var("TOWN_DEBUG").is_ok() {
                            eprintln!(
                                "t {t:.1} person {} stopped {:.0} units from {:?} at {:?}, goal {:?}",
                                p.id,
                                ((goal[0] - me[0]).powi(2) + (goal[2] - me[2]).powi(2)).sqrt(),
                                step.target,
                                me.map(|v| v.round()),
                                goal.map(|v| v.round())
                            );
                        }
                        p.ai.queue.clear();
                        p.started = false;
                    }
                }
                // Shopping (118): the generic task (factory case 107) carries nothing out: the
                // goal is held for its duration (0.25 .. 1.5 hours, F036895)
                SHOPPING => {
                    p.ai.queue.remove(0);
                    p.started = false;
                }
                // Task_Shopping (119): crate::shopping's shopping_tick
                SHOPPING_VISIT => {}
                // Fixing Legs (269): Task_FirstAid on oneself (`first_aid_tick`)
                FIXING_LEGS => {
                    if p.first_aid.is_none() {
                        p.first_aid = Some(p.id);
                    }
                    p.ai.queue.remove(0);
                    p.started = false;
                }
                STAND_AT_NODE => {
                    if let Target::Node(n) = step.target
                        && !p.started
                    {
                        // Task_StandAtNode start (F031320): claim the node unless someone else has it
                        if owners[n].is_some_and(|o| o != p.id) {
                            p.ai.queue.clear();
                            continue;
                        }
                        owners[n] = Some(p.id);
                        p.claimed = Some(n);
                        p.started = true;
                    }
                }
                ENTERING_BUILDING | TO_DOOR_INSIDE => {
                    // Task_EnterBuilding: path to a point inside the door's building, done once
                    // inside; moving to a door from inside: to its inside point
                    let Target::Door(d) = step.target else {
                        p.ai.queue.remove(0);
                        continue;
                    };
                    let b = door_states[d].door.placement;
                    let (goal, done) = if step.task == ENTERING_BUILDING {
                        (
                            inside_point[b].unwrap_or(door_states[d].inside),
                            p.inside == Some(b),
                        )
                    } else {
                        let q = door_states[d].inside;
                        let me = p.mover.position_units();
                        (q, (q[0] - me[0]).hypot(q[2] - me[2]) < DOOR_REACH)
                    };
                    if done {
                        p.ai.queue.remove(0);
                        p.started = false;
                    } else if !p.started {
                        p.pathing.go_to(p.id, &p.mover, goal, false, manager);
                        p.started = true;
                    } else if !p.pathing.pending && p.pathing.follow.is_none() {
                        p.ai.queue.clear();
                        p.started = false;
                    }
                }
                LOCKING_DOOR => {
                    // the door shut first (Task_LockDoor, F030911, F029338); (assumption) the
                    // lock turns at once
                    if let Target::Door(d) = step.target {
                        let s = &mut door_states[d];
                        if !s.broken {
                            s.phase = DOOR_CLOSED;
                            s.open = 0.0;
                            s.locked = true;
                        }
                    }
                    p.ai.queue.remove(0);
                    p.started = false;
                }
                SLEEPING => {
                    let Target::Furniture(b) = step.target else {
                        p.ai.queue.remove(0);
                        continue;
                    };
                    if !p.started {
                        if operators[b].is_some_and(|o| o != p.id) {
                            p.ai.queue.clear();
                            continue;
                        }
                        operators[b] = Some(p.id);
                        bed_taken[b] = Some(p.id);
                        p.bed = Some(b);
                        stop(&mut p.mover, &mut p.pathing);
                        p.started = true;
                    }
                    // lying in the bed
                    p.mover.position = furn[b].position.map(|v| v * METRES);
                    p.mover.velocity = [0.0; 3];
                    p.pathing.follow = None;
                    p.straight = None;
                }
                GETTING_OUT_OF_BED => {
                    if let Some(b) = p.bed.take() {
                        if operators[b] == Some(p.id) {
                            operators[b] = None;
                        }
                        if bed_taken[b] == Some(p.id) {
                            bed_taken[b] = None;
                        }
                        p.mover.position = furniture_approach[b].map(|v| v * METRES);
                        p.woke_at = now;
                    }
                    p.ai.queue.remove(0);
                    p.started = false;
                }
                // Grabbing food (Getting food's step) takes one food from the container; Looting
                // (Looting food's) all its food. (assumption) Within STORE_REACH of its point,
                // at once; the next step otherwise ends it.
                GRABBING_FOOD | LOOTING => {
                    let Target::Store(o) = step.target else {
                        p.ai.queue.remove(0);
                        continue;
                    };
                    let at = stores.iter().find(|s| s.0 == o).map(|s| s.1);
                    let me = p.mover.position_units();
                    if at.is_some_and(|q| (q[0] - me[0]).hypot(q[2] - me[2]) < STORE_REACH) {
                        let inv = if let Some(k) = o.checked_sub(crate::loot::BOX_OWNER) {
                            self.boxes.get_mut(k).map(|b| &mut b.inventory)
                        } else {
                            o.checked_sub(crate::base::BUILDING_OWNER).and_then(|k| self.buildings.get_mut(k)).and_then(|b| b.storage.as_mut())
                        };
                        if let Some(inv) = inv {
                            let all = step.task == LOOTING;
                            'take: for s in 0..inv.sections.len() {
                                while let Some(i) = inv.sections[s].items.iter().position(|i| i.item.food.is_some()) {
                                    let Some(mut item) = inv.take(s, i) else { break };
                                    if !all && item.quantity > 1 {
                                        let mut one = item.clone();
                                        one.quantity = 1;
                                        item.quantity -= 1;
                                        inv.add(item, false);
                                        item = one;
                                    }
                                    if !p.inventory.add(item.clone(), false) && !p.inventory.bag_mut().is_some_and(|b| b.add(item.clone(), false)) {
                                        inv.add(item, false);
                                        break 'take;
                                    }
                                    if !all {
                                        break 'take;
                                    }
                                }
                            }
                        }
                    }
                    p.ai.queue.remove(0);
                    p.started = false;
                }
                // Task_OperateMachine, and Task_UseTurret (F031217) for a turret: whoever
                // operates a turret mans it (crate::turrets)
                OPERATING_MACHINE | TURRET_GUNNER | OPERATING_PRETEND => {
                    let Target::Furniture(f) = step.target else {
                        p.ai.queue.remove(0);
                        continue;
                    };
                    if std::env::var("TOWN_FIGHT_DEBUG").is_ok_and(|w| w == p.id.to_string()) {
                        eprintln!("operate {f}: started {} operator {:?}", p.started, operators[f]);
                    }
                    if !p.started {
                        // Task_OperateMachine start (F031349): stop, and take an operator slot
                        // (functionality vtable +0x4f8) unless someone else has it
                        if operators[f].is_some_and(|o| o != p.id) {
                            p.ai.queue.clear();
                            continue;
                        }
                        operators[f] = Some(p.id);
                        p.operating = Some(f);
                        stop(&mut p.mover, &mut p.pathing);
                        p.started = true;
                    }
                    // update (F031350): on a chair (function 17) or throne (24) the character
                    // sits, placed on the seat (CharMovement F0208f1)
                    p.mover.position = furn[f].position.map(|v| v * METRES);
                    p.mover.velocity = [0.0; 3];
                    p.pathing.follow = None;
                    p.straight = None;
                }
                FOLLOWING_LEADER => {
                    let Target::Person(l) = step.target else {
                        p.ai.queue.remove(0);
                        continue;
                    };
                    let me = bodies[p.id];
                    if !p.started {
                        p.follow.start(me.position);
                        p.started = true;
                    }
                    // (assumption) the task updates every frame, as its facing turns by the
                    // frame time; a member of the leader's platoon keeps its formation spot
                    let offset =
                        (p.platoon.is_some() && p.platoon == platoon_of[l]).then_some(p.offset);
                    let st = p.follow.update(me, bodies[l], offset, dt);
                    p.mover.set_max_speed(speed_limit(p.own_speed, st.cap, None));
                    p.straight = None;
                    match st.action {
                        FollowMove::Stop => stop(&mut p.mover, &mut p.pathing),
                        FollowMove::Direct { dir, dist } => {
                            p.pathing.follow = None;
                            p.straight = Some((dir, dist));
                        }
                        FollowMove::GoTo(spot) => {
                            p.pathing.go_to(p.id, &p.mover, spot, false, manager);
                        }
                    }
                }
                RUNNING => {
                    // Task_Run toward a character: a go-to at the run cap, ordered again when
                    // the target has moved off the ordered point; done at the location
                    // (conditions 46, 40, 41)
                    let Target::Person(e) = step.target else {
                        p.ai.queue.remove(0);
                        continue;
                    };
                    let goal = positions[e];
                    let me = p.mover.position_units();
                    if (goal[0] - me[0]).powi(2) + (goal[2] - me[2]).powi(2) < AT_LOCATION_SQ
                        || downs[e]
                    {
                        p.ai.queue.remove(0);
                        p.started = false;
                        p.mover.set_max_speed(walk);
                        continue;
                    }
                    let ordered = p.run_goal;
                    let moved =
                        ordered.is_none_or(|o| (o[0] - goal[0]).hypot(o[2] - goal[2]) > 50.0);
                    if !p.started || (moved && !p.pathing.pending) {
                        p.mover.set_max_speed(speed_limit(
                            p.own_speed,
                            MoveSpeed::Run.cap(15.0).unwrap(),
                            None,
                        ));
                        p.pathing.go_to(p.id, &p.mover, goal, false, manager);
                        p.run_goal = Some(goal);
                        p.started = true;
                    } else if !p.pathing.pending && p.pathing.follow.is_none() && !moved {
                        // the path ended short: stop here
                        p.ai.queue.clear();
                        p.started = false;
                    }
                }
                ATTACKING => {
                    // the fight: my fighter and the one I attack join the arena; done when the
                    // target is down or I am out of the fight
                    let Target::Person(e) = step.target else {
                        p.ai.queue.remove(0);
                        continue;
                    };
                    if downs[e] {
                        p.ai.queue.remove(0);
                        p.started = false;
                        continue;
                    }
                    // (harness: Task_Attack's own approach is not ported) close in along the
                    // navmesh at a run until within ENGAGE_RANGE, then the arena takes over
                    let goal = positions[e];
                    let me = p.mover.position_units();
                    // one with a crossbow to shoot joins once in its range (F033838)
                    let shoots = kenshi_combat::arena::ranged_usable(&arena.f[p.id])
                        && arena.f[p.id].gun.as_ref().is_some_and(|g| g.in_range(me, goal));
                    // (a slope between them is no bar: only one standing well above or below,
                    // on a roof or a ledge, is out of the fight's reach)
                    if !arena.f[p.id].active
                        && !shoots
                        && ((goal[0] - me[0]).hypot(goal[2] - me[2]) > ENGAGE_RANGE
                            || (goal[1] - me[1]).abs() > ENGAGE_RANGE)
                    {
                        let moved = p
                            .run_goal
                            .is_none_or(|o| (o[0] - goal[0]).hypot(o[2] - goal[2]) > 20.0);
                        // at a run the whole way (a new plan resets the cap to walking, and the
                        // path may still be the one asked for before it)
                        p.mover.set_max_speed(speed_limit(
                            p.own_speed,
                            MoveSpeed::Run.cap(15.0).unwrap(),
                            None,
                        ));
                        // (standing with no path: asked again once a second, not every tick
                        // while the target can't be reached)
                        let idle = !p.pathing.pending && p.pathing.follow.is_none() && (tick + p.id as u64).is_multiple_of(60);
                        if (moved || idle) && !p.pathing.pending {
                            p.pathing.go_to(p.id, &p.mover, goal, false, manager);
                            p.run_goal = Some(goal);
                        }
                        continue;
                    }
                    if !p.started {
                        stop(&mut p.mover, &mut p.pathing);
                        p.mover.set_max_speed(walk);
                        let my_side = arena.f[p.id].team;
                        for (k, at) in [(p.id, p.mover.position), (e, internal[e])] {
                            let f = &mut arena.f[k];
                            if !f.active {
                                f.active = true;
                                f.mover.position = at;
                                f.mover.velocity = [0.0; 3];
                            }
                        }
                        arena.f[p.id].focus = Some(e);
                        arena.f[p.id].pick_timer = 0.0;
                        // the attacked counts my side as enemies
                        if let Some(theirs) = &mut arena.f[e].foes
                            && !theirs.contains(&my_side)
                        {
                            theirs.push(my_side);
                        }
                        p.started = true;
                    }
                }
                PATROLLING if p.caravan.is_some() => {
                    // Package_WanderingTrader (F028938, F029005, F028941): to a town, a stay of
                    // 5 to 8 hours there, on to another town by day
                    let mut c = p.caravan.take().unwrap_or_default();
                    let me = p.mover.position_units();
                    let hour_of_day = (start_hour + now).rem_euclid(24.0) as i32;
                    c.arrive(p.town, now);
                    let lost = c.target.is_none_or(|t| t >= towns.len());
                    if lost || c.leaving(now, hour_of_day) {
                        let mine = p.faction.as_deref().and_then(|f| relations.get(f));
                        let view: Vec<_> = towns
                            .iter()
                            .map(|t| kenshi_ai::caravan::TownView {
                                position: t.position,
                                kind: t.kind,
                                public: t.public,
                                faction: t.faction.as_deref(),
                            })
                            .collect();
                        let roll = rng.next();
                        let next = kenshi_ai::caravan::find_town(
                            &view,
                            p.faction.as_deref(),
                            me,
                            p.town,
                            kenshi_ai::caravan::VISIT_TYPES,
                            |f| mine.is_some_and(|r| r.is_enemy(f)),
                            roll,
                        );
                        c = kenshi_ai::caravan::start(next, rng.next());
                        p.destination = next;
                        p.started = false;
                        if std::env::var("KENSHI_CARAVAN_DEBUG").is_ok() {
                            eprintln!(
                                "caravan {} ({}) heads for {:?}, staying {:.1} h",
                                p.id,
                                p.name,
                                next.map(|t| &towns[t].name),
                                c.stay
                            );
                        }
                    }
                    // CharMovement's road walk (F040650): marker to marker on the navmesh
                    let roads = world.grid.as_deref().map(|g| &g.roads);
                    if let Some(net) = roads
                        && p.roads.active()
                    {
                        let done = !p.pathing.pending && p.pathing.follow.is_none();
                        let failed = done && p.pathing.failed.is_some();
                        // (stand-in for the paging: Kenshi unloads one walking off the loaded
                        // world into an UnloadedPlatoon) a way whose next marker lies off the
                        // loaded navmesh waits there
                        let next = p.roads.path.as_ref().map(|r| r.target(net));
                        let loaded = next.is_some_and(|m| nav.tiles.contains(&zone_at(m[0], m[2])));
                        if loaded && let Some(q) = p.roads.tick(net, me, done && !failed, failed) {
                            p.pathing.go_to(p.id, &p.mover, q, false, manager);
                        }
                    }
                    let idle = !p.pathing.pending && p.pathing.follow.is_none() && !p.roads.active();
                    if let Some(t) = c.target
                        && (!p.started || idle)
                    {
                        // a point in the town (CharMovement::order, area mode), goTo taking the
                        // roads to it when over 1000 away and they pay (F040653 -> F040644)
                        let town = &towns[t];
                        let point = area_point(
                            town.position,
                            order_radius(Some(town.radius())),
                            true,
                            || rng.next(),
                            |x, z| terrain.height_at(x, z).unwrap_or(NO_GROUND),
                        );
                        let point = point.map(|q| roads.map_or(q, |net| p.roads.go_to(net, me, q)));
                        // (stand-in for Havok's paths into unloaded navmesh) off the roads, far
                        // off: a leg of most of a zone toward the town, ordered again when idle
                        let leg = kenshi_world::ZONE_SIZE * 0.3;
                        let point = point.and_then(|q| {
                            let (dx, dz) = (q[0] - me[0], q[2] - me[2]);
                            let d = dx.hypot(dz);
                            if p.roads.active() || d <= town.radius() + leg {
                                return Some(q);
                            }
                            area_point(
                                [me[0] + dx / d * leg, me[1], me[2] + dz / d * leg],
                                leg * 0.1,
                                true,
                                || rng.next(),
                                |x, z| terrain.height_at(x, z).unwrap_or(NO_GROUND),
                            )
                        });
                        let ok = point.is_some_and(|q| p.pathing.go_to(p.id, &p.mover, q, false, manager));
                        if std::env::var("KENSHI_CARAVAN_DEBUG").is_ok() {
                            eprintln!("caravan {} leg to {point:?} (roads {}): {ok}", p.id, p.roads.active());
                        }
                        p.started = true;
                    }
                    p.caravan = Some(c);
                }
                PATROLLING => {
                    // Task_Patrol (F030864 start, F030865 update): an area order (mode 3,
                    // CharMovement::order -> F048160: a zone of the same biome round the one it is
                    // in; (assumption) the zones weigh the same, F048108 not ported) to a random
                    // point there, ordered again whenever the mover is idle
                    let idle = !p.pathing.pending && p.pathing.follow.is_none();
                    if !p.started || idle {
                        let me = p.mover.position_units();
                        let here = zone_at(me[0], me[2]);
                        let mut zones = vec![here];
                        if let Some(pop) = population.as_ref() {
                            let mine = biome_of_zone(&pop.map, &pop.biomes, here.0, here.1);
                            for dx in -1..=1 {
                                for dy in -1..=1 {
                                    let z = (here.0 + dx, here.1 + dy);
                                    if z != here
                                        && nav.tiles.contains(&z)
                                        && biome_of_zone(&pop.map, &pop.biomes, z.0, z.1) == mine
                                    {
                                        zones.push(z);
                                    }
                                }
                            }
                        }
                        let k = ((rng.next() * zones.len() as f32) as usize).min(zones.len() - 1);
                        let (cx, cz) = zone_centre(zones[k].0, zones[k].1);
                        let point = area_point(
                            [cx, me[1], cz],
                            kenshi_world::ZONE_SIZE * 0.8,
                            true,
                            || rng.next(),
                            |x, z| terrain.height_at(x, z).unwrap_or(NO_GROUND),
                        );
                        if let Some(q) = point {
                            p.pathing.go_to(p.id, &p.mover, q, false, manager);
                        }
                        p.started = true;
                    }
                }
                PATROL_TOWN => {
                    // F030869 orders at once (mode 0); F030870 orders again once the mover is
                    // idle (mode 1: CharMovement slots 10, 12, 27)
                    let idle = !p.pathing.pending && p.pathing.follow.is_none();
                    if !p.started || idle {
                        let me = p.mover.position_units();
                        let (centre, r) = match p.destination {
                            Some(t) => (towns[t].position, order_radius(Some(towns[t].radius()))),
                            None => (me, order_radius(None)),
                        };
                        let point = area_point(
                            centre,
                            r,
                            true,
                            || rng.next(),
                            |x, z| terrain.height_at(x, z).unwrap_or(NO_GROUND),
                        );
                        if let Some(q) = point {
                            p.pathing.go_to(p.id, &p.mover, q, false, manager);
                        }
                        p.started = true;
                    }
                }
                _ => {
                    *dropped.entry(step.task).or_default() += 1;
                    p.ai.queue.remove(0);
                }
            }
        }
        prof.mark("sim: plans, movement");
        for (k, n) in dropped {
            *self.dropped_steps.entry(k).or_default() += n;
        }
        // one path request answered per tick (F032235)
        if let Some((who, result)) = manager.update(nav) {
            if std::env::var("TOWN_DEBUG").is_ok() {
                match &result {
                    kenshi_world::navmesh::PathResult::Found(f) => eprintln!(
                        "t {t:.1} person {who} path {} points {:?} -> {:?}, {} gates",
                        f.points.len(),
                        f.points.first().map(|v| v.map(|c| (c / METRES).round())),
                        f.points.last().map(|v| v.map(|c| (c / METRES).round())),
                        f.follow.gates.len()
                    ),
                    other => eprintln!("t {t:.1} person {who} no path: {other:?} (task {:?}, queue {:?})", folk[who].ai.current.as_ref().map(|c| c.task), folk[who].ai.queue.iter().map(|s| s.task).collect::<Vec<_>>()),
                }
            }
            // no way through: a shut gate in the way? (the way round it, ignoring gates)
            if !matches!(result, kenshi_world::navmesh::PathResult::Found(_))
                && let Some(r) = manager.last.as_ref().filter(|r| !r.pass.ignore)
                && let kenshi_world::navmesh::PathResult::Found(free) = nav.find_path_as(r.start, r.goal, r.radius, r.water_cost, kenshi_world::navmesh::Pass::ANYONE)
            {
                let pts = &free.points;
                if let Some(k) = (1..pts.len()).find_map(|i| nav.barriers.iter().position(|b| b.stops(r.pass, pts[i - 1], pts[i]))) {
                    folk[who].gate_in_way = Some(k);
                }
            }
            // F044546 (from Task_Move's update, F030731, and the other moving tasks): a path that
            // can't be had puts its target in the AI's ignore map for 20 s and ends the task
            if !matches!(result, kenshi_world::navmesh::PathResult::Found(_)) && folk[who].gate_in_way.is_none() {
                let p = &mut folk[who];
                let target = p.ai.queue.first().map(|s| s.target).or(p.ai.current.as_ref().map(|c| c.target));
                if let Some(Target::Person(t)) = target {
                    match p.ignored.iter_mut().find(|i| i.0 == t) {
                        Some(i) => i.1 = IGNORE_SECONDS,
                        None => p.ignored.push((t, IGNORE_SECONDS)),
                    }
                    p.ai.queue.clear();
                    p.ai.orders.retain(|o| o.target != Target::Person(t));
                    p.rethink = true;
                }
            }
            folk[who].pathing.answer(result);
        }
        prof.mark("sim: paths");
        // the fights: the arena runs its fighters; a fighter with no one left to fight leaves it
        // those out of the fight stand where they are (bolts and lines of fire meet them there)
        for p in folk.iter() {
            let f = &mut arena.f[p.id];
            // a player's character sent elsewhere runs there through the fight
            f.ordered_away = p.player && p.ai.orders.first().is_some_and(|o| !matches!(o.task, ATTACKING | ATTACKING_ENEMIES | ATTACKING_NEUTRALS));
            if !f.active || f.ordered_away {
                f.mover.position = p.mover.position;
            }
        }
        // CombatClass keeps a path to its target (+0x200; AttackState::canEnter, F029603, asks
        // whether the target is reachable along it): (harness) a fighter whose straight way to
        // its target is blocked steers by the corners of a navmesh path there, looked at again
        // twice a second; the arena is given the next corner
        for p in folk.iter_mut() {
            let target = arena.f[p.id].focus.filter(|_| arena.f[p.id].active && !arena.f[p.id].down);
            let Some(t) = target else {
                p.fight_path.clear();
                arena.f[p.id].via = None;
                continue;
            };
            let (me, goal) = (p.mover.position, arena.f[t].mover.position);
            if (tick + p.id as u64).is_multiple_of(30) {
                let blocked = nav.closest_point(me, 5.0).is_some_and(|(face, _)| !nav.line_clear(face, me, goal, p.mover.radius, kenshi_world::navmesh::edge_enabled));
                p.fight_path = match blocked.then(|| nav.find_path_as(me, goal, p.mover.radius, p.mover.water_cost, p.pathing.pass)) {
                    Some(kenshi_world::navmesh::PathResult::Found(fp)) => fp.points,
                    _ => Vec::new(),
                };
            }
            while p.fight_path.first().is_some_and(|c| (c[0] - me[0]).hypot(c[2] - me[2]) < 0.8) {
                p.fight_path.remove(0);
            }
            // the last corner is the target itself: then it is steered at directly
            arena.f[p.id].via = (p.fight_path.len() > 1).then(|| p.fight_path[0].map(|v| v * 10.0));
        }
        // the ground in each shooter's line of fire (F033852's trace)
        for i in 0..arena.f.len() {
            let blocked = match (&arena.f[i].gun, arena.f[i].focus) {
                (Some(_), Some(t)) if t < arena.f.len() => {
                    let (a, b) = (arena.f[i].pos(), arena.f[t].pos());
                    let h = kenshi_combat::arena::AIM_HEIGHT;
                    kenshi_combat::arena::ground_between([a[0], a[1] + h, a[2]], [b[0], b[1] + h, b[2]], &mut |x, z| terrain.height_at(x, z).unwrap_or(NO_GROUND))
                }
                _ => false,
            };
            arena.f[i].ground_in_line = blocked;
        }
        // (crate::law) the melee hits this step lands
        let hits_from = arena.sh.hits.len();
        arena.step(dt);
        law_state.melee_hits = hits_from..arena.sh.hits.len();
        arena.step_turrets(dt);
        // the bolts in flight, against the terrain; the ammo charges spent taken from inventories
        arena.step_bolts(dt, &mut |x, z| terrain.height_at(x, z).unwrap_or(NO_GROUND));
        for p in folk.iter_mut() {
            let f = &mut arena.f[p.id];
            if f.ammo_spent == 0 {
                continue;
            }
            if let Some(a) = f.gun.as_ref().and_then(|g| g.ammo.clone()) {
                for _ in 0..f.ammo_spent {
                    p.inventory.spend_ammo(&a);
                }
                f.ammo = p.inventory.ammo_charges(&a);
            }
            f.ammo_spent = 0;
        }
        prof.mark("sim: arena");
        // everyone's medical tick (F040477): bleeding, the knockout clock, healing, death; the
        // knocked out come round once it allows (wake_ready)
        let mut woken: Vec<usize> = Vec::new();
        let mut unfed: Vec<(usize, f32)> = Vec::new();
        // each bed's healing when slept in: its BUILDING's "output rate" (a camp bed's 4, "a 400%
        // heal rate when sleeping"), 1 where it gives none
        let bed_rate: Vec<f32> = self.furn.iter().map(|b| self.data.get(&b.record).and_then(|r| r.floats.get("output rate").copied()).filter(|&v| v > 0.0).unwrap_or(1.0)).collect();
        for p in folk.iter_mut() {
            if p.dead {
                continue;
            }
            let f = &mut arena.f[p.id];
            let tick = kenshi_char::medical::MedicalTick {
                dt,
                race_heal: 1.0,
                race_bleed: 1.0,
                race_blood_recovery: 1.0,
                rate: 1.0,
                // in a bed: asleep (bandaged health heals over fast, at the bed's rate) and
                // resting (lost health comes back), as Kenshi's bed rest (F040477's sleeping and
                // resting rates)
                sleep_rate: p.bed.map_or(1.0, |b| bed_rate.get(b).copied().unwrap_or(1.0)),
                rest: if p.bed.is_some() { 1.0 } else { 0.0 },
                resting: p.down || p.bed.is_some(),
                asleep: p.bed.is_some(),
                size: 1.0,
                bleeds: true,
                g: advanced.death,
            };
            // hunger (F040466 with F047009's rate, F032933's period); (assumption) the
            // starvation time is in game hours. Everyone's falls; an NPC's stomach refills it
            // up to its cap (F039188, `hunger_caps`), the player's people must eat
            if !p.player && !p.away {
                use kenshi_char::hunger::{Activity, hunger_rate};
                let [starve, fed, bed, enc, race_rate] = hunger_c;
                let cap = hunger_caps.get(&p.id).copied().unwrap_or(3.0);
                let activity = if p.down { Activity::Resting } else { Activity::Normal };
                let rate = hunger_rate(activity, 1.0, race_rate, bed, enc);
                let period = kenshi_char::hunger::starvation_period(starve * advanced.hunger_time, hour_secs as f32);
                f.medical.hunger.update(dt, cap, period, rate, 1.0, fed, 1.0, false);
            }
            if p.player {
                use kenshi_char::hunger::{Activity, hunger_rate};
                let [starve, fed, bed, enc, race_rate] = hunger_c;
                let ec = kenshi_char::encumbrance::EncumbranceConstants::default();
                let strength = kenshi_char::encumbrance::strength_level(f.level(kenshi_char::StatsEnumerated::StatStrength), 1.0);
                let load = kenshi_char::encumbrance::load(kenshi_char::encumbrance::carried_weight(p.inventory.weight(), p.carrying.is_some(), &ec), 1.0, strength, &ec);
                let activity = if p.down {
                    Activity::Resting
                } else if load < 1.0 {
                    Activity::Encumbered { load }
                } else {
                    Activity::Normal
                };
                let rate = hunger_rate(activity, 1.0, race_rate, bed, enc);
                // (the advanced options' "Hunger time" lengthens it)
                let period = kenshi_char::hunger::starvation_period(starve * advanced.hunger_time, hour_secs as f32);
                f.medical.hunger.tick(dt, period, rate, 1.0, fed, 1.0);
                // eating (CharacterHuman's autoEat, F039268, each update): one with nothing
                // queued to digest (+0x4bc) and hunger under 2.5 (F039117) that is hungry
                // (F039189: one of the player's always, else under 2.1) eats a bite of what it
                // carries, its own then its backpack's (F039256: at most 50, the food's left,
                // the room under 300 less the hunger level as Kenshi takes it)
                p.eat_timer -= dt;
                let h = &f.medical.hunger;
                if p.player && ((t * 60.0) as u64).is_multiple_of(600) && std::env::var("KENSHI_HUNGER_DEBUG").is_ok() {
                    eprintln!("t {t:.1} hunger of {}: {:.1} (stomach {:.3}, queued {:.3}), period {period:.0} s, rate {rate:.2}", p.id, h.shown(), h.stomach, h.queued);
                }
                let shown = (h.level + h.stomach + h.queued) * 100.0;
                let hungry = h.queued <= 0.0 && h.level < 2.5 && (p.player || h.level < 2.1);
                if !p.down && hungry && p.carried_by.is_none() {
                    if let Some((food, bite)) = p.inventory.eat(h.level) {
                        f.medical.hunger.eat(bite);
                        if p.player || std::env::var("KENSHI_HUNGER_DEBUG").is_ok() {
                            eprintln!("t {t:.1} person {} ate {food} ({bite:.0}), hunger {shown:.0}", p.id);
                        }
                    } else if h.level < 2.25 {
                        unfed.push((p.id, h.level));
                    }
                }
            }
            let mc = arena.sh.mc.clone();
            if !f.medical.update(&tick, &f.stats, &mc) {
                if !p.dead {
                    eprintln!("t {t:.1} person {} died", p.id);
                    if let Some(c) = p.character.clone() {
                        world.pending_deaths.push(c);
                    }
                }
                p.dead = true;
                p.down = true;
                f.down = true;
                p.ai.queue.clear();
                stop(&mut p.mover, &mut p.pathing);
                continue;
            }
            if p.down && std::env::var("KENSHI_MED_DEBUG").is_ok() && ((t * 10.0) as u64).is_multiple_of(100) {
                let ko = kenshi_char::medical::Medical::ko_point(&f.stats, &mc);
                let hs: Vec<String> = f.medical.parts.iter().map(|q| format!("{:.0}{}", q.health(), if q.vital { "v" } else { "" })).collect();
                eprintln!("med {} ko {ko:.0} kt {:.1} blood {:.0} bko {} coma {} [{}]", p.id, f.medical.knockout_time, f.medical.blood, f.medical.blood_ko, f.medical.coma, hs.join(" "));
            }
            if p.down && wake_ready(&f.medical, &f.stats, &mc) {
                woken.push(p.id);
            }
        }
        for p in folk.iter_mut() {
            let f = &mut arena.f[p.id];
            if !f.active {
                continue;
            }
            if f.down && !p.down {
                p.down = true;
                p.ai.queue.clear();
                stop(&mut p.mover, &mut p.pathing);
                eprintln!("t {t:.1} person {} is knocked out", p.id);
            }
        }
        // Share Food (the AI tab, F039268): one with nothing of its own to eat, its hunger under
        // 2.25, eats from the backpack of a squad mate within 200 units
        // Feed Animals: "If food is running low, you can elect not to feed your animals": off,
        // the squad's animals are not given its food
        if ai_options.share_food {
            for (id, shown) in unfed {
                if folk[id].race.is_some() && !ai_options.feed_animals {
                    continue;
                }
                let at = folk[id].mover.position_units();
                let platoon = folk[id].platoon;
                let mate = (0..folk.len()).find(|&m| {
                    let q = &folk[m];
                    let r = q.mover.position_units();
                    m != id && q.player && !q.dead && q.platoon == platoon && (r[0] - at[0]).hypot(r[2] - at[2]) < SHARE_FOOD_RANGE && q.inventory.bag().is_some_and(|b| b.sections.iter().any(|s| s.items.iter().any(|i| i.item.food.is_some())))
                });
                if let Some(m) = mate
                    && let Some((food, bite)) = folk[m].inventory.bag_mut().and_then(|b| b.eat(shown))
                {
                    arena.f[id].medical.hunger.eat(bite);
                    eprintln!("t {t:.1} person {id} ate {food} ({bite:.0}) of person {m}'s, hunger {shown:.0}");
                }
            }
        }
        prof.mark("sim: medical");
        // (the player's) come round while enemies fight near: playing dead until they leave, an
        // order to stand up, or a move order ("To get up, right click to move")
        let fighting: Vec<[f32; 3]> = folk.iter().filter(|q| !q.player && !q.down && !q.dead && arena.f[q.id].active).map(|q| q.mover.position_units()).collect();
        let mut woken_now = Vec::new();
        for id in woken {
            let p = &mut folk[id];
            if p.player && p.carried_by.is_none() {
                let at = p.mover.position_units();
                let near = fighting.iter().any(|q| (q[0] - at[0]).hypot(q[2] - at[2]) < 800.0);
                let up = std::mem::take(&mut p.stand_up) || !p.ai.orders.is_empty();
                if near && !up {
                    if !p.playing_dead {
                        eprintln!("t {t:.1} person {id} plays dead");
                    }
                    p.playing_dead = true;
                    continue;
                }
            }
            p.playing_dead = false;
            woken_now.push(id);
        }
        for id in woken_now {
            arena.f[id].medical.knocked_out = false;
            arena.f[id].medical.coma = false;
            let len = arena.sh.lengths.get("standing up 3").copied().unwrap_or(0.0);
            let p = &mut folk[id];
            if p.carried_by.is_none() {
                p.down = false;
                arena.f[id].down = false;
                p.getting_up = len;
                p.rethink = true;
                eprintln!("t {t:.1} person {id} comes round");
            }
        }
        for k in 0..arena.f.len() {
            let f = &arena.f[k];
            if f.active
                && !f.down
                && f.act.is_none()
                && !(0..arena.f.len()).any(|o| o != k && opponent(&arena.f[k], &arena.f[o]))
                && !shooting_at(&arena.f, k)
            {
                if std::env::var("KENSHI_RANGED_DEBUG").is_ok() {
                    eprintln!("t {t:.1} {} leaves the fight", arena.f[k].name);
                }
                arena.f[k].active = false;
                arena.f[k].focus = None;
            }
        }
        prof.mark("sim: wake, leave fights");
        for p in folk.iter_mut() {
            if p.away {
                continue;
            }
            let before = p.mover.position_units();
            if (arena.f[p.id].active && !arena.f[p.id].ordered_away) || p.down || p.getting_up > 0.0 {
                // the arena moves a fighter: (harness) its step is walked across the navmesh
                // and refused where the mesh ends; the knocked out lie still, and stay put
                // while getting up
                if arena.f[p.id].active {
                    let (from, to) = (p.mover.position, arena.f[p.id].mover.position);
                    // (harness for the Havok character) a step into a wall slides along it
                    let face = nav.closest_point(from, 5.0).map(|(f, _)| f);
                    if let Some((f, to)) = face.and_then(|f| nav.slide(f, from, to)) {
                        p.mover.position = [to[0], nav.height_on(f, to), to[2]];
                        if p.inside.is_none() {
                            stand_on_ground(&mut p.mover.position, terrain, nav);
                        }
                    }
                    arena.f[p.id].mover.position = p.mover.position;
                }
                p.mover.velocity = [0.0; 3];
                p.pathing.follow = None;
                p.straight = None;
                let (v, speed) = measured_velocity(before, p.mover.position_units(), dt);
                p.speed = speed;
                if speed > 0.1 {
                    p.facing = v.map(|c| c / speed);
                }
                continue;
            }
            if let Some((dir, dist)) = p.straight {
                // CharMovement mode 2; (harness) the Havok character keeps to the navmesh: it
                // walks from face to face and stops where the mesh ends
                let from_m = p.mover.position;
                let face = nav.closest_point(from_m, 5.0).map(|(f, _)| f);
                direct(&mut p.mover, dir, dist, dt);
                match face.and_then(|f| nav.walk(f, from_m, p.mover.position)) {
                    Some(f) => p.mover.position[1] = nav.height_on(f, p.mover.position),
                    None => {
                        p.mover.position = from_m;
                        p.mover.velocity = [0.0; 3];
                    }
                }
            } else {
                p.pathing.tick(&mut p.mover, nav, dt);
            }
            // outdoors the body stands on the terrain, not the navmesh
            if p.inside.is_none() {
                stand_on_ground(&mut p.mover.position, terrain, nav);
            }
            // F040650: the measured velocity, its speed (+0xb8) and, when moving, its
            // direction (+0xd0)
            let (v, speed) = measured_velocity(before, p.mover.position_units(), dt);
            p.speed = speed;
            if speed > 0.1 {
                p.facing = v.map(|c| c / speed);
            }
            // moving trains the player's people (F047463, for a squad of the player's): in deep
            // water swimming (x 5 a second, x 10 while carrying someone: Character +0x348),
            // else athletics by F047452 (the load factor, at least 0, x 1.5 when faster than 0.8
            // of its run speed, x "xp rate athletics") and strength from walking under a load
            // (F047455)
            if speed > 5.0 && p.player {
                use kenshi_char::StatsEnumerated::{StatStrength, StatSwimming};
                let f = &mut arena.f[p.id];
                let ec = kenshi_char::encumbrance::EncumbranceConstants::default();
                let strength = kenshi_char::encumbrance::strength_level(f.level(StatStrength), 1.0);
                let load = kenshi_char::encumbrance::load(kenshi_char::encumbrance::carried_weight(p.inventory.weight(), p.carrying.is_some(), &ec), 1.0, strength, &ec);
                if p.water == 3 {
                    f.skills.train(StatSwimming, dt * 5.0 * if p.carrying.is_some() { 10.0 } else { 1.0 }, 1.0);
                } else {
                    let my_run = if p.own_speed > 0.0 { p.own_speed } else { run };
                    let pace = if speed > my_run * 0.8 { 1.5 } else { 1.0 };
                    let xp = arena.sh.xp;
                    f.skills.train_athletics(dt * load.max(0.0) * pace, false, &xp);
                }
                let gain = kenshi_char::encumbrance::strength_xp_from_walking(load, p.carrying.is_some(), &ec);
                if gain > 0.0 {
                    f.skills.train(StatStrength, gain * dt, 1.0);
                }
            }
        }
        prof.mark("sim: moving");
        self.t += dt;
        self.tick += 1;
        if self.tick.is_multiple_of(60) {
            self.stream();
        }
        prof.mark("sim: stream");
        if self.tick.is_multiple_of(600) {
            self.restock_shops();
        }
        if self.tick.is_multiple_of(60) {
            self.meet_tick();
            self.capture_tick();
            self.intruders_tick();
            self.speech_tick();
            self.loot_tick();
            self.artifacts_tick();
        }
        prof.mark("sim: shops, meeting, speech, loot");
        if !self.prisoner_squads.is_empty() {
            self.spawn_prisoners();
        }
        self.slavery_tick();
        if self.tick.is_multiple_of(60) {
            self.stalking_tick();
        }
        if self.tick.is_multiple_of(60) {
            self.name_people();
        }
        self.ignore_tick(dt);
        self.slave_work_tick(dt);
        self.disguise_tick(dt);
        self.prospect_tick(dt);
        self.memory_tick();
        self.regenerate_tick();
        self.cage_push();
        prof.mark("sim: slavery, disguise, memory, regenerate");
        self.medic_tick(dt);
        self.looter_tick(dt);
        self.looting_tick(dt);
        prof.mark("sim: medics, looters");
        self.first_aid_tick(dt);
        self.orders_tick(dt);
        self.shopping_tick(dt);
        self.routines_tick(dt);
        prof.mark("sim: first aid, orders, routines");
        self.hazards_tick(dt);
        prof.mark("sim: hazards");
        self.escort_tick();
        self.pickup_tick();
        self.weapon_tick(dt);
        prof.mark("sim: escorts, pickups");
        self.build_tick(dt);
        self.doors_tick(dt);
        self.dismantle_tick(dt);
        self.production_tick(dt);
        self.training_tick(dt);
        self.research_tick(dt);
        self.farm_tick(dt);
        self.power_tick(dt);
        self.limb_tick();
        prof.mark("sim: base");
        self.law_tick(dt);
        prof.mark("sim: law");
        self.world_tick(dt);
        prof.mark("sim: world");
        self.population_tick(dt);
        prof.mark("sim: population");
    }

    /// The arena side of a faction, added (with its relations, and the foes it makes of the
    /// sides already there) when new.
    pub fn side(&mut self, faction: &Option<String>) -> u16 {
        if let Some(k) = self.sides.iter().position(|s| s == faction) {
            return k as u16;
        }
        self.sides.push(faction.clone());
        let k = (self.sides.len() - 1) as u16;
        if let Some(f) = faction
            && !self.relations.contains_key(f)
            && let Some(r) = self.data.get(f)
        {
            self.relations
                .insert(f.clone(), FactionRelations::from_data(r, &self.data));
        }
        // the sides already here that count the newcomer an enemy
        for p in &self.folk {
            if let Some(mine) = &p.faction
                && self
                    .relations
                    .get(mine)
                    .is_some_and(|r| r.is_enemy(faction.as_deref()))
                && let Some(foes) = &mut self.arena.f[p.id].foes
                && !foes.contains(&k)
            {
                foes.push(k);
            }
        }
        k
    }

    /// A character of the zone's race at `at` (units), with its goals, arena fighter and side.
    pub(crate) fn add_person(
        &mut self,
        at: [f32; 3],
        jobs: Vec<PackageGoal>,
        leader: Option<usize>,
        platoon: Option<usize>,
        faction: Option<String>,
        role: u8,
    ) -> usize {
        let id = self.folk.len();
        let p = at.map(|v| v * METRES);
        let jitter = [
            self.rng.next() * 6.0 - 3.0,
            0.0,
            self.rng.next() * 6.0 - 3.0,
        ];
        let start = self
            .nav
            .closest_point([p[0] + jitter[0], p[1], p[2] + jitter[2]], 50.0)
            .map_or(p, |(_, q)| q);
        let mut mover =
            Mover::for_character(&self.race, start.map(|v| v / METRES), 1.0, 1.0, false);
        mover.position = start;
        mover.set_max_speed(self.walk);
        let mut ai = TaskSystem::new(Target::None, start);
        // "attack enemies and neutrals" (Attacking others, 21): its finder takes the neutrals
        // known as well as the enemies (F038642)
        let attacks_neutrals = jobs.iter().any(|g| g.task == ATTACKING_NEUTRALS);
        let captures = jobs.iter().any(|g| g.task == CAPTURE_SLAVES || g.task == CARRY_SLAVES || g.task == KIDNAPPER);
        let duties = duties_of(jobs.iter().map(|g| g.task));
        let wanders = jobs.iter().any(|g| g.task == WANDERING_TRADER);
        ai.jobs = jobs_of(jobs, leader, None);
        let a = self.rng.next() * std::f32::consts::TAU;
        self.folk.push(Person {
            id,
            mover,
            pathing: Pathing::default(),
            ai,
            claimed: None,
            shopping: None,
            shop_visit: None,
            slave: None,
            started: false,
            home: None,
            role,
            town: None,
            destination: None,
            platoon,
            offset: [0.0; 3],
            follow: FollowLeader::default(),
            straight: None,
            facing: [a.sin(), 0.0, a.cos()],
            speed: 0.0,
            ordered_cap: None,
                sneaking: false,
                carrying: None,
                carried_by: None,
                pickup: None,
                own_speed: 0.0,
                name: String::new(),
                inventory: kenshi_char::inventory::Inventory::character(),
                dead: false,
                character: None,
                eat_timer: 0.0,
                shop: None,
                dialogue: None,
                race: None,
                attacks_neutrals: false,
                package: None,
                captures: false,
                duties: 0,
                build_fetch: None,
                stance: Default::default(),
                hold_at: None,
                jobs: Vec::new(),
                jobs_on: false,
                weapon_out: false,
                weapon_idle: 0.0,
                caravan: None,
                first_aid: None,
                loot: None,
                escort: None,
                build: None,
                operate: None,
                haul: None,
                crime: None,
                bounties: Vec::new(),
                heat: 0.0,
                prison: None,
                picking: false,
                shackled: false,
                shaved: false,
                disguised: None,
                disguise_blown: 0.0,
                female: None,
                personality: 0,
                look_race: None,
                escaped: None,
                freeing: None,
                arrest: None,
                surrendered: false,
                playing_dead: false,
                stand_up: false,
                auto_haul: None,
                dust: 0.0,
                hazard: None,
                water: 0,
                float_y: None,
                job: None,
                idle_for: 0.0,
                rest_injured: false,
                age: 1.0,
                ko_target: None,
                fetch: None,
                world_squad: None,
                away: false,
                limbs: Default::default(),
                getting_up: 0.0,
            operating: None,
            inside: None,
            bed: None,
            woke_at: -100.0,
            reserved_bed: None,
            faction: faction.clone(),
            down: false,
            run_goal: None,
            player: false,
            rethink: false,
            gate_in_way: None,
            ignored: Vec::new(),
            fight_path: Vec::new(),
            bash_order: None,
            door_order: None,
            dismantle: None,
            bash_timer: 0.0,
            stalk: None,
            roads: Default::default(),
        });
        self.folk[id].attacks_neutrals = attacks_neutrals;
        // CharMovement::init (F040657): the road factor of the faction's "road preference"
        let road_pref = faction
            .as_deref()
            .and_then(|f| self.data.get(f))
            .and_then(|f| f.floats.get("road preference").copied())
            .unwrap_or(0.0);
        self.folk[id].roads.road_factor = kenshi_world::roads::road_factor(road_pref);
        self.folk[id].captures = captures;
        self.folk[id].duties = duties;
        self.folk[id].caravan = wanders.then(kenshi_ai::caravan::Caravan::default);
        let side = self.side(&faction);
        let race = self.data.get(&self.race_record).expect("race").clone();
        let mut f = Fighter::new(
            &self.data,
            &race,
            &self.kit,
            &id.to_string(),
            side,
            KIT_SKILL,
            at,
        );
        f.mover.position = start;
        f.active = false;
        let rel = faction.as_ref().and_then(|f| self.relations.get(f));
        f.foes = Some(
            self.sides
                .iter()
                .enumerate()
                .filter(|(_, o)| rel.is_some_and(|r| r.is_enemy(o.as_deref())))
                .map(|(k, _)| k as u16)
                .collect(),
        );
        if attacks_neutrals {
            // every other side
            f.foes = None;
        }
        self.arena.add(f);
        id
    }

    /// A zone streamed in: its buildings' nodes, furniture, interiors and doors joined to the
    /// world's (their placement indices offset past the ones already in), the navmesh round it,
    /// its buildings' resident squads (building state "squad template") at home, and its town's
    /// bar squads at its bar, each kitted from its template's characters. Zones already in are
    /// left alone.
    pub fn add_zone(&mut self, zx: i32, zy: i32, res: &kenshi_io::resources::Resources) -> usize {
        if !self.zones.insert((zx, zy)) {
            return 0;
        }
        self.zone_order.push((zx, zy));
        let Some(zone) = kenshi_world::read_zone(&self.install, zx, zy) else { return 0 };
        let placed = placements(&zone);
        if placed.is_empty() {
            return 0;
        }
        let install = self.install.clone();
        let data = self.data.clone();
        if self.layouts.is_none() {
            self.layouts = Some(Layouts::load(&install));
        }
        // the navmesh round the zone's middle
        let c = |i: i32| (i as f32 + 0.5) * kenshi_world::ZONE_SIZE - kenshi_world::WORLD_SIZE / 2.0;
        let (tx, ty) = tile_of([c(zx) * METRES, 0.0, c(zy) * METRES]);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let _ = self.nav.ensure(&install, tx + dx, ty + dy);
            }
        }
        let off = self.placed.len();
        self.zone_offsets.insert((zx, zy), off);
        let layouts = self.layouts.as_ref().unwrap();
        let near = |nav: &NavWorld, at: [f32; 3], r: f32| {
            nav.closest_point(at.map(|v| v * METRES), r).map_or(at, |(_, q)| q.map(|v| v / METRES))
        };
        // nodes
        for mut n in nodes(&data, layouts, &placed).into_iter().filter(|n| n.kind <= NODE_GUARDPOST) {
            n.placement += off;
            self.approach.push(near(&self.nav, n.position, 50.0));
            self.owners.push(None);
            self.general.push(n);
        }
        // furniture
        let furn_off = self.furn.len();
        for mut f in furniture(&data, layouts, &placed) {
            f.placement += off;
            self.furniture_approach.push(near(&self.nav, f.position, 50.0));
            self.operators.push(None);
            self.bed_taken.push(None);
            self.furn.push(f);
        }
        // containers (`loot`)
        self.add_containers(&placed, off);
        // interiors, inside points and doors
        let mut inner = interiors(&data, res, &placed);
        for v in &mut inner {
            v.placement += off;
        }
        self.inner.extend(inner);
        let new_doors = doors(&data, res, &placed);
        for (i, pl) in placed.iter().enumerate() {
            let at = [pl.position[0], pl.world_y.unwrap_or(pl.position[1]), pl.position[2]];
            self.placed_at.push(at);
            let inside = (furn_off..self.furn.len())
                .find(|&f| self.furn[f].placement == off + i && building_at(&self.inner, self.furniture_approach[f]) == Some(off + i))
                .map(|f| self.furniture_approach[f]);
            self.inside_point.push(inside);
        }
        for mut door in new_doors {
            door.placement += off;
            let (locked, level) = placed[door.placement - off].locks.get(door.index).copied().unwrap_or((false, 0));
            let inside = match self.inside_point[door.placement] {
                Some(q) => {
                    let (dx, dz) = (q[0] - door.position[0], q[2] - door.position[2]);
                    let l = dx.hypot(dz).max(1e-3);
                    let p = [door.position[0] + dx / l * 25.0, door.position[1], door.position[2] + dz / l * 25.0];
                    near(&self.nav, p, 10.0)
                }
                None => door.position,
            };
            let state = DoorState::new(&data, &placed[door.placement - off], door, locked, level, inside);
            self.door_states.push(state);
        }
        // the zone's town, and its bars
        let mid = [c(zx), 0.0, c(zy)];
        let town = current_town(&self.towns, mid, None);
        for (i, pl) in placed.iter().enumerate() {
            let bar = pl.residents.as_deref().and_then(|r| data.get(r)).and_then(|r| r.ints.get("building designation")) == Some(&BD_BAR);
            if bar && town.is_none_or(|t| self.towns[t].contains(self.placed_at[off + i], 1.0)) {
                self.bars.push(off + i);
            }
        }
        self.placed.extend(placed.iter().cloned());
        let first = self.folk.len();
        // the town as it is now (`world`: an override town once the world states call for one)
        let now = town.map(|t| self.town_record(t));
        let override_rec = town
            .zip(now.as_ref())
            .filter(|(t, n)| **n != self.towns[*t].record)
            .and_then(|(_, n)| data.get(n))
            .filter(|r| r.bools.get("residents override").copied().unwrap_or(false));
        // residents at home (an override's: those of the building's designation, none if it has
        // none)
        let quiet = self.quiet_zones;
        for (pi, pl) in placed.iter().enumerate().filter(|_| !quiet) {
            let Some(t) = pl.squad.as_deref().and_then(|s| data.get(s)) else { continue };
            let at = [pl.position[0], pl.world_y.unwrap_or(pl.position[1]), pl.position[2]];
            let in_town = town.is_some_and(|tw| self.towns[tw].contains(at, 1.0));
            let t = match override_rec.filter(|_| in_town) {
                Some(o) => {
                    let d = t.ints.get("building designation").copied().unwrap_or(0);
                    let Some(sq) = o.refs("residents").iter().find(|(_, v)| v.get(1).copied().unwrap_or(0) == d).and_then(|(s, _)| data.get(s)) else { continue };
                    sq
                }
                None => t,
            };
            self.spawn_resident_squad(t, at, Some(off + pi));
        }
        // the town's bar squads at its bar (the home town's are in already)
        if let Some(t) = town.filter(|_| !self.quiet_zones)
            && let Some(&bar) = self.bars.iter().rev().find(|&&b| b >= off)
            && let Some(town_rec) = data.get(now.as_deref().unwrap_or(&self.towns[t].record))
        {
            for (sid, v) in town_rec.refs("bar squads") {
                let chance = if v[1] > 0 { v[1] as f32 } else { 100.0 };
                for _ in 0..v[0].max(1) {
                    if self.rng.uniform() * 100.0 < chance
                        && let Some(sq) = data.get(sid)
                    {
                        let at = self.placed_at[bar];
                        self.spawn_resident_squad(sq, at, Some(bar));
                    }
                }
            }
        }
        let added = self.folk.len() - first;
        eprintln!(
            "zone {zx}.{zy}: {} placements, {} people{}",
            placed.len(),
            added,
            town.map_or(String::new(), |t| format!(" (town {})", self.towns[t].name))
        );
        added
    }

    /// A squad of a template living at placement `home`: its leaders on the package's leader
    /// goals, the rest on its squad goals, in its faction's formation, kitted from its
    /// characters.
    pub(crate) fn spawn_resident_squad(&mut self, t: &kenshi_data::GameData, at: [f32; 3], home: Option<usize>) -> Vec<usize> {
        let data = self.data.clone();
        let Some(package) = running_package(&data, t) else {
            return Vec::new();
        };
        // who it is made of (F038271), the first leading
        let rng = &mut self.rng;
        let roster = template_roster(t, 1.0, &mut || rng.uniform());
        let (leaders, squad) = (i32::from(!roster.is_empty()), roster.len().saturating_sub(1) as i32);
        // F038271: the template's "faction", else its owner's (the town it lives in)
        let faction = faction_of(t).or_else(|| kenshi_world::town::current_town(&self.towns, at, None).and_then(|k| self.towns[k].faction.clone()));
        let pid = self.bar_states.len();
        self.squad_template.insert(pid, t.string_id.clone());
        self.note_squad_hunger(pid, t);
        let mut members = vec![];
        for _ in 0..leaders {
            members.push(self.add_person(at, package_goals(&data, package, Role::Leader), None, Some(pid), faction.clone(), 5));
        }
        let leader = members.first().copied();
        for _ in 0..squad {
            members.push(self.add_person(at, package_goals(&data, package, Role::Squad), leader, Some(pid), faction.clone(), 6));
        }
        let sig = *package.ints.get("signal func").unwrap_or(&0);
        self.bar_states.push([17, 26, 27].contains(&sig).then(BarState::default));
        // their kit: each one's character
        let chars = roster;
        for (k, &m) in members.iter().enumerate() {
            self.folk[m].home = home;
            if let Some(c) = chars.get(k % chars.len().max(1)).and_then(|c| data.get(c)) {
                let rng = &mut self.rng;
                self.folk[m].inventory = kenshi_char::inventory::Inventory::from_template(&data, c, &mut || rng.next() * 100.0);
                self.folk[m].character = Some(c.string_id.clone());
                self.apply_stats(m);
            }
        }
        for (k, &m) in members.iter().enumerate() {
            self.assign_dialogue(m, Some(&t.string_id), k == 0);
        }
        let (captures, duties) = template_duties(&data, t);
        for &m in &members {
            self.folk[m].package = Some(package.string_id.clone());
            if self.folk[m].race.is_none() {
                self.folk[m].captures |= captures;
                self.folk[m].duties |= duties;
            }
        }
        if let Some(&leader) = members.first() {
            self.open_shop(leader, &t.string_id);
        }
        let kind = formation_of(&data, t);
        let ms: Vec<Member> = members
            .iter()
            .enumerate()
            .map(|(k, &i)| Member { role: if k == 0 { 2 } else { 0 }, radius: self.folk[i].mover.radius * 10.0, animal: false })
            .collect();
        if !ms.is_empty() {
            let rng = &mut self.rng;
            let offs = layout(kind, &ms, ms[0].radius, &mut || rng.next());
            for (k, &i) in members.iter().enumerate() {
                self.folk[i].offset = offs[k];
            }
        }
        members
    }

    /// @fn F048832
    ///
    /// Every game hour: a resident squad whose template "regenerates" and has lost at least 0.6
    /// of its people (TownPopulationManager's check, F048832) gets them all back, members of its
    /// template's make-up (`template_roster`), by its leader. (assumption) Hourly and out of the
    /// player's sight (no character of theirs within a zone and a half), a day apart: the game
    /// marks the squad (+0x118) and remakes it when it is next loaded; the town's population cap
    /// is not kept.
    pub fn regenerate_tick(&mut self) {
        let now = self.hours_gone();
        if self.regen_check > now {
            return;
        }
        self.regen_check = now + 1.0;
        let data = self.data.clone();
        let players: Vec<[f32; 3]> = self.folk.iter().filter(|p| p.player && !p.dead).map(|p| p.mover.position_units()).collect();
        let squads: Vec<(usize, String)> = self.squad_template.iter().map(|(k, v)| (*k, v.clone())).collect();
        for (pid, sid) in squads {
            let Some(t) = data.get(&sid) else { continue };
            if !t.bools.get("regenerates").copied().unwrap_or(false) || self.regenerated.get(&pid).is_some_and(|&at| now < at + 24.0) {
                continue;
            }
            let members: Vec<usize> = self.folk.iter().filter(|p| p.platoon == Some(pid) && !p.player).map(|p| p.id).collect();
            let alive: Vec<usize> = members.iter().copied().filter(|&m| !self.folk[m].dead && !self.folk[m].away && self.folk[m].prison.is_none()).collect();
            let Some(&leader) = alive.iter().find(|&&m| self.folk[m].role == 5).or(alive.first()) else { continue };
            let missing = members.len() - alive.len();
            if missing == 0 || (missing as f64) < members.len() as f64 * 0.6 {
                continue;
            }
            let at = self.folk[leader].mover.position_units();
            if players.iter().any(|q| (q[0] - at[0]).hypot(q[2] - at[2]) < kenshi_world::ZONE_SIZE * 1.5) {
                continue;
            }
            let Some(package) = running_package(&data, t) else { continue };
            let rng = &mut self.rng;
            let roster = template_roster(t, 1.0, &mut || rng.uniform());
            for k in 0..missing {
                let Some(c) = roster.get(1 + k % roster.len().saturating_sub(1).max(1)).or(roster.first()).and_then(|c| data.get(c)) else { continue };
                let faction = faction_of(t).or_else(|| self.folk[leader].faction.clone());
                let m = self.add_person(at, package_goals(&data, package, Role::Squad), Some(leader), Some(pid), faction, 6);
                self.folk[m].home = self.folk[leader].home;
                let rng = &mut self.rng;
                self.folk[m].inventory = kenshi_char::inventory::Inventory::from_template(&data, c, &mut || rng.next() * 100.0);
                self.folk[m].character = Some(c.string_id.clone());
                self.apply_stats(m);
                self.assign_dialogue(m, Some(&sid), false);
                self.folk[m].package = Some(package.string_id.clone());
                if std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
                    eprintln!("regenerate: squad {pid} ({}) gets {} back", t.name, c.name);
                }
            }
            self.regenerated.insert(pid, now);
        }
    }

    /// Spawn a squad of a SQUAD_TEMPLATE at `at` (units): its leaders and members (the
    /// template's "leader" and "squad" counts) on its running package ([`running_package`]), in
    /// its faction's formation. The members' bodies
    /// are the zone's race and the harness kit.
    pub fn spawn_template(&mut self, template: &str, at: [f32; 3]) -> Option<Vec<usize>> {
        let data = self.data.clone();
        let t = data.get(template)?;
        // a squad without packages (a prisoners' template) has no goals
        let package = running_package(&data, t);
        let goals_of = |r: Role| package.map(|p| package_goals(&data, p, r)).unwrap_or_default();
        let leaders: i32 = t.refs("leader").iter().map(|(_, v)| v[0].max(1)).sum();
        let squad: i32 = t.refs("squad").iter().map(|(_, v)| v[0]).sum();
        let (leaders, squad) = if leaders == 0 && squad > 0 {
            (1, squad - 1)
        } else {
            (leaders, squad)
        };
        // its animals (the "animals" refs, fcs.def: min and max count, and age, 100 adult; the
        // age randomised by up to "animal age random" either way, (assumption) uniformly)
        let mut beasts: Vec<(String, f32)> = Vec::new();
        let age_random = t.floats.get("animal age random").copied().unwrap_or(0.1);
        for (sid, v) in t.refs("animals").iter().chain(t.refs("animals2").iter()) {
            let lo = v[0].max(1);
            let hi = v[1].max(lo);
            let n = lo + ((self.rng.next() * (hi - lo + 1) as f32) as i32).min(hi - lo);
            for _ in 0..n {
                let age = (v[2] as f32 * 0.01 + (self.rng.next() * 2.0 - 1.0) * age_random).clamp(0.0, 1.0);
                beasts.push((sid.clone(), age));
            }
        }
        if leaders + squad == 0 && beasts.is_empty() {
            return None;
        }
        let faction = t.refs("faction").first().map(|(f, _)| f.clone());
        let pid = self.bar_states.len();
        self.note_squad_hunger(pid, t);
        let mut members = vec![];
        for _ in 0..leaders {
            let id = self.add_person(
                at,
                goals_of(Role::Leader),
                None,
                Some(pid),
                faction.clone(),
                5,
            );
            members.push(id);
        }
        let leader = members.first().copied();
        for _ in 0..squad {
            let id = self.add_person(
                at,
                goals_of(Role::Squad),
                leader,
                Some(pid),
                faction.clone(),
                6,
            );
            members.push(id);
        }
        let package_id = package.map(|p| p.string_id.clone());
        // the animals: an all-animal squad's first leads
        for (sid, age) in &beasts {
            let leader = members.first().copied();
            let role = if leader.is_none() { Role::Leader } else { Role::Squad };
            let id = self.add_person(at, goals_of(role), leader, Some(pid), faction.clone(), 6);
            self.folk[id].age = *age;
            self.make_animal(id, sid);
            members.push(id);
        }
        // the people's kit and stats, from the template's characters in turn; their dialogue
        let mut chars = Vec::new();
        for cat in ["leader", "squad"] {
            for (c, n) in t.refs(cat) {
                for _ in 0..n[0].max(1) {
                    chars.push(c.clone());
                }
            }
        }
        for (k, &m) in members.iter().enumerate() {
            if self.folk[m].race.is_some() {
                continue;
            }
            if let Some(c) = chars.get(k % chars.len().max(1)).and_then(|c| data.get(c)) {
                let rng = &mut self.rng;
                self.folk[m].inventory = kenshi_char::inventory::Inventory::from_template(&data, c, &mut || rng.next() * 100.0);
                self.folk[m].character = Some(c.string_id.clone());
                self.apply_stats(m);
            }
            self.assign_dialogue(m, Some(template), k == 0);
        }
        // slavers: any of the squad's packages capturing makes its people capture
        let captures = t.refs("AI packages").iter().filter_map(|(p, _)| data.get(p)).any(|p| {
            [Role::Leader, Role::Squad]
                .into_iter()
                .any(|r| package_goals(&data, p, r).iter().any(|g| g.task == CAPTURE_SLAVES || g.task == CARRY_SLAVES || g.task == KIDNAPPER))
        });
        // the other goals run as routines, from any of the squad's packages (an "addon-" one too)
        let mut goals: Vec<u16> = Vec::new();
        for p in t.refs("AI packages").iter().filter_map(|(p, _)| data.get(p)) {
            for r in [Role::Leader, Role::Squad] {
                goals.extend(package_goals(&data, p, r).iter().map(|g| g.task));
            }
        }
        let duties = duties_of(goals);
        for &m in &members {
            self.folk[m].package = package_id.clone();
            if captures && self.folk[m].race.is_none() {
                self.folk[m].captures = true;
            }
            if self.folk[m].race.is_none() {
                self.folk[m].duties = duties;
            }
        }
        let sig = package.and_then(|p| p.ints.get("signal func").copied()).unwrap_or(0);
        self.bar_states
            .push([17, 26, 27].contains(&sig).then(BarState::default));
        // the formation (F028591)
        let kind = formation_of(&data, t);
        let ms: Vec<Member> = members
            .iter()
            .enumerate()
            .map(|(k, &i)| Member {
                role: if k == 0 { 2 } else { 0 },
                radius: self.folk[i].mover.radius * 10.0,
                animal: false,
            })
            .collect();
        let rng = &mut self.rng;
        let off = layout(kind, &ms, ms[0].radius, &mut || rng.next());
        for (k, &i) in members.iter().enumerate() {
            self.folk[i].offset = off[k];
        }
        Some(members)
    }

    /// @fn F038271
    ///
    /// createRandomSquad's prisoners, for the squads loaded with them: the free cages about the
    /// leader (F051407's search; (assumption) within PRISON_SEARCH units), then each "prisoners"
    /// template's people made beside it (in their own squad, of the template's faction), each
    /// put in one of those cages at random, the cage taken; one with no cage left is not made
    /// (Kenshi destroys it). Prisoners for good: no sentence ends (the "imprisoned"
    /// messages not shown, as for slaves made at the start).
    pub fn spawn_prisoners(&mut self) {
        const PRISON_SEARCH: f32 = 1000.0;
        let pending = std::mem::take(&mut self.prisoner_squads);
        let data = self.data.clone();
        let said = self.messages.len();
        for (first, template) in pending {
            let Some(t) = data.get(&template) else { continue };
            let Some(leader) = self.folk.get(first) else { continue };
            let at = leader.mover.position_units();
            let jailers = leader.faction.clone().or_else(|| faction_of(t));
            let Some(jailers) = jailers else { continue };
            let taken: Vec<usize> = self.folk.iter().filter_map(|p| p.prison.as_ref().map(|q| q.cage)).collect();
            let mut cages: Vec<usize> = (0..self.furn.len())
                .filter(|&k| self.furn[k].function == kenshi_world::furniture::BF_CAGE && !taken.contains(&k))
                .filter(|&k| (self.furn[k].position[0] - at[0]).hypot(self.furn[k].position[2] - at[2]) < PRISON_SEARCH)
                .collect();
            for (sid, _) in t.refs("prisoners") {
                let Some(members) = self.spawn_template(sid, at) else { continue };
                for m in members {
                    if cages.is_empty() {
                        if std::env::var("KENSHI_PRISON_DEBUG").is_ok() {
                            let near = (0..self.furn.len()).filter(|&k| self.furn[k].function == kenshi_world::furniture::BF_CAGE).map(|k| (self.furn[k].position[0] - at[0]).hypot(self.furn[k].position[2] - at[2]) as i32).min();
                            eprintln!("prisoner {m} of {template}: no free cage within {PRISON_SEARCH} units (nearest cage {near:?})");
                        }
                        // no cage: the prisoner goes (left dead and away, out of the world)
                        let p = &mut self.folk[m];
                        p.dead = true;
                        p.away = true;
                        continue;
                    }
                    let k = ((self.rng.next() * cages.len() as f32) as usize).min(cages.len() - 1);
                    let cage = cages.remove(k);
                    self.imprison(m, cage, &jailers, false);
                    if let Some(q) = self.folk[m].prison.as_mut() {
                        q.until = f64::MAX;
                    }
                    if std::env::var("KENSHI_PRISON_DEBUG").is_ok() {
                        eprintln!("prisoner {m} ({}) of {template} in cage {cage}", self.folk[m].name);
                    }
                }
            }
        }
        self.messages.truncate(said);
    }

    /// @fn F048140
    ///
    /// The population's tick, every 2 s: the zones round the player's characters (their loaded
    /// tiles) hold squads up to the player's share (F048109: the multiplier times each player
    /// squad's zone's biome "homeless spawn amount"); below it a zone is picked by its shortfall
    /// (mult x amount + 1 - its squads; halved with a town; doubled plus one when empty) and
    /// a homeless squad spawned in it (F048137).
    fn population_tick(&mut self, dt: f32) {
        let Some(pop) = self.population.as_mut() else {
            return;
        };
        pop.timer += dt;
        if pop.timer < 2.0 {
            return;
        }
        pop.timer = 0.0;
        // BiomeManager's nests first (F048140 -> F048131), then those near built
        self.spawn_nests();
        self.nests_tick();
        let Some(pop) = self.population.as_mut() else {
            return;
        };
        // standing squads per zone (their leader's zone)
        let mut count: HashMap<(i32, i32), f32> = HashMap::new();
        pop.squads
            .retain(|(m, _, _)| m.iter().any(|&i| !self.folk[i].down));
        for (m, _, _) in &pop.squads {
            if let Some(&l) = m.iter().find(|&&i| !self.folk[i].down) {
                let q = self.folk[l].mover.position_units();
                *count.entry(zone_at(q[0], q[2])).or_default() += 1.0;
            }
        }
        let players: Vec<(i32, i32)> = self
            .folk
            .iter()
            .filter(|p| p.player)
            .map(|p| {
                let q = p.mover.position_units();
                zone_at(q[0], q[2])
            })
            .collect();
        if players.is_empty() {
            return;
        }
        let amount = |pop: &Population, z: (i32, i32)| {
            biome_of_zone(&pop.map, &pop.biomes, z.0, z.1)
                .map_or(0.0, |b| pop.biomes[b].homeless_spawn_amount)
        };
        // the player's share: one squad (the player's platoon), its zone's biome
        let cap = pop.mult * amount(pop, players[0]);
        let has_town = |z: (i32, i32)| {
            self.towns
                .iter()
                .any(|t| zone_at(t.position[0], t.position[2]) == z)
        };
        let mut active: Vec<(i32, i32)> = vec![];
        for &(x, y) in &players {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let z = (x + dx, y + dy);
                    if self.nav.tiles.contains(&z) && !active.contains(&z) {
                        active.push(z);
                    }
                }
            }
        }
        let here: f32 = active
            .iter()
            .map(|&z| {
                let c = count.get(&z).copied().unwrap_or(0.0);
                if has_town(z) { c.clamp(0.0, 2.0) } else { c }
            })
            .sum();
        if std::env::var("POP_DEBUG").is_ok() {
            eprintln!(
                "t {:.1} population: here {here} cap {cap} active {active:?} players {players:?}",
                self.t
            );
        }
        if here >= cap {
            return;
        }
        // the zone, weighted by its shortfall; not a zone a player character stands in
        let weights: Vec<((i32, i32), f32)> = active
            .iter()
            .filter(|z| !players.contains(z))
            .map(|&z| {
                let c = count.get(&z).copied().unwrap_or(0.0);
                let mut w = (pop.mult * amount(pop, z) + 1.0 - c).max(0.0);
                if has_town(z) {
                    w *= 0.5;
                }
                if c == 0.0 {
                    w = (w + 1.0) * 2.0;
                }
                (z, w)
            })
            .filter(|(_, w)| *w > 0.0)
            .collect();
        let total: f32 = weights.iter().map(|(_, w)| w).sum();
        if total <= 0.0 {
            return;
        }
        let mut r = self.rng.next() * total;
        let zone = weights
            .iter()
            .find(|(_, w)| {
                r -= w;
                r < 0.0
            })
            .map_or(weights[0].0, |(z, _)| *z);
        self.spawn_homeless(zone, count.get(&zone).copied().unwrap_or(0.0), active.len());
    }

    /// @fn F048137
    ///
    /// A homeless squad for a zone below its biome's amount: from the biome's homeless list, each
    /// weighed down by the squads of that template already out (weight - existing x ratio, the
    /// ratio being the list's total weight over the active zones times mult x amount); placed at
    /// a random point in the zone (F048102 / F048115: (harness) the middle 80%, on the navmesh,
    /// 1500 units clear of the player's characters and outside towns). An entry with world
    /// states spawns only while they stand (F050367).
    fn spawn_homeless(&mut self, zone: (i32, i32), squads_here: f32, active: usize) {
        let Some(pop) = self.population.as_ref() else {
            return;
        };
        let Some(b) = biome_of_zone(&pop.map, &pop.biomes, zone.0, zone.1) else {
            return;
        };
        let biome = &pop.biomes[b];
        let amount = pop.mult * biome.homeless_spawn_amount;
        if squads_here >= amount {
            return;
        }
        // F048135: the zone's town (F048114) or else the nearest (TownManager::nearest, any type
        // but 8); its "roaming squads" join the biome's when it is within 11566 units (squared
        // 133772080) of the zone and not the player's. (Town's +0x270 is 0, so the biome's list
        // is always in.) The weights' ratio counts the biome's list only for a zone without a
        // town of its own.
        let (cx, cz) = zone_centre(zone.0, zone.1);
        let d2 = |t: &kenshi_world::town::Town| (t.position[0] - cx).powi(2) + (t.position[2] - cz).powi(2);
        let own = self.towns.iter().position(|t| t.kind != 8 && zone_at(t.position[0], t.position[2]) == zone);
        let town = own.or_else(|| (0..self.towns.len()).filter(|&t| self.towns[t].kind != 8).min_by(|&a, &b| d2(&self.towns[a]).total_cmp(&d2(&self.towns[b]))));
        let player_faction = self.folk.iter().find(|p| p.player).and_then(|p| p.faction.clone());
        let near = town.filter(|&t| d2(&self.towns[t]) < 133772080.0 && (player_faction.is_none() || self.towns[t].faction != player_faction));
        // (the town's list: its record now, the ref's weight and its least roaming budget left)
        let roam: Vec<(String, i32, i32)> = near
            .and_then(|t| self.data.get(&self.world.towns.get(t).map_or(self.towns[t].record.clone(), |w| w.record.clone())))
            .map(|r| r.refs("roaming squads").iter().filter(|(sq, v)| v[0] > 0 && self.data.get(sq).is_some_and(|t| t.item_type() == Some(kenshi_data::ItemType::SquadTemplate))).map(|(sq, v)| (sq.clone(), v[0], v[1])).collect())
            .unwrap_or_default();
        if biome.homeless.is_empty() && roam.is_empty() {
            return;
        }
        let mut total: i32 = if own.is_none() { biome.homeless.iter().map(|s| s.weight).sum() } else { 0 };
        total += roam.iter().map(|r| r.1).sum::<i32>();
        let ratio = total as f32 / (active.max(1) as f32 * amount.max(1e-3));
        let mut candidates: Vec<(String, f32, Option<usize>)> = biome
            .homeless
            .iter()
            .filter(|s| {
                self.data
                    .get(&s.template)
                    .is_some_and(|t| self.template_states_hold(t))
            })
            .map(|s| {
                let out = pop
                    .squads
                    .iter()
                    .filter(|(_, _, t)| *t == s.template)
                    .count();
                let w = (s.weight as f32 - out as f32 * ratio).max(0.0);
                (s.template.clone(), w, None)
            })
            .filter(|(_, w, _)| *w > 0.0)
            .collect();
        if let Some(t) = near {
            // TownPopulationManager list 2 (F048143): its budget the town faction's "roaming
            // population", less the squads out from the town; an entry needs its second value
            // of it left (F048134)
            let budget = self.towns[t].faction.as_ref().and_then(|f| self.data.get(f)).and_then(|f| f.ints.get("roaming population").copied()).unwrap_or(0) as i32;
            let from_town: Vec<&String> = pop.squads.iter().filter(|(m, _, _)| m.first().is_some_and(|l| pop.homes.get(l) == Some(&t))).map(|(_, _, tp)| tp).collect();
            let left = budget - from_town.len() as i32;
            for (sq, w, least) in &roam {
                if *least > left {
                    continue;
                }
                let out = from_town.iter().filter(|tp| **tp == sq).count();
                let w = (*w as f32 - out as f32 * ratio).max(0.0);
                if w > 0.0 && self.data.get(sq).is_some_and(|d| self.template_states_hold(d)) {
                    candidates.push((sq.clone(), w, Some(t)));
                }
            }
        }
        let sum: f32 = candidates.iter().map(|(_, w, _)| w).sum();
        if sum <= 0.0 {
            return;
        }
        let mut r = self.rng.next() * sum;
        let (template, home) = candidates
            .iter()
            .find(|(_, w, _)| {
                r -= w;
                r < 0.0
            })
            .map_or((candidates[0].0.clone(), candidates[0].2), |(t, _, h)| (t.clone(), *h));
        // the place
        let half = kenshi_world::ZONE_SIZE * 0.4;
        let players: Vec<[f32; 3]> = self
            .folk
            .iter()
            .filter(|p| p.player)
            .map(|p| p.mover.position_units())
            .collect();
        let mut at = None;
        for _ in 0..10 {
            let x = cx + (self.rng.next() * 2.0 - 1.0) * half;
            let z = cz + (self.rng.next() * 2.0 - 1.0) * half;
            let y = self.terrain.height_at(x, z).unwrap_or(0.0);
            let Some((_, q)) = self.nav.closest_point([x, y, z].map(|v| v * METRES), 60.0) else {
                continue;
            };
            let q = q.map(|v| v / METRES);
            let clear = players
                .iter()
                .all(|p| (p[0] - q[0]).hypot(p[2] - q[2]) > 1500.0)
                && self.towns.iter().all(|t| !t.contains(q, 1.0));
            if clear {
                at = Some(q);
                break;
            }
        }
        let Some(at) = at else { return };
        if let Some(members) = self.spawn_template(&template, at) {
            let name = self
                .data
                .get(&template)
                .map_or("?", |t| t.name.as_str())
                .to_string();
            eprintln!(
                "t {:.1} homeless squad {name} ({} people) spawned in zone {}.{} at {:?}",
                self.t,
                members.len(),
                zone.0,
                zone.1,
                at.map(|v| v.round())
            );
            if let Some(pop) = self.population.as_mut() {
                if let (Some(t), Some(&l)) = (home, members.first()) {
                    pop.homes.insert(l, t);
                }
                pop.squads.push((members, zone, template));
            }
        }
    }

    /// @fn F050367
    ///
    /// Whether a squad template's "world state" refs all stand: each state (its record found)
    /// holds when the ref's first value is 1, and does not otherwise.
    pub fn template_states_hold(&self, t: &kenshi_data::GameData) -> bool {
        t.refs("world state").iter().all(|(st, v)| self.data.get(st).is_none() || self.world_state(st) == (v.first().copied().unwrap_or(0) == 1))
    }

    /// Drawing and sheathing: the weapon comes out (drawWeapon F039274, at once: Kenshi has no
    /// draw animation) when a fight starts or an attack task (Attacking, Attacking enemies,
    /// Attacking neutrals) is taken up, so it is in hand on the way to the target; with neither
    /// it goes back (the goal "Sheathing weapon", task 7, done once no weapon is in hand).
    /// (assumption) The sheathe task's turn comes a second after the last fight or attack task
    /// (the AI's thinking once a second); knocked out, the weapon is put away with the body.
    fn weapon_tick(&mut self, dt: f32) {
        for id in 0..self.folk.len() {
            let p = &self.folk[id];
            let f = &self.arena.f[id];
            let attacking = p.ai.current.iter().chain(p.ai.queue.iter()).chain(p.ai.orders.iter()).any(|t| matches!(t.task, ATTACKING | ATTACKING_ENEMIES | ATTACKING_NEUTRALS));
            let fighting = (f.active || attacking) && !p.down && !p.dead;
            let p = &mut self.folk[id];
            if fighting {
                p.weapon_out = true;
                p.weapon_idle = 0.0;
            } else if p.down || p.dead {
                p.weapon_out = false;
            } else if p.weapon_out {
                p.weapon_idle += dt;
                if p.weapon_idle >= WEAPON_SHEATHE_AFTER {
                    p.weapon_out = false;
                }
            }
        }
    }

    /// (harness: Kenshi pages the world round the camera and the player's characters) the
    /// navmesh tiles round every player character.
    pub fn stream(&mut self) {
        let mut want = vec![];
        for p in self.folk.iter().filter(|p| p.player) {
            let (tx, ty) = tile_of(p.mover.position);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    want.push((tx + dx, ty + dy));
                }
            }
        }
        for (x, y) in want {
            if let Err(e) = self.nav.ensure(&self.install, x, y) {
                eprintln!("navmesh tile {x}.{y}: {e:?}");
            }
        }
    }
}

/// @fn F038271
///
/// RootObjectFactory::createRandomSquad's people of a SQUAD_TEMPLATE, in the order it makes
/// them (the first leads): its first "leader"; "num random chars" .. "num random chars max"
/// (randInt) picked from its "choosefrom list" (by the refs' first values); then each "squad"
/// and "squad2" ref's count, its first value or, when its second is above 0 and not 100,
/// randInt(first, second), times the squad size multiplier (`mult`; 1 for "dont multiply"), at
/// least one when any. Its animals are `spawn_template`'s, its slaves `slavery`'s
/// (`spawn_owned_slaves`) and its prisoners `spawn_prisoners`'.
pub fn template_roster(t: &kenshi_data::GameData, mult: f32, rand: &mut dyn FnMut() -> f32) -> Vec<String> {
    let rand_int = |rand: &mut dyn FnMut() -> f32, a: i32, b: i32| if b <= a { a } else { (a + (rand() * (b - a + 1) as f32) as i32).min(b) };
    let mut out = Vec::new();
    if let Some((c, _)) = t.refs("leader").first() {
        out.push(c.clone());
    }
    let (lo, hi) = (t.ints.get("num random chars").copied().unwrap_or(0) as i32, t.ints.get("num random chars max").copied().unwrap_or(0) as i32);
    let n = if lo < hi { rand_int(rand, lo, hi) } else { lo };
    let pool = t.refs("choosefrom list");
    let total: f32 = pool.iter().map(|(_, v)| v[0].max(0) as f32).sum();
    for _ in 0..n.max(0) {
        if pool.is_empty() {
            break;
        }
        let pick = if total > 0.0 {
            let mut r = rand() * total;
            pool.iter().find(|(_, v)| {
                r -= v[0].max(0) as f32;
                r < 0.0
            })
        } else {
            pool.get(((rand() * pool.len() as f32) as usize).min(pool.len() - 1))
        };
        if let Some((c, _)) = pick.or(pool.last()) {
            out.push(c.clone());
        }
    }
    let mult = if t.bools.get("dont multiply").copied().unwrap_or(false) { 1.0 } else { mult };
    for cat in ["squad", "squad2"] {
        for (c, v) in t.refs(cat) {
            let mut count = v[0];
            if 0 < v[1] && v[1] != 100 {
                count = rand_int(rand, v[0], v[1]);
            }
            let count = if count < 1 { 0 } else { ((count as f32 * mult) as i32).max(1) };
            for _ in 0..count {
                out.push(c.clone());
            }
        }
    }
    out
}

/// (assumption) Seconds without a fight or an attack task before the weapon is sheathed.
pub const WEAPON_SHEATHE_AFTER: f32 = 1.0;

/// The water's surface (F050961) and where a swimmer's body floats (F040642: 90).
pub const WATER_LEVEL: f32 = 100.0;
pub const SWIM_HEIGHT: f32 = 90.0;

/// @fn F039118
///
/// Character::waterState from the ground under it (`ground`, +0x698), its size (vtable 0x390)
/// and whether its race swims (+0x79): 0 dry; with the ground 2 or more under the water,
/// swimming (3) when its size x 15 is too, wading (2) at size x 6, else shallow (1) for a
/// swimmer.
pub fn water_state(ground: f32, size: f32, swims: bool) -> i32 {
    if ground + 2.0 > WATER_LEVEL {
        return 0;
    }
    if size * 15.0 + ground <= WATER_LEVEL {
        3
    } else if size * 6.0 + ground <= WATER_LEVEL {
        2
    } else if swims {
        1
    } else {
        0
    }
}

/// A package's goals as a person's AI jobs (by classification): the wandering trader's trips as
/// patrolling (`Person::caravan`), a goal targeting the leader on `leader`, one targeting the
/// squad's mission on `mission` (a contract's hirer). "Following" (44) a mission target is
/// Task_Follow (factory case 14), whose update is Task_FollowLeader's (F031009): it runs as
/// following the leader; (assumption) Bodyguard (45), a generic task followed by Staying close
/// (31, Task_Follow too), likewise; Task_Follow's own distance (random 20 .. 40) not kept.
pub(crate) fn jobs_of(goals: Vec<PackageGoal>, leader: Option<usize>, mission: Option<usize>) -> [Vec<Task<Target>>; 4] {
    let mut jobs: [Vec<Task<Target>>; 4] = Default::default();
    for mut g in goals {
        if g.task == WANDERING_TRADER {
            g.task = PATROLLING;
        }
        let class = (g.classification as usize).min(3);
        let target = match (g.targeting, leader, mission) {
            (2, Some(l), _) => Target::Person(l),
            (3, _, Some(m)) => Target::Person(m),
            _ => Target::None,
        };
        if g.targeting == 3 && mission.is_some() && (g.task == 44 || g.task == 45) {
            g.task = FOLLOWING_LEADER;
        }
        jobs[class].push(Task {
            task: g.task,
            classification: g.classification,
            target,
            target2: Target::None,
            priority: g.priority,
            pos: [0.0; 3],
            hours: g.hours,
        });
    }
    jobs
}

/// @fn F040052
///
/// An ANIMAL_CHARACTER's inventory: its backpack slot and an "inventory w" x "inventory h" grid,
/// the "backpack" it wears put on (`worn`).
pub fn animal_inventory(data: &kenshi_data::GameDataContainer, a: &kenshi_data::GameData, worn: bool) -> kenshi_char::inventory::Inventory {
    let int = |k: &str| a.ints.get(k).copied().unwrap_or(0).max(1) as u32;
    let mut inv = kenshi_char::inventory::Inventory::animal(int("inventory w"), int("inventory h"));
    if worn && let Some(b) = a.refs("backpack").first().and_then(|(s, _)| data.get(s)) {
        inv.add(kenshi_char::inventory::Item::from_record(b, 1), true);
    }
    inv
}
