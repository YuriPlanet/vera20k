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
//! `crate::app::input::commands::place_ready_building_at_cell`,
//! `crate::app::input::context_order` (rally).
//!
//! # `PlayEVA` census (75 xrefs, `get_xrefs_to 0x00752700`; plus the five
//! direct `QueueVoice @ 0x00752480` callers)
//!
//! Reachable in a stock skirmish and produced (site → line → VERA owner):
//! - `0x004D9911` `TechnoClass::Death_Announcement` → `EVA_UnitLost` (combat).
//! - `HouseClass::NotifyUnderAttack 0x004F94FB/0x004F95B3`,
//!   `UnitClass::ReceiveDamage 0x00738530` → under-attack family (combat).
//! - `TemporalClass::InitiateWarp 0x0071B05F` (harvester) and its building
//!   call `0x0071B0CA` → the same under-attack family (`sim::temporal` and
//!   the shared House dispatcher).
//! - `HouseClass::Update 0x004F8BA0/0x004F8D14` → funds / low power
//!   (`sim::house_eva`).
//! - `HouseClass::RobotTanksBackOnline 0x0050E0D0` →
//!   `EVA_RobotTanksBackOnline`; `LostPoweredCenter 0x0050E19B`,
//!   `Removed_From_Game 0x00502776` → `EVA_RobotTanksOffline` (each behind
//!   `[0xA8E7AC]==0`; `sim::powered_unit`).
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
//! - `BuildingClass::Mission_Selling 0x00449CE5` (state 2) / `0x0044AB36` (upgrade) →
//!   `EVA_StructureSold`; `BuildingClass::ToggleRepair 0x004470B7` →
//!   `EVA_Repairing` (sim events).
//! - Depot `BuildingClass::MissionRepairAndProduce 0x0044BDC5`
//!   (`CreateRadarEvent(8)` → `EVA_UnitRepaired`), `0x0044BFEA` (available
//!   money `== 0` → `EVA_InsufficientFunds`), `0x0044C507` (`+0x41A` →
//!   `EVA_Repairing`) (sim events, app listener/radar admission).
//! - `BuildingClass::ChangeOwner 0x00448428/0x0044848A` (+ `0x00448459`
//!   `QueueVoice(CaptureEvaEvent)`) → TechBuildingLost / BuildingCaptured /
//!   per-type capture line (sim event).
//! - `BuildingClass::Infiltrate 0x00457288..0x0045758B` → the infiltration
//!   family (sim event, app radar admission; [`infiltration_eva_events`]).
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
//! - Hospital `BuildingClass::MissionRepairAndProduce 0x0044B973` →
//!   `EVA_UnitHealed` (separate hospital servicing mechanism).
//! - `FootClass::OnSold 0x004D9F94` → `EVA_UnitSold` (no unit selling).
//! - `0x00448226` (primary-factory setter, `[this+0x3D3]=1` then
//!   `0x0050B6F0`) → `EVA_PrimaryBuildingSelected` (no primary factory).
//! - `RadarClass::PlaceBeacon 0x00430D78` (`EVA_BeaconPlaced`), `0x00430F1B`
//!   (`CreateRadarEvent(0xB)` → `EVA_BeaconDetected`) (no beacons).
//! - `HouseClass::MakeAlly 0x004F9F35` (`EVA_AllianceRequested` string
//!   `0x82479C`), `MakeEnemy 0x004FA1D5` (`EVA_AllianceBroken`) (no in-match
//!   diplomacy).
//! - Crate handlers `0x00482EBB/0x004830AA/0x0048328A` (`EVA_UnitArmor|Speed|
//!   FirePowerUpgraded`, spoken when a local-player unit's multiplier was
//!   exactly `1.0`, `0x00482E61..0x00482E8D`): the sim's crate pickup
//!   (`crates::pickup`) emits them as `HouseEva` for each changed house and
//!   this side applies the local test. Retail `evamd.ini` defines none of the
//!   three sections, so they resolve to silence, as natively.
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
use crate::sim::spy_infiltration::InfiltrationEffect;

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
/// String `0x008191E4`.
const EVA_RADAR_SABOTAGED: &str = "EVA_RadarSabotaged";
/// String `0x008191C4`.
const EVA_BUILDING_INF_RADAR_SABOTAGED: &str = "EVA_BuildingInfRadarSabotaged";
/// String `0x008191B0`.
const EVA_POWER_SABOTAGED: &str = "EVA_PowerSabotaged";
/// String `0x00819198`.
const EVA_BUILDING_INFILTRATED: &str = "EVA_BuildingInfiltrated";
/// String `0x0081917C`.
const EVA_ENEMY_BASE_POWERED_DOWN: &str = "EVA_EnemyBasePoweredDown";
/// String `0x0081916C`.
const EVA_CASH_STOLEN: &str = "EVA_CashStolen";
/// String `0x00819150`.
const EVA_BUILDING_INF_CASH_STOLEN: &str = "EVA_BuildingInfCashStolen";
/// String `0x00819138`.
const EVA_TECHNOLOGY_STOLEN: &str = "EVA_TechnologyStolen";
/// String `0x0081911C`.
const EVA_NEW_TECHNOLOGY_ACQUIRED: &str = "EVA_NewTechnologyAcquired";

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
        SuperWeaponKind::ChronoWarp
        | SuperWeaponKind::GenericWarhead
        | SuperWeaponKind::UnitDelivery => return None,
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
pub(crate) fn super_weapon_detected_event(
    kind: SuperWeaponKind,
    list_index: usize,
) -> Option<&'static str> {
    // Extension types have no stock construction announcement, even when a
    // mod places one in an early SuperWeaponTypes slot.
    if !(0..12).contains(&kind.native_index()) {
        return None;
    }
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

