//! Kenshi's game: the simulation of a zone (`sim`) on the ported systems.

pub mod sim;
pub mod base;
pub mod law;
pub mod slavery;
pub mod limbs;
pub mod production;
pub mod world;
pub mod trade;
pub mod save;
pub mod dialogue;
pub mod disguise;
pub mod looks_roll;
pub mod body_file;
pub mod kenshi_save;
pub mod speech;
pub mod loot;
pub mod artifacts;
pub mod turrets;
pub mod nests;
pub use kenshi_io::cfg as settings;
