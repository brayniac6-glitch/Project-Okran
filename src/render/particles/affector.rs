//! ParticleUniverse's affectors as Kenshi's scripts use them (Align, Colour, FlockCentering,
//! ForceField with its realtime Perlin noise, GeometryRotator, Gravity, Jet, LinearForce,
//! Randomiser, Scale, ScaleVelocity, SineForce, TextureRotator, Vortex).

use super::dynamic::Dyn;
use super::math::{Quat, Rand};
use super::particle::{PType, Particle};
use super::script::{Item, Node, Prop};
use bevy::math::{Vec3, Vec4};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Specialisation {
    Default,
    TtlIncrease,
    TtlDecrease,
}

#[derive(Debug, Clone)]
pub enum AffectorKind {
    LinearForce { force: Vec3, average: bool, scaled: Vec3 },
    SineForce { force: Vec3, average: bool, scaled: Vec3, angle: f32, freq_min: f32, freq_max: f32, freq: f32 },
    /// time -> colour (std::map: sorted, unique times).
    Colour { map: Vec<(f32, Vec4)>, multiply: bool },
    Scale { x: Option<Dyn>, y: Option<Dyn>, z: Option<Dyn>, xyz: Option<Dyn>, since_start: bool },
    TextureRotator { own_speed: bool, rotation: Dyn, speed: Dyn },
    Vortex { rotation: Quat, vector: Vec3, speed: Dyn },
    Randomiser { deviation: Vec3, time_since: f32, step: f32, update: bool, random_direction: bool },
    Gravity { gravity: f32 },
    FlockCentering { sum: Vec3, average: Vec3, count: u32 },
    ForceField(Box<ForceField>),
    Jet { acceleration: Dyn },
    Align { resize: bool },
    GeometryRotator { speed: Dyn, own_speed: bool, axis: Vec3, axis_set: bool },
    /// @fn F033093
    ///
    /// Kenshi's light affector (type "Colour", added last by F033084 when the effect's "sky
    /// colour multiplier" > 0): visual particles' colour (or original colour, when the
    /// technique has no Colour affector) times White + (sun colour - White) x mult, saturated.
    KenshiLight { mult: f32, has_colour: bool },
    ScaleVelocity { scale: Dyn, since_start: bool, stop_at_flip: bool },
}

/// What an affector reads of its technique and system.
pub struct AffectCtx {
    pub time: f32,
    pub sys_orientation: Quat,
    pub tech_position: Vec3,
    pub scale_velocity: f32,
    pub num_emitted: usize,
    /// The sun's colour (Kenshi's light affector).
    pub sun_colour: Vec3,
}

#[derive(Debug, Clone)]
pub struct Affector {
    /// The affector as a particle (position offset, mass, enabled).
    pub p: Particle,
    pub kind: AffectorKind,
    pub name: String,
    pub excluded: Vec<String>,
    pub specialisation: Specialisation,
    pub scale: Vec3,
}

impl Affector {
    /// Kenshi's light affector (F033084 builds it with the default affector state).
    pub fn kenshi_light(mult: f32, has_colour: bool) -> Affector {
        let mut a = Affector::from_node(&Node {
            class: "affector".into(),
            name: "Colour".into(),
            ..Default::default()
        })
        .expect("Colour affector");
        a.kind = AffectorKind::KenshiLight { mult, has_colour };
        a
    }

    /// setForceVector (BaseForceAffector).
    pub fn set_force_vector(&mut self, v: Vec3) {
        if let AffectorKind::LinearForce { force, .. } | AffectorKind::SineForce { force, .. } = &mut self.kind {
            *force = v;
        }
    }

