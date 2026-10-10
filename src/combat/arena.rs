//! A melee arena (port_map `combat`): any number of fighters on the ported combat rules.
//!
//! Each fighter's combat AI is the ported CombatClass ([`crate::fighter`]): its update runs the
//! original's state machine. This module is the game side around it ([`Ctx`]): the characters'
//! animation actions (timed by the ported animation clock, moved by the clips' root bone),
//! movement (the ported mover and formation steering), hits (resolution, armour, stumbles,
//! knockouts) and target finding (F040734: the current target is kept for a second, then the best
//! standing enemy). Not ported here: the threat lists and the squad's scoring of them (F038638;
//! harness: nearest standing enemy), neighbour avoidance, knockback and knockdowns; the ground
//! is flat (no navmesh).

use crate::attack::{Target, TargetKind, Unarmed, attack_damage, martial_arts};
use crate::engage::{Approach, Asked, can_engage, request_attack};
use crate::fighter::{CombatClass, Fighting};
use crate::hit::{Armour, HitConstants, HitContext, resolve_hit};
use crate::melee::{Action, Defence, block_direction, defend_outcome};
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
use kenshi_char::stats::{CombatEvent, CombatTraining, Skills, XpConstants};
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
}

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

#[derive(Clone, Copy, PartialEq)]
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
    pub medical: Medical,
    pub stats: Stats,
    pub wielded: Wielded,
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
    /// The worn armour's modifiers (F047071) and the weapon's attack and defence mods
    /// ("attack mod", "defence mod", with its model's; F047028 adds them to the armour's).
    pub gear: crate::armour::GearMods,
    pub weapon_mods: (i32, i32),
    /// The last blow's knockback (units, direction x strength) and when it landed (s).
    pub knock: [f32; 3],
    pub knock_at: f32,
}

impl Fighter {
    pub fn pos(&self) -> [f32; 3] {
        self.mover.position_units()
    }

    pub fn level(&self, s: StatsEnumerated) -> f32 {
        self.skills.level(s)
    }

