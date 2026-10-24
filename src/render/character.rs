//! Characters as Bevy entities (port_map `character_appearance`, `skeletal_animation`): a race's
//! skinned body with its head texture, hair/beard attachments and worn clothing, all on one
//! skeleton, animated by sampling Ogre keyframes.

use crate::{converted_submeshes, hair_image, split_head_tile, to_bevy_skinned, with_poses};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use kenshi_anim::Rig;
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_io::{dds, ogre_mesh, ogre_skeleton, resources::Resources};
use std::path::Path;
use std::sync::Arc;

/// What to build: race record name, head record name (default: the race's first male head), hair
/// and beard attachment names, worn ARMOUR names, and the hair tint.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CharacterSpec {
    pub race: String,
    pub head: Option<String>,
    pub hairs: Vec<String>,
    pub wear: Vec<String>,
    pub hair_colour: [f32; 3],
    /// The COLOR_DATA its clothes are painted with (F037438: the CHARACTER's "color", F037403),
    /// unless a piece has its own.
    pub colour: Option<String>,
    /// The female body (the race's "female mesh", heads, skin and ragdoll; clothes' and hairs'
    /// "mesh female").
    pub female: bool,
}

impl CharacterSpec {
    pub fn new(race: &str) -> Self {
        CharacterSpec {
            race: race.into(),
            head: None,
            hairs: Vec::new(),
            wear: Vec::new(),
            hair_colour: [0.24, 0.16, 0.1],
            colour: None,
            female: false,
        }
    }
}

/// A hair or beard attachment's map on the head (F037420: hairMap / beardMap, the attachment's
/// "head texture"; hairMult = the unit vector of its "head channel", hairAlpha / beardAlpha that
/// of its "head alpha channel"; hairColor the hair colour).
#[derive(Debug, Clone, PartialEq)]
pub struct Scalp {
    pub map: String,
    pub mult: [f32; 4],
    pub alpha: [f32; 4],
    /// A beard ("attach slot" 13) rather than hair.
    pub beard: bool,
    pub colour: [f32; 3],
}

/// A hair or beard attachment's cards as character.hlsl hair_fs takes them (F037428: the
/// "texture map"; diffuseChannel / alphaChannel the unit vectors of "hair diffuse channel" /
/// "hair alpha channel"; threshold "alpha rejection" / 255; specularMult "specular"; color the
/// hair colour).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HairCard {
    pub texture: Option<String>,
    pub diffuse: [f32; 4],
    pub alpha: [f32; 4],
    pub threshold: f32,
    pub specular: f32,
    pub colour: [f32; 3],
}

/// How one mesh of a character is drawn.
#[derive(Clone)]
pub enum Surface {
    /// Body skin or head: texture and normal map paths.
    Skin(Option<String>, Option<String>),
    /// The head: texture and normal map paths, and the hair and beard maps drawn on it.
    Head(Option<String>, Option<String>, Vec<Scalp>),
    /// Hair/beard cards: CPU-tinted image, alpha-masked, double-sided; and the card as Kenshi's
    /// hair shader takes it.
    Hair(Option<Image>, HairCard),
    /// Clothing: texture and normal map paths from its material spec, and its paint.
    Cloth(Option<String>, Option<String>, Option<ClothPaint>),
    /// An animal's body (Kenshi's "Creature" material): texture and normal map paths, wrapping.
    Creature(Option<String>, Option<String>),
    /// Body skin with painted-on clothing (a shirt's "vest texture"): the skin's texture and
    /// normal map paths, then the vest's, laid over the skin in the shader.
    Vested(Option<String>, Option<String>, String, String),
}

/// A piece of clothing's paint (F037442, skin.hlsl COLOURING): its material spec's "color map"
/// (red and green: where colours 1 and 2 go, blue: metalness) and the COLOR_DATA's "color 1" and
/// "color 2" (0xAARRGGBB) as colours, each's alpha the item's "paint factor 1" / "2" (0 when it
/// has none: the colour takes the texture's intensity).
#[derive(Debug, Clone, PartialEq)]
pub struct ClothPaint {
    pub color_map: String,
    pub colours: [[f32; 4]; 2],
}

