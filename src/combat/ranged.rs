//! Ranged combat (rangedCombat.cpp): a character's crossbow (GunClassPersonal), the aim, fire and
//! reload loop its ranged-combat state runs, and the bolts in flight (HarpoonManager's harpoons).
//!
//! Specs: F033878 GunClass ctor, F033879 GunClassPersonal ctor, F033825 reload time, F033824
//! height factor, F033838 in range, F033858 ranged update, F033852 fire attempt, F033853 reload,
//! F033851 enemy close, F033867 fire, F033856 harpoon update, F033863 onProjectileHit, F047468
//! skill use XP rates, F037165 reload / aim clips.
//!
//! Units are world units (10 to the metre); positions are y up.

use kenshi_data::{GameData, GameDataContainer};

/// The aim timer a loaded crossbow counts down before a shot (F033852, 0x3ecccccd).
pub const AIM_TIME: f32 = 0.4;
/// How long a clear line of fire is held while walking up before stopping to shoot (F033858).
pub const CLEAR_TIME: f32 = 0.3;
/// An enemy this close (squared, 40 units) ends ranged combat (F033851, 1600).
pub const CLOSE_SQ: f32 = 1600.0;
/// Bolts drop (pitch) this many degrees a second once past their flight distance (F033856).
pub const DROP_DEG: f32 = 8.0;
/// A bolt is gone once this far past its flight distance (F033856, -5000).
pub const GONE: f32 = -5000.0;
/// The bleed a bolt's damage carries (F033863, the damage's +0x10 = 2).
pub const BOLT_BLEED: f32 = 2.0;
/// The XP rates of the skills ranged combat uses each tick (F047468): crossbows 0.8 (falling
/// through to farming's x1.5 in the original's switch: 1.2), perception 3, dexterity the default
/// 0.5.
pub const XP_CROSSBOWS: f32 = 0.8 * 1.5;
pub const XP_PERCEPTION: f32 = 3.0;
pub const XP_DEXTERITY: f32 = 0.5;
/// Turrets' XP rate (F047468: 1).
pub const XP_TURRETS: f32 = 1.0;

/// (GunClass, not in the type sheet)
///
/// A crossbow as its GunClassPersonal holds it (F033878, F033879).
#[derive(Debug, Clone, PartialEq)]
pub struct Gun {
    /// The CROSSBOW record.
    pub record: String,
    /// "range" (+0x8): lerped to "range 1" by quality.
    pub range: f32,
    /// "accuracy perfect skill" (+0xc).
    pub accuracy_perfect: f32,
    /// "accuracy deviation at 0 skill" (+0x10), degrees: lerped to its "1".
    pub deviation: f32,
    /// "aim speed" (+0x14).
    pub aim_speed: f32,
    /// "shot speed" (+0x18), units/s: lerped to its "1"; 1 when not above 0.
    pub shot_speed: f32,
    /// "num shots" (+0x1c) and the shots loaded (+0x20).
    pub num_shots: i32,
    pub shots: i32,
    /// "reload time min / max" (+0x24, +0x28), s: lerped to their "1".
    pub reload_min: f32,
    pub reload_max: f32,
    /// The user's skill / 100 (+0x2c, set before each shot and reload).
    pub skill: f32,
    /// "barrel pos Y / Z" (+0x90): the barrel's offset (0, y, z) on the weapon.
    pub barrel: [f32; 3],
    /// "pierce damage min / max" (+0x118, +0x11c), lerped "0" to "1" by quality, times the
    /// global damage factor, as whole numbers.
    pub damage_min: i32,
    pub damage_max: i32,
    /// The "ammo" ITEM it loads, if it needs one (+0x58).
    pub ammo: Option<String>,
    /// The "reload anim" ANIMATION (else "reload 1 phase", F037165).
    pub reload_anim: Option<String>,
    /// "live ammo": the bolt's mesh in flight.
    pub live_ammo: Option<String>,
    /// "shoot sound" (enum into WEAPONS switch names; personal crossbows sound as "Crossbow").
    pub shoot_sound: i32,
}

fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * t
}

