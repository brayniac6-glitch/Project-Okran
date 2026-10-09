//! ParticleUniverse's particle systems and techniques (ParticleUniverseSystem,
//! ParticleUniverseTechnique): a system's state (prepared, started, stopped), time scaling,
//! iteration interval, fast forward, fixed timeout and fading stop, LOD; each technique's pools,
//! emission, pre-processing, the per-particle loop (behaviours, affectors, emitted emitters and
//! techniques, motion with keep_local) and observers, as PU orders them.

use super::affector::{AffectCtx, Affector, AffectorKind};
use super::emitter::{EmitCtx, Emitter, EmitterKind};
use super::math::{Quat, Rand};
use super::observer::{Handler, Observer, ObserverKind, Placement};
use super::particle::{EmitterId, PEF_EMITTED, PEF_EXPIRED, PRef, PType, Particle, Pool};
use super::script::{Item, Node};
use bevy::math::{Vec3, Vec4};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillboardType {
    Point,
    OrientedCommon,
    OrientedSelf,
    OrientedShape,
    PerpendicularCommon,
    PerpendicularSelf,
}

/// Ogre::BillboardOrigin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Debug, Clone)]
pub struct Billboard {
    pub ty: BillboardType,
    pub origin: Origin,
    /// BBR_VERTEX (else BBR_TEXCOORD).
    pub rotate_vertices: bool,
    pub common_direction: Vec3,
    pub common_up: Vec3,
    pub accurate_facing: bool,
    /// texture_coords_define rectangles (left, top, right, bottom); else rows x columns.
    pub texture_coords: Vec<[f32; 4]>,
    pub rows_columns: Option<(u8, u8)>,
}

#[derive(Debug, Clone)]
pub enum RendererKind {
    Billboard(Billboard),
    Sphere,
}

#[derive(Debug, Clone)]
pub struct Renderer {
    pub kind: RendererKind,
    pub sorted: bool,
    pub visible: bool,
    pub initialised: bool,
    /// The default dimensions the renderer was told at its preparation (scaled).
    pub default_dims: Vec3,
}

impl Renderer {
    fn from_node(n: &Node) -> Option<Renderer> {
        let mut kind = match n.name.as_str() {
            "Billboard" => RendererKind::Billboard(Billboard {
                ty: BillboardType::Point,
                origin: Origin::Center,
                rotate_vertices: false,
                common_direction: Vec3::Z,
                common_up: Vec3::Y,
                accurate_facing: false,
                texture_coords: Vec::new(),
                rows_columns: None,
            }),
            "Sphere" => RendererKind::Sphere,
            _ => return None,
        };
        let mut sorted = false;
        let (mut rows, mut cols) = (1u8, 1u8);
        let mut rc_set = false;
        for p in &n.props {
            match (p.name.as_str(), &mut kind) {
                ("sorting", _) => sorted = p.boolean().unwrap_or(false),
                ("texture_coords_rows", _) => {
                    rows = p.f(0).unwrap_or(1.0) as u8;
                    rc_set = true;
                }
                ("texture_coords_columns", _) => {
                    cols = p.f(0).unwrap_or(1.0) as u8;
                    rc_set = true;
                }
                ("texture_coords_define", RendererKind::Billboard(b)) => {
                    if let (Some(u), Some(v), Some(w), Some(h)) = (p.f(0), p.f(1), p.f(2), p.f(3)) {
                        b.texture_coords.push([u, v, u + w, v + h]);
                    }
                }
                ("billboard_type", RendererKind::Billboard(b)) => {
                    b.ty = match p.string() {
                        Some("point") => BillboardType::Point,
                        Some("oriented_common") => BillboardType::OrientedCommon,
                        Some("oriented_self") => BillboardType::OrientedSelf,
                        Some("oriented_shape") => BillboardType::OrientedShape,
                        Some("perpendicular_common") => BillboardType::PerpendicularCommon,
                        Some("perpendicular_self") => BillboardType::PerpendicularSelf,
                        _ => b.ty,
                    }
                }
                ("billboard_origin", RendererKind::Billboard(b)) => {
                    b.origin = match p.string() {
                        Some("top_left") => Origin::TopLeft,
                        Some("top_center") => Origin::TopCenter,
                        Some("top_right") => Origin::TopRight,
                        Some("center_left") => Origin::CenterLeft,
                        Some("center") => Origin::Center,
                        Some("center_right") => Origin::CenterRight,
                        Some("bottom_left") => Origin::BottomLeft,
                        Some("bottom_center") => Origin::BottomCenter,
                        Some("bottom_right") => Origin::BottomRight,
                        _ => b.origin,
                    }
                }
                ("billboard_rotation_type", RendererKind::Billboard(b)) => match p.string() {
                    Some("vertex") => b.rotate_vertices = true,
                    Some("texcoord") => b.rotate_vertices = false,
                    _ => {}
                },
                ("common_direction", RendererKind::Billboard(b)) => b.common_direction = p.vec3().unwrap_or(b.common_direction),
                ("common_up_vector", RendererKind::Billboard(b)) => b.common_up = p.vec3().unwrap_or(b.common_up),
                ("accurate_facing", RendererKind::Billboard(b)) => b.accurate_facing = p.boolean().unwrap_or(false),
                _ => {}
            }
        }
        if let RendererKind::Billboard(b) = &mut kind
            && rc_set
            && b.texture_coords.is_empty()
        {
            b.rows_columns = Some((rows, cols));
        }
        Some(Renderer { kind, sorted, visible: true, initialised: false, default_dims: Vec3::ZERO })
    }

    /// _getSortMode: by distance (Ogre's BillboardSet with accurate facing or perpendicular
    /// billboards; spheres), else along the camera's direction.
    fn sort_by_distance(&self) -> bool {
        match &self.kind {
            RendererKind::Billboard(b) => {
                b.accurate_facing
                    || matches!(b.ty, BillboardType::PerpendicularCommon | BillboardType::PerpendicularSelf)
            }
            RendererKind::Sphere => true,
        }
    }
}

/// A technique listening to emissions in another (TechniqueListener): a Slave emitter.
#[derive(Debug, Clone, Copy)]
pub struct SlaveListener {
    pub technique: usize,
    pub emitter: usize,
}

