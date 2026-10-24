//! A melee arena (port_map `combat`): any number of fighters on the ported combat rules.
//!
//! Each fighter's combat AI is the ported CombatClass ([`crate::fighter`]): its update runs the
//! original's state machine. This module is the game side around it ([`Ctx`]): the characters'
//! animation actions (timed by the ported animation clock, moved by the clips' root bone),
//! movement (the ported mover and formation steering), hits (resolution, armour, stumbles,
//! knockouts) and target finding (F040734: the current target is kept for a second, two on hold,
//! then the best scored standing enemy, F038638 [`score_targets`], with the characters' stances
//! and the player's leash). Not ported here: the threat lists, neighbour avoidance and the
//! ragdoll falls (a collapse's, F040453); the ground is flat (no navmesh).

use crate::attack::{Target, TargetKind, Unarmed, attack_damage, martial_arts};
use crate::engage::{Approach, Asked, can_engage, request_attack};
use crate::fighter::{CombatClass, Fighting};
use crate::hit::{Armour, HitConstants, HitContext, resolve_hit};
use crate::melee::{Action, Defence, Striker, Struck, block_direction, defend_outcome, punch_feedback};
use crate::states::{
    AttackerSlots, EngagedStep, IdleChoice, Swing, engaged_step, extra_reach, frame_damage, idle,
    in_swing_box, pull_back, recovery_time, slot_count, swing_box, try_attack,
};
use crate::stumble::stumbles;
use crate::stumble::{hit_from_behind, pick_stumble, stagger, stagger_threshold};
use crate::technique::Techniques;
use crate::technique::{
    Attacking, CombatTechnique, Defender, Reaction, Threat, choose_defence, react_to_attack,
    technique_play,
};
use crate::weapon::Wielded;
use crate::weapon::{Weapon, set_weapon};
use crate::{CombatConstants, SwordState, block_chance};
use kenshi_anim::clip::SingleAnimation;
use kenshi_anim::layer::{AnimationLayer, LAYER_BLEND, PlayParams};
use kenshi_anim::select::{
    AnimationData, Library, Situation, idle as idle_anim, movement, pain_postures, shows_pain,
};
use kenshi_char::combat_speed::{StrainConstants, speeds, weapon_handling, weapon_strain};
use kenshi_char::encumbrance::run_speed;
use kenshi_char::movement::{MoveSpeed, Mover, RaceMovement, speed_limit};
use kenshi_char::steering::{Band, Steering, circle_distance, steer_band};
use crate::WeaponCategory;
use kenshi_char::stats::{CombatEvent, CombatTraining, Skills, ToughnessKind, XpConstants};
use kenshi_char::{Constants as MedConstants, Medical, Stats, StatsEnumerated};
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_io::{ogre_mesh, ogre_skeleton, resources::Resources};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// Fighters, what they share, and their animation layers.
pub struct Arena {
    pub f: Vec<Fighter>,
    pub sh: Shared,
    pub layers: Vec<Layers>,
    pub roots: HashMap<String, Vec<(f32, [f32; 2])>>,
    pub tick: u32,
    /// Seconds run.
    pub t: f32,
    /// Print the renderer's rows (`f,...`).
    pub rows: bool,
    pub blend_rate: f32,
    /// The bolts in the world (HarpoonManager's harpoons).
    pub bolts: Vec<crate::ranged::Bolt>,
    /// The mounted guns ([`Arena::step_turrets`]).
    pub turrets: Vec<crate::ranged::Turret>,
    /// The animals' skeletons' root tracks by race (string id), loaded once.
    pub animal_roots: HashMap<String, std::sync::Arc<HashMap<String, Vec<(f32, [f32; 2])>>>>,
    /// (set by the world) The buildings' solids, in a frame whose origin is the given world
    /// point: what stops bolts and blocks lines of fire (the physics trace of F033852 / F033856).
    pub obstacles: Option<(std::sync::Arc<kenshi_physics::world::StaticWorld>, [f32; 3])>,
}

/// The distance from `from` along the unit `dir` to the first building solid within `len`
/// (world units), if any.
pub fn wall_hit(obs: &Option<(std::sync::Arc<kenshi_physics::world::StaticWorld>, [f32; 3])>, from: [f32; 3], dir: [f32; 3], len: f32, groups: u32) -> Option<f32> {
    let (w, o) = obs.as_ref()?;
    w.raycast_groups_at([from[0] - o[0], from[1] - o[1], from[2] - o[2]], dir, len, groups)
}

/// @fn F033821
///
/// The line of fire's trace's PhysX collision groups (F033852 asks F033821 with its flag set):
/// 0, 1, 2 (the terrain), 9 - 12 (floors), 15 - 22 (stairs, furniture) and 27 (roofs); not the
/// map features' and foliage's 6 and 14, the doors' 5 or the passable parts' 23 - 26.
pub const LINE_OF_FIRE_GROUPS: u32 = 0x87f_9e07;
/// @fn F033856
///
/// A bolt's trace's groups (its ray query's +0x78): those of the line of fire and 6 and 14 (the
/// map features and the foliage), but not 3 and 4 (the ragdolls).
pub const BOLT_GROUPS: u32 = 0x87f_de47;

/// A small deterministic generator (the original's rolls are uniform in [0, 1)).
pub struct Rng(pub u64);

impl Rng {
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

/// The root bone's ground-plane position (side, forward) at clip time `t`.
pub fn root_at(track: &[(f32, [f32; 2])], t: f32) -> [f32; 2] {
    let Some(first) = track.first() else {
        return [0.0; 2];
    };
    if t <= first.0 {
        return first.1;
    }
    for w in track.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t <= b.0 {
            let k = if b.0 > a.0 {
                (t - a.0) / (b.0 - a.0)
            } else {
                1.0
            };
            return [
                a.1[0] + (b.1[0] - a.1[0]) * k,
                a.1[1] + (b.1[1] - a.1[1]) * k,
            ];
        }
    }
    track.last().unwrap().1
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Attack(usize),
    Defence(usize),
    Stumble(usize),
}

/// The animation action a character plays.
pub struct Act {
    pub kind: Kind,
    pub clip: SingleAnimation<()>,
    /// Where the move began (CombatClass +0x15c), for the swing box.
    pub start: [f32; 3],
}

/// The movement order of this tick.
#[derive(Clone, Copy)]
pub enum Order {
    Hold,
    Goal([f32; 3]),
    Drive([f32; 3]),
}

pub struct Fighter {
    pub name: String,
    /// Which side (faction) it fights for.
    pub team: u16,
    /// The teams it fights (None: every other team).
    pub foes: Option<Vec<u16>>,
    /// Taking part: a fighter out of the fight is neither run nor targeted.
    pub active: bool,
    /// The target this tick's game side measures against (the combat AI's +0x290).
    pub focus: Option<usize>,
    /// Time since the last target pick (CombatClass +0x2f0, s).
    pub pick_timer: f32,
    /// Its stats (CharStats, T00026) by id.
    pub skills: Skills,
    pub mover: Mover,
    /// Collision radius (CombatClass +0x1a0): the race's footprint.
    pub radius: f32,
    /// Those its mover pushes against (F024363): its sensor's proximity list (CharSensor +0x68,
    /// F046548) less those that push no one (carried, seated); set by the game each tick.
    pub near: Vec<usize>,
    pub medical: Medical,
    pub stats: Stats,
    pub wielded: Wielded,
    /// The load factor (CharStats +0x190, F047034: 1 within what it can carry), set by the game
    /// each tick.
    pub load: f32,
    /// The dust storm on it (MedicalSystem +0x14c while +0x148 is 1, F040451: its strength less
    /// the gear's protection), set by the game.
    pub dust: f32,
    /// The legs' fitness (F047037 legFitness: the weaker leg's, x 1.8, within 0 .. 1), set by
    /// the game.
    pub legs: f32,
    pub armour: Vec<Armour>,
    pub attack_speed: f32,
    pub defence_speed: f32,
    pub run: f32,
    /// Weapon strain (F047010; CharStats +0x184, assumption) and the weapon's weight.
    pub strain: f32,
    pub weapon_weight: f32,
    pub combat: CombatClass<usize>,
    pub act: Option<Act>,
    pub slots: AttackerSlots<usize>,
    pub order: Order,
    pub down: bool,
    /// When it was knocked out (s).
    pub down_at: f32,
    /// An animal: its own skeleton's clip lengths (its techniques are its ANIMAL_CHARACTER
    /// "attack type" moves, played on that skeleton).
    pub clips: Option<std::sync::Arc<HashMap<String, f32>>>,
    /// An animal: its skeleton's root-bone tracks by clip (lower case), which its moves drive
    /// it by (the lunges), times its size.
    pub roots: Option<std::sync::Arc<HashMap<String, Vec<(f32, [f32; 2])>>>>,
    /// The race's "extra attack slots" (+0x84): more fighters may attack it at once.
    pub extra_slots: i32,
    pub last_stumble: Option<String>,
    pub steering: Steering,
    /// CircleStateAI's timer (+0x10).
    pub circle_timer: f32,
    pub max_reach: f32,
    pub last_attack: Option<usize>,
    /// The animation's last weighted root position (AnimationClass +0x8c).
    pub root_prev: [f32; 2],
    /// A crossbow it carries (GunClassPersonal), its ranged combat (F033858), the ammo charges it
    /// carries for it, and the charges spent that the game has yet to take from its inventory.
    pub gun: Option<crate::ranged::Gun>,
    pub ranged: crate::ranged::Ranged,
    pub ammo: u32,
    pub ammo_spent: u32,
    /// Shooting this tick: its ranged combat runs instead of the melee AI.
    pub shooting: bool,
    /// The ground stands between its gun and its target (F033852's trace against the world,
    /// set from the terrain before each step: `ground_in_line`).
    pub ground_in_line: bool,
    /// The worn armour's modifiers (F047071) and the weapon's attack and defence mods
    /// ("attack mod", "defence mod", with its model's; F047028 adds them to the armour's).
    pub gear: crate::armour::GearMods,
    pub weapon_mods: (i32, i32),
    /// CharStats +0x28 (F047028): the weapon's "indoors mod" (F047085), 8 with no weapon at all.
    pub indoors_mod: i32,
    /// Inside a building (Character vtable 0x1d8), set by the game: the buffed bonuses take
    /// the indoors mod (F047029 / F047030).
    pub indoors: bool,
    /// Carrying someone over the left shoulder (Character +0x348; F039254 marks the animation
    /// class's left arm busy, +0x21e), set by the game: its clips are the carrying ones
    /// ("sword ready (carry)" its combat idle).
    pub carrying: bool,
    /// The last blow's knockback (units, direction x strength) and when it landed (s).
    pub knock: [f32; 3],
    pub knock_at: f32,
    /// What target scoring reads of the character besides the fight (the game sets it each
    /// tick): its stance, whether it is the player's, knocked over, crippled.
    pub targeting: Targeting,
    /// Those its AI leaves alone (its ignore map, AI +0x218): not scored as targets (F038789).
    pub ignored: Vec<usize>,
    /// (units) Where it steers on its way to its target when the straight line is blocked: the
    /// next corner of its path there (CombatClass's path list, +0x200), set by the world.
    pub via: Option<[f32; 3]>,
    /// (set by the world) Under its player's order to be elsewhere: the fight neither steers nor
    /// moves it (it runs where it was sent), though it can still be struck.
    pub ordered_away: bool,
    /// What its body is (the Character's virtuals and its race's flags the fight reads).
    pub body: Body,
    /// (set by the world) In deep water (F039118 state 3): its combat speeds x 0.8.
    pub deep_water: bool,
}

/// What a fighter's body is, as the combat code asks its Character and race.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// An animal (Character vtable 0x248 answers: a CharacterAnimal).
    pub animal: bool,
    /// The race's "is robot" (RaceData +0x7c).
    pub robot: bool,
    /// The race's string id (weapons' "race damage" is keyed by it).
    pub race: String,
    /// The race's "bleed rate" (RaceData +0x5c): the blood a cut or pierce costs at once
    /// (F040472).
    pub bleed: f32,
    /// Character vtable 0x390: 1 for people (F039359); an animal's lerp(growth, "scale min",
    /// "scale max") (F039296).
    pub size: f32,
    /// Character vtable 0x398: 1 for people (F039360); an animal's growth (F039298), which
    /// CharStatsAnimal::update copies into CharStats +0x34 (F047068) to scale its stats.
    pub maturity: f32,
    /// The race's "hull size X / Y / Z": its physics hull (F040654).
    pub hull: [f32; 3],
    /// GLOBAL CONSTANTS "min / max toughness ko point" (its KO point's range, F040402).
    pub ko_range: (f32, f32),
    /// The race's "min blood" and "max blood" (RaceData +0x50, +0x54: maxBlood F040410).
    pub blood_range: (f32, f32),
    /// The height of the bone shots aim at (F033844) in its skeleton's bind pose at size 1:
    /// "Bip01 Spine2" for a person ([`AIM_HEIGHT`]), "Bip01 Spine1" for an animal.
    pub aim: f32,
}

impl Body {
    /// A person of `race`.
    pub fn of_race(race: &GameData) -> Body {
        Body {
            animal: false,
            robot: race.bools.get("is robot").copied().unwrap_or(false),
            race: race.string_id.clone(),
            bleed: race.floats.get("bleed rate").copied().unwrap_or(1.0),
            size: 1.0,
            maturity: 1.0,
            hull: {
                let h = |k: &str, d: f32| race.floats.get(k).copied().or_else(|| race.ints.get(k).map(|&v| v as f32)).unwrap_or(d);
                [h("hull size X", 5.0), h("hull size Y", 17.0), h("hull size Z", 5.0)]
            },
            aim: AIM_HEIGHT,
            ko_range: (10.0, 85.0),
            blood_range: {
                let h = |k: &str, d: f32| race.floats.get(k).copied().or_else(|| race.ints.get(k).map(|&v| v as f32)).unwrap_or(d);
                (h("min blood", 75.0), h("max blood", 150.0))
            },
        }
    }
}

/// A fighter's side of target scoring (F038638) that the fight does not keep.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Targeting {
    /// The stance toggles (CharStats +0x128 .. +0x12c, kenshi_char::stance).
    pub stance: kenshi_char::stance::Stance,
    /// One of the player's people (its faction's +0x250): its targets kept within a leash.
    pub player: bool,
    /// Lying knocked over (Character +0x5b8, the ragdoll AnimationClass::playGetUp gets up
    /// from), and getting up (+0x278, set by Task_GetUp's start, F031044).
    pub knocked: bool,
    pub getting_up: bool,
    /// What its AI's task is set on (the AI's task target, a character: type 1).
    pub task_target: Option<usize>,
    /// A leg lost, or both arms (F040412: limb slot 2 or 3 a stump, or 0 and 1).
    pub crippled: bool,
    /// Its hold position (CharStats +0x130, while on hold): F038560's leash is measured from it.
    pub hold_at: Option<[f32; 3]>,
}

impl Fighter {
    pub fn pos(&self) -> [f32; 3] {
        self.mover.position_units()
    }

    pub fn level(&self, s: StatsEnumerated) -> f32 {
        self.skills.level(s)
    }

    /// The skill the wielded weapon's category trains and uses (CharStats +0x1f8; setWeapon
    /// F047095 points it at the category's field), None unarmed or with an animal's natural
    /// weapon (both the martial arts field, [`Fighter::weapon_skill_stat`]).
    pub fn weapon_stat(&self) -> Option<StatsEnumerated> {
        use StatsEnumerated::*;
        Some(match self.wielded.category {
            WeaponCategory::SkillKatanas => StatKatanas,
            WeaponCategory::SkillSabres => StatSabres,
            WeaponCategory::SkillBlunt => StatBlunt,
            WeaponCategory::SkillHeavy => StatHeavyweapons,
            WeaponCategory::SkillHackers => StatHackers,
            WeaponCategory::AttackPolearms => StatPolearms,
            WeaponCategory::SkillBow => StatCrossbows,
            WeaponCategory::SkillTurret => StatTurrets,
            _ => return None,
        })
    }

    /// The field CharStats +0x1f8 points at: the weapon's skill, else martial arts (+0x10c: setWeapon
    /// F047095 starts it there and leaves it for the unarmed and the animals' categories).
    pub fn weapon_skill_stat(&self) -> StatsEnumerated {
        self.weapon_stat().unwrap_or(StatsEnumerated::StatMartialarts)
    }

    /// Unarmed fighting (CharStats +0x1f0 = 5).
    pub fn unarmed(&self) -> bool {
        self.wielded.category == WeaponCategory::SkillUnarmed
    }

    /// @fn F040654
    ///
    /// The physics hull's half extents: the race's hull size x the size x 0.5; lying knocked
    /// over its height is its larger width.
    pub fn hull(&self) -> [f32; 3] {
        let k = self.body.size * 0.5;
        let [x, mut y, z] = self.body.hull;
        if self.down || self.targeting.knocked {
            y = x.max(z);
        }
        [x * k, y * k, z * k]
    }

    /// CharStats vtable 0x18, the weapon skill level: the +0x1f8 field (F047247); an animal's
    /// martial arts x CharStats +0x34, its maturity (CharStatsAnimal, F040361).
    pub fn weapon_skill(&self) -> f32 {
        if self.body.animal {
            self.level(StatsEnumerated::StatMartialarts) * self.body.maturity
        } else {
            self.level(self.weapon_skill_stat())
        }
    }

    /// @fn F040360
    ///
    /// CharStats vtable 0x10, the strength level: strength (+0x80) x CharStats +0x34 (the
    /// maturity).
    pub fn strength_level(&self) -> f32 {
        self.level(StatsEnumerated::StatStrength) * self.body.maturity
    }