impl Gun {
    /// @fn F033879
    ///
    /// A personal crossbow from its CROSSBOW record at `quality` (0 .. 100), `damage_mult` the
    /// global damage factor (0x142133580).
    /// (assumption) The lerp's factor, lost in the decompilation, is the item's quality / 100.
    pub fn personal(data: &GameDataContainer, r: &GameData, quality: f32, damage_mult: f32) -> Gun {
        let t = (quality * 0.01).clamp(0.0, 1.0);
        let f = |k: &str| r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|v| *v as f32));
        let pair = |a: &str, b: &str, d: f32| {
            let lo = f(a).unwrap_or(d);
            lerp(t, lo, f(b).unwrap_or(lo))
        };
        let reference = |k: &str| r.refs(k).first().map(|(id, _)| id.clone()).filter(|id| data.get(id).is_some());
        let shot_speed = pair("shot speed", "shot speed 1", 1.0);
        Gun {
            record: r.string_id.clone(),
            range: pair("range", "range 1", 1000.0),
            accuracy_perfect: f("accuracy perfect skill").unwrap_or(0.0),
            deviation: pair("accuracy deviation at 0 skill", "accuracy deviation at 0 skill 1", 0.0),
            aim_speed: f("aim speed").unwrap_or(1.0),
            shot_speed: if shot_speed <= 0.0 { 1.0 } else { shot_speed },
            num_shots: f("num shots").unwrap_or(1.0).max(1.0) as i32,
            shots: 0,
            reload_min: pair("reload time min", "reload time min 1", 1.0),
            reload_max: pair("reload time max", "reload time max 1", 1.0),
            skill: 0.0,
            barrel: [0.0, f("barrel pos Y").unwrap_or(0.0), f("barrel pos Z").unwrap_or(0.0)],
            damage_min: (pair("pierce damage min 0", "pierce damage min 1", 0.0) * damage_mult) as i32,
            damage_max: (pair("pierce damage max 0", "pierce damage max 1", 0.0) * damage_mult) as i32,
            ammo: reference("ammo"),
            reload_anim: reference("reload anim").and_then(|id| data.get(&id)).map(|a| a.name.clone()),
            live_ammo: r.files.get("live ammo").filter(|s| !s.is_empty()).cloned(),
            shoot_sound: f("shoot sound").unwrap_or(0.0) as i32,
        }
    }

    /// @fn F033825
    ///
    /// The reload time at skill `t` (level / 100): from max at 0 to min at 1, kept within them.
    pub fn reload_time(&self, t: f32) -> f32 {
        lerp(t, self.reload_max, self.reload_min).min(self.reload_max).max(self.reload_min)
    }

    /// @fn F033838
    ///
    /// Whether `to` is in range from `from`: closer than the range times the height factor.
    pub fn in_range(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        let r = height_factor(from[1], to[1]) * self.range;
        dist_sq(from, to) < r * r
    }
}

/// @fn F033824
///
/// Shooting down reaches further and hits harder, up less: by the height the shot falls (`from`
/// above `to`), from 20 to 300 units, up to x1.5; by the height it climbs, down to x0.6. A target at
/// height 0 is taken as level (1). Under 20 units the factor runs slightly past 1 the other way, as
/// the original's unclamped inverse lerp does.
pub fn height_factor(from_y: f32, to_y: f32) -> f32 {
    if to_y == 0.0 {
        return 1.0;
    }
    let dy = from_y - to_y;
    let (t, end) = if dy <= 0.0 { ((dy.abs() - 20.0) / 280.0, 0.6) } else { ((dy - 20.0) / 280.0, 1.5) };
    (end - 1.0) * t.min(1.0) + 1.0
}