#[derive(Debug, Clone)]
pub struct Technique {
    /// The technique as a particle (its position offset; where it is when emitted).
    pub p: Particle,
    pub name: String,
    pub material: String,
    pub visual_quota: usize,
    pub emitter_quota: usize,
    pub technique_quota: usize,
    pub default_dims: Vec3,
    pub lod_index: u16,
    pub keep_local: bool,
    pub max_velocity: Option<f32>,
    pub emitters: Vec<Emitter>,
    pub affectors: Vec<Affector>,
    pub observers: Vec<Observer>,
    pub renderer: Option<Renderer>,
    pub has_slave_behaviour: bool,
    pub visuals: Vec<Particle>,
    pub pooled_emitters: Vec<Emitter>,
    pub pooled_techniques: Vec<Technique>,
    pub pool: Pool,
    visual_pool_increased: bool,
    emitter_pool_increased: bool,
    technique_pool_increased: bool,
    prepare_emitter: bool,
    prepare_affector: bool,
    pub listeners: Vec<SlaveListener>,
    pub scale: Vec3,
    pub camera_square_distance: f32,
    update_world_aabb: bool,
    has_extents: bool,
    min_extent: Vec3,
    max_extent: Vec3,
    pub world_aabb: Option<(Vec3, Vec3)>,
    /// Kenshi's camera wrap extern is on it (F023396 adds it to every technique).
    pub camera_wrap: bool,
    /// Kenshi's global extern is on it (F023401).
    pub global_wrap: bool,
}

/// What a technique reads of its system.
#[derive(Debug, Clone, Copy)]
pub struct SysCtx {
    pub position: Vec3,
    pub orientation: Quat,
    pub latest_position: Vec3,
    pub time: f32,
    pub stop_fade: bool,
    pub smooth_lod: bool,
    pub tight_bounding_box: bool,
    pub keep_local: bool,
    pub scale_velocity: f32,
    pub rotation_centre: Vec3,
    pub rotation_offset: Quat,
    pub sun_colour: Vec3,
}

/// A pool slot taken out while it is worked on.
#[allow(clippy::large_enum_variant)]
enum Taken {
    V(Particle),
    E(Box<Emitter>),
    T(Box<Technique>),
}

impl Taken {
    fn p(&mut self) -> &mut Particle {
        match self {
            Taken::V(p) => p,
            Taken::E(e) => &mut e.p,
            Taken::T(t) => &mut t.p,
        }
    }
}

impl Technique {
    fn empty() -> Technique {
        let mut p = Particle::new(PType::Technique);
        p.marked_for_emission = false;
        Technique {
            p,
            name: String::new(),
            material: "BaseWhite".into(),
            visual_quota: 500,
            emitter_quota: 50,
            technique_quota: 10,
            default_dims: Vec3::splat(50.0),
            lod_index: 0,
            keep_local: false,
            max_velocity: None,
            emitters: Vec::new(),
            affectors: Vec::new(),
            observers: Vec::new(),
            renderer: None,
            has_slave_behaviour: false,
            visuals: Vec::new(),
            pooled_emitters: Vec::new(),
            pooled_techniques: Vec::new(),
            pool: Pool::default(),
            visual_pool_increased: false,
            emitter_pool_increased: false,
            technique_pool_increased: false,
            prepare_emitter: false,
            prepare_affector: false,
            listeners: Vec::new(),
            camera_wrap: false,
            global_wrap: false,
            scale: Vec3::ONE,
            camera_square_distance: 0.0,
            update_world_aabb: false,
            has_extents: false,
            min_extent: Vec3::ZERO,
            max_extent: Vec3::ZERO,
            world_aabb: None,
        }
    }

    fn from_node(n: &Node) -> Technique {
        let mut t = Technique::empty();
        t.name = n.name.clone();
        for item in &n.order {
            match *item {
                Item::Prop(i) => {
                    let p = &n.props[i];
                    let u = || p.f(0).map(|v| v as usize);
                    match p.name.as_str() {
                        "enabled" => {
                            if let Some(v) = p.boolean() {
                                t.p.set_enabled(v);
                            }
                        }
                        "position" => {
                            if let Some(v) = p.vec3() {
                                t.p.position = v;
                                t.p.original_position = v;
                            }
                        }
                        "keep_local" => t.keep_local = p.boolean().unwrap_or(false),
                        "visual_particle_quota" => {
                            t.visual_quota = u().unwrap_or(t.visual_quota);
                            t.visual_pool_increased = false;
                        }
                        "emitted_emitter_quota" => {
                            t.emitter_quota = u().unwrap_or(t.emitter_quota);
                            t.emitter_pool_increased = false;
                        }
                        "emitted_technique_quota" => {
                            t.technique_quota = u().unwrap_or(t.technique_quota);
                            t.technique_pool_increased = false;
                        }
                        "material" => t.material = p.string().unwrap_or_default().to_string(),
                        "lod_index" => t.lod_index = p.f(0).unwrap_or(0.0) as u16,
                        "default_particle_width" => t.default_dims.x = p.f(0).unwrap_or(t.default_dims.x),
                        "default_particle_height" => t.default_dims.y = p.f(0).unwrap_or(t.default_dims.y),
                        "default_particle_depth" => t.default_dims.z = p.f(0).unwrap_or(t.default_dims.z),
                        "max_velocity" => t.max_velocity = p.f(0),
                        // emitted affector and system quotas: nothing in Kenshi's scripts emits
                        // those; spatial hashing: no affector of Kenshi's uses it
                        _ => {}
                    }
                }
                Item::Child(i) => {
                    let c = &n.children[i];
                    match c.class.as_str() {
                        "renderer" => t.renderer = Renderer::from_node(c),
                        "emitter" => {
                            if let Some(e) = Emitter::from_node(c) {
                                t.emitters.push(e);
                                t.prepare_emitter = true;
                            }
                        }
                        "affector" => {
                            if let Some(a) = Affector::from_node(c) {
                                t.affectors.push(a);
                                t.prepare_affector = true;
                            }
                        }
                        "observer" => t.observers.push(Observer::from_node(c)),
                        "behaviour" if c.name == "Slave" => t.has_slave_behaviour = true,
                        _ => {}
                    }
                }
            }
        }
        t
    }

    /// getDerivedPosition.
    pub fn derived(&self, s: &SysCtx) -> Vec3 {
        if self.p.marked_for_emission {
            self.p.position
        } else {
            s.position + s.orientation.rotate(self.scale * self.p.position)
        }
    }

    fn emit_ctx(&self, s: &SysCtx) -> EmitCtx {
        EmitCtx {
            time: s.time,
            sys_orientation: s.orientation,
            tech_position: self.derived(s),
            default_dims: self.default_dims,
            stop_fade: s.stop_fade,
        }
    }

    fn affect_ctx(&self, s: &SysCtx) -> AffectCtx {
        AffectCtx {
            time: s.time,
            sys_orientation: s.orientation,
            tech_position: self.derived(s),
            scale_velocity: s.scale_velocity,
            num_emitted: self.num_emitted(),
            sun_colour: s.sun_colour,
        }
    }

    /// getNumberOfEmittedParticles (with those of emitted techniques).
    pub fn num_emitted(&self) -> usize {
        self.pool.size()
            + self
                .pool
                .technique
                .released()
                .iter()
                .map(|&i| self.pooled_techniques[i as usize].num_emitted())
                .sum::<usize>()
    }

