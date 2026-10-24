//! Kenshi's effects: EFFECT records (F023262) run as particle systems (EffectsManager F033127,
//! the wrapper F033084, the Effect F023225..F023268), placed by effect handlers in effect
//! groups (F023405..F023435) that each weather's "effects" make (F051022: count, min and max
//! respawn seconds).
//!
//! The handlers by EFFECT "type" (F023419):
//! - CAMERA, CAMERA_RAIN, CAMERA_ACID_RAIN: one system ahead of the camera, its particles
//!   wrapped into a box round the camera (F023396, F023397), while the camera's weather region
//!   is the effect's (F023373).
//! - GLOBAL: one system at the camera's target, its particles kept round it (F023401, F023399).
//! - POINT, POINT_LIGHTING: systems at random points of the region (F023424, F048159).
//! - WANDERING, WANDERING_STORM, WANDERING_GAS: the same, wandering with the wind (F023412).
//! - GLOBAL_POINT: systems kept in two rings round the camera's target (F023435, F023434).
//!
//! Point, wandering and global point systems run while their zone is loaded and is in the
//! group's region (F023369, F048106); each lives "min/max time to live" seconds (0: for ever)
//! and fades out ("particle fade out delay"; F023411 drops it once the "fog fade out duration"
//! has passed and its particles are gone).
//!
//! Each effect's fog volumes ("fog volumes", EFFECT_FOG_VOLUME) go with its system, faded in
//! over its "fog fade in duration" once it starts and out over its "fog fade out duration" once
//! it stops (F023246, F023247, F023243); `crate::fog_volume` draws them.
//!
//! A POINT_LIGHTING system is aimed at the object nearest its random point (F023424 with
//! F045501 over the list at 0x142134690 +0x2a0; (assumption) the towns): within 2000 of it on x
//! and z, 10 above the ground there.
//!
//! An effect's harm to characters (F023403) is kenshi_game::hazards' `Harm`. Not ported: the
//! effects' lights ("lights") and sounds ("sound"): no EFFECT of the game's data has either;
//! the per-region list of running effect types (F023398's +0x90), and saving
//! (F023422..F023445).
//! (stand-in) The manager's pool of finished systems (F033120) is not kept: a new system is
//! made each time.

use super::dynamic::Dyn;
use super::emitter::EmitterKind;
use super::kenshi::{GroundMap, Wind};
use super::math::{Quat, Rand, normalise};
use super::observer::{Handler as PuHandler, Observer};
use super::particle::PType;
use super::render::ParticleEffect;
use super::affector::{Affector, AffectorKind};
use super::system::{State, System};
use bevy::math::{Vec3, Vec4};
use std::collections::HashMap;
use std::sync::Arc;

/// fcs_enums EffectType.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectType {
    None,
    Camera,
    Point,
    Wandering,
    Global,
    CameraRain,
    CameraAcidRain,
    PointLighting,
    WanderingStorm,
    WanderingGas,
    GlobalPoint,
}

impl EffectType {
    fn from_i32(v: i32) -> EffectType {
        use EffectType::*;
        [None, Camera, Point, Wandering, Global, CameraRain, CameraAcidRain, PointLighting, WanderingStorm, WanderingGas, GlobalPoint]
            .get(v as usize)
            .copied()
            .unwrap_or(None)
    }
}

/// @fn F023262
///
/// An EFFECT record and what the effect reads of its particle system.
#[derive(Debug, Clone)]
pub struct EffectDef {
    pub id: String,
    pub ty: EffectType,
    pub wind_affected: bool,
    pub wind_speed_mult: f32,
    /// min / max wind span rate (both 0 when max <= min).
    pub wind_span: (f32, f32),
    pub wind_direction_emission: bool,
    pub sky_colour_mult: f32,
    pub colour_mult: Vec4,
    pub altitude: (f32, f32),
    pub ground_colour: bool,
    pub wandering_speed: f32,
    pub strength: (f32, f32),
    pub slope: (f32, f32),
    pub ttl: (f32, f32),
    pub particle_fade_out_delay: f32,
    pub fog_fade_in: f32,
    pub fog_fade_out: f32,
    /// Its fog volumes (F023263), filled in by `Effects::new`.
    pub fog: Vec<crate::fog_volume::FogVolumeDef>,
    pub max_view_distance: f32,
    /// "affect type" (WeatherAffecting: 1 dust, 2 acid, 3 burning, 4 gas): what it does to
    /// characters within its radius.
    pub affect: i32,
    pub effect_radius: f32,
    /// The size of its emitters (the boxes' and circles' largest side, scaled; x 1.5).
    pub size: f32,
    pub wind_affectors: Arc<Vec<String>>,
    pub rates: Arc<Vec<(String, Dyn)>>,
    /// The particle system as the effect has it (F033123: its emitters' colours times the
    /// colour multiplier).
    pub template: Arc<System>,
}

