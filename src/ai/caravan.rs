//! The wandering trader package (Package_WanderingTrader, AI_PACKAGE "signal func" 16: caravans,
//! drifters, bounty hunters wandering between towns): pick a town, travel there, stay a few
//! hours, pick the next.
//!
//! Specs: F028938 (start: stay time, town choice), F029005 (update: arriving), F028941 (whether
//! the stay is over), F048860 (the town finder).

/// What the package remembers (Package_WanderingTrader +0xd0..+0xe0).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Caravan {
    /// The town it is heading for or staying in.
    pub target: Option<usize>,
    /// Game hours to stay once there (+0xd4).
    pub stay: f32,
    /// 0 travelling, 1 just arrived, 2 staying (+0xe0).
    pub state: u8,
    /// When it arrived (game hours).
    pub arrived: f64,
}

/// A town as the finder sees it.
pub struct TownView<'a> {
    pub position: [f32; 3],
    /// The TOWN record "type" (+0xd8).
    pub kind: i32,
    /// The TOWN record "is public" ((assumption) the flag at +0x380 that opens a town of another
    /// faction to visitors).
    pub public: bool,
    pub faction: Option<&'a str>,
}

/// The town types a wandering trader visits (F028938 passes the type mask 0x6c: 2, 3, 5, 6).
pub const VISIT_TYPES: u32 = 0x6c;

/// @fn F048860
///
/// The town to go to next: of the towns other than `current`, of a type in `mask`, public or of
/// `faction`, and not refused by `refused` ((assumption) the finder's faction-relation tests:
/// an enemy town), the nearest few (at most 4) by distance from `from`, one picked at random by
/// `roll` in [0, 1).
pub fn find_town(
    towns: &[TownView],
    faction: Option<&str>,
    from: [f32; 3],
    current: Option<usize>,
    mask: u32,
    refused: impl Fn(Option<&str>) -> bool,
    roll: f32,
) -> Option<usize> {
    let mut near: Vec<(f32, usize)> = towns
        .iter()
        .enumerate()
        .filter(|&(k, t)| {
            Some(k) != current
                && (t.public || t.faction == faction)
                && (0..32).contains(&t.kind)
                && mask >> t.kind & 1 != 0
                && !refused(t.faction)
        })
        .map(|(k, t)| ((t.position[0] - from[0]).powi(2) + (t.position[2] - from[2]).powi(2), k))
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    if near.is_empty() {
        return None;
    }
    let top = (near.len() - 1).min(3);
    let pick = ((roll * (top + 1) as f32) as usize).min(top);
    Some(near[pick].1)
}

/// @fn F028938
///
/// Start (again): a stay of 5 to 8 hours (`roll` in [0, 1)), travelling, toward `town`.
pub fn start(town: Option<usize>, roll: f32) -> Caravan {
    Caravan {
        target: town,
        stay: 5.0 + roll * 3.0,
        state: 0,
        arrived: 0.0,
    }
}

impl Caravan {
    /// @fn F029005
    ///
    /// Inside its target town (`in_town`): the state steps on to staying (0 -> 1 -> 2), the
    /// arrival time kept from the first step.
    pub fn arrive(&mut self, in_town: Option<usize>, now: f64) {
        if self.target.is_some() && in_town == self.target {
            if self.state == 0 {
                self.arrived = now;
            }
            self.state = (self.state + 1).min(2);
        }
    }

    /// @fn F028941
    ///
    /// Time to move on: staying, the stay over, and the hour of the day between 6 and 20.
    pub fn leaving(&self, now: f64, hour_of_day: i32) -> bool {
        self.state == 2 && self.stay as f64 <= now - self.arrived && 5 < hour_of_day && hour_of_day < 21
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn town(x: f32, kind: i32, public: bool, faction: &str) -> TownView<'_> {
        TownView { position: [x, 0.0, 0.0], kind, public, faction: Some(faction) }
    }

    #[test]
    fn picks_among_the_four_nearest_open_towns() {
        let towns = [
            town(100.0, 2, true, "a"),
            town(200.0, 4, true, "a"), // a ruin: not visited
            town(300.0, 2, false, "b"), // private, not mine
            town(400.0, 3, false, "a"),
            town(500.0, 5, true, "c"),
            town(600.0, 6, true, "c"),
            town(700.0, 2, true, "c"),
        ];
        let got = |roll| find_town(&towns, Some("a"), [0.0; 3], Some(0), VISIT_TYPES, |_| false, roll);
        assert_eq!(got(0.0), Some(3));
        assert_eq!(got(0.99), Some(6));
        // an enemy faction's towns are left out
        let no_c = find_town(&towns, Some("a"), [0.0; 3], Some(0), VISIT_TYPES, |f| f == Some("c"), 0.99);
        assert_eq!(no_c, Some(3));
    }

    #[test]
    fn stays_then_leaves_by_day() {
        let mut c = start(Some(2), 0.0);
        c.arrive(Some(1), 10.0);
        assert_eq!(c.state, 0);
        c.arrive(Some(2), 10.0);
        c.arrive(Some(2), 11.0);
        assert_eq!((c.state, c.arrived), (2, 10.0));
        assert!(!c.leaving(14.0, 12));
        assert!(c.leaving(15.0, 12));
        assert!(!c.leaving(15.0, 22));
    }
}