    /// The skill the wielded weapon's category trains and uses (CharStats +0x1f8).
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
            WeaponCategory::SkillUnarmed => return None,
            _ => return None,
        })
    }

    /// F047041 at full load: martial arts (unarmed skill).
    fn martial(&self) -> f32 {
        martial_arts(1.0, self.gear.unarmed, self.level(StatsEnumerated::StatMartialarts), 1.0)
    }

    /// @fn F047029
    ///
    /// The attack bonus (CharStats +0x20): the armour's "combat attk bonus" summed plus the
    /// weapon's attack mod (F047028); likewise the defence bonus (+0x24).
    /// (stand-in) +0x28 (a squad bonus) and the injury penalty (F? 8495) are not added.
    pub fn attack_bonus(&self) -> i32 {
        self.gear.attack + self.weapon_mods.0
    }
    pub fn defence_bonus(&self) -> i32 {
        self.gear.defence + self.weapon_mods.1
    }

    /// @fn F047043
    ///
    /// Unarmed: martial arts (with the armour's unarmed bonus, F047030); else the attack bonus
    /// plus the weapon's skill plus melee attack (+0x120), times the character factor
    /// ((assumption) 1).
    pub fn attack_level(&self) -> f32 {
        match self.weapon_stat() {
            None => self.martial(),
            Some(w) => self.attack_bonus() as f32 + self.level(w) + self.level(StatsEnumerated::StatMeleeAttack),
        }
    }

    /// @fn F047060
    ///
    /// Unarmed: the attack level; else F047048's buffed defence at full load and health
    /// ((stand-in) the load and injury factors are 1 here). (assumption) F047048 reads the
    /// field +0xf0, which the STATS export names "dodge"; read as the raw dodge stat, equal
    /// fighters (attack = weapon skill + melee attack, F047043) would almost never block, so it
    /// is taken as the defence total: the defence bonus (+0x24, F047065) plus melee defence plus
    /// the weapon's skill, mirroring attack. F047048 then scales it by the armour's dodge mult
    /// (+0x38: + level x mult - level).
    pub fn defence_level(&self) -> f32 {
        match self.weapon_stat() {
            None => self.attack_level() * self.gear.dodge,
            Some(w) => {
                (self.defence_bonus() as f32 + self.level(w) + self.level(StatsEnumerated::StatMeleeDefence)) * self.gear.dodge
            }
        }
    }

    /// A ranged skill as the armour leaves it (F040433 with its fitness: crossbows and precision
    /// shooting x "ranged skill mult"; perception plus the armour's perception bonus).
    pub fn ranged_level(&self, s: StatsEnumerated) -> f32 {
        match s {
            StatsEnumerated::StatPerception => self.level(s) + self.gear.perception as f32,
            StatsEnumerated::StatCrossbows | StatsEnumerated::StatFriendlyFire => self.level(s) * self.gear.ranged,
            _ => self.level(s),
        }
    }

    /// Recompute what the stats set (CharStats::update F047065, the parts ported): weapon
    /// strain and handling, attack and defence speeds, run speed, the medical toughness.
    pub fn refresh(&mut self, sc: &StrainConstants, rm: &RaceMovement) {
        use StatsEnumerated::*;
        let strain = weapon_strain(self.weapon_weight, 1.0, self.level(StatStrength), sc);
        self.strain = strain;
        // the armour's combat speed (+0x1c) slows the handling, its dexterity mult the
        // dexterity (F040433 fitness), its athletics (+0x18) the run: (run - 11) x it + 11
        let handling = weapon_handling(strain, 1.0, self.gear.combat_speed, sc);
        let defence = self.defence_bonus() as f32
            + self.level(StatMeleeDefence)
            + self.weapon_stat().map_or(0.0, |w| self.level(w));
        let dex = self.level(StatDexterity) * self.gear.dexterity;
        let (a, d) = speeds(handling, dex, self.attack_level(), defence, false);
        self.attack_speed = a;
        self.defence_speed = d;
        self.run = (run_speed(self.level(StatAthletics), 70.0, 120.0, 1.0) - 11.0) * self.gear.athletics + 11.0;
        self.mover.set_max_speed(speed_limit(self.run, MoveSpeed::Run.cap(15.0).unwrap(), None));
        self.stats.toughness = self.level(StatToughness);
        let _ = rm;
    }

    /// The fighter's side of a combat event against `other` (trainFromCombat's inputs).
    fn training(&self, other: &Fighter, defending: bool) -> CombatTraining {
        let (mine, theirs) = if defending {
            (self.defence_level(), other.attack_level())
        } else {
            (self.attack_level(), other.defence_level())
        };
        CombatTraining {
            weapon: self.weapon_stat(),
            mine,
            theirs,
            load: 1.0,
            // (stand-in) armour weight is not tracked yet
            armour_share: 0.5,
            strain: self.strain,
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
    pub chance_factor: f32,
    pub unarmed_mult: f32,
    /// The global damage factor (the advanced options' "Global damage multiplier", 0x142133580).
    pub damage_mult: f32,
    pub xp: XpConstants,
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
    b.active && !b.down && b.team != a.team && a.foes.as_ref().is_none_or(|v| v.contains(&b.team))
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
/// The AI's two leash radii (AI::create F040019: +8 = 18, +0xc = 6) that make the distance
/// term's reach in its default mode 0 ((18 + 6) x 2, F038607).
pub const AI_RADII: (f32, f32) = (18.0, 6.0);

/// @fn F038638
///
/// The enemy `me` should fight: each standing enemy scored, the best above -9999 chosen.
/// Score: +5 for `current` (the target held); -1 when it is moving; minus 1 per fighter already
/// attacking it (not counting me), a downed one +2 with the attackers weighing 0.35; plus the
/// distance term 1 - d / reach (F038607, reach (18 + 6) x 2 in the AI's default mode 0); -2 for
/// one not in a fight, halved again when standing still.
/// (stand-in) The faction-relation value term (off in combat), the squad's chosen-target bonus
/// (+10 for a lone attacker), the hold leash, sneaking (x20), the ignore list and the
/// animal multiplier are not ported; teams stand in for factions.
pub fn score_targets(f: &[Fighter], me: usize, current: Option<usize>) -> Option<usize> {
    let p = f[me].pos();
    let reach = (AI_RADII.0 + AI_RADII.1) * 2.0;
    let mut best: Option<(usize, f32)> = None;
    for o in (0..f.len()).filter(|&o| o != me && opponent(&f[me], &f[o])) {
        let c = &f[o];
        let mut score = 0.0;
        if Some(o) == current {
            score += SCORE_TARGET;
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
        if c.down {
            score += 2.0;
            weight = 0.35;
        }
        score -= n * SCORE_ATTACKER * weight;
        let q = c.pos();
        let d = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt();
        score += 1.0 - d / reach;
        if !c.active {
            score += SCORE_NOT_FIGHTING;
            if !moving {
                score *= 0.5;
            }
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
    /// The direction from me toward `who`.
    fn toward(&self, who: usize) -> [f32; 3] {
        let (a, b) = (self.f[self.me].pos(), self.f[who].pos());
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
            busy: false,
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
                passive: false,
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
        let att = Attacking {
            category: me.wielded.category,
            skill: me.attack_level(),
            load: 1.0,
            prone: false,
            size: 1.0,
            distance: crate::states::attack_gap(self.dist(), c.radius, 1.0, other.radius),
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
            me.clips.is_some(),
            me.attack_level(),
            current,
            other.combat.state,
            me.attack_speed,
            &mut max_reach,
            |_| 1.0,
            |x, y| rng.range(x, y),
        );
        self.f[self.me].max_reach = max_reach;
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
                let (pos, tpos, tr, run) = (
                    me.pos(),
                    self.f[target].pos(),
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
}

impl Fighting for Ctx<'_> {
    type Id = usize;

    fn is_down(&self, who: usize) -> bool {
        self.f[who].down
    }
    fn is_animal(&self) -> bool {
        self.f[self.me].clips.is_some()
    }
    fn staggered(&self) -> bool {
        matches!(&self.f[self.me].act, Some(Act { kind: Kind::Stumble(_), clip, .. }) if clip.playing)
    }
    fn collision_radius(&self) -> f32 {
        self.f[self.me].radius
    }
    fn size(&self) -> f32 {
        1.0
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
    /// second ago; else the best scored standing enemy (F038638, [`score_targets`]).
    fn find_target(&mut self) -> Option<usize> {
        let me = &self.f[self.me];
        if let Some(t) = me.focus
            && me.pick_timer < 1.0
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

    /// CircleStateAI::update (F029599): hold the band around the target until attacked.
    fn circle(&mut self, c: &mut CombatClass<usize>, dt: f32) {
        let Some(target) = c.target else {
            c.set_state(self, SwordState::Idle);
            return;
        };
        let me = &self.f[self.me];
        let outer = circle_distance(me.max_reach, c.radius, extra_reach(me.wielded.length));
        let band = Band::new(c.radius, outer, me.radius, 0.0);
        let (pos, tpos, tr, run) = (
            me.pos(),
            self.f[target].pos(),
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
        // ask again; once the target holds a slot for me, go back to idle to attack
        if self.try_engage(c).is_some() {
            let (me, max_slots) = (self.me, self.sh.max_slots);
            let tf = &mut self.f[target];
            let count = slot_count(tf.combat.state == SwordState::Attack, false, max_slots, tf.extra_slots);
            if tf.slots.add(me, count) {
                c.set_state(self, SwordState::Idle);
            }
        }
        let timer = &mut self.f[self.me].circle_timer;
        *timer -= dt;
        if *timer <= 0.0 {
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
    /// its crowd; once the target accepts me as an engager, go back to idle; attacked -> idle.
    fn wait(&mut self, c: &mut CombatClass<usize>, dt: f32) {
        self.stop_moving();
        if let Some(t) = self.find_target() {
            c.target = Some(t);
        }
        if let Some(target) = c.target {
            let crowd = self.f[target].combat.crowd() as f32;
            let me = &self.f[self.me];
            let band = Band::new(crowd * 0.25 + 15.0, crowd * 3.0 + 25.0, me.radius, 0.0);
            let (pos, tpos, tr, run) = (
                me.pos(),
                self.f[target].pos(),
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
            if self.f[target].combat.accept_engager(me) {
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

    /// CombatClass::strike (F039803) on the target inside the swing box (F039793).
    fn strike(&mut self, c: &mut CombatClass<usize>, frame: i32) {
        let Some(Kind::Attack(k)) = self.f[self.me].act.as_ref().map(|a| a.kind) else {
            return;
        };
        let Some(o) = c.target else { return };
        let t = self.sh.t;
        let tech = self.sh.attacks[k].clone();
        let dist = self.dist();
        let to_me = self.toward(o).map(|v| -v);
        let (me, other) = pair_mut(self.f, self.me, o);
        // the box: from the move's attack distance less what the lunge already covered, pulled
        // back; the target is in front (the attacker faces it)
        let start = me.act.as_ref().map_or(me.pos(), |a| a.start);
        let now = me.pos();
        let moved = ((now[0] - start[0]).powi(2) + (now[2] - start[2]).powi(2)).sqrt();
        let leg = tech
            .frame(frame)
            .is_some_and(|f| f.limb == 2 || f.limb == 3);
        let reach = pull_back(
            swing_box(
                me.wielded.length,
                me.radius,
                1.0,
                leg,
                tech.attack_distance,
                moved,
            ),
            me.radius,
        );
        if !in_swing_box(&reach, 0.0, dist, other.radius) {
            return;
        }
        let dir = tech.frame(frame).map_or(1, |f| f.direction);
        let (action, dprog) = match &other.act {
            Some(Act {
                kind: Kind::Defence(d),
                clip,
                ..
            }) => {
                let dt_ = &self.sh.defences[*d];
                let a = if dt_.is_dodge {
                    Action::Dodge
                } else {
                    Action::Block {
                        direction: dt_.frame(i32::MAX).map_or(0, |f| f.direction),
                    }
                };
                (a, clip.progress)
            }
            _ => (Action::Other, 0.0),
        };
        // the defender faces its target; a blow from behind needs a rear block
        let facing = other
            .combat
            .target
            .map_or(to_me.map(|v| -v), |_| to_me.map(|v| -v));
        let behind = hit_from_behind(facing, to_me.map(|v| -v));
        let outcome = defend_outcome(false, action, dprog, dir, behind);
        c.recovery = recovery_time(outcome);
        if outcome != Defence::Hit {
            // trainFromCombat: the attacker's blow was stopped, the defender stopped it
            let (ta, td) = (me.training(other, false), other.training(me, true));
            me.skills.train_from_combat(CombatEvent::Blocked, &ta, &self.sh.xp);
            other.skills.train_from_combat(CombatEvent::Parried, &td, &self.sh.xp);
            log(
                t,
                format!(
                    "{} {:?} {}'s '{}'",
                    other.name, outcome, me.name, tech.anim_name
                ),
            );
            return;
        }
        let (str_, dex) = (me.level(StatsEnumerated::StatStrength), me.level(StatsEnumerated::StatDexterity));
        let att = me.wielded.attacker(me.weapon_stat().map_or(0.0, |w| me.level(w)), (dex, 1.0), (str_, 1.0), 1.0);
        let un = Unarmed {
            level: me.martial(),
            toughness: me.level(StatsEnumerated::StatToughness),
            toughness_scale: 1.0,
            unarmed_damage_mult: self.sh.unarmed_mult,
            limbs: 1.0,
            // (assumption) the character factor (vtable 0x398) is 1
            char_factor: 1.0,
        };
        let base = attack_damage(
            &me.wielded,
            &att,
            &self.sh.cc,
            &un,
            Some(Target {
                kind: TargetKind::Human,
                race: &self.sh.race,
            }),
            // the armour's damage output mult (CharStats +0x48)
            me.gear.damage_output,
        );
        let hit = frame_damage(base, &tech, frame);
        let ctx = HitContext {
            low_strike: tech.low_strike,
            direction: dir,
            robot: false,
            damage_mult: self.sh.damage_mult,
            race_bleed: 1.0,
        };
        let rng = &mut self.sh.rng;
        let r = resolve_hit(
            &mut other.medical,
            hit,
            &other.armour,
            &other.stats,
            &ctx,
            &self.sh.hc,
            &self.sh.mc,
            |a, b| rng.range(a, b),
        );
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
                "{} hits {} with '{}' frame {frame}: part {} (kind {}) cut {:.1} blunt {:.1} -> health {:.1}/{:.0}",
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
        // the hit reaction (Character::onHit, F033861)
        let total = r.damage.cut + r.damage.blunt + r.damage.pierce + r.damage.stun;
        // trainFromCombat: a blow landed, a blow taken
        let (ta, td) = (me.training(other, false), other.training(me, true));
        me.skills.train_from_combat(CombatEvent::Landed, &ta, &self.sh.xp);
        other.skills.train_from_combat(CombatEvent::Hit { damage: total }, &td, &self.sh.xp);
        other.stats.toughness = other.level(StatsEnumerated::StatToughness);
        let threshold = stagger_threshold(other.level(StatsEnumerated::StatToughness), 1.0, self.sh.stumble_max);
        let attacked = !other.combat.attackers.is_empty();
        let part_id = other.medical.parts[r.part].id.clone();
        // (assumption) an animal doesn't play the people's stumbles: its skeleton has none
        if !other.medical.knocked_out
            && other.clips.is_none()
            && let Some(big) = stagger(total, threshold, attacked)
            && let Some(st) = pick_stumble(
                &self.sh.stumbles,
                &part_id,
                &situation(other),
                1.0,
                i32::from(behind),
                big,
                other.last_stumble.as_deref(),
                self.sh.rng.next(),
            )
        {
            let s = self
                .sh
                .stumbles
                .iter()
                .position(|x| std::ptr::eq(x, st))
                .unwrap();
            let len = self.sh.clip_len(other, &st.anim_name);
            log(
                t,
                format!(
                    "{} staggers: '{}'{}",
                    other.name,
                    st.name,
                    if big { " (big)" } else { "" }
                ),
            );
            other.last_stumble = Some(st.name.clone());
            let start = other.pos();
            other.act = Some(Act {
                kind: Kind::Stumble(s),
                clip: SingleAnimation::new(&st.anim_name, len, 1.0, 1.0, false, vec![]),
                start,
            });
            other.combat.state = SwordState::Locked;
        }
        if other.medical.knocked_out {
            let ko = other
                .medical
                .compute_knockout_time(&other.stats, &self.sh.mc);
            log(t, format!("{} is knocked out ({ko:.0} s)", other.name));
            other.down = true;
            other.down_at = t;
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
        let my_skill = self.f[self.me].defence_level();
        let chance = block_chance(my_skill - skill, &self.sh.cc);
        let sloppy = chance < self.sh.rng.next() * 100.0;
        let d = Defender {
            category: self.f[self.me].wielded.category,
            prone: false,
            staggered: self.staggered(),
            dodge_ok: self.staggered(),
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
        match r.reaction {
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
                        if sloppy { " (wrong side)" } else { "" }
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
            _ => false,
        }
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
    fn block_chance(&self, skill: f32) -> f32 {
        block_chance(self.f[self.me].defence_level() - skill, &self.sh.cc)
    }
    /// CharStats::trainDefence (F047459) after the block roll.
    fn train_defence(&mut self, skill: f32, blocked: bool) {
        let xp = self.sh.xp;
        let f = &mut self.f[self.me];
        let mine = f.defence_level();
        f.skills.train_defence(
            mine,
            skill,
            blocked,
            1.0,
            xp.exp_gain * 0.1,
            xp.global,
            (xp.bonus_gap, xp.penalty_gap, 1),
        );
    }
    fn rand01(&mut self) -> f32 {
        self.sh.rng.next()
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
    Situation {
        category: 0,
        speed: (v[0] * v[0] + v[2] * v[2]).sqrt() * 10.0,
        weapon_l: false,
        weapon_r: true,
        ignore_legs: false,
        carrying_left: false,
        carrying_right: false,
        being_carried: false,
        combat_mode: 1,
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
        }
    }

    /// Add a fighter; its index.
    pub fn add(&mut self, x: Fighter) -> usize {
        self.layers.push(new_layers(self.blend_rate, x.pos()));
        self.f.push(x);
        self.f.len() - 1
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
            ..
        } = self;
        *t += dt;
        sh.t = *t;
        let t = *t;
        for i in 0..f.len() {
            if f[i].down || !f[i].active {
                continue;
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
                ranged_tick(f, sh, bolts, i, other, dt);
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
        // (assumption) a knocked-out fighter drops its target (setTarget to none, F040712)
        for i in 0..f.len() {
            if f[i].down
                && let Some(o) = f[i].combat.target.take()
            {
                f[o].combat.engagers.retain(|&e| e != i);
            }
        }
        *tick += 1;
        // the animation layers (F038910) and their root motion: while a clip with root motion
        // plays, the mover follows the change of the layers' weighted root position (turned to
        // the facing) - its 'animation driving' mode (F040617, F040615); otherwise its order
        let roots_now = record(layers, f, sh, roots, dt, t, *tick, *rows);
        for i in 0..f.len() {
            if f[i].down || !f[i].active {
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

/// (stand-in for the Havok characters' collision) Standing fighters in the fight don't overlap:
/// two closer than their footprint radii together are pushed apart along the line between
/// them, each half the overlap (on the ground plane).
fn separate(f: &mut [Fighter]) {
    let live: Vec<usize> = (0..f.len()).filter(|&i| f[i].active && !f[i].down).collect();
    for (a, &i) in live.iter().enumerate() {
        for &j in &live[a + 1..] {
            let (p, q) = (f[i].mover.position, f[j].mover.position);
            let (dx, dz) = (q[0] - p[0], q[2] - p[2]);
            let d = (dx * dx + dz * dz).sqrt();
            // radii in units, positions in metres
            let min = (f[i].radius + f[j].radius) * 0.1;
            if d >= min || (q[1] - p[1]).abs() > 1.5 {
                continue;
            }
            let (nx, nz) = if d < 1e-4 { (1.0, 0.0) } else { (dx / d, dz / d) };
            let push = (min - d) * 0.5;
            f[i].mover.position[0] -= nx * push;
            f[i].mover.position[2] -= nz * push;
            f[j].mover.position[0] += nx * push;
            f[j].mover.position[2] += nz * push;
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
        // (stand-in) shooting and standing, the crossbow stance "stand 1 crossbow" (category 8,
        // which the ported choice doesn't pick by)
        if fi.shooting && fi.act.is_none() && sit.speed.abs() <= 0.5
            && let Some(a) = sh.library.idles.iter().chain(&sh.library.other).find(|a| a.name == "stand 1 crossbow")
        {
            wanted = vec![(a, a.play_speed, 1.0)];
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
            // (an animal's clip has no root track here: no root motion)
            let Some(tr) = roots.get(&c.name.to_lowercase()) else { continue };
            let (r0, r1) = (root_at(tr, 0.0), root_at(tr, c.time));
            root[0] += c.weight * (r1[0] - r0[0]);
            root[1] += c.weight * (r1[1] - r0[1]);
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
                // (an animal's clip has no root track here: no root motion)
                let Some(tr) = roots.get(&c.name.to_lowercase()) else { continue };
                let (r0, r1) = (root_at(tr, 0.0), root_at(tr, c.time));
                off[0] += c.weight * (r1[0] - r0[0]);
                off[1] += c.weight * (r1[1] - r0[1]);
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
            chance_factor: g("attack chance factor").unwrap_or(0.0),
            unarmed_mult: g("unarmed damage mult").unwrap_or(1.0),
            damage_mult: 1.0,
            xp: XpConstants::from_record(|k| g(k)),
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
        let blood = Medical::max_blood(75.0, 150.0, skill * 0.01, 1.0, 1.0);
        medical.blood = blood;
        medical.max_blood = blood;
        let mover = Mover::for_character(&rm, position, 1.0, 1.0, false);
        let mut skills = Skills::new(g("exp gain multiplier").unwrap_or(3.0));
        for s in COMBAT_STATS {
            skills.levels.insert(s, skill);
        }
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
            weapon_weight: kit.weapon.combat_weight,
            combat: CombatClass::default(),
            act: None,
            slots: AttackerSlots::default(),
            order: Order::Hold,
            down: false,
            down_at: 0.0,
            clips: None,
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
            knock: [0.0; 3],
            knock_at: f32::NEG_INFINITY,
            gear: crate::armour::GearMods::default(),
            weapon_mods: (0, 0),
        };
        f.refresh(&sc, &rm);
        f
    }

    /// Wield the WEAPON item `weapon` (None: unarmed) and wear the ARMOUR items `armour`
    /// (CharStats::setWeapon F047095 and the armour list): what it hits with, its weapon
    /// skill, strain and speeds, and what covers it; each at its quality (0 .. 100), a weapon
    /// with its maker and model (F047085: the maker's damage and weight mods, the model's
    /// attack and defence mods).
    pub fn equip(&mut self, data: &GameDataContainer, race: &GameData, weapon: Option<&kenshi_char::inventory::Item>, armour: &[kenshi_char::inventory::Item]) {
        let g = |k: &str| global(data, k);
        let wi = weapon.filter(|i| data.get(&i.record).is_some_and(|r| r.item_type() == Some(ItemType::Weapon)));
        let w = wi.and_then(|i| data.get(&i.record));
        let item = wi.zip(w).map(|(i, r)| {
            let maker = i.maker.as_ref().and_then(|m| data.get(m));
            let model = i.model.as_ref().and_then(|m| data.get(m));
            Weapon::from_record(r, maker, model, i.quality * 0.01, g("weapon inventory weight mult").unwrap_or(0.5))
        });
        self.wielded = set_weapon(
            item.as_ref().zip(w),
            g("blunt damage 1").unwrap_or(20.0),
            g("cut damage 1").unwrap_or(20.0),
        );
        self.weapon_weight = item.as_ref().map_or(0.0, |i| i.combat_weight);
        let medical = &self.medical;
        let pieces: Vec<crate::armour::PieceMods> = armour
            .iter()
            .filter_map(|i| data.get(&i.record).map(|r| (i, r)))
            .filter(|(_, r)| r.item_type() == Some(ItemType::Armour))
            .filter_map(|(i, r)| crate::armour::piece_mods(r, i.quality * 0.01))
            .collect();
        self.gear = crate::armour::gear_mods(&pieces);
        self.weapon_mods = item.as_ref().map_or((0, 0), |w| (w.attack_mod, w.defence_mod));
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
/// (stand-in) The aim point on the target, not decompiled, is taken at the same height.
pub const AIM_HEIGHT: f32 = 10.0;
/// (stand-in) A character's height for bolts (no physics shapes): 18 units.
pub const BODY_HEIGHT: f32 = 18.0;

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
            radius: f[o].radius.max(2.0),
            height: BODY_HEIGHT,
            enemy: o != me && f[me].team != f[o].team && f[me].foes.as_ref().is_none_or(|v| v.contains(&f[o].team)),
        })
        .collect()
}

/// The line of fire from `from` to `to` (F033852's trace with F033779): the first character on it
/// short of the target, if not an enemy, holds the shot.
/// (stand-in) Only characters are traced: the ground and buildings are not (a bolt into the ground
/// strikes it in flight).
fn line_of_fire(f: &[Fighter], me: usize, target: usize, from: [f32; 3], to: [f32; 3]) -> crate::ranged::Line {
    use crate::ranged::{Line, cylinder_hit, dist_sq, normalise};
    let len = dist_sq(from, to).sqrt();
    let dir = normalise([to[0] - from[0], to[1] - from[1], to[2] - from[2]]);
    let first = bodies(f, me)
        .into_iter()
        .filter(|b| b.id != me)
        .filter_map(|b| cylinder_hit(from, dir, len, &b).map(|t| (t, b)))
        .min_by(|a, b| a.0.total_cmp(&b.0));
    match first {
        Some((t, b)) if b.id != target && !b.enemy => {
            if std::env::var("KENSHI_RANGED_DEBUG").is_ok() {
                eprintln!("line of fire from {from:?} to {to:?}: {} at {t:.0} ({:?}, radius {})", f[b.id].name, b.pos, b.radius);
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
fn ranged_tick(f: &mut [Fighter], sh: &mut Shared, bolts: &mut Vec<crate::ranged::Bolt>, i: usize, target: usize, dt: f32) {
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
            if line_of_fire(f, i, target, raised(me), raised(tp)) == Line::Clear {
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
    let aim_at = [tp[0], tp[1] + AIM_HEIGHT, tp[2]];
    let barrel = barrel_at(&f[i], &gun, aim_at);
    let mut r = f[i].ranged;
    let res = r.attempt(0 < gun.shots, dt, || line_of_fire(f, i, target, barrel, aim_at));
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
    /// terrain's height (units). Stuck bolts follow whom they are in; (stand-in) at most 300 are
    /// kept, the oldest stuck ones going first.
    pub fn step_bolts(&mut self, dt: f32, ground: &mut dyn FnMut(f32, f32) -> f32) {
        use crate::ranged::{BOLT_BLEED, BoltHit, BoltState};
        let Arena { f, sh, bolts, .. } = self;
        for k in 0..bolts.len() {
            let shooter = bolts[k].shooter;
            let list = bodies(f, shooter);
            let rng = &mut sh.rng;
            let hit = bolts[k].update(dt, &list, ground, || rng.next());
            match hit {
                Some(BoltHit::Character(v)) => {
                    let b = &bolts[k];
                    let hit = crate::hit::Hit { pierce: b.damage, bleed: BOLT_BLEED, ..Default::default() };
                    let ctx = HitContext { low_strike: false, direction: 6, robot: false, damage_mult: sh.damage_mult, race_bleed: 1.0 };
                    let victim = &mut f[v];
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
        while bolts.len() > 300 {
            let i = bolts.iter().position(|b| b.state != BoltState::Flying).unwrap_or(0);
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
    /// (stand-in) The target is the one held while it stays in reach, else the nearest in the
    /// arc (F033850's threat weights with F033781's arc weight 1 / 0.1 are not ported); the
    /// line of fire is not traced (the gunner stands above, on walls).
    pub fn step_turrets(&mut self, dt: f32) {
        use crate::ranged::*;
        use StatsEnumerated::*;
        let Arena { f, sh, bolts, turrets, .. } = self;
        for t in turrets.iter_mut() {
            let Some(o) = t.operator.filter(|&o| o < f.len() && !f[o].down) else {
                t.target = None;
                continue;
            };
            let aim_at = |q: [f32; 3]| [q[0], q[1] + AIM_HEIGHT, q[2]];
            let reach = |t: &Turret, q: [f32; 3]| t.gun.in_range(t.pivot_world(), q) && t.in_arc(q);
            let foe = |k: usize| {
                k != o && !f[k].down && f[k].team != f[o].team && f[o].foes.as_ref().is_none_or(|v| v.contains(&f[k].team))
            };
            if t.target.is_none_or(|k| !foe(k) || !reach(t, f[k].pos())) {
                let p = t.pivot_world();
                t.target = (0..f.len())
                    .filter(|&k| foe(k) && reach(t, f[k].pos()))
                    .min_by(|&a, &b| dist_sq(f[a].pos(), p).total_cmp(&dist_sq(f[b].pos(), p)));
            }
            let skill = f[o].level(StatTurrets);
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
            let goal = aim_at(f[k].pos());
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
            if TURRET_AIMED < off {
                continue;
            }
            let mut r = t.ranged;
            let res = r.attempt(0 < t.gun.shots, dt, || Line::Clear);
            t.ranged = r;
            if res == 1 {
                let (per, pre) = (f[o].ranged_level(StatPerception), f[o].ranged_level(StatFriendlyFire));
                let barrel = t.barrel();
                let rng = &mut sh.rng;
                let bolt = fire(&mut t.gun, o, Some(k), barrel, goal, skill, per, pre, || rng.next());
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