    /// @fn F040433
    ///
    /// The body's fitness for an activity as the damage curves ask it (hunger, the parts, the
    /// arms' use, the armour's mods).
    pub fn fitness(&self, activity: i32) -> f32 {
        let mods = kenshi_char::fitness::Mods { dexterity: self.gear.dexterity, damage_output: self.gear.damage_output, ranged: self.gear.ranged, ..Default::default() };
        self.medical.fitness(activity, self.body.animal, self.ko_point(), kenshi_char::fitness::FOR_DAMAGE, &mods)
    }

    /// @fn F040410
    ///
    /// The most blood: the race's min blood + (max - min) x the strength level (vtable 0x10)
    /// x 0.01 x the size.
    pub fn max_blood(&self) -> f32 {
        let (lo, hi) = self.body.blood_range;
        Medical::max_blood(lo, hi, self.strength_level() * 0.01, 1.0, self.body.size)
    }

    /// @fn F040402
    ///
    /// The KO point: -lerp(toughness x CharStats +0x34 x 0.01, min, max toughness ko point).
    pub fn ko_point(&self) -> f32 {
        let (lo, hi) = self.body.ko_range;
        -kenshi_char::medical::lerp(self.level(StatsEnumerated::StatToughness) * self.body.maturity * 0.01, lo, hi)
    }

    /// The legs collapsed (MedicalSystem +0x160, F040452): lying prone, crawling (Character
    /// vtable 0x368 answers 2), it fights with the prone moves ("is prone": the "desperate
    /// attack"s), has no block or dodge (none is prone) and no block stance. (not ported) The
    /// fall itself (the ragdoll F040453 starts with the collapse's reason).
    pub fn prone(&self) -> bool {
        self.medical.legs_collapsed(self.body.animal, self.ko_point())
    }

    /// @fn F047041
    ///
    /// Martial arts: lerp(load (+0x190), 0.2, 1) x (the unarmed attack bonus (F047030: the
    /// armour's unarmed bonus, +0x2c, buffed as `attack_bonus`: the indoors mod inside, less the
    /// dust storm's toll) + martial arts (+0x10c)) x the maturity (vtable 0x398).
    /// (assumption) F047041 leaves F047030's buffed flag as its own caller's (dl is not set
    /// before the call): taken as buffed.
    pub fn martial(&self) -> f32 {
        martial_arts(self.load, self.buffed(self.gear.unarmed), self.level(StatsEnumerated::StatMartialarts), self.body.maturity)
    }

    /// @fn F040399
    ///
    /// The dust storm's toll on the buffed bonuses (F047029 / F047030): int(dust x 10 x 1).
    fn dust_toll(&self) -> i32 {
        if self.dust > 0.0 { (self.dust * 10.0) as i32 } else { 0 }
    }

    /// F047029 / F047030: a buffed bonus inside a building: plus the indoors mod (+0x28), a
    /// positive bonus first dropped to 0 when the mod is negative; then less the dust's toll.
    /// (assumption) Character vtable 0x1e0 answering false and the building's +0xc2 unset.
    fn buffed(&self, bonus: i32) -> i32 {
        let mut b = bonus;
        if self.indoors {
            if 0 < b && self.indoors_mod < 0 {
                b = 0;
            }
            b += self.indoors_mod;
        }
        b - self.dust_toll()
    }

    /// @fn F047029
    ///
    /// The attack bonus (CharStats +0x20): the armour's "combat attk bonus" summed plus the
    /// weapon's attack mod (F047028); likewise the defence bonus (+0x24). Buffed (F047029) it
    /// takes the indoors mod inside a building and loses the dust storm's toll (F040399:
    /// int(dust x 10)): `buffed`.
    pub fn attack_bonus(&self) -> i32 {
        self.buffed(self.gear.attack + self.weapon_mods.0)
    }
    pub fn defence_bonus(&self) -> i32 {
        self.gear.defence + self.weapon_mods.1
    }

    /// @fn F047043
    ///
    /// The attack skill: unarmed, martial arts (F047041); else (the attack bonus, buffed
    /// (F047029), + melee attack (+0x120)) x the maturity (vtable 0x398). The weapon's own skill
    /// is not part of it (it sets the damage, the speed and the moves).
    pub fn attack_level(&self) -> f32 {
        if self.unarmed() {
            self.martial()
        } else {
            (self.attack_bonus() as f32 + self.level(StatsEnumerated::StatMeleeAttack)) * self.body.maturity
        }
    }

    /// @fn F047044
    ///
    /// The melee defence: the defence bonus (+0x24, with the weapon's defence mod) + melee
    /// defence (+0x124); buffed, +20 in the block stance (F029592: CharStats +0x128, not
    /// prone).
    pub fn melee_defence(&self, buffed: bool) -> f32 {
        let block = buffed && self.targeting.stance.block && !self.targeting.knocked && !self.prone();
        self.defence_bonus() as f32 + self.level(StatsEnumerated::StatMeleeDefence) + if block { 20.0 } else { 0.0 }
    }

    /// @fn F047060
    ///
    /// The defence level: unarmed, the attack skill (F047043); else the melee defence
    /// (F047044, `buffed` as asked).
    pub fn defence_level_as(&self, buffed: bool) -> f32 {
        if self.unarmed() { self.attack_level() } else { self.melee_defence(buffed) }
    }

    /// F047060 buffed.
    pub fn defence_level(&self) -> f32 {
        self.defence_level_as(true)
    }

    /// @fn F047048
    ///
    /// The dodge level blocks are rolled with (the STATS "dodge", CharStats +0xf0): buffed, +20
    /// in the block stance when unarmed (F029592), less its load's toll (1 - the load factor
    /// within 0.4 .. 1, times the dodge) and its legs' (F047037 legFitness within 0.5 .. 1; 1
    /// less that, times the dodge), plus the armour's dodge mult's share (+0x38: dodge x mult -
    /// dodge).
    pub fn dodge_level(&self, buffed: bool) -> f32 {
        let d = self.level(StatsEnumerated::StatDodge);
        if !buffed {
            return d;
        }
        let block = self.targeting.stance.block && !self.targeting.knocked && !self.prone() && self.unarmed();
        let v = if block { d + 20.0 } else { d };
        let load = 1.0 - self.load.clamp(0.4, 1.0);
        let legs = self.legs.clamp(0.5, 1.0);
        v - load * d - (1.0 - legs) * d + (d * self.gear.dodge - d)
    }

    /// @fn F047056
    ///
    /// The chance (0 .. 95) to block an attacker of attack skill `skill`: the block curve on my
    /// buffed dodge level (F047048) less it.
    pub fn block_chance(&self, skill: f32, c: &CombatConstants) -> f32 {
        block_chance(self.dodge_level(true) - skill, c)
    }

    /// @fn F047063
    ///
    /// The chance (5 .. 95) to choose the right block against an attacker of attack skill
    /// `skill`: the block curve on (unarmed: my attack skill; else my buffed melee defence,
    /// F047044) less it.
    pub fn defence_chance(&self, skill: f32, c: &CombatConstants) -> f32 {
        let l = if self.unarmed() { self.attack_level() } else { self.melee_defence(true) };
        block_chance(l - skill, c).max(5.0)
    }

    /// @fn F047040
    ///
    /// A stat's level as its use reads it (CharStats::getStatLevel, buffed): the field times the
    /// body's fitness for the stat (F040433 with the stat's id for the activity, everything
    /// considered: crossbows and precision shooting with the "ranged skill mult", by the head;
    /// turrets by the head); perception adds the armour's perception bonus (+0x30) first.
    pub fn ranged_level(&self, s: StatsEnumerated) -> f32 {
        let mods = kenshi_char::fitness::Mods { dexterity: self.gear.dexterity, damage_output: self.gear.damage_output, ranged: self.gear.ranged, ..Default::default() };
        let fit = self.medical.fitness(s as i32, self.body.animal, self.ko_point(), kenshi_char::fitness::FOR_LEVEL, &mods);
        match s {
            StatsEnumerated::StatPerception => (self.gear.perception as f32 + self.level(s)) * fit,
            _ => self.level(s) * fit,
        }
    }

    /// Recompute what the stats set (CharStats::update F047065, the parts ported): weapon
    /// strain and handling, attack and defence speeds (F047012: the attack speed by martial arts
    /// unarmed, else (the weapon skill + melee attack) x the maturity; the defence speed by the
    /// buffed melee defence, F047044), run speed, the medical toughness.
    pub fn refresh(&mut self, sc: &StrainConstants, rm: &RaceMovement) {
        use StatsEnumerated::*;
        self.update_speeds(sc);
        // the most blood by the strength now (full blood stays full)
        let blood = self.max_blood();
        if self.medical.max_blood <= self.medical.blood {
            self.medical.blood = blood;
        }
        self.medical.max_blood = blood;
        self.run = (run_speed(self.level(StatAthletics), 70.0, 120.0, 1.0) - 11.0) * self.gear.athletics + 11.0;
        self.mover.set_max_speed(speed_limit(self.run, MoveSpeed::Run.cap(15.0).unwrap(), None));
        self.stats.toughness = self.level(StatToughness);
        self.stats.toughness_scale = self.body.maturity;
        let _ = rm;
    }

    /// The combat speeds as CharStats::update (F047065) keeps them: the weapon strain (F047010,
    /// by the arms' fitness and the strength level) and handling (F047011, with the load and the
    /// armour's combat speed), then the attack speed (F047012) by the dexterity level (its
    /// fitness x dexterity x +0x34) and martial arts unarmed, else (the weapon skill + melee
    /// attack) x the maturity, and the defence speed by the buffed melee defence (F047044);
    /// both x 0.8 in deep water (F039118 state 3).
    pub fn update_speeds(&mut self, sc: &StrainConstants) {
        use StatsEnumerated::*;
        let strain = weapon_strain(self.weapon_weight, self.fitness(kenshi_char::fitness::STRENGTH), self.strength_level(), sc);
        self.strain = strain;
        let handling = weapon_handling(strain, self.load, self.gear.combat_speed, sc);
        let dex = self.fitness(kenshi_char::fitness::DEXTERITY) * self.level(StatDexterity) * self.body.maturity;
        let attack = if self.unarmed() { self.martial() } else { (self.weapon_skill() + self.level(StatMeleeAttack)) * self.body.maturity };
        let (a, d) = speeds(handling, dex, attack, self.melee_defence(true), self.deep_water);
        self.attack_speed = a;
        self.defence_speed = d;
    }

    /// The fighter's side of a combat event against `other` (trainFromCombat's inputs, F047467).
    fn training(&self, other: &Fighter) -> CombatTraining {
        CombatTraining {
            weapon: self.weapon_skill_stat(),
            unarmed: self.unarmed(),
            attack: self.attack_level(),
            defence: self.defence_level_as(false),
            dodge: self.dodge_level(true),
            weapon_level: self.weapon_skill(),
            their_attack: other.attack_level(),
            their_defence: other.defence_level_as(false),
            // dexterity trains by the weapon's cutting and piercing share of its damage
            // (CharStats +0x19c cut, +0x1a0 blunt, +0x1a8 pierce; 1 with none)
            cut_share: {
                let w = &self.wielded;
                let sum = w.cut_factor + w.blunt_factor + w.pierce_factor;
                if sum > 0.0 { (w.cut_factor + w.pierce_factor) / sum } else { 1.0 }
            },
            load: self.load,
            strain: self.strain,
            toughness: self.level(StatsEnumerated::StatToughness) * self.body.maturity,
            group: 1,
        }
    }
}

/// What every fighter's game side reads.
pub struct Shared {
    pub attacks: Vec<CombatTechnique>,
    pub defences: Vec<CombatTechnique>,
    pub stumbles: Vec<AnimationData>,
    pub library: Library,
    /// Body part id -> (pain animation, threshold percent) from its "pain anim" reference.
    pub pain: HashMap<String, (String, i32)>,
    pub lengths: HashMap<String, f32>,
    pub cc: CombatConstants,
    pub hc: HitConstants,
    pub mc: MedConstants,
    pub race: String,
    pub stumble_max: f32,
    pub max_slots: i32,
    /// settings.cfg "harpoonLimit" (the options' "Harpoon limit", 10 .. 500): how many stuck bolts
    /// are kept (F033871).
    pub harpoon_limit: usize,
    pub chance_factor: f32,
    pub unarmed_mult: f32,
    /// The global damage factor (the advanced options' "Global damage multiplier", 0x142133580).
    pub damage_mult: f32,
    pub xp: XpConstants,
    /// GLOBAL CONSTANTS the weapon strain reads (CharStats::update's speeds each tick).
    pub strain: StrainConstants,
    pub rng: Rng,
    pub t: f32,
    /// Hits landed since the game last took them (attacker, victim, part, the floater's
    /// numbers): what the damage floaters show.
    pub hits: Vec<(usize, usize, usize, crate::hit::Floater)>,
    /// What the fights did since the game last took them, for their sounds.
    pub cues: Vec<Cue>,
}

/// A fight's moment with a sound (the game's "Attack", "Impact" and voice events).
#[derive(Clone, Copy, Debug)]
pub enum Cue {
    /// Someone swings (their attack begins).
    Swing(usize),
    /// A hit lands (attacker, victim, its damage).
    Hit(usize, usize, f32),
    /// Someone blocks or dodges an attack (defender, attacker, dodged).
    Block(usize, usize, bool),
    /// A crossbow shot ("Attack", WEAPONS its "shoot sound": shooter, the switch value).
    Shoot(usize, &'static str),
    /// A reload begins (the reload clip's "Reload Crossbow" event).
    Reload(usize),
    /// A bolt hits someone (shooter, victim, the hit material: -1 flesh "Impact", else
    /// "Deflection" with Armor_Worn).
    Bolt(usize, usize, i32),
    /// A bolt strikes the ground there ("Impact", Impacts "Stone").
    BoltGround([f32; 3]),
    /// A turret's head starts or stops turning there ("Turret_Mvt_Start" / "_Stop").
    TurretMove([f32; 3], bool),
}

/// Event lines are printed unless [`QUIET`] is set.
pub static QUIET: AtomicBool = AtomicBool::new(false);

pub fn log(t: f32, s: String) {
    if !QUIET.load(Ordering::Relaxed) {
        println!("{t:6.2}s {s}");
    }
}

/// Two fighters at once.
pub fn pair_mut(f: &mut [Fighter], a: usize, b: usize) -> (&mut Fighter, &mut Fighter) {
    assert_ne!(a, b);
    if a < b {
        let (x, y) = f.split_at_mut(b);
        (&mut x[a], &mut y[0])
    } else {
        let (x, y) = f.split_at_mut(a);
        (&mut y[0], &mut x[b])
    }
}

/// Whether `b` is someone `a` fights: in the fight, standing, of a team `a` is hostile to.
pub fn opponent(a: &Fighter, b: &Fighter) -> bool {
    b.active && !b.down && hostile(a, b)
}

/// Whether `a` is hostile to `b` (Character vtable 0x3e8): of a team `a` fights.
pub fn hostile(a: &Fighter, b: &Fighter) -> bool {
    b.team != a.team && a.foes.as_ref().is_none_or(|v| v.contains(&b.team))
}

/// Fighter `i`'s facing on the ground (a unit vector): toward `toward`, else its target (its
/// focus, its combat target), else the nearest standing enemy; None with none.
pub fn facing(f: &[Fighter], i: usize, toward: Option<usize>) -> Option<[f32; 3]> {
    let o = toward.or(f[i].focus).or(f[i].combat.target).or_else(|| nearest_enemy(f, i)).filter(|&o| o != i)?;
    let (a, b) = (f[i].pos(), f[o].pos());
    let d = [b[0] - a[0], 0.0, b[2] - a[2]];
    let l = (d[0] * d[0] + d[2] * d[2]).sqrt();
    (l > 1e-6).then(|| d.map(|v| v / l))
}

/// The target-scoring constants a character's stats start with (CharStats::fromCharacter
/// F040461, +0x154..+0x16c): the bonus for one's current target, the moving penalty, the
/// distance term's weight and reach (mode 2), the penalty per attacker already on a target,
/// the bonus (a penalty) for one not fighting.
pub const SCORE_TARGET: f32 = 5.0;
pub const SCORE_MOVING: f32 = 1.0;
pub const SCORE_DISTANCE_MULT: f32 = 2.0;
pub const SCORE_DISTANCE_REACH: f32 = 200.0;
pub const SCORE_ATTACKER: f32 = 1.0;
pub const SCORE_NOT_FIGHTING: f32 = -2.0;
/// F038638: how far the player's people look for targets (units), and how near one must be for
/// a character on hold to count it fully.
pub const PLAYER_LEASH: f32 = 400.0;
pub const HOLD_NEAR: f32 = 40.0;
/// The AI's two leash radii (AI::create F040019: +8 = 18, +0xc = 6) that make the distance
/// term's reach in its default mode 0 ((18 + 6) x 2, F038607).
pub const AI_RADII: (f32, f32) = (18.0, 6.0);

/// @fn F038638
///
/// The enemy `me` should fight: each standing enemy scored, the best above -9999 chosen.
/// Score: for `current` (the target held) +5, x4 while in a fight (Character +0xe0, its combat
/// mode); -1 when it is moving; minus 1 per fighter already attacking it (not counting me), a
/// taunting one (CharStats +0x12a) +2 with the attackers weighing 0.35; -1 for a passive one
/// (+0x12c). The player's people leave targets beyond their leash: 400 units, or in ranged mode
/// (+0x129) with a crossbow its range (F045489, F033839). On hold (+0x12b) one farther than 40
/// counts a hundredth. Then the distance term 1 - d / reach (F038607, reach (18 + 6) x 2 in the
/// AI's default mode 0); +10 for its AI task's own target while under two attack it; -2 for
/// one not in a fight, halved again when standing still; x0.1 for one getting up (Task_GetUp);
/// x3 for one aiming a crossbow (F033834); -10 for one knocked over or crippled (F040412).
/// One its AI ignores (F038789) is passed over.
/// Only those inside the hold leash count (F038560: from the hold position, 80 units, 140 for one
/// fighting toward me; beyond it only within 60 of me; a helpless one within 40); the distance
/// term counts x20 while I lie knocked over. (F038638's faction-relation term is only for
/// Attacking trouble makers' finder, F050280, never in a fight.) Teams stand in for factions.
pub fn score_targets(f: &[Fighter], me: usize, current: Option<usize>) -> Option<usize> {
    let p = f[me].pos();
    let reach = (AI_RADII.0 + AI_RADII.1) * 2.0;
    let mine = f[me].targeting;
    let mut best: Option<(usize, f32)> = None;
    for o in (0..f.len()).filter(|&o| o != me && opponent(&f[me], &f[o]) && !f[me].ignored.contains(&o)) {
        let c = &f[o];
        let theirs = c.targeting;
        let mut score = 0.0;
        if Some(o) == current {
            score += if f[me].active { SCORE_TARGET * 4.0 } else { SCORE_TARGET };
        }
        let v = c.mover.velocity;
        let moving = (v[0] * v[0] + v[2] * v[2]).sqrt() > 0.05;
        if moving {
            score -= SCORE_MOVING;
        }
        let mut n = c.combat.attackers.len() as f32;
        if c.combat.attackers.contains(&me) {
            n -= 1.0;
        }
        let mut weight = 1.0;
        if theirs.stance.taunt {
            score += 2.0;
            weight = 0.35;
        }
        if theirs.stance.passive {
            score -= 1.0;
        }
        score -= n * SCORE_ATTACKER * weight;
        // the AI's task target, while under two attack it
        if mine.task_target == Some(o) && (c.combat.attackers.len() as f32) < 2.0 {
            score += 10.0;
        }
        let q = c.pos();
        let d = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt();
        // F038560, the hold leash
        if let Some(h) = mine.hold_at {
            let sq = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
            let helpless = theirs.knocked || c.down;
            let toward = c.active && c.focus == Some(me);
            if helpless && sq(q, p) > 1600.0 {
                continue;
            }
            let leash = if toward { 140.0 } else { 80.0 };
            if leash * leash < sq(h, q) && sq(q, p) > 3600.0 {
                continue;
            }
        }
        // the player's leash
        if mine.player {
            let leash = match (&f[me].gun, mine.stance.ranged) {
                (Some(g), true) => g.range,
                _ => PLAYER_LEASH,
            };
            if d > leash {
                continue;
            }
        }
        if mine.stance.hold && d > HOLD_NEAR {
            score *= 0.01;
        }
        score += (1.0 - d / reach) * if mine.knocked { 20.0 } else { 1.0 };
        if !c.active {
            score += SCORE_NOT_FIGHTING;
            if !moving {
                score *= 0.5;
            }
        }
        if theirs.getting_up {
            score *= 0.1;
        }
        if c.gun.is_some() && c.shooting {
            score *= 3.0;
        }
        if theirs.knocked || theirs.crippled {
            score -= 10.0;
        }
        if -9999.0 < score && best.is_none_or(|(_, b)| b < score) {
            best = Some((o, score));
        }
    }
    best.map(|(o, _)| o)
}

/// (harness) The nearest standing fighter of another team.
pub fn nearest_enemy(f: &[Fighter], me: usize) -> Option<usize> {
    let p = f[me].pos();
    (0..f.len())
        .filter(|&o| o != me && opponent(&f[me], &f[o]))
        .min_by(|&a, &b| {
            let d = |o: usize| {
                let q = f[o].pos();
                (q[0] - p[0]).powi(2) + (q[2] - p[2]).powi(2)
            };
            d(a).total_cmp(&d(b))
        })
}

/// One fighter's game side for one tick.
pub struct Ctx<'a> {
    pub me: usize,
    pub f: &'a mut [Fighter],
    pub sh: &'a mut Shared,
}