/// A loaded character: its rig, animations and skinned meshes (joint indices into `rig` order).
pub struct CharacterAsset {
    pub rig: Rig,
    pub animations: Vec<ogre_skeleton::Animation>,
    /// Shared copies for `Animated`.
    pub library: Arc<Vec<ogre_skeleton::Animation>>,
    pub upper_mask: Arc<Vec<bool>>,
    pub parts: Vec<(Mesh, Surface)>,
    /// Per part: the key it is shared by between looks (the race's body and head, the same mesh
    /// whatever is worn), none for a part of this look only.
    pub share: Vec<Option<String>>,
    /// Per animation (library order): the joints it takes over (F046683 override bones).
    pub overrides: Arc<Vec<Vec<usize>>>,
    /// The race's ragdoll ("male ragdoll", F039259) and the one it hangs from a carrier by
    /// ("male carry ragdoll").
    pub ragdoll: Option<Arc<kenshi_io::phs::Phs>>,
    pub carry_ragdoll: Option<Arc<kenshi_io::phs::Phs>>,
}

/// An ARMOUR record (or a worn CONTAINER: a backpack, its "mesh" skinned as clothing is) by
/// string id or name (a spec's "wear").
fn armour_named<'a>(data: &'a GameDataContainer, name: &str) -> Option<&'a GameData> {
    let worn = |r: &&GameData| matches!(r.item_type(), Some(ItemType::Armour | ItemType::Container));
    data.get(name).filter(worn).or_else(|| data.records.values().filter(worn).find(|r| r.name == name))
}

fn file_of(res: &Resources, r: &GameData, key: &str) -> Option<String> {
    r.files
        .get(key)
        .filter(|p| !p.is_empty())
        .and_then(|p| res.find(p))
}

/// Assemble a character from game data.
pub fn load_character(
    root: &Path,
    data: &GameDataContainer,
    res: &Resources,
    spec: &CharacterSpec,
) -> Result<CharacterAsset, String> {
    load_character_shared(root, data, res, spec, None)
}

