//! What ranged combat looks like: the bolts in flight and stuck where they hit (each its
//! crossbow's "live ammo" mesh, its +z along the flight, as HarpoonManager draws them, F033866),
//! and a shooting character's crossbow in hand - its "mesh" with "extra mesh loaded" while a shot
//! is in it, else "extra mesh unloaded" (F033884, F033826) - and everyone else their own weapon.
//!
//! (stand-in) The crossbow hangs on the sword's prop bone ("Bip01 Prop2") with no offset: where
//! Kenshi's attach point and "equip offset" put it is not decompiled.
use super::Game;
use kenshi_render::object_material::KenshiObjectMaterial;
use kenshi_render::terrain_material::TerrainLook;
use bevy::prelude::*;
use std::collections::HashMap;

/// Meshes made from Kenshi's files, by file (None: could not be read).
#[derive(Resource, Default)]
pub struct RangedMeshes {
    meshes: HashMap<String, Option<(Vec<Handle<Mesh>>, Handle<KenshiObjectMaterial>)>>,
    /// The bolts' entities, by index into the arena's bolts.
    bolts: Vec<(Entity, String)>,
    /// What each body holds and wears: the entities, and what they show (the records, their
    /// mesh files and the bones they hang from).
    bows: HashMap<usize, (Vec<Entity>, String)>,
}

/// The sword drawn on a body's prop bone (person id).
#[derive(Component)]
pub struct SwordOf(pub usize);

impl RangedMeshes {
    #[allow(clippy::too_many_arguments)]
    fn get(&mut self, game: &Game, file: &str, material_of: Option<&str>, assets: &AssetServer, meshes: &mut Assets<Mesh>, look: &mut TerrainLook, objects: &mut Assets<KenshiObjectMaterial>) -> Option<(Vec<Handle<Mesh>>, Handle<KenshiObjectMaterial>)> {
        let key = format!("{file}|{}", material_of.unwrap_or(""));
        if let Some(m) = self.meshes.get(&key) {
            return m.clone();
        }
        let made = (|| {
            let path = game.res.path(file)?;
            let mesh = kenshi_io::ogre_mesh::read(&path).ok()?;
            let data = &game.sim.data;
            let mat = material_of.and_then(|r| data.get(r)).and_then(|r| kenshi_render::appearance_material(data, r));
            // (assumption) drawn with Kenshi's object material, as the game's other objects: its
            // item shader is not ported. Plain lit materials sat far darker than the scene's HDR.
            let o = kenshi_render::object_material::ObjectLook::from_record(mat, |f| game.res.find(f));
            let material = objects.add(look.object(assets, &o, game.zone.origin, false, [0.0; 3]));
            // a normal map needs tangents: the weapon meshes carry none
            let hs: Vec<Handle<Mesh>> = kenshi_render::to_bevy(&mesh)
                .into_iter()
                .map(|(_, mut m)| {
                    if m.attribute(Mesh::ATTRIBUTE_TANGENT).is_none() {
                        let _ = m.generate_tangents();
                    }
                    meshes.add(m)
                })
                .collect();
            Some((hs, material))
        })();
        self.meshes.insert(key, made.clone());
        made
    }
}

fn spawn_parts(commands: &mut Commands, parts: &(Vec<Handle<Mesh>>, Handle<KenshiObjectMaterial>)) -> Vec<Entity> {
    parts.0.iter().map(|h| commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(parts.1.clone()), Transform::default())).id()).collect()
}

