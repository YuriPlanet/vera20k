//! The listener-side halves of the sim-emitted EVA lines, and the
//! `VoxClass::PlayEVA @ 0x00752700` call-site census that scopes them.
//!
//! The sim emits a named event for every gamemd site whose predicate is a sim
//! fact (`SimSoundEvent`); this module turns those into `GameSoundEvent::Eva`
//! requests by applying the gates gamemd evaluates against `PlayerPtr`
//! (`0x00A83D4C`) — the local player the sim cannot see — and the two jump
//! tables that pick a line per superweapon. Lines gamemd speaks inside the
//! click handler itself (before its `EventClass` is queued) are app events
//! and live next to the click: `crate::app::input::sidebar_eva`,
//! `crate::app::input::commands::place_ready_building_at_cursor`,
//! `crate::app::input::context_order` (rally).
//!
//! # `PlayEVA` census (75 xrefs, `get_xrefs_to 0x00752700`; plus the five
//! direct `QueueVoice @ 0x00752480` callers)
//!
//! Reachable in a stock skirmish and produced (site → line → VERA owner):
//! - `0x004D9911` `TechnoClass::Death_Announcement` → `EVA_UnitLost` (combat).
//! - `HouseClass::NotifyUnderAttack 0x004F94FB/0x004F95B3`,
//!   `UnitClass::ReceiveDamage 0x00738530` → under-attack family (combat).
//! - `HouseClass::Update 0x004F8BA0/0x004F8D14` → funds / low power
//!   (`sim::house_eva`).
//! - `StripClass::AI 0x006A8E2F` → `EVA_ConstructionComplete`;
//!   `HouseClass::Place_Production 0x004FB644` → `EVA_UnitReady`
//!   (production queue).
//! - `HouseClass::Place_Production 0x004FB377` (placement `Unlimbo` failed,
//!   `0x004FB369 CMP EBP,[PlayerPtr]`), `UnitClass::Deploy 0x0073950A`,
//!   `DisplayClass::BandBox_LeftUp 0x004ABC80` (invalid placement click,
//!   `0x004ABAAC/0x004ABABA` on Display `+0x1180/+0x1181`) →
//!   `EVA_CannotDeployHere` (sim event ×2, app click ×1).
//! - `SidebarClass::AddCameo 0x006A6415`, `StripClass::AddEntry 0x006A8837`
//!   → `EVA_NewConstructionOptions` (`[0xA8E7AC]==0`, RTTI `!= 0x1F`; app
//!   sidebar projection).
//! - `SelectClass::Action 0x006AAE39/0x006AAFA7/0x006AB007/0x006AB108/
//!   0x006AB3B1/0x006AB498/0x006AB693/0x006AB6C9` → Canceled / SelectTarget /
//!   OnHold ×2 / UnableToComply ×2 / Building|Training ×2 (app sidebar).
//! - `BuildingClass::SetRallyPoint 0x00443A69` → `EVA_NewRallyPointEstablished`
//!   (click handler; app rally order).
//! - `BuildingClass::Sell 0x00449CE5` (state 2) / `0x0044AB36` (upgrade) →
//!   `EVA_StructureSold`; `BuildingClass::ToggleRepair 0x004470B7` →
//!   `EVA_Repairing` (sim events).
//! - `BuildingClass::ChangeOwner 0x00448428/0x0044848A` (+ `0x00448459`
//!   `QueueVoice(CaptureEvaEvent)`) → TechBuildingLost / BuildingCaptured /
//!   per-type capture line (sim event).
//! - `SuperClass::AI_Ready 0x006CBE63` → `*Ready` (sim event, table below);
//!   `BuildingClass::OnConstructionComplete 0x00446995` → `*Detected` (sim
//!   event, table below); `SuperClass::Launch` ×7 → `*Activated` (landed).
//! - `HouseClass::MPlayer_Defeated 0x004FC3BC` → `EVA_PlayerDefeated` (sim
//!   event); `0x004FC2EA`, `Flag_To_Win 0x004FCBA9`, `Flag_To_Lose
//!   0x004FCDA1` → outcome lines (landed); `GameExit::BattleControlTerminated
//!   0x00686616` (landed).
//! - `AddGarrisonOccupant 0x005229C1`, `CheckAutoSellOrCivilian 0x004582D8`
//!   (`EVA_StructureAbandoned`), `InfantryClass::PerCellProcess 0x00519BC9`
//!   (bridge), `TechnoClass::AI_Update 0x006FA0CB/0x006FA139` (promotion),
//!   `LightningStorm::Process 0x0053AB11` — landed earlier.
//!
//! Reachable in a stock skirmish, prerequisite mechanism missing in VERA
//! (documented, not produced):
//! - `FootClass::OnSold 0x004D9F94` → `EVA_UnitSold` (no unit selling).
//! - `0x00448226` (primary-factory setter, `[this+0x3D3]=1` then
//!   `0x0050B6F0`) → `EVA_PrimaryBuildingSelected` (no primary factory).
//! - `BuildingClass::MissionRepairAndProduce 0x0044B973/0x0044BDC5`
//!   (`CreateRadarEvent(8)` → `EVA_UnitRepaired`), `0x0044BFEA` (available
//!   money `== 0` → `EVA_InsufficientFunds`), `0x0044C507` (`+0x41A` →
//!   `EVA_Repairing`) (no service-depot unit repair).
//! - `BuildingClass::OnSpyInfiltrate 0x00457288..0x0045758B` (infiltration
//!   family, `0x0050B6F0` on the spy owner / building owner) (no spy entry).
//! - `RadarClass::PlaceBeacon 0x00430D78` (`EVA_BeaconPlaced`), `0x00430F1B`
//!   (`CreateRadarEvent(0xB)` → `EVA_BeaconDetected`) (no beacons).
//! - `HouseClass::MakeAlly 0x004F9F35` (`EVA_AllianceRequested` string
//!   `0x82479C`), `MakeEnemy 0x004FA1D5` (`EVA_AllianceBroken`) (no in-match
//!   diplomacy).
//! - `HouseClass::RobotTanksBackOnline 0x0050E0D0`, `LostPoweredCenter
//!   0x0050E19B`, `Removed_From_Game 0x00502776` (`EVA_RobotTanksOffline`,
//!   `[0xA8E7AC]==0`) (no robot control centre).
//! - Crate handlers `0x00482EBB/0x004830AA/0x0048328A` (`EVA_UnitArmor|Speed|
//!   FirePowerUpgraded`, spoken when a local-player unit's multiplier was
//!   exactly `1.0`, `0x00482E61..0x00482E8D`) (no stat-upgrade crates).
//! - `TemporalClass::InitiateWarp 0x0071B05F` (`EVA_OreMinerUnderAttack`
//!   after `CreateRadarEvent(4)`) (no temporal erase).
//!
//! Excluded (not reachable from stock-skirmish input):
//! - `BuildingClass::ReadFromINI 0x0044FD1A`, `GoOnline 0x00452355`,
//!   `GoOffline 0x004523DE` (`EVA_BuildingOnLine/OffLine`): callers are
//!   `ReadFromINI` (map-placed player buildings), `EventClass::Execute` (the
//!   TS power-toggle event YR exposes no UI for) and `TriggerAction::Execute`.
//! - `SuperClass::AI_Charging 0x006CC179` (`*Ready` on a superweapon granted
//!   already charged, `SuperClass::Grant 0x006CB560` `+0x6E`): the
//!   building-complete grant passes 0.
//! - `QueueVoice` direct: `TriggerAction::Execute 0x006DE911`,
//!   `RadarClass::PlayRadarMovie 0x006579BB` (campaign), `FUN_00771620`
//!   (no callers), `0x005BC7CE` (unlabeled, walks the whole `[DialogList]`
//!   with a `[0xABF220]` cursor — a menu sampler, not the match loop).
//!
//! ## Dependency rules
//! - App-side; may read `sim` state and `rules`, never mutate the sim.

