//! A character's body shape (port_map `character`): the character editor's Body and Face sizes
//! (the race's "editor limits", e.g. editor_data_human.xml) put on the skeleton's bones as
//! Kenshi's Ogre does, by each bone's own size and its "positional size".
//!
//! Specs: F037414 (the sizes from the editor values, the character's muscle and fat, and its
//! lost limbs: a lost limb's bones sized to nothing), OgreMain OldBone::setBoneSize /
//! setBonePositionalSize (fields +0x168 / +0x174) and OldBone::updateFromParentImpl (a bone's
//! derived scale is its own size times its local scale, not its parent's; its derived position
//! is its parent's plus the parent's orientation applied to its local position times its
//! positional size times the parent's derived Y scale).
//! (assumption) The race's size factor F037414 multiplies Height and Frame by (a virtual call on
//! the character) is 1; the skinny-leg checks (+0x4e0 / +0x4d8) and the leg difference (+0x190)
//! are off. (Posture, Shoulder set and Neck position are clips, kenshi_render::character::Posture.)

use crate::character::Animated;
use bevy::prelude::*;
use std::collections::HashMap;

/// Each joint's size and positional size (rig order); a body with these has its joints placed
/// by Kenshi's bone rule instead of plain inheritance.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct BoneSizes {
    pub size: Vec<Vec3>,
    pub pos: Vec<Vec3>,
}

fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * t
}