impl Ctx<'_> {
    /// Who I measure against: my target, else the nearest standing enemy, else myself.
    fn other(&self) -> usize {
        self.f[self.me]
            .focus
            .or_else(|| nearest_enemy(self.f, self.me))
            .unwrap_or(self.me)
    }
    fn dist(&self) -> f32 {
        let (a, b) = (self.f[self.me].pos(), self.f[self.other()].pos());
        ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }
    /// Where I steer to reach `who`: the corner of my path there when the way is blocked, else
    /// where it stands.
    fn steer_point(&self, who: usize) -> [f32; 3] {
        self.f[self.me].via.unwrap_or_else(|| self.f[who].pos())
    }
    /// The direction from me toward `who` (by my path when the way is blocked).
    fn toward(&self, who: usize) -> [f32; 3] {
        let (a, b) = (self.f[self.me].pos(), self.steer_point(who));
        let d = [b[0] - a[0], 0.0, b[2] - a[2]];
        let l = (d[0] * d[0] + d[2] * d[2]).sqrt().max(1e-6);
        d.map(|v| v / l)
    }
    fn play(&mut self, kind: Kind, name: &str, speed: f32) {
        let len = self.sh.clip_len(&self.f[self.me], name);
        let start = self.f[self.me].pos();
        self.f[self.me].act = Some(Act {
            kind,
            clip: SingleAnimation::new(name, len, speed, 1.0, false, vec![]),
            start,
        });
    }

    /// CombatClass::tryEngage (F039834): close enough to ask the target (F040732), whose answer
    /// becomes my desired state.
    fn try_engage(&mut self, c: &mut CombatClass<usize>) -> Option<SwordState> {
        let target = c.target?;
        let me = self.me;
        let approach = Approach {
            attackable: self.f[target].combat.accept_engager(me),
            distance: self.dist(),
            target_radius: self.f[target].radius,
            max_reach: self.f[self.me].max_reach,
            reach_margin: c.reach_margin,
            in_hold_area: true,
            // F029592: in the block stance (CharStats +0x128, not prone) it never asks
            busy: {
                let t = self.f[self.me].targeting;
                t.stance.block && !t.knocked && !self.f[self.me].prone()
            },
        };
        if can_engage(&approach) {
            let me = self.me;
            let my_skill = self.f[me].attack_level();
            let r1 = self.sh.rng.next();
            let r2 = self.sh.rng.next();
            let (cf, max_slots) = (self.sh.chance_factor, self.sh.max_slots);
            let tf = &mut self.f[target];
            let asked = Asked {
                fighting: true,
                state: tf.combat.state,
                desired: tf.combat.desired,
                progress_at_asker: if tf.combat.target == Some(me) {
                    tf.combat.swing_progress
                } else {
                    0.0
                },
                staggered: matches!(&tf.act, Some(Act { kind: Kind::Stumble(_), clip, .. }) if clip.playing),
                has_target: tf.combat.target.is_some(),
                targets_asker: tf.combat.target == Some(me),
                reacting: tf.combat.waiting,
                attackers: tf.combat.attackers.len(),
                // F029592 on the asked: in the block stance, not prone
                passive: tf.targeting.stance.block && !tf.targeting.knocked && !tf.prone(),
                asker_wins_contest: !crate::states::attack_contest(tf.attack_level(), my_skill, cf, r1, r2),
                outside_hold: false,
                flagged: false,
                acting: tf.act.is_some(),
            };
            let count = slot_count(tf.combat.state == SwordState::Attack, false, max_slots, tf.extra_slots);
            let slots = &mut tf.slots;
            let (granted, u) =
                request_attack(&asked, || slots.add(me, count), tf.combat.recovery <= 0.0);
            if u.defending {
                tf.combat.defending = true;
            }
            if let Some(d) = u.desired {
                tf.combat.desired = d;
            }
            if u.engage {
                tf.combat.state = SwordState::Engaged;
            }
            if u.reset_recovery {
                tf.combat.recovery = 0.6;
            }
            c.desired = granted as i32;
            Some(granted)
        } else {
            None
        }
    }

    /// AttackState::canEnter + tryAttack (F029603, F029616): start an attack, or run at the
    /// target.
    fn attack_enter(&mut self, c: &mut CombatClass<usize>) -> bool {
        let Some(target) = c.target else { return false };
        if !c.attackers.is_empty() || !self.f[target].combat.is_engager(self.me) {
            return false;
        }
        let me = &self.f[self.me];
        let other = &self.f[target];
        // F047076: the moves by the weapon skill (CharStats vtable 0x18), the load (+0x190), the
        // size (vtable 0x390); unarmed fallbacks by martial arts (F047041)
        let att = Attacking {
            category: me.wielded.category,
            skill: me.weapon_skill(),
            load: me.load,
            prone: me.prone(),
            size: me.body.size,
            distance: crate::states::attack_gap(self.dist(), c.radius, me.body.size, other.radius),
            extra_reach: extra_reach(me.wielded.length),
            repeat_ok: false,
            allow_weak: false,
        };
        let last = me.last_attack.map(|k| &self.sh.attacks[k]);
        let current = match me.act.as_ref().map(|a| a.kind) {
            Some(Kind::Attack(k)) => Some(&self.sh.attacks[k]),
            Some(Kind::Defence(k)) => Some(&self.sh.defences[k]),
            _ => last,
        };
        let mut max_reach = me.max_reach;
        let rng = &mut self.sh.rng;
        let swing = try_attack(
            &self.sh.attacks,
            &att,
            me.body.animal,
            me.martial(),
            current,
            other.combat.state,
            me.attack_speed,
            &mut max_reach,
            |_| 1.0,
            |x, y| rng.range(x, y),
        );
        self.f[self.me].max_reach = max_reach;
        if std::env::var("KENSHI_SWING_DEBUG").is_ok() && matches!(swing, Swing::Approach) && (self.sh.t * 2.0) as i32 != ((self.sh.t - 1.0 / 60.0) * 2.0) as i32 {
            let unarmed: Vec<(f32, f32, f32)> = self.sh.attacks.iter().filter(|t| t.allowed.contains(&WeaponCategory::SkillUnarmed)).map(|t| (t.attack_distance, t.min_skill, t.max_skill)).collect();
            eprintln!("swing: {} no attack: dist {:.1} gap {:.1} radii {:.1}/{:.1} skill {:.1} cat {:?} unarmed {:?}", self.f[self.me].name, self.dist(), att.distance, c.radius, self.f[target].radius, att.skill, att.category, unarmed);
        }
        match swing {
            Swing::Start { technique, speed } => {
                let k = self
                    .sh
                    .attacks
                    .iter()
                    .position(|x| std::ptr::eq(x, technique))
                    .unwrap();
                let name = technique.anim_name.clone();
                log(
                    self.sh.t,
                    format!(
                        "{} attacks with '{}' ({} hits, speed {speed:.2}) from {:.0} units",
                        self.f[self.me].name,
                        name,
                        technique.frames.len(),
                        self.dist()
                    ),
                );
                self.play(Kind::Attack(k), &name, speed);
                if self.sh.cues.len() < 256 {
                    self.sh.cues.push(Cue::Swing(self.me));
                }
                self.stop_moving();
                self.f[self.me].last_attack = Some(k);
                c.frame = 0;
                c.struck = 0;
                true
            }
            Swing::Approach => {
                // F029616: switch to blocking if someone strikes first, else close in by
                // formation steering to half my radius .. half my circling distance
                c.switch_to_block(self, 0.5, 0.5);
                let me = &self.f[self.me];
                let outer = circle_distance(me.max_reach, c.radius, extra_reach(me.wielded.length));
                let band = Band::new(c.radius * 0.5, outer * 0.5, me.radius, 0.0);
                // (on my way round an obstacle: to the corner of my path, not a ring round the target)
                let band = if me.via.is_some() { Band::new(0.0, 0.0, 0.0, 0.0) } else { band };
                let (pos, tpos, tr, run) = (
                    me.pos(),
                    self.steer_point(target),
                    self.f[target].radius,
                    me.run,
                );
                let (v, _) = steer_band(
                    &mut self.f[self.me].steering,
                    &band,
                    pos,
                    tpos,
                    tr,
                    run,
                    200.0,
                    1.0 / 60.0,
                );
                self.f[self.me].order = Order::Drive(v);
                false
            }
        }
    }
    /// One swing target's blow (F039803's loop) and its onMeleeHit (F033862): the defence
    /// (defendOutcome F040724: an animal can't defend; a dodge in time, a block of the right side
    /// late enough), my recovery by it (F047022: hit 0.6, blocked 0.3, dodged 0.2) and my training
    /// (F047467: landed 0, blocked 1, dodged 6); one not dodged leaves my engagers (F040718). A
    /// block: the blocker trains (2) and holds (F040711: re-check 0.01, recovery 0.2). A hit:
    /// attackDamage (F047066) against it times the frame's power through resolveHit (F040476),
    /// the struck one trains (4) on the blow as it came, then its hit reaction (onHit F033861).
    /// Last, an unarmed blow hurts my own limb (punchFeedback F040731).
    fn strike_one(&mut self, c: &mut CombatClass<usize>, tech: &CombatTechnique, frame: i32, o: usize) {
        let t = self.sh.t;
        let dir = tech.frame(frame).map_or(1, |f| f.direction);
        // the struck one's facing: toward its own target, else (assumption) toward me
        let their_face = facing(self.f, o, None).filter(|_| self.f[o].focus.or(self.f[o].combat.target).is_some());
        let (me, other) = pair_mut(self.f, self.me, o);
        // the struck one hears of it (its AI fights back)
        other.active = true;
        let (action, dprog) = match &other.act {
            Some(Act { kind: Kind::Defence(d), clip, .. }) => {
                let dt_ = &self.sh.defences[*d];
                let a = if dt_.is_dodge {
                    Action::Dodge
                } else {
                    Action::Block { direction: dt_.frame(i32::MAX).map_or(0, |f| f.direction) }
                };
                (a, clip.progress)
            }
            _ => (Action::Other, 0.0),
        };
        // F033811: from behind, against the struck one's facing (toward its own target, else
        // (assumption) toward me)
        let behind = {
            let (a, b) = (me.pos(), other.pos());
            let to_me = [a[0] - b[0], 0.0, a[2] - b[2]];
            let l = (to_me[0] * to_me[0] + to_me[2] * to_me[2]).sqrt().max(1e-6);
            hit_from_behind(their_face.unwrap_or(to_me.map(|v| v / l)), to_me)
        };
        let outcome = defend_outcome(other.body.animal, action, dprog, dir, behind);
        c.recovery = recovery_time(outcome);
        if outcome != Defence::Dodged {
            c.engagers.retain(|&e| e != o);
        }
        // the blow as it comes (attackDamage against the struck one, times the frame's power)
        let att = me.wielded.attacker(
            me.weapon_skill(),
            (me.level(StatsEnumerated::StatDexterity), me.fitness(kenshi_char::fitness::DEXTERITY)),
            (me.strength_level(), me.fitness(kenshi_char::fitness::STRENGTH)),
            me.body.maturity,
        );
        let un = Unarmed {
            level: me.martial(),
            toughness: me.level(StatsEnumerated::StatToughness),
            toughness_scale: me.body.maturity,
            unarmed_damage_mult: self.sh.unarmed_mult,
            limbs: me.fitness(kenshi_char::fitness::STRENGTH),
            char_factor: me.strength_level(),
        };
        let kind = if other.body.robot {
            TargetKind::Robot
        } else if other.body.animal {
            TargetKind::Animal
        } else {
            TargetKind::Human
        };
        let base = attack_damage(&me.wielded, &att, &self.sh.cc, &un, Some(Target { kind, race: &other.body.race }), me.gear.damage_output);
        let hit = frame_damage(base, tech, frame);
        let incoming = hit.cut + hit.blunt + hit.pierce;
        let xp = XpConstants { global: self.sh.damage_mult, ..self.sh.xp };
        let (ta, td) = (me.training(other), other.training(me));
        let struck = match outcome {
            Defence::Dodged => {
                me.skills.train_from_combat(CombatEvent::Dodged, &ta, &xp);
                log(t, format!("{} dodges {}'s '{}'", other.name, me.name, tech.anim_name));
                Struck::Soft
            }
            Defence::Blocked => {
                me.skills.train_from_combat(CombatEvent::Blocked, &ta, &xp);
                other.skills.train_from_combat(CombatEvent::Parried, &td, &xp);
                other.combat.block_timer = 0.01;
                other.combat.recovery = 0.2;
                log(t, format!("{} blocks {}'s '{}'", other.name, me.name, tech.anim_name));
                Struck::Blocked
            }
            Defence::Hit => {
                me.skills.train_from_combat(CombatEvent::Landed, &ta, &xp);
                let r = self.hit(o, hit, tech, dir, behind, incoming, &td);
                Struck::from_material(r)
            }
        };
        // punchFeedback (F040731): an unarmed blow, not by a robot, hurts the striking limb; a
        // robot struck counts as plate; a dodged blow as soft; trains toughness by the outcome
        // (F047466)
        let me = &mut self.f[self.me];
        let s = Striker {
            category: me.wielded.category,
            robot: me.body.robot,
            martial_arts: me.martial(),
            toughness: me.level(StatsEnumerated::StatToughness),
            toughness_scale: me.body.maturity,
            self_damage_mult: me.gear.fist_injury,
            damage_mult: self.sh.damage_mult,
        };
        let slot = tech.frame(frame).map_or(4, |f| f.limb);
        let victim_robot = self.f[o].body.robot;
        let me = &mut self.f[self.me];
        if let Some((part, total)) = punch_feedback(&mut me.medical, &s, struck, victim_robot, slot, &self.sh.mc) {
            let kind = match if victim_robot { Struck::Plate } else { struck } {
                Struck::Plate => ToughnessKind::Kind1,
                Struck::Blocked => ToughnessKind::Kind5,
                _ => ToughnessKind::Other,
            };
            me.skills.train_toughness(kind, self.sh.xp.xp_rate_toughness, self.sh.xp.exp_gain * 0.1, self.sh.damage_mult);
            if 0.0 < total {
                log(t, format!("{} hurts its own {} ({total:.1})", me.name, me.medical.parts[part].id));
            }
        }
    }

    /// The hit part of onMeleeHit (F033862) on `o` from me: resolveHit (F040476) on its body and
    /// armour, the damage floater and sound, the knockback kept for its body, its training on the
    /// blow (`incoming`: cut + blunt + pierce as it came), its hit reaction (onHit F033861: a
    /// stumble when the hit beats its stagger threshold or nobody else attacks it; not an animal)
    /// and a knockout. Returns the hit material (-1 flesh).
    #[allow(clippy::too_many_arguments)]
    fn hit(&mut self, o: usize, hit: crate::hit::Hit, tech: &CombatTechnique, dir: i32, behind: bool, incoming: f32, td: &CombatTraining) -> i32 {
        let t = self.sh.t;
        let xp = XpConstants { global: self.sh.damage_mult, ..self.sh.xp };
        let (me, other) = pair_mut(self.f, self.me, o);
        let ctx = HitContext {
            low_strike: tech.low_strike,
            direction: dir,
            robot: other.body.robot,
            animal: other.body.animal,
            damage_mult: self.sh.damage_mult,
            race_bleed: other.body.bleed,
        };
        let rng = &mut self.sh.rng;
        let r = resolve_hit(&mut other.medical, hit, &other.armour, &other.stats, &ctx, &self.sh.hc, &self.sh.mc, |a, b| rng.range(a, b));
        if self.sh.hits.len() < 256 {
            self.sh.hits.push((self.me, o, r.part, r.floater));
        }
        if self.sh.cues.len() < 256 {
            let f = &r.floater;
            self.sh.cues.push(Cue::Hit(self.me, o, f.cut + f.blunt + f.pierce));
        }
        // the knockback (F040476: the blunt damage x 1000 along the attack's direction) kept for
        // the body: a blow that knocks out throws the ragdoll with it.
        // (assumption) along the ground it pushes away from the attacker
        {
            let (a, b) = (me.pos(), other.pos());
            let (dx, dz) = (a[0] - b[0], a[2] - b[2]);
            let l = (dx * dx + dz * dz).sqrt().max(1e-6);
            let d = crate::hit::knockback_dir(dir, [dx / l, 0.0, dz / l]);
            let d = if matches!(dir, 1 | 5) { d } else { [-d[0], -d[1], -d[2]] };
            other.knock = d.map(|v| v * r.knockback);
            other.knock_at = t;
        }
        let part = &other.medical.parts[r.part];
        log(
            t,
            format!(
                "{} hits {} with '{}': part {} (kind {}) cut {:.1} blunt {:.1} -> health {:.1}/{:.0}",
                me.name,
                other.name,
                tech.anim_name,
                r.part,
                part.kind,
                r.damage.cut,
                r.damage.blunt,
                part.health(),
                part.max_health()
            ),
        );
        if !QUIET.load(Ordering::Relaxed) {
            for (k, pt) in other.medical.parts.iter().enumerate() {
                println!("hp,{t},{},{k},{}", other.name, pt.health());
            }
        }
        // the struck one trains on the blow as it came (F033862 keeps a copy of it)
        other.skills.train_from_combat(CombatEvent::Hit { damage: incoming }, td, &xp);
        // the hit reaction (Character::onHit, F033861)
        let total = r.damage.cut + r.damage.blunt + r.damage.pierce + r.damage.stun;
        on_hit(other, &mut self.sh, total, r.part, behind);
        r.material
    }
}