/// The bolts as they fly and stick.
#[allow(clippy::too_many_arguments)]
pub fn bolts(
    mut commands: Commands,
    game: Res<Game>,
    mut rm: ResMut<RangedMeshes>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut look: ResMut<TerrainLook>,
    mut objects: ResMut<Assets<KenshiObjectMaterial>>,
    mut q: Query<&mut Transform>,
) {
    let list = &game.sim.arena.bolts;
    for (k, b) in list.iter().enumerate() {
        let file = b.mesh.clone().unwrap_or_default();
        if rm.bolts.get(k).is_none_or(|(_, f)| *f != file) {
            if let Some((e, _)) = rm.bolts.get(k) {
                commands.entity(*e).despawn();
            }
            let bow = game.sim.arena.f.get(b.shooter).and_then(|f| f.gun.as_ref()).map(|g| g.record.clone());
            let root = commands.spawn((Transform::default(), Visibility::Visible)).id();
            if let Some(parts) = rm.get(&game, &file, bow.as_deref(), &assets, &mut meshes, &mut look, &mut objects) {
                for e in spawn_parts(&mut commands, &parts) {
                    commands.entity(root).add_child(e);
                }
            }
            if k < rm.bolts.len() {
                rm.bolts[k] = (root, file.clone());
            } else {
                rm.bolts.push((root, file.clone()));
            }
        }
        let (e, _) = rm.bolts[k];
        let at = Vec3::from(b.pos) - game.zone.origin;
        let rot = Quat::from_rotation_arc(Vec3::Z, Vec3::from(b.dir).normalize_or(Vec3::Z));
        match q.get_mut(e) {
            Ok(mut t) => *t = Transform::from_translation(at).with_rotation(rot),
            Err(_) => {
                commands.entity(e).insert(Transform::from_translation(at).with_rotation(rot));
            }
        }
    }
    while rm.bolts.len() > list.len() {
        let (e, _) = rm.bolts.pop().expect("bolt");
        commands.entity(e).despawn();
    }
}

/// A body's joint a weapon hangs from out of a fight (person, bone name).
#[derive(Component)]
pub struct AttachBone(pub usize, pub String);

/// The race's weapon attachment points (RACE "attachment points", attachments-weapons.phs: hip,
/// back, back2): each its bone and, on it, where a sheathed weapon sits (the point's model
/// offset, in bind space, brought into the bone's frame).
#[derive(Resource, Default)]
pub struct AttachPoints(pub HashMap<String, (String, Transform)>);

impl AttachPoints {
    pub fn load(root: &std::path::Path, data: &kenshi_data::GameDataContainer, res: &kenshi_io::resources::Resources) -> AttachPoints {
        let mut out = HashMap::new();
        let file = data
            .of_type(kenshi_data::ItemType::Race)
            .find_map(|r| r.files.get("attachment points").filter(|p| !p.is_empty()).and_then(|p| res.find(p)));
        let Some(phs) = file.and_then(|f| kenshi_io::phs::read(&root.join(f)).ok()) else { return AttachPoints(out) };
        let mat = |f: &kenshi_io::phs::Frame| {
            let [a, b, c] = f.rows.map(Vec3::from);
            // the frame's three vectors are its matrix rows (as the drawn result shows: a
            // back sword lies across the back)
            let q = Quat::from_mat3(&Mat3::from_cols(a, b, c).transpose()).normalize();
            Mat4::from_rotation_translation(q, Vec3::from(f.pos))
        };
        for a in &phs.actors {
            let Some((frame, _, _)) = a.offsets.first() else { continue };
            let local = mat(&a.bone_frame).inverse() * mat(frame);
            out.insert(a.name.clone(), (a.bone.clone(), Transform::from_matrix(local)));
        }
        AttachPoints(out)
    }
}

