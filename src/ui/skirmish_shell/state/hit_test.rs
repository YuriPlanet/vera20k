//! Hover targets and status-help keys for the skirmish shell.

use crate::skirmish_launch::LaunchCountry;

use super::super::layout::{
    ChooseMapModalButton, RandomMapSetupControl, SkirmishCheckboxId, SkirmishTrackbarId,
};
use super::{
    ChooseMapHoverTarget, OwnerDrawButton, SkirmishAiRowType, SkirmishComboId, SkirmishComboItem,
    SkirmishCountryChoice, SkirmishHoverTarget,
};

/// Like [`status_help_key_for_hover`], but an Ares country row may supply
/// its own `MenuText.Status` CSF key from the refreshed catalog.
pub(crate) fn status_help_key_for_hover_with_catalog<'a>(
    target: SkirmishHoverTarget,
    catalog: Option<&'a super::CountryMenuCatalog>,
) -> Option<&'a str> {
    if let SkirmishHoverTarget::ComboItem {
        id: SkirmishComboId::Side(_),
        item: SkirmishComboItem::Country(country),
    } = target
    {
        if let Some(key) = catalog.and_then(|catalog| catalog.status_key_for_choice(country)) {
            return Some(key);
        }
    }
    status_help_key_for_hover(target)
}

pub fn status_help_key_for_hover(target: SkirmishHoverTarget) -> Option<&'static str> {
    match target {
        SkirmishHoverTarget::StatusHelp0x695 => None,
        SkirmishHoverTarget::PlayerName0x6a0 => Some("STT:SkirmishEditPlayer"),
        SkirmishHoverTarget::MapPreview0x468 => Some("STT:SkirmishMapThumbnail"),
        SkirmishHoverTarget::OwnerDrawButton(OwnerDrawButton::StartGame0x617) => {
            Some("STT:SkirmishButtonStartGame")
        }
        SkirmishHoverTarget::OwnerDrawButton(OwnerDrawButton::ChooseMap0x5aa) => {
            Some("STT:SkirmishButtonChooseMap")
        }
        SkirmishHoverTarget::OwnerDrawButton(OwnerDrawButton::GameOptions) => {
            Some(crate::ui::skirmish_shell::game_options::GAME_OPTIONS_HELP)
        }
        SkirmishHoverTarget::OwnerDrawButton(OwnerDrawButton::Setups) => Some("VERA:SetupsHelp"),
        SkirmishHoverTarget::GameOptions(control) => control.status_help_key(),
        SkirmishHoverTarget::OwnerDrawButton(OwnerDrawButton::Back0x5c0) => {
            Some("STT:SkirmishButtonBack")
        }
        SkirmishHoverTarget::Checkbox(SkirmishCheckboxId::ShortGame0x54e) => {
            Some("STT:SkirmishCBoxShortGame")
        }
        SkirmishHoverTarget::Checkbox(SkirmishCheckboxId::McvRepacks0x693) => {
            Some("STT:SkirmishCBoxRedeploys")
        }
        SkirmishHoverTarget::Checkbox(SkirmishCheckboxId::CratesAppear0x696) => {
            Some("STT:SkirmishCBoxCrates")
        }
        SkirmishHoverTarget::Checkbox(SkirmishCheckboxId::SuperWeapons0x69a) => {
            Some("STT:SkirmishCBoxSWAllowed")
        }
        SkirmishHoverTarget::Checkbox(SkirmishCheckboxId::BuildOffAlly0x69d) => {
            Some("STT:SkirmishCBoxBuildOffAlly")
        }
        SkirmishHoverTarget::Trackbar(SkirmishTrackbarId::GameSpeed0x529) => {
            Some("STT:SkirmishSliderSpeed")
        }
        SkirmishHoverTarget::Trackbar(SkirmishTrackbarId::Credits0x511) => {
            Some("STT:SkirmishSliderCredits")
        }
        SkirmishHoverTarget::Trackbar(SkirmishTrackbarId::UnitCount0x50c) => {
            Some("STT:SkirmishSliderUnit")
        }
        SkirmishHoverTarget::FlagPicture => Some("STT:SkirmishPictureFlag"),
        SkirmishHoverTarget::GameTypeLabel0x6ec => Some("STT:SkirmishLabelGameType"),
        SkirmishHoverTarget::ScenarioLabel0x5a8 => Some("STT:SkirmishLabelScenario"),
        SkirmishHoverTarget::ComboFace(id) => status_help_key_for_combo(id),
        SkirmishHoverTarget::ComboItem {
            id: SkirmishComboId::AiType(_),
            item: SkirmishComboItem::AiType(row_type),
        } => status_help_key_for_ai_row_type(row_type),
        SkirmishHoverTarget::ComboItem {
            id: id @ SkirmishComboId::Side(_),
            item: SkirmishComboItem::Country(country),
        } => status_help_key_for_side_item(country).or_else(|| status_help_key_for_combo(id)),
        SkirmishHoverTarget::ComboItem {
            id: id @ SkirmishComboId::Color(_),
            item,
        } => status_help_key_for_color_item(item).or_else(|| status_help_key_for_combo(id)),
        SkirmishHoverTarget::ComboItem { id, .. } => status_help_key_for_combo(id),
    }
}

