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

/// New events in appendix order, the number being [`FIRST_DELIVERED_EVENT`]
/// plus the index: delivered events (4.3, 1000..1025), polling events (4.4,
/// 1026..1030), decision-only events (4.5, 1031..1036), the reserved
/// 1037..1039, then RFC 0001 section 6h (1040..1042).
#[rustfmt::skip]
pub const DELIVERED_EVENTS: &[&str] = &[
    "TakeDamage", "HitByWarhead", "AttackedBy", "FatalDamage", "Killed", "Fire", "SpawnFire",
    "HitTarget", "TargetChanged", "ProjectileIncoming", "Detonated", "Birth",
    "TransportEntered", "TransportExited", "PassengerAdded", "PassengerRemoved", "Commanded",
    "Crushed", "HouseTransfer", "HouseTransferred", "ModifierStart", "ModifierExpire",
    "ModifierStacks", "Timer", "SuperWeaponLaunched", "ProductionCancelled",
    // 4.4
    "PropertyPoll", "HousePowerPoll", "ModifierCountPoll", "DistancePoll", "VariablePoll",
    // 4.5
    "ProductionAllowed", "ProductionQueued", "Defeat", "CommandAllowed", "Launch",
    "ProjectileFrame",
    // 1037..1039: reserved and the preview shield event, not raised here
    "OnBehavior", "OnBehaviorExpire", "DamageIncoming",
    // RFC 0001 section 6h: the module event and the start-of-match
    // records (1040..1042)
    "ModuleEvent", "IniEntry", "IniKey",
    // RFC 0001 6i: an order refused at execution (1043)
    "OrderRefused",
    // RFC 0001 0.0.22 (1044..1055): decision-only (4.5) and delivered (4.3)
    "FireAllowed", "Retaliate", "DestinationSet", "CrushAllowed", "MissionChanged",
    "HarvestCell", "DeployAllowed", "Deployed", "Promoted", "ExperienceGained", "Cloaked",
    "Decloaked",
];

/// An event kind's number by name, ASCII case-insensitive.
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
