//! Kenshi's tutorials (TutorialManager, F049922's registry; each tutorial's creator and its
//! steps, F049959 .. F050216) as where their texts sit in kenshi_x64.exe: its id, the creator,
//! its title's address and its pages' in order. The texts themselves are read from the
//! player's own copy of the game at run time (`tutorial_ui::texts`), not kept here.
//! Made from the decompilation by the session's tut.py.

/// (id, creator, title, pages)
pub const TUTORIALS: &[(&str, &str, u64, &[u64])] = &[
    ("Tutorial_BasicControls", "F049973", 0x141739898, &[0x141739480, 0x141739590, 0x141739690, 0x141739740, 0x141739800]),
    ("Tutorial_GUIOverview", "F050000", 0x141739f60, &[0x141739928, 0x1417399b0, 0x141739aa0, 0x141739b80, 0x141739c30, 0x141739ce8, 0x141739d70, 0x141739eb0]),
    ("Tutorial_WhatDoIDo", "F050006", 0x14173a528, &[0x14173a440, 0x141739ff0, 0x14173a330, 0x14173a2a0, 0x14173a110]),
    ("Tutorial_WhatDoIDo_InTown", "F050012", 0x14173a808, &[0x14173a790, 0x14173a5b0, 0x14173a690]),
    ("Tutorial_Injured_Intro", "F050018", 0x14173aa50, &[0x14173a8a0, 0x14173a9d0]),
    ("Tutorial_Injured_FirstAid", "F050036", 0x14173b250, &[0x14173aae0, 0x14173ac30, 0x14173b1c0, 0x14173ad30, 0x14173ae70, 0x14173afb0, 0x14173b0d0]),
    ("Tutorial_UnderAttack", "F050042", 0x14173b738, &[0x14173b6a0, 0x14173b5b0, 0x14173b480, 0x14173b2e0]),
    ("Tutorial_HiredRecruit", "F050056", 0x14173bb90, &[0x14173b7d0, 0x14173b920, 0x14173bac0, 0x14173b9f0]),
    ("Tutorial_StolenGoods", "F050060", 0x14173bf98, &[0x14173bf60, 0x14173bea0, 0x14173bd80, 0x14173bc40]),
    ("Tutorial_KOd", "F050065", 0x14173c3e8, &[0x14173c3c8, 0x14173c040, 0x14173c2d0, 0x14173c1d0, 0x14173c0c0]),
    ("Tutorial_KOdWithNonUnconsciousPC", "F050071", 0x14173c6a8, &[0x14173c5f0, 0x14173c580, 0x14173c480]),
    ("Tutorial_EscapePrision", "F050078", 0x14173cae8, &[0x14173ca30, 0x14173c740, 0x14173c8a0, 0x14173c990]),
    ("Tutorial_EscapeCannibalCage", "F050082", 0x14173cd68, &[0x14173cd00, 0x14173cbd0]),
    ("Tutorial_Sneaking", "F050086", 0x14173d210, &[0x14173d170, 0x14173d060, 0x14173cfb0, 0x14173cee0, 0x14173ce70]),
    ("Tutorial_HowToSteal", "F050089", 0x14173d5e8, &[0x14173d4f0, 0x14173d430, 0x14173d320, 0x14173d2a0]),
    ("Tutorial_HowToBounties", "F050095", 0x14173d928, &[0x14173d670, 0x14173d870, 0x14173d800, 0x14173d790]),
    ("Tutorial_YouAreHungry", "F050098", 0x14173dad0, &[0x14173da50, 0x14173d9c0]),
    ("Tutorial_PlayingDead", "F050104", 0x14173dcb0, &[0x14173db70, 0x14173dc50]),
    ("Tutorial_PermaJobs", "F050113", 0x14173e0c0, &[0x14173dd40, 0x14173e010, 0x14173df50, 0x14173deb0]),
    ("Tutorial_BeignSlave", "F050119", 0x14173e448, &[0x14173e3c0, 0x14173e180, 0x14173e330, 0x14173e2a0]),
    ("Tutorial_Disguises", "F050125", 0x14173e6c0, &[0x14173e610, 0x14173e4d0, 0x14173e590]),
    ("Tutorial_MachineFull", "F050131", 0x14173e8f0, &[0x14173e760, 0x14173e8b8]),
    ("Tutorial_BuildingForSale", "F050186", 0x14173fe60, &[0x14173fce0, 0x14173fdd0]),
    ("Tutorial_ProductionBuilding", "F050174", 0x14173f920, &[0x14173f8e0, 0x14173f400, 0x14173f4a0, 0x14173f530, 0x14173f5c0, 0x14173f690, 0x14173f8a8, 0x14173f7b0]),
    ("Tutorial_BuildingGateWalls", "F050134", 0x14173eb88, &[0x14173ead0, 0x14173ea78, 0x14173ea20, 0x14173e9c0]),
    ("Tutorial_HowToCraft", "F050148", 0x14173f0c8, &[0x14173ec50, 0x14173ed08, 0x14173f060, 0x14173edb0, 0x14173efd0, 0x14173ef10, 0x14173eec0]),
    ("Tutorial_HowToCraft_NoCraftingBench", "F050154", 0x14173f338, &[0x14173f180, 0x14173f290, 0x14173f248]),
    ("Tutorial_Research_Benches", "F050180", 0x14173fc10, &[0x14173fa00, 0x14173fb60]),
    ("Tutorial_Research_Built", "F050192", 0x1417401b8, &[0x141740140, 0x141740020, 0x14173fef0, 0x14173ffa0]),
    ("Tutorial_Research_Blueprint", "F050195", 0x1417403f0, &[0x1417403d0, 0x141740340, 0x141740290]),
    ("Tutorial_BuildMode", "F050201", 0x1416bd0c8, &[0x1417408e8, 0x1417404c0, 0x1417407f0, 0x141740740, 0x1417406c0, 0x141740630]),
    ("Tutorial_BuildMode_Construction", "F050210", 0x141740b68, &[0x141740b30, 0x1417409b0, 0x141740a70]),
    ("Tutorial_Prospecting", "F050213", 0x1416bc7e0, &[0x141740d90, 0x141740c30]),
    ("Tutorial_MissingLimb", "F050216", 0x1417410b8, &[0x141740ff0, 0x141740ea0]),
];