impl Fighting for Ctx<'_> {
    type Id = usize;

    fn is_down(&self, who: usize) -> bool {
        self.f[who].down
    }
    fn is_animal(&self) -> bool {
        self.f[self.me].body.animal
    }
    fn staggered(&self) -> bool {
        matches!(&self.f[self.me].act, Some(Act { kind: Kind::Stumble(_), clip, .. }) if clip.playing)
    }
    fn collision_radius(&self) -> f32 {
        self.f[self.me].radius
    }
    fn size(&self) -> f32 {
        self.f[self.me].body.size
    }
    fn weapon_length(&self) -> f32 {
        self.f[self.me].wielded.length
    }
    fn action(&self) -> Option<&CombatTechnique> {
        match self.f[self.me].act.as_ref()?.kind {
            Kind::Attack(k) => Some(&self.sh.attacks[k]),
            Kind::Defence(k) => Some(&self.sh.defences[k]),
            Kind::Stumble(_) => None,
        }
    }
    fn action_progress(&self) -> f32 {
        match &self.f[self.me].act {
            Some(a) if !matches!(a.kind, Kind::Stumble(_)) => a.clip.progress,
            _ => 0.0,
        }
    }
    fn action_playing(&self) -> bool {
        matches!(&self.f[self.me].act, Some(a) if !matches!(a.kind, Kind::Stumble(_)) && a.clip.playing)
    }
    fn action_clip_progress(&self) -> f32 {
        self.action_progress()
    }
    fn stop_action(&mut self) {
        if !matches!(
            self.f[self.me].act.as_ref().map(|a| a.kind),
            Some(Kind::Stumble(_))
        ) {
            self.f[self.me].act = None;
        }
    }
    fn progress_at_me(&self, who: usize) -> f32 {
        let c = &self.f[who].combat;
        if c.target == Some(self.me) {
            c.swing_progress
        } else {
            0.0
        }
    }
    fn is_blocking(&self, who: usize) -> bool {
        matches!(
            self.f[who].act.as_ref().map(|a| a.kind),
            Some(Kind::Defence(_))
        )
    }
    fn frame_of(&self, who: usize) -> i32 {
        self.f[who].combat.frame
    }
    /// CombatClass::findTarget (F040734): the current target is kept while picked under a
    /// second ago (two on hold, CharStats +0x12b); else the best scored standing enemy
    /// (F038638, [`score_targets`]).
    fn find_target(&mut self) -> Option<usize> {
        let me = &self.f[self.me];
        let keep = if me.targeting.stance.hold { 2.0 } else { 1.0 };
        if let Some(t) = me.focus
            && me.pick_timer < keep
            && !self.f[t].down
        {
            return Some(t);
        }
        self.f[self.me].pick_timer = 0.0;
        let t = score_targets(self.f, self.me, self.f[self.me].focus);
        self.f[self.me].focus = t;
        t
    }
    /// The standing enemies around me (CombatClass +0x208): those targeting me with their swing.
    fn candidates(&self) -> Vec<(usize, bool, f32)> {
        (0..self.f.len())
            .filter(|&o| o != self.me && opponent(&self.f[self.me], &self.f[o]))
            .map(|o| {
                let c = &self.f[o].combat;
                (o, c.target == Some(self.me), c.swing_progress)
            })
            .collect()
    }

    /// CombatClassAI::idle (F039837) with tryEngage (F039834) asking the target (F040732).
    fn idle(&mut self, c: &mut CombatClass<usize>) {
        if self.f[self.me].act.is_some() {
            return;
        }
        if c.target.is_none() {
            c.state = SwordState::TargetLost;
            return;
        }
        c.desired = 5;
        c.recovery = 0.5;
        c.state = SwordState::Engaged;
        let engage = self.try_engage(c);
        let choice = idle(
            false,
            true,
            true,
            engage,
            !c.attackers.is_empty(),
            c.attackers.is_empty(),
            false,
        );
        if let IdleChoice::Engage { desired } = choice {
            if desired == SwordState::Circle && c.desired != 5 {
                log(
                    self.sh.t,
                    format!("{} is told to wait its turn", self.f[self.me].name),
                );
            }
            c.desired = desired as i32;
        }
    }

    /// CombatClassAI::engagedStep (F039848).
    fn engaged(&mut self, c: &mut CombatClass<usize>) {
        let desired = SwordState::from_i32(c.desired);
        let start = match desired {
            Some(SwordState::Attack) => Some(self.attack_enter(c)),
            // CircleStateAI::enter (F029598) and WaitingStateAI::enter (F029601) always start:
            // the timer (the orbit offset is not used here); waiting also stops the move
            Some(SwordState::Circle) => {
                self.f[self.me].circle_timer = self.sh.rng.next() * 3.0 + 1.5;
                Some(true)
            }
            Some(SwordState::Waiting) => {
                if self.f[self.me].act.is_some() {
                    self.stop_action();
                }
                self.stop_moving();
                Some(true)
            }
            _ => None,
        };
        let reacted =
            desired == Some(SwordState::Block) && c.reaction < 0.0 && self.react(c, false);
        let dt = 1.0 / 60.0;
        let mut recovery = c.recovery;
        let step = engaged_step(
            c.target.is_some(),
            true,
            desired.unwrap_or(SwordState::Circle),
            start,
            c.reaction,
            reacted,
            &mut recovery,
            dt,
        );
        c.recovery = recovery;
        match step {
            EngagedStep::Enter(s) => c.state = s,
            EngagedStep::Idle => c.set_state(self, SwordState::Idle),
            EngagedStep::TargetLost => c.state = SwordState::TargetLost,
            EngagedStep::Disengage => c.state = SwordState::Disengage,
            EngagedStep::Hold => {
                if desired == Some(SwordState::Block) {
                    self.stop_moving();
                } else if let Some(t) = c.target {
                    self.close_on(t, false);
                }
            }
        }
    }

    /// CircleStateAI::update (F029599): hold the band around the target until attacked. One in
    /// the block stance (CharStats +0x128, not prone) never asks in: it keeps circling, its
    /// timer clearing its attacker slots now and then, until someone attacks it.
    fn circle(&mut self, c: &mut CombatClass<usize>, dt: f32) {
        let Some(target) = c.target else {
            c.set_state(self, SwordState::Idle);
            return;
        };
        let me = &self.f[self.me];
        let outer = circle_distance(me.max_reach, c.radius, extra_reach(me.wielded.length));
        let band = Band::new(c.radius, outer, me.radius, 0.0);
        // (on my way round an obstacle: to the corner of my path, not a ring round the target)
        let band = if me.via.is_some() { Band::new(0.0, 0.0, 0.0, 0.0) } else { band };
        let (pos, tpos, tr, run) = (
            me.pos(),
            self.steer_point(target),
            self.f[target].radius,
            me.run,
        );
        let (v, _) = steer_band(
            &mut self.f[self.me].steering,
            &band,
            pos,
            tpos,
            tr,
            run,
            200.0,
            dt,
        );
        self.f[self.me].order = Order::Drive(v);
        if !c.attackers.is_empty() {
            c.set_state(self, SwordState::Idle);
            self.stop_moving();
        }
        let mine = self.f[self.me].targeting;
        let timer = &mut self.f[self.me].circle_timer;
        *timer -= dt;
        if !mine.stance.block || mine.knocked {
            // ask again; once the target holds a slot for me, go back to idle to attack
            if self.try_engage(c).is_some() {
                let (me, max_slots) = (self.me, self.sh.max_slots);
                let tf = &mut self.f[target];
                let count = slot_count(tf.combat.state == SwordState::Attack, false, max_slots, tf.extra_slots);
                if tf.slots.add(me, count) {
                    c.set_state(self, SwordState::Idle);
                }
            }
        }
        // in the block stance the timer clears my attacker slots (F040719); otherwise they are
        // let go when a swing ends (F039804) or its swinger is struck (F039803)
        let timer = &mut self.f[self.me].circle_timer;
        if *timer < 0.0 && mine.stance.block && !mine.knocked {
            // the timer restarts and my attacker slots open up again (F040719)
            *timer = self.sh.rng.next() * 3.0 + 1.5;
            self.f[self.me]
                .slots
                .slots
                .iter_mut()
                .for_each(|s| *s = None);
        }
    }

    /// WaitingStateAI::update (F029602): hover around the target at a distance that grows with
    /// its crowd; once the target accepts me as an engager, go back to idle (not in the block
    /// stance, CharStats +0x128, unless prone); attacked -> idle.
    fn wait(&mut self, c: &mut CombatClass<usize>, dt: f32) {
        self.stop_moving();
        if let Some(t) = self.find_target() {
            c.target = Some(t);
        }
        if let Some(target) = c.target {
            let crowd = self.f[target].combat.crowd() as f32;
            let me = &self.f[self.me];
            let band = Band::new(crowd * 0.25 + 15.0, crowd * 3.0 + 25.0, me.radius, 0.0);
            // (on my way round an obstacle: to the corner of my path, not a ring round the target)
            let band = if me.via.is_some() { Band::new(0.0, 0.0, 0.0, 0.0) } else { band };
            let (pos, tpos, tr, run) = (
                me.pos(),
                self.steer_point(target),
                self.f[target].radius,
                me.run,
            );
            let (v, _) = steer_band(
                &mut self.f[self.me].steering,
                &band,
                pos,
                tpos,
                tr,
                run,
                200.0,
                dt,
            );
            self.f[self.me].order = Order::Drive(v);
            let me = self.me;
            let mine = self.f[me].targeting;
            if (!mine.stance.block || mine.knocked) && self.f[target].combat.accept_engager(me) {
                if c.state != SwordState::Locked {
                    if self.f[self.me].act.is_some() {
                        self.stop_action();
                    }
                    c.state = SwordState::Idle;
                }
                return;
            }
        }
        if !c.attackers.is_empty() {
            c.set_state(self, SwordState::Idle);
            self.stop_moving();
        }
    }

    /// CombatClass::strike (F039803) with its swing targets (F039793): the swing box ahead of me
    /// (along my facing, toward my target); each character whose hull is in it that is my enemy
    /// (Character vtable 0x3e8) or my target, nearest first ((assumption) the physics query's
    /// order), up to the move's "max simultaneous hits", takes the frame's blow
    /// ([`Ctx::strike_one`]).
    /// (not ported) The query also returns buildings, which the blow damages.
    fn strike(&mut self, c: &mut CombatClass<usize>, frame: i32) {
        let Some(Kind::Attack(k)) = self.f[self.me].act.as_ref().map(|a| a.kind) else {
            return;
        };
        let me = self.me;
        let tech = self.sh.attacks[k].clone();
        let Some(face) = facing(self.f, me, c.target) else { return };
        let f = &self.f[me];
        // the box: from the move's attack distance less what the lunge already covered, pulled
        // back
        let start = f.act.as_ref().map_or(f.pos(), |a| a.start);
        let p = f.pos();
        let moved = ((p[0] - start[0]).powi(2) + (p[2] - start[2]).powi(2)).sqrt();
        let leg = tech.frame(frame).is_some_and(|f| f.limb == 2 || f.limb == 3);
        let reach = pull_back(swing_box(f.wielded.length, f.radius, f.body.size, leg, tech.attack_distance, moved), f.radius);
        let mut inside: Vec<(f32, usize)> = (0..self.f.len())
            .filter(|&o| o != me && !self.f[o].down && (Some(o) == c.target || hostile(&self.f[me], &self.f[o])))
            .filter_map(|o| {
                let q = self.f[o].pos();
                let (dx, dz) = (q[0] - p[0], q[2] - p[2]);
                let forward = dx * face[0] + dz * face[2];
                let side = dx * face[2] - dz * face[0];
                // the box meets the character's physics hull (F040654), (assumption) as a
                // circle of its larger half width
                let h = self.f[o].hull();
                in_swing_box(&reach, side, forward, h[0].max(h[2])).then(|| (dx * dx + dz * dz, o))
            })
            .collect();
        inside.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, o) in inside.iter().take(tech.max_simultaneous_hits.max(0) as usize) {
            self.strike_one(c, &tech, frame, o);
        }
    }

    fn commit_point(&self, target: usize) -> f32 {
        let t = &self.f[target];
        CombatClass::<usize>::commit_point(t.defence_level(), self.f[self.me].attack_level(), t.combat.crowd())
    }

    fn leave_attack(&mut self, target: usize) {
        let me = self.me;
        for s in &mut self.f[target].slots.slots {
            if *s == Some(me) {
                *s = None;
            }
        }
    }
    fn keep_attacking(&mut self, _target: usize) {}

    fn close_on(&mut self, who: usize, _far: bool) {
        let me = &self.f[self.me];
        let to = self.toward(who);
        let stand = me.radius + self.f[who].radius + extra_reach(me.wielded.length);
        let d = self.dist();
        let go = (d - stand).max(0.0);
        let p = me.mover.position;
        self.f[self.me].order =
            Order::Goal([p[0] + to[0] * go * 0.1, 0.0, p[2] + to[2] * go * 0.1]);
    }
    fn stop_moving(&mut self) {
        self.f[self.me].order = Order::Hold;
    }

    /// CombatClass::reactToAttack (F039797).
    fn react(&mut self, c: &mut CombatClass<usize>, dodging: bool) -> bool {
        let Some(attacker) = c.nearest_threat(self, 0.0) else {
            return false;
        };
        let progress = self.progress_at_me(attacker);
        if self.is_blocking(attacker) || 1.0 <= progress {
            return false;
        }
        if !dodging {
            c.state = SwordState::Block;
        }
        let skill = self.f[attacker].attack_level();
        let needed = self.needed_block(attacker);
        let threat = Threat {
            progress,
            attacker_blocking: false,
            related: true,
            direction: needed,
            skill,
        };
        // F047075: a wrong (same-side) block when the defence chance (F047063), x the unarmed
        // attacker factor against a person fighting unarmed, fails its roll; fighting unarmed
        // (or with the second arm out of use, MedicalSystem +0x165) it may dodge when staggered
        // or when the block chance (F047056, the dodge level) passes its roll
        let mine = &self.f[self.me];
        let theirs = &self.f[attacker];
        let k = if !theirs.body.animal && theirs.unarmed() {
            crate::technique::unarmed_attacker_factor(mine.level(StatsEnumerated::StatMartialarts))
        } else {
            1.0
        };
        let sloppy = mine.defence_chance(skill, &self.sh.cc) * k < self.sh.rng.next() * 100.0;
        let category = if mine.medical.arms_usable(mine.body.animal, mine.ko_point()).0 { mine.wielded.category } else { WeaponCategory::SkillUnarmed };
        let staggered = self.staggered();
        let mine_prone = self.f[self.me].prone();
        let dodge_ok = staggered
            || (category == WeaponCategory::SkillUnarmed && self.sh.rng.next() * 100.0 < self.f[self.me].block_chance(skill, &self.sh.cc));
        let d = Defender {
            category,
            prone: mine_prone,
            staggered,
            dodge_ok,
            sloppy,
        };
        let speed = self.f[self.me].defence_speed;
        let animal = self.f[self.me].clips.is_some();
        let defences = &self.sh.defences;
        let rng = &mut self.sh.rng;
        let r = react_to_attack(animal, Some(threat), speed, |e, _| {
            choose_defence(defences, &d, e, |x, y| rng.range(x, y))
        });
        if let Some(timer) = r.timer {
            c.reaction = timer;
        }
        // F039797: no defence found, or a dodge: trainDefence (blocked = a dodge)
        let train = r.train;
        let reacted = match r.reaction {
            Reaction::Defend { technique, speed } => {
                let k = defences
                    .iter()
                    .position(|x| std::ptr::eq(x, technique))
                    .unwrap();
                let name = technique.anim_name.clone();
                let dodge = technique.is_dodge;
                log(
                    self.sh.t,
                    format!(
                        "{} {} with '{}'{}",
                        self.f[self.me].name,
                        if dodge { "dodges" } else { "blocks" },
                        name,
                        if sloppy && !dodge { " (wrong side)" } else { "" }
                    ),
                );
                self.play(Kind::Defence(k), &name, speed);
                if self.sh.cues.len() < 256 {
                    self.sh.cues.push(Cue::Block(self.me, attacker, dodge));
                }
                if dodge {
                    self.stop_moving();
                }
                c.block_timer = 1.0;
                c.threat = Some(attacker);
                c.target = Some(attacker);
                true
            }
            // an animal only turns on its attacker: no action, the re-check timer the reaction
            // factor (F047062), the attacker its threat and its target (F039797)
            Reaction::Animal => {
                c.block_timer = 1.0;
                c.threat = Some(attacker);
                c.target = Some(attacker);
                false
            }
            _ => false,
        };
        if let Some((s, blocked)) = train {
            self.train_defence(s, blocked);
        }
        reacted
    }
    fn needed_block(&self, threat: usize) -> i32 {
        let dir = match self.f[threat].act.as_ref().map(|a| a.kind) {
            Some(Kind::Attack(k)) => self.sh.attacks[k]
                .frame(self.f[threat].combat.frame)
                .map_or(1, |f| f.direction),
            _ => 1,
        };
        let to = self.toward(threat);
        let facing = to;
        block_direction(dir, hit_from_behind(facing, to))
    }
    fn hold_block(&mut self) {}
    fn attack_skill(&self, who: usize) -> f32 {
        self.f[who].attack_level()
    }
    /// F047056 (the locked block roll, F039792).
    fn block_chance(&self, skill: f32) -> f32 {
        self.f[self.me].block_chance(skill, &self.sh.cc)
    }
    /// CharStats::trainDefence (F047459) after the block roll.
    fn train_defence(&mut self, skill: f32, blocked: bool) {
        let xp = self.sh.xp;
        let g2 = self.sh.damage_mult;
        let f = &mut self.f[self.me];
        // F047459: by the buffed dodge level and the load
        let (mine, load) = (f.dodge_level(true), f.load);
        f.skills.train_defence(
            mine,
            skill,
            blocked,
            load,
            xp.exp_gain * 0.1,
            g2,
            (xp.bonus_gap, xp.penalty_gap, 1),
        );
    }
    fn rand01(&mut self) -> f32 {
        self.sh.rng.next()
    }
}

