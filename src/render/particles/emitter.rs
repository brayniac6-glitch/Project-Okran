//! ParticleUniverse's emitters (ParticleUniverseEmitter and the Box, Circle, Line, Point, Slave
//! and SphereSurface emitters): how many particles a frame asks for (emission rate, duration,
//! repeat delay, forced emission) and each new particle's position, direction, speed,
//! orientation, mass, colour, time to live and size.

use super::dynamic::Dyn;
use super::math::{Quat, Rand, normalise, perpendicular, random_deviant, rotation_to};
use super::particle::{PEF_EXPIRED, PRef, PType, Particle};
use super::script::{Item, Node};
use bevy::math::{Vec3, Vec4};

#[derive(Debug, Clone)]
pub enum EmitterKind {
    Point,
    Box { width: f32, height: f32, depth: f32 },
    Circle {
        radius: f32,
        circle_angle: f32,
        original_circle_angle: f32,
        step: f32,
        random: bool,
        orientation: Quat,
        normal: Vec3,
        x: f32,
        z: f32,
    },
    SphereSurface { radius: f32, random_vector: Vec3 },
    Line(Box<Line>),
    Slave(Box<Slave>),
}

#[derive(Debug, Clone, Default)]
pub struct Line {
    pub end: Vec3,
    pub perpendicular: Vec3,
    pub max_deviation: f32,
    pub min_increment: f32,
    pub max_increment: f32,
    pub increment: f32,
    pub increments_left: bool,
    pub first: bool,
    pub scaled_end: Vec3,
    pub scaled_max_deviation: f32,
    pub scaled_min_increment: f32,
    pub scaled_max_increment: f32,
    pub scaled_length: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Slave {
    pub master_technique: String,
    pub master_emitter: String,
    pub master_emitter_set: bool,
    pub master_position: Vec3,
    pub master_direction: Vec3,
    pub master_particle: Option<(usize, PRef)>,
}

/// What an emitter reads of its technique and system.
pub struct EmitCtx {
    /// The system's time since start.
    pub time: f32,
    pub sys_orientation: Quat,
    /// The technique's derived position.
    pub tech_position: Vec3,
    /// The technique's default particle width, height, depth.
    pub default_dims: Vec3,
    /// The technique's isStopFade.
    pub stop_fade: bool,
}

#[derive(Debug, Clone)]
pub struct Emitter {
    /// The emitter as a particle (position: its offset, or where it is when emitted).
    pub p: Particle,
    pub kind: EmitterKind,
    pub name: String,
    pub emits_type: PType,
    pub emits_name: String,
    remainder: f32,
    duration_remain: f32,
    repeat_delay_remain: f32,
    pub dyn_duration: Option<Dyn>,
    pub dyn_repeat_delay: Option<Dyn>,
    pub particle_direction: Vec3,
    pub original_particle_direction: Vec3,
    pub up_vector: Vec3,
    pub orientation: Quat,
    pub orientation_range: Option<(Quat, Quat)>,
    pub dyn_all_dimensions: Option<Dyn>,
    pub dyn_width: Option<Dyn>,
    pub dyn_height: Option<Dyn>,
    pub dyn_depth: Option<Dyn>,
    pub auto_direction: bool,
    pub force_emission: bool,
    original_force_emission: bool,
    force_emission_executed: bool,
    original_force_emission_executed: bool,
    pub scale: Vec3,
    pub colour: Vec4,
    pub colour_range: Option<(Vec4, Vec4)>,
    pub keep_local: bool,
    pub texture_coords: u16,
    pub texture_coords_range: Option<(u16, u16)>,
    pub dyn_emission_rate: Dyn,
    pub dyn_time_to_live: Dyn,
    pub dyn_mass: Dyn,
    pub dyn_velocity: Dyn,
    pub dyn_angle: Dyn,
}

impl Emitter {
    pub fn new(kind: EmitterKind) -> Emitter {
        let mut p = Particle::new(PType::Emitter);
        p.marked_for_emission = false;
        let mut e = Emitter {
            p,
            kind,
            name: String::new(),
            emits_type: PType::Visual,
            emits_name: String::new(),
            remainder: 0.0,
            duration_remain: 0.0,
            repeat_delay_remain: 0.0,
            dyn_duration: None,
            dyn_repeat_delay: None,
            particle_direction: Vec3::Y,
            original_particle_direction: Vec3::Y,
            up_vector: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            orientation_range: None,
            dyn_all_dimensions: None,
            dyn_width: None,
            dyn_height: None,
            dyn_depth: None,
            auto_direction: false,
            force_emission: false,
            original_force_emission: false,
            force_emission_executed: false,
            original_force_emission_executed: false,
            scale: Vec3::ONE,
            colour: Vec4::ONE,
            colour_range: None,
            keep_local: false,
            texture_coords: 0,
            texture_coords_range: None,
            dyn_emission_rate: Dyn::Fixed(10.0),
            dyn_time_to_live: Dyn::Fixed(3.0),
            dyn_mass: Dyn::Fixed(1.0),
            dyn_velocity: Dyn::Fixed(100.0),
            dyn_angle: Dyn::Fixed(20.0),
        };
        if let EmitterKind::Line(l) = &mut e.kind {
            l.increments_left = true;
            l.first = true;
        }
        e
    }