pub fn status_help_key_for_choose_map_hover(target: ChooseMapHoverTarget) -> Option<&'static str> {
    match target {
        ChooseMapHoverTarget::StatusHelp0x695 => None,
        ChooseMapHoverTarget::ModeList0x6eb | ChooseMapHoverTarget::ModeListRow0x6eb { .. } => {
            Some("STT:ScenarioListGameType")
        }
        ChooseMapHoverTarget::MapList0x553 => Some("STT:ScenarioListMaps"),
        ChooseMapHoverTarget::Preview0x468 => Some("STT:ScenarioMapThumbnail"),
        ChooseMapHoverTarget::Button(ChooseMapModalButton::UseMap0x6c5) => {
            Some("STT:ScenarioButtonUseMap")
        }
        ChooseMapHoverTarget::Button(ChooseMapModalButton::CreateRandomMap0x583) => {
            Some("STT:ScenarioButtonRandom")
        }
        ChooseMapHoverTarget::Button(ChooseMapModalButton::Cancel0x5c0) => {
            Some("STT:ScenarioButtonCancel")
        }
        // VERA's button has no status help.
        ChooseMapHoverTarget::Button(ChooseMapModalButton::Favorite) => None,
    }
}

/// The random-map dialog `0x105`'s status help: the table lookup
/// `0x006040B0` for its controls (`tools/storage_oracle/shell_help_keys.py`).
pub fn status_help_key_for_random_map_setup(control: RandomMapSetupControl) -> &'static str {
    use RandomMapSetupControl as C;
    match control {
        C::MapType0x405 => "STT:GenerateCBoxEnvironment",
        C::Time0x3ea => "STT:GenerateCBoxTime",
        C::Theater0x407 => "STT:GenerateCBoxTheater",
        C::Size0x406 => "STT:GenerateCBoxMapSize",
        C::Resources0x408 => "STT:GenerateCBoxResources",
        C::Players0x3eb => "STT:GenerateSliderNumPlayers",
        C::Randomize0x621 => "STT:GenerateButtonSurprise",
        C::Generate0x620 => "STT:GenerateButtonPreview",
        C::Ok0x6c5 => "STT:GenerateButtonUseMap",
        C::Load0x6c2 => "STT:GenerateButtonLoadMap",
        C::Save0x6c3 => "STT:GenerateButtonSaveMap",
        C::Delete0x6c4 => "STT:GenerateButtonDeleteMap",
        C::Cancel0x5c0 => "STT:GenerateButtonCancel",
        C::AdvancedToggle => "VERA:RandomMapAdvancedHelp",
        C::AdvancedScrollbar => "VERA:RandomMapScrollHelp",
        C::ExportMap => "VERA:RandomMapExportHelp",
        C::FairStartsToggle => "VERA:RandomMapFairStartsHelp",
        C::PresetCycle => "VERA:RandomMapPresetHelp",
        // The dialog knows which entry a row holds and shows that entry's
        // help (`ExtrasRow::help`); this is the view's own.
        C::ExtrasRow(_) => "VERA:RandomMapExtrasHelp",
        C::Advanced(super::super::layout::AdvancedField::PlateauSize) => {
            "VERA:RandomMapPlateauSizeHelp"
        }
        C::Advanced(super::super::layout::AdvancedField::TechBuildings) => {
            "VERA:RandomMapTechBuildingsHelp"
        }
        C::Advanced(super::super::layout::AdvancedField::Derelicts) => {
            "VERA:RandomMapDerelictsHelp"
        }
        C::Advanced(super::super::layout::AdvancedField::Damage) => "VERA:RandomMapDamageHelp",
        C::Advanced(super::super::layout::AdvancedField::Civilians) => {
            "VERA:RandomMapCiviliansHelp"
        }
        C::Advanced(super::super::layout::AdvancedField::Animals) => "VERA:RandomMapAnimalsHelp",
        C::Advanced(super::super::layout::AdvancedField::Wild) => "VERA:RandomMapWildHelp",
        C::Advanced(_) => "VERA:RandomMapAdvancedSliderHelp",
        // Ares Game_DialogFunc_Subtext_Load (Ares.dll 10075EC0, hook 0x60411B).
        // The box is hit-tested only when Ares.dll is loaded.
        C::UrbanAreas5000 => "STT:RMGUrbanAreas",
    }
}