/// Each tutorial's pages' GUI pointers, for those with any: the case its step's onStart (its
/// vtable's slot 0) gives TutorialGUI's setPointer (F049763; F049757 the cases), 0 none. Made by
/// the session's tutptr.py from the step constructors (the callers of F049748).
pub const POINTERS: &[(&str, &[u8])] = &[
    ("Tutorial_BasicControls", &[0, 1, 0, 0, 0]),
    ("Tutorial_GUIOverview", &[0, 2, 6, 16, 17, 18, 20, 24]),
    ("Tutorial_Injured_Intro", &[6, 0]),
    ("Tutorial_Injured_FirstAid", &[0, 29, 0, 8, 9, 10, 0]),
    ("Tutorial_UnderAttack", &[0, 0, 0, 25]),
    ("Tutorial_HiredRecruit", &[0, 22, 0, 21]),
    ("Tutorial_KOdWithNonUnconsciousPC", &[0, 0, 30]),
    ("Tutorial_HowToBounties", &[19, 0, 0, 0]),
    ("Tutorial_PlayingDead", &[7, 0]),
    ("Tutorial_PermaJobs", &[26, 0, 0, 27]),
    ("Tutorial_BeignSlave", &[0, 28, 0, 0]),
    ("Tutorial_Disguises", &[0, 7, 0]),
    ("Tutorial_MachineFull", &[7, 0]),
    ("Tutorial_BuildingForSale", &[3, 0]),
    ("Tutorial_ProductionBuilding", &[0, 0, 3, 0, 0, 0, 0, 0]),
    ("Tutorial_HowToCraft", &[23, 31, 0, 32, 0, 0, 0]),
    ("Tutorial_HowToCraft_NoCraftingBench", &[18, 0, 0]),
    ("Tutorial_Research_Benches", &[18, 0]),
    ("Tutorial_Research_Built", &[0, 0, 23, 0]),
];