    pub fn from_node(n: &Node) -> Option<Affector> {
        let kind = match n.name.as_str() {
            "LinearForce" => AffectorKind::LinearForce { force: Vec3::ZERO, average: false, scaled: Vec3::ZERO },
            "SineForce" => AffectorKind::SineForce {
                force: Vec3::ZERO,
                average: false,
                scaled: Vec3::ZERO,
                angle: 361.0,
                freq_min: 1.0,
                freq_max: 1.0,
                freq: 1.0,
            },
            "Colour" => AffectorKind::Colour { map: Vec::new(), multiply: false },
            "Scale" => AffectorKind::Scale { x: None, y: None, z: None, xyz: None, since_start: false },
            "TextureRotator" => AffectorKind::TextureRotator {
                own_speed: false,
                rotation: Dyn::Fixed(0.0),
                speed: Dyn::Fixed(10.0),
            },
            "Vortex" => AffectorKind::Vortex { rotation: Quat::IDENTITY, vector: Vec3::ZERO, speed: Dyn::Fixed(1.0) },
            "Randomiser" => AffectorKind::Randomiser {
                deviation: Vec3::ZERO,
                time_since: 0.0,
                step: 0.0,
                update: true,
                random_direction: true,
            },
            "Gravity" => AffectorKind::Gravity { gravity: 1.0 },
            "FlockCentering" => AffectorKind::FlockCentering { sum: Vec3::ZERO, average: Vec3::ZERO, count: 0 },
            "ForceField" => AffectorKind::ForceField(Box::default()),
            "Jet" => AffectorKind::Jet { acceleration: Dyn::Fixed(1.0) },
            "Align" => AffectorKind::Align { resize: false },
            "GeometryRotator" => AffectorKind::GeometryRotator {
                speed: Dyn::Fixed(10.0),
                own_speed: false,
                axis: Vec3::ZERO,
                axis_set: false,
            },
            "ScaleVelocity" => AffectorKind::ScaleVelocity { scale: Dyn::Fixed(1.0), since_start: false, stop_at_flip: false },
            _ => return None,
        };
        let mut p = Particle::new(PType::Affector);
        p.marked_for_emission = false;
        let mut a = Affector {
            p,
            kind,
            name: n.values.first().cloned().unwrap_or_default(),
            excluded: Vec::new(),
            specialisation: Specialisation::Default,
            scale: Vec3::ONE,
        };
        for item in &n.order {
            match *item {
                Item::Prop(i) => {
                    let p = &n.props[i];
                    match p.name.as_str() {
                        "enabled" => {
                            if let Some(v) = p.boolean() {
                                a.p.set_enabled(v);
                            }
                        }
                        "position" => {
                            if let Some(v) = p.vec3() {
                                a.p.position = v;
                                a.p.original_position = v;
                            }
                        }
                        "mass_affector" => a.p.mass = p.f(0).unwrap_or(a.p.mass),
                        "affect_specialisation" => {
                            a.specialisation = match p.string() {
                                Some("special_ttl_increase") => Specialisation::TtlIncrease,
                                Some("special_ttl_decrease") => Specialisation::TtlDecrease,
                                _ => Specialisation::Default,
                            }
                        }
                        "exclude_emitter" => {
                            for v in &p.values {
                                if !a.excluded.contains(v) {
                                    a.excluded.push(v.clone());
                                }
                            }
                        }
                        name => a.kind.property(name, p),
                    }
                }
                Item::Child(i) => {
                    let c = &n.children[i];
                    a.kind.object(&c.class, Dyn::from_node(c));
                }
            }
        }
        Some(a)
    }

    /// getDerivedPosition.
    pub fn derived_position(&mut self, c: &AffectCtx) -> Vec3 {
        self.p.derived_position = if self.p.marked_for_emission {
            self.p.position
        } else {
            c.tech_position + c.sys_orientation.rotate(self.scale * self.p.position)
        };
        self.p.derived_position
    }

    fn specialisation_factor(&self, p: &Particle) -> f32 {
        match self.specialisation {
            Specialisation::Default => 1.0,
            Specialisation::TtlIncrease => p.time_fraction,
            Specialisation::TtlDecrease => 1.0 - p.time_fraction,
        }
    }

    /// _notifyStart.
    pub fn notify_start(&mut self) {
        let e = self.p.original_enabled;
        self.p.set_enabled(e);
        if let AffectorKind::ForceField(f) = &mut self.kind {
            f.movement_frequency_count = 0.0;
        }
    }

