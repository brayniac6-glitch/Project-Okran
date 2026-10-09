//! Kenshi's game data: typed views over FCS records read by `kenshi-io`.

pub mod gamedata;
pub mod item_type;

pub use gamedata::{GameData, GameDataContainer};
pub use item_type::ItemType;