    /// _notifyRescaled.
    pub fn rescale(&mut self, scale: Vec3) {
        self.scale = scale;
        for e in &mut self.emitters {
            e.rescale(scale);
        }
        for a in &mut self.affectors {
            a.scale = scale;
        }
        for o in &mut self.observers {
            o.scale = scale;
        }
        for i in self.pool.technique.released() {
            self.pooled_techniques[i as usize].rescale(scale);
        }
    }

    fn reset_bounds(&mut self, s: &SysCtx) {
        self.world_aabb = None;
        let p = self.derived(s);
        self.min_extent = p;
        self.max_extent = p;
        self.has_extents = false;
    }

    /// _notifyUpdateBounds.
    pub fn notify_update_bounds(&mut self, s: &SysCtx) {
        if self.pool.is_empty() {
            self.reset_bounds(s);
            self.update_world_aabb = false;
        } else {
            self.update_world_aabb = true;
        }
    }

    /// _notifyStart.
    pub fn notify_start(&mut self, s: &SysCtx, rng: &mut Rand) {
        self.p.latest_position = self.derived(s);
        if let Some(r) = &mut self.renderer {
            r.visible = true;
        }
        for o in &mut self.observers {
            o.notify_start();
        }
        for a in &mut self.affectors {
            if !a.p.marked_for_emission {
                a.notify_start();
            }
        }
        let c = self.emit_ctx(s);
        for e in &mut self.emitters {
            if !e.p.marked_for_emission {
                e.notify_start(&c, rng);
            }
        }
        // the pooled components
        let mut cur = self.pool.first();
        while !self.pool.at_end() {
            match cur {
                Some(PRef::Technique(i)) => self.pooled_techniques[i as usize].notify_start(s, rng),
                Some(PRef::Emitter(i)) => self.pooled_emitters[i as usize].notify_start(&c, rng),
                _ => {}
            }
            cur = self.pool.next();
        }
        let e = self.p.original_enabled;
        self.p.set_enabled(e);
    }

    /// _notifyStop: everything expires at once.
    pub fn notify_stop(&mut self, s: &SysCtx) {
        if let Some(r) = &mut self.renderer {
            r.visible = false;
        }
        for i in self.pool.technique.released() {
            self.pooled_techniques[i as usize].notify_stop(s);
        }
        // _initAllParticlesForExpiration
        for i in self.pool.technique.released() {
            self.pooled_techniques[i as usize].notify_stop(s);
        }
        self.pool.lock_all();
        self.reset_bounds(s);
    }

    fn pref_particle(&self, r: PRef) -> &Particle {
        match r {
            PRef::Visual(i) => &self.visuals[i as usize],
            PRef::Emitter(i) => &self.pooled_emitters[i as usize].p,
            PRef::Technique(i) => &self.pooled_techniques[i as usize].p,
        }
    }

    fn pref_particle_mut(&mut self, r: PRef) -> &mut Particle {
        match r {
            PRef::Visual(i) => &mut self.visuals[i as usize],
            PRef::Emitter(i) => &mut self.pooled_emitters[i as usize].p,
            PRef::Technique(i) => &mut self.pooled_techniques[i as usize].p,
        }
    }

    fn take(&mut self, r: PRef) -> Taken {
        match r {
            PRef::Visual(i) => Taken::V(std::mem::replace(&mut self.visuals[i as usize], Particle::new(PType::Visual))),
            PRef::Emitter(i) => {
                Taken::E(Box::new(std::mem::replace(&mut self.pooled_emitters[i as usize], Emitter::placeholder())))
            }
            PRef::Technique(i) => {
                Taken::T(Box::new(std::mem::replace(&mut self.pooled_techniques[i as usize], Technique::empty())))
            }
        }
    }

    fn put(&mut self, r: PRef, t: Taken) {
        match (r, t) {
            (PRef::Visual(i), Taken::V(p)) => self.visuals[i as usize] = p,
            (PRef::Emitter(i), Taken::E(e)) => self.pooled_emitters[i as usize] = *e,
            (PRef::Technique(i), Taken::T(t)) => self.pooled_techniques[i as usize] = *t,
            _ => unreachable!(),
        }
    }

    fn emitter_name(&self, id: EmitterId) -> &str {
        match id {
            EmitterId::Own(e) => &self.emitters[e].name,
            EmitterId::Pooled(k) => &self.pooled_emitters[k].name,
        }
    }

    /// makeParticleLocal for the technique.
    fn make_local(&self, p: &mut Particle, s: &SysCtx) -> bool {
        if !self.keep_local || self.p.has(PEF_EXPIRED) {
            return false;
        }
        p.position += self.derived(s) - self.p.latest_position;
        true
    }
}

/// Everything about one technique update; `techs` the system's techniques (one emitted technique
/// alone when `nested`, which then reaches no other technique).
struct Run<'a> {
    techs: &'a mut [Technique],
    k: &'a super::kenshi::KenshiHooks,
    s: SysCtx,
    dt: f32,
    rng: &'a mut Rand,
    nested: bool,
    /// DoStopSystem ran.
    stop: bool,
}