    /// _prepare (the force field is set up round the system).
    pub fn prepare(&mut self, sys_position: Vec3) {
        if let AffectorKind::ForceField(f) = &mut self.kind {
            f.initialise(sys_position);
        }
    }

    /// _initParticleForEmission.
    pub fn init_particle(&self, p: &mut Particle, c: &AffectCtx, rng: &mut Rand) {
        if p.ptype != PType::Visual {
            return;
        }
        match &self.kind {
            AffectorKind::TextureRotator { rotation, speed, .. } => {
                p.z_rotation = rotation.get(c.time, rng);
                p.z_rotation_speed = speed.get(p.time_fraction, rng);
            }
            AffectorKind::GeometryRotator { speed, own_speed, axis_set, .. } => {
                if !axis_set {
                    p.orientation = Quat::new(
                        rng.range(-1.0, 1.0),
                        rng.range(-1.0, 1.0),
                        rng.range(-1.0, 1.0),
                        rng.range(-1.0, 1.0),
                    );
                    // the script order: x, y, z, then w
                    let q = p.orientation;
                    p.orientation = Quat::new(q.z, q.w, q.x, q.y).normalised();
                    p.rotation_axis = super::math::normalise(Vec3::new(rng.unit(), rng.unit(), rng.unit()));
                }
                if *own_speed {
                    p.rotation_speed = speed.get(p.time_fraction, rng);
                }
            }
            _ => {}
        }
    }

    /// _preProcessParticles.
    pub fn pre_process(&mut self, c: &AffectCtx, dt: f32, rng: &mut Rand) {
        let d = self.derived_position(c);
        match &mut self.kind {
            AffectorKind::LinearForce { force, scaled, .. } => *scaled = *force * dt,
            AffectorKind::SineForce { force, scaled, angle, freq_min, freq_max, freq, .. } => {
                *angle += *freq * dt;
                let s = angle.sin();
                *scaled = *force * dt * s;
                if *angle > std::f32::consts::TAU {
                    *angle = 0.0;
                    if freq_min != freq_max {
                        *freq = rng.range(*freq_min, *freq_max);
                    }
                }
            }
            AffectorKind::Vortex { rotation, vector, speed } => {
                *rotation = Quat::from_angle_axis(speed.get(c.time, rng) * dt, c.sys_orientation.rotate(*vector));
            }
            AffectorKind::Randomiser { time_since, step, update, .. } => {
                if c.num_emitted > 0 {
                    *time_since += dt;
                    if *time_since > *step {
                        *time_since -= *step;
                        *update = true;
                    }
                }
            }
            AffectorKind::FlockCentering { sum, average, count } => {
                *average = if *count != 0 { *sum / *count as f32 } else { d };
                *sum = Vec3::ZERO;
                *count = 0;
            }
            AffectorKind::ForceField(f) => f.pre_process(dt),
            _ => {}
        }
    }

    /// _postProcessParticles.
    pub fn post_process(&mut self) {
        if let AffectorKind::Randomiser { update, .. } = &mut self.kind {
            *update = false;
        }
    }