/// Case-insensitive "starts with" (F023349).
fn istarts_with(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

impl EffectDef {
    pub fn from_record(r: &kenshi_data::GameData, templates: &HashMap<String, System>) -> Option<EffectDef> {
        let f = |k: &str| r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|v| *v as f32)).unwrap_or(0.0);
        let b = |k: &str| r.bools.get(k).copied().unwrap_or(false);
        let i = |k: &str| r.ints.get(k).copied().unwrap_or(0);
        let ps = r.strings.get("particle system").filter(|s| !s.is_empty())?;
        let src = templates.get(ps)?;
        let argb = i("colour multiplier") as u32;
        let colour_mult = Vec4::new(
            (argb >> 16 & 0xff) as f32 / 255.0,
            (argb >> 8 & 0xff) as f32 / 255.0,
            (argb & 0xff) as f32 / 255.0,
            1.0,
        );
        let mut wind_span = (f("min wind span rate"), f("max wind span rate"));
        if wind_span.1 <= wind_span.0 {
            wind_span = (0.0, 0.0);
        }
        let wind_affected = b("wind affected");
        // the emitters' size
        let mut size = 0.0f32;
        let sc = src.scale;
        for t in &src.techniques {
            for e in &t.emitters {
                match &e.kind {
                    EmitterKind::Box { width, height, depth } => {
                        size = size.max(width * sc.x);
                        size = size.max(height * sc.y);
                        size = size.max(depth * sc.z) * 0.5;
                    }
                    EmitterKind::Circle { radius, .. } => size = size.max(radius * sc.x),
                    _ => {}
                }
            }
        }
        size *= 1.5;
        let mut wind_affectors = Vec::new();
        let mut rates = Vec::new();
        for t in &src.techniques {
            if wind_affected {
                for a in &t.affectors {
                    if matches!(a.kind, AffectorKind::LinearForce { .. }) && istarts_with(&a.name, "Wind") {
                        wind_affectors.push(a.name.clone());
                    }
                }
            }
            for e in &t.emitters {
                if e.emits_type == PType::Visual && matches!(e.dyn_emission_rate, Dyn::Fixed(_) | Dyn::Random { .. }) {
                    rates.push((e.name.clone(), e.dyn_emission_rate.clone()));
                }
            }
        }
        // F033123: the template's emitter colours times the colour multiplier, saturated
        let mut template = src.clone();
        let sat = |c: Vec4| (c * colour_mult).clamp(Vec4::ZERO, Vec4::ONE);
        for t in &mut template.techniques {
            for e in &mut t.emitters {
                match &mut e.colour_range {
                    None => e.colour = sat(e.colour),
                    Some((a, z)) => {
                        *a = sat(*a);
                        *z = sat(*z);
                    }
                }
            }
        }
        Some(EffectDef {
            id: r.string_id.clone(),
            ty: EffectType::from_i32(i("type")),
            wind_affected,
            wind_speed_mult: f("wind speed mult"),
            wind_span,
            wind_direction_emission: b("wind direction emission"),
            sky_colour_mult: f("sky colour multiplier"),
            colour_mult,
            altitude: (f("min altitude"), f("max altitude")),
            ground_colour: b("ground colour"),
            wandering_speed: f("wandering speed"),
            strength: (f("effect strength min"), f("effect strength max")),
            slope: (f("min slope"), f("max slope")),
            ttl: (f("min time to live"), f("max time to live")),
            particle_fade_out_delay: f("particle fade out delay"),
            fog_fade_in: f("fog fade in duration"),
            fog_fade_out: f("fog fade out duration"),
            fog: Vec::new(),
            max_view_distance: f("maximum view distance"),
            affect: i("affect type") as i32,
            effect_radius: f("effect radius"),
            size,
            wind_affectors: Arc::new(wind_affectors),
            rates: Arc::new(rates),
            template: Arc::new(template),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffState {
    Running,
    Stopping,
    Stopped,
}

/// A running effect: its particle system (with the wrapper's additions) and the Effect's
/// state (F023225..).
pub struct EffectInstance {
    pub fx: ParticleEffect,
    state: EffState,
    time: f32,
}

impl EffectInstance {
    /// @fn F033127
    /// @fn F033084
    ///
    /// The system made from the effect's template and prepared; then the wrapper's additions to
    /// each of its techniques: the wind handler (on emitted techniques), the ground colour
    /// handler (on emitted visual particles) and the light affector.
    fn new(def: &EffectDef, seed: u32, ground: &Option<Arc<GroundMap>>) -> EffectInstance {
        let mut prof = crate::prof::Marks::start();
        let mut s = System::instance(&def.template, seed);
        prof.mark("fx new: instance");
        s.prepare();
        prof.mark("fx new: prepare");
        s.update(0.0);
        prof.mark("fx new: first update");
        for t in &mut s.techniques {
            if def.wind_affected && t.emitters.iter().any(|e| e.emits_type == PType::Technique) {
                t.observers.push(Observer::kenshi_on_emission(PuHandler::KenshiWind));
            }
            if def.ground_colour && ground.is_some() && t.emitters.iter().any(|e| e.emits_type == PType::Visual) {
                t.observers.push(Observer::kenshi_on_emission(PuHandler::KenshiGroundColour));
            }
            if def.sky_colour_mult > 0.0 {
                let has_colour = t.affectors.iter().any(|a| matches!(a.kind, AffectorKind::Colour { .. }));
                t.affectors.push(Affector::kenshi_light(def.sky_colour_mult, has_colour));
            }
        }
        s.kenshi.ground = ground.clone();
        EffectInstance { fx: ParticleEffect::new(s), state: EffState::Stopped, time: 0.0 }
    }

    pub fn system(&mut self) -> &mut System {
        &mut self.fx.system
    }

    /// Started and not yet told to stop.
    pub fn is_running(&self) -> bool {
        self.state == EffState::Running
    }

    /// @fn F023267
    /// @fn F033071
    fn start(&mut self) {
        if self.state != EffState::Running {
            self.time = 0.0;
            self.state = EffState::Running;
            let s = self.system();
            if matches!(s.state, State::Started | State::Paused) {
                s.stop();
            }
            s.start();
        }
    }

    /// @fn F023252
    /// @fn F023251
    fn stop(&mut self, def: &EffectDef) {
        if self.state != EffState::Stopping {
            self.time = 0.0;
            self.state = EffState::Stopping;
            if def.particle_fade_out_delay <= 0.0 {
                self.system().stop_fade();
            } else {
                self.system().stop_fade_after(def.particle_fade_out_delay);
            }
        }
    }

    /// @fn F023253
    ///
    /// The frame: the system updated; once it stops after being stopped, the effect has.
    fn update(&mut self, dt: f32) {
        if self.state == EffState::Stopped {
            return;
        }
        self.time += dt;
        self.system().update(dt);
        if self.state == EffState::Stopping && self.fx.system.state == State::Stopped {
            self.state = EffState::Stopped;
        }
    }

    /// @fn F023236
    ///
    /// How far in (running: over the fog fade in) or out (stopping: over the fog fade out).
    /// The fog volumes' alpha: rising over the fog fade in once started (F023247), falling over
    /// the fog fade out once stopped (F023243; at once with none).
    fn fog_alpha(&self, def: &EffectDef) -> f32 {
        match self.state {
            EffState::Running if def.fog_fade_in > 0.0 => (self.time / def.fog_fade_in).min(1.0),
            EffState::Running => 1.0,
            EffState::Stopping if def.fog_fade_out > 0.0 => (1.0 - self.time / def.fog_fade_out).max(0.0),
            _ => 0.0,
        }
    }

    fn fade(&self, def: &EffectDef) -> f32 {
        match self.state {
            EffState::Running if def.fog_fade_in >= self.time => self.time / def.fog_fade_in,
            EffState::Running => 1.0,
            EffState::Stopping if def.fog_fade_out == 0.0 => 1.0,
            EffState::Stopping => self.time / def.fog_fade_out,
            EffState::Stopped => 0.0,
        }
    }

    /// @fn F033082
    ///
    /// The weather's wind on the system: the force, direction and span factor; then on each
    /// technique and each emitted technique (F033081).
    fn wind(&mut self, def: &EffectDef, speed: f32, dir: Vec3) {
        if !def.wind_affected {
            return;
        }
        let force = dir * speed * def.wind_speed_mult;
        let mut factor = 1.0;
        if def.wind_span.1 > 0.0 {
            factor = ((speed - def.wind_span.0) / (def.wind_span.1 - def.wind_span.0)).clamp(0.0, 1.0);
        }
        let w = Wind {
            force,
            direction: dir,
            factor,
            direction_emission: def.wind_direction_emission,
            span_max: def.wind_span.1,
            affectors: def.wind_affectors.clone(),
            rates: def.rates.clone(),
        };
        let s = self.system();
        for t in &mut s.techniques {
            if t.pool.technique.released_len == 0 {
                super::kenshi::apply_wind(t, &w);
            } else {
                let mut at = t.pool.technique.first();
                while let Some(k) = at {
                    super::kenshi::apply_wind(&mut t.pooled_techniques[k as usize], &w);
                    at = t.pool.technique.next_el();
                }
            }
        }
        s.kenshi.wind = Some(w);
    }
}

/// What the effects read of the world each frame. Positions are world positions.
pub struct WorldView<'a> {
    /// The camera's position and forward direction.
    pub camera: Vec3,
    pub forward: Vec3,
    /// The camera's target (the point it orbits).
    pub target: Vec3,
    /// The weather region the camera is in (the weather manager's current region).
    pub current_region: Option<usize>,
    /// The region of a zone.
    pub region_of_zone: &'a dyn Fn((i32, i32)) -> Option<usize>,
    /// A zone is loaded (Zone +0xb0).
    pub zone_loaded: &'a dyn Fn((i32, i32)) -> bool,
    /// The terrain's height and normal at (x, z).
    pub ground: &'a mut dyn FnMut(f32, f32) -> (f32, Vec3),
    /// The zones of each region.
    pub region_zones: &'a [Vec<(i32, i32)>],
    /// Each region's wind: speed and direction (x, 0, z).
    pub wind: &'a [(f32, Vec3)],
    /// The main light's colour.
    pub sun_colour: Vec3,
    /// World minus scene positions (the scene's origin).
    pub origin: Vec3,
    /// The towns' positions (world units): where lightning is aimed.
    pub towns: &'a [Vec3],
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Camera,
    Global,
    Point { lightning: bool },
    Wandering { dir: Vec3, target: Vec3, timer: f32, ground_timer: f32 },
    GlobalPoint { dist_sq: f32, near_sq: f32 },
}

/// An EffectHandler (F023421).
struct EffHandler {
    effect: usize,
    region: usize,
    kind: Kind,
    /// +0x20: its zone is loaded.
    zone_flag: bool,
    position: Vec3,
    instance: Option<EffectInstance>,
    /// GLOBAL: the system stopped when the camera left the region, kept till it stops.
    old: Option<EffectInstance>,
    /// +0x38 / +0x39.
    active: bool,
    visible: bool,
    strength: f32,
    effect_radius: f32,
    age: f32,
    ttl: f32,
    infinite: bool,
    region_timer: f32,
}

/// @fn F050502
///
/// The game's randRange(a, b): a + (b - a) x rand() / 32768 (not Ogre's / RAND_MAX).
fn rand_range(rng: &mut Rand, a: f32, b: f32) -> f32 {
    let u = (rng.rand() as f64 * (1.0 / 32768.0)) as f32;
    u * (b - a) + a
}

/// @fn F050500
///
/// The game's unit random: rand() / 32768.
fn rand_unit(rng: &mut Rand) -> f32 {
    (rng.rand() as f64 * (1.0 / 32768.0)) as f32
}

/// @fn F050548
///
/// The game's randInt(a, b): a less rand() x -1/32768 times b - a + 1 (truncated), held to at
/// most b and at least a.
fn rand_int(rng: &mut Rand, a: i32, b: i32) -> i32 {
    let u = (rng.rand() as f64 * (-1.0 / 32768.0)) as f32;
    let v = a - (u * (b - a + 1) as f32) as i32;
    let v = if b >= v { v } else { b };
    if v >= a { v } else { a }
}

impl EffHandler {
    fn new(defs: &[EffectDef], effect: usize, region: usize, position: Vec3, kind: Kind, rng: &mut Rand, w: &mut WorldView) -> EffHandler {
        let d = &defs[effect];
        let mut h = EffHandler {
            effect,
            region,
            kind,
            zone_flag: false,
            position,
            instance: None,
            old: None,
            active: false,
            visible: false,
            strength: rand_range(rng, d.strength.0, d.strength.1),
            effect_radius: d.effect_radius,
            age: 0.0,
            ttl: rand_range(rng, d.ttl.0, d.ttl.1),
            infinite: d.ttl.1 <= 0.0,
            region_timer: 0.0,
        };
        // the constructor runs the base check, whatever the handler becomes
        h.base_region_check(d, 0.0, w);
        h
    }

    /// The handler's region check (vtable): a global point's own (F023390), else F023369.
    fn region_check(&mut self, d: &EffectDef, dt: f32, w: &mut WorldView) {
        if let Kind::GlobalPoint { dist_sq, near_sq } = self.kind {
            self.global_point_check(dist_sq, near_sq, dt);
        } else {
            self.base_region_check(d, dt, w);
        }
    }

    /// @fn F023390
    ///
    /// A global point's region check: until it runs, every 0.2 s, it starts once within the
    /// inner ring (its +0x6c: the group's ring squared, F023434). No region or zone test, and
    /// nothing here stops it: the group does beyond the kill distance (F023435), or its time to
    /// live runs out.
    fn global_point_check(&mut self, dist_sq: f32, near_sq: f32, dt: f32) {
        if self.active {
            return;
        }
        self.region_timer -= dt;
        if self.region_timer <= 0.0 {
            if dist_sq <= near_sq {
                self.active = true;
                self.visible = true;
            }
            self.region_timer = 0.2;
        }
    }

    /// @fn F023369
    ///
    /// Every second (two while it runs): whether its zone is in its region and loaded.
    fn base_region_check(&mut self, d: &EffectDef, dt: f32, w: &mut WorldView) {
        self.region_timer -= dt;
        if self.region_timer > 0.0 {
            return;
        }
        let zone = kenshi_world::zone_of(self.position.x, self.position.z);
        if (w.region_of_zone)(zone) == Some(self.region) {
            let loaded = (w.zone_loaded)(zone);
            self.visible = false;
            self.zone_flag = loaded;
            if !loaded {
                // F048106 reads the same zone's flag
                if d.max_view_distance > 0.0 {
                    self.visible = loaded;
                }
            } else {
                self.visible = true;
            }
            self.region_timer = if self.visible { 2.0 } else { 1.0 };
            self.active = self.visible;
        } else {
            self.kill(d);
        }
    }

    /// @fn F023367
    fn kill(&mut self, d: &EffectDef) {
        self.ttl = 0.0;
        self.infinite = false;
        if let Some(i) = &mut self.instance
            && i.state == EffState::Running
        {
            i.stop(d);
        }
    }

    /// @fn F023365
    fn finished(&self) -> bool {
        if self.infinite || self.ttl > 0.0 {
            return false;
        }
        !(self.active && self.instance.as_ref().is_some_and(|i| i.state != EffState::Stopped))
    }

    /// The group's frame (handler +0x10): F023366 (age, life, region), or the camera's and
    /// global's region test (F023373, F023382), or F023391's distance.
    fn tick(&mut self, d: &EffectDef, dt: f32, w: &mut WorldView) {
        match self.kind {
            Kind::Camera | Kind::Global => {
                self.active = w.current_region == Some(self.region);
            }
            _ => {
                self.age += dt;
                if self.infinite {
                    self.region_check(d, dt, w);
                } else {
                    self.ttl -= dt;
                    if self.ttl > 0.0 {
                        self.region_check(d, dt, w);
                    } else if self.active
                        && let Some(i) = &mut self.instance
                    {
                        match i.state {
                            EffState::Running => i.stop(d),
                            EffState::Stopped => self.visible = false,
                            EffState::Stopping => {}
                        }
                    }
                }
                if let Kind::GlobalPoint { dist_sq, .. } = &mut self.kind {
                    let dx = self.position.x - w.target.x;
                    let dz = self.position.z - w.target.z;
                    *dist_sq = dx * dx + dz * dz;
                }
            }
        }
    }

    /// The manager's frame (handler +0x08).
    fn run(&mut self, defs: &[EffectDef], dt: f32, rng: &mut Rand, w: &mut WorldView, ground: &Option<Arc<GroundMap>>, wrap: &mut f32) {
        let d = &defs[self.effect];
        match self.kind {
            Kind::Camera => self.run_camera(d, rng, w, ground, wrap),
            Kind::Global => self.run_global(d, rng, w, ground),
            Kind::Wandering { .. } => self.run_wandering(d, dt, rng, w, ground),
            _ => self.run_point(d, rng, w, ground),
        }
    }

    fn new_instance(&mut self, d: &EffectDef, rng: &mut Rand, ground: &Option<Arc<GroundMap>>) -> &mut EffectInstance {
        let seed = (rng.unit() * 32767.0) as u32;
        let mut i = EffectInstance::new(d, seed, ground);
        i.fx.system.position = self.position;
        i.start();
        self.instance.insert(i)
    }

    /// @fn F023411
    ///
    /// A point effect: started where it is while visible; stopped when not, and dropped once
    /// faded and empty.
    fn run_point(&mut self, d: &EffectDef, rng: &mut Rand, w: &mut WorldView, ground: &Option<Arc<GroundMap>>) {
        if !self.visible {
            if let Some(i) = &mut self.instance {
                i.stop(d);
                if i.fade(d) >= 1.0 && i.fx.system.num_emitted() == 0 {
                    self.instance = None;
                }
            }
        } else if self.instance.is_none() {
            if self.position.y == -99.0 {
                self.position.y = (w.ground)(self.position.x, self.position.z).0;
            }
            self.new_instance(d, rng, ground);
        }
    }

    /// @fn F023412
    ///
    /// A wandering effect: every 0.1 x ttl seconds a new heading within 90 degrees of the last;
    /// it turns toward it, moves at "wandering speed" and with the wind, on the ground.
    fn run_wandering(&mut self, d: &EffectDef, dt: f32, rng: &mut Rand, w: &mut WorldView, ground: &Option<Arc<GroundMap>>) {
        self.run_point(d, rng, w, ground);
        if !(self.infinite || self.ttl > 0.0) {
            return;
        }
        let Kind::Wandering { mut dir, mut target, mut timer, mut ground_timer } = self.kind else { return };
        timer -= dt;
        if timer <= 0.0 {
            let a = rand_range(rng, -0.5, 0.5) * std::f32::consts::PI;
            target = normalise(Quat::from_angle_axis(a, Vec3::Y).rotate(dir));
            timer = rand_range(rng, d.ttl.0, d.ttl.1) * 0.1;
        }
        dir += (target - dir) * dt;
        let wind = if d.wind_affected { w.wind.get(self.region).map_or(Vec3::ZERO, |(s, v)| *v * *s) } else { Vec3::ZERO };
        self.position += dir * dt * d.wandering_speed;
        self.position += wind * dt * d.wind_speed_mult;
        ground_timer -= dt;
        if self.active && ground_timer <= 0.0 {
            self.position.y = (w.ground)(self.position.x, self.position.z).0;
        }
        self.kind = Kind::Wandering { dir, target, timer, ground_timer };
        if let Some(i) = &mut self.instance {
            i.fx.system.position = self.position;
        }
    }

    /// @fn F023396
    ///
    /// The camera effect: while the camera's region is its, the system 1.25 x its size from the
    /// camera toward the target, no higher than "max altitude"; its techniques wrap their
    /// particles into a box round the camera. Otherwise it is dropped (F023377).
    fn run_camera(&mut self, d: &EffectDef, rng: &mut Rand, w: &mut WorldView, ground: &Option<Arc<GroundMap>>, wrap: &mut f32) {
        if !self.active {
            self.instance = None;
            return;
        }
        self.position = w.camera;
        if self.instance.is_none() {
            *wrap = d.size;
            let i = self.new_instance(d, rng, ground);
            for t in &mut i.fx.system.techniques {
                t.camera_wrap = true;
            }
        }
        let toward = normalise(w.target - self.position);
        self.position += toward * *wrap * 1.25;
        if d.altitude.1 != 0.0 {
            self.position.y = self.position.y.min(d.altitude.1);
        }
        if let Some(i) = &mut self.instance {
            i.fx.system.position = self.position;
        }
    }

    /// @fn F023401
    ///
    /// The global effect: at the camera's target, 100 above the ground and within the
    /// altitudes; its particles kept round it (F023399). When the camera leaves the region it
    /// stops and is dropped once stopped.
    fn run_global(&mut self, d: &EffectDef, rng: &mut Rand, w: &mut WorldView, ground: &Option<Arc<GroundMap>>) {
        if !self.active {
            if let Some(mut i) = self.instance.take() {
                // F023385
                self.ttl = 0.0;
                self.infinite = false;
                if i.state == EffState::Running {
                    i.stop(d);
                }
                self.old = Some(i);
            }
        } else {
            self.position = w.target;
            if self.instance.is_none() {
                let i = self.new_instance(d, rng, ground);
                for t in &mut i.fx.system.techniques {
                    t.global_wrap = true;
                }
            }
            self.position.y = (w.ground)(self.position.x, self.position.z).0 + 100.0;
            if d.altitude.0 != 0.0 || d.altitude.1 != 0.0 {
                self.position.y = self.position.y.min(d.altitude.1).max(d.altitude.0);
            }
            if let Some(i) = &mut self.instance {
                i.fx.system.position = self.position;
            }
        }
        if self.old.as_ref().is_some_and(|o| o.state == EffState::Stopped) {
            self.old = None;
        }
    }

    /// The instances it holds.
    fn instances(&mut self) -> impl Iterator<Item = &mut EffectInstance> {
        self.instance.iter_mut().chain(self.old.iter_mut())
    }
}

/// An EffectGroup (F023413): a weather's effect in a region.
struct Group {
    effect: usize,
    region: usize,
    count: i32,
    respawn: (f32, f32),
    timer: f32,
    handlers: Vec<EffHandler>,
    /// GLOBAL_POINT: the rings' radius (F023418: "maximum view distance", or 1000).
    ring: f32,
}

impl Group {
    /// @fn F023419
    fn new(defs: &[EffectDef], effect: usize, region: usize, count: i32, min: f32, max: f32) -> Group {
        let d = &defs[effect];
        // (testing) KENSHI_EFFECTS_NOW: the first ones 5 s in (once the zones round the camera are in)
        let first = if std::env::var("KENSHI_EFFECTS_NOW").is_ok() { 5.0 } else { min };
        let mut g = Group { effect, region, count, respawn: (min, max), timer: first, handlers: Vec::new(), ring: 0.0 };
        match d.ty {
            EffectType::Camera | EffectType::CameraRain | EffectType::CameraAcidRain => g.count = 1,
            EffectType::GlobalPoint => g.ring = if d.max_view_distance <= 0.0 { 1000.0 } else { d.max_view_distance },
            _ => {}
        }
        g
    }

    /// The group's spawn (vtable +0x20): one handler; false when it can't.
    fn spawn(&mut self, defs: &[EffectDef], rng: &mut Rand, w: &mut WorldView) -> bool {
        let d = &defs[self.effect];
        let (pos, kind) = match d.ty {
            // F023428
            EffectType::Camera | EffectType::CameraRain | EffectType::CameraAcidRain => (Vec3::ZERO, Kind::Camera),
            // F023430
            EffectType::Global => (Vec3::ZERO, Kind::Global),
            // F023424
            EffectType::Point | EffectType::PointLighting => {
                let mut p = random_point(self.region, d, rng, w);
                // the lightning at the nearest town, somewhere within 2000 of it on the ground
                if d.ty == EffectType::PointLighting
                    && std::env::var("KENSHI_EFFECTS_NEAR").is_err()
                    && let Some(t) = w.towns.iter().min_by(|a, b| a.distance_squared(p).total_cmp(&b.distance_squared(p)))
                {
                    p = *t;
                    p.x += rand_range(rng, -2000.0, 2000.0);
                    p.z += rand_range(rng, -2000.0, 2000.0);
                    p.y = (w.ground)(p.x, p.z).0 + 10.0;
                }
                (p, Kind::Point { lightning: d.ty == EffectType::PointLighting })
            }
            // F023426, F023425
            EffectType::Wandering | EffectType::WanderingStorm | EffectType::WanderingGas => {
                let p = random_point(self.region, d, rng, w);
                let x = rand_range(rng, -1.0, 1.0);
                let z = rand_range(rng, -1.0, 1.0);
                let dir = Vec3::new(x, 0.0, z);
                let timer = rand_range(rng, d.ttl.0, d.ttl.1) * 0.1;
                (p, Kind::Wandering { dir, target: dir, timer, ground_timer: 0.0 })
            }
            _ => return false,
        };
        let h = EffHandler::new(defs, self.effect, self.region, pos, kind, rng, w);
        if std::env::var("KENSHI_FOG_DEBUG").is_ok() && !d.fog.is_empty() {
            eprintln!("effect {} spawned at {pos:.0}: active {}, ttl {:.0}, finished {}", d.id, h.active, h.ttl, h.finished());
        }
        self.handlers.push(h);
        true
    }

    /// @fn F023405
    /// @fn F023398
    /// @fn F023402
    /// @fn F023435
    fn update(&mut self, defs: &[EffectDef], dt: f32, rng: &mut Rand, w: &mut WorldView) {
        let d = &defs[self.effect];
        match d.ty {
            EffectType::Camera | EffectType::CameraRain | EffectType::CameraAcidRain | EffectType::Global => {
                while (self.handlers.len() as i32) < self.count {
                    if !self.spawn(defs, rng, w) {
                        break;
                    }
                }
                for h in &mut self.handlers {
                    h.tick(d, dt, w);
                }
            }
            EffectType::GlobalPoint => self.update_global_point(defs, dt, rng, w),
            EffectType::None => {}
            _ => {
                self.timer -= dt;
                while self.timer <= 0.0
                    && (self.count == 0 || (self.handlers.len() as i32) < self.count)
                    && self.spawn(defs, rng, w)
                {
                    self.timer += rand_range(rng, self.respawn.0, self.respawn.1);
                }
                for h in &mut self.handlers {
                    h.tick(d, dt, w);
                }
                self.handlers.retain(|h| !(h.finished() && h.instance.is_none()));
            }
        }
    }

    /// @fn F023435
    ///
    /// Global points: those beyond twice the rings are let go; then up to 3 x count, new ones
    /// in the inner ring (up to count there) and the outer.
    fn update_global_point(&mut self, defs: &[EffectDef], dt: f32, rng: &mut Rand, w: &mut WorldView) {
        let d = &defs[self.effect];
        let (r, r2) = (self.ring, self.ring * 2.0);
        let far = r2 * r2 * 4.0;
        for h in &mut self.handlers {
            h.tick(d, dt, w);
            if let Kind::GlobalPoint { dist_sq, .. } = h.kind
                && far <= dist_sq
            {
                h.kill(d);
            }
        }
        self.handlers.retain(|h| !(h.finished() && h.instance.is_none()));
        let (mut near, mut mid) = (0, 0);
        for h in &self.handlers {
            if let Kind::GlobalPoint { dist_sq, .. } = h.kind {
                if dist_sq <= r * r {
                    near += 1;
                } else if dist_sq <= r2 * r2 {
                    mid += 1;
                }
            }
        }
        let n = self.handlers.len() as i32;
        let free = self.count * 3 - n;
        if free > 0 {
            let a = free.min(self.count - near).max(0);
            let b = (free - a).min(self.count - mid).max(0);
            for _ in 0..a {
                self.spawn_ring(defs, rng, w, 0.0, r);
            }
            for _ in 0..b {
                self.spawn_ring(defs, rng, w, r, r2);
            }
        }
    }

    /// @fn F023434
    ///
    /// A global point in the ring round the camera's target: ten tries for a place within the
    /// altitudes and slopes and in the region.
    fn spawn_ring(&mut self, defs: &[EffectDef], rng: &mut Rand, w: &mut WorldView, rmin: f32, rmax: f32) {
        let d = &defs[self.effect];
        let mut pos = Vec3::ZERO;
        for _ in 0..10 {
            let a = rand_range(rng, 0.0, std::f32::consts::TAU);
            let dist = rand_unit(rng).sqrt() * (rmax - rmin) + rmin;
            pos = w.target;
            pos.x += a.cos() * dist;
            pos.z += a.sin() * dist;
            let (h, n) = (w.ground)(pos.x, pos.z);
            pos.y = h;
            let alt_ok = d.altitude.1 == 0.0 || (d.altitude.0 <= h && h <= d.altitude.1);
            if alt_ok
                && d.slope.0 <= n.y
                && n.y <= d.slope.1
                && (w.region_of_zone)(kenshi_world::zone_of(pos.x, pos.z)) == Some(self.region)
            {
                break;
            }
        }
        let mut h = EffHandler::new(defs, self.effect, self.region, pos, Kind::GlobalPoint { dist_sq: f32::MAX, near_sq: self.ring * self.ring }, rng, w);
        h.active = false;
        h.visible = false;
        h.region_timer = 0.0;
        self.handlers.push(h);
    }
}

/// @fn F048159
///
/// A random point of a region: a random zone of it, a random place in its middle 80%, on the
/// ground; ten tries for the altitudes (when either is set) and slopes (the ground normal's y).
fn random_point(region: usize, d: &EffectDef, rng: &mut Rand, w: &mut WorldView) -> Vec3 {
    // (testing) KENSHI_EFFECTS_NEAR=<distance>: that far ahead of the camera's target
    if let Some(d) = std::env::var("KENSHI_EFFECTS_NEAR").ok().and_then(|v| v.parse::<f32>().ok()) {
        let ahead = Vec3::new(w.forward.x, 0.0, w.forward.z).normalize_or(Vec3::Z);
        let mut p = w.target + ahead * d;
        p.y = (w.ground)(p.x, p.z).0;
        return p;
    }
    let zones = &w.region_zones[region];
    let mut p = Vec3::ZERO;
    if zones.is_empty() {
        return p;
    }
    for _ in 0..10 {
        let z = zones[rand_int(rng, 0, zones.len() as i32 - 1) as usize];
        let x0 = z.0 as f32 * kenshi_world::ZONE_SIZE - kenshi_world::WORLD_SIZE / 2.0;
        let z0 = z.1 as f32 * kenshi_world::ZONE_SIZE - kenshi_world::WORLD_SIZE / 2.0;
        let m = kenshi_world::ZONE_SIZE * 0.2 * 0.5;
        p.x = rand_range(rng, x0 + m, x0 + kenshi_world::ZONE_SIZE - m);
        p.z = rand_range(rng, z0 + m, z0 + kenshi_world::ZONE_SIZE - m);
        let (h, n) = (w.ground)(p.x, p.z);
        p.y = h;
        let alt_bad = (d.altitude.0 != 0.0 || d.altitude.1 != 0.0) && (h < d.altitude.0 || d.altitude.1 < h);
        if !(alt_bad || n.y < d.slope.0 || d.slope.1 < n.y) {
            break;
        }
    }
    p
}

/// The effects of the weathers: the EFFECT records, each region's groups for its current
/// weather, the ground colour map.
#[derive(bevy::prelude::Resource)]
pub struct Effects {
    pub defs: Vec<EffectDef>,
    by_id: HashMap<String, usize>,
    groups: Vec<Vec<Group>>,
    /// The weather each region's groups were made for.
    made_for: Vec<Option<(usize, i32)>>,
    rng: Rand,
    ground: Option<Arc<GroundMap>>,
    /// The camera wrap's size (0x1421282c4: the last camera effect's).
    wrap: f32,
    /// The camera target's last position (F023387).
    last_target: Option<Vec3>,
    /// Draw entities of dropped systems, for the renderer to remove.
    pub dead_draws: Vec<bevy::prelude::Entity>,
    /// The point effects started once (F033135's pool, of 100; each let go when its system
    /// stops or after 10 s, F033136).
    oneshots: Vec<EffectInstance>,
    /// Effects on buildings (F037795), running while their zone is loaded.
    attached: Vec<EffectInstance>,
    /// Each attached effect's building (zone, placement) and its EFFECT (index).
    attached_of: Vec<(Option<((i32, i32), usize)>, usize)>,
    /// Per BIOMES record: its "ground effects" by terrain layer (effect, min speed,
    /// probability) and its "water effects" (effect, min depth, min speed) (F051626).
    ground_effects: HashMap<String, [GroundSlot; 5]>,
    water_effects: HashMap<String, Vec<(Option<usize>, i32, i32)>>,
}

/// A terrain layer's ground effect: effect, min speed, probability.
type GroundSlot = Option<(usize, f32, f32)>;

/// F033135's pool size and F033136's longest life.
const ONESHOTS: usize = 100;
const ONESHOT_LIFE: f32 = 10.0;

impl Effects {
    /// @fn F033129
    /// @fn F033130
    ///
    /// Every EFFECT record whose particle system exists.
    pub fn new(data: &kenshi_data::GameDataContainer, templates: &HashMap<String, System>, install: &std::path::Path) -> Effects {
        let mut defs = Vec::new();
        let mut by_id = HashMap::new();
        for r in data.of_type(kenshi_data::ItemType::Effect) {
            if let Some(mut d) = EffectDef::from_record(r, templates) {
                d.fog = r.refs("fog volumes").iter().filter_map(|(s, _)| data.get(s)).map(crate::fog_volume::FogVolumeDef::from_record).collect();
                by_id.insert(d.id.clone(), defs.len());
                defs.push(d);
            }
        }
        // F051626: the biomes' ground effects by layer (a layer of 5 or more is past Kenshi's
        // five slots and not kept), and their water effects
        let mut ground_effects = HashMap::new();
        let mut water_effects = HashMap::new();
        for r in data.of_type(kenshi_data::ItemType::Biomes) {
            let mut slots: [GroundSlot; 5] = [None; 5];
            for (id, v) in r.refs("ground effects") {
                if let (Some(&e), Some(slot)) = (by_id.get(id), slots.get_mut(v[0].max(0) as usize)) {
                    *slot = Some((e, v[1] as f32, v[2] as f32 * 0.01));
                }
            }
            if slots.iter().any(|s| s.is_some()) {
                ground_effects.insert(r.string_id.clone(), slots);
            }
            let water: Vec<_> = r.refs("water effects").iter().map(|(id, v)| (by_id.get(id).copied(), v[0], v[1])).collect();
            if !water.is_empty() {
                water_effects.insert(r.string_id.clone(), water);
            }
        }
        Effects {
            defs,
            by_id,
            groups: Vec::new(),
            made_for: Vec::new(),
            rng: Rand::new(1),
            ground: GroundMap::load(install).map(Arc::new),
            wrap: 0.0,
            last_target: None,
            dead_draws: Vec::new(),
            oneshots: Vec::new(),
            attached: Vec::new(),
            attached_of: Vec::new(),
            ground_effects,
            water_effects,
        }
    }

    /// @fn F033135
    ///
    /// A point effect started at `at` (world position), if the pool has room.
    pub fn spawn_point(&mut self, effect: usize, at: Vec3) -> bool {
        if self.oneshots.len() >= ONESHOTS {
            return false;
        }
        let seed = (self.rng.unit() * 32767.0) as u32;
        let mut i = EffectInstance::new(&self.defs[effect], seed, &self.ground);
        i.fx.system.position = at;
        i.start();
        self.oneshots.push(i);
        true
    }

    /// @fn F020844
    ///
    /// AnimationGroundEvent (a footstep): in water (the foot below the water level) the biome's
    /// water effect for the depth and speed with the highest min depth (then min speed), at the
    /// surface; on land the biome's ground effect for the terrain layer under the foot, when
    /// fast enough and on a roll under its probability, on the ground. Started only where the
    /// camera sees it (F051547: F041470, within 100).
    #[allow(clippy::too_many_arguments)]
    pub fn ground_event(
        &mut self,
        foot: Vec3,
        speed: f32,
        biome: Option<&str>,
        layer: Option<usize>,
        water_level: f32,
        ground_y: f32,
        visible: impl Fn(Vec3) -> bool,
    ) -> Option<usize> {
        let mut at = foot;
        let depth = water_level - foot.y;
        let effect = if depth >= 0.0 {
            let list = biome.and_then(|b| self.water_effects.get(b))?;
            let (mut best, mut bd, mut bs) = (None, -1, -1);
            for &(e, d, s) in list {
                if d as f32 <= depth && s as f32 <= speed && (bd < d || (d == bd && bs < s)) {
                    best = e;
                    bd = d;
                    bs = s;
                }
            }
            at.y = water_level;
            best?
        } else {
            let slot = biome
                .and_then(|b| self.ground_effects.get(b))
                .zip(layer)
                .and_then(|(t, l)| t.get(l).copied().flatten())?;
            let (e, min_speed, probability) = slot;
            if probability <= 0.0 || speed < min_speed || probability < rand_unit(&mut self.rng) {
                return None;
            }
            at.y = ground_y;
            e
        };
        (visible(at) && self.spawn_point(effect, at)).then_some(effect)
    }

    /// @fn F037795
    ///
    /// An effect on a building at world position `at`, turned `rot`.
    /// Kenshi attaches it unstarted; a building's effects run while it is complete, switched
    /// on (a UseableStuff's setOn, F029284; the caller leaves out those of one stored off) and
    /// in view (F037702: the building's +0x19d, each effect started or stopped by it).
    pub fn attach(&mut self, effect: usize, at: Vec3, rot: bevy::math::Quat) {
        self.attach_to(effect, at, rot, None);
    }

    /// `attach`, the effect following building `building`'s view (`follow_views`).
    pub fn attach_to(&mut self, effect: usize, at: Vec3, rot: bevy::math::Quat, building: Option<((i32, i32), usize)>) {
        let seed = (self.rng.unit() * 32767.0) as u32;
        let mut i = EffectInstance::new(&self.defs[effect], seed, &self.ground);
        i.fx.system.position = at;
        i.fx.system.orientation = Quat { w: rot.w, x: rot.x, y: rot.y, z: rot.z };
        i.start();
        self.attached.push(i);
        self.attached_of.push((building, effect));
    }

    /// @fn F037702
    ///
    /// The buildings' effects following their buildings' view: one whose building came into
    /// view started, one whose building left it stopped (its particles fade out).
    pub fn follow_views(&mut self, in_view: &dyn Fn(((i32, i32), usize)) -> bool) {
        for (i, (key, def)) in self.attached.iter_mut().zip(&self.attached_of) {
            let Some(key) = *key else { continue };
            let want = in_view(key);
            if want && !i.is_running() {
                i.start();
            } else if !want && i.is_running() {
                i.stop(&self.defs[*def]);
            }
        }
    }

    /// The index of an EFFECT record.
    pub fn effect_index(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    /// @fn F051022
    ///
    /// A region's weather began: its groups let go and one made for each of the weather's
    /// effects (count, min and max respawn seconds; a count below 1 is 0: no limit).
    pub fn weather_began(&mut self, region: usize, weather: (usize, i32), effects: &[(String, [i32; 3])]) {
        if self.groups.len() <= region {
            self.groups.resize_with(region + 1, Vec::new);
            self.made_for.resize(region + 1, None);
        }
        if self.made_for[region] == Some(weather) {
            return;
        }
        self.made_for[region] = Some(weather);
        for g in std::mem::take(&mut self.groups[region]) {
            for mut h in g.handlers {
                for i in h.instances() {
                    self.dead_draws.extend(i.fx.take_draws());
                }
            }
        }
        for (id, v) in effects {
            if let Some(&e) = self.by_id.get(id) {
                let count = if v[0] < 1 { 0 } else { v[0] };
                self.groups[region].push(Group::new(&self.defs, e, region, count, v[1] as f32, v[2] as f32));
            }
        }
    }

    /// The frame: the camera wrap's box (F023372), the groups (F023394), the handlers
    /// (F051186's list), then each system's wind and update (F033124 -> F023257).
    pub fn update(&mut self, dt: f32, w: &mut WorldView) {
        let defs = &self.defs;
        let rng = &mut self.rng;
        // the camera target's movement (F023387)
        let moved = match self.last_target {
            Some(l) => normalise(w.target - l),
            None => Vec3::ZERO,
        };
        self.last_target = Some(w.target);
        let mut prof = crate::prof::Marks::start();
        let before = Self::draw_entities(&mut self.groups);
        for gs in &mut self.groups {
            for g in gs.iter_mut() {
                g.update(defs, dt, rng, w);
            }
        }
        prof.mark("fx: groups");
        // the camera wrap: centre ahead of the camera (assumption: F023372's
        // orientation x (0, 0, size) points down the view)
        let wrap_centre = w.camera + w.forward * self.wrap;
        for gs in &mut self.groups {
            for g in gs.iter_mut() {
                for h in &mut g.handlers {
                    prof.mark("fx: instances");
                    h.run(defs, dt, rng, w, &self.ground, &mut self.wrap);
                    prof.mark("fx: handlers");
                    let d = &defs[h.effect];
                    let (speed, dir) = w.wind.get(h.region).copied().unwrap_or((0.0, Vec3::ZERO));
                    let size = d.size;
                    let global = matches!(h.kind, Kind::Global).then_some((h.position, size, d.altitude.1));
                    for i in h.instances() {
                        i.wind(d, speed, dir);
                        let s = i.system();
                        s.kenshi.sun_colour = w.sun_colour;
                        s.kenshi.origin = w.origin;
                        s.kenshi.wrap = Some((wrap_centre - w.origin, self.wrap));
                        s.kenshi.global = global.map(|(p, sz, alt)| (p - w.origin, sz, alt, moved));
                        // the system's node in scene space
                        let world = s.position;
                        s.position = world - w.origin;
                        i.update(dt);
                        let s = i.system();
                        s.position = world;
                    }
                }
            }
        }
        prof.mark("fx: instances");
        // the buildings' effects
        for i in &mut self.attached {
            let s = i.system();
            s.kenshi.sun_colour = w.sun_colour;
            s.kenshi.origin = w.origin;
            let world = s.position;
            s.position = world - w.origin;
            s.update(dt);
            s.position = world;
        }
        prof.mark("fx: buildings");
        // the point effects (F033136)
        let mut k = 0;
        while k < self.oneshots.len() {
            let i = &mut self.oneshots[k];
            let s = i.system();
            s.kenshi.sun_colour = w.sun_colour;
            s.kenshi.origin = w.origin;
            let world = s.position;
            s.position = world - w.origin;
            s.update(dt);
            s.position = world;
            if s.state == State::Stopped || s.time_since_start >= ONESHOT_LIFE {
                let mut i = self.oneshots.swap_remove(k);
                self.dead_draws.extend(i.fx.take_draws());
            } else {
                k += 1;
            }
        }
        prof.mark("fx: point effects");
        let after = Self::draw_entities(&mut self.groups);
        self.dead_draws.extend(before.into_iter().filter(|e| !after.contains(e)));
        prof.mark("fx: draw lists");
    }

    fn draw_entities(groups: &mut [Vec<Group>]) -> std::collections::HashSet<bevy::prelude::Entity> {
        let mut out = std::collections::HashSet::new();
        for gs in groups {
            for g in gs.iter_mut() {
                for h in &mut g.handlers {
                    for i in h.instances() {
                        out.extend(i.fx.draw_entities());
                    }
                }
            }
        }
        out
    }

    /// The running systems, their nodes in scene space for drawing (call after `update`).
    pub fn for_each(&mut self, origin: Vec3, mut f: impl FnMut(&mut ParticleEffect)) {
        for gs in &mut self.groups {
            for g in gs.iter_mut() {
                for h in &mut g.handlers {
                    for i in h.instances() {
                        let world = i.fx.system.position;
                        i.fx.system.position = world - origin;
                        f(&mut i.fx);
                        i.fx.system.position = world;
                    }
                }
            }
        }
        for i in self.oneshots.iter_mut().chain(self.attached.iter_mut()) {
            let world = i.fx.system.position;
            i.fx.system.position = world - origin;
            f(&mut i.fx);
            i.fx.system.position = world;
        }
    }

    /// The running effects' fog volumes, each at its system (scene space) with its alpha.
    pub fn fog_volumes(&mut self, origin: Vec3) -> Vec<crate::fog_volume::FogVolumeDraw> {
        let defs = &self.defs;
        let mut out = Vec::new();
        let mut add = |i: &EffectInstance, effect: usize| {
            let d = &defs[effect];
            if d.fog.is_empty() {
                return;
            }
            let fade = i.fog_alpha(d);
            if fade <= 0.0 {
                return;
            }
            let at = i.fx.system.position - origin;
            out.extend(d.fog.iter().map(|v| crate::fog_volume::FogVolumeDraw { def: v.clone(), at, fade }));
        };
        for gs in &mut self.groups {
            for g in gs.iter_mut() {
                for h in &mut g.handlers {
                    let effect = h.effect;
                    for i in h.instances() {
                        add(i, effect);
                    }
                }
            }
        }
        out
    }

    /// The running weather effects that hurt (their EFFECT's "affect type"), in loaded zones:
    /// (affect type, world position, reach, strength). Reach: its "effect radius", else (0:
    /// the game's own guess) its emitters' size.
    pub fn harmful(&self) -> Vec<(i32, Vec3, f32, f32)> {
        let mut out = Vec::new();
        for gs in &self.groups {
            for g in gs {
                let d = &self.defs[g.effect];
                if d.affect == 0 {
                    continue;
                }
                for h in &g.handlers {
                    if h.zone_flag && h.instance.as_ref().is_some_and(|i| i.is_running()) {
                        let r = if h.effect_radius > 0.0 { h.effect_radius } else { d.size.max(100.0) };
                        out.push((d.affect, h.position, r, h.strength));
                    }
                }
            }
        }
        out
    }

    /// (testing) Per region with groups: (region, effect id, handlers, positions, active).
    pub fn debug_groups(&self) -> Vec<(usize, String, usize, Vec<(Vec3, bool)>)> {
        let mut out = Vec::new();
        for (k, gs) in self.groups.iter().enumerate() {
            for g in gs {
                out.push((k, self.defs[g.effect].id.clone(), g.handlers.len(), g.handlers.iter().map(|h| (h.position, h.active)).collect()));
            }
        }
        out
    }

    /// How many systems are running (for the log).
    pub fn running(&mut self) -> usize {
        let mut n = 0;
        for gs in &mut self.groups {
            for g in gs.iter_mut() {
                for h in &mut g.handlers {
                    n += h.instances().count();
                }
            }
        }
        n
    }
}