use crate::map::houses::{HouseAllianceMap, is_allied_with};
use crate::rules::object_type::ObjectType;
use crate::rules::superweapon_type::SuperWeaponKind;

/// `EVA_NewRallyPointEstablished` (string `0x00818E58`), spoken at
/// `0x00443A69` by the click handler.
pub(crate) const EVA_NEW_RALLY_POINT_ESTABLISHED: &str = "EVA_NewRallyPointEstablished";
/// String `0x00819030`.
pub(crate) const EVA_STRUCTURE_SOLD: &str = "EVA_StructureSold";
/// String `0x00818F18`.
pub(crate) const EVA_REPAIRING: &str = "EVA_Repairing";
/// String `0x00818FAC`.
pub(crate) const EVA_BUILDING_CAPTURED: &str = "EVA_BuildingCaptured";
/// String `0x00818FC4`.
pub(crate) const EVA_TECH_BUILDING_LOST: &str = "EVA_TechBuildingLost";
/// String `0x00824B70`.
pub(crate) const EVA_PLAYER_DEFEATED: &str = "EVA_PlayerDefeated";

/// `SuperClass::AI_Ready @ 0x006CBCA0`: `0x006CBDD7 MOV EAX,[Type+0xB4]`
/// (`Type=`), `CMP EAX,0xB ; JA skip`, then the `0x006CBEA8` jump table. Read
/// from the table bytes: 0 → `0x8424D4`, 1 → `0x8424BC`, 2 → `0x84248C`,
/// 3 → `0x842458`, 4 → `0x006CBE68` (no line), 5/6 → `0x842440`,
/// 7 → `0x842470`, 8 → `0x84242C`, 9 → `0x842414`, 10 → `0x8424A4`,
/// 11 → `0x8423FC`. `SuperClass::AI_Charging` (call site `0x006CC179`)
/// shares the same case → string map.
pub(crate) fn super_weapon_ready_event(kind: SuperWeaponKind) -> Option<&'static str> {
    Some(match kind {
        SuperWeaponKind::MultiMissile => "EVA_NuclearMissileReady",
        SuperWeaponKind::IronCurtain => "EVA_IronCurtainReady",
        SuperWeaponKind::LightningStorm => "EVA_LightningStormReady",
        SuperWeaponKind::ChronoSphere => "EVA_ChronosphereReady",
        SuperWeaponKind::ChronoWarp => return None,
        SuperWeaponKind::ParaDrop | SuperWeaponKind::AmerParaDrop => "EVA_ReinforcementsReady",
        SuperWeaponKind::PsychicDominator => "EVA_PsychicDominatorReady",
        SuperWeaponKind::SpyPlane => "EVA_SpyPlaneReady",
        SuperWeaponKind::GeneticConverter => "EVA_GeneticMutatorReady",
        SuperWeaponKind::ForceShield => "EVA_ForceShieldReady",
        SuperWeaponKind::PsychicReveal => "EVA_PsychicRevealReady",
    })
}