    /// _affect; `previous` the Align affector's previous particle's position.
    pub fn affect(&mut self, p: &mut Particle, dt: f32, c: &AffectCtx, previous: Option<Vec3>, rng: &mut Rand) {
        let spec = self.specialisation_factor(p);
        let scale = self.scale;
        let mass = self.p.mass;
        let derived = self.p.derived_position;
        match &mut self.kind {
            AffectorKind::LinearForce { force, average, scaled } => {
                if !*average {
                    p.direction += *scaled * spec;
                } else {
                    p.direction = (p.direction + *force) / 2.0;
                }
            }
            AffectorKind::SineForce { force, average, scaled, .. } => {
                if !*average {
                    p.direction += *scaled;
                } else {
                    p.direction = (p.direction + *force) / 2.0;
                }
            }
            AffectorKind::KenshiLight { mult, has_colour } => {
                if p.ptype == PType::Visual {
                    let l = Vec4::ONE + (c.sun_colour.extend(1.0) - Vec4::ONE) * *mult;
                    let base = if *has_colour { p.colour } else { p.original_colour };
                    p.colour = (base * l).clamp(Vec4::ZERO, Vec4::ONE);
                }
            }
            AffectorKind::Colour { map, multiply } => {
                if map.is_empty() || p.ptype != PType::Visual {
                    return;
                }
                let tf = (p.total_time_to_live - p.time_to_live) / p.total_time_to_live;
                let mut i1 = map.len() - 1;
                for (i, (t, _)) in map.iter().enumerate() {
                    if tf < *t {
                        i1 = i.saturating_sub(1);
                        break;
                    }
                }
                let colour = match map.get(i1 + 1) {
                    Some((t2, c2)) => {
                        let (t1, c1) = map[i1];
                        c1 + (*c2 - c1) * ((tf - t1) / (t2 - t1))
                    }
                    None => map[i1].1,
                };
                p.colour = if *multiply { colour * p.original_colour } else { colour };
            }
            AffectorKind::Scale { x, y, z, xyz, since_start } => {
                if p.ptype != PType::Visual {
                    return;
                }
                let at = if *since_start { c.time } else { p.time_fraction };
                let (mut w, mut h, mut d) = (0.0, 0.0, 0.0);
                if let Some(s) = xyz {
                    let ds = s.get(at, rng) * dt * spec;
                    let dim = p.width + ds * scale.x;
                    if dim > 0.0 {
                        w = dim;
                    }
                    let dim = p.height + ds * scale.y;
                    if dim > 0.0 {
                        h = dim;
                    }
                    let dim = p.depth + ds * scale.z;
                    if dim > 0.0 {
                        d = dim;
                    }
                } else {
                    if let Some(s) = x {
                        let dim = p.width + s.get(at, rng) * dt * scale.x;
                        if dim > 0.0 {
                            w = dim;
                        }
                    }
                    if let Some(s) = y {
                        let dim = p.height + s.get(at, rng) * dt * scale.y;
                        if dim > 0.0 {
                            h = dim;
                        }
                    }
                    if let Some(s) = z {
                        let dim = p.depth + s.get(at, rng) * dt * scale.z;
                        if dim > 0.0 {
                            d = dim;
                        }
                    }
                }
                p.set_own_dimensions(w, h, d);
            }
            AffectorKind::TextureRotator { own_speed, speed, .. } => {
                if p.ptype != PType::Visual {
                    return;
                }
                let s = if *own_speed { p.z_rotation_speed } else { speed.get(p.time_fraction, rng) } * dt;
                p.z_rotation += s;
                if p.z_rotation > std::f32::consts::TAU {
                    p.z_rotation -= std::f32::consts::TAU;
                }
            }
            AffectorKind::Vortex { rotation, .. } => {
                if p.freezed {
                    return;
                }
                let local = p.position - derived;
                p.position = derived + rotation.rotate(local);
                p.direction = rotation.rotate(p.direction);
                if p.ptype == PType::Visual {
                    p.orientation = *rotation * p.orientation;
                }
            }
            AffectorKind::Randomiser { deviation, update, random_direction, .. } => {
                if !*update {
                    return;
                }
                let v = Vec3::new(
                    rng.range(-deviation.x, deviation.x),
                    rng.range(-deviation.y, deviation.y),
                    rng.range(-deviation.z, deviation.z),
                );
                if *random_direction {
                    p.direction += v;
                } else {
                    if p.freezed {
                        return;
                    }
                    p.position += scale * v;
                }
            }
            AffectorKind::Gravity { gravity } => {
                let distance = derived - p.position;
                let length = distance.length_squared();
                if length > 0.0 {
                    let force = (c.scale_velocity * *gravity * p.mass * mass) / length;
                    p.direction += force * distance * dt * spec;
                }
            }
            AffectorKind::FlockCentering { sum, average, count } => {
                *sum += p.position;
                *count += 1;
                p.direction += (*average - p.position) * dt;
            }
            AffectorKind::ForceField(f) => {
                let mut force = f.determine_force(p.position);
                if f.ignore_negative.x {
                    force.x = 0.0;
                }
                if f.ignore_negative.y {
                    force.y = 0.0;
                }
                if f.ignore_negative.z {
                    force.z = 0.0;
                }
                p.direction += dt * f.scale_force * force;
            }
            AffectorKind::Jet { acceleration } => {
                let scaled = dt * acceleration.get(p.time_fraction, rng);
                if p.direction == Vec3::ZERO {
                    p.direction += p.original_direction * scaled;
                } else {
                    p.direction += p.direction * scaled;
                }
            }
            AffectorKind::Align { resize } => {
                if p.ptype == PType::Visual {
                    let mut diff = previous.unwrap_or(p.position) - p.position;
                    if *resize {
                        let (w, d) = (p.width, p.depth);
                        p.set_own_dimensions(w, diff.length(), d);
                    }
                    diff = super::math::normalise(diff);
                    p.orientation.x = diff.x;
                    p.orientation.y = diff.y;
                    p.orientation.z = diff.z;
                }
            }
            AffectorKind::GeometryRotator { speed, own_speed, axis, axis_set } => {
                if p.ptype != PType::Visual {
                    return;
                }
                let s = if *own_speed { p.rotation_speed } else { speed.get(p.time_fraction, rng) } * dt;
                let q = Quat::from_angle_axis(s, if *axis_set { *axis } else { p.rotation_axis });
                p.orientation = q * p.orientation;
            }
            AffectorKind::ScaleVelocity { scale: sv, since_start, stop_at_flip } => {
                let at = if *since_start { c.time } else { p.time_fraction };
                let ds = dt * sv.get(at, rng);
                let length = p.direction.length();
                let calc = p.direction + ds * (p.direction / length);
                if *stop_at_flip {
                    let d = p.direction;
                    let flip = |a: f32, b: f32| (a > 0.0 && b < 0.0) || (a < 0.0 && b > 0.0);
                    if flip(calc.x, d.x) || flip(calc.y, d.y) || flip(calc.z, d.z) {
                        return;
                    }
                }
                p.direction = calc;
            }
        }
    }
}

