//! A character's combat stance (the main panel's orders, OrdersBlockButton .. OrdersRangedButton:
//! F043076 binds them, F042962..F042968 toggle them through F045513, which sets each selected
//! character's flag through Character +0x308, F039205): CharStats +0x128 block ("defensive
//! mode" in the save), +0x129 ranged ("ranged mode"), +0x12a taunt ("taunt mode"), +0x12b hold
//! ("non combat mode", its hold point +0x130), +0x12c passive ("passive mode"); the order codes
//! 0xb block, 0xc hold, 0xd passive, 0xe taunt, 0xf chase (the squad's AI), 0x11 ranged.

/// The stance flags (CharStats +0x128 .. +0x12c).
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Stance {
    pub block: bool,
    pub ranged: bool,
    pub taunt: bool,
    pub hold: bool,
    pub passive: bool,
}