/// What each character holds and wears: shooting, its crossbow (loaded or not) in hand; in a
/// fight, its first weapon's "bare sword" in hand (else its "mesh"), the sheath left on; out of
/// one, every weapon in its "hip" and "back" slots sheathed at the race's attachment points
/// (hip; back, then back2): the weapon's "mesh" and its "sheath".
/// Drawn while `Person::weapon_out` (kenshi_game::sim::Sim::weapon_tick: from the start of an
/// attack to a second after the fight).
#[allow(clippy::too_many_arguments)]
pub fn bows(
    mut commands: Commands,
    game: Res<Game>,
    mut rm: ResMut<RangedMeshes>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut look: ResMut<TerrainLook>,
    mut objects: ResMut<Assets<KenshiObjectMaterial>>,
    mut swords: Query<(&SwordOf, &ChildOf, &mut Visibility)>,
    bones: Query<(Entity, &AttachBone)>,
    points: Option<Res<AttachPoints>>,
) {
    let arena = &game.sim.arena;
    let mut prop_of: HashMap<usize, Entity> = HashMap::new();
    for (s, parent, mut vis) in &mut swords {
        // the stand-in katana every body was drawn with stays hidden: each holds its own
        if *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
        prop_of.insert(s.0, parent.parent());
    }
    let mut bone_of: HashMap<(usize, &str), Entity> = HashMap::new();
    for (e, b) in &bones {
        bone_of.insert((b.0, b.1.as_str()), e);
    }
    for (id, f) in arena.f.iter().enumerate() {
        let Some(&prop) = prop_of.get(&id) else { continue };
        let Some(p) = game.sim.folk.get(id) else { continue };
        // the pieces: (record, mesh files, the joint, where on it)
        let mut pieces: Vec<(String, Vec<&str>, Entity, Transform)> = Vec::new();
        let slotted: Vec<(&str, &kenshi_char::inventory::Item)> = ["hip", "back"]
            .iter()
            .filter_map(|n| p.inventory.section(n).map(|s| (*n, s)))
            .flat_map(|(n, s)| p.inventory.sections[s].items.iter().map(move |i| (n, &i.item)))
            .filter(|(_, i)| matches!(i.kind, Some(kenshi_data::ItemType::Weapon | kenshi_data::ItemType::Crossbow)))
            .collect();
        if std::env::var("KENSHI_WEAPON_DEBUG").is_ok() && game.sim.tick.is_multiple_of(120) && (f.active || p.weapon_out) {
            let held: Vec<(String, String)> = p
                .inventory
                .sections
                .iter()
                .flat_map(|s| s.items.iter().map(move |i| (s.name.to_string(), &i.item)))
                .filter(|(_, i)| matches!(i.kind, Some(kenshi_data::ItemType::Weapon | kenshi_data::ItemType::Crossbow)))
                .map(|(s, i)| (s, i.name.clone()))
                .collect();
            eprintln!(
                "weapons {id}: out {} active {} down {} shooting {} slotted {:?} wielded {:?} weapons anywhere {:?} race {:?} current {:?}",
                p.weapon_out,
                f.active,
                f.down,
                f.shooting,
                slotted.iter().map(|(s, i)| (*s, i.name.clone(), i.kind)).collect::<Vec<_>>(),
                f.wielded.category,
                held,
                p.race,
                p.ai.current.as_ref().map(|t| t.task)
            );
        }
        let mut drawn: Option<usize> = None;
        if let Some(g) = f.gun.as_ref().filter(|_| f.shooting && !f.down) {
            pieces.push((g.record.clone(), vec!["mesh", if 0 < g.shots { "extra mesh loaded" } else { "extra mesh unloaded" }], prop, Transform::default()));
            drawn = slotted.iter().position(|(_, i)| i.record == g.record);
        } else if p.weapon_out && !f.down {
            if let Some(k) = slotted.iter().position(|(_, i)| i.kind == Some(kenshi_data::ItemType::Weapon)) {
                pieces.push((slotted[k].1.record.clone(), vec!["bare sword"], prop, Transform::default()));
                drawn = Some(k);
            }
        }
        if let Some(points) = &points {
            let mut backs = 0;
            for (k, (slot, item)) in slotted.iter().enumerate() {
                let point = if *slot == "hip" {
                    "hip"
                } else {
                    backs += 1;
                    if backs == 1 { "back" } else { "back2" }
                };
                let Some((bone, at)) = points.0.get(point) else { continue };
                let Some(&joint) = bone_of.get(&(id, bone.as_str())) else { continue };
                let parts = if drawn == Some(k) { vec!["sheath"] } else { vec!["mesh", "sheath"] };
                pieces.push((item.record.clone(), parts, joint, *at));
            }
        }
        let key = pieces.iter().map(|(r, parts, e, _)| format!("{r}|{}|{e:?}", parts.join(","))).collect::<Vec<_>>().join(";");
        if rm.bows.get(&id).is_some_and(|(_, k)| *k == key) {
            continue;
        }
        if let Some((es, _)) = rm.bows.remove(&id) {
            for e in es {
                commands.entity(e).try_despawn();
            }
        }
        let mut roots = Vec::new();
        for (record, parts, parent, at) in pieces {
            let Some(r) = game.sim.data.get(&record) else { continue };
            let file = |k: &str| r.files.get(k).filter(|p| !p.is_empty()).cloned();
            let root = commands.spawn((at, Visibility::Inherited)).id();
            for k in parts {
                // a weapon without a "bare sword" mesh is drawn with its "mesh"
                let path = file(k).or_else(|| (k == "bare sword").then(|| file("mesh")).flatten());
                if let Some(path) = path
                    && let Some(parts) = rm.get(&game, &path, Some(&record), &assets, &mut meshes, &mut look, &mut objects)
                {
                    for e in spawn_parts(&mut commands, &parts) {
                        commands.entity(root).add_child(e);
                    }
                }
            }
            // the body may have gone this frame (a loaded game dresses its people anew): the
            // pieces dropped with it
            commands.queue(move |w: &mut World| {
                if w.get_entity(parent).is_ok() {
                    w.entity_mut(parent).add_child(root);
                } else if let Ok(e) = w.get_entity_mut(root) {
                    e.despawn();
                }
            });
            roots.push(root);
        }
        rm.bows.insert(id, (roots, key));
    }
}