fn status_help_key_for_combo(id: SkirmishComboId) -> Option<&'static str> {
    match id {
        SkirmishComboId::AiType(_) => Some("STT:SkirmishComboAIPlayer"),
        SkirmishComboId::Side(_) => Some("STT:SkirmishComboCountry"),
        SkirmishComboId::Color(_) => Some("STT:SkirmishComboColor"),
        SkirmishComboId::Start(_) => Some("STT:HostComboStart"),
        SkirmishComboId::Team(_) => Some("STT:HostComboTeam"),
        SkirmishComboId::TeamMenu => Some("VERA:QuickTeamSetupHelp"),
        SkirmishComboId::SeatMenu => Some("VERA:BulkSeatActionsHelp"),
    }
}

fn status_help_key_for_ai_row_type(row_type: SkirmishAiRowType) -> Option<&'static str> {
    match row_type {
        SkirmishAiRowType::None => Some("STT:PlayerNone"),
        SkirmishAiRowType::Easy => Some("STT:PlayerDumbAI"),
        SkirmishAiRowType::Normal => Some("STT:PlayerSmartAI"),
        SkirmishAiRowType::Hard => Some("STT:PlayerGeniusAI"),
    }
}

fn status_help_key_for_side_item(country: SkirmishCountryChoice) -> Option<&'static str> {
    match country {
        SkirmishCountryChoice::Random => Some("STT:PlayerSideRandom"),
        SkirmishCountryChoice::Country(country) => match country {
            LaunchCountry::America => Some("STT:PlayerSideAmerica"),
            LaunchCountry::Korea => Some("STT:PlayerSideKorea"),
            LaunchCountry::France => Some("STT:PlayerSideFrance"),
            LaunchCountry::Germany => Some("STT:PlayerSideGermany"),
            LaunchCountry::GreatBritain => Some("STT:PlayerSideBritain"),
            LaunchCountry::Libya => Some("STT:PlayerSideLibya"),
            LaunchCountry::Iraq => Some("STT:PlayerSideIraq"),
            LaunchCountry::Cuba => Some("STT:PlayerSideCuba"),
            LaunchCountry::Russia => Some("STT:PlayerSideRussia"),
            LaunchCountry::Yuri => Some("STT:PlayerSideYuriCountry"),
            _ => None,
        },
    }
}

/// gamemd names its eight colours and the observer's (`STT:PlayerColor*`);
/// VERA's extra colours show their swatch and no status line.
fn status_help_key_for_color_item(item: SkirmishComboItem) -> Option<&'static str> {
    match item {
        SkirmishComboItem::ColorSentinel(-2) => Some("STT:PlayerColorRandom"),
        SkirmishComboItem::Color(0) => Some("STT:PlayerColorGold"),
        SkirmishComboItem::Color(1) => Some("STT:PlayerColorRed"),
        SkirmishComboItem::Color(2) => Some("STT:PlayerColorBlue"),
        SkirmishComboItem::Color(3) => Some("STT:PlayerColorGreen"),
        SkirmishComboItem::Color(4) => Some("STT:PlayerColorOrange"),
        SkirmishComboItem::Color(5) => Some("STT:PlayerColorSkyBlue"),
        SkirmishComboItem::Color(6) => Some("STT:PlayerColorPurple"),
        SkirmishComboItem::Color(7) => Some("STT:PlayerColorPink"),
        SkirmishComboItem::Color(8) => Some("STT:PlayerColorObserver"),
        _ => None,
    }
}
