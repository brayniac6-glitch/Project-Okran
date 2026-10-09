//! Skeletal animation (port_map `skeletal_animation`): bind poses and keyframe sampling for Ogre
//! skeletons. Spec: sheet row T00020 SkeletalPose. Playing clips (progress, events): [`clip`].

pub mod clip;
pub mod layer;
pub mod select;

use glam::{Mat4, Quat, Vec3};
use kenshi_io::ogre_skeleton::{Animation, Skeleton, Track};

/// A bone's local transform (relative to its parent).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Local {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Local {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

/// Ogre stores quaternions as x, y, z, w.
fn quat(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize()
}

/// Bones in a fixed order (the skeleton's file order) with parent indices into that order.
#[derive(Debug, Clone)]
pub struct Rig {
    pub names: Vec<String>,
    pub handles: Vec<u16>,
    pub parents: Vec<Option<usize>>,
    pub bind: Vec<Local>,
}

impl Rig {
    pub fn new(s: &Skeleton) -> Self {
        let handles: Vec<u16> = s.bones.iter().map(|b| b.handle).collect();
        let index = |h: u16| handles.iter().position(|&x| x == h);
        Rig {
            names: s.bones.iter().map(|b| b.name.clone()).collect(),
            parents: s.bones.iter().map(|b| b.parent.and_then(index)).collect(),
            bind: s
                .bones
                .iter()
                .map(|b| Local {
                    translation: Vec3::from(b.position),
                    rotation: quat(b.orientation),
                    scale: b.scale.map_or(Vec3::ONE, Vec3::from),
                })
                .collect(),
            handles,
        }
    }

    pub fn index_of_handle(&self, handle: u16) -> Option<usize> {
        self.handles.iter().position(|&h| h == handle)
    }

    /// Model-space matrices for local transforms (parents come before children in Ogre files,
    /// but this does not rely on it).
    pub fn model_space(&self, locals: &[Local]) -> Vec<Mat4> {
        let mut out: Vec<Option<Mat4>> = vec![None; locals.len()];
        fn solve(i: usize, rig: &Rig, locals: &[Local], out: &mut Vec<Option<Mat4>>) -> Mat4 {
            if let Some(m) = out[i] {
                return m;
            }
            let m = match rig.parents[i] {
                Some(p) => solve(p, rig, locals, out) * locals[i].matrix(),
                None => locals[i].matrix(),
            };
            out[i] = Some(m);
            m
        }
        (0..locals.len())
            .map(|i| solve(i, self, locals, &mut out))
            .collect()
    }

    /// Inverse bind matrices, for skinning.
    pub fn inverse_bind(&self) -> Vec<Mat4> {
        self.model_space(&self.bind)
            .into_iter()
            .map(|m| m.inverse())
            .collect()
    }

    /// Bones at and below `name` (e.g. "Bip01 Spine1" for the upper body).
    pub fn subtree(&self, name: &str) -> Vec<bool> {
        let Some(top) = self.names.iter().position(|n| n == name) else {
            return vec![false; self.names.len()];
        };
        (0..self.names.len())
            .map(|mut i| {
                loop {
                    if i == top {
                        break true;
                    }
                    match self.parents[i] {
                        Some(p) => i = p,
                        None => break false,
                    }
                }
            })
            .collect()
    }

    /// Two-layer pose, as Kenshi's "<x> lower" / "<x> upper" animation pairs: bones in `upper_mask`
    /// take `upper`, the rest take `lower`.
    pub fn sample_layered(
        &self,
        lower: &Animation,
        upper: &Animation,
        upper_mask: &[bool],
        time: f32,
    ) -> Vec<Local> {
        let mut pose = self.sample(lower, time);
        let top = self.sample(upper, time);
        for (i, up) in upper_mask.iter().enumerate() {
            if *up {
                pose[i] = top[i];
            }
        }
        pose
    }

