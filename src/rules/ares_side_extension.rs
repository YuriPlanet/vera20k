//! Installed Ares 3.0 `SideExt` fields shared by the rules reader and consumers.
//!
//! The native extension is allocated by `SideClass_CTOR @ 0x1002EF60` through
//! `SideExt::side_ctor @ 0x1002D9F0`: the constructor leaves the nullable
//! members unset, `SideExt::initialize @ 0x1002E190` supplies the ID defaults,
//! and the reader prefix `0x1002E450..0x1002EA4C` reads the current pass in a
//! fixed order. `SideExt::read_all @ 0x1002EFC0` dispatches every allocated
//! Side. The field offsets below are the 3.0 DLL's, established by the
//! `tools/rules_oracle/ares_side_sidebar.py` fixture and a bounded read of the
//! reader/getter instructions; they are not inferred from the source tree.
//!
//! Type references use three states, as Ares `Nullable<T*>` does:
//! `None` = never set (an absent, empty or unknown value retains),
//! `Some(None)` = `none`/`<none>` (explicitly null), `Some(Some(id))` =
//! resolved registered type. Fixed strings hold `None` for an empty array.

#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AresSideExtension {
    /// `+0x0C` `DefaultDisguise=` (nullable InfantryType).
    pub default_disguise: Option<Option<String>>,
    /// `+0x14` `Crew=` (nullable InfantryType).
    pub crew: Option<Option<String>>,
    /// `+0x1C` `Engineer=` (nullable InfantryType).
    pub engineer: Option<Option<String>>,
    /// `+0x24` `Technician=` (nullable InfantryType).
    pub technician: Option<Option<String>>,
    /// `+0x2C` `SurvivorDivisor=` (nullable int).
    pub survivor_divisor: Option<i32>,
    /// `+0x34` `AI.BaseDefenses=` (nullable BuildingType vector).
    pub base_defenses: Option<Vec<String>>,
    /// `+0x44` `AI.BaseDefenseCounts=` (nullable int vector).
    pub base_defense_counts: Option<Vec<i32>>,
    /// `+0x54` `ParaDrop.Types=` (nullable Infantry/Unit vector).
    pub paradrop_types: Option<Vec<String>>,
    /// `+0x64` `ParaDrop.Num=` (nullable int vector).
    pub paradrop_num: Option<Vec<i32>>,
    /// `+0x74` `ParaDrop.Aircraft=` (ValueableIdx<AircraftType>). Initialized
    /// to the PDPLANE index; `Some(None)` is the -1 index (`none`).
    pub paradrop_aircraft: Option<Option<String>>,
    /// `+0x78` `Parachute.Anim=` (nullable Animation).
    pub parachute_anim: Option<Option<String>>,
    /// `+0x80` `ToolTipColor=` (three bytes).
    pub tooltip_color: [u8; 3],
    /// `+0x84` `MessageTextColor=` (runtime ColorScheme array index). The
    /// reader resolves a scheme name through the shared kind-1 lookup
    /// `0x1001BB30`; the defaults are 11 (Nod), 25 (ThirdSide), 21 (others).
    pub message_text_color_index: i32,
    /// `+0x88` `Sidebar.MixFileIndex=`.
    pub sidebar_mix_file_index: i32,
    /// `+0x8C` `Sidebar.YuriFileNames=`.
    pub sidebar_yuri_file_names: bool,
    /// `+0x90` `EVA.Tag=` resolved against the fixed EVAMD catalog.
    pub eva_voice_index: i32,
    /// `+0x94` `HunterSeeker=` (Valueable<UnitType>).
    pub hunter_seeker: Option<Option<String>>,
    /// `+0x98` `CampaignScore.Background=`.
    pub campaign_score_background: Option<String>,
    /// `+0xB8` `CampaignScore.Transition=`.
    pub campaign_score_transition: Option<String>,
    /// `+0xD8` `CampaignScore.Animation=`.
    pub campaign_score_animation: Option<String>,
    /// `+0xF8` `CampaignScore.Palette=`.
    pub campaign_score_palette: Option<String>,
    /// `+0x118` `MultiplayerScore.Background=`.
    pub multiplayer_score_background: Option<String>,
    /// `+0x138` `MultiplayerScore.Bars=`.
    pub multiplayer_score_bars: Option<String>,
    /// `+0x158` `MultiplayerScore.Palette=`.
    pub multiplayer_score_palette: Option<String>,
    /// `+0x178` `CampaignScore.UnderParTheme=`.
    pub campaign_score_under_par_theme: Option<String>,
    /// `+0x198` `CampaignScore.OverParTheme=`.
    pub campaign_score_over_par_theme: Option<String>,
    /// `+0x1B8` `MultiplayerScore.WinTheme=`.
    pub multiplayer_score_win_theme: Option<String>,
    /// `+0x1D8` `MultiplayerScore.LoseTheme=`.
    pub multiplayer_score_lose_theme: Option<String>,
    /// `+0x1F8` `GraphicalText.Image=`.
    pub graphical_text_image: Option<String>,
    /// `+0x218` `GraphicalText.Palette=`.
    pub graphical_text_palette: Option<String>,
    /// `+0x238` `DialogBackground.Image=`.
    pub dialog_background_image: Option<String>,
    /// `+0x258` `DialogBackground.Palette=`.
    pub dialog_background_palette: Option<String>,
}