/// (testing) KENSHI_CROSSBOW=<frame>[,<crossbow name>]: at that frame the first selected character
/// is given the crossbow (default "Oldworld Bow MkI") and three packs of its ammo, and sent to
/// attack the nearest stranger standing over 150 units away.
pub fn test_hook(mut game: ResMut<Game>, mut frames: Local<u32>) {
    *frames += 1;
    let Ok(v) = std::env::var("KENSHI_CROSSBOW") else { return };
    // the camera kept between the shooter and its target
    if let Some(&me) = game.selected.first()
        && game.sim.arena.f[me].shooting
        && let Some(o) = game.sim.arena.f[me].focus
    {
        let a = Vec3::from(game.sim.arena.f[me].pos());
        let b = Vec3::from(game.sim.arena.f[o].pos());
        game.focus = (a * 0.6 + b * 0.4) - game.zone.origin;
    }
    // melee: the camera on the armed character, at chest height
    if v.contains(",melee")
        && let Some(&me) = game.selected.first()
        && std::env::var("KENSHI_CROSSBOW_FOLLOW").is_ok()
    {
        game.focus = Vec3::from(game.sim.arena.f[me].pos()) + Vec3::Y * 12.0 - game.zone.origin;
    }
    let mut parts = v.splitn(2, ',');
    let at = parts.next().and_then(|f| f.parse::<u32>().ok());
    // KENSHI_CROSSBOW_ATTACK_AT=<frame>: the attack ordered then instead (the weapon sheathed
    // till then)
    let attack_at = std::env::var("KENSHI_CROSSBOW_ATTACK_AT").ok().and_then(|f| f.parse::<u32>().ok()).or(at);
    if at != Some(*frames) && attack_at != Some(*frames) {
        return;
    }
    let name = parts.next().unwrap_or("Oldworld Bow MkI").to_string();
    let Some(&me) = game.selected.first() else { return };
    let data = game.sim.data.clone();
    if at == Some(*frames) {
    // KENSHI_CROSSBOW_ONLY: the weapons worn before taken off first
    if std::env::var("KENSHI_CROSSBOW_ONLY").is_ok() {
        let inv = &mut game.sim.folk[me].inventory;
        for s in ["hip", "back"] {
            if let Some(k) = inv.section(s) {
                inv.sections[k].items.clear();
            }
        }
    }
    // "melee[:<weapon>]": no crossbow, a fight hand to hand or with that weapon equipped
    if let Some(w) = name.strip_prefix("melee:")
        && let Some(r) = data.of_type(kenshi_data::ItemType::Weapon).find(|r| r.name == w)
    {
        let mut it = kenshi_char::inventory::Item::from_record(r, 1);
        it.quality = 50.0;
        let ok = game.sim.folk[me].inventory.add(it, true);
        game.sim.equip(me);
        eprintln!("crossbow test: {w} equipped {ok}");
    }
    let bow = data.of_type(kenshi_data::ItemType::Crossbow).find(|r| r.name == name);
    if let Some(bow) = bow {
    let mut item = kenshi_char::inventory::Item::from_record(bow, 1);
    item.quality = 50.0;
    let _ = game.sim.folk[me].inventory.add(item, true);
    if let Some(ammo) = bow.refs("ammo").first().and_then(|(id, _)| data.get(id)) {
        for _ in 0..3 {
            let it = kenshi_char::inventory::Item::from_record(ammo, 1);
            let inv = &mut game.sim.folk[me].inventory;
            let mut added = inv.add(it.clone(), false);
            if !added && let Some(m) = inv.section("main") {
                eprintln!("crossbow test: main grid {:?} full, emptied", inv.sections[m].items.iter().map(|p| p.item.name.clone()).collect::<Vec<_>>());
                inv.sections[m].items.clear();
                added = inv.add(it.clone(), false);
            }
            eprintln!("crossbow test: ammo {} {:?} added {added}", it.name, it.ammo);
        }
    }
    game.sim.equip(me);
    }
    }
    if attack_at != Some(*frames) {
        return;
    }
    let here = game.sim.folk[me].mover.position_units();
    let foe = game
        .sim
        .folk
        .iter()
        .filter(|p| !p.player && !p.down && !p.dead && p.race.is_none())
        .map(|p| {
            let q = p.mover.position_units();
            (p.id, (q[0] - here[0]).hypot(q[2] - here[2]))
        })
        .filter(|(_, d)| *d > 150.0)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let f = &game.sim.arena.f[me];
    eprintln!(
        "crossbow test: {name} gun {:?} ammo {} -> foe {foe:?}",
        f.gun.as_ref().map(|g| (g.range, g.damage_min, g.damage_max, g.reload_min, g.reload_max, g.shot_speed)),
        f.ammo
    );
    // KENSHI_CROSSBOW_FOE=<distance>: the stranger brought that far off along +x, on the ground
    if let (Some((foe, _)), Some(d)) = (foe, std::env::var("KENSHI_CROSSBOW_FOE").ok().and_then(|v| v.parse::<f32>().ok())) {
        let origin = game.zone.origin;
        let (nx, nz) = (here[0] + d, here[2]);
        let y = game.ground(nx - origin.x, nz - origin.z) + origin.y;
        game.sim.folk[foe].mover.position = [nx / 10.0, y / 10.0, nz / 10.0];
        game.sim.folk[foe].pathing.follow = None;
    }
    if let Some((foe, _)) = foe {
        game.watch = Some(foe);
        game.sim.order_attack(&[me], foe);
        // the rest of the squad steps back out of the line of fire
        let there = game.sim.folk[foe].mover.position_units();
        let (dx, dz) = (here[0] - there[0], here[2] - there[2]);
        let l = dx.hypot(dz).max(1.0);
        let back = [here[0] + dx / l * 80.0, here[1], here[2] + dz / l * 80.0];
        let others: Vec<usize> = game.sim.folk.iter().filter(|p| p.player && p.id != me).map(|p| p.id).collect();
        eprintln!("crossbow test: squad {others:?} back to {back:?}");
        game.sim.order_move_squad(&others, back);
    }
}