    /// Local transforms of every bone at `time` (seconds, wrapped into the animation length).
    /// Bones without a track keep their bind pose.
    pub fn sample(&self, anim: &Animation, time: f32) -> Vec<Local> {
        let t = if anim.length > 0.0 {
            time.rem_euclid(anim.length)
        } else {
            0.0
        };
        let mut pose = self.bind.clone();
        for track in &anim.tracks {
            let Some(i) = self.index_of_handle(track.bone) else {
                continue;
            };
            let Some((dt, dr, ds)) = sample_track(track, t) else {
                continue;
            };
            let b = self.bind[i];
            pose[i] = Local {
                translation: b.translation + dt,
                rotation: (b.rotation * dr).normalize(),
                scale: b.scale * ds,
            };
        }
        pose
    }
}

impl Rig {
    /// Blend several clips as Ogre's average blend mode does (the engine Kenshi used): every
    /// clip's keyframe offsets from the bind pose apply scaled by its weight (rotation slerped from
    /// identity), weights scaled down to sum to 1 when they exceed it. `time` is in seconds and is
    /// clamped to the clip, not wrapped (the clip clock already wraps looping clips).
    pub fn sample_weighted(&self, clips: &[(&Animation, f32, f32)]) -> Vec<Local> {
        self.sample_with_overrides(clips, &|_| &[])
    }

    /// As [`Rig::sample_weighted`], where `overrides(clip)` lists the bone indices that clip
    /// takes over (Kenshi's Animation::setOverrideBone, F046683): on such a bone only the clips
    /// overriding it count.
    pub fn sample_with_overrides<'o>(
        &self,
        clips: &[(&Animation, f32, f32)],
        overrides: &dyn Fn(usize) -> &'o [usize],
    ) -> Vec<Local> {
        let mut taken = vec![false; self.bind.len()];
        for (k, &(_, _, w)) in clips.iter().enumerate() {
            if 0.0 < w {
                for &b in overrides(k) {
                    if let Some(t) = taken.get_mut(b) {
                        *t = true;
                    }
                }
            }
        }
        let counts = |k: usize, i: usize| !taken[i] || overrides(k).contains(&i);
        // each bone's weights are normalised over the clips that animate it (a clip whose track
        // for the bone was stripped leaves it to the others); with every clip animating every
        // bone this is Ogre's average blend (normalised when the total passes 1)
        let mut totals = vec![0.0f32; self.bind.len()];
        for (k, &(anim, _, w)) in clips.iter().enumerate() {
            if w <= 0.0 {
                continue;
            }
            for track in &anim.tracks {
                if let Some(i) = self.index_of_handle(track.bone)
                    && counts(k, i)
                {
                    totals[i] += w;
                }
            }
        }
        let mut pose = self.bind.clone();
        for (k, &(anim, time, w)) in clips.iter().enumerate() {
            if w <= 0.0 {
                continue;
            }
            let t = time.clamp(0.0, anim.length.max(0.0));
            for track in &anim.tracks {
                let Some(i) = self.index_of_handle(track.bone) else {
                    continue;
                };
                if !counts(k, i) {
                    continue;
                }
                let w = if 1.0 < totals[i] { w / totals[i] } else { w };
                let Some((dt, dr, ds)) = sample_track(track, t) else {
                    continue;
                };
                let p = &mut pose[i];
                p.translation += dt * w;
                p.rotation = (p.rotation * Quat::IDENTITY.slerp(dr, w)).normalize();
                p.scale *= Vec3::ONE.lerp(ds, w);
            }
        }
        pose
    }
}