impl Run<'_> {
    /// _update.
    fn update(&mut self, ti: usize) {
        {
            let t = &self.techs[ti];
            if !t.p.enabled && (!self.s.smooth_lod || t.pool.is_empty()) {
                return;
            }
        }
        self.prepare(ti);
        if self.s.tight_bounding_box {
            let s = self.s;
            self.techs[ti].reset_bounds(&s);
        }
        self.emit_particles(ti);
        self.pre_process(ti);
        self.process_particles(ti);
        self.post_process(ti);
        let s = self.s;
        let d = self.techs[ti].derived(&s);
        self.techs[ti].p.latest_position = d;
    }

    /// _prepare: pools sized to their quotas, emitters and affectors prepared, rescaled.
    fn prepare(&mut self, ti: usize) {
        let s = self.s;
        // renderer
        {
            let t = &mut self.techs[ti];
            let dims = t.scale * t.default_dims;
            if let Some(r) = &mut t.renderer
                && !r.initialised
            {
                r.default_dims = dims;
                r.initialised = true;
            }
        }
        // emitters: Slave emitters listen to their master technique
        if self.techs[ti].prepare_emitter {
            for e in 0..self.techs[ti].emitters.len() {
                if let EmitterKind::Slave(sl) = &self.techs[ti].emitters[e].kind {
                    if !self.nested
                        && !sl.master_technique.is_empty()
                        && let Some(m) = self.techs.iter().position(|t| t.name == sl.master_technique)
                    {
                        self.techs[m].listeners.push(SlaveListener { technique: ti, emitter: e });
                    }
                    self.techs[ti].emitters[e].p.enabled = false;
                }
            }
            self.techs[ti].prepare_emitter = false;
        }
        let t = &mut self.techs[ti];
        if !t.emitter_pool_increased {
            let old = t.pooled_emitters.len();
            let marked: Vec<usize> = (0..t.emitters.len()).filter(|&e| t.emitters[e].p.marked_for_emission).collect();
            if t.emitter_quota >= old && !marked.is_empty() {
                let increment = (t.emitter_quota - old) / marked.len();
                for &e in &marked {
                    for _ in 0..increment {
                        let mut c = t.emitters[e].clone();
                        c.p.marked_for_emission = true;
                        let key = c.name.clone();
                        t.pool.emitter.add(Some(key));
                        t.pooled_emitters.push(c);
                    }
                }
            }
            t.emitter_pool_increased = true;
        }
        if t.prepare_affector {
            for a in &mut t.affectors {
                a.prepare(s.position);
            }
            t.prepare_affector = false;
        }
        if !self.techs[ti].technique_pool_increased {
            if !self.nested {
                let old = self.techs[ti].pooled_techniques.len();
                let marked: Vec<usize> =
                    (0..self.techs.len()).filter(|&k| self.techs[k].p.marked_for_emission).collect();
                if self.techs[ti].technique_quota >= old && !marked.is_empty() {
                    let increment = (self.techs[ti].technique_quota - old) / marked.len();
                    for &k in &marked {
                        for _ in 0..increment {
                            let mut c = self.techs[k].clone();
                            c.pooled_techniques.clear();
                            c.pool = Pool::default();
                            c.visuals.clear();
                            c.pooled_emitters.clear();
                            c.visual_pool_increased = false;
                            c.emitter_pool_increased = false;
                            c.technique_pool_increased = true;
                            c.listeners.clear();
                            c.p.marked_for_emission = true;
                            let key = c.name.clone();
                            self.techs[ti].pool.technique.add(Some(key));
                            self.techs[ti].pooled_techniques.push(c);
                        }
                    }
                }
            }
            self.techs[ti].technique_pool_increased = true;
        }
        let t = &mut self.techs[ti];
        if !t.visual_pool_increased {
            while t.visuals.len() < t.visual_quota {
                t.pool.visual.add(None);
                t.visuals.push(Particle::new(PType::Visual));
            }
            t.visual_pool_increased = true;
        }
        let scale = t.scale;
        t.rescale(scale);
    }

    /// _emitParticles.
    fn emit_particles(&mut self, ti: usize) {
        if !self.techs[ti].p.enabled {
            return;
        }
        for e in 0..self.techs[ti].emitters.len() {
            if self.techs[ti].emitters[e].p.marked_for_emission {
                continue;
            }
            let c = self.techs[ti].emit_ctx(&self.s);
            let (req, lock_all) = self.techs[ti].emitters[e].requested(self.dt, &c, self.rng);
            if lock_all {
                self.techs[ti].pool.lock_all();
            }
            let mut em = std::mem::replace(&mut self.techs[ti].emitters[e], Emitter::placeholder());
            self.execute_emit(ti, &mut em, EmitterId::Own(e), req, self.dt, None);
            self.techs[ti].emitters[e] = em;
        }
    }

    /// _executeEmitParticles; `placement` a DoPlacementParticle handler's base particle and
    /// settings while it forces this emission.
    fn execute_emit(
        &mut self,
        ti: usize,
        em: &mut Emitter,
        id: EmitterId,
        requested: u32,
        dt: f32,
        placement: Option<(&Particle, &Placement)>,
    ) {
        if !(self.techs[ti].p.enabled && em.p.enabled) {
            return;
        }
        let s = self.s;
        let mut time_point = 0.0;
        let inc = dt / requested as f32;
        for _ in 0..requested {
            let t = &mut self.techs[ti];
            let r = match em.emits_type {
                PType::Visual => t.pool.visual.release(None).map(PRef::Visual),
                PType::Emitter => t.pool.emitter.release(Some(&em.emits_name)).map(PRef::Emitter),
                PType::Technique => t.pool.technique.release(Some(&em.emits_name)).map(PRef::Technique),
                _ => None,
            };
            let Some(r) = r else { return };
            let c = t.emit_ctx(&s);
            let has_slave = t.has_slave_behaviour;
            let mut item = t.take(r);
            match &mut item {
                Taken::V(p) => p.init_for_emission(),
                Taken::E(e) => {
                    e.p.init_for_emission();
                    e.notify_start(&c, self.rng);
                }
                Taken::T(pt) => {
                    pt.p.init_for_emission();
                    pt.notify_start(&s, self.rng);
                    pt.pool.lock_all();
                }
            }
            {
                let p = item.p();
                p.parent_emitter = Some(id);
                em.init_particle(p, &c, self.rng);
                if !has_slave {
                    p.slave_master = None;
                }
                p.direction = s.orientation.rotate(p.direction);
                p.original_direction = s.orientation.rotate(p.original_direction);
            }
            let ac = self.techs[ti].affect_ctx(&s);
            for a in &self.techs[ti].affectors {
                if a.p.enabled {
                    a.init_particle(item.p(), &ac, self.rng);
                }
            }
            // technique listeners: a placement handler's inheritance, Slave emitters
            if let Some((base, pl)) = placement
                && pl.found.map(|(_, e)| EmitterId::Own(e)) == Some(id)
            {
                pl.inherit(base, item.p());
            }
            let master = (item.p().position, item.p().direction);
            self.techs[ti].put(r, item);
            if !self.nested {
                let listeners = self.techs[ti].listeners.clone();
                for l in listeners {
                    self.slave_emitted(l, ti, r, master, &em.name);
                }
            }
            let p = self.techs[ti].pref_particle_mut(r);
            p.position += p.direction * s.scale_velocity * time_point;
            time_point += inc;
        }
    }

    /// SlaveEmitter::particleEmitted: one particle at the master's.
    fn slave_emitted(&mut self, l: SlaveListener, master_tech: usize, r: PRef, at: (Vec3, Vec3), emitter_name: &str) {
        let mut em = std::mem::replace(&mut self.techs[l.technique].emitters[l.emitter], Emitter::placeholder());
        if let EmitterKind::Slave(sl) = &mut em.kind
            && !(sl.master_emitter_set && sl.master_emitter != emitter_name)
        {
            {
                sl.master_position = at.0;
                sl.master_direction = at.1;
                sl.master_particle = Some((master_tech, r));
                em.p.enabled = true;
                self.execute_emit(l.technique, &mut em, EmitterId::Own(l.emitter), 1, 0.0, None);
                em.p.enabled = false;
            }
        }
        self.techs[l.technique].emitters[l.emitter] = em;
    }

    /// _preProcessParticles.
    fn pre_process(&mut self, ti: usize) {
        let s = self.s;
        let t = &mut self.techs[ti];
        let c = t.emit_ctx(&s);
        for e in &mut t.emitters {
            if !e.p.marked_for_emission && e.p.enabled {
                e.derived_position(&c);
            }
        }
        let ac = t.affect_ctx(&s);
        for a in &mut t.affectors {
            if !a.p.marked_for_emission && a.p.enabled {
                a.pre_process(&ac, self.dt, self.rng);
            }
        }
        for o in 0..self.techs[ti].observers.len() {
            let num = self.techs[ti].num_emitted();
            let obs = &mut self.techs[ti].observers[o];
            obs.pre_process(self.dt);
            if matches!(obs.kind, ObserverKind::OnTime { .. }) && num == 0 && obs.observes(None, s.time) {
                self.handle_event(ti, o, None);
            }
        }
    }

    /// _postProcessParticles.
    fn post_process(&mut self, ti: usize) {
        let s = self.s;
        let t = &mut self.techs[ti];
        t.update_world_aabb = false;
        if t.has_extents {
            t.world_aabb = Some((t.min_extent, t.max_extent));
        }
        let c = t.emit_ctx(&s);
        for e in &mut t.emitters {
            if !e.p.marked_for_emission && e.p.enabled {
                e.p.latest_position = e.derived_position(&c);
            }
        }
        for a in &mut t.affectors {
            if !a.p.marked_for_emission && a.p.enabled {
                a.post_process();
            }
        }
        for o in 0..self.techs[ti].observers.len() {
            let num = self.techs[ti].num_emitted();
            if let ObserverKind::OnClear { cont } = &mut self.techs[ti].observers[o].kind
                && *cont
                && num == 0
            {
                *cont = false;
                self.handle_event(ti, o, None);
            }
        }
    }

    /// _processParticles.
    fn process_particles(&mut self, ti: usize) {
        if self.techs[ti].pool.is_empty() {
            return;
        }
        let s = self.s;
        let dt = self.dt;
        let mut align_previous: Vec<Option<PRef>> = vec![None; self.techs[ti].affectors.len()];
        let mut cur = self.techs[ti].pool.first();
        let mut first_particle = true;
        let mut first_active = true;
        while !self.techs[ti].pool.at_end() {
            if let Some(r) = cur {
                let mut item = self.techs[ti].take(r);
                let expired = {
                    let p = item.p();
                    let e = p.time_to_live < dt;
                    if e {
                        p.flags |= PEF_EXPIRED;
                    }
                    e
                };
                if !expired {
                    self.process_self(ti, item.p());
                    self.process_affectors(ti, r, item.p(), first_active, &mut align_previous);
                    // _processExternals: Kenshi's camera wrap
                    if self.techs[ti].camera_wrap
                        && let Some((centre, size)) = self.k.wrap
                    {
                        let p = item.p();
                        p.position = super::kenshi::wrap(p.position, centre, size);
                    }
                    if self.techs[ti].global_wrap
                        && let Some(g) = self.k.global
                    {
                        let p = item.p();
                        p.position = super::kenshi::global_wrap(p.position, g, self.rng);
                    }
                    if self.techs[ti].p.enabled {
                        match &mut item {
                            Taken::E(e) => {
                                let c = self.techs[ti].emit_ctx(&s);
                                let (req, lock_all) = e.requested(dt, &c, self.rng);
                                if lock_all {
                                    self.techs[ti].pool.lock_all();
                                }
                                let id = match r {
                                    PRef::Emitter(i) => EmitterId::Pooled(i as usize),
                                    _ => unreachable!(),
                                };
                                self.execute_emit(ti, e, id, req, dt, None);
                            }
                            Taken::T(t) => {
                                if self.techs[ti].update_world_aabb {
                                    t.notify_update_bounds(&s);
                                }
                                let mut nested = Run {
                                    techs: std::slice::from_mut(t.as_mut()),
                                    k: self.k,
                                    s,
                                    dt,
                                    rng: self.rng,
                                    nested: true,
                                    stop: false,
                                };
                                nested.update(0);
                                self.stop |= nested.stop;
                                if self.techs[ti].update_world_aabb
                                    && let Some((lo, hi)) = t.world_aabb
                                {
                                    let tt = &mut self.techs[ti];
                                    if tt.has_extents {
                                        tt.min_extent = tt.min_extent.min(lo);
                                        tt.max_extent = tt.max_extent.max(hi);
                                    } else {
                                        tt.min_extent = lo;
                                        tt.max_extent = hi;
                                        tt.has_extents = true;
                                    }
                                }
                            }
                            Taken::V(_) => {}
                        }
                    }
                    first_active = false;
                    let p = item.p();
                    p.latest_position = p.position;
                    self.process_motion(ti, item.p());
                } else {
                    if let Taken::T(t) = &mut item {
                        t.notify_stop(&s);
                    }
                    self.techs[ti].pool.lock_latest();
                }
                self.techs[ti].put(r, item);
                // observed even when expired (and locked)
                self.process_observers(ti, r, first_particle);
                let p = self.techs[ti].pref_particle_mut(r);
                p.flags = if p.has(PEF_EXPIRED) { PEF_EXPIRED } else { 0 };
                p.time_to_live -= dt;
            }
            first_particle = false;
            cur = self.techs[ti].pool.next();
        }
    }

    /// _processParticleSelf: the time fraction, the Slave behaviour, the bounds.
    fn process_self(&mut self, ti: usize, p: &mut Particle) {
        p.process_time_fraction();
        if !self.nested
            && self.techs[ti].has_slave_behaviour
            && let Some((mt, mr)) = p.slave_master
        {
            let m = self.techs[mt].pref_particle(mr);
            if !m.has(PEF_EXPIRED) {
                p.position = m.position;
                p.direction = m.direction;
            }
        }
        let t = &mut self.techs[ti];
        if t.update_world_aabb && p.ptype == PType::Visual {
            let max_dim = if p.own_dimensions {
                p.width.max(p.height).max(p.depth)
            } else {
                t.default_dims.x.max(t.default_dims.y).max(t.default_dims.z)
            };
            let pad = Vec3::splat(0.5 * max_dim);
            if t.has_extents {
                t.min_extent = t.min_extent.min(p.position - pad);
                t.max_extent = t.max_extent.max(p.position + pad);
            } else {
                t.min_extent = p.position - pad;
                t.max_extent = p.position + pad;
                t.has_extents = true;
            }
        }
    }

    /// _processAffectors (the technique's own; no emitted affectors in Kenshi's scripts).
    #[allow(clippy::needless_range_loop)]
    fn process_affectors(&mut self, ti: usize, r: PRef, p: &mut Particle, first: bool, align: &mut [Option<PRef>]) {
        let s = self.s;
        let ac = self.techs[ti].affect_ctx(&s);
        for a in 0..self.techs[ti].affectors.len() {
            let t = &self.techs[ti];
            let af = &t.affectors[a];
            if af.p.marked_for_emission || !af.p.enabled {
                continue;
            }
            let is_align = matches!(af.kind, AffectorKind::Align { .. });
            if first && is_align {
                align[a] = Some(r);
            }
            if !af.excluded.is_empty()
                && let Some(id) = p.parent_emitter
                && af.excluded.iter().any(|x| x == t.emitter_name(id))
            {
                continue;
            }
            let previous = if is_align {
                match align[a] {
                    Some(pr) if pr == r => Some(p.position),
                    Some(pr) => Some(t.pref_particle(pr).position),
                    None => None,
                }
            } else {
                None
            };
            self.techs[ti].affectors[a].affect(p, self.dt, &ac, previous, self.rng);
            if is_align {
                align[a] = Some(r);
            }
        }
    }

    /// _processMotion.
    fn process_motion(&mut self, ti: usize, p: &mut Particle) {
        if p.freezed {
            return;
        }
        let s = self.s;
        let t = &self.techs[ti];
        if !p.has(PEF_EMITTED) {
            let by_emitter = match p.parent_emitter {
                Some(EmitterId::Own(e)) => t.emitters[e].make_particle_local(p),
                Some(EmitterId::Pooled(k)) => t.pooled_emitters[k].make_particle_local(p),
                None => false,
            };
            if !by_emitter && !t.make_local(p, &s) && s.keep_local {
                p.position += s.position - s.latest_position;
            }
        }
        if s.keep_local && !t.keep_local {
            // billboards and spheres are not rotated by Ogre
            p.position = s.rotation_centre + s.rotation_offset.rotate(p.position - s.rotation_centre);
        }
        if p.has(PEF_EMITTED) {
            return;
        }
        if let Some(max) = t.max_velocity
            && p.calculate_velocity() > max
        {
            p.direction *= max / p.direction.length();
        }
        p.position += p.direction * s.scale_velocity * self.dt;
    }

    /// _processObservers.
    fn process_observers(&mut self, ti: usize, r: PRef, first: bool) {
        let time = self.s.time;
        for o in 0..self.techs[ti].observers.len() {
            let fire = {
                let t = &mut self.techs[ti];
                let p = match r {
                    PRef::Visual(i) => &t.visuals[i as usize],
                    PRef::Emitter(i) => &t.pooled_emitters[i as usize].p,
                    PRef::Technique(i) => &t.pooled_techniques[i as usize].p,
                };
                let ptype = p.ptype;
                let obs = &t.observers[o];
                if let ObserverKind::OnClear { .. } = obs.kind {
                    if first && let ObserverKind::OnClear { cont } = &mut t.observers[o].kind {
                        *cont = true;
                    }
                    continue;
                }
                if !obs.enabled || obs.type_to_observe.is_some_and(|ty| ty != ptype) {
                    continue;
                }
                obs.observes(Some(p), time)
            };
            if fire {
                self.handle_event(ti, o, Some(r));
            }
        }
    }

    /// _handleEvent: the observer's handlers in order.
    fn handle_event(&mut self, ti: usize, o: usize, r: Option<PRef>) {
        for h in 0..self.techs[ti].observers[o].handlers.len() {
            let handler = self.techs[ti].observers[o].handlers[h].clone();
            match handler {
                Handler::DoExpire => {
                    if let Some(r) = r {
                        self.techs[ti].pref_particle_mut(r).time_to_live = 0.0;
                    }
                }
                Handler::DoStopSystem => self.stop = true,
                Handler::DoPlacement(mut pl) => {
                    let Some(r) = r else { continue };
                    // getEmitter: a blank name finds nothing
                    if pl.found.is_none() && !pl.force_emitter.is_empty() {
                        let own = self.techs[ti].emitters.iter().position(|e| e.name == pl.force_emitter);
                        pl.found = match own {
                            Some(e) => Some((ti, e)),
                            None if !self.nested => self.techs.iter().enumerate().find_map(|(k, t)| {
                                t.emitters.iter().position(|e| e.name == pl.force_emitter).map(|e| (k, e))
                            }),
                            None => None,
                        };
                        if let Handler::DoPlacement(stored) = &mut self.techs[ti].observers[o].handlers[h] {
                            stored.found = pl.found;
                        }
                    }
                    let Some((tj, ej)) = pl.found else { continue };
                    let base = self.techs[ti].pref_particle(r).clone();
                    // forceEmission
                    if !self.techs[tj].p.enabled {
                        continue;
                    }
                    let mut em = std::mem::replace(&mut self.techs[tj].emitters[ej], Emitter::placeholder());
                    self.execute_emit(tj, &mut em, EmitterId::Own(ej), pl.number, 0.0, Some((&base, &pl)));
                    self.techs[tj].emitters[ej] = em;
                }
                Handler::KenshiGroundColour => {
                    if let (Some(r @ PRef::Visual(_)), Some(map)) = (r, &self.k.ground) {
                        let p = self.techs[ti].pref_particle_mut(r);
                        let at = p.latest_position + self.k.origin;
                        map.tint(at, &mut p.original_colour);
                    }
                }
                Handler::KenshiWind => {
                    if let (Some(PRef::Technique(i)), Some(w)) = (r, &self.k.wind) {
                        super::kenshi::apply_wind(&mut self.techs[ti].pooled_techniques[i as usize], w);
                    }
                }
                Handler::Other => {}
            }
        }
        self.techs[ti].observers[o].handlers_executed = true;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Stopped,
    Prepared,
    Started,
    Paused,
}