/// [`load_character`], the skeleton, its animations and the ragdolls taken from `shared` (a
/// character of the same race and sex) instead of read again: a crowd of looks shares one copy.
/// The asset's own `animations` are then empty; its `library` is the shared one.
pub fn load_character_shared(
    root: &Path,
    data: &GameDataContainer,
    res: &Resources,
    spec: &CharacterSpec,
    shared: Option<&CharacterAsset>,
) -> Result<CharacterAsset, String> {
    let race = data
        .records
        .values()
        .find(|r| r.name == spec.race && r.item_type() == Some(ItemType::Race))
        .ok_or_else(|| format!("no race '{}'", spec.race))?;
    let g = if spec.female { "female" } else { "male" };
    let body_path = file_of(res, race, &format!("{g} mesh"))
        .or_else(|| file_of(res, race, "male mesh"))
        .ok_or("race has no body mesh")?;
    let body = ogre_mesh::read(&root.join(&body_path)).map_err(|e| e.to_string())?;
    let skeleton = match shared {
        Some(_) => None,
        None => {
            let skel_name = body.skeleton.as_ref().ok_or("body mesh has no skeleton")?;
            let skel_path = res
                .path(skel_name)
                .ok_or_else(|| format!("skeleton {skel_name} not found"))?;
            Some(ogre_skeleton::read(&skel_path).map_err(|e| e.to_string())?)
        }
    };
    let rig = match (&skeleton, shared) {
        (Some(sk), _) => Rig::new(sk),
        (None, Some(sh)) => sh.rig.clone(),
        (None, None) => unreachable!(),
    };
    let head_joint = rig
        .names
        .iter()
        .position(|n| n == "Bip01 Head")
        .unwrap_or(0) as u16;
    // Ogre bone handle -> joint index; bones the body lacks map to `fallback`
    let rig_ref = &rig;
    let joint_or = |fallback: u16| {
        move |b: u16| Some(rig_ref.index_of_handle(b).map_or(fallback, |i| i as u16))
    };

    let mut parts = Vec::new();
    // body, with the head UV tile split off and textured from the HEAD record
    let heads: Vec<&GameData> = race
        .refs(if spec.female { "heads female" } else { "heads male" })
        .iter()
        .filter_map(|(sid, _)| data.get(sid))
        .collect();
    let head = heads
        .iter()
        .find(|h| spec.head.as_deref() == Some(h.name.as_str()))
        .or(heads.first());
    // painted-on clothing ("vest" textures: shirts, the only slot that has them): the character
    // shader lays it over the skin by the alpha of its normal map (body = lerp(body, vest, vest
    // normal alpha)), as Kenshi's does
    let vest = spec.wear.iter().filter_map(|name| armour_named(data, name)).find_map(|item| Some((file_of(res, item, "vest texture")?, file_of(res, item, "vest normalmap")?)));
    let body_tex = file_of(res, race, &format!("body texture {g}")).or_else(|| file_of(res, race, "body texture male"));
    if std::env::var("KENSHI_ANIMAL_DEBUG").is_ok() {
        eprintln!("race {} files {:?} -> body {body_tex:?}", race.name, race.files);
    }
    let nm = file_of(res, race, &format!("nm {g}")).or_else(|| file_of(res, race, "nm male"));
    let skin = match vest {
        Some((vd, vn)) => Surface::Vested(body_tex, nm, vd, vn),
        // (assumption) a race whose mesh is under data/animal is drawn with the Creature material
        None if race.files.get("male mesh").is_some_and(|m| m.to_lowercase().replace('\\', "/").contains("data/animal/")) => Surface::Creature(body_tex, nm),
        None => Surface::Skin(body_tex, nm),
    };
    // the hair and beard maps on the head (F037420)
    let unit = |k: i32| {
        let mut v = [0.0f32; 4];
        if (0..4).contains(&k) {
            v[k as usize] = 1.0;
        }
        v
    };
    let scalps: Vec<Scalp> = spec
        .hairs
        .iter()
        .filter_map(|name| {
            race.refs("hairs")
                .iter()
                .filter_map(|(sid, _)| data.get(sid))
                .find(|h| &h.name == name)
        })
        .filter_map(|h| {
            let map = file_of(res, h, "head texture")?;
            let int = |k: &str| *h.ints.get(k).unwrap_or(&-1);
            Some(Scalp {
                map,
                mult: unit(int("head channel")),
                alpha: unit(int("head alpha channel")),
                beard: int("attach slot") == 13,
                colour: spec.hair_colour,
            })
        })
        .collect();
    // the body and its head (only the vertices each uses; the head with the mesh's face poses as
    // morph targets, F021065), shared by every look of the race and sex
    let order = converted_submeshes(&body);
    let mut share: Vec<Option<String>> = Vec::new();
    for (k, (_, m)) in to_bevy_skinned(&body, Some(&joint_or(0))).into_iter().enumerate() {
        let sub = order.get(k).copied().unwrap_or(k);
        match (head, split_head_tile(&m)) {
            (Some(h), Some((b, hm))) => {
                parts.push((with_poses(&b, &body, usize::MAX), skin.clone()));
                share.push(Some(format!("{body_path}#{k}#body")));
                parts.push((
                    with_poses(&hm, &body, sub),
                    Surface::Head(
                        file_of(res, h, "texture map"),
                        file_of(res, h, "normal map"),
                        scalps.clone(),
                    ),
                ));
                share.push(Some(format!("{body_path}#{k}#head")));
            }
            _ => {
                parts.push((m, skin.clone()));
                share.push(Some(format!("{body_path}#{k}")));
            }
        }
    }
    // hair and beards
    for name in &spec.hairs {
        let Some(h) = race
            .refs("hairs")
            .iter()
            .filter_map(|(sid, _)| data.get(sid))
            .find(|h| &h.name == name)
        else {
            continue;
        };
        let Some(path) = spec
            .female
            .then(|| h.files.get("mesh female"))
            .flatten()
            .filter(|p| !p.is_empty())
            .or_else(|| h.files.get("mesh"))
            .and_then(|p| res.path(p))
        else {
            continue;
        };
        let Ok(hm) = ogre_mesh::read(&path) else {
            continue;
        };
        let image = h
            .files
            .get("texture map")
            .and_then(|p| res.path(p))
            .and_then(|p| dds::read(&p).ok())
            .map(|t| hair_image(&t, spec.hair_colour));
        let unit = |k: &str| {
            let mut v = [0.0f32; 4];
            if let Some(&i) = h.ints.get(k)
                && (0..4).contains(&i)
            {
                v[i as usize] = 1.0;
            }
            v
        };
        let card = HairCard {
            texture: file_of(res, h, "texture map"),
            diffuse: unit("hair diffuse channel"),
            alpha: unit("hair alpha channel"),
            threshold: *h.ints.get("alpha rejection").unwrap_or(&0) as f32 / 255.0,
            specular: h.floats.get("specular").copied().unwrap_or(0.0),
            colour: spec.hair_colour,
        };
        for (_, m) in to_bevy_skinned(&hm, Some(&joint_or(head_joint))) {
            parts.push((m, Surface::Hair(image.clone(), card.clone())));
        }
    }
    // clothing
    for name in &spec.wear {
        let Some(item) = armour_named(data, name) else {
            continue;
        };
        let Some(path) = spec
            .female
            .then(|| item.files.get("mesh female"))
            .flatten()
            .filter(|p| !p.is_empty())
            .or_else(|| item.files.get("mesh"))
            .and_then(|p| res.path(p))
        else {
            continue;
        };
        let Ok(cm) = ogre_mesh::read(&path) else {
            continue;
        };
        let mat = crate::appearance_material(data, item);
        let tex = |k: &str| mat.and_then(|m| file_of(res, m, k));
        // F037438 / F037442: painted when it has a colour (its own "color", else the
        // wearer's) and its material a "color map"
        let colour = item.refs("color").first().map(|(c, _)| c.clone()).or_else(|| spec.colour.clone()).and_then(|c| data.get(&c));
        let paint = match (colour, tex("color map")) {
            (Some(c), Some(color_map)) => {
                let argb = |k: &str| {
                    let v = c.ints.get(k).copied().unwrap_or(-1) as u32;
                    [((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]
                };
                let factor = |k: &str| item.floats.get(k).copied().unwrap_or(0.0);
                let (c1, c2) = (argb("color 1"), argb("color 2"));
                Some(ClothPaint {
                    color_map,
                    colours: [[c1[0], c1[1], c1[2], factor("paint factor 1")], [c2[0], c2[1], c2[2], factor("paint factor 2")]],
                })
            }
            _ => None,
        };
        // (testing) KENSHI_PAINT_DEBUG: each piece's paint
        if std::env::var("KENSHI_PAINT_DEBUG").is_ok() {
            eprintln!("paint {}: colour {:?} map {:?} -> {:?}", item.name, colour.map(|c| &c.name), tex("color map"), paint.as_ref().map(|p| p.colours));
        }
        for (_, m) in to_bevy_skinned(&cm, Some(&joint_or(0))) {
            parts.push((m, Surface::Cloth(tex("texture map"), tex("normal map"), paint.clone())));
        }
    }
    share.resize(parts.len(), None);
    if let Some(sh) = shared {
        return Ok(CharacterAsset {
            rig,
            animations: Vec::new(),
            library: sh.library.clone(),
            upper_mask: sh.upper_mask.clone(),
            parts,
            share,
            overrides: sh.overrides.clone(),
            ragdoll: sh.ragdoll.clone(),
            carry_ragdoll: sh.carry_ragdoll.clone(),
        });
    }
    let skeleton = skeleton.expect("read above");
    let upper_mask = Arc::new(rig.subtree("Bip01 Spine1"));
    let library = Arc::new(skeleton.animations.clone());
    let overrides = Arc::new(vec![Vec::new(); skeleton.animations.len()]);
    let phs = |key: &str| {
        file_of(res, race, key)
            .and_then(|p| kenshi_io::phs::read(&root.join(&p)).map_err(|e| eprintln!("ragdoll {p}: {e}")).ok())
            .map(Arc::new)
    };
    let ragdoll = phs(&format!("{g} ragdoll")).or_else(|| phs("male ragdoll"));
    let carry_ragdoll = phs(&format!("{g} carry ragdoll")).or_else(|| ragdoll.clone());
    Ok(CharacterAsset {
        rig,
        animations: skeleton.animations,
        library,
        upper_mask,
        parts,
        share,
        overrides,
        ragdoll,
        carry_ragdoll,
    })
}

/// Plays an animation (or a lower/upper pair, like Kenshi's "<x> lower"/"<x> upper") on a
/// character's joints.
#[derive(Component)]
pub struct Animated {
    pub rig: Rig,
    /// The skeleton's animations.
    pub library: Arc<Vec<ogre_skeleton::Animation>>,
    /// Index of the playing (lower body, if layered) animation.
    pub current: usize,
    /// Upper-body animation index when layered.
    pub upper: Option<usize>,
    /// Bones driven by the upper layer ("Bip01 Spine1" and below).
    pub upper_mask: Arc<Vec<bool>>,
    /// Per animation: the joints it takes over.
    pub overrides: Arc<Vec<Vec<usize>>>,
    pub joints: Vec<Entity>,
    /// Seconds added to the clock, so characters are not in lockstep.
    pub offset: f32,
    /// Fixed time instead of playing (for screenshots).
    pub freeze_at: Option<f32>,
}

impl Animated {
    /// Index of the animation named `name` (case-insensitive), else the first whose name contains it.
    pub fn find(&self, name: &str) -> Option<usize> {
        let want = name.to_lowercase();
        let names = || self.library.iter().map(|a| a.name.to_lowercase());
        names()
            .position(|n| n == want)
            .or_else(|| names().position(|n| n.contains(&want)))
    }

    /// Play one animation on the whole body; no-op if it is missing.
    pub fn play(&mut self, name: &str) {
        if let Some(i) = self.find(name) {
            self.current = i;
            self.upper = None;
        }
    }

    /// Play "<base> lower" on the legs and "<base> upper" on the torso; falls back to `play(base)`.
    pub fn play_layered(&mut self, base: &str) {
        match (
            self.find(&format!("{base} lower")),
            self.find(&format!("{base} upper")),
        ) {
            (Some(l), Some(u)) => {
                self.current = l;
                self.upper = Some(u);
            }
            _ => self.play(base),
        }
    }
}

/// A fixed blend of several clips (library index, clip time, weight) that overrides `Animated`'s
/// own clip, e.g. a snapshot of animation layers (`kenshi_anim::layer`).
#[derive(Component, Clone)]
pub struct Blend(pub Vec<(usize, f32, f32)>);

/// Clips always applied at full weight over what the body plays, each held at a time (Kenshi's
/// "postures", "shoulder set" and "neck set", F037400: the character editor's Posture, Shoulder
/// set and Neck position as a point in the clip).
#[derive(Component, Clone, Debug, Default)]
pub struct Posture(pub Vec<(usize, f32)>);

/// Bevy handles shared by every character spawned from one asset.
pub struct CharacterHandles<M: Material = StandardMaterial> {
    meshes: Vec<(Handle<Mesh>, Handle<M>)>,
    bindposes: Handle<SkinnedMeshInverseBindposes>,
    /// Per mesh: the mesh with its morph targets (the head's face poses), if it has any.
    morphs: Vec<Option<Arc<Mesh>>>,
}

/// A mesh drawn with morph targets baked in (the head and its face poses): the mesh with them
/// (kept on the CPU) and the one drawn while none are set (shared); `weights` what is baked now.
#[derive(Component, Clone, Debug)]
pub struct MorphBase {
    pub base: Arc<Mesh>,
    pub plain: Handle<Mesh>,
    pub weights: Vec<f32>,
}

/// The meshes shared between characters (`CharacterAsset::share`) once uploaded (without morph
/// targets), and the mesh with them, for one that has some.
type SharedMesh = (Handle<Mesh>, Option<Arc<Mesh>>);
static SHARED_MESHES: std::sync::Mutex<Option<std::collections::HashMap<String, SharedMesh>>> = std::sync::Mutex::new(None);

/// Upload a character's meshes, each with the material `material` makes for its surface.
pub fn upload_character_with<M: Material>(
    asset: &CharacterAsset,
    meshes: &mut Assets<Mesh>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    mut material: impl FnMut(&Surface) -> Handle<M>,
) -> CharacterHandles<M> {
    let mut morphs = Vec::new();
    let parts = asset
        .parts
        .iter()
        .enumerate()
        .map(|(k, (m, surface))| {
            let key = asset.share.get(k).cloned().flatten();
            let cached = key.as_ref().and_then(|k| SHARED_MESHES.lock().ok()?.get_or_insert_with(Default::default).get(k).cloned());
            let (handle, names) = match cached {
                Some(c) => c,
                None => {
                    let mut m = m.clone();
                    if m.attribute(Mesh::ATTRIBUTE_TANGENT).is_none() {
                        let _ = m.generate_tangents();
                    }
                    // morph targets baked per character (`MorphBase`), not drawn by the GPU
                    let with = m.morph_target_names().is_some().then(|| Arc::new(m.clone()));
                    let made = (meshes.add(if with.is_some() { crate::without_morphs(&m) } else { m }), with);
                    if let Some(k) = key
                        && let Ok(mut c) = SHARED_MESHES.lock()
                    {
                        c.get_or_insert_with(Default::default).insert(k, made.clone());
                    }
                    made
                }
            };
            morphs.push(names);
            (handle, material(surface))
        })
        .collect();
    CharacterHandles {
        meshes: parts,
        bindposes: bindposes.add(SkinnedMeshInverseBindposes::from(asset.rig.inverse_bind())),
        morphs,
    }
}

/// Upload a character's meshes and materials once; spawn it many times with `spawn_character`.
pub fn upload_character(
    asset: &CharacterAsset,
    assets: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
) -> CharacterHandles {
    let parts = asset
        .parts
        .iter()
        .map(|(m, surface)| {
            let mat = match surface.clone() {
                Surface::Skin(t, n) | Surface::Cloth(t, n, _) | Surface::Creature(t, n) | Surface::Head(t, n, _) => StandardMaterial {
                    base_color_texture: t.map(|p| assets.load(p)),
                    normal_map_texture: n.map(|p| assets.load(p)),
                    perceptual_roughness: 0.85,
                    ..default()
                },
                Surface::Vested(t, n, _, _) => StandardMaterial {
                    base_color_texture: t.map(|p| assets.load(p)),
                    normal_map_texture: n.map(|p| assets.load(p)),
                    perceptual_roughness: 0.85,
                    ..default()
                },
                Surface::Hair(img, _) => StandardMaterial {
                    base_color_texture: img.map(|i| images.add(i)),
                    perceptual_roughness: 0.9,
                    alpha_mode: AlphaMode::Mask(0.4),
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                },
            };
            (meshes.add(crate::without_morphs(m)), materials.add(mat))
        })
        .collect();
    CharacterHandles {
        meshes: parts,
        bindposes: bindposes.add(SkinnedMeshInverseBindposes::from(asset.rig.inverse_bind())),
        morphs: asset.parts.iter().map(|(m, _)| m.morph_target_names().is_some().then(|| Arc::new(m.clone()))).collect(),
    }
}

/// Spawn one character at `at`, playing `animation` (see [`Animated::find`]).
pub fn spawn_character<M: Material>(
    commands: &mut Commands,
    asset: &CharacterAsset,
    handles: &CharacterHandles<M>,
    at: Transform,
    animation: &str,
    offset: f32,
    freeze_at: Option<f32>,
) -> Entity {
    spawn_character_with_joints(commands, asset, handles, at, animation, offset, freeze_at).0
}

/// As [`spawn_character`], also returning the joint entities (in rig order), e.g. to attach items
/// to a bone.
pub fn spawn_character_with_joints<M: Material>(
    commands: &mut Commands,
    asset: &CharacterAsset,
    handles: &CharacterHandles<M>,
    at: Transform,
    animation: &str,
    offset: f32,
    freeze_at: Option<f32>,
) -> (Entity, Vec<Entity>) {
    let root = commands.spawn((at, Visibility::default())).id();
    let rig = &asset.rig;
    let joints: Vec<Entity> = rig
        .bind
        .iter()
        .map(|b| {
            commands
                .spawn((
                    Transform {
                        translation: b.translation,
                        rotation: b.rotation,
                        scale: b.scale,
                    },
                    Visibility::default(),
                ))
                .id()
        })
        .collect();
    for (i, parent) in rig.parents.iter().enumerate() {
        let p = parent.map_or(root, |p| joints[p]);
        commands.entity(p).add_child(joints[i]);
    }
    let skin = SkinnedMesh {
        inverse_bindposes: handles.bindposes.clone(),
        joints: joints.clone(),
    };
    for (k, (mesh, mat)) in handles.meshes.iter().enumerate() {
        let e = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::default(),
                skin.clone(),
            ))
            .id();
        // a mesh with morph targets (the head's face poses): drawn plain until its weights are set
        if let Some(Some(base)) = handles.morphs.get(k) {
            let n = base.morph_target_names().map_or(0, |n| n.len());
            commands.entity(e).insert(MorphBase { base: base.clone(), plain: mesh.clone(), weights: vec![0.0; n] });
        }
        commands.entity(root).add_child(e);
    }
    if !asset.library.is_empty() {
        let mut a = Animated {
            rig: rig.clone(),
            library: asset.library.clone(),
            current: 0,
            upper: None,
            upper_mask: asset.upper_mask.clone(),
            overrides: asset.overrides.clone(),
            joints: joints.clone(),
            offset,
            freeze_at,
        };
        a.play(animation);
        commands.entity(root).insert(a);
    }
    (root, joints)
}

