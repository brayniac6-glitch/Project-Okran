//! Faction relations: who counts as an ally or an enemy, from the FACTION records.
//!
//! Specs: sheet rows F041495 FactionRelations::isAlly, F041494 FactionRelations::isEnemy; type
//! T00047 FactionRelations.

use kenshi_data::GameData;
use std::collections::HashMap;

/// At or above this relation factions are allies.
pub const ALLY_RELATION: f32 = 50.0;
/// At or below this relation factions are enemies.
pub const ENEMY_RELATION: f32 = -30.0;

/// The player's faction ("Nameless").
pub const PLAYER_FACTION: &str = "204-gamedata.base";

/// One relation entry.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Relation {
    pub forced_ally: bool,
    pub relation: f32,
}

/// @type FactionRelations
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FactionRelations {
    /// This faction's string id.
    pub faction: String,
    pub relations: HashMap<String, Relation>,
    pub default_relation: f32,
}

/// A FACTION record's "default relation" (stored as an integer).
fn default_relation(r: &GameData) -> f32 {
    r.ints
        .get("default relation")
        .map(|&v| v as f32)
        .or_else(|| r.floats.get("default relation").copied())
        .unwrap_or(0.0)
}

impl FactionRelations {
    /// From a FACTION record: its `relations` references (value 0 = relation) and
    /// `default relation`.
    pub fn from_record(r: &GameData) -> Self {
        FactionRelations {
            faction: r.string_id.clone(),
            relations: r
                .refs("relations")
                .iter()
                .map(|(sid, v)| {
                    (
                        sid.clone(),
                        Relation {
                            forced_ally: false,
                            relation: v[0] as f32,
                        },
                    )
                })
                .collect(),
            default_relation: default_relation(r),
        }
    }

    /// @fn F041513
    ///
    /// A faction's relations as the game sets them up: to every faction the lower of the two
    /// factions' "default relation", itself forced allied at 100, then its own `relations` list.
    ///
    /// The player's faction keeps no relations of its own: PlayerFactionRelations' entry for a
    /// faction (F041507, vtable +0x50) is that faction's entry for the player, so the player
    /// faction's `relations` list (Dust Bandits -100, ...) is the listed factions' relation to
    /// the player, and the player's to each faction is the faction's to it.
    pub fn from_data(r: &GameData, data: &kenshi_data::GameDataContainer) -> Self {
        let factions: Vec<&GameData> = data.of_type(kenshi_data::ItemType::Faction).collect();
        Self::from_data_among(r, data, &factions)
    }

    /// `from_data` with the FACTION records already listed (`data.of_type(Faction)`, in its
    /// order), for a caller setting up many factions at once: the same relations, without
    /// walking every record for each.
    pub fn from_data_among(r: &GameData, data: &kenshi_data::GameDataContainer, factions: &[&GameData]) -> Self {
        let mine = default_relation(r);
        let mut relations: HashMap<String, Relation> = factions
            .iter()
            .map(|o| {
                let theirs = default_relation(o);
                let own = o.string_id == r.string_id;
                (
                    o.string_id.clone(),
                    Relation {
                        forced_ally: own,
                        relation: if own { 100.0 } else { theirs.min(mine) },
                    },
                )
            })
            .collect();
        for (sid, v) in r.refs("relations") {
            relations
                .entry(sid.clone())
                .and_modify(|e| e.relation = v[0] as f32)
                .or_insert(Relation {
                    forced_ally: false,
                    relation: v[0] as f32,
                });
        }
        if r.string_id == PLAYER_FACTION {
            // each faction's own entry for the player
            for o in factions.iter() {
                if o.string_id == r.string_id {
                    continue;
                }
                let listed = r.refs("relations").iter().any(|(sid, _)| *sid == o.string_id);
                if let Some((_, v)) = o.refs("relations").iter().find(|(sid, _)| *sid == r.string_id)
                    && !listed
                {
                    relations.entry(o.string_id.clone()).or_default().relation = v[0] as f32;
                }
            }
        } else if let Some(player) = data.get(PLAYER_FACTION)
            && let Some((_, v)) = player.refs("relations").iter().find(|(sid, _)| *sid == r.string_id)
        {
            relations.entry(PLAYER_FACTION.to_string()).or_default().relation = v[0] as f32;
        }
        FactionRelations {
            faction: r.string_id.clone(),
            relations,
            default_relation: mine,
        }
    }

    pub fn relation(&self, other: &str) -> f32 {
        self.relations
            .get(other)
            .map_or(self.default_relation, |r| r.relation)
    }

    /// @fn F041495
    pub fn is_ally(&self, other: Option<&str>) -> bool {
        let Some(other) = other else { return false };
        if other == self.faction || self.relations.get(other).is_some_and(|r| r.forced_ally) {
            return true;
        }
        ALLY_RELATION <= self.relation(other)
    }

    /// @fn F041494
    pub fn is_enemy(&self, other: Option<&str>) -> bool {
        let Some(other) = other else { return false };
        other != self.faction && self.relation(other) <= ENEMY_RELATION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relations_from_a_faction_record() {
        let mut r = GameData {
            string_id: "shek".into(),
            ..Default::default()
        };
        r.floats.insert("default relation".into(), 0.0);
        r.references.push((
            "relations".into(),
            vec![
                ("holy nation".into(), [-100, 0, 0]),
                ("friends".into(), [60, 0, 0]),
            ],
        ));
        let f = FactionRelations::from_record(&r);
        assert!(f.is_enemy(Some("holy nation")));
        assert!(f.is_ally(Some("friends")));
        assert!(f.is_ally(Some("shek")) && !f.is_enemy(Some("shek")));
        // unlisted factions take the default (0): neither
        assert!(!f.is_ally(Some("traders")) && !f.is_enemy(Some("traders")));
        assert!(!f.is_ally(None));
    }
}
