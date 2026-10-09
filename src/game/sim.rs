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
/// AI_TASK "Shopping" (civilians', farmers', bar-goers' packages).
pub const SHOPPING: u16 = 118;
/// (assumption) Game hours before a shopper goes shopping again (the score's check, 0x1400265cb,
/// is not decompiled).
const SHOP_AGAIN_HOURS: f64 = 4.0;
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
pub const PLAYER_FACTION: &str = "204-gamedata.base";
pub const RUNNING: u16 = 67;
/// (harness) How near an attacker walks before the fight starts (units).
pub const ENGAGE_RANGE: f32 = 60.0;
pub const ATTACKING_ENEMIES: u16 = 16;
/// "attack enemies and neutrals" (AI_TASK enum 21): anyone not an ally is fought.
pub const ATTACKING_NEUTRALS: u16 = 21;
/// "Wandering trader" (enum 39): travelling town to town.
pub const WANDERING_TRADER: u16 = 39;
/// "Capture Slaves" (enum 167) and "Carry wounded slaves" (168).
pub const CAPTURE_SLAVES: u16 = 167;
pub const CARRY_SLAVES: u16 = 168;
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
}

/// A door and its lock (the building state record's "locked{n}", "lock lvl{n}").
#[derive(Debug, Clone)]
pub struct DoorState {
    pub door: Door,
    pub locked: bool,
    pub level: i32,
    /// The point just inside the door the character locks it from (units).
    pub inside: [f32; 3],
}

/// A squad's Package_HangAroundBar state: its bar and each member's chair.
#[derive(Debug, Default)]
pub struct BarState {
    pub bar: Option<usize>,
    pub held: std::collections::HashMap<usize, usize>,
}

