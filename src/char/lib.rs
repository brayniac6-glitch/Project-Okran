//! Kenshi characters (port_map `character`, `medical`, `stats`). Written from the sheet's specs,
//! never from decompiled code: each item names the row it implements.

pub mod combat_speed;
pub mod encumbrance;
pub mod fitness;
pub mod hunger;
pub mod inventory;
pub mod medical;
pub mod quality;
pub mod stance;
pub mod movement;
pub mod stat_id;
pub mod stats;
pub mod steering;

pub use hunger::{Activity, Hunger};
pub use medical::{BodyPart, Constants, Damage, Medical, MedicalTick, PartState, PartTick, Stats};
pub use stat_id::StatsEnumerated;