/// Character::onHit (F033861), the hit reaction to a blow or a bolt of `total` damage (cut +
/// blunt + pierce + stun as it landed) on part `part`: a stumble (AnimationClass::pickStumble
/// F037176 by the part and the side) when the damage beats its stagger threshold (F047023:
/// toughness x CharStats +0x34 x 0.01 x "stumble damage max") or nobody else attacks it, big
/// when both; not an animal's, nor one lying prone (vtable 0x368); then a knockout.
pub fn on_hit(other: &mut Fighter, sh: &mut Shared, total: f32, part: usize, behind: bool) {
    let t = sh.t;
    other.stats.toughness = other.level(StatsEnumerated::StatToughness);
    let threshold = stagger_threshold(other.level(StatsEnumerated::StatToughness), other.body.maturity, sh.stumble_max);
    let attacked = !other.combat.attackers.is_empty();
    let part_id = other.medical.parts[part].id.clone();
    if !other.medical.knocked_out
        && !other.body.animal
        && !other.prone()
        && let Some(big) = stagger(total, threshold, attacked)
        && let Some(st) = pick_stumble(
            &sh.stumbles,
            &part_id,
            &situation(other),
            1.0,
            i32::from(behind),
            big,
            other.last_stumble.as_deref(),
            sh.rng.next(),
        )
    {
        let s = sh.stumbles.iter().position(|x| std::ptr::eq(x, st)).unwrap();
        let len = sh.clip_len(other, &st.anim_name);
        log(t, format!("{} staggers: '{}'{}", other.name, st.name, if big { " (big)" } else { "" }));
        other.last_stumble = Some(st.name.clone());
        let start = other.pos();
        other.act = Some(Act { kind: Kind::Stumble(s), clip: SingleAnimation::new(&st.anim_name, len, 1.0, 1.0, false, vec![]), start });
        other.combat.state = SwordState::Locked;
    }
    if other.medical.knocked_out && !other.down {
        let ko = other.medical.compute_knockout_time(&other.stats, &sh.mc);
        log(t, format!("{} is knocked out ({ko:.0} s)", other.name));
        other.down = true;
        other.down_at = t;
    }
}

/// What a fighter is doing, as animation choice reads it: fighting, the weapon in the right
/// hand, its speed and leg health (percent).
pub fn situation(f: &Fighter) -> Situation {
    let leg = |side: i32| {
        f.medical
            .parts
            .iter()
            .find(|p| p.kind == 1 && p.side == side)
            .map_or(100.0, |p| p.health() / p.max_health() * 100.0)
    };
    let v = f.mover.velocity;
    // the animations' category: 8 while aiming a crossbow (the aim clips "aimH", "aimwalk",
    // "stand 1 crossbow", the reloads), else 0; (assumption) one aiming is not in the melee
    // combat mode
    let aiming = f.shooting;
    Situation {
        // F037184: 5 lying prone (the legs collapsed: the crawl clips), 8 aiming, else 0
        category: if f.prone() { 5 } else if aiming { 8 } else { 0 },
        speed: (v[0] * v[0] + v[2] * v[2]).sqrt() * 10.0,
        weapon_l: false,
        weapon_r: true,
        ignore_legs: false,
        // the arm busy with a body (F039254: the left, +0x21e)
        carrying_left: f.carrying,
        carrying_right: false,
        being_carried: false,
        combat_mode: if aiming { 0 } else { 1 },
        stealth_mode: 0,
        r_leg: leg(2),
        l_leg: leg(1),
        weapon_category: f.wielded.category as u32,
    }
}

/// A fighter's base layers (lower and upper, by the animations' layer), its action layer, the
/// move on the action layer, and where it stood last tick.
pub type Layers = (
    [AnimationLayer<()>; 3],
    AnimationLayer<()>,
    Option<String>,
    [f32; 3],
);

impl Arena {
    /// An arena of fighters; `roots` are the skeleton's root-bone tracks by clip (lower case),
    /// `blend_rate` GLOBAL CONSTANTS "animation blend rate".
    pub fn new(
        f: Vec<Fighter>,
        sh: Shared,
        roots: HashMap<String, Vec<(f32, [f32; 2])>>,
        blend_rate: f32,
    ) -> Self {
        let layers = f.iter().map(|x| new_layers(blend_rate, x.pos())).collect();
        Arena {
            f,
            sh,
            layers,
            roots,
            tick: 0,
            t: 0.0,
            rows: false,
            blend_rate,
            bolts: Vec::new(),
            turrets: Vec::new(),
            animal_roots: HashMap::new(),
            obstacles: None,
        }
    }

    /// Add a fighter; its index.
    pub fn add(&mut self, x: Fighter) -> usize {
        self.layers.push(new_layers(self.blend_rate, x.pos()));
        self.f.push(x);
        self.f.len() - 1
    }

    /// Fighter `i` attacks no one: the attacker slots it holds on others (CombatClass +0x10..,
    /// taken by F040727) are let go, as its attack's end lets go of its target's
    /// (`leave_attack`). For one knocked out, out of the fight, or running off through it.
    pub fn release_slots(f: &mut [Fighter], i: usize) {
        for o in 0..f.len() {
            for s in &mut f[o].slots.slots {
                if *s == Some(i) {
                    *s = None;
                }
            }
        }
    }

    /// One tick: every standing fighter with an enemy runs its combat AI (CombatClass), then the
    /// animation layers move the bodies (root motion) or the fighters' orders do.
    pub fn step(&mut self, dt: f32) {
        let Arena {
            f,
            sh,
            layers,
            roots,
            tick,
            t,
            rows,
            bolts,
            obstacles,
            ..
        } = self;
        *t += dt;
        sh.t = *t;
        let t = *t;
        for i in 0..f.len() {
            if f[i].down || !f[i].active {
                continue;
            }
            if f[i].ordered_away {
                if matches!(f[i].act.as_ref().map(|a| a.kind), Some(Kind::Attack(_) | Kind::Defence(_))) {
                    f[i].act = None;
                }
                f[i].order = Order::Hold;
                // (its attack broken off: the slot it held on its target let go)
                Arena::release_slots(f, i);
                continue;
            }
            // CharStats::update (F047065): the speeds by its load, body and water now
            f[i].update_speeds(&sh.strain);
            // a weapon newly wielded: its reach as setWeapon finds it (F047095)
            if f[i].max_reach <= 1.0 {
                f[i].max_reach = weapon_reach(&f[i], &sh.attacks);
            }
            f[i].pick_timer += dt;
            if f[i].focus.is_some_and(|o| f[o].down) {
                f[i].focus = None;
            }
            let Some(other) = f[i].focus.or_else(|| nearest_enemy(f, i)) else {
                continue;
            };
            // a crossbow it can shoot, and no enemy close: ranged combat instead (F033858)
            if f[i].act.is_none() && ranged_usable(&f[i]) && !enemy_close(f, i) {
                if !f[i].shooting {
                    f[i].shooting = true;
                    f[i].ranged.state = 0;
                }
                ranged_tick(f, sh, bolts, obstacles, i, other, dt);
                continue;
            }
            if f[i].shooting {
                f[i].shooting = false;
                // (assumption) a reload broken off by closing to melee starts over
                f[i].ranged.reloading = false;
                f[i].ranged.progress = 0.0;
            }
            // the action's clock
            if let Some(act) = &mut f[i].act {
                act.clip.update(0.0, dt, false);
                if matches!(act.kind, Kind::Stumble(_)) && !act.clip.playing {
                    f[i].act = None;
                }
            }
            // the attack state measures to where a target walking a path will be in 1.2 s
            // (F033809); one steering in formation is taken where it is
            let led = {
                let o = &f[other];
                let mut p = o.pos();
                if !matches!(o.order, Order::Drive(_)) {
                    for (k, v) in p.iter_mut().zip(o.mover.velocity) {
                        *k += v * 10.0 * 1.2;
                    }
                }
                let me = f[i].pos();
                ((p[0] - me[0]).powi(2) + (p[2] - me[2]).powi(2)).sqrt()
            };
            // the combat AI
            f[i].order = Order::Hold;
            let mut c = std::mem::take(&mut f[i].combat);
            let old_target = c.target;
            c.begin_tick();
            {
                let mut ctx = Ctx { me: i, f, sh };
                c.update(&mut ctx, i, dt, led);
            }
            // CombatClass::setTarget (F040712): a new target unregisters me with the old one
            // (F039164 flag 0: out of its engagers); registering with the new is the ported
            // accept_engager
            if let Some(o) = old_target
                && c.target != old_target
            {
                f[o].combat.engagers.retain(|&e| e != i);
            }
            f[i].combat = c;
            f[i].focus = f[i].combat.target.filter(|&o| !f[o].down);
            if std::env::var("DUEL_DEBUG").is_ok() && tick.is_multiple_of(30) {
                eprintln!(
                    "{t:.2} {} state {:?} desired {} target {:?} act {} waiting {} rec {:.2} react {:.2} att {:?} slots {:?}",
                    f[i].name,
                    f[i].combat.state,
                    f[i].combat.desired,
                    f[i].combat.target,
                    f[i].act.is_some(),
                    f[i].combat.waiting,
                    f[i].combat.recovery,
                    f[i].combat.reaction,
                    f[i].combat.attackers,
                    f[i].slots.slots
                );
            }
        }
        // (assumption) a knocked-out fighter drops its target (setTarget to none, F040712), and
        // with it the attacker slot it held
        for i in 0..f.len() {
            if f[i].down
                && let Some(o) = f[i].combat.target.take()
            {
                f[o].combat.engagers.retain(|&e| e != i);
                Arena::release_slots(f, i);
            }
        }
        *tick += 1;
        // the animation layers (F038910) and their root motion: while a clip with root motion
        // plays, the mover follows the change of the layers' weighted root position (turned to
        // the facing) - its 'animation driving' mode (F040617, F040615); otherwise its order
        let roots_now = record(layers, f, sh, roots, dt, t, *tick, *rows);
        for i in 0..f.len() {
            if f[i].down || !f[i].active || f[i].ordered_away {
                continue;
            }
            let Some(other) = f[i].focus.or_else(|| nearest_enemy(f, i)) else {
                continue;
            };
            let (total, driving) = roots_now[i];
            let prev = std::mem::replace(&mut f[i].root_prev, total);
            let len = |v: [f32; 2]| (v[0] * v[0] + v[1] * v[1]).sqrt();
            if driving {
                if -1.0 < len(total) - len(prev) {
                    let (side, fwd) = (total[0] - prev[0], total[1] - prev[1]);
                    let to = {
                        let (a, b) = (f[i].pos(), f[other].pos());
                        [b[0] - a[0], b[2] - a[2]]
                    };
                    let d = len(to).max(1e-6);
                    let (fx, fz) = (to[0] / d, to[1] / d);
                    let mut step = [fx * fwd - fz * side, fz * fwd + fx * side];
                    // the mover doesn't pass through the other fighter (the original's collision)
                    let room = d - f[i].radius - f[other].radius;
                    let along = step[0] * fx + step[1] * fz;
                    if along > room {
                        let cut = along - room.max(0.0);
                        step = [step[0] - fx * cut, step[1] - fz * cut];
                    }
                    f[i].mover.drive([step[0] / dt, 0.0, step[1] / dt], dt);
                }
                continue;
            }
            match f[i].order {
                Order::Hold => {
                    let p = f[i].mover.position;
                    f[i].mover.step(p, dt)
                }
                Order::Goal(p) => f[i].mover.step(p, dt),
                Order::Drive(v) => f[i].mover.drive(v, dt),
            }
        }
        separate(f);
    }
}

/// @fn F047095
///
/// CharStats::setWeapon ends with chooseAttack (F047076) at a distance of 199 and no last move:
/// what it leaves in the max reach (CharStats +0x150) is the longest reach among the moves the
/// wielded weapon allows at my skill (gatherAttacks, F047074, measures each before testing the
/// distance), so the reach is known before the first swing (tryEngage, F039834, asks within it).
pub fn weapon_reach(f: &Fighter, attacks: &[CombatTechnique]) -> f32 {
    let att = Attacking {
        category: f.wielded.category,
        skill: f.weapon_skill(),
        load: f.load,
        prone: f.prone(),
        size: f.body.size,
        distance: 199.0,
        extra_reach: extra_reach(f.wielded.length),
        repeat_ok: false,
        allow_weak: false,
    };
    let mut reach = 1.0;
    crate::technique::choose_attack(attacks, &att, f.body.animal, f.martial(), None, &mut reach, |_| 1.0, |a, _| a);
    reach
}

/// @fn F024363
///
/// Standing fighters in the fight push off one another as Kenshi's mover does
/// (`kenshi_char::movement::neighbour_push`): each takes its share of the deepest overlap of
/// footprint radii, the smaller more, a fiftieth of it as they are in combat mode (Character
/// +0xe0; (assumption) every active fighter is), and its next steer bends round it: those on
/// its mover's nearby list (`Fighter::near`, the sensor's, F046548), fighting or not.
fn separate(f: &mut [Fighter]) {
    let live: Vec<usize> = (0..f.len()).filter(|&i| f[i].active && !f[i].down).collect();
    let mut moves = Vec::new();
    for &i in &live {
        let p = f[i].mover.position;
        let near: Vec<kenshi_char::movement::Neighbour> = f[i]
            .near
            .iter()
            .filter(|&&j| j != i && j < f.len())
            .map(|&j| kenshi_char::movement::Neighbour { position: f[j].mover.position, radius: f[j].radius * 0.1 })
            .collect();
        // (stand-in for rand()) a fixed draw per pair of places
        let mut x = (i as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ p[0].to_bits() as u64;
        let rand = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 40) as f32 / (1u64 << 24) as f32
        };
        moves.push((i, kenshi_char::movement::neighbour_push(p, f[i].radius * 0.1, &near, true, rand)));
    }
    for (i, push) in moves {
        f[i].mover.avoid = push;
        if let Some((n, depth)) = push {
            f[i].mover.position[0] += n[0] * depth;
            f[i].mover.position[2] += n[2] * depth;
        }
    }
}