pub struct Lcg(pub u64);
impl Lcg {
    fn next(&mut self) -> f32 {
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
    /// Its goals take the knocked out captive (tasks 167 / 168).
    pub captures: bool,
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
    /// Shopping: when it last came back from a shop, and when the visit ends (game hours).
    pub shopped_at: f64,
    pub shop_until: Option<f64>,
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
    /// When it last came back from shopping (game hours).
    shopped_at: f64,
    /// Who is knocked out.
    downs: &'a [bool],
}

impl View<'_> {
    fn target_pos(&self, t: &Target, pos: [f32; 3]) -> Option<[f32; 3]> {
        match t {
            Target::Node(n) => Some(self.approach[*n]),
            Target::Person(i) => Some(self.positions[*i]),
            Target::Furniture(f) => Some(self.furniture_approach[*f]),
            Target::Door(d) => Some(self.doors[*d].inside),
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
            86 => true,
            // the shop's target check (0x140998fa0): (stand-in) a node target holds
            85 => matches!(t, Target::Node(_)),
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
            // F038552: attacking enemies while standing and not sneaking
            Score::Method(52) => 1.0,
            // F038700, an NPC without a hold position: 1 (0.1 with the AI +0x310 +0x129 flag,
            // halved without F044612: (harness) neither)
            Score::Method(53) => 1.0,
            // Shopping (0x140999620): 1, none while the shopper has been lately
            Score::Function(0x140999620) => {
                if self.now - self.shopped_at < SHOP_AGAIN_HOURS {
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
            // Shopping (0x1409997f0: a building of the shopping list's designation in the town,
            // at its shop counter): (stand-in) a free general node in a building of the town
            // with a shopkeeper's node, not the shopper's home; the one it has kept
            SHOPPING => {
                let shops: Vec<usize> = self.nodes.iter().filter(|n| n.kind == NODE_SHOPKEEPER && Some(n.placement) != self.home).map(|n| n.placement).collect();
                if let Target::Node(n) = t
                    && free(self, *n)
                    && shops.contains(&self.nodes[*n].placement)
                {
                    return Some(*t);
                }
                let spots: Vec<usize> = (0..self.nodes.len()).filter(|&i| self.nodes[i].kind == NODE_GENERAL && free(self, i) && shops.contains(&self.nodes[i].placement)).collect();
                if spots.is_empty() {
                    return None;
                }
                let k = ((self.rng.next() * spots.len() as f32) as usize).min(spots.len() - 1);
                Some(Target::Node(spots[k]))
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
            // Attacking enemies: AI::findEnemy (F038641) over the known characters, the area
            // around me (mode 2), no hold position; the squad's scoring by nearness (harness)
            ATTACKING_ENEMIES => {
                if self.enemies == 0 {
                    return None;
                }
                find_enemy(
                    self.known,
                    self.pos,
                    &Area::Me,
                    None,
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
        let state = self.bar_state.as_deref_mut()?;
        if state.bar.is_none() {
            state.bar = choose_bar(bars, r);
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
        state.held.insert(me, chair);
        Some(chair)
    }

    /// F050297 / F031809: the home building's free node of a kind (when asked), else a random
    /// free one of the town.
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
        let all: Vec<usize> = (0..self.nodes.len()).filter(|&i| free(i)).collect();
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

/// (stand-in) Hunger shown below which a character eats what it carries.
pub const EAT_BELOW: f32 = 200.0;

/// (harness for the Havok character's physics) an outdoor body (`pos` in metres) stands on the
/// terrain itself: the navmesh steers but is only a rough copy of the ground (up to a metre off
/// on slopes). (assumption) A navmesh more than 2 m above the ground is a structure (a bridge,
/// a platform) and is stood on instead.
fn stand_on_ground(pos: &mut [f32; 3], terrain: &mut Terrain) {
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
fn running_package<'a>(data: &'a GameDataContainer, t: &kenshi_data::GameData) -> Option<&'a kenshi_data::GameData> {
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
    /// The zones whose buildings and residents are in (the first one's from `load`), in the
    /// order they came in (their placements', furniture's and nodes' indices follow it).
    pub zones: std::collections::HashSet<(i32, i32)>,
    pub zone_order: Vec<(i32, i32)>,
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
    /// The GLOBAL CONSTANTS first aid reads.
    pub first_aid_c: kenshi_char::medical::FirstAidConstants,
    /// The new game's "Bandits loot the player" (F048535): the world's looters strip the
    /// player's knocked-out characters too. (assumption) On, as Kenshi's default start.
    pub bandits_loot_player: bool,
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
    /// Lock attempts this tick (who, whether it opened), for the "Pick failed" / "Pick success!"
    /// floaters (Task_PickLock::update, F031344).
    pub pick_events: Vec<(usize, bool)>,
    /// Each cage's half size across (units, from its mesh): where its door lets out.
    pub cage_reach: HashMap<usize, f32>,
    /// What the town's machines worked by slaves have made: each furniture's part-made products
    /// and its storage (crate::slavery::slave_work_tick).
    pub machine_stock: HashMap<usize, (Vec<f32>, kenshi_char::inventory::Inventory)>,
    pub dialog_memory: crate::dialogue::DialogMemory,
    /// Who was down and dead at the last memory tick (crate::dialogue::memory_tick).
    pub mem_down: Vec<(bool, bool)>,
    pub npc_talks: Vec<(crate::dialogue::Conversation, Vec<crate::dialogue::Effect>)>,
    /// Lines said aloud (speaker, text), for the game to show over their heads (`speech`).
    pub said: Vec<(usize, String)>,
    /// Items on the ground (`loot`), the next one's number, and the towns whose loot is out.
    pub ground: Vec<crate::loot::GroundItem>,
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
        let terrain = Terrain::open(install).unwrap();
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
                DoorState {
                    door,
                    locked,
                    level,
                    inside,
                }
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
                shopped_at: -100.0,
                shop_until: None,
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
            let leaders: i32 = t.refs("leader").iter().map(|(_, v)| v[0].max(1)).sum();
            let squad: i32 = t.refs("squad").iter().map(|(_, v)| v[0]).sum::<i32>()
                + if t.refs("choosefrom list").is_empty() {
                    0
                } else {
                    2
                };
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
                faction_of(t),
            );
        }
        // the town's bar squads (TOWN "bar squads") at its bar; (harness) one of each listed: the
        // town population manager that picks and times them is not ported
        if let (Some(t), Some(&bar)) = (home_town, bars.first())
            && let Some(town_rec) = data.get(&towns[t].record)
        {
            for (sid, _) in town_rec.refs("bar squads") {
                let Some(t) = data.get(sid) else { continue };
                let Some(package) = t
                    .refs("AI packages")
                    .iter()
                    .max_by_key(|(_, v)| v[0])
                    .and_then(|(s, _)| data.get(s))
                else {
                    continue;
                };
                let leaders: i32 = t.refs("leader").iter().map(|(_, v)| v[0].max(1)).sum();
                let squad: i32 = t.refs("squad").iter().map(|(_, v)| v[0]).sum::<i32>()
                    + if t.refs("choosefrom list").is_empty() {
                        0
                    } else {
                        2
                    };
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
                    faction_of(t),
                );
            }
        }
        // squads asked for on the command line, at the town's centre; (harness) they arrive facing
        // the residents' middle
        let centre = home_town.map_or(general[0].position, |t| towns[t].position);
        let first_extra = folk.len();
        let residents: Vec<[f32; 3]> = folk.iter().map(|p| p.mover.position_units()).collect();
        let mut player_kit: Option<String> = None;
        // a game start's faction relations (faction, value) and research done
        let mut start_relations: Vec<(String, f32)> = Vec::new();
        let mut start_research: Vec<String> = Vec::new();
        // the player's money: the game start's "money", (harness) 1000 cats when launched with no
        // start
        let mut money = 1000;
        for extra in extras {
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
                player_kit = Some(t.to_string());
                continue;
            }
            if let Some(n) = extra.strip_prefix("player:") {
                // the player's squad: Nameless, no goals of its own; (harness) it starts at the
                // town's first general node that is outdoors
                let n: usize = n.parse().expect("player:<count>");
                let centre = approach
                    .iter()
                    .copied()
                    .find(|q| building_at(&inner, *q).is_none())
                    .unwrap_or(centre);
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
            if let Some(t) = &player_kit {
                let players: Vec<usize> = folk.iter().filter(|p| p.player).map(|p| p.id).collect();
                if let (Some(&a), Some(&b)) = (players.first(), players.last()) {
                    ranges.push((a, b + 1, t.clone()));
                }
            }
            for p in folk.iter_mut() {
                let template = ranges.iter().rev().find(|(a, b, _)| (*a..*b).contains(&p.id)).and_then(|(a, _, sid)| {
                    let chars = chars_of(sid);
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
        eprintln!("{} people", folk.len());
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
                mult: 1.0,
                zones: vec![],
            }),
            nests: Vec::new(),
            zones: [(zx, zy)].into_iter().collect(),
            zone_order: vec![(zx, zy)],
            quiet_zones: false,
            pending_world: None,
            layouts: None,
            money,
            met: HashMap::new(),
            pick_events: Vec::new(),
            cage_reach: HashMap::new(),
            machine_stock: HashMap::new(),
            slave_squads: kits.iter().filter(|(_, t)| data_arc.get(t).is_some_and(|r| !r.refs("slaves").is_empty())).cloned().collect(),
            dialog_memory: Default::default(),
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
            wind: 0.0,
            rain: 0.0,
            missing_material: None,
            bandits_loot_player: true,
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
            hunger_c: {
                let g = |k: &str, d: f32| data_arc.get("110-gamedata.quack").and_then(|r| r.floats.get(k).copied()).unwrap_or(d);
                let race = data_arc.get(&race_record_id).and_then(|r| r.floats.get("hunger rate").copied()).unwrap_or(1.0);
                [g("starvation time", 60.0), g("fed recovery rate mult", 3.0), g("bed hunger rate", 0.33), g("encumbrance hunger rate", 0.7), race]
            },
        };
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
    /// not theirs, within 400 units, one each, and carry them on with the squad.
    /// (stand-in) the handlers are not decompiled; cages, shackles and slave camps not ported.
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
            let taken: Vec<usize> = self.folk.iter().filter_map(|p| p.pickup).collect();
            let prey = self
                .folk
                .iter()
                .filter(|p| p.down && !p.dead && p.carried_by.is_none() && p.race.is_none() && p.faction != faction && !taken.contains(&p.id))
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
    /// fighter of that race with its natural weapon ("weapon") and its stats.
    /// (stand-in) Animals fight with the combat system's human techniques' timing.
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
        let old = &self.arena.f[id];
        let (team, foes) = (old.team, old.foes.clone());
        let mut f = Fighter::new(&data, race, &self.kit, &id.to_string(), team, KIT_SKILL, pos.map(|v| v / METRES));
        f.mover.position = pos;
        f.active = false;
        f.foes = foes;
        // (assumption) an animal's natural weapon at quality 50, as the harness had it
        let weapon = a.refs("weapon").first().and_then(|(w, _)| data.get(w)).map(|r| kenshi_char::inventory::Item::from_record(r, 1).with_quality(&data, 50.0));
        f.equip(&data, race, weapon.as_ref(), &[]);
        // its skeleton's clips: it fights with its "attack type" techniques (F040738 "animal")
        let clips = match self.animal_clips.get(&race.string_id) {
            Some(c) => Some(c.clone()),
            None => skeleton_clips(&self.res, race).map(|(l, _)| {
                let c = std::sync::Arc::new(l);
                self.animal_clips.insert(race.string_id.clone(), c.clone());
                c
            }),
        };
        f.clips = Some(clips.unwrap_or_default());
        let mult = kenshi_char::stats::race_multipliers(race);
        let rng = &mut self.rng;
        let mut skills = f.skills.clone();
        skills.levels = kenshi_char::stats::character_levels(&data, a, &mult, &mut || rng.next());
        skills.race_xp = mult;
        f.set_skills(&data, race, skills);
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
        let armour: Vec<kenshi_char::inventory::Item> = inv.equipped().filter(|i| i.kind == Some(kenshi_data::ItemType::Armour)).cloned().collect();
        let Some(race) = self.data.get(&self.race_record).cloned() else { return };
        let data = self.data.clone();
        self.arena.f[id].equip(&data, &race, weapon.as_ref(), &armour);
        // an equipped crossbow (GunClassPersonal, F033879), its loaded shots kept while it stays
        // the same one; (assumption) the global damage factor is 1, as hit resolution takes it
        let bow = inv.equipped().find(|i| i.kind == Some(kenshi_data::ItemType::Crossbow)).cloned();
        let f = &mut self.arena.f[id];
        f.gun = bow.and_then(|b| {
            let r = data.get(&b.record)?;
            let mut g = kenshi_combat::ranged::Gun::personal(&data, r, b.quality, 1.0);
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
            };
            // the downed, and prisoners in their cage, take no orders (a slave let out does)
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
    /// (stand-in) Sight and reach tests (F050277's 0x51f05, 0x1d81d) are not ported.
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
                    if dead != q.dead || (!dead && !q.down) || (!dead && q.player && !self.bandits_loot_player) {
                        return false;
                    }
                    let at = q.mover.position_units();
                    if REACH2 < (at[0] - me[0]).powi(2) + (at[2] - me[2]).powi(2) {
                        return false;
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
    /// seconds it takes a weapon (the "hip" and "back" slots, F043569); at 8 the rest of the
    /// weapons, then everything else that fits (F031165 modes 2 and 4), and it is done.
    /// (stand-in) Armour is left on; a backpack's contents stay in it; the butchering (20
    /// seconds, cannibals) and the crime of looting when seen are not ported.
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
            if t < 0.8 && 0.8 <= now {
                for s in weapons(&self.folk[target].inventory) {
                    if self.loot_one(target, s, id) {
                        break;
                    }
                }
            }
            if 8.0 <= now {
                let inv = &self.folk[target].inventory;
                let mut order = weapons(inv);
                order.extend((0..inv.sections.len()).filter(|&s| !inv.sections[s].is_equipment()));
                for s in order {
                    while self.loot_one(target, s, id) {}
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

    /// Bring a knocked-out person round: they get up (held for the "standing up 3" clip,
    /// F037164) and carry on. (stand-in until recovery is ported: callers decide when)
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
    /// identified) the platoon layout (F028591) of the player faction's "squad formation", its
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
        let kind = self
            .data
            .get(PLAYER_FACTION)
            .and_then(|f| f.ints.get("squad formation"))
            .and_then(|&v| FormationKind::from_int(v))
            .unwrap_or(FormationKind::Loose);
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
        // each to a place short of them (the escort's walk, [`Sim::escort_tick`])
        let at = self.folk[whom].mover.position_units();
        for &id in &ids {
            let a = self.folk[id].mover.position_units();
            let d = (a[0] - at[0]).hypot(a[2] - at[2]);
            let k = ((ESCORT_GAP * 0.5) / d.max(1e-3)).min(1.0);
            self.order_move(&[id], [at[0] + (a[0] - at[0]) * k, at[1], at[2] + (a[2] - at[2]) * k]);
        }
        for &id in &ids {
            if let Some(p) = self.folk.get_mut(id)
                && !p.down
                && p.prison.is_none()
            {
                p.escort = Some((whom, guard));
            }
        }
    }

    /// The Follow and Bodyguard orders, twice a second: (assumption, the game's follow tasks
    /// not ported) each walks after the one it follows once 40 units behind; a bodyguard goes
    /// for anyone fighting them; the order ends with them dead.
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
            if d > ESCORT_GAP {
                // to a place short of them, on the way from here
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
        // (law) the player's people attacking someone of a faction not their enemy: assault
        if let Some(theirs) = self.folk[whom].faction.clone() {
            for &id in ids {
                let p = &self.folk[id];
                let enemy = p.faction.as_ref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_enemy(Some(&theirs)));
                if p.player && !enemy && !self.folk[whom].player {
                    self.commit_crime(id, kenshi_ai::CrimeEnum::CrimeAssault, Some(&theirs));
                }
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
        // (crate::disguise) one disguised passes as its disguise's faction
        let factions: Vec<Option<String>> = folk.iter().map(|p| p.disguised.clone().or_else(|| p.faction.clone())).collect();
        let internal: Vec<[f32; 3]> = folk.iter().map(|p| p.mover.position).collect();
        prof.mark("sim: setup");
        // think once a second, staggered by person
        for p in folk.iter_mut() {
            if tick % 60 != (p.id as u64 * 7) % 60 && !p.rethink {
                continue;
            }
            p.rethink = false;
            if p.down || p.getting_up > 0.0 || p.away {
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
            p.town = current_town(towns, p.mover.position_units(), p.town);
            p.inside = building_at(inner, p.mover.position_units());
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
                bar_state: p.platoon.and_then(|pl| bar_states[pl].as_mut()),
                inside: p.inside,
                shopped_at: p.shopped_at,
                in_bed: p.bed,
                woke_at: p.woke_at,
                start_hour,
                doors: door_states,
                bed_taken,
                known: &known,
                enemies,
                free_will: !p.player,
                downs: &downs,
            };
            if std::env::var("TOWN_FIGHT_DEBUG").is_ok_and(|w| w == p.id.to_string()) {
                let pos = v.pos;
                let mut pl =
                    kenshi_ai::planner::Planner::new(&mut v, kenshi_ai::planner::Repertoire::Human);
                let r = pl.plan(ATTACKING_ENEMIES, Target::None, pos);
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
                // a new plan: let go of the old node (Task_StandAtNode end, F031321) unless the
                // plan goes back to it; (assumption) the node the plan stands at is reserved at
                // once, so the next decision finds it again
                let next = p.ai.queue.iter().find_map(|s| match (s.task, s.target) {
                    (STAND_AT_NODE | SHOPPING, Target::Node(n)) => Some(n),
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
        // (F047037, the worse leg's health x 1.8), hunger (F040424, the player's only: NPCs
        // don't get hungry here), its load (F047034: what it carries against its strength;
        // carrying someone (stand-in) at most 0.6), sneaking (F047035's stealth level x 0.01,
        // at least 0.1: (stand-in) the stealth buffs are not ported); an ordered move keeps its
        // mode's cap over it
        for p in folk.iter_mut() {
            let f = &arena.f[p.id];
            let ec = kenshi_char::encumbrance::EncumbranceConstants::default();
            let strength = kenshi_char::encumbrance::strength_level(f.level(kenshi_char::StatsEnumerated::StatStrength), 1.0);
            let mut load = kenshi_char::encumbrance::load(p.inventory.weight(), 1.0, strength, &ec);
            if p.carrying.is_some() {
                load = load.min(0.6);
            }
            let hunger = if p.player { kenshi_char::encumbrance::hunger_speed(f.medical.hunger.level) } else { 1.0 };
            let stealth = (f.level(kenshi_char::StatsEnumerated::StatStealth) * 0.01).clamp(0.1, 1.0);
            let leg = |side: i32| {
                f.medical
                    .parts
                    .iter()
                    .find(|q| q.kind == 1 && q.side == side)
                    .map_or(1.0, |q| (q.health() / q.max_health()).max(0.0))
            };
            let m = kenshi_char::encumbrance::MoveInputs {
                run_speed: if f.run > 0.0 { f.run } else { run },
                legs: kenshi_char::encumbrance::leg_fitness(leg(1), leg(2)),
                hunger,
                equipment: 1.0,
                load,
                mult: 1.0,
                sneaking: p.sneaking.then_some(stealth),
                water: kenshi_char::encumbrance::Water::Dry,
                swimmer: true,
                walk_speed: walk,
                swim_speed: walk,
                prone: false,
                slowed: false,
            };
            p.own_speed = kenshi_char::encumbrance::move_speed(&m);
            if p.getting_up > 0.0 {
                p.getting_up = (p.getting_up - dt).max(0.0);
                stop(&mut p.mover, &mut p.pathing);
            }
            if let Some(mode) = p.ordered_cap {
                p.mover.set_max_speed(speed_limit(p.own_speed, mode, None));
            }
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
                                "t {t:.1} person {} go_to {:?} {:?} accepted {ok}",
                                p.id,
                                step.target,
                                goal.map(|v| v.round())
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
                // Shopping: at the shop (within 30 units of its spot) for 0.25 .. 1.5 game hours
                // (the task's duration), then home again
                SHOPPING => {
                    let Target::Node(n) = step.target else {
                        p.ai.queue.clear();
                        continue;
                    };
                    if !p.started {
                        if owners[n].is_some_and(|o| o != p.id) {
                            p.ai.queue.clear();
                            continue;
                        }
                        owners[n] = Some(p.id);
                        p.claimed = Some(n);
                        p.started = true;
                    }
                    let (a, b) = (p.mover.position_units(), approach[n]);
                    if (a[0] - b[0]).hypot(a[2] - b[2]) < 30.0 {
                        match p.shop_until {
                            None => {
                                p.shop_until = Some(now + 0.25 + rng.next() as f64 * 1.25);
                                if std::env::var("KENSHI_SHOP_DEBUG").is_ok() {
                                    eprintln!("shopping: {} at the shop of node {n}", p.id);
                                }
                            }
                            Some(until) if until <= now => {
                                p.shop_until = None;
                                p.shopped_at = now;
                                p.ai.queue.clear();
                                p.ai.current = None;
                                p.rethink = true;
                            }
                            _ => {}
                        }
                    }
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
                    // (assumption) the lock turns at once
                    if let Target::Door(d) = step.target {
                        door_states[d].locked = true;
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
                OPERATING_MACHINE => {
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
                    if !arena.f[p.id].active
                        && !shoots
                        && ((goal[0] - me[0]).hypot(goal[2] - me[2]) > ENGAGE_RANGE
                            || (goal[1] - me[1]).abs() > AT_LOCATION_DY)
                    {
                        let moved = p
                            .run_goal
                            .is_none_or(|o| (o[0] - goal[0]).hypot(o[2] - goal[2]) > 20.0);
                        if moved && !p.pathing.pending {
                            p.mover.set_max_speed(speed_limit(
                                p.own_speed,
                                MoveSpeed::Run.cap(15.0).unwrap(),
                                None,
                            ));
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
                    let idle = !p.pathing.pending && p.pathing.follow.is_none();
                    if let Some(t) = c.target
                        && (!p.started || idle)
                    {
                        // (stand-in for the global pathing) far off: a leg of most of a zone
                        // toward the town, ordered again when idle; there: a point in the town
                        let town = &towns[t];
                        let (dx, dz) = (town.position[0] - me[0], town.position[2] - me[2]);
                        let d = dx.hypot(dz);
                        let leg = kenshi_world::ZONE_SIZE * 0.3;
                        let (centre, r) = if d > town.radius() + leg {
                            ([me[0] + dx / d * leg, me[1], me[2] + dz / d * leg], leg * 0.1)
                        } else {
                            (town.position, order_radius(Some(town.radius())))
                        };
                        let point = area_point(
                            centre,
                            r,
                            true,
                            || rng.next(),
                            |x, z| terrain.height_at(x, z).unwrap_or(NO_GROUND),
                        );
                        let ok = point.is_some_and(|q| p.pathing.go_to(p.id, &p.mover, q, false, manager));
                        if std::env::var("KENSHI_CARAVAN_DEBUG").is_ok() {
                            eprintln!("caravan {} leg to {point:?} (town {:.0} away): {ok}", p.id, d);
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
                    p.ai.queue.remove(0);
                }
            }
        }
        prof.mark("sim: plans, movement");
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
                    other => eprintln!("t {t:.1} person {who} no path: {other:?}"),
                }
            }
            folk[who].pathing.answer(result);
        }
        prof.mark("sim: paths");
        // the fights: the arena runs its fighters; a fighter with no one left to fight leaves it
        // those out of the fight stand where they are (bolts and lines of fire meet them there)
        for p in folk.iter() {
            let f = &mut arena.f[p.id];
            if !f.active {
                f.mover.position = p.mover.position;
            }
        }
        arena.step(dt);
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
                sleep_rate: 1.0,
                rest: 0.0,
                resting: p.down,
                asleep: false,
                size: 1.0,
                bleeds: true,
                g: 1.0,
            };
            // hunger (F040466 with F047009's rate, F032933's period); (assumption) the
            // starvation time is in game hours. (stand-in) only the player's characters get
            // hungry: NPCs' eating (bars, their own food) is not ported, and the game's NPCs do
            // not starve in practice
            if p.player {
                use kenshi_char::hunger::{Activity, hunger_rate};
                let [starve, fed, bed, enc, race_rate] = hunger_c;
                let ec = kenshi_char::encumbrance::EncumbranceConstants::default();
                let strength = kenshi_char::encumbrance::strength_level(f.level(kenshi_char::StatsEnumerated::StatStrength), 1.0);
                let load = kenshi_char::encumbrance::load(p.inventory.weight(), 1.0, strength, &ec);
                let activity = if p.down {
                    Activity::Resting
                } else if load < 1.0 {
                    Activity::Encumbered { load }
                } else {
                    Activity::Normal
                };
                let rate = hunger_rate(activity, 1.0, race_rate, bed, enc);
                let period = kenshi_char::hunger::starvation_period(starve, hour_secs as f32);
                f.medical.hunger.tick(dt, period, rate, 1.0, fed, 1.0);
                // (stand-in) eating: awake characters eat a bite from what they carry every two
                // seconds while their hunger shows under 200 (Kenshi's "hungry"); the AI's eat
                // goal is not ported
                p.eat_timer -= dt;
                let h = &f.medical.hunger;
                if p.player && ((t * 60.0) as u64).is_multiple_of(600) && std::env::var("KENSHI_HUNGER_DEBUG").is_ok() {
                    eprintln!("t {t:.1} hunger of {}: {:.1} (stomach {:.3}, queued {:.3}), period {period:.0} s, rate {rate:.2}", p.id, h.shown(), h.stomach, h.queued);
                }
                let shown = (h.level + h.stomach + h.queued) * 100.0;
                if !p.down && shown < EAT_BELOW && p.eat_timer <= 0.0 {
                    p.eat_timer = 2.0;
                    if let Some((food, bite)) = p.inventory.eat(shown) {
                        f.medical.hunger.eat(bite);
                        if p.player || std::env::var("KENSHI_HUNGER_DEBUG").is_ok() {
                            eprintln!("t {t:.1} person {} ate {food} ({bite:.0}), hunger {shown:.0}", p.id);
                        }
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
        prof.mark("sim: medical");
        for id in woken {
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
            if arena.f[p.id].active || p.down || p.getting_up > 0.0 {
                // the arena moves a fighter: (harness) its step is walked across the navmesh
                // and refused where the mesh ends; the knocked out lie still, and stay put
                // while getting up
                if arena.f[p.id].active {
                    let (from, to) = (p.mover.position, arena.f[p.id].mover.position);
                    let face = nav.closest_point(from, 5.0).map(|(f, _)| f);
                    if let Some(f) = face.and_then(|f| nav.walk(f, from, to)) {
                        p.mover.position = [to[0], nav.height_on(f, to), to[2]];
                        if p.inside.is_none() {
                            stand_on_ground(&mut p.mover.position, terrain);
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
                stand_on_ground(&mut p.mover.position, terrain);
            }
            // F040650: the measured velocity, its speed (+0xb8) and, when moving, its
            // direction (+0xd0)
            let (v, speed) = measured_velocity(before, p.mover.position_units(), dt);
            p.speed = speed;
            if speed > 0.1 {
                p.facing = v.map(|c| c / speed);
            }
            // moving trains (F047463 athletics; F047455 strength from walking under a load).
            // (stand-in) athletics gains 0.01 x the distance (units) per tick above walking pace;
            // the walking strength rate is taken per second
            if speed > 5.0 {
                use kenshi_char::StatsEnumerated::StatStrength;
                let f = &mut arena.f[p.id];
                if speed > walk * 1.2 {
                    let xp = arena.sh.xp;
                    f.skills.train_athletics(speed * dt * 0.01, false, &xp);
                }
                let ec = kenshi_char::encumbrance::EncumbranceConstants::default();
                let strength = kenshi_char::encumbrance::strength_level(f.level(StatStrength), 1.0);
                let load = kenshi_char::encumbrance::load(p.inventory.weight(), 1.0, strength, &ec);
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
        if self.tick.is_multiple_of(600) {
            self.restock_shops();
        }
        if self.tick.is_multiple_of(60) {
            self.meet_tick();
            self.capture_tick();
            self.speech_tick();
            self.loot_tick();
            self.artifacts_tick();
        }
        self.slavery_tick();
        self.slave_work_tick(dt);
        self.disguise_tick(dt);
        self.memory_tick();
        self.cage_push();
        self.medic_tick(dt);
        self.looter_tick(dt);
        self.looting_tick(dt);
        self.first_aid_tick(dt);
        self.escort_tick();
        self.pickup_tick();
        self.build_tick(dt);
        self.production_tick(dt);
        self.research_tick(dt);
        self.farm_tick(dt);
        self.power_tick(dt);
        self.limb_tick();
        self.law_tick(dt);
        self.world_tick(dt);
        prof.mark("sim: stream");
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
        // (stand-in) "attack enemies and neutrals" runs as attacking enemies, the neutrals
        // counted as enemies (its own handler is not ported)
        let attacks_neutrals = jobs.iter().any(|g| g.task == ATTACKING_NEUTRALS);
        let captures = jobs.iter().any(|g| g.task == CAPTURE_SLAVES || g.task == CARRY_SLAVES);
        let wanders = jobs.iter().any(|g| g.task == WANDERING_TRADER);
        ai.jobs = jobs_of(jobs, leader);
        let a = self.rng.next() * std::f32::consts::TAU;
        self.folk.push(Person {
            id,
            mover,
            pathing: Pathing::default(),
            ai,
            claimed: None,
            shopped_at: -100.0,
            shop_until: None,
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
        });
        self.folk[id].attacks_neutrals = attacks_neutrals;
        self.folk[id].captures = captures;
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
            self.door_states.push(DoorState { door, locked, level, inside });
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
            for (sid, _) in town_rec.refs("bar squads") {
                if let Some(sq) = data.get(sid) {
                    let at = self.placed_at[bar];
                    self.spawn_resident_squad(sq, at, Some(bar));
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
    fn spawn_resident_squad(&mut self, t: &kenshi_data::GameData, at: [f32; 3], home: Option<usize>) -> Vec<usize> {
        let data = self.data.clone();
        let Some(package) = running_package(&data, t) else {
            return Vec::new();
        };
        let leaders: i32 = t.refs("leader").iter().map(|(_, v)| v[0].max(1)).sum();
        let squad: i32 = t.refs("squad").iter().map(|(_, v)| v[0]).sum::<i32>()
            + if t.refs("choosefrom list").is_empty() { 0 } else { 2 };
        let (leaders, squad) = if leaders == 0 && squad > 0 { (1, squad - 1) } else { (leaders, squad) };
        let faction = faction_of(t);
        let pid = self.bar_states.len();
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
        // their kit: the template's characters in turn
        let mut chars = Vec::new();
        for cat in ["leader", "squad", "choosefrom list"] {
            for (c, n) in t.refs(cat) {
                for _ in 0..n.first().copied().unwrap_or(1).max(1) {
                    chars.push(c.clone());
                }
            }
        }
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
        for &m in &members {
            self.folk[m].package = Some(package.string_id.clone());
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

    /// Spawn a squad of a SQUAD_TEMPLATE at `at` (units): its leaders and members (the
    /// template's "leader" and "squad" counts) on its running package ([`running_package`]), in
    /// its faction's formation. The members' bodies
    /// are the zone's race and the harness kit.
    pub fn spawn_template(&mut self, template: &str, at: [f32; 3]) -> Option<Vec<usize>> {
        let data = self.data.clone();
        let t = data.get(template)?;
        let package = running_package(&data, t)?;
        let leaders: i32 = t.refs("leader").iter().map(|(_, v)| v[0].max(1)).sum();
        let squad: i32 = t.refs("squad").iter().map(|(_, v)| v[0]).sum();
        let (leaders, squad) = if leaders == 0 && squad > 0 {
            (1, squad - 1)
        } else {
            (leaders, squad)
        };
        // its animals (the "animals" refs: (assumption) values min and max count; the third,
        // not known, is not used)
        let mut beasts: Vec<String> = Vec::new();
        for (sid, v) in t.refs("animals") {
            let lo = v[0].max(1);
            let hi = v[1].max(lo);
            let n = lo + ((self.rng.next() * (hi - lo + 1) as f32) as i32).min(hi - lo);
            for _ in 0..n {
                beasts.push(sid.clone());
            }
        }
        if leaders + squad == 0 && beasts.is_empty() {
            return None;
        }
        let faction = t.refs("faction").first().map(|(f, _)| f.clone());
        let pid = self.bar_states.len();
        let mut members = vec![];
        for _ in 0..leaders {
            let id = self.add_person(
                at,
                package_goals(&data, package, Role::Leader),
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
                package_goals(&data, package, Role::Squad),
                leader,
                Some(pid),
                faction.clone(),
                6,
            );
            members.push(id);
        }
        let package_id = package.string_id.clone();
        // the animals: an all-animal squad's first leads
        for sid in &beasts {
            let leader = members.first().copied();
            let role = if leader.is_none() { Role::Leader } else { Role::Squad };
            let id = self.add_person(at, package_goals(&data, package, role), leader, Some(pid), faction.clone(), 6);
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
                .any(|r| package_goals(&data, p, r).iter().any(|g| g.task == CAPTURE_SLAVES || g.task == CARRY_SLAVES))
        });
        for &m in &members {
            self.folk[m].package = Some(package_id.clone());
            if captures && self.folk[m].race.is_none() {
                self.folk[m].captures = true;
            }
        }
        let sig = *package.ints.get("signal func").unwrap_or(&0);
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
    /// 1500 units clear of the player's characters and outside towns). Entries that depend on
    /// world states (F050367) are left out until world states are ported.
    fn spawn_homeless(&mut self, zone: (i32, i32), squads_here: f32, active: usize) {
        let Some(pop) = self.population.as_ref() else {
            return;
        };
        let Some(b) = biome_of_zone(&pop.map, &pop.biomes, zone.0, zone.1) else {
            return;
        };
        let biome = &pop.biomes[b];
        let amount = pop.mult * biome.homeless_spawn_amount;
        if squads_here >= amount || biome.homeless.is_empty() {
            return;
        }
        let total: i32 = biome.homeless.iter().map(|s| s.weight).sum();
        let ratio = total as f32 / (active.max(1) as f32 * amount.max(1e-3));
        let candidates: Vec<(String, f32)> = biome
            .homeless
            .iter()
            .filter(|s| {
                self.data
                    .get(&s.template)
                    .is_some_and(|t| t.refs("world state").is_empty())
            })
            .map(|s| {
                let out = pop
                    .squads
                    .iter()
                    .filter(|(_, _, t)| *t == s.template)
                    .count();
                let w = (s.weight as f32 - out as f32 * ratio).max(0.0);
                (s.template.clone(), w)
            })
            .filter(|(_, w)| *w > 0.0)
            .collect();
        let sum: f32 = candidates.iter().map(|(_, w)| w).sum();
        if sum <= 0.0 {
            return;
        }
        let mut r = self.rng.next() * sum;
        let template = candidates
            .iter()
            .find(|(_, w)| {
                r -= w;
                r < 0.0
            })
            .map_or(candidates[0].0.clone(), |(t, _)| t.clone());
        // the place
        let (cx, cz) = zone_centre(zone.0, zone.1);
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
                pop.squads.push((members, zone, template));
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

/// A package's goals as a person's AI jobs (by classification): "attack enemies and neutrals"
/// run as attacking enemies, the wandering trader's trips as patrolling (`Person::caravan`), a
/// goal targeting the leader on `leader`.
pub(crate) fn jobs_of(goals: Vec<PackageGoal>, leader: Option<usize>) -> [Vec<Task<Target>>; 4] {
    let mut jobs: [Vec<Task<Target>>; 4] = Default::default();
    for mut g in goals {
        if g.task == ATTACKING_NEUTRALS {
            g.task = ATTACKING_ENEMIES;
        }
        if g.task == WANDERING_TRADER {
            g.task = PATROLLING;
        }
        let class = (g.classification as usize).min(3);
        let target = match (g.targeting, leader) {
            (2, Some(l)) => Target::Person(l),
            _ => Target::None,
        };
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