/// `BuildingClass::OnConstructionComplete @ 0x00445F80`: `0x0044693D MOV
/// EAX,[Type+0x16F0]` (the `SuperWeapon=` **list index**, not `Type=`),
/// `CMP EAX,0x9 ; JA skip`, `JMP [EAX*4+0x00446FC0]`. Table bytes:
/// 0 → `0x0044694F` (`0x818F00`), 1 → `0x0044695B` (`0x818EE8`),
/// 2 → `0x00446967` (`0x818ED0`, the misnamed `EVA_WeatherDeviceReady`),
/// 3 → `0x0044698B` (`0x818E78`), 4/5/6/8 → `0x0044699A` (no line),
/// 7 → `0x00446973` (`0x818EB0`), 9 → `0x0044697F` (`0x818E94`). Stock
/// `[SuperWeaponTypes]` order puts NukeSpecial, IronCurtainSpecial,
/// LightningStormSpecial, ChronoSphereSpecial, ..., PsychicDominatorSpecial
/// (7), ..., GeneticConverterSpecial (9) on exactly those slots.
pub(crate) fn super_weapon_detected_event(list_index: usize) -> Option<&'static str> {
    match list_index {
        0 => Some("EVA_NuclearSiloDetected"),
        1 => Some("EVA_IronCurtainDetected"),
        2 => Some("EVA_WeatherDeviceReady"),
        3 => Some("EVA_ChronosphereDetected"),
        7 => Some("EVA_PsychicDominatorDetected"),
        9 => Some("EVA_GeneticMutatorDetected"),
        _ => None,
    }
}