/// The turrets' heads posed as the arena aims them (kenshi_render::zone::TurretAims, keyed by
/// the building's world position).
pub fn turret_aims(game: Res<Game>, mut aims: ResMut<kenshi_render::zone::TurretAims>) {
    aims.0.clear();
    for t in &game.sim.arena.turrets {
        aims.0.insert(kenshi_render::zone::TurretAims::key(Vec3::from(t.at)), (t.yaw, t.pitch));
    }
}

/// (testing) KENSHI_TURRET=<frame>: the first selected character mans the nearest turret and the
/// nearest stranger, set 250 units in front of it, comes to attack them.
pub fn turret_test(mut game: ResMut<Game>, mut frames: Local<u32>) {
    *frames += 1;
    let Some(at) = std::env::var("KENSHI_TURRET").ok().and_then(|v| v.parse::<u32>().ok()) else { return };
    let Some(&me) = game.selected.first() else { return };
    if *frames > at
        && let Some(t) = game.sim.arena.turrets.iter().find(|t| t.operator == Some(me))
    {
        let p = Vec3::from(t.pivot_world());
        let o = t.target.map(|k| Vec3::from(game.sim.arena.f[k].pos())).unwrap_or(p + Vec3::new(t.facing.sin(), 0.0, t.facing.cos()) * 150.0);
        let w = if std::env::var("KENSHI_TURRET_CLOSE").is_ok() { 0.0 } else { 0.4 };
        game.focus = (p * (1.0 - w) + o * w) - game.zone.origin;
    }
    if *frames != at {
        return;
    }
    eprintln!("turret test: {} turrets", game.sim.arena.turrets.len());
    let here = game.sim.folk[me].mover.position_units();
    let Some(k) = (0..game.sim.arena.turrets.len()).filter(|&k| game.sim.arena.turrets[k].furniture.is_some()).min_by(|&a, &b| {
        let d = |k: usize| {
            let p = game.sim.arena.turrets[k].at;
            (p[0] - here[0]).hypot(p[2] - here[2])
        };
        d(a).total_cmp(&d(b))
    }) else {
        eprintln!("turret test: no turret");
        return;
    };
    let (f, facing, pivot) = {
        let t = &game.sim.arena.turrets[k];
        (t.furniture.unwrap(), t.facing, t.pivot_world())
    };
    let stand = game.sim.furniture_approach[f];
    // put the gunner at the turret
    game.sim.folk[me].mover.position = stand.map(|v| v / 10.0);
    game.sim.order_operate_furniture(me, f, stand);
    let foe = game
        .sim
        .folk
        .iter()
        .filter(|p| !p.player && !p.down && !p.dead && p.race.is_none())
        .map(|p| p.id)
        .min_by(|&a, &b| {
            let d = |i: usize| {
                let q = game.sim.folk[i].mover.position_units();
                (q[0] - here[0]).hypot(q[2] - here[2])
            };
            d(a).total_cmp(&d(b))
        });
    if let Some(foe) = foe {
        let (nx, nz) = (pivot[0] + facing.sin() * 250.0, pivot[2] + facing.cos() * 250.0);
        let origin = game.zone.origin;
        let y = game.ground(nx - origin.x, nz - origin.z) + origin.y;
        game.sim.folk[foe].mover.position = [nx / 10.0, y / 10.0, nz / 10.0];
        game.sim.folk[foe].pathing.follow = None;
        let team = game.sim.arena.f[foe].team;
        if let Some(v) = &mut game.sim.arena.f[me].foes
            && !v.contains(&team)
        {
            v.push(team);
        }
        game.sim.order_attack(&[foe], me);
        game.watch = Some(foe);
        eprintln!("turret test: {me} mans turret {k} (furniture {f}) facing {facing:.2}; {foe} comes from {nx:.0},{nz:.0}");
    }
}