/// @fn F037414
///
/// The bone sizes for a body of rig bones `names` from the editor's values (raw, as the sliders
/// show them: "Height" 100 is normal), its sex, muscle and fat (0 .. 1, F050479's lerp weights)
/// and lost limbs (left arm, right arm, left leg, right leg). None unless the rig is a 30-bone
/// (human-shaped) skeleton, as the game.
pub fn bone_sizes(names: &[String], values: &HashMap<String, f32>, female: bool, muscle: f32, fat: f32, lost: [bool; 4]) -> Option<BoneSizes> {
    if names.len() != 30 {
        return None;
    }
    let v = |k: &str| values.get(k).copied().unwrap_or(100.0) * 0.01;
    let raw = |k: &str| values.get(k).copied().unwrap_or(100.0);
    let mut size = vec![Vec3::ONE; names.len()];
    let mut pos = vec![Vec3::ONE; names.len()];
    let bone = |n: &str| names.iter().position(|b| b == n);
    let mut set = |n: &str, s: Vec3| {
        if let Some(i) = bone(n) {
            size[i] = s;
        }
    };
    // the fat and muscle factors
    let fat_a = lerp(fat, 1.0, 0.7);
    let fat_b = lerp(fat, 1.0, 0.86);
    let fat_c = lerp(fat, 1.0, 0.6);
    let fat_d = lerp(fat, 1.0, 0.4);
    let mus_a = lerp(muscle, 1.0, if female { 1.24 } else { 1.27 });
    let mus_b = lerp(muscle, 1.0, if female { 1.12 } else { 1.13 });
    let h = v("Height");
    let h2 = 2.0 - h;
    let ll = v("Leg length");
    let f = v("Frame");
    let leg_y = h + ll - 1.0;
    let legs_bulk = v("Legs bulk") * fat_a * mus_a;
    let legs_shape = v("Legs shape");
    let hips = v("Hips");
    // a lost limb's bones sized to nothing (F040471's slot states)
    let k = lost.map(|x| if x { 0.0 } else { 1.0 });
    let (l_arm, r_arm, l_leg, r_leg) = (k[0], k[1], k[2], k[3]);
    // thighs
    let t = (legs_shape * legs_bulk + (hips - 1.0) / 3.0) * f;
    let thigh = Vec3::new(t, leg_y, t) * Vec3::new(1.0, 0.95, 1.0);
    set("Bip01 R Thigh", thigh);
    set("Bip01 L Thigh", thigh);
    let thigh_pos = Vec3::new(1.0, f * h2 * hips, 1.0);
    for n in ["Bip01 R Thigh", "Bip01 L Thigh"] {
        if let Some(i) = bone(n) {
            pos[i] = thigh_pos;
        }
    }
    // calves
    let c = (2.0 - legs_shape) * legs_bulk * f;
    let calf = Vec3::new(c, leg_y, c);
    set("Bip01 L Calf", calf * l_leg);
    set("Bip01 R Calf", calf * r_leg);
    // the trunk
    let chest = v("Chest") * mus_b * fat_b;
    let waist = v("Waist") * fat_a;
    let stomach = v("Stomach") * fat_c;
    let hips_spine = (hips - 1.0) * 0.6 + 1.0;
    set("Bip01 Pelvis", Vec3::new(hips * f, h, hips * f));
    set("Bip01 Spine", Vec3::new(hips_spine * f, h, hips_spine * stomach * f));
    set("Bip01 Spine1", Vec3::new(waist * f, h, stomach * f));
    let chest_w = (chest - 1.0) * 0.45 + 1.0;
    let chest_d = (chest - 1.0) * 0.9 + 1.0;
    set("Bip01 Spine2", Vec3::new(chest_w * f, h, chest_d * f));
    // breasts (a rig with them)
    if bone("L Boob").is_some() {
        let bs = v("Breast size") * fat_d;
        let (w, y) = if bs < 1.0 { ((bs - 1.0) * 0.75 + 1.0, (bs - 1.0) * 0.5 + 1.0) } else { (bs, bs) };
        let height = if female { v("Breast height") } else { 1.0 };
        let spacing = if female { v("Breast spacing") } else { 1.0 };
        let s = Vec3::new(w * f, y * h, bs * f);
        set("L Boob", s);
        set("R Boob", s);
        let p = Vec3::new(height, spacing * chest_d * f * ((h2 - 1.0) * 0.5 + 1.0), h2 * chest_d * f);
        for n in ["L Boob", "R Boob"] {
            if let Some(i) = bone(n) {
                pos[i] = p;
            }
        }
    }
    // arms
    let arm = v("Arm bulk") * fat_a * mus_a;
    let arm_depth = (arm - 1.0) * 1.5;
    let shoulders = v("Shoulders") * mus_b;
    let clav = (shoulders - 1.0) * 0.3;
    let arm_f = arm * f;
    let upper = Vec3::new(arm_f, h, (arm_depth + 1.0) * f);
    // (assumption) the upper arms stay for a lost arm: the armour's "hide stump" bits are not read
    set("Bip01 L UpperArm", upper);
    set("Bip01 R UpperArm", upper);
    let fore = Vec3::new(arm_f, h, arm_f);
    set("Bip01 L Forearm", fore * l_arm);
    set("Bip01 R Forearm", fore * r_arm);
    let clavicle = Vec3::new(shoulders * f, (clav + 1.0) * f, shoulders * f);
    set("Bip01 L Clavicle", clavicle);
    set("Bip01 R Clavicle", clavicle);
    for n in ["Bip01 L UpperArm", "Bip01 R UpperArm"] {
        if let Some(i) = bone(n) {
            pos[i] = Vec3::new(shoulders * chest_w, 1.0, 1.0);
        }
    }
    let hand = Vec3::new(arm_f, h, arm_f) * v("Hands");
    set("Bip01 L Hand", hand * l_arm);
    set("Bip01 R Hand", hand * r_arm);
    // feet
    let feet = v("Feet") * h;
    let foot = Vec3::new(feet, ll, feet);
    set("Bip01 L Foot", foot * l_leg);
    set("Bip01 R Foot", foot * r_leg);
    set("Bip01 L Toe0", Vec3::splat(feet) * l_leg);
    set("Bip01 R Toe0", Vec3::splat(feet) * r_leg);
    for (n, k) in [("Bip01 L Toe0", l_leg), ("Bip01 R Toe0", r_leg)] {
        if let Some(i) = bone(n) {
            pos[i] = Vec3::new(1.0, 1.0, feet) * k;
        }
    }
    // neck and head
    let head = v("Head size");
    let head_shape = raw("Head shape");
    let neck = v("Neck") * fat_a;
    let neck_w = v("Neck width") * fat_c * mus_b;
    set("Bip01 Neck", Vec3::new(neck_w * f, v("Neck length"), neck * f));
    let frame_head = (f - 1.0) * 0.25 + 1.0;
    let head_size = Vec3::new(frame_head * head * head_shape * 0.01, head, frame_head * head);
    set("Bip01 Head", head_size);
    if bone("Bip01 Jaw").is_some() {
        set("Bip01 Jaw", Vec3::new(v("Jaw"), 1.0, 1.0) * head_size);
    }
    Some(BoneSizes { size, pos })
}

