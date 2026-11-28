//! Action kind names (RFC appendix section 5), for `lookup` and `name_of`
//! with name kind `ActionKind` (21). The numbers are ABI data, like
//! [`crate::PROPERTIES`].

/// Stock actions 0..149 (appendix 5.2, 146..149 with Ares) by stock number.
/// `None` marks a number with no API name (no operation, unavailable).
#[rustfmt::skip]
pub const STOCK_ACTIONS: [Option<&str>; 150] = [
    None, Some("DeclareWinner"), Some("DeclareLoser"), Some("BeginProduction"),
    Some("CreateTeam"), Some("DestroyTeams"), Some("AllToHunt"), Some("Reinforce"),
    Some("DropZoneFlare"), Some("FireSale"), Some("PlayMovie"), Some("ShowText"),
    Some("DestroyTrigger"), Some("BeginAutocreate"), Some("ChangeHouse"), Some("AllowWin"),
    Some("RevealAll"), Some("RevealAround"), Some("RevealZone"), Some("PlaySound"),
    Some("PlayTheme"), Some("PlaySpeech"), Some("ForceTrigger"), Some("TimerStart"),
    Some("TimerStop"), Some("TimerExtend"), Some("TimerShorten"), Some("TimerSet"),
    Some("SetGlobal"), Some("ClearGlobal"), Some("AutoBaseBuilding"), Some("GrowShroud"),
    Some("DestroyObject"), Some("OneTimeSuperWeapon"), Some("RepeatingSuperWeapon"),
    Some("PreferredTarget"), Some("AllChangeHouse"), Some("MakeAlly"), Some("MakeEnemy"),
    None, Some("SetVisibleMapArea"), Some("PlayAnimAt"), Some("DetonateWeaponAt"),
    Some("PlayVoxelAnimAt"), Some("IonStormStart"), Some("IonStormStop"), Some("LockInput"),
    Some("UnlockInput"), Some("CenterCamera"), None, None, Some("Reshroud"),
    Some("SpotlightBehavior"), Some("EnableTrigger"), Some("DisableTrigger"),
    Some("RadarEvent"), Some("SetLocal"), Some("ClearLocal"), Some("MeteorShower"),
    Some("ReduceOre"), Some("SellBuilding"), Some("TurnOffBuilding"), Some("TurnOnBuilding"),
    Some("DamageBridgeAt"), Some("LightFlashSmall"), Some("LightFlashMedium"),
    Some("LightFlashLarge"), Some("AnnounceWin"), Some("AnnounceLose"), Some("EndScenario"),
    Some("DestroyTag"), Some("SetAmbientStep"), Some("SetAmbientRate"),
    Some("SetAmbientLight"), Some("AiTriggersOn"), Some("AiTriggersOff"),
    Some("RatioAiTriggers"), Some("RatioAircraft"), Some("RatioInfantry"), Some("RatioUnits"),
    Some("ReinforceAt"), Some("WakeUp"), Some("WakeSleepers"), Some("WakeHarmless"),
    Some("WakeGroup"), Some("VeinGrowth"), Some("OreGrowth"), Some("IceGrowth"),
    Some("CreateParticleSystem"), Some("RemoveParticleSystems"), Some("LightningStrike"),
    Some("GoBerserk"), Some("FirestormOn"), Some("FirestormOff"), Some("IonCannonStrike"),
    Some("NukeStrike"), Some("ChemStrike"), Some("ToggleTrainCargo"),
    Some("PlaySoundAtRandomWaypoint"), Some("PlaySoundAt"), Some("PlayRadarMovie"),
    Some("ReshroudAt"), Some("LightningStormStrike"), Some("TimerText"), Some("FlashTeam"),
    Some("TalkBubble"), Some("SetTechLevel"), Some("ReinforceByChrono"), Some("CreateCrate"),
    Some("IronCurtainAt"), Some("PauseGame"), Some("EvictOccupants"), Some("JumpCamera"),
    Some("Cheer"), Some("SetSidebarTab"), Some("FlashCameo"), Some("StopSoundsAt"),
    Some("PlayRadarMovieAndPause"), Some("ClearSmudges"), Some("DestroyAll"),
    Some("DestroyAllBuildings"), Some("DestroyAllLandUnits"), Some("DestroyAllNavalUnits"),
    Some("MindControlBase"), Some("RestoreMindControlledBase"), Some("CreateBuildingAt"),
    Some("RestoreStartingTechnos"), Some("ChronoScreenEffect"), Some("TeleportAll"),
    Some("SuperWeaponSetCharge"), Some("RestoreStartingBuildings"), Some("FlashBuildings"),
    Some("SuperWeaponSetRecharge"), Some("SuperWeaponResetRecharge"),
    Some("SuperWeaponReset"), Some("SetPreferredTargetCell"),
    Some("ClearPreferredTargetCell"), Some("SetAlternateBase"), Some("ClearAlternateBase"),
    Some("RadarBlackout"), Some("SetDefensiveTargetCell"), Some("ClearDefensiveTargetCell"),
    Some("RetintRed"), Some("RetintGreen"), Some("RetintBlue"), Some("HomeCamera"),
    // Ares (unavailable without Ares)
    Some("AddAuxPower"), Some("KillDrivers"), Some("SetEvaVoice"), Some("SetGroup"),
];