/// A particle system: a template from a script, or an instance of one.
#[derive(Debug, Clone)]
pub struct System {
    pub name: String,
    pub techniques: Vec<Technique>,
    pub state: State,
    pub iteration_interval: Option<f32>,
    pub nonvisible_update_timeout: Option<f32>,
    pub fixed_timeout: Option<f32>,
    pub fast_forward: Option<(f32, f32)>,
    original_fast_forward: Option<(f32, f32)>,
    pub scale: Vec3,
    pub scale_velocity: f32,
    pub scale_time: f32,
    pub keep_local: bool,
    pub smooth_lod: bool,
    pub tight_bounding_box: bool,
    /// Squared distances.
    pub lod_distances: Vec<f32>,
    pub time_since_start: f32,
    time_since_last_update: f32,
    time_since_last_visible: f32,
    pub stop_fade_set: bool,
    at_least_one_emitted: bool,
    pub enabled: bool,
    last_lod_index: u16,
    /// The scene node: where the system is.
    pub position: Vec3,
    pub orientation: Quat,
    pub latest_position: Vec3,
    latest_orientation: Quat,
    rotation_offset: Quat,
    rotation_centre: Vec3,
    /// The system's bounds in its node's space.
    pub aabb: Option<(Vec3, Vec3)>,
    /// Seen by the camera last frame (for nonvisible_update_timeout).
    pub visible_last_frame: bool,
    pub rng: Rand,
    /// What Kenshi's effect wrapper adds (`kenshi`).
    pub kenshi: super::kenshi::KenshiHooks,
}

