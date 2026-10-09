//! ParticleUniverse's observers and event handlers as Kenshi's scripts use them (OnPosition,
//! OnTime, OnEmission, OnClear observers; DoExpire, DoPlacementParticle, DoStopSystem
//! handlers). An observer watches each particle (or the technique) and when its condition holds
//! runs its handlers; the technique carries the handlers out.

use super::particle::{PEF_EMITTED, PType, Particle};
use super::script::{Item, Node};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    LessThan,
    GreaterThan,
    Equals,
}

impl Compare {
    fn parse(s: &str) -> Option<Compare> {
        match s {
            "less_than" => Some(Compare::LessThan),
            "greater_than" => Some(Compare::GreaterThan),
            "equals" => Some(Compare::Equals),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ObserverKind {
    OnPosition { x: Option<(Compare, f32)>, y: Option<(Compare, f32)>, z: Option<(Compare, f32)> },
    OnTime { threshold: f32, compare: Compare, since_start: bool },
    OnEmission,
    OnClear { cont: bool },
    /// Types Kenshi's scripts do not use: never observe anything.
    Other,
}

#[derive(Debug, Clone)]
pub struct Placement {
    pub force_emitter: String,
    pub number: u32,
    /// The technique and emitter found by name, once found.
    pub found: Option<(usize, usize)>,
    pub inherit_position: bool,
    pub inherit_direction: bool,
    pub inherit_orientation: bool,
    pub inherit_time_to_live: bool,
    pub inherit_mass: bool,
    pub inherit_texture_coord: bool,
    pub inherit_colour: bool,
    pub inherit_width: bool,
    pub inherit_height: bool,
    pub inherit_depth: bool,
}

#[derive(Debug, Clone)]
pub enum Handler {
    DoExpire,
    DoPlacement(Box<Placement>),
    DoStopSystem,
    /// Kenshi's ground colour handler (F033108).
    KenshiGroundColour,
    /// Kenshi's wind handler on emitted techniques (F033083).
    KenshiWind,
    Other,
}

impl Handler {
    fn from_node(n: &Node) -> Handler {
        match n.name.as_str() {
            "DoExpire" => Handler::DoExpire,
            "DoStopSystem" => Handler::DoStopSystem,
            "DoPlacementParticle" => {
                let mut p = Placement {
                    force_emitter: String::new(),
                    number: 1,
                    found: None,
                    inherit_position: true,
                    inherit_direction: false,
                    inherit_orientation: false,
                    inherit_time_to_live: false,
                    inherit_mass: false,
                    inherit_texture_coord: false,
                    inherit_colour: false,
                    inherit_width: false,
                    inherit_height: false,
                    inherit_depth: false,
                };
                for pr in &n.props {
                    let b = pr.boolean().unwrap_or(false);
                    match pr.name.as_str() {
                        "force_emitter" => p.force_emitter = pr.string().unwrap_or_default().to_string(),
                        "number_of_particles" => p.number = pr.f(0).unwrap_or(1.0) as u32,
                        "inherit_position" => p.inherit_position = b,
                        "inherit_direction" => p.inherit_direction = b,
                        "inherit_orientation" => p.inherit_orientation = b,
                        "inherit_time_to_live" => p.inherit_time_to_live = b,
                        "inherit_mass" => p.inherit_mass = b,
                        "inherit_texture_coord" => p.inherit_texture_coord = b,
                        "inherit_colour" => p.inherit_colour = b,
                        "inherit_width" => p.inherit_width = b,
                        "inherit_height" => p.inherit_height = b,
                        "inherit_depth" => p.inherit_depth = b,
                        _ => {}
                    }
                }
                Handler::DoPlacement(Box::new(p))
            }
            _ => Handler::Other,
        }
    }
}

impl Placement {
    /// particleEmitted: the new particle takes the base particle's attributes.
    pub fn inherit(&self, base: &Particle, p: &mut Particle) {
        if self.inherit_position {
            p.position = base.position;
            p.original_position = p.position;
        }
        if self.inherit_direction {
            p.direction = base.direction;
            p.original_direction = p.direction;
            p.original_direction_length = base.original_direction_length;
            p.original_scaled_direction_length = base.original_scaled_direction_length;
            p.original_velocity = base.original_velocity;
        }
        let both_visual = base.ptype == PType::Visual && p.ptype == PType::Visual;
        if self.inherit_orientation && both_visual {
            p.orientation = base.orientation;
            p.original_orientation = base.original_orientation;
        }
        if self.inherit_time_to_live {
            p.time_to_live = base.time_to_live;
            p.total_time_to_live = base.total_time_to_live;
            p.time_fraction = base.time_fraction;
        }
        if self.inherit_mass {
            p.mass = base.mass;
        }
        if self.inherit_texture_coord && both_visual {
            p.texture_coords_current = base.texture_coords_current;
        }
        if self.inherit_colour && both_visual {
            p.colour = base.colour;
            p.original_colour = base.original_colour;
        }
        if self.inherit_width && both_visual {
            let (h, d) = (p.height, p.depth);
            p.set_own_dimensions(base.width, h, d);
        }
        if self.inherit_height && both_visual {
            let (w, d) = (p.width, p.depth);
            p.set_own_dimensions(w, base.height, d);
        }
        if self.inherit_depth && both_visual {
            let (w, h) = (p.width, p.height);
            p.set_own_dimensions(w, h, base.depth);
        }
    }
}

#[derive(Debug, Clone)]
pub struct Observer {
    pub name: String,
    pub kind: ObserverKind,
    pub enabled: bool,
    pub original_enabled: bool,
    pub original_enabled_set: bool,
    pub observe: bool,
    pub type_to_observe: Option<PType>,
    pub interval: f32,
    pub interval_remainder: f32,
    pub interval_set: bool,
    pub until_event: bool,
    pub handlers_executed: bool,
    pub handlers: Vec<Handler>,
    pub scale: bevy::math::Vec3,
}

impl Observer {
    /// An OnEmission observer with one handler (F033084's createObserver("OnEmission")).
    pub fn kenshi_on_emission(handler: Handler) -> Observer {
        let mut o = Observer::from_node(&Node {
            class: "observer".into(),
            name: "OnEmission".into(),
            ..Default::default()
        });
        o.handlers.push(handler);
        o
    }

    pub fn from_node(n: &Node) -> Observer {
        let mut kind = match n.name.as_str() {
            "OnPosition" => ObserverKind::OnPosition { x: None, y: None, z: None },
            "OnTime" => ObserverKind::OnTime { threshold: 0.0, compare: Compare::GreaterThan, since_start: false },
            "OnEmission" => ObserverKind::OnEmission,
            "OnClear" => ObserverKind::OnClear { cont: false },
            _ => ObserverKind::Other,
        };
        let mut o = Observer {
            name: n.values.first().cloned().unwrap_or_default(),
            kind: ObserverKind::Other,
            enabled: true,
            original_enabled: true,
            original_enabled_set: false,
            observe: true,
            type_to_observe: None,
            interval: 0.05,
            interval_remainder: 0.0,
            interval_set: false,
            until_event: false,
            handlers_executed: false,
            handlers: Vec::new(),
            scale: bevy::math::Vec3::ONE,
        };
        for item in &n.order {
            match *item {
                Item::Prop(i) => {
                    let p = &n.props[i];
                    let compared = || -> Option<(Compare, f32)> { Some((Compare::parse(p.values.first()?)?, p.f(1)?)) };
                    match p.name.as_str() {
                        "enabled" => {
                            if let Some(v) = p.boolean() {
                                o.set_enabled(v);
                            }
                        }
                        "observe_particle_type" => {
                            o.type_to_observe = match p.string() {
                                Some("visual_particle") => Some(PType::Visual),
                                Some("emitter_particle") => Some(PType::Emitter),
                                Some("affector_particle") => Some(PType::Affector),
                                Some("technique_particle") => Some(PType::Technique),
                                Some("system_particle") => Some(PType::System),
                                _ => o.type_to_observe,
                            }
                        }
                        "observe_interval" => {
                            if let Some(v) = p.f(0) {
                                o.interval = v;
                                o.interval_set = true;
                            }
                        }
                        "observe_until_event" => o.until_event = p.boolean().unwrap_or(o.until_event),
                        "position_x" | "position_y" | "position_z" => {
                            if let (ObserverKind::OnPosition { x, y, z }, Some(c)) = (&mut kind, compared()) {
                                match p.name.as_str() {
                                    "position_x" => *x = Some(c),
                                    "position_y" => *y = Some(c),
                                    _ => *z = Some(c),
                                }
                            }
                        }
                        "on_time" => {
                            if let (ObserverKind::OnTime { threshold, compare, .. }, Some((c, t))) = (&mut kind, compared()) {
                                *compare = c;
                                *threshold = t;
                            }
                        }
                        "since_start_system" => {
                            if let ObserverKind::OnTime { since_start, .. } = &mut kind {
                                *since_start = p.boolean().unwrap_or(false);
                            }
                        }
                        _ => {}
                    }
                }
                Item::Child(i) => {
                    let c = &n.children[i];
                    if c.class == "handler" {
                        o.handlers.push(Handler::from_node(c));
                    }
                }
            }
        }
        o.kind = kind;
        o
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !self.original_enabled_set {
            self.original_enabled = enabled;
            self.original_enabled_set = true;
        }
    }

    /// _notifyStart.
    pub fn notify_start(&mut self) {
        self.handlers_executed = false;
        self.observe = true;
        let e = self.original_enabled;
        self.set_enabled(e);
        if let ObserverKind::OnClear { cont } = &mut self.kind {
            *cont = false;
        }
    }

    /// ParticleObserver::_preProcessParticles (the interval).
    pub fn pre_process(&mut self, dt: f32) {
        if !self.enabled {
            return;
        }
        if self.interval_set {
            self.interval_remainder -= dt;
            if self.interval_remainder < 0.0 {
                self.interval_remainder += self.interval;
                self.observe = true;
            } else {
                self.observe = false;
            }
        }
    }

    /// _handleObserve's test: whether the handlers run for `particle` (None: the technique).
    pub fn observes(&self, particle: Option<&Particle>, time_since_start: f32) -> bool {
        if !(self.enabled && self.observe) {
            return false;
        }
        if self.until_event && self.handlers_executed {
            return false;
        }
        match &self.kind {
            ObserverKind::OnPosition { x, y, z } => {
                let Some(p) = particle else { return false };
                let test = |c: &Option<(Compare, f32)>, v: f32, s: f32| match c {
                    Some((Compare::LessThan, t)) => v < t * s,
                    Some((Compare::GreaterThan, t)) => v > t * s,
                    Some((Compare::Equals, t)) => v == t * s,
                    None => false,
                };
                test(x, p.position.x, self.scale.x) || test(y, p.position.y, self.scale.y) || test(z, p.position.z, self.scale.z)
            }
            ObserverKind::OnTime { threshold, compare, since_start } => {
                let value = if *since_start {
                    Some(time_since_start)
                } else {
                    particle.map(|p| p.total_time_to_live - p.time_to_live)
                };
                let Some(v) = value else { return false };
                match compare {
                    Compare::GreaterThan => v > *threshold,
                    Compare::LessThan => v < *threshold,
                    // almostEquals: within (not at) the tolerance
                    Compare::Equals => v + 0.01 > *threshold && v - 0.01 < *threshold,
                }
            }
            ObserverKind::OnEmission => particle.is_some_and(|p| p.has(PEF_EMITTED)),
            ObserverKind::OnClear { .. } | ObserverKind::Other => false,
        }
    }
}
