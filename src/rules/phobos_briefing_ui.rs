//! Phobos `uimd.ini [UISettings]` briefing-dialog labels.
//!
//! `src/Phobos.INI.cpp:246-251` reads both keys from `UISettings` at startup:
//! `ShowBriefingResumeButtonLabel` (`ReadString`, default `GUI:Resume`, loaded
//! as a CSF label through `GeneralUtils::LoadStringOrDefault`) and
//! `ShowBriefingResumeButtonStatusLabel` (`ReadString`, default
//! `STT:BriefingButtonReturn`, copied into `char[0x20]`).
//!
//! The two hooks consume them while `BriefingTemp::ShowBriefing` is set:
//! `DEFINE_HOOK(0x65F764, BriefingDialog_ShowBriefing, 0x5)` rewrites dialog
//! item 1059 (0x423) with the resume label, and
//! `DEFINE_HOOK(0x604985, GetDialogUIStatusLabels_ShowBriefing, 0x5)` returns
//! the status label. That flag is armed by
//! `DEFINE_HOOK(0x683E41, ScenarioClass_Start_ShowBriefing, 0x6)`, gated by
//! `[Phobos] ShowBriefing`, the scenario's `[Basic] ShowBriefing`,
//! the caller's `showBriefing` argument and `SessionClass::IsCampaign()`.
//!
//! VERA has the *restate* briefing shell (`InGameMenuState::MissionBriefing`,
//! `ui/shell/restate.rs`) whose 0x423 button is `GUI:Return` with help key
//! `STT:BriefingButtonReturn`, but no scenario-start briefing-dialog pass.
//! These readers therefore land the keys at their native layer without
//! rewriting that stock shell (which would turn "Return" into "Resume").
//! See `docs/phobos-show-briefing.md`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on app/, render/, ui/, sim/ or audio/.

use crate::rules::ini_parser::{IniFile, IniSection};

/// Phobos `Phobos::UI` briefing-dialog labels from `uimd.ini [UISettings]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefingDialogUi {
    /// `ShowBriefingResumeButtonLabel=`, Phobos default `GUI:Resume`.
    pub resume_button_label: String,
    /// `ShowBriefingResumeButtonStatusLabel=`, Phobos default
    /// `STT:BriefingButtonReturn`; native stores it in `char[0x20]`.
    pub resume_button_status_label: String,
}

impl Default for BriefingDialogUi {
    fn default() -> Self {
        Self {
            resume_button_label: "GUI:Resume".to_owned(),
            resume_button_status_label: "STT:BriefingButtonReturn".to_owned(),
        }
    }
}

impl BriefingDialogUi {
    pub fn from_uimd_bytes(bytes: &[u8]) -> Self {
        let Ok(ini) = IniFile::from_bytes(bytes) else {
            log::warn!("Could not parse UIMD.INI for briefing labels");
            return Self::default();
        };
        Self::from_ini(&ini)
    }

    pub fn from_ini(ini: &IniFile) -> Self {
        ini.section("UISettings")
            .map_or_else(Self::default, Self::from_section)
    }

    /// `BriefingDialog_ShowBriefing @ 0x65F764` uses the authored label only
    /// while `BriefingTemp::ShowBriefing` is set (the scenario-start path).
    /// The stock F2 restate dialog keeps its own caption.
    pub fn button_label_key<'a>(&'a self, at_start: bool, stock: &'a str) -> &'a str {
        if at_start {
            &self.resume_button_label
        } else {
            stock
        }
    }

    /// `GetDialogUIStatusLabels_ShowBriefing @ 0x604985`, same gate.
    pub fn status_label_key<'a>(&'a self, at_start: bool, stock: &'a str) -> &'a str {
        if at_start {
            &self.resume_button_status_label
        } else {
            stock
        }
    }

    pub fn from_section(section: &IniSection) -> Self {
        // `INI_EX::ReadString`: the default is copied only when the key is
        // absent. The physical INI loader drops an authored empty value
        // (gamemd's parser stores no empty entry), so a file can only ever
        // produce the default; a projected section may still hold an empty
        // string, which `read_string` keeps. The status label lands in
        // native's `char[0x20]`, while the button label uses the shared
        // `Phobos::readBuffer` (`Phobos.h:26`, 2048 bytes).
        Self {
            resume_button_label: section.read_string(
                "ShowBriefingResumeButtonLabel",
                "GUI:Resume",
                0x800,
            ),
            resume_button_status_label: section.read_string(
                "ShowBriefingResumeButtonStatusLabel",
                "STT:BriefingButtonReturn",
                0x20,
            ),
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;

    #[test]
    fn an_absent_uisettings_section_keeps_the_phobos_reader_defaults() {
        let defaults = BriefingDialogUi::default();
        assert_eq!(defaults.resume_button_label, "GUI:Resume");
        assert_eq!(
            defaults.resume_button_status_label,
            "STT:BriefingButtonReturn"
        );
        assert_eq!(
            BriefingDialogUi::from_uimd_bytes(b"[Sidebar]\nCenterPauseMenuBackground=no\n"),
            defaults
        );
        assert_eq!(
            BriefingDialogUi::from_uimd_bytes(b"[UISettings]\n"),
            defaults
        );
    }

    #[test]
    fn authored_uisettings_labels_replace_only_their_own_defaults() {
        let ui = BriefingDialogUi::from_uimd_bytes(
            b"[UISettings]\nShowBriefingResumeButtonLabel=GUI:StartMission\n\
              ShowBriefingResumeButtonStatusLabel=STT:StartMission\n",
        );
        assert_eq!(ui.resume_button_label, "GUI:StartMission");
        assert_eq!(ui.resume_button_status_label, "STT:StartMission");

        let only_label = BriefingDialogUi::from_uimd_bytes(
            b"[UISettings]\nShowBriefingResumeButtonLabel=GUI:Go\n",
        );
        assert_eq!(only_label.resume_button_label, "GUI:Go");
        assert_eq!(
            only_label.resume_button_status_label,
            "STT:BriefingButtonReturn"
        );
    }

    #[test]
    fn the_authored_labels_apply_only_to_the_scenario_start_path() {
        let ui = BriefingDialogUi::from_uimd_bytes(
            b"[UISettings]\nShowBriefingResumeButtonLabel=GUI:Go\n\
              ShowBriefingResumeButtonStatusLabel=STT:Go\n",
        );
        assert_eq!(ui.button_label_key(true, "GUI:Return"), "GUI:Go");
        assert_eq!(ui.button_label_key(false, "GUI:Return"), "GUI:Return");
        assert_eq!(
            ui.status_label_key(true, "STT:BriefingButtonReturn"),
            "STT:Go"
        );
        assert_eq!(
            ui.status_label_key(false, "STT:BriefingButtonReturn"),
            "STT:BriefingButtonReturn"
        );
    }

    #[test]
    fn an_authored_empty_status_label_reads_as_absent_in_a_physical_ini() {
        // gamemd's loader stores no empty entry, so the key is missing and
        // `ReadString` copies the default.
        let ui = BriefingDialogUi::from_uimd_bytes(
            b"[UISettings]\nShowBriefingResumeButtonStatusLabel=\n",
        );
        assert_eq!(ui.resume_button_status_label, "STT:BriefingButtonReturn");
        assert_eq!(ui.resume_button_label, "GUI:Resume");
    }
}