impl System {
    /// A `system Name { ... }` object.
    pub fn from_node(n: &Node) -> System {
        let mut s = System {
            name: n.name.clone(),
            techniques: Vec::new(),
            state: State::Stopped,
            iteration_interval: None,
            nonvisible_update_timeout: None,
            fixed_timeout: None,
            fast_forward: None,
            original_fast_forward: None,
            scale: Vec3::ONE,
            scale_velocity: 1.0,
            scale_time: 1.0,
            keep_local: false,
            smooth_lod: false,
            tight_bounding_box: false,
            lod_distances: Vec::new(),
            time_since_start: 0.0,
            time_since_last_update: 0.0,
            time_since_last_visible: 0.0,
            stop_fade_set: false,
            at_least_one_emitted: false,
            enabled: true,
            last_lod_index: 0,
            position: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            latest_position: Vec3::ZERO,
            latest_orientation: Quat::IDENTITY,
            rotation_offset: Quat::IDENTITY,
            rotation_centre: Vec3::ZERO,
            aabb: None,
            visible_last_frame: true,
            rng: Rand::default(),
            kenshi: Default::default(),
        };
        for item in &n.order {
            match *item {
                Item::Prop(i) => {
                    let p = &n.props[i];
                    match p.name.as_str() {
                        "iteration_interval" => s.iteration_interval = p.f(0).filter(|v| *v > 0.0),
                        "nonvisible_update_timeout" => s.nonvisible_update_timeout = p.f(0).filter(|v| *v > 0.0),
                        "fixed_timeout" => s.fixed_timeout = p.f(0).filter(|v| *v > 0.0),
                        "lod_distances" => {
                            for v in p.values.iter().filter_map(|v| super::script::parse_f32(v)) {
                                s.lod_distances.push(v * v);
                            }
                        }
                        "smooth_lod" => s.smooth_lod = p.boolean().unwrap_or(false),
                        "fast_forward" => {
                            if let (Some(a), Some(b)) = (p.f(0), p.f(1)) {
                                s.fast_forward = Some((a, b));
                                s.original_fast_forward = Some((a, b));
                            }
                        }
                        "scale" => s.scale = p.vec3().unwrap_or(s.scale),
                        "scale_velocity" => s.scale_velocity = p.f(0).unwrap_or(1.0),
                        "scale_time" => s.scale_time = p.f(0).unwrap_or(1.0),
                        "keep_local" => s.keep_local = p.boolean().unwrap_or(false),
                        "tight_bounding_box" => s.tight_bounding_box = p.boolean().unwrap_or(false),
                        _ => {}
                    }
                }
                Item::Child(i) => {
                    let c = &n.children[i];
                    if c.class == "technique" {
                        s.techniques.push(Technique::from_node(c));
                    }
                }
            }
        }
        s
    }

