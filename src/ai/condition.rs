//! AI conditions (port_map `ai`): what Kenshi registers at start-up (F039876, 147 registrations
//! through F039875 register) as condition id -> AI check method and a debug name (or the line a
//! character says when it fails). A task definition lists conditions as preconditions and as
//! completion conditions (`task_def`); AI slot 0 (F039867) looks the check up, calls it with the
//! task's target and context and compares the answer with the expected value (a condition with
//! no check passes). A few conditions are registered inline (ids 0, 2, 3, 6, ...: not listed).

/// The check behind a condition (not ported yet).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Check {
    /// An AI virtual method by vtable slot.
    Method(u8),
    /// A plain function by address.
    Function(u64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Condition {
    pub id: u16,
    pub name: &'static str,
    pub check: Check,
}

/// The condition with an id.
pub fn condition(id: u16) -> Option<&'static Condition> {
    CONDITIONS
        .binary_search_by_key(&id, |c| c.id)
        .ok()
        .map(|i| &CONDITIONS[i])
}

/// Registered conditions, by id.
pub static CONDITIONS: &[Condition] = &[
    Condition {
        id: 1,
        name: "isAtLocation",
        check: Check::Method(11),
    },
    Condition {
        id: 4,
        name: "hasWeapon",
        check: Check::Function(0x14059a9c0),
    },
    Condition {
        id: 5,
        name: "hasWeaponEquipped",
        check: Check::Function(0x140596a30),
    },
    Condition {
        id: 7,
        name: "isTargetDown",
        check: Check::Function(0x14099a5a0),
    },
    Condition {
        id: 12,
        name: "isAlliesFighting",
        check: Check::Function(0x14099a350),
    },
    Condition {
        id: 15,
        name: "isItemLootable",
        check: Check::Function(0x14059dcc0),
    },
    Condition {
        id: 16,
        name: "isAggressionTowards",
        check: Check::Function(0x14059de10),
    },
    Condition {
        id: 17,
        name: "AM_IDLE",
        check: Check::Function(0x14060e5b0),
    },
    Condition {
        id: 18,
        name: "isAtHome",
        check: Check::Function(0x1409996a0),
    },
    Condition {
        id: 22,
        name: "I need a medkit.",
        check: Check::Function(0x14099d8f0),
    },
    Condition {
        id: 23,
        name: "I need a medkit.",
        check: Check::Function(0x140598860),
    },
    Condition {
        id: 24,
        name: "isCrouching",
        check: Check::Function(0x140597670),
    },
    Condition {
        id: 25,
        name: "isLyingProne",
        check: Check::Function(0x140597690),
    },
    Condition {
        id: 26,
        name: "IS_CROUCHING_OR_LYING",
        check: Check::Function(0x140599e90),
    },
    Condition {
        id: 27,
        name: "isMovementAllowed",
        check: Check::Function(0x1405a3370),
    },
    Condition {
        id: 28,
        name: "isNearTo",
        check: Check::Function(0x1405a3270),
    },
    Condition {
        id: 29,
        name: "isGeneralAggressionLevel",
        check: Check::Function(0x140597ac0),
    },
    Condition {
        id: 31,
        name: "atNode",
        check: Check::Function(0x1405a32d0),
    },
    Condition {
        id: 32,
        name: "hasDestinationTown",
        check: Check::Function(0x1405075d0),
    },
    Condition {
        id: 33,
        name: "",
        check: Check::Function(0x140507730),
    },
    Condition {
        id: 36,
        name: "atDestinationTown",
        check: Check::Function(0x140507610),
    },
    Condition {
        id: 37,
        name: "isReadyForAction",
        check: Check::Function(0x140998550),
    },
    Condition {
        id: 38,
        name: "My arms are messed up...",
        check: Check::Function(0x140998470),
    },
    Condition {
        id: 39,
        name: "isAggressionTowardsMe",
        check: Check::Function(0x1405a34a0),
    },
    Condition {
        id: 40,
        name: "isWithin10Meters",
        check: Check::Function(0x1405a32b0),
    },
    Condition {
        id: 41,
        name: "isWithin50Meters",
        check: Check::Function(0x1405a3290),
    },
    Condition {
        id: 42,
        name: "targetStandingStill",
        check: Check::Function(0x14079b120),
    },
    Condition {
        id: 43,
        name: "iAmDown",
        check: Check::Function(0x140598bb0),
    },
    Condition {
        id: 45,
        name: "canGetUp",
        check: Check::Function(0x140598c20),
    },
    Condition {
        id: 46,
        name: "isAtLocation",
        check: Check::Method(11),
    },
    Condition {
        id: 47,
        name: "IS_CARRYING_SOMETHING",
        check: Check::Function(0x140598d20),
    },
    Condition {
        id: 48,
        name: "IS_CARRYING_TARGET",
        check: Check::Function(0x14059a300),
    },
    Condition {
        id: 50,
        name: "I don't have any materials",
        check: Check::Function(0x14059ba50),
    },
    Condition {
        id: 51,
        name: "buildingHasBuildingMaterials",
        check: Check::Function(0x1405a39c0),
    },
    Condition {
        id: 52,
        name: "DOOR_IS_OPEN",
        check: Check::Function(0x1405a3ad0),
    },
    Condition {
        id: 53,
        name: "DOOR_IS_OPEN_HERE",
        check: Check::Function(0x1405a3b90),
    },
    Condition {
        id: 54,
        name: "It's locked.",
        check: Check::Function(0x1405a00c0),
    },
    Condition {
        id: 55,
        name: "It's locked.",
        check: Check::Function(0x1405a3bd0),
    },
    Condition {
        id: 56,
        name: "DOOR_IS_BROKEN",
        check: Check::Function(0x1405a3a10),
    },
    Condition {
        id: 57,
        name: "AT_LOCATION_BUILDING_DOOR",
        check: Check::Function(0x1405a25b0),
    },
    Condition {
        id: 58,
        name: "AT_CURRENT_LOCATION_BUILDING_DOOR",
        check: Check::Function(0x1405a2680),
    },
    Condition {
        id: 59,
        name: "I can't get there.",
        check: Check::Function(0x1405a2820),
    },
    Condition {
        id: 60,
        name: "I can't get out of here.",
        check: Check::Function(0x1405a2710),
    },
    Condition {
        id: 62,
        name: "It's empty.",
        check: Check::Function(0x1405974e0),
    },
    Condition {
        id: 63,
        name: "The machine is full.",
        check: Check::Function(0x1405a2ba0),
    },
    Condition {
        id: 64,
        name: "isMachineJammed",
        check: Check::Function(0x1405a2e60),
    },
    Condition {
        id: 65,
        name: "isAnyMachineJammed",
        check: Check::Function(0x1405a2f00),
    },
    Condition {
        id: 66,
        name: "haveSomeResourcesFrom",
        check: Check::Function(0x140597530),
    },
    Condition {
        id: 67,
        name: "It's out of resources.",
        check: Check::Function(0x1405a45f0),
    },
    Condition {
        id: 68,
        name: "HAVE_SOME_LOOT_FROM",
        check: Check::Function(0x140597530),
    },
    Condition {
        id: 69,
        name: "isRouteTraceBlocked",
        check: Check::Function(0x140597620),
    },
    Condition {
        id: 70,
        name: "isObjectMine",
        check: Check::Function(0x1405a3c10),
    },
    Condition {
        id: 71,
        name: "isBuildingDamaged",
        check: Check::Function(0x140598d60),
    },
    Condition {
        id: 72,
        name: "isFullyRested",
        check: Check::Function(0x1405a0610),
    },
    Condition {
        id: 73,
        name: "machineHasFreeOperatorSlot",
        check: Check::Function(0x140598de0),
    },
    Condition {
        id: 74,
        name: "isEnemyForceDefeated",
        check: Check::Function(0x140597650),
    },
    Condition {
        id: 76,
        name: "isMessageDelivered",
        check: Check::Function(0x140792190),
    },
    Condition {
        id: 77,
        name: "isCarriedDudeInBed",
        check: Check::Function(0x14060e5c0),
    },
    Condition {
        id: 78,
        name: "TARGET_IS_IMPRISONED",
        check: Check::Function(0x1405a1cc0),
    },
    Condition {
        id: 79,
        name: "I_AM_IMPRISONED",
        check: Check::Function(0x140599570),
    },
    Condition {
        id: 80,
        name: "My arm is messed up too badly to lift anything.",
        check: Check::Function(0x1405c7930),
    },
    Condition {
        id: 81,
        name: "CARRIED_DUDE_NOW_IN_CAGE",
        check: Check::Function(0x14060e5c0),
    },
    Condition {
        id: 82,
        name: "I don't have space to pick that up!",
        check: Check::Function(0x14059bd80),
    },
    Condition {
        id: 83,
        name: "isInBed",
        check: Check::Function(0x140599540),
    },
    Condition {
        id: 84,
        name: "isAtAShop",
        check: Check::Function(0x140998680),
    },
    Condition {
        id: 85,
        name: "",
        check: Check::Function(0x140998fa0),
    },
    Condition {
        id: 86,
        name: "atATown",
        check: Check::Function(0x140506d10),
    },
    Condition {
        id: 87,
        name: "allDoorsLocked",
        check: Check::Function(0x140596bd0),
    },
    Condition {
        id: 88,
        name: "isInsideTargetBuilding",
        check: Check::Function(0x140596b90),
    },
    Condition {
        id: 89,
        name: "haveSomeResourcesFromThisMachineButWantThemGoneIfPossible",
        check: Check::Function(0x1405a2c90),
    },
    Condition {
        id: 90,
        name: "DESTINATION_NOT_BLOCKED_BY_FORTIFICATIONS",
        check: Check::Function(0x1405a28a0),
    },
    Condition {
        id: 91,
        name: "AT_LOCATION_FORTIFICATION_GATE",
        check: Check::Function(0x1405a28d0),
    },
    Condition {
        id: 92,
        name: "isEnemy",
        check: Check::Function(0x1405a3e00),
    },
    Condition {
        id: 93,
        name: "It needs electricity.",
        check: Check::Function(0x140596ab0),
    },
    Condition {
        id: 94,
        name: "atDestinationTown",
        check: Check::Function(0x140507610),
    },
    Condition {
        id: 95,
        name: "isAtHomeTown",
        check: Check::Function(0x140997fe0),
    },
    Condition {
        id: 97,
        name: "CAGES_ARE_ALL_FULL",
        check: Check::Function(0x1405a1ab0),
    },
    Condition {
        id: 98,
        name: "MACHINE_OCCUPANT_IS_ALIVE",
        check: Check::Function(0x1405a1940),
    },
    Condition {
        id: 99,
        name: "isCarriedDudeDead",
        check: Check::Function(0x1405a14e0),
    },
    Condition {
        id: 100,
        name: "CARRIED_DUDE_NOW_IN_DISPOSAL_MACHINE",
        check: Check::Function(0x14060e5c0),
    },
    Condition {
        id: 101,
        name: "isLeaderDown",
        check: Check::Function(0x140997c70),
    },
    Condition {
        id: 102,
        name: "isSquadmatesFighting",
        check: Check::Function(0x14099a3e0),
    },
    Condition {
        id: 103,
        name: "My legs are messed up.",
        check: Check::Function(0x1409980c0),
    },
    Condition {
        id: 104,
        name: "using target turret",
        check: Check::Function(0x140997d30),
    },
    Condition {
        id: 105,
        name: "isMachineOutputEmpty",
        check: Check::Function(0x1405a2c40),
    },
    Condition {
        id: 106,
        name: "machineHasFullInputs",
        check: Check::Function(0x1405a2bf0),
    },
    Condition {
        id: 107,
        name: "I_AM_INSIDE_TARGET_BUILDING",
        check: Check::Function(0x14059a4b0),
    },
    Condition {
        id: 108,
        name: "strangers in house",
        check: Check::Function(0x14060e5e0),
    },
    Condition {
        id: 109,
        name: "isDialogDelivered",
        check: Check::Function(0x140792140),
    },
    Condition {
        id: 110,
        name: "isDialogDelivered",
        check: Check::Function(0x140792140),
    },
    Condition {
        id: 111,
        name: "AT_LOCATION_BUILDING_DOOR_INSIDE",
        check: Check::Function(0x140596cf0),
    },
    Condition {
        id: 112,
        name: "It's locked.",
        check: Check::Function(0x1405a02d0),
    },
    Condition {
        id: 113,
        name: "targetAtATown",
        check: Check::Function(0x140508ab0),
    },
    Condition {
        id: 114,
        name: "isWearingShackles_locked",
        check: Check::Function(0x1405a1f00),
    },
    Condition {
        id: 115,
        name: "IS_CARRYING_TARGET_HEALTHY",
        check: Check::Function(0x14059ab30),
    },
    Condition {
        id: 116,
        name: "TARGET_IS_MY_SLAVE",
        check: Check::Function(0x1405a2170),
    },
    Condition {
        id: 117,
        name: "TARGET_HAS_BEEN_LOOTED",
        check: Check::Function(0x14099a710),
    },
    Condition {
        id: 118,
        name: "TARGET_IS_UNARMED",
        check: Check::Function(0x1405a2290),
    },
    Condition {
        id: 120,
        name: "TARGET_LOOKS_LIKE_A_SLAVE",
        check: Check::Function(0x1405a6f60),
    },
    Condition {
        id: 121,
        name: "CARRYING_EXCESS_LOOT",
        check: Check::Function(0x14059b7b0),
    },
    Condition {
        id: 122,
        name: "I don't have any tools",
        check: Check::Function(0x1405989e0),
    },
    Condition {
        id: 123,
        name: "IS_WORKING",
        check: Check::Function(0x1405974d0),
    },
    Condition {
        id: 124,
        name: "isTargetEscapee",
        check: Check::Function(0x1405a2080),
    },
    Condition {
        id: 125,
        name: "TARGET_IS_CONSCIOUS",
        check: Check::Function(0x14099a480),
    },
    Condition {
        id: 126,
        name: "AT_LOCATION_BUILDING_DOOR_OUTSIDE",
        check: Check::Function(0x140596e90),
    },
    Condition {
        id: 127,
        name: "It's locked.",
        check: Check::Function(0x1405a0470),
    },
    Condition {
        id: 128,
        name: "isAlly",
        check: Check::Function(0x1405a3eb0),
    },
    Condition {
        id: 129,
        name: "TARGET_IS_ARRESTED",
        check: Check::Function(0x14059a380),
    },
    Condition {
        id: 130,
        name: "ALARMS_IN_THE_VICINITY",
        check: Check::Function(0x140599060),
    },
    Condition {
        id: 131,
        name: "atPackageTarget",
        check: Check::Function(0x140506d30),
    },
    Condition {
        id: 132,
        name: "I need a repair kit.",
        check: Check::Function(0x140598960),
    },
    Condition {
        id: 133,
        name: "I need a repair kit.",
        check: Check::Function(0x14099da00),
    },
    Condition {
        id: 134,
        name: "isChained",
        check: Check::Function(0x1405a1db0),
    },
    Condition {
        id: 135,
        name: "TARGET_IS_EATEN",
        check: Check::Function(0x1405a1940),
    },
    Condition {
        id: 136,
        name: "IS_WORKING_PRETEND",
        check: Check::Function(0x1405974d0),
    },
    Condition {
        id: 137,
        name: "INTRUDER_IS_OUTSIDE",
        check: Check::Function(0x140598ef0),
    },
    Condition {
        id: 139,
        name: "I'm busy swimming.",
        check: Check::Function(0x140598e90),
    },
    Condition {
        id: 140,
        name: "He's in the water.",
        check: Check::Function(0x140598ec0),
    },
    Condition {
        id: 141,
        name: "strangers in house P",
        check: Check::Function(0x14060e640),
    },
    Condition {
        id: 142,
        name: "isTargetRestrainedOrKO",
        check: Check::Function(0x1405a17a0),
    },
    Condition {
        id: 143,
        name: "farm has food",
        check: Check::Function(0x1408ca350),
    },
    Condition {
        id: 144,
        name: "am hungry",
        check: Check::Function(0x1408ca340),
    },
    Condition {
        id: 145,
        name: "using any turret",
        check: Check::Function(0x140997d60),
    },
    Condition {
        id: 146,
        name: "target worshiped",
        check: Check::Function(0x1408ca340),
    },
    Condition {
        id: 147,
        name: "DITCHED_ALL_RESOURCES",
        check: Check::Function(0x140999370),
    },
    Condition {
        id: 148,
        name: "LOOTED_STORABLE_ITEMS",
        check: Check::Function(0x14099a820),
    },
    Condition {
        id: 149,
        name: "gotAFoodItem",
        check: Check::Function(0x140597e20),
    },
    Condition {
        id: 150,
        name: "HAVE_SOME_BUILD_MATS",
        check: Check::Function(0x140597530),
    },
    Condition {
        id: 151,
        name: "hasSplintKit",
        check: Check::Function(0x1405988e0),
    },
    Condition {
        id: 152,
        name: "needsSplint",
        check: Check::Function(0x14099db10),
    },
    Condition {
        id: 153,
        name: "FINAL_GOT_A_KIDNAP_VICTIM",
        check: Check::Function(0x1405a3980),
    },
    Condition {
        id: 154,
        name: "FINAL_FOUND_AND_PUT_CARRIED_IN_A_CAGE",
        check: Check::Function(0x140598d00),
    },
    Condition {
        id: 155,
        name: "iHaveSomeSlaves",
        check: Check::Function(0x14059a0e0),
    },
    Condition {
        id: 156,
        name: "hasWeaponEquipped",
        check: Check::Function(0x140596a70),
    },
    Condition {
        id: 157,
        name: "hasWeaponEquipped",
        check: Check::Function(0x14060e540),
    },
    Condition {
        id: 158,
        name: "iAmBeingMeleeAttacked immediate",
        check: Check::Function(0x1404357d0),
    },
    Condition {
        id: 159,
        name: "iAmBeingMeleeAttacked general",
        check: Check::Function(0x140436720),
    },
    Condition {
        id: 160,
        name: "isWithinMyBowRange",
        check: Check::Function(0x1405a32d0),
    },
    Condition {
        id: 161,
        name: "My arm is messed up too badly.",
        check: Check::Function(0x1409984d0),
    },
    Condition {
        id: 162,
        name: "iAmProne",
        check: Check::Function(0x140598bf0),
    },
    Condition {
        id: 163,
        name: "isBuildingDestroyed",
        check: Check::Function(0x140598da0),
    },
    Condition {
        id: 164,
        name: "INTRUDER_IS_OUTSIDE_GATES",
        check: Check::Function(0x140598fa0),
    },
    Condition {
        id: 165,
        name: "crowdLimit8",
        check: Check::Function(0x140599b10),
    },
    Condition {
        id: 166,
        name: "I'm not skilled enough...",
        check: Check::Function(0x1408ca230),
    },
    Condition {
        id: 167,
        name: "I'm not skilled enough...",
        check: Check::Function(0x1408ca120),
    },
    Condition {
        id: 168,
        name: "I can't get there.",
        check: Check::Function(0x1405a2860),
    },
    Condition {
        id: 169,
        name: "isMachineInputInvalid",
        check: Check::Function(0x1405975d0),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        assert_eq!(CONDITIONS.len(), 147);
        assert!(CONDITIONS.windows(2).all(|w| w[0].id < w[1].id));
        assert_eq!(condition(7).unwrap().name, "isTargetDown");
    }
}
