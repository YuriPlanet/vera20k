//! Phobos client settings, read from the imported player profile.
//!
//! Phobos reads `RA2MD.INI [Phobos]` directly each launch. VERA imports that
//! file once into `VERA.INI` and keeps it the writable profile, so the same
//! section arrives here from `profile_file`. Only settings that gate a
//! compiled extension live here; retail options keep `options_profile`.

use std::path::Path;

use crate::rules::ini_parser::IniFile;

/// The `[Phobos]` client switches this build reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhobosClientSettings {
    /// `[Phobos] ShowPlacementPreview=`, source default `yes` (b36 key; the
    /// old `ShowBuildingPlacementPreview` is not read).
    pub(crate) show_placement_preview: bool,
    /// `[Phobos] DigitalDisplay.Enable=`, default `no`.
    pub(crate) digital_display_enabled: bool,
    /// `[Phobos] SuperWeaponSidebar.RequiredSignificance=`, default `0`.
    pub(crate) superweapon_sidebar_required_significance: i32,
    /// `[Phobos] ShowFlashOnSelecting=`, default `no`: the client switch for
    /// `[AudioVisual] SelectionFlashDuration`.
    pub(crate) show_flash_on_selecting: bool,
    /// `[Phobos] ShowPowerPlantEnhancerRange=`, default `no`: the client switch
    /// for the placement-time enhancer rings; the rules switch must also be on.
    pub(crate) show_power_plant_enhancer_range: bool,
    /// `[Phobos] ShowDesignatorRange=`, default `no` (`Phobos.INI.cpp:133`,
    /// static `:64`): the client switch for the armed superweapon's
    /// designator/inhibitor range rings; the `[AudioVisual]` and the armed
    /// `[SuperWeaponType]` switches must also be on.
    pub(crate) show_designator_range: bool,
    /// `[Phobos] ShowPowerDelta=`, default `yes` (`Phobos.INI.cpp:108`): the
    /// client half of the power-delta sidebar indicator. The `uimd.ini` switch
    /// must also be on.
    pub(crate) show_power_delta: bool,
    /// `[Phobos] ShowHarvesterCounter=`, default `yes` (`Phobos.INI.cpp:109`):
    /// the client half of the harvester counter; the `uimd.ini` switch must be
    /// on too.
    pub(crate) show_harvester_counter: bool,
    /// `[Phobos] ShowWeedsCounter=`, default `yes` (`Phobos.INI.cpp:110` reads
    /// with `true`; the `false` static initializer at `:71` is overwritten):
    /// the client half of the weeds counter.
    pub(crate) show_weeds_counter: bool,
    /// `[Phobos] PriorityDeployFiltering=`, default `yes`
    /// (`Phobos.INI.cpp:46` static, read `:94`): drops a
    /// `LowDeployPriority=yes` techno from a multi-object deploy when the
    /// selection also holds one that is not low priority.
    /// Phobos `[Phobos] PriorityDeployFiltering=` (source default `yes`): the
    /// gate on `LowDeployPriority` in the selection-wide deploy walk
    /// (`Ext/Techno/Hooks.cpp`, `0x730D0F`).
    pub(crate) priority_deploy_filtering: bool,
    /// Phobos `[Phobos] PrioritySelectionFiltering=` (`Phobos.INI.cpp:93`,
    /// source default `yes`): the gate on `LowSelectionPriority` in the band
    /// walk (`Misc/Selection.cpp:169`).
    pub(crate) priority_selection_filtering: bool,
    /// `[Phobos] TypeSelectUseIFVMode=`, default `yes`
    /// (`Phobos.INI.cpp:95`): a type-selection click enters IFV mode instead of
    /// a plain move/select.
    pub(crate) type_select_use_ifv_mode: bool,
    /// `[Phobos] MessageDisplayInCenter=`, default `no`
    /// (`Phobos.INI.cpp:98`): the center-message layout switch.
    pub(crate) message_display_in_center: bool,
    /// `[Phobos] MessageDisplayInCenter.BoardOpacity=`, int default 40
    /// (`Phobos.INI.cpp:99`, static `:57`): `FillRectTrans`'s 0..100 percent
    /// for the message row boards (`Misc/MessageColumn.cpp:299, :1112`).
    pub(crate) message_display_in_center_board_opacity: i32,
    /// `[Phobos] MessageDisplayInCenter.LabelsCount=`, int default 6
    /// (`Phobos.INI.cpp:100`, static `:58`): the column's visible label cap
    /// (`Math::clamp(..., 1, maxLines)` at `MessageColumn.cpp:463`).
    pub(crate) message_display_in_center_labels_count: i32,
    /// `[Phobos] MessageDisplayInCenter.RecordsCount=`, int default 12
    /// (`Phobos.INI.cpp:101`, static `:59`): the column's record cap
    /// (`Math::clamp(..., 4, maxLines)` at `MessageColumn.cpp:462`).
    pub(crate) message_display_in_center_records_count: i32,
    /// `[Phobos] SaveGameOnScenarioStart=`, default `yes`
    /// (`Phobos.INI.cpp:106`): a campaign/mission start writes a save before
    /// play begins.
    pub(crate) save_game_on_scenario_start: bool,
    /// `[Phobos] ShowBriefing=`, default `yes` (`Phobos.INI.cpp:107`,
    /// static `:68`): the client gate of the scenario-start briefing dialog
    /// (`DEFINE_HOOK(0x683E41, ScenarioClass_Start_ShowBriefing, 0x6)`).
    /// VERA has no scenario-start briefing dialog pass yet; see
    /// `docs/phobos-show-briefing.md`.
    pub(crate) show_briefing: bool,
    /// `[Phobos] HideLaserTrailEffects=`, default `no`
    /// (`Phobos.INI.cpp:73` static, read `:112`): suppresses laser-trail
    /// drawing; the read default overrides the static `true`.
    pub(crate) hide_laser_trail_effects: bool,
    /// `[Phobos] HideLightFlashEffects=`, default `no`
    /// (`Phobos.INI.cpp:72` static, read `:111`): skips the combat light
    /// flash (`DoFlash @ 0x48A62E`, `Misc/Hooks.LightEffects.cpp:51-56`).
    pub(crate) hide_light_flash_effects: bool,
    /// `[Phobos] HideShakeEffects=`, default `no`
    /// (`Phobos.INI.cpp:74` static, read `:113`): skips the screen-shake
    /// writes (`Ext/Bullet/Hooks.DetonateLogics.cpp:33`).
    pub(crate) hide_shake_effects: bool,
    /// `[Phobos] ShowGameTime=`, default `no` (`Phobos.INI.cpp:116`): draws the
    /// elapsed-game-time clock.
    pub(crate) show_game_time: bool,
    /// `[Phobos] ShowGameTime.BoardOpacity=`, int default 40
    /// (`Phobos.INI.cpp:117`): `FillRectTrans(&rectB, &color, opacity)` in
    /// `GScreenClass_DrawText @ 0x4F4583` (`src/Phobos.cpp:392`).
    pub(crate) show_game_time_board_opacity: i32,
    /// `[Phobos] EnableSelectBox=`, default `no` (`Phobos.INI.cpp:104`): the
    /// client switch for the `[SelectBoxTypes]` custom selection box.
    pub(crate) enable_select_box: bool,
    /// `[Phobos] RealTimeTimers=`, default `no` (`Phobos.INI.cpp:102`): the
    /// tactical timer lines print wall-clock time instead of the stock
    /// `frames / 15` (`PrintTimerOnTactical_Start` at `0x006D4B50`).
    pub(crate) real_time_timers: bool,
    /// `[Phobos] RealTimeTimers.Adaptive=`, default `no` (`Phobos.INI.cpp:103`):
    /// the wall-clock divisor follows the measured frame rate instead of the
    /// GameSpeed table.
    pub(crate) real_time_timers_adaptive: bool,
    /// `[Phobos] ToolTipDescriptions=`, default `yes` (`Phobos.INI.cpp:91`):
    /// the gate of the extended tooltip's `UIDescription` line
    /// (`Misc/PhobosToolTip.cpp:22-32`, reached from `0x6A9316` and
    /// `0x4AE51E`); a techno or superweapon type with a non-empty
    /// `UIDescription=` appends it as the tip's last line.
    pub(crate) tool_tip_descriptions: bool,
    /// `[Phobos] ToolTipBlur=`, default `no` (`Phobos.INI.cpp:92`): the gate
    /// of `SurfaceExt::BlurRect` behind the tooltip background
    /// (`Misc/PhobosToolTip.cpp:439`, hook `0x478FDC`). RESIDUAL: the blur
    /// itself needs a new GPU pass (native blurs the composited surface
    /// pixels, `Ext/Surface/Body.cpp:9`), and its size comes from the
    /// `ToolTip.Background.BlurSize` family this package leaves unported —
    /// the rules global defaults `0.0`, which native early-returns on, so
    /// the switch alone changes no pixels.
    pub(crate) tool_tip_blur: bool,
    /// `[Phobos] MessageApplyHoverState=`, default `no` (`Phobos.INI.cpp:97`):
    /// the pointer over the in-game message stack holds `OnOldMessages`,
    /// which stops `MessageListClass::Manage` expiring the labels
    /// (`0x4F43BE`, `0x55DDA0`).
    pub(crate) message_apply_hover_state: bool,
    /// `[Phobos] CampaignDefaultGameSpeed=`, engine speed index
    /// (`6 - ReadInteger(4)` at `Phobos.INI.cpp:120`): 2 when the computed
    /// index leaves 0..=6 (`:121-124`), i.e. the stock `GS4` forced when speed
    /// control is off.
    pub(crate) campaign_default_game_speed: i32,
}