    /// An instance of a template (createParticleSystem: the template copied, what is emitted by
    /// what marked, rescaled).
    pub fn instance(template: &System, seed: u32) -> System {
        let mut s = template.clone();
        s.rng = Rand::new(seed);
        // _markForEmission
        for t in &mut s.techniques {
            t.p.marked_for_emission = false;
        }
        for ti in 0..s.techniques.len() {
            for e in &mut s.techniques[ti].emitters {
                e.p.marked_for_emission = false;
            }
            for a in &mut s.techniques[ti].affectors {
                a.p.marked_for_emission = false;
            }
            for e in 0..s.techniques[ti].emitters.len() {
                let (ty, name, own) = {
                    let em = &s.techniques[ti].emitters[e];
                    (em.emits_type, em.emits_name.clone(), em.name.clone())
                };
                match ty {
                    PType::Technique => {
                        if let Some(k) = s.techniques.iter().position(|t| t.name == name && !name.is_empty()) {
                            s.techniques[k].p.marked_for_emission = true;
                        }
                    }
                    PType::Emitter if name != own && !name.is_empty() => {
                        if let Some(k) = s.techniques[ti].emitters.iter().position(|x| x.name == name) {
                            s.techniques[ti].emitters[k].p.marked_for_emission = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        let scale = s.scale;
        for t in &mut s.techniques {
            t.rescale(scale);
        }
        s
    }

    fn derived_orientation(&self) -> Quat {
        self.orientation
    }

    fn ctx(&self) -> SysCtx {
        SysCtx {
            position: self.position,
            orientation: self.orientation,
            latest_position: self.latest_position,
            time: self.time_since_start,
            stop_fade: self.is_stop_fade(),
            smooth_lod: self.smooth_lod,
            tight_bounding_box: self.tight_bounding_box,
            keep_local: self.keep_local,
            scale_velocity: self.scale_velocity,
            rotation_centre: self.rotation_centre,
            rotation_offset: self.rotation_offset,
            sun_colour: self.kenshi.sun_colour,
        }
    }

    /// isStopFade.
    pub fn is_stop_fade(&self) -> bool {
        match self.fixed_timeout {
            Some(t) => self.stop_fade_set && self.time_since_start >= t,
            None => self.stop_fade_set,
        }
    }

    /// prepare.
    pub fn prepare(&mut self) {
        self.state = State::Prepared;
    }

    /// start: everything restarts (and fast-forwards when set).
    pub fn start(&mut self) {
        self.state = State::Started;
        self.time_since_start = 0.0;
        self.time_since_last_visible = 0.0;
        self.time_since_last_update = 0.0;
        self.aabb = None;
        self.fast_forward = self.original_fast_forward;
        self.latest_position = self.position;
        self.latest_orientation = self.derived_orientation();
        self.enabled = true;
        self.stop_fade_set = false;
        self.at_least_one_emitted = false;
        self.last_lod_index = 0;
        let s = self.ctx();
        for t in &mut self.techniques {
            t.notify_start(&s, &mut self.rng);
        }
        if let Some((time, interval)) = self.fast_forward {
            let mut f = 0.0;
            while f < time {
                self.update(interval);
                f += interval;
            }
            self.fast_forward = None;
        }
    }

    /// stop: everything expires.
    pub fn stop(&mut self) {
        if self.state != State::Stopped {
            let s = self.ctx();
            for t in &mut self.techniques {
                t.notify_stop(&s);
            }
            self.aabb = None;
            self.state = State::Stopped;
            self.stop_fade_set = false;
            self.enabled = false;
        }
    }

    /// stopFade: the emitters stop; the system stops once its particles are gone.
    pub fn stop_fade(&mut self) {
        let s = self.ctx();
        for t in &mut self.techniques {
            let c = t.emit_ctx(&s);
            for e in &mut t.emitters {
                e.set_enabled(false, &c, &mut self.rng);
            }
        }
        self.stop_fade_set = true;
    }

    /// stopFade(stopTime).
    pub fn stop_fade_after(&mut self, time: f32) {
        self.fixed_timeout = (time > 0.0).then_some(time);
        self.stop_fade_set = true;
    }

    pub fn num_emitted(&self) -> usize {
        self.techniques.iter().map(|t| t.num_emitted()).sum()
    }

    /// _update.
    pub fn update(&mut self, dt: f32) {
        match self.state {
            State::Started => {
                let mut dt = dt;
                if let Some(timeout) = self.nonvisible_update_timeout
                    && !self.visible_last_frame
                {
                    self.time_since_last_visible += dt;
                    if self.time_since_last_visible >= timeout {
                        return;
                    }
                }
                dt *= self.scale_time;
                self.time_since_start += dt;
                if let Some(t) = self.fixed_timeout
                    && self.time_since_start >= t
                {
                    if self.stop_fade_set {
                        self.stop_fade();
                        self.fixed_timeout = None;
                    } else {
                        self.stop();
                        return;
                    }
                }
                // calulateRotationOffset
                self.rotation_centre = self.position;
                self.rotation_offset = self.derived_orientation() * self.latest_orientation.inverse();
                let mut left = None;
                if let Some(interval) = self.iteration_interval {
                    self.time_since_last_update += dt;
                    while self.time_since_last_update >= interval {
                        left = Some(self.update_techniques(interval));
                        self.time_since_last_update -= interval;
                    }
                } else {
                    left = Some(self.update_techniques(dt));
                }
                if let Some(left) = left {
                    if left == 0 {
                        self.at_least_one_emitted = false;
                        if self.stop_fade_set
                            && self.fixed_timeout.is_none_or(|t| self.time_since_start >= t)
                        {
                            self.stop();
                            return;
                        }
                    } else {
                        self.at_least_one_emitted = true;
                    }
                }
            }
            State::Prepared => {
                let s = self.ctx();
                let mut rng = std::mem::take(&mut self.rng);
                for ti in 0..self.techniques.len() {
                    let mut run = Run { techs: &mut self.techniques, k: &self.kenshi, s, dt: 0.0, rng: &mut rng, nested: false, stop: false };
                    run.prepare(ti);
                }
                self.rng = rng;
                self.state = State::Stopped;
            }
            _ => {}
        }
        self.latest_position = self.position;
        self.latest_orientation = self.derived_orientation();
    }

    /// _updateTechniques: the techniques not emitted themselves; returns the particles left.
    fn update_techniques(&mut self, dt: f32) -> usize {
        let s = self.ctx();
        let mut rng = std::mem::take(&mut self.rng);
        let mut left = 0;
        let mut stop = false;
        let mut world: Option<(Vec3, Vec3)> = Some((self.position, self.position));
        for ti in 0..self.techniques.len() {
            if self.techniques[ti].p.marked_for_emission {
                continue;
            }
            self.techniques[ti].notify_update_bounds(&s);
            let mut run = Run { techs: &mut self.techniques, k: &self.kenshi, s, dt, rng: &mut rng, nested: false, stop: false };
            run.update(ti);
            stop |= run.stop;
            if let (Some((lo, hi)), Some((tl, th))) = (world, self.techniques[ti].world_aabb) {
                world = Some((lo.min(tl), hi.max(th)));
            }
            left += self.techniques[ti].num_emitted();
        }
        self.rng = rng;
        if stop {
            self.stop();
        }
        if self.state == State::Stopped {
            self.aabb = None;
        } else if let Some((lo, hi)) = world {
            // the bounds in the node's space, merged with the last (not tight)
            let inv = self.orientation.inverse();
            let corners = [
                Vec3::new(lo.x, lo.y, lo.z),
                Vec3::new(hi.x, lo.y, lo.z),
                Vec3::new(lo.x, hi.y, lo.z),
                Vec3::new(lo.x, lo.y, hi.z),
                Vec3::new(hi.x, hi.y, lo.z),
                Vec3::new(hi.x, lo.y, hi.z),
                Vec3::new(lo.x, hi.y, hi.z),
                Vec3::new(hi.x, hi.y, hi.z),
            ];
            let mut l = Vec3::splat(f32::MAX);
            let mut h = Vec3::splat(f32::MIN);
            for c in corners {
                let v = inv.rotate(c - self.position);
                l = l.min(v);
                h = h.max(v);
            }
            self.aabb = Some(match (self.tight_bounding_box, self.aabb) {
                (false, Some((al, ah))) => (al.min(l), ah.max(h)),
                _ => (l, h),
            });
        }
        left
    }

    /// _notifyCurrentCamera: the camera distances, LOD, and the sorting of sorted renderers.
    pub fn notify_camera(&mut self, camera: Vec3, camera_dir: Vec3) {
        self.time_since_last_visible = 0.0;
        let sq_node = (camera - self.position).length_squared();
        let s = self.ctx();
        for t in &mut self.techniques {
            let sq = if t.p.marked_for_emission { (camera - t.p.position).length_squared() } else { sq_node };
            t.camera_square_distance = sq;
            if !self.lod_distances.is_empty() {
                let index = self.lod_distances.iter().position(|d| sq < *d).unwrap_or(self.lod_distances.len()) as u16;
                if t.lod_index == index {
                    t.p.set_enabled(true);
                    self.last_lod_index = index;
                } else {
                    t.p.set_enabled(false);
                }
            }
            sort_visuals(t, camera, camera_dir);
            let _ = &s;
        }
    }

    /// Each technique's visual particles to draw (with those of emitted techniques), in order.
    pub fn for_each_visible<F: FnMut(&Technique, &[Particle], &[u32])>(&self, mut f: F) {
        for t in &self.techniques {
            visit(t, &mut f);
        }
    }
}

fn visit<F: FnMut(&Technique, &[Particle], &[u32])>(t: &Technique, f: &mut F) {
    let Some(r) = &t.renderer else { return };
    if !r.initialised || !r.visible {
        return;
    }
    f(t, &t.visuals, &t.pool.visual.released());
    for i in t.pool.technique.released() {
        visit(&t.pooled_techniques[i as usize], f);
    }
}

/// _sortVisualParticles: the released list itself reordered (RadixSort, stable, ascending).
fn sort_visuals(t: &mut Technique, camera: Vec3, camera_dir: Vec3) {
    let Some(r) = &t.renderer else { return };
    if !r.initialised {
        return;
    }
    if r.sorted {
        let by_distance = r.sort_by_distance();
        let mut order = t.pool.visual.released();
        let key = |p: &Particle| {
            if by_distance { -(camera - p.position).length_squared() } else { (-camera_dir).dot(p.position) }
        };
        order.sort_by(|&a, &b| {
            key(&t.visuals[a as usize]).partial_cmp(&key(&t.visuals[b as usize])).unwrap_or(std::cmp::Ordering::Equal)
        });
        t.pool.visual.set_released_order(&order);
    }
    for i in t.pool.technique.released() {
        sort_visuals(&mut t.pooled_techniques[i as usize], camera, camera_dir);
    }
}

/// Unused: keeps Vec4 in scope for colour helpers used by renderers.
pub fn colour_bytes(c: Vec4) -> [u8; 4] {
    // Ogre's ColourValue::getAsRGBA: (u8)(v * 255), no clamping
    let b = |v: f32| (v * 255.0) as i32 as u8;
    [b(c.x), b(c.y), b(c.z), b(c.w)]
}