#[cfg(feature = "ares")]
impl AresSideExtension {
    /// The seven score-screen names, as `Game_LoadCampaignScoreAssets`
    /// (`0x1002F3A0`, patch site `0x72D300`), `Game_LoadMultiplayerScoreAssets`
    /// (`0x1002F430`, `0x72D730`) and `Game_GetMultiplayerScoreScreenBar`
    /// (`0x1002F300`, `0x5CA110`) copy them over gamemd's own name buffers:
    /// each name is substituted on its own, so `None` keeps the caller's fixed
    /// table entry for that name alone.
    pub fn score_art(&self) -> crate::rules::ruleset::SideScoreArt<'_> {
        crate::rules::ruleset::SideScoreArt {
            campaign_background: self.campaign_score_background.as_deref(),
            campaign_transition: self.campaign_score_transition.as_deref(),
            campaign_animation: self.campaign_score_animation.as_deref(),
            campaign_palette: self.campaign_score_palette.as_deref(),
            multiplayer_background: self.multiplayer_score_background.as_deref(),
            multiplayer_bars: self.multiplayer_score_bars.as_deref(),
            multiplayer_palette: self.multiplayer_score_palette.as_deref(),
        }
    }

    /// The theme name one of the two score dialogs asks for. Both sites read
    /// the field buffer directly, so a name is returned as authored.
    pub fn score_theme(&self, theme: crate::rules::ruleset::ScoreTheme) -> Option<&str> {
        let field = match theme {
            crate::rules::ruleset::ScoreTheme::Campaign { over_par: false } => {
                &self.campaign_score_under_par_theme
            }
            crate::rules::ruleset::ScoreTheme::Campaign { over_par: true } => {
                &self.campaign_score_over_par_theme
            }
            crate::rules::ruleset::ScoreTheme::Multiplayer { defeated: false } => {
                &self.multiplayer_score_win_theme
            }
            crate::rules::ruleset::ScoreTheme::Multiplayer { defeated: true } => {
                &self.multiplayer_score_lose_theme
            }
        };
        field.as_deref()
    }

    /// The `DialogBackground.Image/.Palette` names, while both are set. The
    /// game-load writer `0x1002ECC0` fills the image and palette slots only
    /// from a non-empty name (`0x1002EE6B`, `0x1002EEB9`), and the hook at
    /// `0x622223` (body `0x1002FB50`) resumes past the stock selection at
    /// `0x62226A` only while both loaded slots are non-null; the caller
    /// still needs both files to load.
    pub fn dialog_background(&self) -> Option<(&str, &str)> {
        let image = self.dialog_background_image.as_deref()?;
        let palette = self.dialog_background_palette.as_deref()?;
        (!image.is_empty() && !palette.is_empty()).then_some((image, palette))
    }
}

#[cfg(feature = "ares")]
impl Default for AresSideExtension {
    fn default() -> Self {
        Self {
            default_disguise: None,
            crew: None,
            engineer: None,
            technician: None,
            survivor_divisor: None,
            base_defenses: None,
            base_defense_counts: None,
            paradrop_types: None,
            paradrop_num: None,
            paradrop_aircraft: None,
            parachute_anim: None,
            // `SideExt::constructor @ 0x1002D9F0` leaves the selected fields
            // at their zero/-1 member defaults; `SideExt::initialize`
            // overwrites them by stored Side ID on the first reached pass.
            tooltip_color: [0, 0, 0],
            message_text_color_index: 0,
            sidebar_mix_file_index: 0,
            sidebar_yuri_file_names: false,
            eva_voice_index: -1,
            hunter_seeker: None,
            campaign_score_background: None,
            campaign_score_transition: None,
            campaign_score_animation: None,
            campaign_score_palette: None,
            multiplayer_score_background: None,
            multiplayer_score_bars: None,
            multiplayer_score_palette: None,
            campaign_score_under_par_theme: None,
            campaign_score_over_par_theme: None,
            multiplayer_score_win_theme: None,
            multiplayer_score_lose_theme: None,
            graphical_text_image: None,
            graphical_text_palette: None,
            dialog_background_image: None,
            dialog_background_palette: None,
        }
    }
}