impl Default for PhobosClientSettings {
    /// The source defaults, also used when there is no readable profile and by
    /// sealed captures.
    fn default() -> Self {
        Self {
            show_placement_preview: true,
            digital_display_enabled: false,
            superweapon_sidebar_required_significance: 0,
            show_flash_on_selecting: false,
            show_power_plant_enhancer_range: false,
            show_designator_range: false,
            show_power_delta: true,
            show_harvester_counter: true,
            show_weeds_counter: true,
            priority_deploy_filtering: true,
            priority_selection_filtering: true,
            type_select_use_ifv_mode: true,
            message_display_in_center: false,
            message_display_in_center_board_opacity: 40,
            message_display_in_center_labels_count: 6,
            message_display_in_center_records_count: 12,
            save_game_on_scenario_start: true,
            show_briefing: true,
            hide_laser_trail_effects: false,
            hide_light_flash_effects: false,
            hide_shake_effects: false,
            show_game_time: false,
            show_game_time_board_opacity: 40,
            enable_select_box: false,
            real_time_timers: false,
            real_time_timers_adaptive: false,
            tool_tip_descriptions: true,
            tool_tip_blur: false,
            message_apply_hover_state: false,
            campaign_default_game_speed: 2,
        }
    }
}

/// Read the switches from the profile in `settings_dir`.
pub(crate) fn read(settings_dir: &Path) -> PhobosClientSettings {
    let defaults = PhobosClientSettings::default();
    let path = super::profile_file::path(settings_dir);
    let Ok(bytes) = std::fs::read(&path) else {
        return defaults;
    };
    let Ok(ini) = IniFile::from_bytes(&bytes) else {
        log::warn!("Could not parse {} for Phobos settings", path.display());
        return defaults;
    };
    let Some(section) = ini.section("Phobos") else {
        return defaults;
    };
    PhobosClientSettings {
        show_placement_preview: section
            .read_bool("ShowPlacementPreview", defaults.show_placement_preview),
        digital_display_enabled: section
            .read_bool("DigitalDisplay.Enable", defaults.digital_display_enabled),
        superweapon_sidebar_required_significance: section.read_int(
            "SuperWeaponSidebar.RequiredSignificance",
            defaults.superweapon_sidebar_required_significance,
        ),
        show_flash_on_selecting: section
            .read_bool("ShowFlashOnSelecting", defaults.show_flash_on_selecting),
        show_power_plant_enhancer_range: section.read_bool(
            "ShowPowerPlantEnhancerRange",
            defaults.show_power_plant_enhancer_range,
        ),
        show_designator_range: section
            .read_bool("ShowDesignatorRange", defaults.show_designator_range),
        show_power_delta: section.read_bool("ShowPowerDelta", defaults.show_power_delta),
        show_harvester_counter: section
            .read_bool("ShowHarvesterCounter", defaults.show_harvester_counter),
        show_weeds_counter: section.read_bool("ShowWeedsCounter", defaults.show_weeds_counter),
        priority_deploy_filtering: section.read_bool(
            "PriorityDeployFiltering",
            defaults.priority_deploy_filtering,
        ),
        // `Phobos.INI.cpp:93`: `ReadBool(..., "PrioritySelectionFiltering", true)`.
        priority_selection_filtering: section.read_bool(
            "PrioritySelectionFiltering",
            defaults.priority_selection_filtering,
        ),
        type_select_use_ifv_mode: section
            .read_bool("TypeSelectUseIFVMode", defaults.type_select_use_ifv_mode),
        message_display_in_center: section
            .read_bool("MessageDisplayInCenter", defaults.message_display_in_center),
        // `Phobos.INI.cpp:99-101`: three independent ReadInteger calls on the
        // same `[Phobos]` section; an absent key keeps its static default.
        message_display_in_center_board_opacity: section.read_int(
            "MessageDisplayInCenter.BoardOpacity",
            defaults.message_display_in_center_board_opacity,
        ),
        message_display_in_center_labels_count: section.read_int(
            "MessageDisplayInCenter.LabelsCount",
            defaults.message_display_in_center_labels_count,
        ),
        message_display_in_center_records_count: section.read_int(
            "MessageDisplayInCenter.RecordsCount",
            defaults.message_display_in_center_records_count,
        ),
        save_game_on_scenario_start: section.read_bool(
            "SaveGameOnScenarioStart",
            defaults.save_game_on_scenario_start,
        ),
        show_briefing: section.read_bool("ShowBriefing", defaults.show_briefing),
        hide_laser_trail_effects: section
            .read_bool("HideLaserTrailEffects", defaults.hide_laser_trail_effects),
        hide_light_flash_effects: section
            .read_bool("HideLightFlashEffects", defaults.hide_light_flash_effects),
        hide_shake_effects: section.read_bool("HideShakeEffects", defaults.hide_shake_effects),
        show_game_time: section.read_bool("ShowGameTime", defaults.show_game_time),
        show_game_time_board_opacity: section.read_int(
            "ShowGameTime.BoardOpacity",
            defaults.show_game_time_board_opacity,
        ),
        enable_select_box: section.read_bool("EnableSelectBox", defaults.enable_select_box),
        // `Phobos.INI.cpp:102-103`: both default to `false`.
        real_time_timers: section.read_bool("RealTimeTimers", defaults.real_time_timers),
        real_time_timers_adaptive: section.read_bool(
            "RealTimeTimers.Adaptive",
            defaults.real_time_timers_adaptive,
        ),
        // `Phobos.INI.cpp:91`: `ReadBool(..., "ToolTipDescriptions", true)`.
        tool_tip_descriptions: section
            .read_bool("ToolTipDescriptions", defaults.tool_tip_descriptions),
        tool_tip_blur: section.read_bool("ToolTipBlur", defaults.tool_tip_blur),
        message_apply_hover_state: section
            .read_bool("MessageApplyHoverState", defaults.message_apply_hover_state),
        // `Phobos.INI.cpp:120-124`: index `6 - key`, then the same 0..=6 guard
        // that forces the stock `GS4` speed 2 when the read leaves the range.
        campaign_default_game_speed: {
            let speed = 6 - section.read_int("CampaignDefaultGameSpeed", 4);
            if (0..=6).contains(&speed) { speed } else { 2 }
        },
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    static NEXT_PROFILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn profile_with(section: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vera-phobos-settings-{}-{}",
            std::process::id(),
            NEXT_PROFILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("VERA.INI"), section).unwrap();
        dir
    }

    #[test]
    fn missing_profile_or_section_keeps_the_source_defaults() {
        let missing = std::env::temp_dir().join("vera-phobos-settings-missing");
        assert_eq!(read(&missing), PhobosClientSettings::default());
        let dir = profile_with("[Options]\nGameSpeed=1\n");
        assert_eq!(read(&dir), PhobosClientSettings::default());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn imported_switches_are_read() {
        let dir = profile_with(
            "[Phobos]\nShowPlacementPreview=no\nDigitalDisplay.Enable=yes\n\
             SuperWeaponSidebar.RequiredSignificance=2\nShowFlashOnSelecting=yes\n\
             ShowPowerPlantEnhancerRange=yes\nShowDesignatorRange=yes\nShowPowerDelta=no\nShowHarvesterCounter=no\n\
             ShowWeedsCounter=no\nPriorityDeployFiltering=no\nPrioritySelectionFiltering=no\nTypeSelectUseIFVMode=no\n\
             MessageDisplayInCenter=yes\nMessageDisplayInCenter.BoardOpacity=72\n\
             MessageDisplayInCenter.LabelsCount=3\nMessageDisplayInCenter.RecordsCount=9\n\
             SaveGameOnScenarioStart=no\nShowBriefing=no\n\
             HideLaserTrailEffects=yes\nShowGameTime=yes\nShowGameTime.BoardOpacity=55\n\
             EnableSelectBox=yes\n\
             CampaignDefaultGameSpeed=4\n\
             HideLightFlashEffects=yes\nHideShakeEffects=yes\n\
             ToolTipBlur=yes\nMessageApplyHoverState=yes\n\
             ToolTipDescriptions=no\n",
        );
        assert_eq!(
            read(&dir),
            PhobosClientSettings {
                show_placement_preview: false,
                digital_display_enabled: true,
                superweapon_sidebar_required_significance: 2,
                show_flash_on_selecting: true,
                show_power_plant_enhancer_range: true,
                show_designator_range: true,
                show_power_delta: false,
                show_harvester_counter: false,
                show_weeds_counter: false,
                priority_deploy_filtering: false,
                priority_selection_filtering: false,
                type_select_use_ifv_mode: false,
                message_display_in_center: true,
                message_display_in_center_board_opacity: 72,
                message_display_in_center_labels_count: 3,
                message_display_in_center_records_count: 9,
                save_game_on_scenario_start: false,
                show_briefing: false,
                hide_laser_trail_effects: true,
                hide_light_flash_effects: true,
                hide_shake_effects: true,
                show_game_time: true,
                show_game_time_board_opacity: 55,
                enable_select_box: true,
                real_time_timers: false,
                real_time_timers_adaptive: false,
                tool_tip_descriptions: false,
                tool_tip_blur: true,
                message_apply_hover_state: true,
                campaign_default_game_speed: 2,
            }
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// `Phobos.INI.cpp:120-124`: the stored value is `6 - key`; a key that
    /// leaves the 0..=6 range (here 7 gives -1) falls back to speed 2.
    /// `Phobos.INI.cpp:99-101,117`: the four board/count keys keep their
    /// static defaults when absent and read authored integers verbatim.
    #[test]
    fn the_message_column_and_game_time_boards_read_their_source_defaults() {
        let absent = read(&profile_with("[Phobos]\nMessageDisplayInCenter=yes\n"));
        assert_eq!(absent.message_display_in_center_board_opacity, 40);
        assert_eq!(absent.message_display_in_center_labels_count, 6);
        assert_eq!(absent.message_display_in_center_records_count, 12);
        assert_eq!(absent.show_game_time_board_opacity, 40);

        let dir = profile_with(
            "[Phobos]\nMessageDisplayInCenter.BoardOpacity=0\n\
             MessageDisplayInCenter.LabelsCount=1\n\
             MessageDisplayInCenter.RecordsCount=4\n\
             ShowGameTime.BoardOpacity=200\n",
        );
        let authored = read(&dir);
        assert_eq!(authored.message_display_in_center_board_opacity, 0);
        assert_eq!(authored.message_display_in_center_labels_count, 1);
        assert_eq!(authored.message_display_in_center_records_count, 4);
        assert_eq!(authored.show_game_time_board_opacity, 200);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn campaign_default_game_speed_inverts_the_key_and_clamps() {
        let dir = profile_with("[Phobos]\nCampaignDefaultGameSpeed=0\n");
        assert_eq!(read(&dir).campaign_default_game_speed, 6);
        std::fs::remove_dir_all(dir).unwrap();
        let dir = profile_with("[Phobos]\nCampaignDefaultGameSpeed=7\n");
        assert_eq!(read(&dir).campaign_default_game_speed, 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// `Phobos.INI.cpp:107` reads `[Phobos] ShowBriefing` with a `true`
    /// default (static `:68`); `0x683E41` refuses the scenario-start briefing
    /// dialog when the client switch is off.
    #[test]
    fn show_briefing_defaults_to_yes_and_reads_an_explicit_no() {
        assert!(PhobosClientSettings::default().show_briefing);
        let dir = profile_with("[Phobos]\nShowBriefing=no\n");
        assert!(!read(&dir).show_briefing);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// `Phobos.INI.cpp:102-103` reads `[Phobos] RealTimeTimers` and
    /// `RealTimeTimers.Adaptive` with `false` defaults; `0x006D4B50` prints
    /// wall-clock seconds when the first is on and follows the measured frame
    /// rate when the second is on too.
    #[test]
    fn real_time_timers_defaults_to_no_and_reads_both_switches() {
        let defaults = PhobosClientSettings::default();
        assert!(!defaults.real_time_timers);
        assert!(!defaults.real_time_timers_adaptive);
        let dir = profile_with("[Phobos]\nRealTimeTimers=yes\nRealTimeTimers.Adaptive=yes\n");
        let settings = read(&dir);
        assert!(settings.real_time_timers);
        assert!(settings.real_time_timers_adaptive);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