    /// A cheap stand-in kept in a slot while the emitter is worked on.
    pub fn placeholder() -> Emitter {
        Emitter::new(EmitterKind::Point)
    }

    /// An `emitter <type> [name] { ... }` object (EmitterTranslator: the base properties first,
    /// then the type's).
    pub fn from_node(n: &Node) -> Option<Emitter> {
        let kind = match n.name.as_str() {
            "Point" => EmitterKind::Point,
            "Box" => EmitterKind::Box { width: 100.0, height: 100.0, depth: 100.0 },
            "Circle" => EmitterKind::Circle {
                radius: 100.0,
                circle_angle: 0.0,
                original_circle_angle: 0.0,
                step: 0.1,
                random: true,
                orientation: Quat::IDENTITY,
                normal: Vec3::ZERO,
                x: 0.0,
                z: 0.0,
            },
            "SphereSurface" => EmitterKind::SphereSurface { radius: 10.0, random_vector: Vec3::ZERO },
            "Line" => EmitterKind::Line(Box::default()),
            "Slave" => EmitterKind::Slave(Box::default()),
            _ => return None,
        };
        let mut e = Emitter::new(kind);
        e.name = n.values.first().cloned().unwrap_or_default();
        let mut colour_start = Vec4::new(0.0, 0.0, 0.0, 1.0);
        let mut colour_end = Vec4::ONE;
        let mut orient_start = Quat::IDENTITY;
        let mut orient_end = Quat::IDENTITY;
        for item in &n.order {
            match *item {
                Item::Prop(i) => {
                    let p = &n.props[i];
                    let fixed = || p.f(0).map(Dyn::Fixed);
                    let quat = || {
                        Some(Quat::new(p.f(0)?, p.f(1)?, p.f(2)?, p.f(3)?))
                    };
                    let colour = || {
                        Some(Vec4::new(p.f(0)?, p.f(1)?, p.f(2)?, p.f(3).unwrap_or(1.0)))
                    };
                    match p.name.as_str() {
                        "enabled" => {
                            if let Some(v) = p.boolean() {
                                e.p.set_enabled(v);
                            }
                        }
                        "position" => {
                            if let Some(v) = p.vec3() {
                                e.p.position = v;
                                e.p.original_position = v;
                            }
                        }
                        "direction" => {
                            if let Some(v) = p.vec3() {
                                e.set_particle_direction(v);
                            }
                        }
                        "orientation" => {
                            if let Some(q) = quat() {
                                e.orientation = q;
                            }
                        }
                        "start_orientation_range" | "range_start_orientation" => {
                            if let Some(q) = quat() {
                                orient_start = q;
                                e.orientation_range = Some((orient_start, orient_end));
                            }
                        }
                        "end_orientation_range" | "range_end_orientation" => {
                            if let Some(q) = quat() {
                                orient_end = q;
                                e.orientation_range = Some((orient_start, orient_end));
                            }
                        }
                        "emission_rate" => e.dyn_emission_rate = fixed().unwrap_or(e.dyn_emission_rate.clone()),
                        "time_to_live" => e.dyn_time_to_live = fixed().unwrap_or(e.dyn_time_to_live.clone()),
                        "mass" => e.dyn_mass = fixed().unwrap_or(e.dyn_mass.clone()),
                        "velocity" => e.dyn_velocity = fixed().unwrap_or(e.dyn_velocity.clone()),
                        "angle" => e.dyn_angle = fixed().unwrap_or(e.dyn_angle.clone()),
                        "duration" => {
                            if let Some(d) = fixed() {
                                e.dyn_duration = Some(d);
                            }
                        }
                        "repeat_delay" => {
                            if let Some(d) = fixed() {
                                e.dyn_repeat_delay = Some(d);
                            }
                        }
                        "all_particle_dimensions" => e.dyn_all_dimensions = fixed().or(e.dyn_all_dimensions.take()),
                        "particle_width" => e.dyn_width = fixed().or(e.dyn_width.take()),
                        "particle_height" => e.dyn_height = fixed().or(e.dyn_height.take()),
                        "particle_depth" => e.dyn_depth = fixed().or(e.dyn_depth.take()),
                        "start_colour_range" => {
                            if let Some(c) = colour() {
                                colour_start = c;
                                e.colour_range = Some((colour_start, colour_end));
                            }
                        }
                        "end_colour_range" => {
                            if let Some(c) = colour() {
                                colour_end = c;
                                e.colour_range = Some((colour_start, colour_end));
                            }
                        }
                        "colour" => {
                            if let Some(c) = colour() {
                                e.colour = c;
                            }
                        }
                        "texture_coords" => e.texture_coords = p.f(0).unwrap_or(0.0) as u16,
                        "emits" => {
                            if p.values.len() == 2 {
                                e.emits_type = match p.values[0].as_str() {
                                    "emitter_particle" => PType::Emitter,
                                    "technique_particle" => PType::Technique,
                                    "affector_particle" => PType::Affector,
                                    "system_particle" => PType::System,
                                    _ => PType::Visual,
                                };
                                e.emits_name = p.values[1].clone();
                            }
                        }
                        "auto_direction" => e.auto_direction = p.boolean().unwrap_or(false),
                        "keep_local" => e.keep_local = p.boolean().unwrap_or(false),
                        "force_emission" => {
                            let v = p.boolean().unwrap_or(false);
                            e.force_emission = v;
                            e.original_force_emission = v;
                            e.force_emission_executed = false;
                            e.original_force_emission_executed = false;
                        }
                        name => e.type_property(name, p),
                    }
                }
                Item::Child(i) => {
                    let c = &n.children[i];
                    let d = Dyn::from_node(c);
                    match c.class.as_str() {
                        "emission_rate" => e.dyn_emission_rate = d,
                        "time_to_live" => e.dyn_time_to_live = d,
                        "mass" => e.dyn_mass = d,
                        "velocity" => e.dyn_velocity = d,
                        "duration" => e.dyn_duration = Some(d),
                        "repeat_delay" => e.dyn_repeat_delay = Some(d),
                        "angle" => e.dyn_angle = d,
                        "all_particle_dimensions" => e.dyn_all_dimensions = Some(d),
                        "particle_width" => e.dyn_width = Some(d),
                        "particle_height" => e.dyn_height = Some(d),
                        "particle_depth" => e.dyn_depth = Some(d),
                        _ => {}
                    }
                }
            }
        }
        Some(e)
    }

