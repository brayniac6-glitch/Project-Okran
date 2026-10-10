//! Prospecting (Task_Prospecting, F031238 / F031299; the results, F035095).
//!
//! The main panel's PROSPECT order sets each selected character prospecting: kneeling a while
//! under a "Prospecting" progress bar, done when the task's progress (its per-tick time added
//! up) reaches 1, i.e. after a second; then the prospecting window opens on the results there:
//! where it was done, the prospector's Science ("Affects skill at prospecting, and researching
//! new technologies."), and its name.
//!
//! (assumption) The kneel (the "medic" animation the task plays) is not shown; a prospector given
//! another order, or knocked down, stops.

use crate::sim::Sim;
use kenshi_char::StatsEnumerated;

/// The prospecting window's results: the place (units), the prospector's Science level, its name.
#[derive(Debug, Clone)]
pub struct ProspectResult {
    pub at: [f32; 3],
    pub skill: f32,
    pub name: String,
}

impl Sim {
    /// The PROSPECT order: each of `ids` starts prospecting where it stands.
    pub fn order_prospect(&mut self, ids: &[usize]) {
        for &id in ids {
            let Some(p) = self.folk.get_mut(id) else { continue };
            if p.dead || p.down || p.prison.is_some() {
                continue;
            }
            p.ai.queue.clear();
            kenshi_char::movement::stop(&mut p.mover, &mut p.pathing);
            self.prospectors.retain(|(o, _)| *o != id);
            self.prospectors.push((id, 0.0));
        }
    }

    /// How far `id`'s prospecting has gone (0 .. 1), if it is prospecting.
    pub fn prospect_status(&self, id: usize) -> Option<f32> {
        self.prospectors.iter().find(|(o, _)| *o == id).map(|(_, f)| *f)
    }

    /// @fn F031238
    ///
    /// Each prospector's progress: the time gone added; at 1 the results are out
    /// (`Sim::prospect_result`). One down, dead or walking off stops.
    pub fn prospect_tick(&mut self, dt: f32) {
        let mut done = Vec::new();
        let folk = &self.folk;
        let easy = self.advanced.easy_prospecting;
        self.prospectors.retain_mut(|(id, f)| {
            let p = &folk[*id];
            if p.dead || p.down || p.mover.velocity[0].hypot(p.mover.velocity[2]) > 0.5 {
                return false;
            }
            // (crate::advanced) easy prospecting: at once
            *f += if easy { 1.0 } else { dt };
            if *f >= 1.0 {
                done.push(*id);
                return false;
            }
            true
        });
        for id in done {
            self.prospect_result = Some(ProspectResult {
                at: self.folk[id].mover.position_units(),
                skill: self.arena.f[id].level(StatsEnumerated::StatScience),
                name: self.display_name(id),
            });
        }
    }
}
