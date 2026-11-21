//! Event kind names (RFC appendix section 4), for INI trigger sections and
//! name kind `EventKind` (20). The numbers are ABI data, like
//! [`crate::action_kinds`].

/// Stock events 0..61 and Ares events 62..88 (appendix 4.1, 4.2) by number.
/// `None` marks the private event 0.
#[rustfmt::skip]
pub const STOCK_EVENTS: [Option<&str>; 89] = [
    None, Some("EnteredBy"), Some("SpiedBy"), Some("ThievedBy"), Some("DiscoveredByPlayer"),
    Some("HouseDiscovered"), Some("AttackedByAnyHouse"), Some("DestroyedByAnyHouse"),
    Some("AnyEvent"), Some("AllUnitsDestroyed"), Some("AllBuildingsDestroyed"),
    Some("AllDestroyed"), Some("CreditsExceed"), Some("ElapsedTime"),
    Some("MissionTimerExpired"), Some("BuildingsDestroyedCount"),
    Some("UnitsDestroyedCount"), Some("NoFactoriesLeft"), Some("CiviliansEvacuated"),
    Some("BuildBuildingType"), Some("BuildUnitType"), Some("BuildInfantryType"),
    Some("BuildAircraftType"), Some("TeamLeavesMap"), Some("ZoneEntryBy"),
    Some("CrossesHorizontalLine"), Some("CrossesVerticalLine"), Some("GlobalIsSet"),
    Some("GlobalIsClear"), Some("DestroyedByAnythingNotInfiltrate"), Some("LowPower"),
    Some("BridgeDestroyed"), Some("BuildingExists"), Some("SelectedByPlayer"),
    Some("ComesNearWaypoint"), Some("EnemyInSpotlight"), Some("LocalIsSet"),
    Some("LocalIsClear"), Some("FirstDamagedCombat"), Some("HalfHealthCombat"),
    Some("QuarterHealthCombat"), Some("FirstDamagedAny"), Some("HalfHealthAny"),
    Some("QuarterHealthAny"), Some("AttackedByHouse"), Some("AmbientLightBelow"),
    Some("AmbientLightAbove"), Some("ElapsedScenarioTime"), Some("DestroyedByAnything"),
    Some("PickupCrate"), Some("PickupCrateAny"), Some("RandomDelay"), Some("CreditsBelow"),
    Some("SpyAsHouse"), Some("SpyAsInfantry"), Some("NavalUnitsDestroyed"),
    Some("LandUnitsDestroyed"), Some("BuildingDoesNotExist"), Some("PowerFull"),
    Some("EnteredOrOverflownBy"), Some("TechTypeExists"), Some("TechTypeDoesNotExist"),
    // Ares (unavailable without Ares)
    Some("UnderEmp"), Some("UnderEmpByHouse"), Some("RemovedFromEmp"),
    Some("RemovedFromEmpByHouse"), Some("InSpotlight"), Some("DriverKilled"),
    Some("DriverKilledByHouse"), Some("VehicleTaken"), Some("VehicleTakenByHouse"),
    Some("Abducted"), Some("AbductedByHouse"), Some("AbductsSomething"),
    Some("AbductsSomethingOfHouse"), Some("SuperWeaponActivated"),
    Some("SuperWeaponDeactivated"), Some("SuperWeaponNearWaypoint"),
    Some("HouseReverseEngineeredType"), Some("SomethingReverseEngineered"),
    Some("TypeReverseEngineered"), Some("HouseOwnsAtLeast"), Some("HouseOwnsAtMost"),
    Some("AttackedOrDestroyedByAnybody"), Some("AttackedOrDestroyedByHouse"),
    Some("DestroyedByHouse"), Some("TypeCountAtMost"), Some("HouseNoKeepAliveObjects"),
    Some("HouseNoKeepAliveBuildings"),
];

/// The first new delivered event number. Provisional (open question 5):
/// appendix 4.3 is numbered from here in table order.
pub const FIRST_DELIVERED_EVENT: i32 = 1000;

/// New delivered events (appendix 4.3) in table order; the number is
/// [`FIRST_DELIVERED_EVENT`] plus the index.
#[rustfmt::skip]
pub const DELIVERED_EVENTS: &[&str] = &[
    "TakeDamage", "HitByWarhead", "AttackedBy", "FatalDamage", "Killed", "Fire", "SpawnFire",
    "HitTarget", "TargetChanged", "ProjectileIncoming", "Detonated", "Birth",
    "TransportEntered", "TransportExited", "PassengerAdded", "PassengerRemoved", "Commanded",
    "Crushed", "HouseTransfer", "HouseTransferred", "ModifierStart", "ModifierExpire",
    "ModifierStacks", "Timer", "SuperWeaponLaunched", "ProductionCancelled",
];

/// An event kind's number by name, ASCII case-insensitive. The polling (4.4)
/// and decision-only (4.5) events have no numbers yet.
pub fn event_kind_id(name: &str) -> Option<i32> {
    let stock = STOCK_EVENTS
        .iter()
        .position(|n| n.is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .map(|i| i as i32);
    stock.or_else(|| {
        DELIVERED_EVENTS
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
            .map(|i| FIRST_DELIVERED_EVENT + i as i32)
    })
}