/// (testing) KENSHI_NEST=<frame>: at that frame a nest of the biome rises 500 units from the
/// first selected character, and the camera looks at it.
pub fn nest_test(mut game: ResMut<Game>, mut frames: Local<u32>, mut at: Local<Option<Vec3>>) {
    *frames += 1;
    let Some(f) = std::env::var("KENSHI_NEST").ok().and_then(|v| v.parse::<u32>().ok()) else { return };
    if let Some(p) = *at {
        game.focus = p - game.zone.origin;
    }
    if *frames != f {
        return;
    }
    let Some(&me) = game.selected.first() else { return };
    let q = game.sim.folk[me].mover.position_units();
    let dist = std::env::var("KENSHI_NEST_DIST").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(500.0);
    let (x, z) = (q[0] + dist, q[2]);
    let origin = game.zone.origin;
    let y = game.ground(x - origin.x, z - origin.z) + origin.y;
    let name = game.sim.spawn_nest_at([x, y, z]);
    eprintln!("nest test: {name:?} at {x:.0},{z:.0}");
    *at = Some(Vec3::new(x, y, z));
}

/// (testing) KENSHI_WEAPON_ROW=<frame>: at that frame every weapon and crossbow is laid on the
/// ground in rows beside the first selected character, and the camera looks at them.
pub fn weapon_row(mut game: ResMut<Game>, mut frames: Local<u32>, mut at: Local<Option<Vec3>>) {
    *frames += 1;
    let Some(f) = std::env::var("KENSHI_WEAPON_ROW").ok().and_then(|v| v.parse::<u32>().ok()) else { return };
    if let Some(p) = *at {
        game.focus = p - game.zone.origin;
    }
    if *frames != f {
        return;
    }
    let Some(&me) = game.selected.first() else { return };
    let q = game.sim.folk[me].mover.position_units();
    let data = game.sim.data.clone();
    let mut recs: Vec<_> = data
        .of_type(kenshi_data::ItemType::Weapon)
        .chain(data.of_type(kenshi_data::ItemType::Crossbow))
        .collect();
    recs.sort_by(|a, b| a.name.cmp(&b.name));
    for (k, r) in recs.iter().enumerate() {
        let (row, col) = ((k / 14) as f32, (k % 14) as f32);
        let it = kenshi_char::inventory::Item::from_record(r, 1);
        game.sim.next_ground += 1;
        let id = game.sim.next_ground;
        let (x, z) = (q[0] + 10.0 + col * 6.0, q[2] + row * 25.0);
        let origin = game.zone.origin;
        let y = game.ground(x - origin.x, z - origin.z) + origin.y;
        game.sim.ground.push(kenshi_game::loot::GroundItem { id, item: it, at: [x, y, z], yaw: 0.0 });
    }
    *at = Some(Vec3::new(q[0] + 50.0, q[1], q[2] + 25.0));
    eprintln!("weapon row: {} weapons", recs.len());
}