/// A character's base layers (movement and idles: lower, upper, overlay), fading at the
/// GLOBAL CONSTANTS "animation blend rate" times each layer's multiplier (F040013).
pub fn base_layers(blend_rate: f32) -> [AnimationLayer<()>; 3] {
    [
        AnimationLayer::with_blend(blend_rate, LAYER_BLEND[0]),
        AnimationLayer::with_blend(blend_rate, LAYER_BLEND[1]),
        AnimationLayer::with_blend(blend_rate, LAYER_BLEND[2]),
    ]
}

/// The base layers made to play `wanted` (clip, rate, weight) on each clip's layer: clips no
/// longer wanted stop (and fade), wanted ones start looping or take their new rate and weight
/// (the animation class's base-layer update, F037184 -> F038900).
pub fn set_base(
    lengths: &HashMap<String, f32>,
    base: &mut [AnimationLayer<()>; 3],
    wanted: &[(&AnimationData, f32, f32)],
) {
    for (li, layer) in base.iter_mut().enumerate() {
        let here: Vec<&(&AnimationData, f32, f32)> = wanted
            .iter()
            .filter(|(a, _, _)| (a.layer.clamp(0, 2) as usize) == li)
            .collect();
        let names: Vec<String> = layer.active.iter().map(|c| c.name.clone()).collect();
        for n in names {
            if !here.iter().any(|(a, _, _)| a.anim_name == n) {
                layer.stop(&n);
            }
        }
        for (a, rate, w) in here {
            let Some(&len) = lengths.get(&a.anim_name.to_lowercase()) else {
                continue;
            };
            if let Some(c) = layer.active.iter_mut().find(|c| c.name == a.anim_name) {
                c.speed = *rate;
                c.target_weight = *w;
            } else {
                let looped = PlayParams {
                    speed: *rate,
                    looping: true,
                    target_weight: *w,
                    end_time: 1.0,
                    root_motion: false,
                    keep_weight: false,
                };
                let name = a.anim_name.clone();
                layer.play(&name, looped, || SingleAnimation::new(&name, len, *rate, 1.0, true, vec![]));
            }
        }
    }
}

/// The base layers advanced `dt` (no action layer: each layer normalised, the overlay at most 1).
pub fn update_base(base: &mut [AnimationLayer<()>; 3], dt: f32) {
    for (li, layer) in base.iter_mut().enumerate() {
        layer.update(0.0, dt, false, li != 2, 1.0);
    }
}

fn new_layers(blend_rate: f32, at: [f32; 3]) -> Layers {
    (
        [
            AnimationLayer::with_blend(blend_rate, LAYER_BLEND[0]),
            AnimationLayer::with_blend(blend_rate, LAYER_BLEND[1]),
            AnimationLayer::with_blend(blend_rate, LAYER_BLEND[2]),
        ],
        AnimationLayer::with_blend(blend_rate, LAYER_BLEND[1]),
        None,
        at,
    )
}

/// Run the animation layers; with `rows`, print a row per fighter every third tick for the
/// renderer.
#[allow(clippy::too_many_arguments)]
fn record(
    layers: &mut [Layers],
    f: &[Fighter],
    sh: &Shared,
    roots: &HashMap<String, Vec<(f32, [f32; 2])>>,
    dt: f32,
    t: f32,
    tick: u32,
    rows: bool,
) -> Vec<([f32; 2], bool)> {
    let mut roots_out = Vec::new();
    for (k, (base, action, current, last)) in layers.iter_mut().enumerate() {
        let fi = &f[k];
        // the root tracks of its skeleton: an animal's own, else the people's
        let tracks = fi.roots.as_deref().unwrap_or(roots);
        let now = fi.pos();
        if !fi.active {
            roots_out.push(([0.0; 2], false));
            continue;
        }
        // (render stand-in until ragdolls are ported) a knocked-out fighter plays the stealth
        // knockout ("stealthKO") once and stays on the ground; no facing: it keeps its last
        if fi.down {
            roots_out.push(([0.0; 2], false));
            if rows && tick.is_multiple_of(3) {
                let len = sh.lengths.get("stealthko").copied().unwrap_or(1.0);
                println!(
                    "f,{t},{k},{},{},0,0,0,0,stealthKO:{}:1",
                    now[0],
                    now[2],
                    (t - fi.down_at).min(len - 0.01)
                );
            }
            continue;
        }
        let moved = ((now[0] - last[0]).powi(2) + (now[2] - last[2]).powi(2)).sqrt() / dt;
        let last_pos = *last;
        *last = now;
        // the move on the action layer; a stumble plays like a move that moves the body
        let want: Option<(String, PlayParams, bool)> = fi.act.as_ref().map(|a| match a.kind {
            Kind::Stumble(s) => (
                sh.stumbles[s].anim_name.clone(),
                PlayParams {
                    speed: 1.0,
                    looping: false,
                    target_weight: 1.0,
                    end_time: 1.0,
                    root_motion: true,
                    keep_weight: true,
                },
                false,
            ),
            Kind::Attack(i) | Kind::Defence(i) => {
                let tech = if matches!(a.kind, Kind::Attack(_)) {
                    &sh.attacks[i]
                } else {
                    &sh.defences[i]
                };
                let (params, reverse) = technique_play(tech, a.clip.speed);
                (tech.anim_name.clone(), params, reverse)
            }
        });
        // shooting (F037165): reloading, the gun's "reload anim" (else "reload 1 phase") held at
        // the reload's progress; else aiming, "aimH" ("aimshoot") played once and held, again
        // from its start after each shot
        let shoot_clip = shooting_clip(fi, sh);
        let want = want.or_else(|| {
            shoot_clip.as_ref().map(|(n, _)| {
                (
                    n.clone(),
                    PlayParams { speed: 1.0, looping: false, target_weight: 1.0, end_time: 1.0, root_motion: false, keep_weight: true },
                    false,
                )
            })
        });
        if fi.shooting && fi.act.is_none() && fi.ranged.since_shot <= dt && fi.ranged.fired > 0 {
            if let Some(old) = current.take() {
                action.stop(&old);
            }
        }
        let name = want.as_ref().map(|w| w.0.clone());
        if name != *current {
            if let Some(old) = current.take() {
                action.stop(&old);
            }
            if let Some((anim, params, reverse)) = &want {
                let len = sh.clip_len(fi, anim);
                action.play(anim, *params, || {
                    let mut c = SingleAnimation::new(
                        anim,
                        len,
                        params.speed,
                        params.end_time,
                        false,
                        vec![],
                    );
                    c.reverse_fading = *reverse;
                    c
                });
            }
            *current = name;
        }
        // the base layers, as the animation class chooses them (F037184): moving, the movement
        // blend by the speed along the facing (signed); standing, the best idle
        let mut sit = situation(fi);
        let o = &f[fi.focus.or_else(|| nearest_enemy(f, k)).unwrap_or(k)];
        let to = [o.pos()[0] - now[0], o.pos()[2] - now[2]];
        let l = (to[0] * to[0] + to[1] * to[1]).sqrt().max(1e-6);
        let v = [(now[0] - last_pos[0]) / dt, (now[2] - last_pos[2]) / dt];
        sit.speed = (v[0] * to[0] + v[1] * to[1]) / l;
        // the speed across the facing (positive to the right)
        let side = (v[0] * to[1] - v[1] * to[0]) / l;
        let _ = moved;
        let mut wanted: Vec<(&AnimationData, f32, f32)> = if fi.act.is_some() {
            Vec::new()
        } else if sit.speed.abs() > 0.5 || side.abs() > 0.5 {
            // moving, the stance idle is kept (F037184 stops the idle only when none is held,
            // +0xa5): the combat shuffles are lower-body clips, so the upper body keeps its
            // fighting stance while the legs step
            let mut m = movement(&sh.library, &sit, 1.0, 1.0);
            // F037179 / F037168: in a fight, the step across the facing: "strafe lower" on the
            // legs, taking the weight the forward movement leaves. (assumption) The weight is
            // the sideways share of the speed and the clip runs at its play speed times the
            // sideways speed (its sign the side: the play speed is negative)
            if side.abs() > 0.5
                && let Some(st) = sh.library.strafes.iter().chain(&sh.library.other).find(|a| a.anim_name == "strafe lower")
            {
                let share = side.abs() / (side.abs() + sit.speed.abs()).max(1e-3);
                for c in m.iter_mut().filter(|(a, _, _)| a.layer == 0) {
                    c.2 *= 1.0 - share;
                }
                m.push((st, st.play_speed * side, share));
            }
            if !m.iter().any(|(a, _, _)| a.layer != 0)
                && let Some(a) = idle_anim(&sh.library, &sit, 1.0)
            {
                m.push((a, a.play_speed, 1.0));
            }
            m
        } else {
            idle_anim(&sh.library, &sit, 1.0)
                .map(|a| vec![(a, a.play_speed, 1.0)])
                .unwrap_or_default()
        };
        // the most hurt part's pain posture while standing or walking (F040458, F037181)
        if fi.act.is_none() && shows_pain(sit.category, sit.speed.abs()) {
            let parts = fi.medical.parts.iter().filter_map(|p| {
                let (anim, v) = sh.pain.get(&p.id)?;
                Some((p.health() / p.max_health(), anim.as_str(), *v))
            });
            if let Some(name) = pain_postures(parts).first()
                && let Some(a) = sh
                    .library
                    .other
                    .iter()
                    .chain(&sh.library.idles)
                    .find(|a| &a.anim_name == name)
            {
                wanted.push((a, a.play_speed, 1.0));
            }
        }
        set_base(&sh.lengths, base, &wanted);
        action.update(0.0, dt, false, true, 1.0);
        // the reload clip follows the reload's progress, not its clock
        if let Some((n, Some(k))) = &shoot_clip
            && let Some(c) = action.active.iter_mut().find(|c| &c.name == n)
        {
            c.speed = 0.0;
            c.time = (k * c.length).min(c.length - 0.01).max(0.0);
            c.progress = c.time / c.length.max(1e-6);
        }
        let keep: f32 = action
            .active
            .iter()
            .chain(&action.fading)
            .filter(|c| c.keep_weight)
            .map(|c| c.weight)
            .sum();
        for (li, layer) in base.iter_mut().enumerate() {
            // the overlay layer isn't normalised, at most 1 (F038910)
            layer.update(0.0, dt, false, li != 2, 1.0 - keep);
        }
        // each layer's root motion: its root-motion clip's root position at its time, weighted
        // (F038905); the class sums the layers
        let mut root = [0.0f32; 2];
        let mut driving = false;
        for c in base
            .iter()
            .flat_map(|l| l.active.iter().chain(&l.fading))
            .chain(&action.active)
            .chain(&action.fading)
            .filter(|c| c.root_motion)
        {
            // (an animal's own skeleton's tracks, at its size)
            let Some(tr) = tracks.get(&c.name.to_lowercase()) else { continue };
            let (r0, r1) = (root_at(tr, 0.0), root_at(tr, c.time));
            root[0] += c.weight * (r1[0] - r0[0]) * fi.body.size;
            root[1] += c.weight * (r1[1] - r0[1]) * fi.body.size;
            driving = true;
        }
        roots_out.push((root, driving));
        // (testing) KENSHI_CLIPS=<fighter>: its clips, speed and order each tenth tick
        if tick.is_multiple_of(6) && std::env::var("KENSHI_CLIPS").is_ok_and(|v| v == k.to_string()) {
            let list: Vec<String> = base
                .iter()
                .enumerate()
                .flat_map(|(li, l)| l.active.iter().chain(&l.fading).map(move |c| (li, c)))
                .map(|(li, c)| format!("L{li} {} t{:.2} w{:.2} x{:.2}", c.name, c.time, c.weight, c.speed))
                .chain(action.active.iter().chain(&action.fading).map(|c| format!("A {} t{:.2} w{:.2}", c.name, c.time, c.weight)))
                .collect();
            let d = ((o.pos()[0] - now[0]).powi(2) + (o.pos()[2] - now[2]).powi(2)).sqrt();
            eprintln!(
                "clips t{t:.2} speed {:.1} dist {d:.0} state {:?} order {} | {}",
                sit.speed,
                fi.combat.state,
                match fi.order { Order::Hold => "hold", Order::Goal(_) => "goal", Order::Drive(_) => "drive" },
                list.join(" | ")
            );
        }
        if rows && tick.is_multiple_of(3) {
            let clips: Vec<&SingleAnimation<()>> = base
                .iter()
                .flat_map(|l| l.active.iter().chain(&l.fading))
                .chain(&action.active)
                .chain(&action.fading)
                .collect();
            // root-motion clips play with the root's translation switched off
            // (setDisableTranslation): the renderer takes their in-clip travel back out
            let mut off = [0.0f32; 2];
            for c in clips.iter().filter(|c| c.root_motion) {
                let Some(tr) = tracks.get(&c.name.to_lowercase()) else { continue };
                let (r0, r1) = (root_at(tr, 0.0), root_at(tr, c.time));
                off[0] += c.weight * (r1[0] - r0[0]) * fi.body.size;
                off[1] += c.weight * (r1[1] - r0[1]) * fi.body.size;
            }
            let list: Vec<String> = clips
                .iter()
                .filter(|c| c.weight > 0.001)
                .map(|c| format!("{}:{}:{}", c.name, c.time, c.weight))
                .collect();
            println!(
                "f,{t},{k},{},{},{},{},{},{},{}",
                now[0],
                now[2],
                off[0],
                off[1],
                to[0] / l,
                to[1] / l,
                list.join("|")
            );
        }
    }
    roots_out
}

/// Clip lengths and root-bone tracks (side, forward) by clip name (lower case), from the race's
/// male skeleton.
pub type Clips = (HashMap<String, f32>, HashMap<String, Vec<(f32, [f32; 2])>>);

/// The race's skeleton clips: lengths and the root bone's ("Bip01") ground-plane track.
pub fn skeleton_clips(res: &Resources, race: &GameData) -> Option<Clips> {
    let mesh = ogre_mesh::read(&res.path(race.files.get("male mesh")?)?).ok()?;
    let skel = ogre_skeleton::read(&res.path(mesh.skeleton.as_ref()?)?).ok()?;
    let lengths = skel
        .animations
        .iter()
        .map(|a| (a.name.to_lowercase(), a.length))
        .collect();
    let root_bone = skel
        .bones
        .iter()
        .find(|b| b.name == "Bip01")
        .map_or(0, |b| b.handle);
    let roots = skel
        .animations
        .iter()
        .map(|a| {
            let k = a
                .tracks
                .iter()
                .find(|t| t.bone == root_bone)
                .map(|t| {
                    t.keyframes
                        .iter()
                        .map(|k| (k.time, [k.translate[0], k.translate[2]]))
                        .collect()
                })
                .unwrap_or_default();
            (a.name.to_lowercase(), k)
        })
        .collect();
    Some((lengths, roots))
}

/// Add the clip copies the ANIMATION records were given (F046697,
/// `kenshi_anim::select::split_shared_clips`) to a skeleton's clips: each as long as, and with
/// the root track of, its shared clip.
pub fn copy_clips(clips: &mut Clips, data: &GameDataContainer) {
    for (shared, copy) in kenshi_anim::select::clip_copies(data) {
        let (shared, copy) = (shared.to_lowercase(), copy.to_lowercase());
        if let Some(&len) = clips.0.get(&shared) {
            clips.0.entry(copy.clone()).or_insert(len);
        }
        if let Some(root) = clips.1.get(&shared).cloned() {
            clips.1.entry(copy).or_insert(root);
        }
    }
}

/// GLOBAL CONSTANTS, a float (or an int as a float).
fn global(data: &GameDataContainer, k: &str) -> Option<f32> {
    let gc = data.get("110-gamedata.quack")?;
    gc.floats
        .get(k)
        .copied()
        .or_else(|| gc.ints.get(k).map(|i| *i as f32))
}

impl Shared {
    /// The length of clip `name` on fighter `f`'s skeleton (an animal's own, else the shared
    /// one's); 1 s when it has no such clip.
    pub fn clip_len(&self, f: &Fighter, name: &str) -> f32 {
        let key = name.to_lowercase();
        f.clips
            .as_ref()
            .and_then(|c| c.get(&key))
            .or_else(|| self.lengths.get(&key))
            .copied()
            .unwrap_or(1.0)
    }

