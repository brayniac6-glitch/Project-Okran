//! Mounted turrets in the simulation: the towns' (furniture of function 12, "Gun turret") and the
//! player's built ones, each an arena turret ([`kenshi_combat::ranged::Turret`]) manned by
//! whoever operates it (Task_UseTurret, F031217).
//!
//! (stand-in) Kenshi's "Turret gunner" goal (task 146, an urgent AI goal) is not planned: a
//! town's guard standing near a free turret mans it when an enemy of theirs comes into its reach.

use crate::sim::{OPERATING_MACHINE, Sim, Target};
use kenshi_combat::ranged::{Turret, turret_gun};
use kenshi_data::{GameData, GameDataContainer};

/// BuildingFunction 12: a gun turret.
pub const BF_TURRET: i32 = 12;
/// How near a guard must stand to a free turret to man it (units).
pub const GUNNER_REACH: f32 = 200.0;

/// The pivot of a building's swivelling head: the offsets down to its first turning part.
fn pivot(data: &GameDataContainer, b: &GameData) -> [f32; 3] {
    kenshi_render::zone::part_meshes_placed(data, b, false)
        .into_iter()
        .find_map(|p| {
            let k = p.chain.iter().position(|(_, a)| a.is_some())?;
            let v = p.chain[..=k].iter().fold(bevy::math::Vec3::ZERO, |s, (o, _)| s + *o);
            Some([v.x, v.y, v.z])
        })
        .unwrap_or([0.0, 10.0, 0.0])
}

/// The turret a BUILDING record makes at `at` facing `yaw`: its "gun turret" GUN_DATA, its
/// "output rate" (the damage) and power factor 1.
/// (stand-in) Power (ProductionBuilding::powerFactor, F029260) is taken as full.
pub fn turret_for(data: &GameDataContainer, b: &GameData, at: [f32; 3], yaw: f32) -> Option<Turret> {
    let gun = data.get(&b.refs("gun turret").first()?.0)?;
    let output = b.floats.get("output rate").copied().or_else(|| b.ints.get("output rate").map(|v| *v as f32)).unwrap_or(50.0);
    Some(Turret::new(at, yaw, pivot(data, b), turret_gun(gun, output), 1.0, output))
}

/// A placement's yaw from its quaternion (w, x, y, z).
fn yaw_of(q: [f32; 4]) -> f32 {
    let [w, x, y, z] = q;
    (2.0 * (w * y + x * z)).atan2(1.0 - 2.0 * (y * y + x * x))
}

impl Sim {
    /// The arena's turrets kept to the world's (new zones' and new buildings' added) and each
    /// one's gunner set: whoever operates it and stands at it.
    pub fn sync_turrets(&mut self) {
        let data = self.data.clone();
        // the towns' turrets
        for f in 0..self.furn.len() {
            if self.furn[f].function != BF_TURRET || self.arena.turrets.iter().any(|t| t.furniture == Some(f)) {
                continue;
            }
            let fu = &self.furn[f];
            let Some(b) = data.get(&fu.record) else { continue };
            // the furniture's own place and facing (an instance in a building is moved and
            // turned with it)
            let (at, yaw) = (fu.position, yaw_of(fu.rotation));
            if let Some(mut t) = turret_for(&data, b, at, yaw) {
                t.furniture = Some(f);
                self.arena.turrets.push(t);
            }
        }
        // the player's
        for b in 0..self.buildings.len() {
            if !self.buildings[b].done || self.arena.turrets.iter().any(|t| t.building == Some(b)) {
                continue;
            }
            let pb = &self.buildings[b];
            let Some(r) = data.get(&pb.record) else { continue };
            if let Some(mut t) = turret_for(&data, r, pb.at, pb.yaw) {
                t.building = Some(b);
                self.arena.turrets.push(t);
            }
        }
        if std::env::var("KENSHI_RANGED_DEBUG").is_ok() && self.tick.is_multiple_of(30) {
            for t in self.arena.turrets.iter().filter(|t| t.operator.is_some()) {
                eprintln!(
                    "turret at {:?} gunner {:?} target {:?} shots {} reloading {} ({:.2}) yaw {:.2} pitch {:.2} aim {:.2}",
                    t.at.map(|v| v as i32), t.operator, t.target, t.gun.shots, t.ranged.reloading, t.ranged.progress, t.yaw, t.pitch, t.ranged.aim
                );
            }
        }
        // gunners
        for k in 0..self.arena.turrets.len() {
            let t = &self.arena.turrets[k];
            let gunner = match (t.furniture, t.building) {
                (Some(f), _) => self.operators.get(f).copied().flatten().filter(|&o| self.folk[o].operating == Some(f) && !self.folk[o].down),
                (_, Some(b)) => {
                    let at = self.buildings[b].at;
                    let r = self.buildings[b].radius;
                    self.folk.iter().find(|p| {
                        let me = p.mover.position_units();
                        p.operate == Some(b) && !p.down && (me[0] - at[0]).hypot(me[2] - at[2]) <= r + 40.0
                    })
                    .map(|p| p.id)
                }
                _ => None,
            };
            self.arena.turrets[k].operator = gunner;
        }
        self.man_turrets();
    }

    /// (stand-in for the "Turret gunner" goal) A free town turret with an enemy of a guard
    /// nearby in its reach: the guard (a town's character, not fighting, within 200 units) goes
    /// to man it.
    fn man_turrets(&mut self) {
        for k in 0..self.arena.turrets.len() {
            let t = &self.arena.turrets[k];
            let Some(f) = t.furniture else { continue };
            if t.operator.is_some() || self.operators.get(f).copied().flatten().is_some() {
                continue;
            }
            let p0 = t.pivot_world();
            let guard = self
                .folk
                .iter()
                .filter(|p| !p.player && !p.down && !p.dead && p.race.is_none() && p.operating.is_none() && !self.arena.f[p.id].active)
                .filter(|p| {
                    let me = p.mover.position_units();
                    (me[0] - p0[0]).hypot(me[2] - p0[2]) <= GUNNER_REACH
                })
                .find(|p| {
                    let me = &self.arena.f[p.id];
                    self.arena.f.iter().any(|e| {
                        !e.down
                            && e.team != me.team
                            && me.foes.as_ref().is_some_and(|v| v.contains(&e.team))
                            && t.gun.in_range(p0, e.pos())
                            && t.in_arc(e.pos())
                    })
                })
                .map(|p| p.id);
            if let Some(g) = guard {
                let at = self.furniture_approach[f];
                self.order_operate_furniture(g, f, at);
            }
        }
    }

    /// Send `id` to man town turret (furniture) `f`, standing at `at`.
    pub fn order_operate_furniture(&mut self, id: usize, f: usize, at: [f32; 3]) {
        self.order_task(&[id], OPERATING_MACHINE, Target::Furniture(f), at);
    }
}
