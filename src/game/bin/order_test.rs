//! (testing) KENSHI_ORDER_TEST=<scenario>: the right-click menu's errands (`kenshi_game::orders`)
//! driven straight through the sim, each step and result printed ("order test: ..."):
//! bed (sleep in the nearest town bed), putbed (knock the nearest townsperson out, carry them,
//! put them in a bed), cage (the same into the nearest free cage), getin (get in a cage),
//! splint (break the second character's leg, the first splints it), shackles (shackle the
//! second: Use Strength weak then strong, then Use Tools, then Unlock Shackles), dispose (a body
//! carried away), kidnap (the Pick Up / Kidnap choice). The selection is teleported where it
//! needs to be.
use super::Game;
use bevy::prelude::*;
use kenshi_game::orders::ShackleHow;

fn log(s: String) {
    eprintln!("order test: {s}");
}

fn give(game: &mut Game, id: usize, record: &str, n: u32) {
    if let Some(r) = game.sim.data.get(record) {
        let item = kenshi_char::inventory::Item::from_record(r, n);
        let inv = &mut game.sim.folk[id].inventory;
        let mut ok = inv.add(item.clone(), false);
        if !ok {
            // no room: the main grid's items go
            if let Some(s) = inv.sections.iter_mut().find(|s| s.name == "main") {
                s.items.clear();
            }
            ok = inv.add(item, false);
        }
        log(format!("gave {} to {id}: {ok}", r.name));
    }
}

fn nearest_furniture(game: &Game, id: usize, function: i32, free: impl Fn(&Game, usize) -> bool) -> Option<usize> {
    let at = game.sim.folk[id].mover.position_units();
    (0..game.sim.furn.len()).filter(|&k| game.sim.furn[k].function == function && free(game, k)).min_by(|&a, &b| {
        let d = |k: usize| (game.sim.furn[k].position[0] - at[0]).hypot(game.sim.furn[k].position[2] - at[2]);
        d(a).total_cmp(&d(b))
    })
}

fn teleport(game: &mut Game, id: usize, at: [f32; 3], off: f32) {
    let p = [at[0] / 10.0 + off, at[1] / 10.0, at[2] / 10.0];
    game.sim.folk[id].mover.position = p;
    game.sim.arena.f[id].mover.position = p;
    game.focus = Vec3::from(at) - game.zone.origin;
}

fn nearest_townsperson(game: &Game, id: usize) -> Option<usize> {
    let at = game.sim.folk[id].mover.position_units();
    game.sim
        .folk
        .iter()
        .filter(|p| !p.player && !p.down && !p.dead && p.inside.is_none() && p.prison.is_none())
        .min_by(|a, b| {
            let d = |p: &kenshi_game::sim::Person| {
                let q = p.mover.position_units();
                (q[0] - at[0]).hypot(q[2] - at[2])
            };
            d(a).total_cmp(&d(b))
        })
        .map(|p| p.id)
}

#[derive(Default)]
pub struct State {
    frame: u32,
    a: Option<usize>,
    b: Option<usize>,
}