    /// The emitter type's own properties (their translators).
    fn type_property(&mut self, name: &str, p: &super::script::Prop) {
        match &mut self.kind {
            EmitterKind::Box { width, height, depth } => match name {
                "box_width" | "box_em_width" => *width = p.f(0).unwrap_or(*width),
                "box_height" | "box_em_height" => *height = p.f(0).unwrap_or(*height),
                "box_depth" | "box_em_depth" => *depth = p.f(0).unwrap_or(*depth),
                _ => {}
            },
            EmitterKind::Circle { radius, circle_angle, original_circle_angle, step, random, orientation, normal, .. } => {
                match name {
                    "radius" | "circle_em_radius" => *radius = p.f(0).unwrap_or(*radius),
                    "step" | "circle_em_step" => *step = p.f(0).unwrap_or(*step),
                    "circle_em_angle" => {
                        if let Some(v) = p.f(0) {
                            *circle_angle = v;
                            *original_circle_angle = v;
                        }
                    }
                    "emit_random" | "circle_em_random" => *random = p.boolean().unwrap_or(*random),
                    "normal" | "circle_em_normal" => {
                        if let Some(v) = p.vec3() {
                            *orientation = rotation_to(Vec3::Y, v, Vec3::X);
                            *normal = v;
                        }
                    }
                    _ => {}
                }
            }
            EmitterKind::SphereSurface { radius, .. } => {
                if name == "radius" || name == "sphere_surface_em_radius" {
                    *radius = p.f(0).unwrap_or(*radius);
                }
            }
            EmitterKind::Line(l) => match name {
                "end" | "line_em_end" => {
                    if let Some(v) = p.vec3() {
                        l.end = v;
                    }
                }
                "max_increment" | "line_em_max_increment" => l.max_increment = p.f(0).unwrap_or(l.max_increment),
                "min_increment" | "line_em_min_increment" => l.min_increment = p.f(0).unwrap_or(l.min_increment),
                "max_deviation" | "line_em_max_deviation" => l.max_deviation = p.f(0).unwrap_or(l.max_deviation),
                _ => {}
            },
            EmitterKind::Slave(s) => match name {
                "master_technique_name" => s.master_technique = p.string().unwrap_or_default().to_string(),
                "master_emitter_name" => {
                    s.master_emitter = p.string().unwrap_or_default().to_string();
                    s.master_emitter_set = true;
                }
                _ => {}
            },
            EmitterKind::Point => {}
        }
    }

