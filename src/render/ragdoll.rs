//! Ragdolls on characters: a race's ragdoll file (F039259: "male ragdoll" / "female ragdoll")
//! bound to the skeleton, started from the pose last drawn, and its bodies turned back into the
//! joints' local transforms.
use bevy::math::{Affine3A, Quat, Vec3};
use bevy::prelude::*;
use kenshi_anim::Rig;
use kenshi_io::phs::Phs;
use kenshi_physics::ragdoll::{Ragdoll, Template};

/// A ragdoll file bound to a skeleton.
pub struct RagdollRig {
    pub template: Template,
    /// Per body: the bone it moves, and the body's frame in that bone's.
    bodies: Vec<Option<(usize, Affine3A)>>,
    /// Per bone: the body moving it.
    driven: Vec<Option<usize>>,
    /// Bones parents first.
    order: Vec<usize>,
}

fn affine(l: &kenshi_anim::Local) -> Affine3A {
    Affine3A::from_scale_rotation_translation(l.scale, l.rotation, l.translation)
}

impl RagdollRig {
    pub fn new(phs: &Phs, rig: &Rig) -> RagdollRig {
        let template = Template::new(phs);
        let n = rig.names.len();
        // parents first
        let mut order = Vec::with_capacity(n);
        let mut placed = vec![false; n];
        while order.len() < n {
            let before = order.len();
            for k in 0..n {
                if !placed[k] && rig.parents[k].is_none_or(|p| placed[p]) {
                    placed[k] = true;
                    order.push(k);
                }
            }
            if order.len() == before {
                break;
            }
        }
        // bind globals in the skeleton's space, which the ragdoll file shares
        let mut bind = vec![Affine3A::IDENTITY; n];
        for &k in &order {
            let l = affine(&rig.bind[k]);
            bind[k] = rig.parents[k].map_or(l, |p| bind[p] * l);
        }
        let mut driven = vec![None; n];
        let bodies = template
            .bodies
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let k = rig.names.iter().position(|x| x == &b.bone)?;
                driven[k] = Some(i);
                Some((k, bind[k].inverse() * Affine3A::from_translation(b.bind)))
            })
            .collect();
        RagdollRig { template, bodies, driven, order }
    }

    /// The body moving bone `k`, if any.
    pub fn body_of_bone(&self, k: usize) -> Option<usize> {
        self.driven.get(k).copied().flatten()
    }

    /// The number of bodies.
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    /// Body `i`'s world pose (centre of mass, rotation from bind) where the bones' world
    /// transforms put it.
    pub fn body_pose(&self, i: usize, bones: &[Affine3A]) -> Option<(Vec3, Quat)> {
        let (k, off) = self.bodies.get(i).copied().flatten()?;
        let (_, r, t) = (*bones.get(k)? * off).to_scale_rotation_translation();
        Some((t, r))
    }

    /// The body moving the skeleton's root bone, and that body's frame in the bone's.
    pub fn root_body(&self) -> Option<(usize, Affine3A)> {
        let root = *self.order.first()?;
        let i = self.driven[root]?;
        Some((i, self.bodies[i]?.1))
    }

    /// A ragdoll whose bodies sit where the bones' world transforms put them.
    pub fn start(&self, bones: &[Affine3A], velocity: Vec3) -> Ragdoll {
        let poses: Vec<(Vec3, Quat)> = self
            .bodies
            .iter()
            .zip(&self.template.bodies)
            .map(|(b, def)| match b {
                Some((k, off)) => {
                    let (_, r, t) = (bones[*k] * *off).to_scale_rotation_translation();
                    (t, r)
                }
                None => (def.bind, Quat::IDENTITY),
            })
            .collect();
        let vel = vec![velocity; poses.len()];
        let mut r = Ragdoll::new(&poses, &vel);
        r.fit(&self.template);
        r
    }

    /// The joints' local transforms: bones with a body where it lies, the rest as `locals`
    /// (the pose when it went limp) under their parents; `root_parent` is the world transform
    /// the root bone hangs from.
    pub fn locals(&self, r: &Ragdoll, root_parent: Affine3A, parents: &[Option<usize>], locals: &[Transform]) -> Vec<Transform> {
        let poses: Vec<(Vec3, Quat)> = r.poses().collect();
        let n = parents.len();
        let mut world = vec![Affine3A::IDENTITY; n];
        let mut out = locals.to_vec();
        for &k in &self.order {
            let up = parents[k].map_or(root_parent, |p| world[p]);
            let body = self.driven[k].and_then(|i| Some((i, self.bodies[i]?.1)));
            world[k] = match body {
                Some((i, off)) => {
                    let (x, q) = poses[i];
                    let w = Affine3A::from_rotation_translation(q, x) * off.inverse();
                    let (s, r, t) = (up.inverse() * w).to_scale_rotation_translation();
                    out[k] = Transform { translation: t, rotation: r.normalize(), scale: s };
                    w
                }
                None => up * locals[k].compute_affine(),
            };
        }
        out
    }
}

/// Joint transforms that replace (`weight` 1) or blend over the animation's: on every bone, or
/// only on those listed in `only`.
#[derive(Component, Clone)]
pub struct PoseOverride {
    pub locals: Vec<Transform>,
    pub weight: f32,
    pub only: Vec<usize>,
}