pub fn script(mut s: Local<State>, mut game: ResMut<Game>) {
    let Ok(which) = std::env::var("KENSHI_ORDER_TEST") else { return };
    s.frame += 1;
    let f = s.frame;
    let Some(&me) = game.selected.first() else { return };
    let other = game.sim.folk.iter().find(|p| p.player && p.id != me).map(|p| p.id);
    match which.as_str() {
        "bed" => {
            if f == 150 {
                let Some(b) = nearest_furniture(&game, me, kenshi_game::sim::BF_BED, |g, k| g.sim.bed_free(k)) else { return log("no free bed".into()) };
                let at = game.sim.furn[b].position;
                teleport(&mut game, me, at, 4.0);
                s.a = Some(b);
                log(format!("bed {b} {}", game.sim.furn[b].name));
            }
            if f == 200
                && let Some(b) = s.a
            {
                game.sim.order_sleep(&[me], b);
            }
            if f == 600 {
                log(format!("{me} in bed {:?} (errands {})", game.sim.folk[me].bed, game.sim.bed_errands.len()));
                let at = game.sim.folk[me].mover.position_units();
                game.sim.order_move(&[me], [at[0] + 80.0, at[1], at[2]]);
            }
            if f == 700 {
                log(format!("{me} after a move order: bed {:?}", game.sim.folk[me].bed));
            }
        }
        "putbed" | "cage" => {
            if f == 150 {
                let bed = which == "putbed";
                let spot = if bed {
                    nearest_furniture(&game, me, kenshi_game::sim::BF_BED, |g, k| g.sim.bed_free(k))
                } else {
                    nearest_furniture(&game, me, kenshi_world::furniture::BF_CAGE, |g, k| g.sim.cage_free(k))
                };
                let Some(k) = spot else { return log("no free furniture".into()) };
                let at = game.sim.furn[k].position;
                teleport(&mut game, me, at, 6.0);
                s.a = Some(k);
                if let Some(v) = nearest_townsperson(&game, me) {
                    let p = game.sim.folk[me].mover.position;
                    game.sim.folk[v].mover.position = [p[0] + 1.5, p[1], p[2]];
                    game.sim.arena.f[v].mover.position = game.sim.folk[v].mover.position;
                    game.sim.knock_out(v);
                    s.b = Some(v);
                    log(format!("knocked out {v} {}; kidnap? {}", game.sim.display_name(v), game.sim.is_kidnap(me, v)));
                }
            }
            if f == 220
                && let Some(v) = s.b
            {
                game.sim.order_pickup(&[me], v);
            }
            if f == 400 {
                let (Some(k), v) = (s.a, s.b.unwrap_or(0)) else { return };
                log(format!("{me} carrying {:?}", game.sim.folk[me].carrying));
                if which == "putbed" {
                    game.sim.order_put_in_bed(&[me], k);
                } else {
                    game.sim.order_cage(&[me], k, false);
                }
                let _ = v;
            }
            if f == 900 {
                let v = s.b.unwrap_or(0);
                let p = &game.sim.folk[v];
                log(format!("{v}: bed {:?}, prison {:?}, carried by {:?}", p.bed, p.prison.as_ref().map(|q| (q.cage, q.faction.clone(), q.lock.level)), p.carried_by));
            }
        }
        "getin" => {
            if f == 150 {
                let Some(k) = nearest_furniture(&game, me, kenshi_world::furniture::BF_CAGE, |g, k| g.sim.cage_free(k)) else { return log("no free cage".into()) };
                let at = game.sim.furn[k].position;
                teleport(&mut game, me, at, 6.0);
                game.sim.order_cage(&[me], k, true);
                s.a = Some(k);
            }
            if f == 600 {
                let p = &game.sim.folk[me];
                log(format!("{me}: prison {:?}", p.prison.as_ref().map(|q| (q.cage, q.lock.level))));
            }
        }
        "splint" => {
            let Some(o) = other else { return };
            if f == 150 {
                give(&mut game, me, "1435-gamedata.base", 2);
                let m = &mut game.sim.arena.f[o].medical;
                if let Some(leg) = m.parts.iter_mut().find(|q| q.kind == 1) {
                    // broken and bandaged: a splint goes on a part with treatment (F040439)
                    leg.flesh = -20.0;
                    leg.treatment = 30.0;
                    leg.treated = true;
                }
                log(format!("{o} needs splint: {}", game.sim.needs_splint(o)));
                game.sim.order_splint(&[me], o);
            }
            if f == 700 {
                for k in [me, o] {
                    let parts: Vec<String> = game.sim.arena.f[k].medical.parts.iter().filter(|q| q.kind == 1 || q.kind == 2).map(|q| format!("{} {:.0} splint {:.1}", q.id, q.flesh, q.bonus)).collect();
                    log(format!("{k} limbs: {parts:?}; kit {:?}, medic {:.0}", game.sim.splint_kit(k), game.sim.arena.f[k].level(kenshi_char::StatsEnumerated::StatMedic)));
                }
            }
        }
        "shackles" => {
            let Some(o) = other else { return };
            if f == 150 {
                let fac = "18-gamedata.base".to_string();
                game.sim.dress_as_slave(o, &fac);
                log(format!("{o} shackles lock {:?}", game.sim.shackles_lock(o)));
                game.sim.arena.f[me].skills.levels.insert(kenshi_char::StatsEnumerated::StatStrength, 30.0);
                game.sim.order_shackles(&[me], o, ShackleHow::Strength);
            }
            if f == 300 {
                log(format!("after weak strength: lock {:?}", game.sim.shackles_lock(o)));
                game.sim.arena.f[me].skills.levels.insert(kenshi_char::StatsEnumerated::StatStrength, 70.0);
                game.sim.order_shackles(&[me], o, ShackleHow::Strength);
            }
            if f == 450 {
                log(format!("after strong strength: lock {:?}", game.sim.shackles_lock(o)));
                game.sim.dress_as_slave(o, "18-gamedata.base");
                give(&mut game, me, "581-gamedata.base", 1);
                game.sim.order_shackles(&[me], o, ShackleHow::Cut);
            }
            if f == 600 {
                log(format!("cutting: {:?}", game.sim.shackle_bar(me)));
            }
            if f == 1200 {
                log(format!("after cutting a while: lock {:?}, bar {:?}", game.sim.shackles_lock(o), game.sim.shackle_bar(me)));
                game.sim.shackle_work.clear();
                game.sim.arena.f[me].skills.levels.insert(kenshi_char::StatsEnumerated::StatLockpicking, 100.0);
                game.sim.order_shackles(&[me], o, ShackleHow::Pick);
            }
            if f == 2400 {
                log(format!("after picking: lock {:?}", game.sim.shackles_lock(o)));
            }
        }
        "dispose" => {
            if f == 150
                && let Some(v) = nearest_townsperson(&game, me)
            {
                let p = game.sim.folk[me].mover.position;
                game.sim.folk[v].mover.position = [p[0] + 1.5, p[1], p[2]];
                game.sim.arena.f[v].mover.position = game.sim.folk[v].mover.position;
                game.sim.knock_out(v);
                game.sim.folk[v].dead = true;
                game.sim.arena.f[v].medical.dead = true;
                s.b = Some(v);
                log(format!("{v} dead; kidnap? {}", game.sim.is_kidnap(me, v)));
            }
            if f == 200
                && let Some(v) = s.b
            {
                game.sim.order_dispose(&[me], v);
                log(format!("disposers {:?}", game.sim.disposers));
            }
            if f == 2400
                && let Some(v) = s.b
            {
                let a = game.sim.folk[v].mover.position_units();
                log(format!("body {v} now at {a:?}, carried by {:?}, disposers {:?}", game.sim.folk[v].carried_by, game.sim.disposers));
            }
        }
        // the nearest door: the first selected sent to open it (task 72), then where it stands
        "door" => {
            if f == 150 {
                let at = game.sim.folk[me].mover.position_units();
                if let Some(d) = game.sim.door_near(at, 3000.0) {
                    let s = &game.sim.door_states[d];
                    log(format!("door {d} at {:?} phase {} locked {}", s.door.position.map(|v| v as i32), s.phase, s.locked));
                    game.sim.order_door(&[me], d);
                }
            }
            if f == 900 {
                let at = game.sim.folk[me].mover.position_units();
                if let Some(d) = game.sim.door_near(at, 3000.0) {
                    let s = &game.sim.door_states[d];
                    log(format!("door {d} phase {} open {:.2}; {me} at {:?}, order {:?}", s.phase, s.open, at.map(|v| v as i32), game.sim.folk[me].door_order));
                }
            }
        }
        "towns" => {
            if f == 100 {
                let z = |v: f32| ((v + kenshi_world::WORLD_SIZE / 2.0) / kenshi_world::ZONE_SIZE).floor() as i32;
                for t in &game.sim.towns {
                    log(format!("town {} {} zone {}.{}", t.record, t.name, z(t.position[0]), z(t.position[2])));
                }
            }
        }
        // a squad hired as bodyguards for a short while: followed, then the contract's end said
        "hire" => {
            if f == 150 {
                let at = game.sim.folk[me].mover.position_units();
                let near = game
                    .sim
                    .folk
                    .iter()
                    .filter(|p| !p.player && !p.down && !p.dead && p.role == 5 && p.prison.is_none() && p.shop.is_none())
                    .min_by(|a, b| {
                        let d = |p: &kenshi_game::sim::Person| {
                            let q = p.mover.position_units();
                            (q[0] - at[0]).hypot(q[2] - at[2])
                        };
                        d(a).total_cmp(&d(b))
                    })
                    .map(|p| p.id);
                let Some(l) = near else { return log("no squad leader here".into()) };
                s.b = Some(l);
                game.sim.dialog_memory.partner.insert(l, me);
                game.sim.hire(l, "5090-gamedata.base", 0.25);
                log(format!("hired {l} {} ({:?}): contracts {:?}", game.sim.display_name(l), game.sim.folk[l].package, game.sim.contracts.iter().map(|c| (c.members.clone(), c.until)).collect::<Vec<_>>()));
                let at = game.sim.folk[me].mover.position_units();
                game.sim.order_move(&[me], [at[0] + 600.0, at[1], at[2]]);
            }
            if f % 300 == 0
                && let Some(l) = s.b
            {
                let (a, b) = (game.sim.folk[me].mover.position_units(), game.sim.folk[l].mover.position_units());
                log(format!(
                    "f {f}: leader {:.0} from me, package {:?}, contracts {}, said {:?}",
                    (a[0] - b[0]).hypot(a[2] - b[2]),
                    game.sim.folk[l].package,
                    game.sim.contracts.len(),
                    game.sim.said.iter().filter(|x| x.0 == l).map(|x| x.1.clone()).collect::<Vec<_>>()
                ));
            }
        }
        // a crowd trigger: the war cry taken up by the squad of the nearest leader with mates
        "crowd" => {
            if f == 150 {
                let l = game.sim.folk.iter().filter(|p| !p.player && !p.down && p.role == 5 && p.platoon.is_some()).map(|p| p.id).find(|&l| game.sim.squad_of(l).len() > 2);
                let Some(l) = l else { return log("no squad here".into()) };
                game.sim.dialog_memory.partner.insert(l, me);
                game.sim.crowd(l, "61023-Dialogue.mod", 100.0);
                let said: Vec<String> = game.sim.said.iter().map(|(w, t)| format!("{w}: {t}")).collect();
                log(format!("squad of {l}: {:?}; said {said:?}", game.sim.squad_of(l)));
            }
        }
        // short-term tags: a squad leader's squad takes the player's for a temporary ally, then
        // a day on it has forgotten
        "tempally" => {
            use kenshi_ai::PerceptionShortTerm::StTemporaryAlly;
            let l = game.sim.folk.iter().filter(|p| !p.player && !p.down && p.role == 5 && p.platoon.is_some()).map(|p| p.id).next();
            let Some(l) = l else { return };
            let mate = game.sim.squad_of(l).into_iter().find(|&k| k != l).unwrap_or(l);
            if f == 150 {
                game.sim.tag_short(l, me, StTemporaryAlly);
                log(format!("{l} tags {me}: mate {mate} remembers {} (and of {:?}: {})", game.sim.has_short(mate, me, StTemporaryAlly), other, other.is_some_and(|o| game.sim.has_short(mate, o, StTemporaryAlly))));
            }
            if f == 200 {
                game.sim.t += (25.0 * game.sim.hour_secs) as f32;
            }
            if f == 300 {
                log(format!("a day on: {}", game.sim.has_short(mate, me, StTemporaryAlly)));
            }
        }
        // weather damage: what hurts the selection and its health, every five seconds
        "hazard" => {
            // (KENSHI_HAZARD_AT=x,z) taken there first
            if f == 100
                && let Ok(v) = std::env::var("KENSHI_HAZARD_AT")
            {
                let c: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                if let [x, z] = c[..] {
                    let y = game.sim.folk[me].mover.position_units()[1];
                    teleport(&mut game, me, [x, y, z], 0.0);
                    if let Some(o) = other {
                        teleport(&mut game, o, [x, y, z], 3.0);
                    }
                }
            }
            // into the first hurting effect, once there is one
            if f % 60 == 0 && f < 600 {
                let q = game.sim.folk[me].mover.position_units();
                log(format!("f {f}: me at {:.0},{:.0}; harms {:?}", q[0], q[2], game.sim.harms.iter().map(|h| (h.at[0] as i32, h.at[2] as i32)).collect::<Vec<_>>()));
            }
            if f == 150
                && let Some(h) = game.sim.harms.first().copied()
            {
                log(format!("harms {:?}", game.sim.harms));
                teleport(&mut game, me, h.at, 0.0);
                if let Some(o) = other {
                    teleport(&mut game, o, h.at, 30.0);
                }
            }
            if f % 300 == 0 {
                let p = &game.sim.folk[me];
                let hp: Vec<String> = game.sim.arena.f[me].medical.parts.iter().map(|q| format!("{:.1}", q.flesh - q.stun)).collect();
                let here = p.mover.position_units();
                let z = kenshi_world::zone_of(here[0], here[2]);
                log(format!(
                    "f {f}: hazard {:?}, dust {:.2}, inside {:?}, climate {:?}, harms {}, parts {hp:?}",
                    p.hazard.as_ref().map(|h| h.text()),
                    p.dust,
                    p.inside,
                    game.sim.climate.get(&z),
                    game.sim.harms.len()
                ));
            }
        }
        // swimming: the selection walks to the nearest deep water about and swims on
        "swim" => {
            if f == 150 {
                let at = game.sim.folk[me].mover.position_units();
                let m = kenshi_world::navmesh::METRES;
                let mut best: Option<[f32; 3]> = None;
                'find: for r in (200..6000).step_by(200) {
                    for k in 0..24 {
                        let a = k as f32 / 24.0 * std::f32::consts::TAU;
                        let want = [at[0] + a.cos() * r as f32, 0.0, at[2] + a.sin() * r as f32];
                        if let Some((_, q)) = game.sim.nav.closest_point(want.map(|v| v * m), 50.0) {
                            let q = q.map(|v| v / m);
                            let ground = game.sim.terrain.height_at(q[0], q[2]).unwrap_or(q[1]);
                            if ground < 70.0 {
                                best = Some(q);
                                break 'find;
                            }
                        }
                    }
                }
                let mut low = (f32::MAX, [0.0f32; 3]);
                for r in (200..6000).step_by(200) {
                    for k in 0..24 {
                        let a = k as f32 / 24.0 * std::f32::consts::TAU;
                        let want = [at[0] + a.cos() * r as f32, 0.0, at[2] + a.sin() * r as f32];
                        if let Some((_, q)) = game.sim.nav.closest_point(want.map(|v| v * m), 50.0) {
                            let q = q.map(|v| v / m);
                            if q[1] < low.0 {
                                low = (q[1], q);
                            }
                        }
                    }
                }
                log(format!("lowest navmesh about: {low:?}; me at {at:?}"));
                let Some(to) = best else { return log("no water about".into()) };
                log(format!("deep water at {to:?}"));
                s.a = Some(0);
                game.sim.player_run = true;
                game.sim.order_move(&[me], to);
            }
            if f % 120 == 0 && s.a.is_some() {
                let p = &game.sim.folk[me];
                let q = p.mover.position_units();
                log(format!(
                    "f {f}: at {:.0},{:.0},{:.0} water {} float {:?} speed {:.1} swimming {:.2}",
                    q[0],
                    q[1],
                    q[2],
                    p.water,
                    p.float_y,
                    p.speed,
                    game.sim.arena.f[me].level(kenshi_char::StatsEnumerated::StatSwimming)
                ));
            }
        }
        // item spots: the nearest with an item taken, then two days on it is back
        "spots" => {
            if f == 150 {
                let at = game.sim.folk[me].mover.position_units();
                let near = (0..game.sim.spots.len()).filter(|&k| game.sim.spots[k].item.is_some()).min_by(|&a, &b| {
                    let d = |k: usize| (game.sim.spots[k].at[0] - at[0]).hypot(game.sim.spots[k].at[2] - at[2]);
                    d(a).total_cmp(&d(b))
                });
                let Some(k) = near else { return log(format!("no spots with items ({} spots)", game.sim.spots.len())) };
                let id = game.sim.spots[k].item.unwrap();
                let name = game.sim.ground.iter().find(|g| g.id == id).map(|g| g.item.name.clone());
                log(format!("{} spots, {} with items; nearest {k} {:?} owner {:?} item {name:?}", game.sim.spots.len(), game.sim.spots.iter().filter(|s| s.item.is_some()).count(), game.sim.spots[k].group, game.sim.spots[k].owner));
                let p = game.sim.spots[k].at;
                teleport(&mut game, me, p, 1.0);
                game.sim.order_pick_item(&[me], id);
                s.a = Some(k);
            }
            if f == 400
                && let Some(k) = s.a
            {
                log(format!("spot {k} now {:?} due {:.1} (now {:.1}); crime {:?}", game.sim.spots[k].item, game.sim.spots[k].due, game.sim.hours_gone(), game.sim.folk[me].crime.as_ref().map(|c| (c.kind, c.faction.clone()))));
                game.sim.t += (50.0 * game.sim.hour_secs) as f32;
            }
            if f == 500
                && let Some(k) = s.a
            {
                let name = game.sim.spots[k].item.and_then(|id| game.sim.ground.iter().find(|g| g.id == id)).map(|g| g.item.name.clone());
                log(format!("two days on: spot {k} has {name:?}"));
            }
        }
        // a regenerating squad loses one, the player is away a day, it gets one back
        "regen" => {
            if f == 150 {
                let pick = game.sim.squad_template.iter().find(|(_, t)| game.sim.data.get(*t).is_some_and(|r| r.bools.get("regenerates").copied().unwrap_or(false))).map(|(k, t)| (*k, t.clone()));
                let Some((pid, t)) = pick else { return log(format!("no regenerating squad of {}", game.sim.squad_template.len())) };
                let members: Vec<usize> = game.sim.folk.iter().filter(|p| p.platoon == Some(pid)).map(|p| p.id).collect();
                log(format!("squad {pid} {:?}: {} members", game.sim.data.get(&t).map(|r| r.name.clone()), members.len()));
                // two thirds of it dead (the game remakes a squad that has lost 0.6)
                let kill = (members.len() * 3).div_ceil(4);
                let doomed: Vec<usize> = members.iter().rev().copied().filter(|&m| game.sim.folk[m].role != 5).take(kill).collect();
                for m in doomed {
                    game.sim.folk[m].dead = true;
                    game.sim.arena.f[m].medical.dead = true;
                }
                s.a = Some(pid);
                // the player's people far off
                let at = game.sim.folk[me].mover.position_units();
                for id in game.sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect::<Vec<_>>() {
                    game.sim.folk[id].mover.position = [(at[0] + 20000.0) / 10.0, at[1] / 10.0, at[2] / 10.0];
                    game.sim.arena.f[id].mover.position = game.sim.folk[id].mover.position;
                }
            }
            if f == 200 {
                game.sim.t += (25.0 * game.sim.hour_secs) as f32;
            }
            if f == 400
                && let Some(pid) = s.a
            {
                let alive = game.sim.folk.iter().filter(|p| p.platoon == Some(pid) && !p.dead).count();
                let all = game.sim.folk.iter().filter(|p| p.platoon == Some(pid)).count();
                log(format!("a day on: squad {pid} {alive} alive of {all}"));
            }
        }
        // the squad split across the map: the selection taken 35000 units off, the other left
        "split" => {
            if f == 150 {
                let at = game.sim.folk[me].mover.position_units();
                teleport(&mut game, me, [at[0] + 35000.0, at[1], at[2] + 1000.0], 0.0);
                log("split".into());
            }
            if f % 300 == 0 {
                log(format!("f {f}: running"));
            }
        }
        // the Rescue and Medic orders: the other knocked out and rescued to a bed, then hurt and
        // treated
        "jobs" => {
            let Some(o) = other else { return };
            if f == 150 {
                let b = nearest_furniture(&game, me, kenshi_game::sim::BF_BED, |g, k| g.sim.bed_free(k));
                let Some(b) = b else { return log("no bed".into()) };
                let at = game.sim.furn[b].position;
                teleport(&mut game, me, at, 120.0);
                teleport(&mut game, o, at, 140.0);
                game.sim.knock_out(o);
                game.sim.order_rescue(&[me]);
                log(format!("{o} knocked out; rescue ordered (bed {b} free)"));
            }
            if f % 300 == 0 && f < 2400 {
                log(format!("f {f}: {o} bed {:?}, carried by {:?}, {me} carrying {:?} errands {:?} at {:?}", game.sim.folk[o].bed, game.sim.folk[o].carried_by, game.sim.folk[me].carrying, game.sim.bed_errands.iter().map(|e| (e.id, e.bed, e.put, e.lying)).collect::<Vec<_>>(), game.sim.folk[me].mover.position_units()));
            }
            if f == 2400 {
                log(format!("rescue: {o} bed {:?}, carried by {:?}, {me} job {:?}", game.sim.folk[o].bed, game.sim.folk[o].carried_by, game.sim.folk[me].job));
                game.sim.get_out_of_bed_test(o);
                if let Some(leg) = game.sim.arena.f[o].medical.parts.iter_mut().find(|q| q.kind == 1) {
                    leg.flesh = -10.0;
                }
                give(&mut game, me, "209-gamedata.base", 1);
                game.sim.order_medic(&[me]);
            }
            if f == 3600 {
                let need = game.sim.arena.f[o].medical.aid_need(false);
                log(format!("medic: {o} need {:.2}, {me} job {:?}, first aid {:?}", need.flesh, game.sim.folk[me].job, game.sim.folk[me].first_aid));
            }
        }
        // who is about: each non-player near the selection, what it is doing
        // into the nearest private home (its residents' "public day" false) with someone of it
        // home; KENSHI_INTRUDE_NIGHT=1: at 23 o'clock with its doors shut and locked
        "intrude" => {
            if f == 150 {
                let at = game.sim.folk[me].mover.position_units();
                let private = |g: &Game, b: usize| {
                    g.sim.folk.iter().find(|p| p.home == Some(b) && !p.player).and_then(|p| p.platoon).and_then(|pl| g.sim.squad_template.get(&pl)).and_then(|t| g.sim.data.get(t)).is_some_and(|t| !t.bools.get("public day").copied().unwrap_or(true))
                };
                let host = game
                    .sim
                    .folk
                    .iter()
                    .filter(|p| !p.player && !p.dead && p.home.is_some() && p.inside == p.home && private(&game, p.home.unwrap()))
                    .min_by(|a, b| {
                        let d = |p: &kenshi_game::sim::Person| {
                            let q = p.mover.position_units();
                            (q[0] - at[0]).hypot(q[2] - at[2])
                        };
                        d(a).total_cmp(&d(b))
                    })
                    .map(|p| p.id);
                let Some(h) = host else { return log("no one at home in a private building".into()) };
                let home = game.sim.folk[h].home.unwrap();
                let t = game.sim.folk[h].platoon.and_then(|pl| game.sim.squad_template.get(&pl).cloned());
                log(format!("host {h} {} home {home} template {t:?} doors {}", game.sim.display_name(h), game.sim.door_states.iter().filter(|d| d.door.placement == home).count()));
                if std::env::var("KENSHI_INTRUDE_NIGHT").is_ok() {
                    let hour = game.sim.hour();
                    let wait = (23.0 - hour).rem_euclid(24.0);
                    game.sim.t += (wait * game.sim.hour_secs) as f32;
                    for d in game.sim.door_states.iter_mut().filter(|d| d.door.placement == home) {
                        d.phase = kenshi_game::sim::DOOR_CLOSED;
                        d.open = 0.0;
                        d.locked = true;
                        d.level = d.level.max(1);
                    }
                }
                let p = game.sim.folk[h].mover.position_units();
                teleport(&mut game, me, p, 2.0);
                s.a = Some(h);
                s.b = Some(home);
            }
            // KENSHI_INTRUDE_KO=1: the intruder knocked out (to be carried out)
            if f == 600 && std::env::var("KENSHI_INTRUDE_KO").is_ok() {
                game.sim.knock_out(me);
                log(format!("{me} knocked out"));
            }
            if f > 150
                && f % 120 == 0
                && let (Some(h), Some(home)) = (s.a, s.b)
            {
                log(format!(
                    "f {f} hour {:.1}: down {} carried by {:?} at {:?} inside {:?} public {} locked up {} trespassing {} shooing {} crime {:?} bounty {:?} host active {} focus {:?} d {:.0} host speed {:.1} me hp {:.0} arena d {:.0} attackers {:?} wielded {:?}",
                    game.sim.hour(),
                    game.sim.folk[me].down,
                    game.sim.folk[me].carried_by,
                    game.sim.folk[me].mover.position_units().map(|v| v as i32),
                    game.sim.folk[me].inside,
                    game.sim.building_public(home),
                    game.sim.locked_up(home),
                    game.sim.trespassing(h, me),
                    game.sim.shooing(h, me),
                    game.sim.folk[me].crime.as_ref().map(|c| c.kind),
                    game.sim.folk[me].bounties.iter().map(|b| (b.faction.clone(), b.amount)).collect::<Vec<_>>(),
                    game.sim.arena.f[h].active,
                    game.sim.arena.f[h].focus,
                    {
                        let (a, b) = (game.sim.folk[h].mover.position_units(), game.sim.folk[me].mover.position_units());
                        (a[0] - b[0]).hypot(a[2] - b[2])
                    },
                    game.sim.folk[h].speed,
                    game.sim.arena.f[me].medical.parts.iter().map(|q| q.health()).sum::<f32>(),
                    {
                        let (a, b) = (game.sim.arena.f[h].pos(), game.sim.arena.f[me].pos());
                        (a[0] - b[0]).hypot(a[2] - b[2])
                    },
                    game.sim.arena.f[h].combat.attackers,
                    game.sim.arena.f[h].wielded.category
                ));
            }
        }
        // SURRENDER_NON_HUMANS: the second character a Shek, the nearest townsperson beside the
        // squad takes it (and not the first, a Greenlander)
        "surrender" => {
            if f == 150
                && let Some(o) = other
            {
                game.sim.folk[o].look_race = Some("5276-chareditor.mod".into());
                game.sim.folk[me].look_race = Some("17-gamedata.quack".into());
                let Some(t) = game.sim.folk.iter().filter(|p| !p.player && !p.dead && p.race.is_none() && p.faction.is_some() && p.prison.is_none()).map(|p| p.id).find(|&k| game.sim.race_of(k).as_deref() == Some("17-gamedata.quack")) else { return log("no Greenlander with a faction".into()) };
                let at = game.sim.folk[me].mover.position_units();
                teleport(&mut game, t, at, 3.0);
                teleport(&mut game, o, at, -3.0);
                let n = game.sim.surrender_non_humans(t, me);
                log(format!("{t} ({:?}, race {:?}) took {n}: {o} slave {:?} shackled {} escort {:?}; {me} slave {:?}", game.sim.folk[t].faction, game.sim.race_of(t), game.sim.folk[o].slave.as_ref().map(|s| s.faction.clone()), game.sim.folk[o].shackled, game.sim.folk[o].escort, game.sim.folk[me].slave.is_some()));
                s.a = Some(t);
            }
            if f == 900
                && let (Some(o), Some(t)) = (other, s.a)
            {
                let (a, b) = (game.sim.folk[o].mover.position_units(), game.sim.folk[t].mover.position_units());
                log(format!("later: {o} {:.0} from its captor", (a[0] - b[0]).hypot(a[2] - b[2])));
            }
        }
        // a town changing hands: the town the squad is in, the uniques its first override's world
        // states want dead marked dead, then the squad sent 9000 units away and back (Kenshi
        // takes the override as the town's zone loads in a later game load: it stays as it is)
        "reseat" => {
            if f == 150 {
                let Some(t) = game.sim.folk[me].town else { return log("not in a town".into()) };
                let data = game.sim.data.clone();
                let Some(rec) = data.get(&game.sim.towns[t].record) else { return };
                let Some(over) = rec.refs("override town").first().and_then(|(o, _)| data.get(o)) else { return log(format!("{} has no override", rec.name)) };
                let mut dead = Vec::new();
                for (ws, v) in over.refs("world state") {
                    let Some(w) = data.get(ws) else { continue };
                    for (c, cv) in w.refs("NPC is") {
                        if (v[0] != 0) == (cv[0] == 0) {
                            dead.push(c.clone());
                        }
                    }
                    // "NPC is NOT" alive (1), wanted to hold: dead
                    for (c, cv) in w.refs("NPC is NOT") {
                        if v[0] != 0 && cv[0] == 1 {
                            dead.push(c.clone());
                        }
                    }
                }
                log(format!("{} (now {}) -> {} wants dead {dead:?}; residents now {}", rec.name, game.sim.town_record(t), over.name, game.sim.folk.iter().filter(|p| p.home.is_some() && !p.away && !p.player).count()));
                for c in dead {
                    game.sim.unique_died(&c);
                }
                let at = game.sim.folk[me].mover.position_units();
                for id in game.sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect::<Vec<_>>() {
                    teleport(&mut game, id, [at[0] + 9000.0, at[1], at[2]], 0.0);
                }
                s.a = Some(t);
            }
            if f == 400
                && let Some(t) = s.a
            {
                log(format!("town {} record now {}; residents loaded and here {}", game.sim.towns[t].name, game.sim.town_record(t), game.sim.folk.iter().filter(|p| p.home.is_some() && !p.away && !p.player).count()));
            }
        }
        // EV_USING_MY_TRAINING_EQUIPMENT: the first character trains on the nearest town training
        // equipment
        "trainhere" => {
            if f == 150 {
                let at = game.sim.folk[me].mover.position_units();
                let Some(k) = (0..game.sim.furn.len()).filter(|&k| game.sim.furniture_training(k).is_some()).min_by(|&a, &b| {
                    let d = |k: usize| (game.sim.furn[k].position[0] - at[0]).hypot(game.sim.furn[k].position[2] - at[2]);
                    d(a).total_cmp(&d(b))
                }) else {
                    return log("no training equipment".into());
                };
                let p = game.sim.furn[k].position;
                teleport(&mut game, me, p, 3.0);
                game.sim.order_train_at(&[me], k);
                log(format!("training at {k} {}", game.sim.furn[k].name));
            }
            if f % 300 == 0 && f > 150 {
                log(format!("f {f}: operating {:?}, owner {:?}", game.sim.folk[me].operating, game.sim.training_owner(me)));
            }
        }
        // the AI tab's formation (KENSHI_FORMATION=0 random, 1 caravan, 2 military): the whole
        // squad ordered 300 units on
        "faction" => {
            // the selection beside the nearest of KENSHI_NEAR_FACTION's people (by faction name)
            if f == 60
                && let Ok(name) = std::env::var("KENSHI_NEAR_FACTION")
            {
                let at = game.sim.folk[me].mover.position_units();
                let near = game
                    .sim
                    .folk
                    .iter()
                    .filter(|p| !p.player && !p.dead && p.faction.as_deref().and_then(|f| game.sim.data.get(f)).is_some_and(|r| r.name == name))
                    .min_by(|a, b| {
                        let d = |p: &kenshi_game::sim::Person| {
                            let q = p.mover.position_units();
                            (q[0] - at[0]).hypot(q[2] - at[2])
                        };
                        d(a).total_cmp(&d(b))
                    })
                    .map(|p| (p.id, p.mover.position_units()));
                if let Some((id, there)) = near {
                    log(format!("beside {id} {}", game.sim.display_name(id)));
                    teleport(&mut game, me, there, 2.0);
                    game.selected = vec![me];
                }
            }
        }
        "inside" => {
            // the selection inside a building: KENSHI_INSIDE=<n> the n-th tallest interior here
            // (its volume's height), else the tallest; the camera after it
            if f == 60 {
                let mut tall: Vec<(usize, f32)> = game.sim.inner.iter().filter(|i| game.sim.inside_point.get(i.placement).copied().flatten().is_some()).map(|i| (i.placement, i.hi[1] - i.lo[1])).collect();
                tall.sort_by(|a, b| b.1.total_cmp(&a.1));
                for (k, (pl, h)) in tall.iter().take(8).enumerate() {
                    let name = game.sim.data.get(&game.sim.placed[*pl].building).map_or("?".to_string(), |r| r.name.clone());
                    log(format!("interior {k}: placement {pl} {name} {h:.0} tall"));
                }
                let n: usize = std::env::var("KENSHI_INSIDE").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
                // (KENSHI_INSIDE=dirt: the one the ground reaches into most, sampled on a grid)
                if std::env::var("KENSHI_INSIDE").is_ok_and(|v| v == "dirt") {
                    let o = game.zone.origin;
                    let mut best: Vec<(usize, usize)> = game
                        .sim
                        .inner
                        .iter()
                        .map(|i| {
                            let mut n = 0;
                            for a in 0..12 {
                                for b in 0..12 {
                                    let x = i.lo[0] + (i.hi[0] - i.lo[0]) * (a as f32 + 0.5) / 12.0;
                                    let z = i.lo[2] + (i.hi[2] - i.lo[2]) * (b as f32 + 0.5) / 12.0;
                                    let y = game.ground(x - o.x, z - o.z) + o.y;
                                    if i.contains([x, y + 3.0, z]) {
                                        n += 1;
                                    }
                                }
                            }
                            (i.placement, n)
                        })
                        .collect();
                    best.sort_by_key(|b| std::cmp::Reverse(b.1));
                    log(format!("ground inside: {:?}", &best[..best.len().min(8)]));
                    if let Some(&(pl, _)) = best.first() {
                        tall.retain(|t| t.0 == pl);
                    }
                }
                // (KENSHI_INSIDE=p<placement>: that placement)
                let pick = std::env::var("KENSHI_INSIDE").ok().and_then(|v| v.strip_prefix('p').and_then(|v| v.parse::<usize>().ok()));
                if let Some(&(pl, _)) = pick.and_then(|p| tall.iter().find(|t| t.0 == p)).or(tall.get(n))
                    && let Some(at) = game.sim.inside_point.get(pl).copied().flatten()
                {
                    let name = game.sim.data.get(&game.sim.placed[pl].building).map_or("?".to_string(), |r| r.name.clone());
                    log(format!("inside {name} at {at:?}"));
                    teleport(&mut game, me, at, 0.0);
                    game.selected = vec![me];
                }
            }
        }
        "packbull" => {
            // the second character made a pack bull (its bags on), the first given meat to
            // carry in them; selected: its inventory (the animal window) is the one opened
            if f == 60
                && let Some(o) = other
                && let Some(bull) = game.sim.data.records.values().find(|r| r.name == "Pack Bull" && r.item_type() == Some(kenshi_data::ItemType::AnimalCharacter)).map(|r| r.string_id.clone())
            {
                game.sim.make_animal(o, &bull);
                if let Some(meat) = game.sim.data.records.values().find(|r| r.name == "Dried Meat").cloned() {
                    let ok = game.sim.folk[o].inventory.bag_mut().is_some_and(|b| b.add(kenshi_char::inventory::Item::from_record(&meat, 40), false));
                    log(format!("pack bull {o}: meat in its bags {ok}, weight {:.1}", game.sim.folk[o].inventory.weight()));
                }
                // (KENSHI_BULL_GRID=<item>) something in its own grid too
                if let Ok(n) = std::env::var("KENSHI_BULL_GRID")
                    && let Some(r) = game.sim.data.records.values().find(|r| r.name == n && r.item_type().is_some()).cloned()
                {
                    let ok = game.sim.folk[o].inventory.add(kenshi_char::inventory::Item::from_record(&r, 1), false);
                    log(format!("pack bull {o}: {n} in its grid {ok}"));
                }
                game.selected = vec![o];
            }
        }
        "formation" => {
            if f == 150 {
                game.sim.ai_options.formation = std::env::var("KENSHI_FORMATION").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
                let ids: Vec<usize> = game.sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
                let at = game.sim.folk[me].mover.position_units();
                game.sim.order_move_squad(&ids, [at[0] + 300.0, at[1], at[2]]);
                game.focus = bevy::math::Vec3::from([at[0] + 300.0, at[1], at[2]]) - game.zone.origin;
                log(format!("formation {} for {} people", game.sim.ai_options.formation, ids.len()));
            }
        }
        "about" => {
            // (KENSHI_BOUNTY=n) the selection's bounties that big
            if f == 120
                && let Some(n) = std::env::var("KENSHI_BOUNTY").ok().and_then(|v| v.parse::<i32>().ok())
            {
                for b in &mut game.sim.folk[me].bounties {
                    b.amount = n;
                }
            }
            if f % 300 == 0 {
                let at = game.sim.folk[me].mover.position_units();
                let near: Vec<String> = game
                    .sim
                    .folk
                    .iter()
                    .filter(|p| !p.player && {
                        let q = p.mover.position_units();
                        (q[0] - at[0]).hypot(q[2] - at[2]) < 600.0
                    })
                    .map(|p| {
                        let q = p.mover.position_units();
                        format!("{} {} d{:.0} active {} away {} dlg {:?} fac {:?} arrest {:?}", p.id, game.sim.display_name(p.id), (q[0] - at[0]).hypot(q[2] - at[2]), game.sim.arena.f[p.id].active, p.away, p.dialogue.is_some(), p.faction, p.arrest.as_ref().map(|a| a.target))
                    })
                    .collect();
                log(format!("f {f}: {near:?}"));
            }
        }
        _ => {}
    }
}
