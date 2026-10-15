//! Mounted turrets in the simulation: the towns' (furniture of function 12, "Gun turret") and the
//! player's built ones, each an arena turret ([`kenshi_combat::ranged::Turret`]) manned by
//! whoever operates it (Task_UseTurret, F031217).
//!
//! A town's people man its turrets through their packages' jobs Man the turrets (149) and
//! Turret Gunner Homebuilding (222), planned into Turret gunner (146) (`sim`).

use crate::sim::{OPERATING_MACHINE, Sim, Target};
use kenshi_combat::ranged::{Turret, turret_gun};
use kenshi_data::{GameData, GameDataContainer};

/// BuildingFunction 12: a gun turret.
pub const BF_TURRET: i32 = 12;

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
/// "output rate" (the damage) and power factor 1 (a player's turret's is its base's,
/// `sync_turrets`).
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
        // power (F029260): a player's turret drawing power works as the base's supply allows
        // (F037662's efficiency); (assumption) a town's turrets are powered
        for k in 0..self.arena.turrets.len() {
            if let Some(b) = self.arena.turrets[k].building {
                let p = self.power_efficiency(b);
                self.arena.turrets[k].set_power(p);
            }
        }
        // the output rate with the research done (F029301): every turret of an improved
        // building record, the towns' too
        for k in 0..self.arena.turrets.len() {
            let t = &self.arena.turrets[k];
            let record = match (t.furniture, t.building) {
                (Some(f), _) => self.furn.get(f).map(|x| x.record.clone()),
                (_, Some(b)) => self.buildings.get(b).map(|x| x.record.clone()),
                _ => None,
            };
            if let Some(r) = record {
                let rate = t.base_output * self.improvements(&r).0;
                if rate != t.output {
                    self.arena.turrets[k].set_output_rate(rate);
                }
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
        // @fn F033850
        //
        // Shoot First (the AI tab, "Shoot first, ask later", the player's flag +0x11a): "Your
        // turret gunners will shoot at any potentially dangerous but neutral targets, such as
        // wild animals or bandits, without waiting for them to attack the base first". With no
        // enemy to shoot a player's gunner takes a neutral whose faction stands under -30 with
        // the player's, or one of a squad whose +0x34 is 0 or 8 ((assumption) taken as a wild
        // animal's: of a squad with no people in it, as a herder's goats are not). (not ported)
        // the town gunners' own pass over neutrals (F0141a0's test).
        let also: Vec<u16> = if self.ai_options.shoot_first {
            let kept: std::collections::HashSet<usize> = self.folk.iter().filter(|p| p.race.is_none()).filter_map(|p| p.platoon).collect();
            let player = crate::sim::PLAYER_FACTION;
            let mut v: Vec<u16> = self
                .folk
                .iter()
                .filter(|p| !p.player && !p.dead)
                .filter(|p| {
                    let wild = p.race.is_some() && p.platoon.is_none_or(|pl| !kept.contains(&pl));
                    let hostile = p.faction.as_ref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.relation(player) < -30.0);
                    wild || hostile
                })
                .map(|p| self.arena.f[p.id].team)
                .collect();
            v.sort();
            v.dedup();
            let mine = self.folk.iter().find(|p| p.player).map(|p| self.arena.f[p.id].team);
            v.retain(|t| Some(*t) != mine);
            v
        } else {
            Vec::new()
        };
        for t in self.arena.turrets.iter_mut().filter(|t| t.building.is_some()) {
            t.also = also.clone();
        }
    }

    /// Send `id` to man town turret (furniture) `f`, standing at `at`.
    pub fn order_operate_furniture(&mut self, id: usize, f: usize, at: [f32; 3]) {
        self.order_task(&[id], OPERATING_MACHINE, Target::Furniture(f), at);
    }
}