    /// setParticleDirection.
    pub fn set_particle_direction(&mut self, d: Vec3) {
        self.original_particle_direction = d;
        self.particle_direction = normalise(d);
        self.up_vector = normalise(perpendicular(self.particle_direction));
    }

    /// _notifyRescaled (the Line emitter rescales its line).
    pub fn rescale(&mut self, scale: Vec3) {
        self.scale = scale;
        if let EmitterKind::Line(l) = &mut self.kind {
            let s = scale.length();
            l.scaled_end = l.end * scale;
            l.scaled_max_deviation = l.max_deviation * s;
            l.scaled_min_increment = l.min_increment * s;
            l.scaled_max_increment = (l.max_increment - l.min_increment) * s;
            l.scaled_length = l.scaled_end.length();
        }
    }

    /// getDerivedPosition.
    pub fn derived_position(&mut self, c: &EmitCtx) -> Vec3 {
        self.p.derived_position = if self.p.marked_for_emission {
            self.p.position
        } else {
            c.tech_position + c.sys_orientation.rotate(self.scale * self.p.position)
        };
        self.p.derived_position
    }

    /// setEnabled: also restarts the duration or repeat delay (_initTimeBased).
    pub fn set_enabled(&mut self, enabled: bool, c: &EmitCtx, rng: &mut Rand) {
        self.p.set_enabled(enabled);
        self.init_time_based(c, rng);
    }

    fn init_time_based(&mut self, c: &EmitCtx, rng: &mut Rand) {
        if self.p.enabled {
            if let Some(d) = &self.dyn_duration {
                self.duration_remain = d.get(c.time, rng);
            }
            if self.duration_remain > 0.0 {
                self.p.enabled = true;
                self.repeat_delay_remain = 0.0;
            }
        } else if let Some(d) = &self.dyn_repeat_delay {
            self.repeat_delay_remain = d.get(c.time, rng);
        }
    }

    /// _notifyStart.
    pub fn notify_start(&mut self, c: &EmitCtx, rng: &mut Rand) {
        self.p.latest_position = self.derived_position(c);
        self.force_emission = self.original_force_emission;
        self.force_emission_executed = self.original_force_emission_executed;
        self.remainder = 0.0;
        self.duration_remain = 0.0;
        self.repeat_delay_remain = 0.0;
        let e = self.p.original_enabled;
        self.set_enabled(e, c, rng);
        match &mut self.kind {
            EmitterKind::Circle { circle_angle, original_circle_angle, .. } => {
                *circle_angle = *original_circle_angle;
            }
            EmitterKind::Line(l) => {
                l.increment = 0.0;
                l.increments_left = true;
                l.first = true;
            }
            EmitterKind::Slave(_) => self.set_enabled(false, c, rng),
            _ => {}
        }
    }

    /// _calculateRequestedParticles; the bool asks the technique to lock all its particles (the
    /// Line emitter restarting its increments).
    pub fn requested(&mut self, dt: f32, c: &EmitCtx, rng: &mut Rand) -> (u32, bool) {
        let mut requested: u32 = 0;
        if self.p.enabled {
            let rate = self.dyn_emission_rate.get(c.time, rng);
            if self.force_emission {
                if self.force_emission_executed {
                    requested = 0;
                } else {
                    requested = rate as u16 as u32;
                    self.force_emission_executed = true;
                }
            } else {
                self.remainder += rate * dt;
                requested = self.remainder as u16 as u32;
            }
            self.remainder -= requested as f32;
            if self.dyn_duration.is_some() {
                self.duration_remain -= dt;
                if self.duration_remain <= 0.0 {
                    self.set_enabled(false, c, rng);
                }
            }
        } else if self.dyn_repeat_delay.is_some() {
            self.repeat_delay_remain -= dt;
            if self.repeat_delay_remain <= 0.0 && !c.stop_fade {
                self.set_enabled(true, c, rng);
            }
        }
        let mut lock_all = false;
        if let EmitterKind::Line(l) = &self.kind
            && l.scaled_max_increment > 0.0
        {
            if !l.increments_left {
                requested = 0;
            }
            if !self.p.enabled {
                self.notify_start(c, rng);
                lock_all = true;
            }
        }
        (requested, lock_all)
    }