pub fn dist_sq(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn normalise(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l < 1e-9 { [0.0, 0.0, 1.0] } else { scale(a, 1.0 / l) }
}

/// `v` turned by `angle` radians about the unit `axis` (Rodrigues; Ogre's Quaternion(angle, axis)
/// applied to a vector).
pub fn rotate(v: [f32; 3], axis: [f32; 3], angle: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    let k = normalise(axis);
    add(add(scale(v, c), scale(cross(k, v), s)), scale(k, dot(k, v) * (1.0 - c)))
}

/// Ogre's Vector3::randomDeviant(angle, UNIT_Y): the up vector turned about the direction by a
/// random whole-turn angle (`roll` in [0, 1)), then the direction turned about that by `angle`.
pub fn random_deviant(dir: [f32; 3], angle: f32, roll: f32) -> [f32; 3] {
    let up = rotate([0.0, 1.0, 0.0], dir, roll * std::f32::consts::TAU);
    rotate(dir, up, angle)
}

/// Ranged combat's state (the RangedCombat object of F033858).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Ranged {
    /// 0 standing to shoot, 1 moving into a line of fire, 2 out of shots, 3 no target.
    pub state: i32,
    /// The aim timer (+0x70).
    pub aim: f32,
    /// Time the line of fire has been clear while walking up (+0x7c).
    pub clear: f32,
    /// Reloading (+0x80): the time left (+0x78) of the whole (+0x74).
    pub reloading: bool,
    pub reload_left: f32,
    pub reload_total: f32,
    /// The reload's progress shown (the gun's +0xe0: 1 - left / total, kept in 0 .. 1).
    pub progress: f32,
    /// Shots fired, for the shoot clip's restart.
    pub fired: u32,
    /// Seconds since the last shot.
    pub since_shot: f32,
}

impl Ranged {
    /// @fn F033853
    ///
    /// The reload: loaded, true. Out of shots, a reload starts when no ammo is needed or
    /// `has_ammo`; then the time runs down (the reload clip shown by its progress) and at its end
    /// `take_ammo` spends one, the shots are filled and the aim timer set. False while not loaded.
    /// The caller trains crossbows and dexterity each tick it returns false with a reload running.
    pub fn reload(&mut self, gun: &mut Gun, skill: f32, dt: f32, has_ammo: bool, mut take_ammo: impl FnMut()) -> bool {
        gun.skill = skill;
        if !self.reloading {
            if 0 < gun.shots {
                return true;
            }
            if gun.ammo.is_none() || has_ammo {
                self.progress = 0.0;
                let t = gun.reload_time(skill);
                self.reloading = true;
                self.reload_total = t;
                self.reload_left = t;
            }
            return false;
        }
        self.reload_left -= dt;
        self.progress = (1.0 - self.reload_left / self.reload_total.max(1e-6)).clamp(0.0, 1.0);
        if self.reload_left <= 0.0 {
            if gun.ammo.is_some() {
                take_ammo();
            }
            // F029767 -> F033845: the shots filled
            gun.shots = gun.num_shots;
            self.reloading = false;
            self.progress = 0.0;
            self.aim = AIM_TIME;
        }
        false
    }

    /// @fn F033852
    ///
    /// The fire attempt once aimed: while not loaded the aim timer is held at 0.4; loaded, it runs
    /// down and then the line of fire is looked along. 2: hold (no target, or a character who
    /// isn't an enemy in the way); 0: blocked; 1: fire.
    /// (assumption) "aimed" (the gun's +0x68 at most 1) is taken as always so: the body turns to
    /// its target as the renderer draws it.
    pub fn attempt(&mut self, loaded: bool, dt: f32, line: impl FnOnce() -> Line) -> i32 {
        if !loaded {
            self.aim = AIM_TIME;
            return 2;
        }
        self.aim -= dt;
        if 0.0 < self.aim {
            return 2;
        }
        match line() {
            Line::Friend => 2,
            Line::Blocked => 0,
            Line::Clear => 1,
        }
    }
}

/// What a trace along the line of fire found (F033852 with F033779).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Clear,
    /// The ground (or a thing) before the target.
    Blocked,
    /// A character not the target and not an enemy.
    Friend,
}

/// Where a bolt is (the harpoon's +0x70).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoltState {
    Flying,
    /// Stuck in a character: (who, its offset from their position).
    InCharacter(usize, [f32; 3]),
    /// Stuck in the ground.
    InGround,
}