impl AffectorKind {
    fn property(&mut self, name: &str, p: &Prop) {
        let fixed = || p.f(0).map(Dyn::Fixed);
        match self {
            AffectorKind::LinearForce { force, average, .. } | AffectorKind::SineForce { force, average, .. }
                if matches!(name, "force_vector" | "force_aff_vector" | "force_application" | "force_aff_application") =>
            {
                if name.ends_with("vector") {
                    if let Some(v) = p.vec3() {
                        *force = v;
                    }
                } else {
                    match p.string() {
                        Some("add") => *average = false,
                        Some("average") => *average = true,
                        _ => {}
                    }
                }
            }
            AffectorKind::SineForce { freq_min, freq_max, freq, .. } => match name {
                "min_frequency" | "sinef_aff_frequency_min" => {
                    if let Some(v) = p.f(0) {
                        *freq_min = v;
                        if v > *freq_max {
                            *freq = v;
                        }
                    }
                }
                "max_frequency" | "sinef_aff_frequency_max" => {
                    if let Some(v) = p.f(0) {
                        *freq_max = v;
                        *freq = v;
                    }
                }
                _ => {}
            },
            AffectorKind::Colour { map, multiply } => match name {
                "time_colour" | "colour_aff_time_colour" => {
                    if (4..=5).contains(&p.values.len()) {
                        let v: Vec<f32> = p.values.iter().map(|s| super::script::parse_f32(s).unwrap_or(0.0)).collect();
                        let colour = Vec4::new(v[1], v[2], v[3], v.get(4).copied().unwrap_or(1.0));
                        match map.binary_search_by(|(t, _)| t.partial_cmp(&v[0]).unwrap_or(std::cmp::Ordering::Less)) {
                            Ok(i) => map[i].1 = colour,
                            Err(i) => map.insert(i, (v[0], colour)),
                        }
                    }
                }
                "colour_operation" | "colour_aff_operation" => match p.string() {
                    Some("multiply") => *multiply = true,
                    Some("set") => *multiply = false,
                    _ => {}
                },
                _ => {}
            },
            AffectorKind::Scale { x, y, z, xyz, since_start } => match name {
                "xyz_scale" => *xyz = fixed().or(xyz.take()),
                "x_scale" => *x = fixed().or(x.take()),
                "y_scale" => *y = fixed().or(y.take()),
                "z_scale" => *z = fixed().or(z.take()),
                "since_start_system" => *since_start = p.boolean().unwrap_or(*since_start),
                _ => {}
            },
            AffectorKind::TextureRotator { own_speed, rotation, speed } => match name {
                "use_own_rotation" | "tex_rot_use_own_rotation" => *own_speed = p.boolean().unwrap_or(*own_speed),
                "rotation_speed" | "tex_rot_speed" => *speed = fixed().unwrap_or(speed.clone()),
                "rotation" | "tex_rot_rotation" => *rotation = fixed().unwrap_or(rotation.clone()),
                _ => {}
            },
            AffectorKind::Vortex { vector, speed, .. } => match name {
                "rotation_axis" | "vortex_aff_vector" => *vector = p.vec3().unwrap_or(*vector),
                "rotation_speed" | "vortex_aff_speed" => *speed = fixed().unwrap_or(speed.clone()),
                _ => {}
            },
            AffectorKind::Randomiser { deviation, time_since, step, random_direction, .. } => match name {
                "max_deviation_x" | "rand_aff_max_deviation_x" => deviation.x = p.f(0).unwrap_or(deviation.x),
                "max_deviation_y" | "rand_aff_max_deviation_y" => deviation.y = p.f(0).unwrap_or(deviation.y),
                "max_deviation_z" | "rand_aff_max_deviation_z" => deviation.z = p.f(0).unwrap_or(deviation.z),
                "time_step" | "rand_aff_time_step" => {
                    if let Some(v) = p.f(0) {
                        *step = v;
                        *time_since = v;
                    }
                }
                "use_direction" | "rand_aff_direction" => *random_direction = p.boolean().unwrap_or(*random_direction),
                _ => {}
            },
            AffectorKind::Gravity { gravity } => {
                if name == "gravity" {
                    *gravity = p.f(0).unwrap_or(*gravity);
                }
            }
            AffectorKind::ForceField(f) => f.property(name, p),
            AffectorKind::Jet { acceleration } => {
                if name == "acceleration" || name == "jet_aff_accel" {
                    *acceleration = fixed().unwrap_or(acceleration.clone());
                }
            }
            AffectorKind::Align { resize } => {
                if name == "resize" || name == "align_aff_resize" {
                    *resize = p.boolean().unwrap_or(*resize);
                }
            }
            AffectorKind::GeometryRotator { speed, own_speed, axis, axis_set } => match name {
                "use_own_rotation" | "geom_rot_use_own_rotation" => *own_speed = p.boolean().unwrap_or(*own_speed),
                "rotation_speed" | "geom_rot_rotation_speed" => *speed = fixed().unwrap_or(speed.clone()),
                "rotation_axis" | "geom_rot_axis" => {
                    if let Some(v) = p.vec3() {
                        *axis = v;
                        *axis_set = true;
                    }
                }
                _ => {}
            },
            AffectorKind::ScaleVelocity { scale, since_start, stop_at_flip } => match name {
                "velocity_scale" => *scale = fixed().unwrap_or(scale.clone()),
                "since_start_system" => *since_start = p.boolean().unwrap_or(*since_start),
                "stop_at_flip" => *stop_at_flip = p.boolean().unwrap_or(*stop_at_flip),
                _ => {}
            },
            _ => {}
        }
    }