/// OldBone::updateFromParentImpl: the joints of bodies with bone sizes placed again after the
/// transforms are propagated (each joint's derived scale its own, its offset scaled by its
/// positional size and its parent's derived Y scale), and what hangs on them (worn items,
/// weapons) moved with them.
pub fn place_sized_bones(
    bodies: Query<(Entity, &Animated, &BoneSizes)>,
    locals: Query<&Transform>,
    mut globals: Query<&mut GlobalTransform>,
    children: Query<&Children>,
) {
    for (root, anim, sizes) in &bodies {
        let Ok(root_g) = globals.get(root).map(|g| g.affine()) else { continue };
        let n = anim.joints.len().min(sizes.size.len()).min(anim.rig.parents.len());
        // skeleton space: (position, orientation, scale) per joint, parents first
        let mut derived: Vec<Option<(Vec3, Quat, Vec3)>> = vec![None; n];
        let mut left = n;
        let mut guard = 0;
        while left > 0 && guard < n + 1 {
            guard += 1;
            for i in 0..n {
                if derived[i].is_some() {
                    continue;
                }
                let parent = match anim.rig.parents[i] {
                    Some(p) if p < n => match derived[p] {
                        Some(d) => d,
                        None => continue,
                    },
                    _ => (Vec3::ZERO, Quat::IDENTITY, Vec3::ONE),
                };
                let Ok(t) = locals.get(anim.joints[i]) else {
                    derived[i] = Some(parent);
                    left -= 1;
                    continue;
                };
                let rot = parent.1 * t.rotation;
                let scale = sizes.size[i] * t.scale;
                let at = parent.0 + parent.1 * (t.translation * sizes.pos[i] * parent.2.y);
                derived[i] = Some((at, rot, scale));
                left -= 1;
            }
        }
        let joints: std::collections::HashSet<Entity> = anim.joints.iter().copied().collect();
        for i in 0..n {
            let Some((at, rot, scale)) = derived[i] else { continue };
            let g = GlobalTransform::from(root_g * bevy::math::Affine3A::from_scale_rotation_translation(scale, rot, at));
            if let Ok(mut gt) = globals.get_mut(anim.joints[i]) {
                *gt = g;
            }
            // items hung on the joint follow it (their own scale kept)
            for c in children.get(anim.joints[i]).into_iter().flat_map(|c| c.iter()) {
                if joints.contains(&c) {
                    continue;
                }
                let Ok(local) = locals.get(c) else { continue };
                let unscaled = GlobalTransform::from(root_g * bevy::math::Affine3A::from_rotation_translation(rot, at));
                if let Ok(mut gt) = globals.get_mut(c) {
                    *gt = unscaled.mul_transform(*local);
                }
            }
        }
    }
}

/// The editor's ranges by name as values (the range's middle where the character has none).
pub fn with_defaults(values: &HashMap<String, f32>, mids: &[(String, f32)]) -> HashMap<String, f32> {
    let mut out: HashMap<String, f32> = mids.iter().cloned().collect();
    out.extend(values.iter().map(|(k, v)| (k.clone(), *v)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig() -> Vec<String> {
        let mut v: Vec<String> = ["Bip01 Pelvis", "Bip01 Spine", "Bip01 Spine1", "Bip01 Spine2", "Bip01 L Calf", "Bip01 L Hand"].iter().map(|s| s.to_string()).collect();
        while v.len() < 30 {
            v.push(format!("x{}", v.len()));
        }
        v
    }

    #[test]
    fn default_body_is_unit() {
        let s = bone_sizes(&rig(), &HashMap::new(), false, 0.0, 0.0, [false; 4]).unwrap();
        assert!(s.size.iter().all(|v| (*v - Vec3::ONE).abs().max_element() < 1e-5), "{:?}", s.size);
    }

    #[test]
    fn height_and_lost_limbs() {
        let vals = HashMap::from([("Height".to_string(), 110.0)]);
        let s = bone_sizes(&rig(), &vals, false, 0.0, 0.0, [false, false, true, false]).unwrap();
        assert!((s.size[0].y - 1.1).abs() < 1e-5);
        assert_eq!(s.size[4], Vec3::ZERO);
        assert!(s.size[5].x > 0.0);
    }
}

#[cfg(test)]
mod arm_test {
    use super::*;

    #[test]
    fn arm_bulk_widens_upper_arm() {
        let mut names: Vec<String> = vec!["Bip01 L UpperArm".to_string()];
        while names.len() < 30 {
            names.push(format!("x{}", names.len()));
        }
        let vals = HashMap::from([("Height".to_string(), 120.0), ("Arm bulk".to_string(), 145.0)]);
        let s = bone_sizes(&names, &vals, false, 0.01, 0.0, [false; 4]).unwrap();
        println!("{:?}", s.size[0]);
        assert!(s.size[0].x > 1.4 && (s.size[0].y - 1.2).abs() < 1e-4);
    }
}