    /// _initParticleForEmission (the caller sets the particle's emitter).
    pub fn init_particle(&mut self, p: &mut Particle, c: &EmitCtx, rng: &mut Rand) {
        self.init_position(p, c, rng);
        self.init_direction(p, c, rng);
        // velocity
        let scalar = self.dyn_velocity.get(c.time, rng);
        p.direction *= scalar;
        p.original_velocity = scalar;
        p.original_scaled_direction_length = p.direction.length();
        // orientation
        if p.ptype == PType::Visual {
            p.orientation = match self.orientation_range {
                Some((a, b)) => Quat::nlerp(rng.unit(), a, b, true),
                None => self.orientation,
            };
            p.original_orientation = p.orientation;
        }
        // mass
        p.mass = self.dyn_mass.get(c.time, rng);
        // colour
        if p.ptype == PType::Visual {
            p.colour = match self.colour_range {
                Some((a, b)) => Vec4::new(
                    rng.range(a.x, b.x),
                    rng.range(a.y, b.y),
                    rng.range(a.z, b.z),
                    rng.range(a.w, b.w),
                ),
                None => self.colour,
            };
            p.original_colour = p.colour;
            p.texture_coords_current = match self.texture_coords_range {
                Some((a, b)) => rng.range(a as f32, b as f32 + 0.999) as u16,
                None => self.texture_coords,
            };
        }
        p.total_time_to_live = self.dyn_time_to_live.get(c.time, rng);
        p.time_to_live = p.total_time_to_live;
        // dimensions
        if p.ptype == PType::Visual {
            if self.dyn_all_dimensions.is_some()
                || self.dyn_width.is_some()
                || self.dyn_height.is_some()
                || self.dyn_depth.is_some()
            {
                if let Some(d) = &self.dyn_all_dimensions {
                    let extend = d.get(c.time, rng);
                    p.set_own_dimensions(self.scale.x * extend, self.scale.y * extend, self.scale.z * extend);
                    return;
                }
                let w = self.dyn_width.as_ref().map_or(0.0, |d| d.get(c.time, rng));
                let h = self.dyn_height.as_ref().map_or(0.0, |d| d.get(c.time, rng));
                let dp = self.dyn_depth.as_ref().map_or(0.0, |d| d.get(c.time, rng));
                p.set_own_dimensions(self.scale.x * w, self.scale.y * h, self.scale.z * dp);
            } else {
                p.width = self.scale.x * c.default_dims.x;
                p.height = self.scale.y * c.default_dims.y;
                p.depth = self.scale.z * c.default_dims.z;
                p.calculate_bounding_sphere_radius();
            }
        }
    }