/// (testing) KENSHI_WOUND=<frame>: at that frame the townsperson nearest the first selected
/// character who has a squadmate is cut (each part to 40% of its health, bleeding), and the
/// camera follows them.
pub fn wound_test(mut game: ResMut<Game>, mut frames: Local<u32>, mut who: Local<Option<usize>>) {
    *frames += 1;
    let Some(f) = std::env::var("KENSHI_WOUND").ok().and_then(|v| v.parse::<u32>().ok()) else { return };
    if let Some(v) = *who {
        game.focus = Vec3::from(game.sim.folk[v].mover.position_units()) - game.zone.origin;
    }
    if *frames != f {
        return;
    }
    let Some(&me) = game.selected.first() else { return };
    let at = Vec3::from(game.sim.folk[me].mover.position_units());
    let sim = &game.sim;
    let victim = sim
        .folk
        .iter()
        .filter(|p| !p.player && !p.down && !p.dead && p.race.is_none() && p.platoon.is_some())
        .filter(|p| sim.folk.iter().any(|q| q.id != p.id && q.platoon == p.platoon && !q.dead))
        .min_by(|a, b| {
            let d = |p: &kenshi_game::sim::Person| Vec3::from(p.mover.position_units()).distance(at);
            d(a).total_cmp(&d(b))
        })
        .map(|p| p.id);
    let Some(v) = victim else { return };
    let m = &mut game.sim.arena.f[v].medical;
    for p in &mut m.parts {
        p.flesh = p.max_health() * 0.4;
    }
    m.bleed_rate = 0.3;
    *who = Some(v);
    eprintln!("wound test: {} ({v}) wounded", game.sim.folk[v].name);
}