/// Poses every animated character's joints from its animation at the current time (or from its
/// [`Blend`]).
pub fn animate_characters(
    time: Res<Time>,
    characters: Query<(&Animated, Option<&Blend>, Option<&crate::ragdoll::PoseOverride>, Option<&Posture>)>,
    mut transforms: Query<&mut Transform>,
) {
    for (a, blend, over, posture) in &characters {
        let t = a.freeze_at.unwrap_or(time.elapsed_secs() + a.offset);
        let lower = &a.library[a.current];
        // the posture clips join whatever plays (a single clip becoming a blend of one)
        let single;
        let blend = match (blend, posture) {
            (None, Some(_)) if a.upper.is_none() => {
                single = Blend(vec![(a.current, t % a.library[a.current].length.max(1e-3), 1.0)]);
                Some(&single)
            }
            (b, _) => b,
        };
        let pose = match (blend, a.upper) {
            (Some(b), _) => {
                let mut list = b.0.clone();
                if let Some(p) = posture {
                    list.extend(p.0.iter().filter(|(i, _)| *i < a.library.len()).map(|&(i, t)| (i, t, 1.0)));
                }
                let clips: Vec<_> = list.iter().map(|&(i, t, w)| (&a.library[i], t, w)).collect();
                let none: Vec<usize> = Vec::new();
                a.rig.sample_with_overrides(&clips, &|k| {
                    a.overrides.get(list[k].0).unwrap_or(&none).as_slice()
                })
            }
            (None, Some(u)) => a.rig.sample_layered(lower, &a.library[u], &a.upper_mask, t),
            (None, None) => a.rig.sample(lower, t),
        };
        for (k, (local, &e)) in pose.iter().zip(&a.joints).enumerate() {
            if let Ok(mut tr) = transforms.get_mut(e) {
                let mut t = Transform {
                    translation: local.translation,
                    rotation: local.rotation,
                    scale: local.scale,
                };
                if let Some(o) = over.filter(|o| o.weight > 0.0 && (o.only.is_empty() || o.only.contains(&k)))
                    && let Some(r) = o.locals.get(k)
                {
                    let w = o.weight.min(1.0);
                    t.translation = t.translation.lerp(r.translation, w);
                    t.rotation = t.rotation.slerp(r.rotation, w);
                    t.scale = t.scale.lerp(r.scale, w);
                }
                *tr = t;
            }
        }
    }
}