    fn object(&mut self, class: &str, d: Dyn) {
        match self {
            AffectorKind::Scale { x, y, z, xyz, .. } => match class {
                "xyz_scale" => *xyz = Some(d),
                "x_scale" => *x = Some(d),
                "y_scale" => *y = Some(d),
                "z_scale" => *z = Some(d),
                _ => {}
            },
            AffectorKind::TextureRotator { rotation, speed, .. } => match class {
                "rotation_speed" | "tex_rot_speed" => *speed = d,
                "rotation" | "tex_rot_rotation" => *rotation = d,
                _ => {}
            },
            AffectorKind::Vortex { speed, .. } if class == "rotation_speed" || class == "vortex_aff_speed" => *speed = d,
            AffectorKind::Jet { acceleration } if class == "acceleration" || class == "jet_aff_accel" => *acceleration = d,
            AffectorKind::GeometryRotator { speed, .. } if class == "rotation_speed" || class == "geom_rot_rotation_speed" => {
                *speed = d
            }
            AffectorKind::ScaleVelocity { scale, .. } if class == "velocity_scale" => *scale = d,
            _ => {}
        }
    }
}

/// ForceFieldAffector with the realtime calculation (ForceField, Noise3D): the force is the
/// noise's gradient at the particle, mapped into a world box round where the system was.
#[derive(Debug, Clone)]
pub struct ForceField {
    pub delta: f32,
    pub scale_force: f32,
    pub octaves: u16,
    pub frequency: f64,
    pub amplitude: f64,
    pub persistence: f64,
    pub world_size: Vec3,
    pub ignore_negative: bevy::math::BVec3,
    pub movement: Vec3,
    pub movement_set: bool,
    pub movement_frequency: f32,
    pub movement_frequency_count: f32,
    base_position: Vec3,
    position_base: Vec3,
    initialised: bool,
}