/// The listener gates of the `*Detected` block (`0x004468AD..0x00446935`),
/// evaluated by the machine that hears the line:
/// - `0x004468B3 CALL 0x0050B6F0` — the building's owner is the local player
///   → silent;
/// - `0x004468CD CALL HouseClass::IsAlliedWith(owner, PlayerPtr)` — the
///   owner counts the local player as an ally → silent;
/// - `0x004468DA MOV EAX,[0xA8B538]` — the local player was defeated
///   (`MPlayer_Defeated 0x004FC205` sets it) → silent;
/// - `0x004468E7 MOV EAX,[0xA8B238]` — `GameMode == 0` (campaign) → silent;
/// - `0x004468FA..0x00446935` — the superweapon's `AuxBuilding=` is set and
///   the building's owner owns none of it → silent.
pub(crate) fn super_weapon_detected_allowed(
    alliances: &HouseAllianceMap,
    owner_name: &str,
    local_name: &str,
    local_defeated: bool,
    game_mode_nonzero: bool,
    aux_building_satisfied: bool,
) -> bool {
    !owner_name.eq_ignore_ascii_case(local_name)
        && !is_allied_with(alliances, owner_name, local_name)
        && !local_defeated
        && game_mode_nonzero
        && aux_building_satisfied
}

/// The lines the capture block speaks on the local machine
/// (`BuildingClass::ChangeOwner 0x004483FB..0x0044848F`), in native order:
/// a `NeedsEngineer=` building says `EVA_TechBuildingLost` when the OLD
/// owner is local (`0x00448415`) and then queues the type's
/// `CaptureEvaEvent=` when the NEW owner is local (`0x0044842F..0x00448459`);
/// any other building says `EVA_BuildingCaptured` once the radar event was
/// accepted (`0x0044847C`) — for whichever side is local (the outer gate
/// `0x004483C6/0x004483D1` already required one of them to be).
pub(crate) fn capture_eva_events(
    tech_building: bool,
    radar_accepted: bool,
    capture_eva_event: Option<&str>,
    local_is_old_owner: bool,
    local_is_new_owner: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if !(local_is_old_owner || local_is_new_owner) {
        return lines;
    }
    if tech_building {
        if local_is_old_owner {
            lines.push(EVA_TECH_BUILDING_LOST.to_string());
        }
        if local_is_new_owner && let Some(event) = capture_eva_event {
            lines.push(event.to_string());
        }
    } else if radar_accepted {
        lines.push(EVA_BUILDING_CAPTURED.to_string());
    }
    lines
}