/// The lines `BuildingClass::Infiltrate` (`0x004571E0`) speaks on the local
/// machine, in native order. Every line but the owner's superweapon one
/// (`0x0045738D..0x004573B7`) needs `radar_accepted`: its radar event
/// (`0x0045722F CreateRadarEvent(9)`), created only when the owner or the
/// spy's house is local (`0x004571FE`, `0x0045720E`). The owner's Spy
/// Satellite silences both radar lines (`0x0045726C`, `0x004572A2`).
pub(crate) fn infiltration_eva_events(
    effect: InfiltrationEffect,
    radar_accepted: bool,
    local_is_owner: bool,
    local_is_infiltrator: bool,
) -> Vec<String> {
    let owner = local_is_owner && radar_accepted;
    let infiltrator = local_is_infiltrator && radar_accepted;
    let mut lines: Vec<&str> = Vec::new();
    match effect {
        InfiltrationEffect::Radar { owner_spy_sat } => {
            if owner && !owner_spy_sat {
                lines.push(EVA_RADAR_SABOTAGED);
            }
            if infiltrator && !owner_spy_sat {
                lines.push(EVA_BUILDING_INF_RADAR_SABOTAGED);
            }
        }
        // `0x004572ED..0x00457347`: the owner's line, else the spy's two.
        InfiltrationEffect::Power => {
            if owner {
                lines.push(EVA_POWER_SABOTAGED);
            } else if infiltrator {
                lines.extend([EVA_BUILDING_INFILTRATED, EVA_ENEMY_BASE_POWERED_DOWN]);
            }
        }
        InfiltrationEffect::Tech | InfiltrationEffect::Factory => {
            if owner {
                lines.push(EVA_TECHNOLOGY_STOLEN);
            }
            if infiltrator {
                lines.extend([EVA_BUILDING_INFILTRATED, EVA_NEW_TECHNOLOGY_ACQUIRED]);
            }
        }
        InfiltrationEffect::SuperWeapon => {
            if local_is_owner || infiltrator {
                lines.push(EVA_BUILDING_INFILTRATED);
            }
        }
        InfiltrationEffect::Cash => {
            if owner {
                lines.push(EVA_CASH_STOLEN);
            }
            if infiltrator {
                lines.push(EVA_BUILDING_INF_CASH_STOLEN);
            }
        }
        InfiltrationEffect::None => {}
    }
    lines.into_iter().map(str::to_string).collect()
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
        assert_eq!(super_weapon_ready_event(K::GenericWarhead), None);
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
            super_weapon_detected_event(SuperWeaponKind::MultiMissile, 0),
            Some("EVA_NuclearSiloDetected")
        );
        assert_eq!(
            super_weapon_detected_event(SuperWeaponKind::MultiMissile, 1),
            Some("EVA_IronCurtainDetected")
        );
        assert_eq!(
            super_weapon_detected_event(SuperWeaponKind::MultiMissile, 2),
            Some("EVA_WeatherDeviceReady")
        );
        assert_eq!(
            super_weapon_detected_event(SuperWeaponKind::MultiMissile, 3),
            Some("EVA_ChronosphereDetected")
        );
        for silent in [4, 5, 6, 8, 10, 11, 12] {
            assert_eq!(
                super_weapon_detected_event(SuperWeaponKind::MultiMissile, silent),
                None,
                "index {silent}"
            );
        }
        assert_eq!(
            super_weapon_detected_event(SuperWeaponKind::MultiMissile, 7),
            Some("EVA_PsychicDominatorDetected")
        );
        assert_eq!(
            super_weapon_detected_event(SuperWeaponKind::MultiMissile, 9),
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
    fn unit_delivery_superweapon_has_no_native_eva_even_in_a_stock_list_slot() {
        for index in 0..12 {
            assert_eq!(
                super_weapon_detected_event(SuperWeaponKind::UnitDelivery, index),
                None
            );
        }
        assert_eq!(
            super_weapon_ready_event(SuperWeaponKind::UnitDelivery),
            None
        );
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

    /// Supplied clear terrain, a stock ore patch and an idle healthy Yuri
    /// base, with actual layered Battle/XMP03T4 rules and fixed retail art.
    /// No type/weapon or Temporal state is patched; the attack reaches its
    /// ordinary projectile.
    fn retail_temporal_warning_runtime(
        target_type: &str,
    ) -> Option<(
        crate::sim::runtime::SimRuntime,
        crate::sim::intern::InternedId,
        crate::sim::intern::InternedId,
        u64,
        u64,
    )> {
        use crate::rules::overlay_types::OverlayTypeRegistry;
        use crate::sim::house_state::HouseState;
        use crate::sim::overlay_grid::OverlayGrid;
        use crate::sim::runtime::{SimResources, SimRuntime};
        use crate::sim::world::Simulation;

        let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")?;
        let overlays =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        let rules = retail.rules;
        let mut sim = Simulation::with_seed(0x4F93_E000);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .short_game = false;
        let local = sim.intern("YuriCountry");
        let enemy = sim.intern("Americans");
        for owner in [local, enemy] {
            let side = rules
                .country_side_index(sim.interner().resolve(owner))
                .expect("the stock country has a Side")
                .0;
            let mut house = HouseState::new(owner, side, Some(owner), true, 50_000, 10);
            house.project_country_mults(&rules, sim.interner());
            sim.houses_mut_for_test().register(house);
        }
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        // A stock deployed Slave Miner packs up when its short scan finds
        // no ore (SlaveManager6B0062). Supply map-like ore east of the
        // target's 2x2 foundation, through the shared overlay/recalc owner,
        // so ordinary slaves work while the real attacker closes in.
        let ore_id = overlays.id_for_name("TIB01").expect("stock ore overlay");
        let mut ore = OverlayGrid::new(32, 32);
        for ry in 19..=21 {
            for rx in 23..=25 {
                ore.place_overlay(rx, ry, ore_id, 11);
                ore.recalculate_runtime_cell(
                    sim.resolved_terrain_mut_for_test()
                        .as_mut()
                        .expect("arena terrain"),
                    &overlays,
                    (rx, ry),
                );
            }
        }
        *sim.overlay_grid_mut_for_test() = Some(ore);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        sim.fog_mut_for_test().reveal_all_for_owner(local);
        for (kind, rx) in [("YACNST", 4), ("YAPOWR", 12)] {
            sim.spawn_object_at_height(kind, "YuriCountry", rx, 4, 0, 0, &rules)
                .unwrap_or_else(|| panic!("healthy stock base prerequisite {kind}"));
        }
        let target = sim
            .spawn_object_at_height(target_type, "YuriCountry", 20, 20, 0, 0, &rules)
            .unwrap_or_else(|| panic!("healthy stock target {target_type}"));
        // Outside the supplied base/target's sight during settlement. The
        // real Attack command must bring this stock Teleport locomotor in.
        let attacker = sim
            .spawn_object_at_height("CLEG", "Americans", 6, 20, 0, 0, &rules)
            .expect("stock hostile Chrono Legionnaire");
        let mut resources = SimResources::empty();
        resources.rules = rules;
        resources.overlay_registry = overlays;
        Some((
            SimRuntime {
                simulation: sim,
                resources,
            },
            local,
            enemy,
            target,
            attacker,
        ))
    }

    /// Original Temporal71B0CA -> House4F9491..4F9500 selects a deployed
    /// Slave Miner's ore-miner line and skips the base siren. The ordinary
    /// YAPOWR control keeps its base line/siren. This tests real attack/frame
    /// producers and the existing app dispatcher; native movement cadence,
    /// radar-array dedupe and physical audio output are separate evidence.
    #[test]
    fn retail_temporal_deployed_miner_warning_and_release_use_the_miner_cue() {
        use crate::app::match_runtime::sound_dispatch::dispatch_sim_sound_events;
        use crate::audio::events::{GameSoundEvent, SoundEventQueue};
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::radar::{RadarEventRequest, RadarEventType};
        use crate::sim::world::{FrameEffects, SimSoundEvent, TickLane};

        for (target_type, expected_miner, expected_radar, expected_eva, expected_siren) in [
            (
                "YAREFN",
                true,
                RadarEventType::HarvesterUnderAttack,
                "EVA_OreMinerUnderAttack",
                false,
            ),
            (
                "YAPOWR",
                false,
                RadarEventType::BaseUnderAttack,
                "EVA_OurBaseIsUnderAttack",
                true,
            ),
        ] {
            let Some((mut runtime, local, enemy, target, attacker)) =
                retail_temporal_warning_runtime(target_type)
            else {
                return;
            };
            let health = runtime
                .simulation
                .entities()
                .get(target)
                .unwrap()
                .health
                .current();
            for _ in 0..8 {
                let due = runtime.simulation.take_due_commands();
                let frame = runtime
                    .advance_frame(&due, 67, TickLane::Ordinary, FrameEffects::default())
                    .expect("settled retail base frame");
                assert!(
                    frame
                        .sound_events
                        .iter()
                        .all(|event| { !matches!(event, SimSoundEvent::UnderAttack { .. }) })
                );
            }
            assert!(
                !runtime
                    .simulation
                    .entities()
                    .get(target)
                    .unwrap()
                    .is_warped_out()
            );
            let attack = CommandEnvelope::new(
                enemy,
                runtime.simulation.session().tick() + 1,
                Command::Attack {
                    attacker_id: attacker,
                    target_id: target,
                },
            );
            runtime.simulation.queue_command(attack.clone());
            let mut attack_dispatched = false;
            let mut notice_seen = false;
            for _ in 0..1024 {
                let due = runtime.simulation.take_due_commands();
                let mut frame = runtime
                    .advance_frame(&due, 67, TickLane::Ordinary, FrameEffects::default())
                    .expect("ordinary retail Temporal attack frame");
                attack_dispatched |= frame
                    .take_admitted_commands()
                    .iter()
                    .any(|command| command.payload == attack.payload);
                let Some((miner, radar)) = frame.sound_events.iter().find_map(|event| {
                    if let SimSoundEvent::UnderAttack {
                        owner,
                        miner,
                        radar,
                        ..
                    } = event
                        && *owner == local
                    {
                        Some((*miner, *radar))
                    } else {
                        None
                    }
                }) else {
                    continue;
                };
                assert!(
                    attack_dispatched,
                    "the real Attack event must reach its receiver"
                );
                let victim = runtime.simulation.entities().get(target).unwrap();
                assert_eq!(victim.temporal.chain_head(), Some(attacker));
                assert_eq!(victim.health.current(), health);
                assert_eq!(victim.owner(), local);
                assert_eq!(
                    (miner, radar.event_type),
                    (expected_miner, expected_radar),
                    "{target_type}: the actual first-head notice must use HouseNotify's stock subtype"
                );

                // Use the native control's accepted first-event outcome at
                // this app boundary. The real requests are recorded below;
                // this does not substitute another radar/dedupe algorithm.
                let radar_accepted = true;
                let mut admitted = Vec::<RadarEventRequest>::new();
                let mut output = SoundEventQueue::new();
                dispatch_sim_sound_events(
                    frame.sound_events.iter().cloned(),
                    &runtime.simulation,
                    &runtime.resources.rules,
                    Some("YuriCountry"),
                    &mut |request| {
                        admitted.push(request);
                        radar_accepted
                    },
                    &mut output,
                );
                assert_eq!(admitted, [radar], "one real local radar admission request");
                let sounds = output.drain();
                assert!(sounds.iter().any(|event| {
                    matches!(event, GameSoundEvent::Eva { event, .. } if event == expected_eva)
                }));
                let wrong_eva = if expected_miner {
                    "EVA_OurBaseIsUnderAttack"
                } else {
                    "EVA_OreMinerUnderAttack"
                };
                assert!(!sounds.iter().any(|event| {
                    matches!(event, GameSoundEvent::Eva { event, .. } if event == wrong_eva)
                }));
                assert_eq!(
                    sounds.iter().any(|event| {
                        matches!(event, GameSoundEvent::BaseUnderAttackSfx { .. })
                    }),
                    expected_siren,
                    "{target_type}: the ore-miner jump must skip the shared base siren"
                );
                notice_seen = true;
                break;
            }
            assert!(
                notice_seen,
                "the stock {target_type} must receive the real beam"
            );

            // A retained first head does not announce again on each warped
            // object visit. Stop releases through the ordinary Attack idle
            // transition, before the victim can be erased.
            for _ in 0..4 {
                let due = runtime.simulation.take_due_commands();
                let frame = runtime
                    .advance_frame(&due, 67, TickLane::Ordinary, FrameEffects::default())
                    .expect("held first-head frame");
                assert!(
                    frame
                        .sound_events
                        .iter()
                        .all(|event| { !matches!(event, SimSoundEvent::UnderAttack { .. }) })
                );
                assert!(
                    runtime
                        .simulation
                        .entities()
                        .get(target)
                        .unwrap()
                        .is_warped_out()
                );
            }
            runtime.simulation.queue_command(CommandEnvelope::new(
                enemy,
                runtime.simulation.session().tick() + 1,
                Command::Stop {
                    entity_id: attacker,
                },
            ));
            let mut released = false;
            for _ in 0..128 {
                let due = runtime.simulation.take_due_commands();
                let frame = runtime
                    .advance_frame(&due, 67, TickLane::Ordinary, FrameEffects::default())
                    .expect("ordinary Stop/release frame");
                assert!(
                    frame
                        .sound_events
                        .iter()
                        .all(|event| { !matches!(event, SimSoundEvent::UnderAttack { .. }) })
                );
                let victim = runtime
                    .simulation
                    .entities()
                    .get(target)
                    .expect("not erased");
                assert_eq!(victim.owner(), local);
                assert_eq!(victim.health.current(), health);
                if !victim.is_warped_out() {
                    assert!(victim.building_online());
                    released = true;
                    break;
                }
            }
            assert!(
                released,
                "Stop must release and preserve the stock {target_type}"
            );
        }
    }
}