/// A bolt (Harpoon, F033836) in the world.
#[derive(Debug, Clone, PartialEq)]
pub struct Bolt {
    pub pos: [f32; 3],
    /// Its unit direction (the node's z axis).
    pub dir: [f32; 3],
    /// Units / s (+0x44).
    pub speed: f32,
    /// The flight distance left (+0x40): past it the bolt drops.
    pub left: f32,
    /// The pierce damage it deals (+0x4c).
    pub damage: f32,
    /// The chance it passes a friend by (+0x48: precision shooting x 0.015, at most 1).
    pub pass: f32,
    pub shooter: usize,
    pub target: Option<usize>,
    /// Seconds since fired (+0x60).
    pub age: f32,
    pub state: BoltState,
    /// The "live ammo" mesh.
    pub mesh: Option<String>,
}

/// What a bolt met this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoltHit {
    /// A character, to be dealt the bolt's damage.
    Character(usize),
    /// The ground at this point (the "Impact" sound, Impacts "Stone").
    Ground([f32; 3]),
}

/// @fn F033867
///
/// A shot: one shot spent, the bolt sent from the barrel toward `aim_at`, turned off the line by
/// up to the deviation (lerped from "accuracy deviation at 0 skill" down to none by crossbow skill
/// x 0.005 plus perception x 0.005, kept at least 0) times a roll, in degrees. Its damage is a roll
/// between the pierce damages, its flight distance the range, its speed 0.2 of the shot speed plus
/// 0.8 of it scaled - each scaled by the height factor. `precision` is the precision shooting
/// level. `roll()` is uniform in [0, 1).
pub fn fire(gun: &mut Gun, shooter: usize, target: Option<usize>, barrel: [f32; 3], aim_at: [f32; 3], skill_level: f32, perception: f32, precision: f32, mut roll: impl FnMut() -> f32) -> Bolt {
    gun.skill = skill_level * 0.01;
    gun.shots -= 1;
    let hf = height_factor(barrel[1], aim_at[1]);
    let t = skill_level * 0.005 + perception * 0.005;
    let deviation = lerp(t, gun.deviation, 0.0).max(0.0);
    let angle = (roll() * deviation).to_radians();
    let dir = random_deviant(normalise(sub(aim_at, barrel)), angle, roll());
    let damage = (gun.damage_min as f32 + (gun.damage_max - gun.damage_min) as f32 * roll()) * hf;
    Bolt {
        pos: barrel,
        dir,
        speed: gun.shot_speed * 0.8 * hf + gun.shot_speed * 0.2,
        left: gun.range * hf,
        damage,
        pass: (precision * 0.015).min(1.0),
        shooter,
        target,
        age: 0.0,
        state: BoltState::Flying,
        mesh: gun.live_ammo.clone(),
    }
}

/// A character as a bolt sees it: an upright cylinder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub id: usize,
    /// Feet position.
    pub pos: [f32; 3],
    pub radius: f32,
    pub height: f32,
    /// Whether the shooter fights them (a hit on anyone else may pass by).
    pub enemy: bool,
}

/// The distance along `dir` from `from` to where the segment first enters the cylinder, if within
/// `len`.
pub fn cylinder_hit(from: [f32; 3], dir: [f32; 3], len: f32, b: &Body) -> Option<f32> {
    let (ox, oz) = (from[0] - b.pos[0], from[2] - b.pos[2]);
    let (dx, dz) = (dir[0], dir[2]);
    let a = dx * dx + dz * dz;
    let c = ox * ox + oz * oz - b.radius * b.radius;
    let t = if c <= 0.0 {
        0.0
    } else if a < 1e-9 {
        return None;
    } else {
        let bb = ox * dx + oz * dz;
        let disc = bb * bb - a * c;
        if disc < 0.0 {
            return None;
        }
        (-bb - disc.sqrt()) / a
    };
    if !(0.0..=len).contains(&t) {
        return None;
    }
    let y = from[1] + dir[1] * t - b.pos[1];
    (0.0..=b.height).contains(&y).then_some(t)
}