impl Default for ForceField {
    fn default() -> Self {
        ForceField {
            delta: 1.0,
            scale_force: 400.0,
            octaves: 2,
            frequency: 1.0,
            amplitude: 1.0,
            persistence: 3.0,
            world_size: Vec3::splat(500.0),
            ignore_negative: bevy::math::BVec3::FALSE,
            movement: Vec3::new(500.0, 0.0, 0.0),
            movement_set: false,
            movement_frequency: 5.0,
            movement_frequency_count: 0.0,
            base_position: Vec3::ZERO,
            position_base: Vec3::ZERO,
            initialised: false,
        }
    }
}

impl ForceField {
    fn property(&mut self, name: &str, p: &Prop) {
        match name {
            "delta" => self.delta = p.f(0).unwrap_or(self.delta),
            "force" => self.scale_force = p.f(0).unwrap_or(self.scale_force),
            "octaves" => self.octaves = p.f(0).map_or(self.octaves, |v| v as u16),
            "frequency" => self.frequency = p.f(0).map_or(self.frequency, |v| v as f64),
            "amplitude" => self.amplitude = p.f(0).map_or(self.amplitude, |v| v as f64),
            "persistence" => self.persistence = p.f(0).map_or(self.persistence, |v| v as f64),
            "worldsize" => {
                if let Some(v) = p.vec3()
                    && v != Vec3::ZERO
                {
                    self.world_size = v;
                }
            }
            "ignore_negative_x" => self.ignore_negative.x = p.boolean().unwrap_or(false),
            "ignore_negative_y" => self.ignore_negative.y = p.boolean().unwrap_or(false),
            "ignore_negative_z" => self.ignore_negative.z = p.boolean().unwrap_or(false),
            "movement" => {
                if let Some(v) = p.vec3() {
                    self.movement = v;
                    self.movement_set = v != Vec3::ZERO;
                }
            }
            "movement_frequency" => {
                if let Some(v) = p.f(0) {
                    self.movement_frequency = v;
                    self.movement_set = v > 0.0;
                }
            }
            // forcefield_size only sizes the matrix calculation
            _ => {}
        }
    }

    fn initialise(&mut self, sys_position: Vec3) {
        self.position_base = sys_position - 0.5 * self.world_size;
        self.base_position = self.position_base;
        self.initialised = true;
    }

    fn pre_process(&mut self, dt: f32) {
        if self.movement_set {
            if dt > self.movement_frequency {
                return;
            }
            self.movement_frequency_count += dt;
            if self.movement_frequency_count > self.movement_frequency {
                self.movement_frequency_count -= self.movement_frequency;
            }
            let disp = (std::f32::consts::TAU * self.movement_frequency_count / self.movement_frequency).sin() * self.movement;
            self.position_base = self.base_position + disp;
        }
    }