    /// The tables every fighter reads: the techniques, stumbles and animations the skeleton has,
    /// the pain postures and the constants.
    pub fn load(
        data: &GameDataContainer,
        race: &GameData,
        lengths: HashMap<String, f32>,
        seed: u64,
    ) -> Shared {
        let g = |k: &str| global(data, k);
        let gc = data.get("110-gamedata.quack").expect("GLOBAL CONSTANTS");
        let techniques = Techniques::from_records(data.of_type(ItemType::CombatTechnique));
        let has = |n: &str| lengths.contains_key(&n.to_lowercase());
        Shared {
            attacks: techniques
                .attacks
                .into_iter()
                .filter(|t| t.animal != 0 || has(&t.anim_name))
                .collect(),
            defences: techniques
                .defences
                .into_iter()
                .filter(|t| t.animal != 0 || has(&t.anim_name))
                .collect(),
            stumbles: stumbles(data.of_type(ItemType::Animation))
                .into_iter()
                .filter(|s| has(&s.anim_name))
                .collect(),
            library: Library::sort(data.of_type(ItemType::Animation)),
            pain: data
                .of_type(ItemType::LocationalDamage)
                .filter_map(|part| {
                    let (sid, v) = part.refs("pain anim").first()?;
                    let anim = data.get(sid)?.strings.get("anim name")?.clone();
                    Some((part.string_id.clone(), (anim, v[0])))
                })
                .collect(),
            lengths,
            cc: CombatConstants::from_floats(g),
            hc: HitConstants::from_floats(g),
            mc: MedConstants::from_record(gc),
            race: race.string_id.clone(),
            stumble_max: g("stumble damage max").unwrap_or(80.0),
            max_slots: g("max num attack slots").unwrap_or(1.0) as i32,
            harpoon_limit: 60,
            chance_factor: g("attack chance factor").unwrap_or(0.0),
            unarmed_mult: g("unarmed damage mult").unwrap_or(1.0),
            damage_mult: 1.0,
            xp: XpConstants::from_record(|k| g(k)),
            strain: StrainConstants::from_record(gc),
            hits: Vec::new(),
            cues: Vec::new(),
            rng: Rng(seed),
            t: 0.0,
        }
    }
}

/// (harness) What a fighter carries: one weapon at quality 0.5 and an outfit of ARMOUR records
/// (by name) at 0.5.
pub struct Kit {
    pub weapon: Weapon,
    pub weapon_record: String,
    pub outfit: Vec<String>,
}

impl Kit {
    pub fn new(data: &GameDataContainer, weapon: &str, outfit: &[&str]) -> Option<Kit> {
        let wr = data.of_type(ItemType::Weapon).find(|r| r.name == weapon)?;
        Some(Kit {
            weapon: Weapon::from_record(
                wr,
                None,
                None,
                0.5,
                global(data, "weapon inventory weight mult").unwrap_or(0.5),
            ),
            weapon_record: wr.string_id.clone(),
            outfit: outfit.iter().map(|s| s.to_string()).collect(),
        })
    }
}

impl Fighter {
    /// (harness) A fighter of a race with every attribute and skill at `skill`, at `position`
    /// (units).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        data: &GameDataContainer,
        race: &GameData,
        kit: &Kit,
        name: &str,
        team: u16,
        skill: f32,
        position: [f32; 3],
    ) -> Fighter {
        let g = |k: &str| global(data, k);
        let gc = data.get("110-gamedata.quack").expect("GLOBAL CONSTANTS");
        let sc = StrainConstants::from_record(gc);
        let rm = RaceMovement::from_record(race);
        let wr = data.get(&kit.weapon_record).expect("weapon");
        let wielded = set_weapon(
            Some((&kit.weapon, wr)),
            g("blunt damage 1").unwrap_or(20.0),
            g("cut damage 1").unwrap_or(20.0),
        );
        let mut medical = Medical::from_race(race, data, 1.0, 1.0);
        // (assumption) a new character starts well fed: hunger shown 250 of 300
        medical.hunger.level = 2.5;
        let armour: Vec<Armour> = kit
            .outfit
            .iter()
            .filter_map(|name| data.of_type(ItemType::Armour).find(|r| &r.name == name))
            .filter_map(|r| {
                Armour::from_record(r, 0.5, |sid| medical.parts.iter().position(|p| p.id == sid))
            })
            .collect();
        let body = Body::of_race(race);
        let blood = Medical::max_blood(body.blood_range.0, body.blood_range.1, skill * 0.01, 1.0, 1.0);
        medical.blood = blood;
        medical.max_blood = blood;
        let mover = Mover::for_character(&rm, position, 1.0, 1.0, false);
        let mut skills = Skills::new(g("exp gain multiplier").unwrap_or(3.0));
        for s in COMBAT_STATS {
            skills.levels.insert(s, skill);
        }
        let mc = MedConstants::from_record(gc);
        let mut f = Fighter {
            name: name.into(),
            team,
            foes: None,
            active: true,
            focus: None,
            pick_timer: 0.0,
            skills,
            mover,
            radius: rm.footprint_radius,
            near: Vec::new(),
            medical,
            stats: Stats {
                toughness: skill,
                toughness_scale: 1.0,
            },
            wielded,
            armour,
            attack_speed: 1.0,
            defence_speed: 1.0,
            run: 0.0,
            strain: 0.0,
            load: 1.0,
            dust: 0.0,
            indoors_mod: 8,
            indoors: false,
            carrying: false,
            legs: 1.0,
            weapon_weight: kit.weapon.combat_weight,
            combat: CombatClass::default(),
            act: None,
            slots: AttackerSlots::default(),
            order: Order::Hold,
            down: false,
            down_at: 0.0,
            clips: None,
            roots: None,
            extra_slots: race.ints.get("extra attack slots").copied().unwrap_or(0),
            last_stumble: None,
            steering: Steering::default(),
            circle_timer: 0.0,
            max_reach: 1.0,
            last_attack: None,
            root_prev: [0.0; 2],
            gun: None,
            ranged: crate::ranged::Ranged::default(),
            ammo: 0,
            ammo_spent: 0,
            shooting: false,
            ground_in_line: false,
            knock: [0.0; 3],
            knock_at: f32::NEG_INFINITY,
            targeting: Targeting::default(),
            ignored: Vec::new(),
            ordered_away: false,
            via: None,
            gear: crate::armour::GearMods::default(),
            weapon_mods: (0, 0),
            body: Body { ko_range: (mc.min_toughness_ko_point, mc.max_toughness_ko_point), ..Body::of_race(race) },
            deep_water: false,
        };
        f.refresh(&sc, &rm);
        f
    }

    /// @fn F040054
    ///
    /// The fighter becomes the ANIMAL_CHARACTER `animal` (of `race`) at `growth` (CharacterAnimal
    /// +0x700: 0 newborn .. 1 grown): its size lerp(growth, "scale min" +0x6f0, "scale max"
    /// +0x6f4) (F039296), its maturity the growth (F039298), its body parts' health times the
    /// size and "HP mult" (+0x6dc, 1 when not above 0; the health factor, vtable 0x238,
    /// F039302), its blood the race's at its strength and size (F040410). Its weapon skill is
    /// martial arts (CharStatsAnimal, F040363).
    pub fn set_animal(&mut self, data: &GameDataContainer, race: &GameData, animal: &GameData, growth: f32) {
        let f = |k: &str| animal.floats.get(k).copied().or_else(|| animal.ints.get(k).map(|&v| v as f32));
        let (lo, hi) = (f("scale min").unwrap_or(1.0), f("scale max").unwrap_or(1.0));
        let hp = f("HP mult").filter(|&v| 0.0 < v).unwrap_or(1.0);
        self.body.animal = true;
        self.body.maturity = growth;
        self.body.size = kenshi_char::medical::lerp(growth, lo, hi);
        let hunger = self.medical.hunger;
        self.medical = Medical::from_race(race, data, self.body.size, hp);
        self.medical.hunger = hunger;
        let blood = self.max_blood();
        self.medical.blood = blood;
        self.medical.max_blood = blood;
        let gc = data.get("110-gamedata.quack").expect("GLOBAL CONSTANTS");
        self.refresh(&StrainConstants::from_record(gc), &RaceMovement::from_record(race));
    }

    /// Wield the WEAPON item `weapon` (None: unarmed) and wear the ARMOUR items `armour`
    /// (CharStats::setWeapon F047095 and the armour list): what it hits with, its weapon
    /// skill, strain and speeds, and what covers it; each at its quality (0 .. 100), a weapon
    /// with its maker and model (F047085: the maker's damage and weight mods, the model's
    /// attack and defence mods).
    pub fn equip(&mut self, data: &GameDataContainer, race: &GameData, weapon: Option<&kenshi_char::inventory::Item>, armour: &[kenshi_char::inventory::Item]) {
        let g = |k: &str| global(data, k);
        let wi = weapon.filter(|i| data.get(&i.record).is_some_and(|r| r.item_type() == Some(ItemType::Weapon)));
        let mut w = wi.and_then(|i| data.get(&i.record));
        let mut item = wi.zip(w).map(|(i, r)| {
            let maker = i.maker.as_ref().and_then(|m| data.get(m));
            let model = i.model.as_ref().and_then(|m| data.get(m));
            Weapon::from_record(r, maker, model, i.quality * 0.01, g("weapon inventory weight mult").unwrap_or(0.5))
        });
        // unarmed: its fists (F040049), the race's "fists lv1" weapon, else the WEAPON
        // "FISTS", at its martial arts level: their reach ("length"), damage multipliers
        // and bleeding
        if item.is_none() {
            let fists = race.refs("fists lv1").first().and_then(|(sid, _)| data.get(sid)).or_else(|| data.of_type(ItemType::Weapon).find(|r| r.name == "FISTS"));
            if let Some(r) = fists {
                let level = (self.level(StatsEnumerated::StatMartialarts) * 0.01).clamp(0.0, 1.0);
                item = Some(Weapon::from_record(r, None, None, level, g("weapon inventory weight mult").unwrap_or(0.5)));
                w = Some(r);
            }
        }
        self.wielded = set_weapon(
            item.as_ref().zip(w),
            g("blunt damage 1").unwrap_or(20.0),
            g("cut damage 1").unwrap_or(20.0),
        );
        self.weapon_weight = item.as_ref().map_or(0.0, |i| i.combat_weight);
        // its reach is measured again for the new weapon (Arena::step, F047095)
        self.max_reach = 1.0;
        let medical = &self.medical;
        let pieces: Vec<crate::armour::PieceMods> = armour
            .iter()
            .filter_map(|i| data.get(&i.record).map(|r| (i, r)))
            .filter(|(_, r)| r.item_type() == Some(ItemType::Armour))
            .filter_map(|(i, r)| crate::armour::piece_mods(r, i.quality * 0.01))
            .collect();
        let bag = armour.iter().filter_map(|i| data.get(&i.record)).find(|r| r.item_type() == Some(ItemType::Container)).map(crate::armour::BagMods::from_record);
        self.gear = crate::armour::gear_mods(&pieces, bag);
        self.weapon_mods = item.as_ref().map_or((0, 0), |w| (w.attack_mod, w.defence_mod));
        self.indoors_mod = item.as_ref().map_or(8, |w| w.indoors_mod);
        self.armour = armour
            .iter()
            .filter_map(|i| data.get(&i.record).map(|r| (i, r)))
            .filter(|(_, r)| r.item_type() == Some(ItemType::Armour))
            .filter_map(|(i, r)| Armour::from_record(r, i.quality * 0.01, |sid| medical.parts.iter().position(|p| p.id == sid)))
            .collect();
        let gc = data.get("110-gamedata.quack").expect("GLOBAL CONSTANTS");
        self.refresh(&StrainConstants::from_record(gc), &RaceMovement::from_record(race));
    }

    /// Set its stats and what they drive.
    pub fn set_skills(&mut self, data: &GameDataContainer, race: &GameData, skills: Skills) {
        let gc = data.get("110-gamedata.quack").expect("GLOBAL CONSTANTS");
        self.skills = skills;
        self.refresh(&StrainConstants::from_record(gc), &RaceMovement::from_record(race));
    }
}

/// The stats a harness fighter starts with all at its one level.
pub const COMBAT_STATS: [StatsEnumerated; 17] = {
    use StatsEnumerated::*;
    [
        StatStrength,
        StatDexterity,
        StatToughness,
        StatAthletics,
        StatPerception,
        StatMeleeAttack,
        StatMeleeDefence,
        StatDodge,
        StatMartialarts,
        StatKatanas,
        StatSabres,
        StatHackers,
        StatHeavyweapons,
        StatBlunt,
        StatPolearms,
        StatCrossbows,
        StatStealth,
    ]
};

/// The height above the feet a crossbow is held at and aimed at (F033800: the gun's place when
/// not drawn on a body, the character's position raised by 10).
/// @fn F033844
///
/// The aim point on the target: its "Bip01 Spine2" bone (13.07 units up in the human skeleton's
/// bind pose), "Bip01 Spine1" for an animal (vtable +0x248 answers; [`bone_height`] at its
/// size; (assumption) the bone's height only, not its place along the body), (not ported) the
/// bone's animated place; led by the target's velocity over the bolt's flight time (the
/// distance from the gun over its shot speed). (assumption) The held crossbow's barrel is at the
/// same height.
pub const AIM_HEIGHT: f32 = 13.07;

/// The height of bone `bone` in the bind pose of the race's skeleton (its "male mesh"'s), the
/// bones' positions turned by their parents' orientations (units, at size 1).
pub fn bone_height(res: &Resources, race: &GameData, bone: &str) -> Option<f32> {
    let mesh = ogre_mesh::read(&res.path(race.files.get("male mesh")?)?).ok()?;
    let skel = ogre_skeleton::read(&res.path(mesh.skeleton.as_ref()?)?).ok()?;
    let mut chain = Vec::new();
    let mut cur = skel.bones.iter().find(|b| b.name == bone);
    while let Some(b) = cur {
        chain.push(b);
        cur = b.parent.and_then(|p| skel.bone(p));
    }
    if chain.is_empty() {
        return None;
    }
    // quaternions as stored: x, y, z, w
    let mul = |a: [f32; 4], b: [f32; 4]| {
        let [ax, ay, az, aw] = a;
        let [bx, by, bz, bw] = b;
        [
            aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz,
        ]
    };
    let rotate = |q: [f32; 4], v: [f32; 3]| {
        let p = mul(mul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]);
        [p[0], p[1], p[2]]
    };
    let (mut pos, mut rot) = ([0.0f32; 3], [0.0, 0.0, 0.0, 1.0]);
    for b in chain.iter().rev() {
        let p = rotate(rot, b.position);
        pos = [pos[0] + p[0], pos[1] + p[1], pos[2] + p[2]];
        rot = mul(rot, b.orientation);
    }
    Some(pos[1])
}

/// Whether the ground rises above the straight line from `from` to `to` (units; sampled every
/// 20 units, the ends left out), `ground` the terrain's height there.
pub fn ground_between(from: [f32; 3], to: [f32; 3], ground: &mut dyn FnMut(f32, f32) -> f32) -> bool {
    let d = ((to[0] - from[0]).powi(2) + (to[2] - from[2]).powi(2)).sqrt();
    let n = (d / 20.0) as i32;
    (1..n).any(|k| {
        let t = k as f32 / n as f32;
        let x = from[0] + (to[0] - from[0]) * t;
        let z = from[2] + (to[2] - from[2]) * t;
        let y = from[1] + (to[1] - from[1]) * t;
        ground(x, z) > y
    })
}

/// F033844's aim point on fighter `t` for a gun at `from` firing at `shot_speed` (units/s).
pub fn aim_point(t: &Fighter, from: [f32; 3], shot_speed: f32) -> [f32; 3] {
    let p = t.pos();
    let at = [p[0], p[1] + t.body.aim * t.body.size, p[2]];
    let d = ((at[0] - from[0]).powi(2) + (at[1] - from[1]).powi(2) + (at[2] - from[2]).powi(2)).sqrt();
    let k = d / shot_speed.max(1e-3);
    let v = t.mover.velocity.map(|c| c * 10.0);
    [at[0] + v[0] * k, at[1] + v[1] * k, at[2] + v[2] * k]
}

/// The clip shooting plays on the action layer (F037165): reloading, the reload clip with the
/// reload's progress (0 .. 1); else, with a loaded crossbow, "aimH"'s clip.
fn shooting_clip(fi: &Fighter, sh: &Shared) -> Option<(String, Option<f32>)> {
    if !fi.shooting || fi.act.is_some() {
        return None;
    }
    let gun = fi.gun.as_ref()?;
    let by_record = |n: &str| sh.library.other.iter().chain(&sh.library.idles).find(|a| a.name == n).map(|a| a.anim_name.clone());
    if fi.ranged.reloading && 0.0 < fi.ranged.progress && gun.shots == 0 {
        let rec = gun.reload_anim.as_deref().unwrap_or("reload 1 phase");
        let clip = by_record(rec).or_else(|| by_record("reload 1 phase"))?;
        return Some((clip, Some(fi.ranged.progress)));
    }
    (0 < gun.shots).then(|| by_record("aimH").map(|c| (c, None))).flatten()
}

/// A crossbow it can use: loaded, reloading, or with a reload to come (no ammo needed or some
/// carried); out of ammo it fights hand to hand (F033858 state 2).
pub fn ranged_usable(f: &Fighter) -> bool {
    f.gun.as_ref().is_some_and(|g| 0 < g.shots || f.ranged.reloading || g.ammo.is_none() || 0 < f.ammo)
}

/// @fn F033851
///
/// An enemy standing within 40 units: ranged combat gives way to melee.
pub fn enemy_close(f: &[Fighter], me: usize) -> bool {
    let p = f[me].pos();
    (0..f.len()).any(|o| {
        o != me && opponent(&f[me], &f[o]) && {
            let q = f[o].pos();
            (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2) < crate::ranged::CLOSE_SQ
        }
    })
}

/// Everyone as bolts and lines of fire see them, `me`'s enemies marked.
fn bodies(f: &[Fighter], me: usize) -> Vec<crate::ranged::Body> {
    (0..f.len())
        .filter(|&o| !f[o].down || o == me)
        .map(|o| crate::ranged::Body {
            id: o,
            pos: f[o].pos(),
            half: f[o].hull(),
            enemy: o != me && f[me].team != f[o].team && f[me].foes.as_ref().is_none_or(|v| v.contains(&f[o].team)),
        })
        .collect()
}