/// `BuildingClass::SetRallyPoint 0x00443A45..0x00443A5D`: the rally click
/// announces unless the factory is a `ConstructionYard=` (`Type+0x16B9`) or
/// a `ResourceDestination=` (`Type+0x5ED`, stored by
/// `TechnoTypeClass::ReadINI 0x007143FE`; stock refineries). The owner gate
/// (`0x00443A3C CALL 0x0050B6F0`) is the caller's — the click is local.
pub(crate) fn rally_point_announces(obj: &ObjectType) -> bool {
    !obj.construction_yard && !obj.resource_destination
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_table_follows_the_type_index_jump_table() {
        use SuperWeaponKind as K;
        assert_eq!(
            super_weapon_ready_event(K::MultiMissile),
            Some("EVA_NuclearMissileReady")
        );
        assert_eq!(
            super_weapon_ready_event(K::IronCurtain),
            Some("EVA_IronCurtainReady")
        );
        assert_eq!(
            super_weapon_ready_event(K::LightningStorm),
            Some("EVA_LightningStormReady")
        );
        assert_eq!(
            super_weapon_ready_event(K::ChronoSphere),
            Some("EVA_ChronosphereReady")
        );
        assert_eq!(
            super_weapon_ready_event(K::ChronoWarp),
            None,
            "case 4 jumps past PlayEVA"
        );
        assert_eq!(
            super_weapon_ready_event(K::ParaDrop),
            Some("EVA_ReinforcementsReady")
        );
        assert_eq!(
            super_weapon_ready_event(K::AmerParaDrop),
            Some("EVA_ReinforcementsReady")
        );
        assert_eq!(
            super_weapon_ready_event(K::PsychicDominator),
            Some("EVA_PsychicDominatorReady")
        );
        assert_eq!(
            super_weapon_ready_event(K::SpyPlane),
            Some("EVA_SpyPlaneReady")
        );
        assert_eq!(
            super_weapon_ready_event(K::GeneticConverter),
            Some("EVA_GeneticMutatorReady")
        );
        assert_eq!(
            super_weapon_ready_event(K::ForceShield),
            Some("EVA_ForceShieldReady")
        );
        assert_eq!(
            super_weapon_ready_event(K::PsychicReveal),
            Some("EVA_PsychicRevealReady")
        );
    }

    #[test]
    fn detected_table_follows_the_list_index_jump_table() {
        assert_eq!(
            super_weapon_detected_event(0),
            Some("EVA_NuclearSiloDetected")
        );
        assert_eq!(
            super_weapon_detected_event(1),
            Some("EVA_IronCurtainDetected")
        );
        assert_eq!(
            super_weapon_detected_event(2),
            Some("EVA_WeatherDeviceReady")
        );
        assert_eq!(
            super_weapon_detected_event(3),
            Some("EVA_ChronosphereDetected")
        );
        for silent in [4, 5, 6, 8, 10, 11, 12] {
            assert_eq!(super_weapon_detected_event(silent), None, "index {silent}");
        }
        assert_eq!(
            super_weapon_detected_event(7),
            Some("EVA_PsychicDominatorDetected")
        );
        assert_eq!(
            super_weapon_detected_event(9),
            Some("EVA_GeneticMutatorDetected")
        );
    }

    #[test]
    fn detected_listener_gates_match_native_order() {
        let mut alliances = HouseAllianceMap::new();
        assert!(super_weapon_detected_allowed(
            &alliances,
            "Russians",
            "Americans",
            false,
            true,
            true
        ));
        assert!(!super_weapon_detected_allowed(
            &alliances,
            "Americans",
            "Americans",
            false,
            true,
            true
        ));
        assert!(!super_weapon_detected_allowed(
            &alliances,
            "Russians",
            "Americans",
            true,
            true,
            true
        ));
        assert!(!super_weapon_detected_allowed(
            &alliances,
            "Russians",
            "Americans",
            false,
            false,
            true
        ));
        assert!(!super_weapon_detected_allowed(
            &alliances,
            "Russians",
            "Americans",
            false,
            true,
            false
        ));
        // One-way: only the OWNER's ally bit is read (`IsAlliedWith(owner, PlayerPtr)`).
        alliances
            .entry("RUSSIANS".to_string())
            .or_default()
            .insert("AMERICANS".to_string());
        assert!(!super_weapon_detected_allowed(
            &alliances,
            "Russians",
            "Americans",
            false,
            true,
            true
        ));
        assert!(super_weapon_detected_allowed(
            &alliances,
            "Americans",
            "Russians",
            false,
            true,
            true
        ));
    }

    #[test]
    fn capture_lines_follow_the_change_owner_block() {
        // Ordinary building: the radar result gates one line for either side.
        assert_eq!(
            capture_eva_events(false, true, None, false, true),
            vec![EVA_BUILDING_CAPTURED.to_string()]
        );
        assert_eq!(
            capture_eva_events(false, true, None, true, false),
            vec![EVA_BUILDING_CAPTURED.to_string()]
        );
        assert!(capture_eva_events(false, false, None, true, false).is_empty());
        assert!(capture_eva_events(false, true, None, false, false).is_empty());
        // Tech building: lost line for a local old owner, per-type line for a
        // local new owner, no radar-gated line at all.
        assert_eq!(
            capture_eva_events(true, false, Some("EVA_OilRefineryCaptured"), false, true),
            vec!["EVA_OilRefineryCaptured".to_string()]
        );
        assert_eq!(
            capture_eva_events(true, false, Some("EVA_OilRefineryCaptured"), true, false),
            vec![EVA_TECH_BUILDING_LOST.to_string()]
        );
        assert!(capture_eva_events(true, false, None, false, true).is_empty());
        assert!(capture_eva_events(true, true, None, false, false).is_empty());
    }

    #[test]
    fn rally_line_skips_construction_yards_and_resource_destinations() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GACNST\n1=GAREFN\n2=GAWEAP\n\
             [GACNST]\nStrength=1000\nArmor=wood\nConstructionYard=yes\n\
             [GAREFN]\nStrength=1000\nArmor=wood\nRefinery=yes\nDockUnload=yes\nResourceDestination=yes\n\
             [GAWEAP]\nStrength=1000\nArmor=wood\nFactory=UnitType\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rally test rules should parse");
        assert!(rally_point_announces(
            rules.object("GAWEAP").expect("GAWEAP")
        ));
        assert!(!rally_point_announces(
            rules.object("GACNST").expect("GACNST")
        ));
        assert!(!rally_point_announces(
            rules.object("GAREFN").expect("GAREFN")
        ));
    }
}