    fn noise(&self, x: f64, y: f64, z: f64) -> f64 {
        let mut n = 0.0;
        let mut freq = self.frequency;
        let mut ampl = self.amplitude;
        for _ in 0..self.octaves {
            n += perlin(x * freq, y * freq, z * freq) * ampl;
            freq *= 2.0;
            ampl *= self.persistence;
        }
        n
    }

    fn determine_force(&self, position: Vec3) -> Vec3 {
        if !self.initialised {
            return Vec3::ZERO;
        }
        let p = position - self.position_base;
        let m = Vec3::new(p.x / self.world_size.x, p.y / self.world_size.y, p.z / self.world_size.z);
        if m.x < 0.0 || m.x > 1.0 || m.y < 0.0 || m.y > 1.0 || m.z < 0.0 || m.z > 1.0 {
            return Vec3::ZERO;
        }
        let (x, y, z, d) = (m.x as f64, m.y as f64, m.z as f64, self.delta as f64);
        Vec3::new(
            (self.noise(x + d, y, z) - self.noise(x - d, y, z)) as f32,
            (self.noise(x, y + d, z) - self.noise(x, y - d, z)) as f32,
            (self.noise(x, y, z + d) - self.noise(x, y, z - d)) as f32,
        )
    }
}

const PERMUTATION: [u8; 256] = [
    151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69, 142, 8, 99, 37, 240,
    21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219, 203, 117, 35, 11, 32, 57, 177, 33, 88,
    237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175, 74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83,
    111, 229, 122, 60, 211, 133, 230, 220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80,
    73, 209, 76, 132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173, 186, 3, 64,
    52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206, 59, 227, 47, 16, 58, 17, 182,
    189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163, 70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22,
    39, 253, 19, 98, 108, 110, 79, 113, 224, 232, 178, 185, 112, 104, 218, 246, 97, 228, 251, 34, 242, 193, 238, 210,
    144, 12, 191, 179, 162, 241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157, 184, 84,
    204, 176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141, 128, 195, 78,
    66, 215, 61, 156, 180,
];

fn perm(i: i32) -> i32 {
    PERMUTATION[(i & 255) as usize] as i32
}

/// Noise3D::_noise: Ken Perlin's improved noise, its absolute value (as a float).
fn perlin(mut x: f64, mut y: f64, mut z: f64) -> f64 {
    let xi = (x.floor() as i32) & 255;
    let yi = (y.floor() as i32) & 255;
    let zi = (z.floor() as i32) & 255;
    x -= x.floor();
    y -= y.floor();
    z -= z.floor();
    let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let lerp = |t: f64, a: f64, b: f64| a + t * (b - a);
    let grad = |hash: i32, x: f64, y: f64, z: f64| {
        let h = hash & 15;
        let u = if h < 8 { x } else { y };
        let v = if h < 4 { y } else if h == 12 || h == 14 { x } else { z };
        (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
    };
    let (u, v, w) = (fade(x), fade(y), fade(z));
    let a = perm(xi) + yi;
    let aa = perm(a) + zi;
    let ab = perm(a + 1) + zi;
    let b = perm(xi + 1) + yi;
    let ba = perm(b) + zi;
    let bb = perm(b + 1) + zi;
    let r = lerp(
        w,
        lerp(
            v,
            lerp(u, grad(perm(aa), x, y, z), grad(perm(ba), x - 1.0, y, z)),
            lerp(u, grad(perm(ab), x, y - 1.0, z), grad(perm(bb), x - 1.0, y - 1.0, z)),
        ),
        lerp(
            v,
            lerp(u, grad(perm(aa + 1), x, y, z - 1.0), grad(perm(ba + 1), x - 1.0, y, z - 1.0)),
            lerp(u, grad(perm(ab + 1), x, y - 1.0, z - 1.0), grad(perm(bb + 1), x - 1.0, y - 1.0, z - 1.0)),
        ),
    );
    (r as f32).abs() as f64
}