impl Bolt {
    /// @fn F033856
    ///
    /// A tick of flight: the bolt looks ahead twice its step; a character in the way is hit -
    /// unless they aren't the target nor the shooter's enemy and a roll under the pass chance
    /// lets the bolt by; the ground stops it. Otherwise it moves on, and once past its flight
    /// distance its nose drops 8 degrees a second, gone 5000 units later.
    /// (as Kenshi) The drop is a pitch about the world x axis (Node::pitch in world space), so a
    /// bolt flying along x rolls instead of dropping and one flying toward -z climbs.
    /// (stand-in) Characters are upright cylinders (no physics shapes); the shooter is never hit;
    /// the ground is `ground(x, z)` (buildings and water are not traced).
    pub fn update(&mut self, dt: f32, bodies: &[Body], ground: &mut dyn FnMut(f32, f32) -> f32, mut roll: impl FnMut() -> f32) -> Option<BoltHit> {
        self.age += dt;
        if self.state != BoltState::Flying {
            return None;
        }
        let step = self.speed * dt;
        self.left -= step;
        let ahead = step * 2.0;
        let mut best: Option<(f32, usize, bool)> = None;
        for b in bodies.iter().filter(|b| b.id != self.shooter) {
            if let Some(t) = cylinder_hit(self.pos, self.dir, ahead, b)
                && best.is_none_or(|(d, _, _)| t < d)
            {
                best = Some((t, b.id, b.enemy));
            }
        }
        let ground_t = {
            let n = 4;
            (1..=n).map(|k| ahead * k as f32 / n as f32).find(|&s| {
                let p = add(self.pos, scale(self.dir, s));
                p[1] <= ground(p[0], p[2])
            })
        };
        if let Some((t, who, enemy)) = best
            && ground_t.is_none_or(|g| t <= g)
        {
            let passes = Some(who) != self.target && !enemy && roll() < self.pass;
            if !passes {
                let hit_at = add(self.pos, scale(self.dir, t));
                let feet = bodies.iter().find(|b| b.id == who).map_or(hit_at, |b| b.pos);
                self.pos = hit_at;
                self.state = BoltState::InCharacter(who, sub(hit_at, feet));
                return Some(BoltHit::Character(who));
            }
        }
        if let Some(g) = ground_t {
            let at = add(self.pos, scale(self.dir, g));
            self.pos = at;
            self.state = BoltState::InGround;
            return Some(BoltHit::Ground(at));
        }
        self.pos = add(self.pos, scale(self.dir, step));
        if self.left <= 0.0 {
            self.dir = normalise(rotate(self.dir, [1.0, 0.0, 0.0], (dt * DROP_DEG).to_radians()));
            if self.left < GONE {
                self.age = f32::INFINITY;
            }
        }
        None
    }

    /// Gone: flown out (marked by an infinite age).
    pub fn gone(&self) -> bool {
        self.age.is_infinite()
    }
}

/// @fn F033878
///
/// A mounted gun (GunClassTurret) from its GUN_DATA record, as the GunClass constructor reads it
/// (no quality): its damage is the building's output rate x 0.75 to x 1.25 (F033802, F033803:
/// Building::researchOutputRate, F029301).
/// (stand-in) Research raising the output rate is not applied: the record's "output rate".
pub fn turret_gun(r: &GameData, output_rate: f32) -> Gun {
    let f = |k: &str| r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|v| *v as f32));
    let shot_speed = f("shot speed").unwrap_or(1.0);
    Gun {
        record: r.string_id.clone(),
        range: f("range").unwrap_or(1000.0),
        accuracy_perfect: f("accuracy perfect skill").unwrap_or(0.0),
        deviation: f("accuracy deviation at 0 skill").unwrap_or(0.0),
        aim_speed: f("aim speed").unwrap_or(1.0),
        shot_speed: if shot_speed <= 0.0 { 1.0 } else { shot_speed },
        num_shots: f("num shots").unwrap_or(1.0).max(1.0) as i32,
        shots: 0,
        reload_min: f("reload time min").unwrap_or(1.0),
        reload_max: f("reload time max").unwrap_or(1.0),
        skill: 0.0,
        barrel: [0.0, f("barrel pos Y").unwrap_or(0.0), f("barrel pos Z").unwrap_or(0.0)],
        damage_min: (output_rate * 0.75) as i32,
        damage_max: (output_rate * 1.25) as i32,
        ammo: None,
        reload_anim: None,
        live_ammo: r.files.get("live ammo").filter(|s| !s.is_empty()).cloned(),
        shoot_sound: f("shoot sound").unwrap_or(0.0) as i32,
    }
}