/// Strip clip tracks as the original's animation loader does (F046693): each ANIMATION record's
/// "delete ..." bones (F046685) and the lower body of blocks (F046682). First, as it does, each
/// clip copy the records were given (F046697, `split_shared_clips`) is cloned from its shared clip,
/// so each record strips its own.
pub fn strip_tracks(c: &mut CharacterAsset, data: &GameDataContainer) {
    use kenshi_anim::select::{BELOW_WAIST, clip_copies, deleted_bones};
    for (shared, copy) in clip_copies(data) {
        let has = |n: &str| c.animations.iter().any(|a| a.name.eq_ignore_ascii_case(n));
        if has(&copy) {
            continue;
        }
        if let Some(a) = c.animations.iter().find(|a| a.name.eq_ignore_ascii_case(&shared)) {
            let mut a = a.clone();
            a.name = copy;
            c.animations.push(a);
        }
    }
    let mut strip: std::collections::HashMap<String, Vec<&str>> = Default::default();
    for r in data.of_type(ItemType::Animation) {
        let bones = deleted_bones(r);
        if let (Some(name), false) = (r.strings.get("anim name"), bones.is_empty()) {
            strip.entry(name.to_lowercase()).or_default().extend(bones);
        }
    }
    for r in data.of_type(ItemType::CombatTechnique) {
        let b = |k: &str| r.bools.get(k).copied().unwrap_or(false);
        if b("is block")
            && !b("is dodge")
            && let Some(name) = r.strings.get("anim name")
        {
            strip
                .entry(name.to_lowercase())
                .or_default()
                .extend(BELOW_WAIST);
        }
    }
    let rig = c.rig.clone();
    for a in &mut c.animations {
        let Some(bones) = strip.get(&a.name.to_lowercase()) else {
            continue;
        };
        let handles: Vec<u16> = bones
            .iter()
            .filter_map(|b| rig.names.iter().position(|n| n == b))
            .map(|i| rig.handles[i])
            .collect();
        a.tracks.retain(|t| !handles.contains(&t.bone));
    }
    c.library = std::sync::Arc::new(c.animations.clone());
    // override bones (F046683): the joints a clip takes over
    let mut over: std::collections::HashMap<String, Vec<&str>> = Default::default();
    for r in data.of_type(ItemType::Animation) {
        let bones = kenshi_anim::select::override_bones(r);
        if let (Some(name), false) = (r.strings.get("anim name"), bones.is_empty()) {
            over.entry(name.to_lowercase()).or_default().extend(bones);
        }
    }
    c.overrides = std::sync::Arc::new(
        c.animations
            .iter()
            .map(|a| {
                over.get(&a.name.to_lowercase())
                    .map_or(Vec::new(), |bones| {
                        bones
                            .iter()
                            .filter_map(|b| rig.names.iter().position(|n| n == b))
                            .collect()
                    })
            })
            .collect(),
    );
}