/// The line of fire from `from` to `to` (F033852's trace with F033779): the first character on it
/// short of the target, if not an enemy, holds the shot.
/// The ground in the way blocks it (`Fighter::ground_in_line`), and so does a building (the
/// arena's obstacles).
fn line_of_fire(f: &[Fighter], obstacles: &Option<(std::sync::Arc<kenshi_physics::world::StaticWorld>, [f32; 3])>, me: usize, target: usize, from: [f32; 3], to: [f32; 3]) -> crate::ranged::Line {
    use crate::ranged::{Line, body_hit, dist_sq, normalise};
    if f[me].ground_in_line {
        return Line::Blocked;
    }
    let len = dist_sq(from, to).sqrt();
    let dir = normalise([to[0] - from[0], to[1] - from[1], to[2] - from[2]]);
    if wall_hit(obstacles, from, dir, len, LINE_OF_FIRE_GROUPS).is_some() {
        return Line::Blocked;
    }
    let first = bodies(f, me)
        .into_iter()
        .filter(|b| b.id != me)
        .filter_map(|b| body_hit(from, dir, len, &b).map(|t| (t, b)))
        .min_by(|a, b| a.0.total_cmp(&b.0));
    match first {
        Some((t, b)) if b.id != target && !b.enemy => {
            if std::env::var("KENSHI_RANGED_DEBUG").is_ok() {
                eprintln!("line of fire from {from:?} to {to:?}: {} at {t:.0} ({:?}, hull {:?})", f[b.id].name, b.pos, b.half);
            }
            Line::Friend
        }
        _ => Line::Clear,
    }
}

/// The barrel's place: the held crossbow's barrel offset turned to the facing (toward `at`).
fn barrel_at(me: &Fighter, gun: &crate::ranged::Gun, at: [f32; 3]) -> [f32; 3] {
    let p = me.pos();
    let (dx, dz) = (at[0] - p[0], at[2] - p[2]);
    let l = (dx * dx + dz * dz).sqrt().max(1e-6);
    let (fx, fz) = (dx / l, dz / l);
    [p[0] + fx * gun.barrel[2], p[1] + AIM_HEIGHT + gun.barrel[1], p[2] + fz * gun.barrel[2]]
}

/// @fn F033858
///
/// A tick of ranged combat for fighter `i` against `target`: out of shots, the reload (F033853,
/// standing); loaded, standing to shoot while in range - the aim and fire attempt (F033852)
/// firing (F033867), a blocked line sending it walking - else walking up until in range with a
/// clear line held 0.3 s. XP as F047468: crossbows and perception while aiming in range,
/// crossbows and dexterity while reloading.
#[allow(clippy::too_many_arguments)]
fn ranged_tick(f: &mut [Fighter], sh: &mut Shared, bolts: &mut Vec<crate::ranged::Bolt>, obstacles: &Option<(std::sync::Arc<kenshi_physics::world::StaticWorld>, [f32; 3])>, i: usize, target: usize, dt: f32) {
    use crate::ranged::*;
    use StatsEnumerated::*;
    f[i].focus = Some(target);
    f[i].ranged.since_shot += dt;
    if std::env::var("KENSHI_RANGED_DEBUG").is_ok() && (sh.t * 4.0) as i32 != ((sh.t - dt) * 4.0) as i32 {
        let g = f[i].gun.as_ref();
        eprintln!(
            "ranged {} -> {}: state {} shots {:?} ammo {} reloading {} ({:.2}) aim {:.2} dist {:.0} in range {:?}",
            f[i].name,
            f[target].name,
            f[i].ranged.state,
            g.map(|g| g.shots),
            f[i].ammo,
            f[i].ranged.reloading,
            f[i].ranged.progress,
            f[i].ranged.aim,
            crate::ranged::dist_sq(f[i].pos(), f[target].pos()).sqrt(),
            g.map(|g| g.in_range(f[i].pos(), f[target].pos()))
        );
    }
    let skill = f[i].ranged_level(StatCrossbows);
    let Some(mut gun) = f[i].gun.take() else { return };
    // the reload
    if gun.shots <= 0 {
        f[i].order = Order::Hold;
        let (has, was) = (0 < f[i].ammo, f[i].ranged.reloading);
        let mut took = false;
        let mut r = f[i].ranged;
        r.reload(&mut gun, skill * 0.01, dt, has, || took = true);
        f[i].ranged = r;
        if took {
            f[i].ammo = f[i].ammo.saturating_sub(1);
            f[i].ammo_spent += 1;
        }
        if !was && f[i].ranged.reloading && sh.cues.len() < 256 {
            sh.cues.push(Cue::Reload(i));
        }
        if f[i].ranged.reloading || took {
            f[i].skills.train(StatCrossbows, dt, XP_CROSSBOWS);
            f[i].skills.train(StatDexterity, dt, XP_DEXTERITY);
        }
        f[i].gun = Some(gun);
        return;
    }
    let me = f[i].pos();
    let tp = f[target].pos();
    let in_range = gun.in_range(me, tp);
    let raised = |p: [f32; 3]| [p[0], p[1] + 5.0, p[2]];
    if f[i].ranged.state == 1 {
        // walking up (F033858 state 1)
        let p = f[i].mover.position;
        f[i].order = Order::Goal([tp[0] * 0.1, p[1], tp[2] * 0.1]);
        if in_range {
            if line_of_fire(f, obstacles, i, target, raised(me), raised(tp)) == Line::Clear {
                f[i].ranged.clear += dt;
                if CLEAR_TIME < f[i].ranged.clear {
                    f[i].order = Order::Hold;
                    f[i].ranged.state = 0;
                }
            } else {
                f[i].ranged.clear = 0.0;
            }
        }
        f[i].gun = Some(gun);
        return;
    }
    f[i].ranged.state = 0;
    f[i].order = Order::Hold;
    if !in_range {
        f[i].ranged.state = 1;
        f[i].ranged.clear = 0.0;
        f[i].gun = Some(gun);
        return;
    }
    f[i].skills.train(StatPerception, dt, XP_PERCEPTION);
    f[i].skills.train(StatCrossbows, dt, XP_CROSSBOWS);
    let barrel = barrel_at(&f[i], &gun, [tp[0], tp[1] + f[target].body.aim * f[target].body.size, tp[2]]);
    let aim_at = aim_point(&f[target], barrel, gun.shot_speed);
    let mut r = f[i].ranged;
    let res = r.attempt(0 < gun.shots, dt, || line_of_fire(f, obstacles, i, target, barrel, aim_at));
    f[i].ranged = r;
    match res {
        1 => {
            let (per, pre) = (f[i].ranged_level(StatPerception), f[i].ranged_level(StatFriendlyFire));
            let rng = &mut sh.rng;
            let bolt = fire(&mut gun, i, Some(target), barrel, aim_at, skill, per, pre, || rng.next());
            if std::env::var("KENSHI_RANGED_DEBUG").is_ok() {
                eprintln!("{} shoots at {} ({} shots left)", f[i].name, f[target].name, gun.shots);
            }
            bolts.push(bolt);
            f[i].ranged.fired += 1;
            f[i].ranged.since_shot = 0.0;
            // (assumption) the next shot aims afresh
            f[i].ranged.aim = AIM_TIME;
            if sh.cues.len() < 256 {
                const SOUNDS: [&str; 4] = ["Crossbow", "Harpoon", "Crossbow_Turret", "Harpoon_Turret"];
                sh.cues.push(Cue::Shoot(i, SOUNDS.get(gun.shoot_sound as usize).copied().unwrap_or("Crossbow")));
            }
            // the target's AI hears of it (F01814c): it joins the fight
            if !f[target].down {
                f[target].active = true;
            }
        }
        0 => {
            f[i].ranged.state = 1;
            f[i].ranged.clear = 0.0;
        }
        _ => {}
    }
    f[i].gun = Some(gun);
}

impl Arena {
    /// The bolts' flight (F033856) and their hits (F033863, onProjectileHit): the bolt's damage as
    /// pierce with bleed 2 on a part picked ignoring focus (direction 6); `ground(x, z)` the
    /// terrain's height (units). Stuck bolts follow whom they are in; past the harpoon limit the
    /// oldest stuck one goes (F033871: the one longest since fired; Kenshi keeps a list per bolt
    /// mesh, one list here).
    pub fn step_bolts(&mut self, dt: f32, ground: &mut dyn FnMut(f32, f32) -> f32) {
        use crate::ranged::{BOLT_BLEED, BoltHit, BoltState};
        let Arena { f, sh, bolts, obstacles, .. } = self;
        for k in 0..bolts.len() {
            let shooter = bolts[k].shooter;
            let list = bodies(f, shooter);
            let rng = &mut sh.rng;
            let walls = if bolts[k].mounted { &None } else { &*obstacles };
            let hit = bolts[k].update_among(dt, &list, ground, &mut |p, d, l| wall_hit(walls, p, d, l, BOLT_GROUPS), || rng.next());
            match hit {
                Some(BoltHit::Character(v)) => {
                    let b = &bolts[k];
                    let hit = crate::hit::Hit { pierce: b.damage, bleed: BOLT_BLEED, ..Default::default() };
                    let victim = &mut f[v];
                    let ctx = HitContext { low_strike: false, direction: 6, robot: victim.body.robot, animal: victim.body.animal, damage_mult: sh.damage_mult, race_bleed: victim.body.bleed };
                    let rng = &mut sh.rng;
                    let r = resolve_hit(&mut victim.medical, hit, &victim.armour, &victim.stats, &ctx, &sh.hc, &sh.mc, |a, c| rng.range(a, c));
                    if std::env::var("KENSHI_RANGED_DEBUG").is_ok() {
                        eprintln!("bolt from {} hits {}: part {} pierce {:.1}", f[shooter].name, f[v].name, r.part, r.damage.pierce);
                    }
                    if sh.hits.len() < 256 {
                        sh.hits.push((shooter, v, r.part, r.floater));
                    }
                    if sh.cues.len() < 256 {
                        sh.cues.push(Cue::Bolt(shooter, v, r.material));
                    }
                    if !f[v].down {
                        f[v].active = true;
                    }
                    // F033863: the struck one trains on the bolt's pierce (as a hit, 4) and takes
                    // its hit reaction (onHit, from its facing against the shooter)
                    let face = facing(f, v, None);
                    let (vp, sp) = (f[v].pos(), f[shooter].pos());
                    let to = [sp[0] - vp[0], 0.0, sp[2] - vp[2]];
                    let behind = face.is_some_and(|fc| hit_from_behind(fc, to));
                    let xp = XpConstants { global: sh.damage_mult, ..sh.xp };
                    if shooter != v {
                        let (victim, s) = pair_mut(f, v, shooter);
                        let td = victim.training(s);
                        victim.skills.train_from_combat(CombatEvent::Hit { damage: hit.pierce }, &td, &xp);
                    }
                    let total = r.damage.cut + r.damage.blunt + r.damage.pierce + r.damage.stun;
                    on_hit(&mut f[v], sh, total, r.part, behind);
                }
                Some(BoltHit::Ground(at)) => {
                    if sh.cues.len() < 256 {
                        sh.cues.push(Cue::BoltGround(at));
                    }
                }
                None => {}
            }
            if let BoltState::InCharacter(who, off) = bolts[k].state {
                let p = f[who].pos();
                bolts[k].pos = [p[0] + off[0], p[1] + off[1], p[2] + off[2]];
            }
        }
        bolts.retain(|b| !b.gone());
        while bolts.iter().filter(|b| b.state != BoltState::Flying).count() > sh.harpoon_limit {
            let Some(i) = bolts.iter().position(|b| b.state != BoltState::Flying) else { break };
            bolts.remove(i);
        }
    }
}

impl Arena {
    /// The manned turrets (TurretBuilding with its GunClassTurret, worked by Task_UseTurret's
    /// gunner through the ranged update F033858): each picks an enemy of its gunner standing in
    /// range and in its fire arc, turns its head toward them (F033784), reloads at the gunner's
    /// Turrets skill (F033853, no ammo), aims and fires (F033852, F033867) with the gunner's
    /// Turrets, Perception and Precision shooting. XP as F047468: turrets and perception while
    /// aiming, turrets and dexterity while reloading.
    /// The target (F033850): of its gunner's enemies the best by F033823, (1 - distance over
    /// the gun's range)^2 (a knocked-out one 0.01 the weight for the player's gunner, none for
    /// another's; 0.001 out of range), x the arc weight (F033781: 1 in the arc, 0.1 out),
    /// x 1.5 for one going for the gunner, + 0.1 and then x 1.5 for the one held; with none and
    /// Shoot First, of the dangerous neutrals (`also`) over 0.6 the range. It fires only at one
    /// in range and in the arc. (assumption) The gunner sees all (F033850's x 0.1 for one
    /// unseen not applied); the line of fire is not traced (the gunner stands above, on walls).
    pub fn step_turrets(&mut self, dt: f32) {
        use crate::ranged::*;
        use StatsEnumerated::*;
        let Arena { f, sh, bolts, turrets, .. } = self;
        for t in turrets.iter_mut() {
            let Some(o) = t.operator.filter(|&o| o < f.len() && !f[o].down) else {
                t.target = None;
                continue;
            };
            let reach = |t: &Turret, q: [f32; 3]| t.gun.in_range(t.pivot_world(), q) && t.in_arc(q);
            let also = t.also.clone();
            let foe = |k: usize| {
                k != o && !f[k].down && f[k].team != f[o].team && (f[o].foes.as_ref().is_none_or(|v| v.contains(&f[k].team)) || also.contains(&f[k].team))
            };
            let players = t.building.is_some();
            let held = t.target;
            let score = |t: &Turret, k: usize, mult: f32| -> f32 {
                // F033823
                let w = if f[k].down {
                    if !players {
                        return 0.0;
                    }
                    0.1 * 0.1
                } else {
                    1.0
                };
                let d = dist_sq(f[o].pos(), f[k].pos()).sqrt();
                let x = (1.0 - d / (mult * t.gun.range)).max(0.0);
                let mut s = (w * x) * (w * x);
                if s <= 0.0 {
                    return 0.001;
                }
                s *= if t.in_arc(f[k].pos()) { 1.0 } else { 0.1 };
                if f[k].focus.or(f[k].combat.target) == Some(o) {
                    s *= 1.5;
                }
                if held == Some(k) {
                    s += 0.1;
                }
                // F033850: the one held
                if held == Some(k) { s * 1.5 } else { s }
            };
            let best = |t: &Turret, pick: &dyn Fn(usize) -> bool, mult: f32| {
                (0..f.len())
                    .filter(|&k| k != o && pick(k))
                    .map(|k| (k, score(t, k, mult)))
                    .filter(|&(_, s)| s > 1e-6)
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(k, _)| k)
            };
            let enemy = |k: usize| f[k].team != f[o].team && f[o].foes.as_ref().is_none_or(|v| v.contains(&f[k].team));
            let neutral = |k: usize| f[k].team != f[o].team && also.contains(&f[k].team);
            t.target = best(t, &enemy, 1.0).or_else(|| if players { best(t, &neutral, 0.6) } else { None });
            let _ = foe;
            let skill = f[o].ranged_level(StatTurrets);
            // the reload
            if t.gun.shots <= 0 {
                let was = t.ranged.reloading;
                let mut r = t.ranged;
                r.reload(&mut t.gun, skill * 0.01, dt, false, || {});
                t.ranged = r;
                if !was && t.ranged.reloading && sh.cues.len() < 256 {
                    sh.cues.push(Cue::Reload(o));
                }
                if t.ranged.reloading {
                    f[o].skills.train(StatTurrets, dt, XP_TURRETS);
                    f[o].skills.train(StatDexterity, dt, XP_DEXTERITY);
                }
                continue;
            }
            let Some(k) = t.target else { continue };
            let goal = aim_point(&f[k], t.barrel(), t.gun.shot_speed);
            let (yaw0, pitch0) = (t.yaw, t.pitch);
            let off = t.turn_to(goal, dt);
            // the head's movement sound: started when it turns more than 0.1 rad/s, stopped 8
            // frames after (F033784)
            let speed = ((t.yaw - yaw0).powi(2) + (t.pitch - pitch0).powi(2)).sqrt() / dt.max(1e-6);
            if t.moving == 0 && 0.1 < speed && sh.cues.len() < 256 {
                sh.cues.push(Cue::TurretMove(t.at, true));
                t.moving = 8;
            }
            if 0 < t.moving {
                t.moving -= 1;
                if t.moving == 0 && sh.cues.len() < 256 {
                    sh.cues.push(Cue::TurretMove(t.at, false));
                }
            }
            f[o].skills.train(StatPerception, dt, XP_PERCEPTION);
            f[o].skills.train(StatTurrets, dt, XP_TURRETS);
            if TURRET_AIMED < off || !reach(t, f[k].pos()) {
                continue;
            }
            let mut r = t.ranged;
            let res = r.attempt(0 < t.gun.shots, dt, || Line::Clear);
            t.ranged = r;
            if res == 1 {
                let (per, pre) = (f[o].ranged_level(StatPerception), f[o].ranged_level(StatFriendlyFire));
                let barrel = t.barrel();
                let rng = &mut sh.rng;
                let mut bolt = fire(&mut t.gun, o, Some(k), barrel, goal, skill, per, pre, || rng.next());
                bolt.mounted = true;
                bolts.push(bolt);
                t.ranged.aim = AIM_TIME;
                if sh.cues.len() < 256 {
                    const SOUNDS: [&str; 4] = ["Crossbow", "Harpoon", "Crossbow_Turret", "Harpoon_Turret"];
                    sh.cues.push(Cue::Shoot(o, SOUNDS.get(t.gun.shoot_sound as usize).copied().unwrap_or("Crossbow_Turret")));
                }
                if std::env::var("KENSHI_RANGED_DEBUG").is_ok() {
                    eprintln!("turret of {} shoots at {}", f[o].name, f[k].name);
                }
                if !f[k].down {
                    f[k].active = true;
                }
            }
        }
    }
}