/// A mounted crossbow or harpoon (TurretBuilding with its GunClassTurret): where it stands, how
/// its head is turned, its gun and whoever mans it.
#[derive(Debug, Clone)]
pub struct Turret {
    /// The building's world position (units) and facing (yaw, radians; its +z forward).
    pub at: [f32; 3],
    pub facing: f32,
    /// The swivel's pivot above the building's origin (the turning part's offset, units).
    pub pivot: [f32; 3],
    /// The head's turn relative to the building (radians): yaw about y, pitch about x.
    pub yaw: f32,
    pub pitch: f32,
    /// How fast it turns (rad/s): power factor x "aim speed" x (1 + output rate x 0.01) (F033785).
    /// (assumption) F03c5ce turns the head by this many radians a second.
    pub turn: f32,
    pub gun: Gun,
    pub ranged: Ranged,
    /// The gunner (Task_UseTurret, F031217) and the turret's target.
    pub operator: Option<usize>,
    pub target: Option<usize>,
    /// The turret's sim identity: the furniture or player building it is.
    pub furniture: Option<usize>,
    pub building: Option<usize>,
    /// Frames left before the head's movement sound stops ("Turret_Mvt_Stop", F033784).
    pub moving: i32,
}

/// Half the fire arc (F033781's arc test, the record's "180 degree fire arc").
pub const TURRET_HALF_ARC: f32 = std::f32::consts::FRAC_PI_2;
/// The head counts as aimed within this angle (the gun's aim error +0x68 at most 1 degree).
pub const TURRET_AIMED: f32 = 1.0 * std::f32::consts::PI / 180.0;

impl Turret {
    pub fn new(at: [f32; 3], facing: f32, pivot: [f32; 3], gun: Gun, power: f32, output_rate: f32) -> Turret {
        let turn = power * gun.aim_speed * (1.0 + output_rate * 0.01);
        Turret { at, facing, pivot, yaw: 0.0, pitch: 0.0, turn, gun, ranged: Ranged::default(), operator: None, target: None, furniture: None, building: None, moving: 0 }
    }

    /// The swivel's pivot in the world.
    pub fn pivot_world(&self) -> [f32; 3] {
        let (s, c) = self.facing.sin_cos();
        let [x, y, z] = self.pivot;
        [self.at[0] + x * c + z * s, self.at[1] + y, self.at[2] - x * s + z * c]
    }

    /// The direction the head points (world).
    pub fn aim_dir(&self) -> [f32; 3] {
        let a = self.facing + self.yaw;
        let (cp, sp) = (self.pitch.cos(), self.pitch.sin());
        [a.sin() * cp, -sp, a.cos() * cp]
    }

    /// The barrel's tip: the pivot plus the barrel offset (0, y, z) along the head.
    pub fn barrel(&self) -> [f32; 3] {
        let d = self.aim_dir();
        let p = self.pivot_world();
        [p[0] + d[0] * self.gun.barrel[2], p[1] + self.gun.barrel[1] + d[1] * self.gun.barrel[2], p[2] + d[2] * self.gun.barrel[2]]
    }

    /// Whether `to` lies in the fire arc (in front of the building, within 90 degrees).
    pub fn in_arc(&self, to: [f32; 3]) -> bool {
        let p = self.pivot_world();
        let a = (to[0] - p[0]).atan2(to[2] - p[2]);
        wrap(a - self.facing).abs() <= TURRET_HALF_ARC
    }

    /// The head turned toward `to` by at most `turn * dt`: the angle still off afterwards.
    pub fn turn_to(&mut self, to: [f32; 3], dt: f32) -> f32 {
        let p = self.pivot_world();
        let (dx, dy, dz) = (to[0] - p[0], to[1] - p[1], to[2] - p[2]);
        let want_yaw = wrap(dx.atan2(dz) - self.facing).clamp(-TURRET_HALF_ARC, TURRET_HALF_ARC);
        let want_pitch = (-dy).atan2((dx * dx + dz * dz).sqrt());
        let step = self.turn * dt;
        let dy_ = wrap(want_yaw - self.yaw);
        let dp_ = want_pitch - self.pitch;
        self.yaw += dy_.clamp(-step, step);
        self.pitch += dp_.clamp(-step, step);
        (wrap(want_yaw - self.yaw).powi(2) + (want_pitch - self.pitch).powi(2)).sqrt()
    }
}