/// (testing) KENSHI_KO_ME=<frame>: at that frame the first selected character is knocked out
/// beside the nearest looter (else where it stands), and the camera stays on them.
pub fn ko_me_test(mut game: ResMut<Game>, mut frames: Local<u32>, mut who: Local<Option<usize>>) {
    *frames += 1;
    let Some(f) = std::env::var("KENSHI_KO_ME").ok().and_then(|v| v.parse::<u32>().ok()) else { return };
    if let Some(v) = *who {
        game.focus = Vec3::from(game.sim.folk[v].mover.position_units()) - game.zone.origin;
    }
    if *frames != f {
        return;
    }
    let Some(&me) = game.selected.first() else { return };
    // beside the nearest looter (AI_TASK 177), when there is one
    let at = Vec3::from(game.sim.folk[me].mover.position_units());
    let looter = game
        .sim
        .folk
        .iter()
        .filter(|p| !p.player && !p.dead && p.ai.jobs.iter().flatten().any(|g| g.task == 177))
        .min_by(|a, b| Vec3::from(a.mover.position_units()).distance(at).total_cmp(&Vec3::from(b.mover.position_units()).distance(at)))
        .map(|p| p.id);
    if let Some(l) = looter {
        let mut spot = game.sim.folk[l].mover.position;
        spot[0] += 6.0;
        game.sim.folk[me].mover.position = spot;
        game.sim.arena.f[me].mover.position = spot;
    }
    game.sim.knock_out(me);
    game.sim.arena.f[me].medical.knockout_time = 600.0;
    *who = Some(me);
    eprintln!("ko test: {me} knocked out");
}

/// (testing) KENSHI_ARM=<frame>[,<weapon>...]: at that frame each of the player's characters is
/// given the weapons (default a Katana and a Nodachi) equipped, without a fight.
pub fn arm_test(mut game: ResMut<Game>, mut frames: Local<u32>) {
    *frames += 1;
    let Ok(v) = std::env::var("KENSHI_ARM") else { return };
    let mut parts = v.split(',');
    if parts.next().and_then(|f| f.parse::<u32>().ok()) != Some(*frames) {
        return;
    }
    let mut names: Vec<String> = parts.map(str::to_string).collect();
    if names.is_empty() {
        names = vec!["Katana".into(), "Nodachi".into()];
    }
    let data = game.sim.data.clone();
    let ids: Vec<usize> = game.sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    for id in ids {
        for n in &names {
            // (a weapon by that name, else any item: a backpack, armour)
            let r = data.of_type(kenshi_data::ItemType::Weapon).find(|r| &r.name == n).or_else(|| data.records.values().find(|r| &r.name == n && r.item_type().is_some_and(|t| matches!(t, kenshi_data::ItemType::Container | kenshi_data::ItemType::Armour | kenshi_data::ItemType::Item))));
            if let Some(r) = r {
                let mut it = kenshi_char::inventory::Item::from_record(r, 1);
                it.quality = 50.0;
                let ok = game.sim.folk[id].inventory.add(it, true);
                eprintln!("arm test: {id} given {n}: {ok}");
            }
        }
    }
}