    fn init_position(&mut self, p: &mut Particle, c: &EmitCtx, rng: &mut Rand) {
        let base = self.derived_position(c);
        let scale = self.scale;
        match &mut self.kind {
            EmitterKind::Point => {
                p.position = base;
                p.original_position = base;
                p.latest_position = base;
            }
            EmitterKind::Box { width, height, depth } => {
                let v = Vec3::new(
                    rng.symmetric() * 0.5 * *width,
                    rng.symmetric() * 0.5 * *height,
                    rng.symmetric() * 0.5 * *depth,
                );
                p.position = base + c.sys_orientation.rotate(scale * v);
                p.original_position = p.position;
            }
            EmitterKind::Circle { radius, circle_angle, step, random, orientation, x, z, .. } => {
                let angle = if *random {
                    rng.range(0.0, std::f32::consts::TAU)
                } else {
                    *circle_angle += *step;
                    if *circle_angle > std::f32::consts::TAU {
                        *circle_angle -= std::f32::consts::TAU;
                    }
                    *circle_angle
                };
                *x = angle.cos();
                *z = angle.sin();
                let v = orientation.rotate(scale * Vec3::new(*x * *radius, 0.0, *z * *radius));
                p.position = base + c.sys_orientation.rotate(v);
                p.original_position = p.position;
            }
            EmitterKind::SphereSurface { radius, random_vector } => {
                *random_vector = normalise(Vec3::new(
                    rng.range(-1.0, 1.0),
                    rng.range(-1.0, 1.0),
                    rng.range(-1.0, 1.0),
                ));
                p.position = base + c.sys_orientation.rotate(scale * *random_vector * *radius);
                p.original_position = p.position;
            }
            EmitterKind::Line(l) => {
                if self.auto_direction || (l.scaled_max_deviation > 0.0 && !l.first) {
                    l.perpendicular = normalise(l.end.cross(Vec3::new(
                        rng.range(-1.0, 1.0),
                        rng.range(-1.0, 1.0),
                        rng.range(-1.0, 1.0),
                    )));
                }
                let mut fraction = 0.0;
                if l.scaled_max_increment > 0.0 {
                    if !l.first {
                        l.increment += l.scaled_min_increment + rng.unit() * l.scaled_max_increment;
                        if l.increment >= l.scaled_length {
                            l.increments_left = false;
                        }
                        fraction = l.increment / l.scaled_length;
                    }
                } else {
                    fraction = rng.unit();
                }
                if l.scaled_max_deviation > 0.0 && l.increments_left {
                    if !l.first {
                        let b = base + fraction * l.scaled_end;
                        p.position = b + l.scaled_max_deviation * rng.unit() * l.perpendicular;
                        p.original_position = b;
                    } else {
                        p.position = base;
                        p.original_position = base;
                    }
                } else {
                    p.position = base + fraction * l.scaled_end;
                    p.original_position = p.position;
                }
                l.first = false;
            }
            EmitterKind::Slave(s) => {
                p.position = s.master_position;
                p.original_position = p.position;
            }
        }
    }

    /// _generateAngle: degrees; a fixed angle is a random angle up to it.
    fn generate_angle(&self, c: &EmitCtx, rng: &mut Rand) -> f32 {
        let a = self.dyn_angle.get(c.time, rng).to_radians();
        if self.dyn_angle.is_fixed() { rng.unit() * a } else { a }
    }

    fn base_direction(&self, p: &mut Particle, c: &EmitCtx, rng: &mut Rand) {
        let angle = self.generate_angle(c, rng);
        p.direction = if angle != 0.0 {
            random_deviant(self.particle_direction, angle, self.up_vector, rng)
        } else {
            self.particle_direction
        };
        p.original_direction = p.direction;
        p.original_direction_length = p.direction.length();
    }

    fn init_direction(&self, p: &mut Particle, c: &EmitCtx, rng: &mut Rand) {
        match &self.kind {
            EmitterKind::Circle { orientation, x, z, .. } if self.auto_direction => {
                let angle = self.generate_angle(c, rng);
                let d = orientation.rotate(Vec3::new(*x, 0.0, *z));
                if angle != 0.0 {
                    p.direction = random_deviant(d, angle, self.up_vector, rng);
                    p.original_direction = p.direction;
                } else {
                    p.direction = d;
                }
            }
            EmitterKind::SphereSurface { random_vector, .. } if self.auto_direction => {
                let angle = self.generate_angle(c, rng);
                p.direction = if angle != 0.0 {
                    random_deviant(*random_vector, angle, self.up_vector, rng)
                } else {
                    *random_vector
                };
                p.original_direction = p.direction;
            }
            EmitterKind::Line(l) if self.auto_direction => {
                let angle = self.generate_angle(c, rng);
                p.direction = if angle != 0.0 {
                    random_deviant(l.perpendicular, angle, self.up_vector, rng)
                } else {
                    l.perpendicular
                };
                p.original_direction = p.direction;
            }
            EmitterKind::Slave(s) => {
                p.direction = s.master_direction;
                p.original_direction = p.direction;
                p.original_direction_length = p.direction.length();
                p.slave_master = s.master_particle;
            }
            _ => self.base_direction(p, c, rng),
        }
    }

    /// makeParticleLocal (keep_local): the particle follows the emitter's move since the last
    /// frame.
    pub fn make_particle_local(&self, p: &mut Particle) -> bool {
        if !self.keep_local || self.p.has(PEF_EXPIRED) {
            return false;
        }
        let derived = if self.p.marked_for_emission { self.p.position } else { self.p.derived_position };
        p.position += derived - self.p.latest_position;
        true
    }
}