fn wrap(a: f32) -> f32 {
    let t = std::f32::consts::TAU;
    let mut a = a % t;
    if a > std::f32::consts::PI {
        a -= t;
    } else if a < -std::f32::consts::PI {
        a += t;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gun() -> Gun {
        Gun {
            record: String::new(),
            range: 1500.0,
            accuracy_perfect: 80.0,
            deviation: 2.5,
            aim_speed: 1.0,
            shot_speed: 700.0,
            num_shots: 1,
            shots: 0,
            reload_min: 5.0,
            reload_max: 10.0,
            skill: 0.0,
            barrel: [0.0, -1.0, 5.9],
            damage_min: 50,
            damage_max: 70,
            ammo: None,
            reload_anim: None,
            live_ammo: None,
            shoot_sound: 0,
        }
    }

    #[test]
    fn reload_time_runs_from_max_to_min() {
        let g = gun();
        assert_eq!(g.reload_time(0.0), 10.0);
        assert_eq!(g.reload_time(1.0), 5.0);
        assert_eq!(g.reload_time(2.0), 5.0);
        assert_eq!(g.reload_time(0.5), 7.5);
    }

    #[test]
    fn height_reaches_further_downhill() {
        assert_eq!(height_factor(100.0, 0.0), 1.0);
        assert!((height_factor(320.0, 20.0) - 1.5).abs() < 1e-6);
        assert!((height_factor(20.0, 320.0) - 0.6).abs() < 1e-6);
        assert!(height_factor(50.0, 50.0) > 1.0);
    }

    #[test]
    fn reload_fills_after_its_time() {
        let mut g = gun();
        let mut r = Ranged::default();
        assert!(!r.reload(&mut g, 0.0, 0.1, false, || {}));
        assert!(r.reloading);
        for _ in 0..99 {
            assert!(!r.reload(&mut g, 0.0, 0.1, false, || {}));
        }
        assert!(!r.reload(&mut g, 0.0, 0.1, false, || {}));
        assert_eq!(g.shots, 1);
        assert!(r.reload(&mut g, 0.0, 0.1, false, || {}));
    }

    #[test]
    fn a_shot_spends_and_flies_to_its_target() {
        let mut g = gun();
        g.shots = 1;
        let mut k = 0;
        let mut rolls = || {
            k += 1;
            0.0
        };
        let mut b = fire(&mut g, 0, Some(1), [0.0, 10.0, 0.0], [0.0, 10.0, 300.0], 100.0, 100.0, 0.0, &mut rolls);
        assert_eq!(g.shots, 0);
        assert!((b.dir[2] - 1.0).abs() < 1e-5);
        let target = Body { id: 1, pos: [0.0, 0.0, 300.0], radius: 3.0, height: 20.0, enemy: true };
        let mut hit = None;
        for _ in 0..200 {
            if let Some(h) = b.update(1.0 / 60.0, &[target], &mut |_, _| -100.0, || 0.5) {
                hit = Some(h);
                break;
            }
        }
        assert_eq!(hit, Some(BoltHit::Character(1)));
        assert_eq!(b.damage, 50.0 * height_factor(10.0, 10.0));
    }

    #[test]
    fn a_friend_may_be_passed() {
        let friend = Body { id: 2, pos: [0.0, 0.0, 100.0], radius: 3.0, height: 20.0, enemy: false };
        let mut b = Bolt {
            pos: [0.0, 10.0, 90.0],
            dir: [0.0, 0.0, 1.0],
            speed: 600.0,
            left: 1000.0,
            damage: 10.0,
            pass: 1.0,
            shooter: 0,
            target: Some(1),
            age: 0.0,
            state: BoltState::Flying,
            mesh: None,
        };
        assert_eq!(b.update(1.0 / 60.0, &[friend], &mut |_, _| -100.0, || 0.5), None);
        b.pass = 0.0;
        b.pos = [0.0, 10.0, 90.0];
        assert_eq!(b.update(1.0 / 60.0, &[friend], &mut |_, _| -100.0, || 0.5), Some(BoltHit::Character(2)));
    }
}