/// (translate, rotation, scale) offsets of a track at time `t`.
fn sample_track(track: &Track, t: f32) -> Option<(Vec3, Quat, Vec3)> {
    let keys = &track.keyframes;
    let key = |k: &kenshi_io::ogre_skeleton::Keyframe| {
        (
            Vec3::from(k.translate),
            quat(k.rotation),
            k.scale.map_or(Vec3::ONE, Vec3::from),
        )
    };
    let first = keys.first()?;
    if keys.len() == 1 || t <= first.time {
        return Some(key(first));
    }
    let next = keys
        .iter()
        .position(|k| k.time >= t)
        .unwrap_or(keys.len() - 1);
    let (a, b) = (&keys[next.saturating_sub(1)], &keys[next]);
    let span = b.time - a.time;
    let f = if span > 0.0 {
        ((t - a.time) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (ta, ra, sa) = key(a);
    let (tb, rb, sb) = key(b);
    Some((ta.lerp(tb, f), ra.slerp(rb, f), sa.lerp(sb, f)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kenshi_io::ogre_skeleton::{Bone, Keyframe};

    fn bone(name: &str, handle: u16, parent: Option<u16>, pos: [f32; 3]) -> Bone {
        Bone {
            name: name.into(),
            handle,
            position: pos,
            orientation: [0.0, 0.0, 0.0, 1.0],
            scale: None,
            parent,
        }
    }

    fn skel() -> Skeleton {
        Skeleton {
            bones: vec![
                bone("root", 0, None, [0.0, 1.0, 0.0]),
                bone("arm", 1, Some(0), [1.0, 0.0, 0.0]),
            ],
            animations: vec![Animation {
                name: "move".into(),
                length: 2.0,
                base: None,
                tracks: vec![Track {
                    bone: 1,
                    keyframes: vec![
                        Keyframe {
                            time: 0.0,
                            rotation: [0.0, 0.0, 0.0, 1.0],
                            translate: [0.0; 3],
                            scale: None,
                        },
                        Keyframe {
                            time: 2.0,
                            rotation: [0.0, 0.0, 0.0, 1.0],
                            translate: [0.0, 2.0, 0.0],
                            scale: None,
                        },
                    ],
                }],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn bind_pose_is_hierarchical() {
        let rig = Rig::new(&skel());
        let m = rig.model_space(&rig.bind);
        assert_eq!(m[1].transform_point3(Vec3::ZERO), Vec3::new(1.0, 1.0, 0.0));
        let inv = rig.inverse_bind();
        assert!((inv[1] * m[1]).abs_diff_eq(Mat4::IDENTITY, 1e-6));
    }

    #[test]
    fn keyframes_offset_the_bind_pose_and_interpolate() {
        let s = skel();
        let rig = Rig::new(&s);
        let pose = rig.sample(&s.animations[0], 1.0); // halfway
        assert_eq!(pose[1].translation, Vec3::new(1.0, 1.0, 0.0));
        assert_eq!(pose[0], rig.bind[0]); // untracked bone keeps bind pose
        let wrapped = rig.sample(&s.animations[0], 3.0);
        assert_eq!(wrapped[1].translation, pose[1].translation);
    }

    #[test]
    fn weighted_blend_scales_offsets() {
        let s = skel();
        let rig = Rig::new(&s);
        let a = &s.animations[0];
        // half weight at the end of the clip: half of the +2 offset
        let half = rig.sample_weighted(&[(a, 2.0, 0.5)]);
        assert!((half[1].translation - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-5);
        // weights above 1 are normalised; time is clamped, not wrapped
        let both = rig.sample_weighted(&[(a, 9.0, 1.0), (a, 0.0, 1.0)]);
        assert!((both[1].translation - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn layers_split_at_a_subtree() {
        let s = skel();
        let rig = Rig::new(&s);
        assert_eq!(rig.subtree("arm"), [false, true]);
        assert_eq!(rig.subtree("root"), [true, true]);
        let still = Animation {
            name: "still".into(),
            length: 1.0,
            base: None,
            tracks: vec![],
        };
        // lower = still (bind pose), upper = the moving animation on the arm only
        let pose = rig.sample_layered(&still, &s.animations[0], &rig.subtree("arm"), 1.0);
        assert_eq!(pose[0], rig.bind[0]);
        assert_eq!(pose[1].translation, Vec3::new(1.0, 1.0, 0.0));
    }
}