/// The first atomic action number. Provisional (open question 5): atomic
/// actions are numbered from here in appendix 5.3 table order.
pub const FIRST_ATOMIC_ACTION: i32 = 1000;

/// Atomic actions (appendix 5.3) in table order; the ID is
/// [`FIRST_ATOMIC_ACTION`] plus the index.
#[rustfmt::skip]
pub const ATOMIC_ACTIONS: &[&str] = &[
    "FireWeapon", "LaunchSuperWeapon", "SpawnAnim", "AttachAnim", "Remove", "Kill", "Damage",
    "Heal", "CreateObject", "SetMission", "Teleport", "ClearTarget", "ClearDisguise",
    "ClearMindControl", "AddExperience", "AddMoney", "AllowType", "SetOwner", "AddAmmo",
    "AddPassenger", "RemovePassenger", "SetProperty", "Modify", "RemoveModifiers", "SetVar",
    "DetonateWeapon", "DetonateWarhead", "CreateProjectile", "DetonateProjectile",
    "RemoveProjectile", "EndFor", "SetScore", "AddScore", "Log", "Convert", "Push", "Module",
    "CreateTeamUnlimited", "DisbandTeam", "AddTeamMember", "RemoveTeamMember",
    // 1041 is reserved; 1042 comes from RFC 0001 section 6h
    "ApplyEffect", "RaiseEvent",
    // RFC 0001 6i: engine-side routines, reserved (later)
    "AttachRoutine", "DetachRoutine",
    // Movement (RFC 0001 0.0.23): the body facing (1045)
    "SetFacing",
    // RFC 0001 0.2.0: removes kept attached animations (1046)
    "DetachAnim",
];

/// An action kind's ID by name, ASCII case-insensitive.
pub fn action_kind_id(name: &str) -> Option<i32> {
    let stock = STOCK_ACTIONS
        .iter()
        .position(|n| n.is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .map(|i| i as i32);
    stock.or_else(|| {
        ATOMIC_ACTIONS
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
            .map(|i| FIRST_ATOMIC_ACTION + i as i32)
    })
}

/// An action kind's name. `Some(None)` for a stock number without a name
/// (0, 39, 49, 50); `None` outside both tables.
pub fn action_kind_name(id: i32) -> Option<Option<&'static str>> {
    if let Some(stock) = usize::try_from(id).ok().and_then(|i| STOCK_ACTIONS.get(i)) {
        return Some(*stock);
    }
    let atomic = usize::try_from(id.checked_sub(FIRST_ATOMIC_ACTION)?).ok()?;
    ATOMIC_ACTIONS.get(atomic).map(|name| Some(*name))
}
