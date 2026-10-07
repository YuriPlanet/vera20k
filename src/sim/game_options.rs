//! Per-match game settings — the lobby options card.
//!
//! Parsed from `[MultiplayerDialogSettings]` in rulesmd.ini. Most fields are set
//! once at game start; the offline in-game Options dialog may change game speed
//! through a synchronized simulation command. Included in the deterministic
//! state hash for lockstep correctness.

use crate::rules::ini_parser::IniFile;

const NORMALIZED_DELAY_SHORT: [[u16; 8]; 4] = [
    [2, 2, 1, 1, 1, 1, 1, 1],
    [3, 3, 3, 2, 2, 2, 1, 1],
    [5, 4, 4, 3, 3, 2, 2, 1],
    [7, 6, 5, 4, 4, 4, 3, 2],
];

/// Highest stored value emitted by the retail in-game speed trackbar.
pub(crate) const IN_GAME_OPTIONS_MAX_SPEED: u8 = 6;

/// Per-match game settings from the lobby / `[MultiplayerDialogSettings]`.
///
/// Set once at game start except for the synchronized offline game-speed
/// transition admitted by [`GameOptions::apply_in_game_speed`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GameOptions {
    // --- Runtime-checked by gameplay systems ---
    /// Defeat when all buildings lost (vs all objects lost). Rules+0x14B6.
    pub short_game: bool,
    /// Construction yards / base building enabled. Rules+0x14AF.
    pub bases: bool,
    /// Bridges can be destroyed. Rules+0x14AC.
    pub bridges_destroyable: bool,
    /// Superweapons can be built. Rules+0x14B9.
    pub super_weapons: bool,
    /// Build adjacent to allied buildings. Rules+0x14BA.
    pub build_off_ally: bool,
    /// Random crate spawning. Rules+0x14B1.
    pub crates: bool,
    /// MCV can repack into vehicle. Rules+0x14B8.
    pub mcv_redeploy: bool,
    /// TS-legacy semi-transparent fog. Default false in YR. Rules+0x14B7.
    pub fog_of_war: bool,
    /// Unexplored cells are black. Rules+0x14AE.
    pub shroud: bool,
    /// Ore/gems regenerate on the map. Rules+0x14B0.
    pub tiberium_grows: bool,
    /// Engineers capture at reduced HP only. Rules+0x14B4.
    pub multi_engineer: bool,
    /// Harvesters immune to enemy fire. Rules+0x14B3.
    pub harvester_truce: bool,
    /// Alliances can be changed mid-game. Rules+0x14BB (YR addition).
    pub ally_change_allowed: bool,

    // --- Used at init (Create_Houses, spawn units) ---
    /// Default starting credits per player. Rules+0x1484.
    pub starting_credits: i32,
    /// Number of starting units to spawn. Rules+0x1494.
    pub unit_count: i32,
    /// Maximum tech level for this match. Rules+0x149C.
    pub tech_level: i32,
    /// Stored game-speed index. Retail gameplay uses 0 through 7.
    pub game_speed: i32,
    /// Rules-owned default AI difficulty in the native HouseClass convention:
    /// 0=Hard, 1=Normal, 2=Easy. Offline lobby rows copy their own selected
    /// value into each HouseState. Rules+0x14A4.
    pub ai_difficulty: i32,
    /// Number of AI opponents. Rules+0x14A8.
    pub ai_players: i32,
}

impl Default for GameOptions {
    /// Defaults from `[MultiplayerDialogSettings]` in rulesmd.ini (YR).
    fn default() -> Self {
        Self {
            short_game: true,
            bases: true,
            bridges_destroyable: true,
            super_weapons: true,
            build_off_ally: true,
            crates: true,
            mcv_redeploy: true,
            fog_of_war: false,
            shroud: true,
            tiberium_grows: true,
            multi_engineer: false,
            harvester_truce: false,
            ally_change_allowed: true,
            starting_credits: 10000,
            unit_count: 10,
            tech_level: 10,
            game_speed: 1,
            ai_difficulty: 0,
            ai_players: 0,
        }
    }
}

impl GameOptions {
    /// Apply one offline in-game Options speed transition.
    ///
    /// `OptionsClass::ApplyFromInGameDialog @ 0x004E1DE0` maps the 0..=6
    /// trackbar to `stored = 6 - slider_position`. Native offline modes 0/5
    /// store that value before the next `Main_Tick`; active network modes use
    /// EventClass opcode 0x0D at the tail instead. VERA currently implements
    /// only the offline ingress timing and rejects values the dialog cannot
    /// emit rather than fabricating a clamp.
    pub(crate) fn apply_in_game_speed(&mut self, speed: u8) -> bool {
        if speed > IN_GAME_OPTIONS_MAX_SPEED {
            return false;
        }
        self.game_speed = i32::from(speed);
        true
    }

    /// Scale a normalized animation delay through the currently stored speed.
    pub fn normalized_anim_delay(&self, delay: u16) -> u16 {
        self.speed_normalize(i32::from(delay)) as u16
    }

    /// `GameOptionsClass::SpeedNormalize @ 0x005FB2E0` (receiver `0xA8EB60`,
    /// `[this]` = the stored speed index): `0 -> 0`; `1..=4` index the 4x8
    /// table at `0x00832CEC` (`[value*8 + speed]`); otherwise
    /// `(value << 3) / (speed + 1)` (`0x005FB2FC..0x005FB301`, signed IDIV).
    ///
    /// The frame pacer admits one sim tick per `stored_speed * 16 ms` bucket
    /// (`app::types::tps_for_game_speed`), the same clock native's
    /// `FrameTimer` runs on, so dividing a frame count by `speed + 1` here
    /// reproduces native wall-clock cadence at every stored index.
    pub fn speed_normalize(&self, value: i32) -> i32 {
        if value == 0 {
            return 0;
        }
        let speed = usize::try_from(self.game_speed)
            .ok()
            .filter(|speed| *speed < 8)
            .expect("stored game speed must be in 0..=7");
        if (1..5).contains(&value) {
            return i32::from(NORMALIZED_DELAY_SHORT[(value - 1) as usize][speed]);
        }
        value.wrapping_shl(3) / (speed as i32 + 1)
    }

    /// Override the per-match defaults from a merged rules INI's
    /// `[MultiplayerDialogSettings]` section.
    ///
    /// This section is read once into the rules data that both the skirmish
    /// setup dialog and the launched match draw from. Each key is optional: a
    /// missing key (or a missing section) keeps the corresponding
    /// [`GameOptions::default`] value, so the stock INI — whose values equal the
    /// defaults — parses to an unchanged result, and only a mod that edits a key
    /// shifts behaviour.
    ///
    /// `GameSpeed` is stored as parsed (0 = fastest); the setup trackbar inverts
    /// it only for display. The setup dialog exposes superweapons and
    /// build-off-ally as checkboxes even though the stock section omits both
    /// keys, so they fall back to the enabled defaults until a mod sets
    /// `SuperWeaponsAllowed` / `BuildOffAlly`.
    ///
    /// Three keys this section also carries are intentionally not mapped here:
    /// `ShadowGrow` and `CaptureTheFlag` drive systems this engine does not
    /// model, and the per-mode allies setting is sourced from the selected game
    /// mode rather than this global default. `AIDifficulty` / `AIPlayers` are
    /// parsed for a faithful round-trip. A skirmish launch replaces `AIPlayers`
    /// from the configured slots, while each AI row's selected difficulty is
    /// copied to its own `HouseState` and this rules-owned difficulty remains
    /// the global/default mirror.
    pub fn from_multiplayer_dialog_settings(ini: &IniFile) -> Self {
        let mut options = Self::default();
        // `0x00671EF7`-`0x0067220E`: ReadInt/ReadBool over each current value.
        let section = ini.section_or_empty("MultiplayerDialogSettings");

        options.starting_credits = section.read_int("Money", options.starting_credits);
        options.unit_count = section.read_int("UnitCount", options.unit_count);
        options.tech_level = section.read_int("TechLevel", options.tech_level);
        options.game_speed = section.read_int("GameSpeed", options.game_speed);
        options.ai_difficulty = section.read_int("AIDifficulty", options.ai_difficulty);
        options.ai_players = section.read_int("AIPlayers", options.ai_players);

        options.bridges_destroyable =
            section.read_bool("BridgeDestruction", options.bridges_destroyable);
        options.shroud = section.read_bool("Shroud", options.shroud);
        options.bases = section.read_bool("Bases", options.bases);
        options.tiberium_grows = section.read_bool("TiberiumGrows", options.tiberium_grows);
        options.crates = section.read_bool("Crates", options.crates);
        options.harvester_truce = section.read_bool("HarvesterTruce", options.harvester_truce);
        options.multi_engineer = section.read_bool("MultiEngineer", options.multi_engineer);
        options.ally_change_allowed =
            section.read_bool("AllyChangeAllowed", options.ally_change_allowed);
        options.short_game = section.read_bool("ShortGame", options.short_game);
        options.super_weapons = section.read_bool("SuperWeaponsAllowed", options.super_weapons);
        options.build_off_ally = section.read_bool("BuildOffAlly", options.build_off_ally);
        options.fog_of_war = section.read_bool("FogOfWar", options.fog_of_war);
        options.mcv_redeploy = section.read_bool("MCVRedeploys", options.mcv_redeploy);

        options
    }
}

#[cfg(test)]
mod tests {
    use super::GameOptions;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn build_off_ally_default_matches_yr_enabled() {
        assert!(GameOptions::default().build_off_ally);
    }

    #[test]
    fn in_game_speed_transition_accepts_six_and_rejects_seven_without_clamping() {
        let mut options = GameOptions::default();
        assert!(options.apply_in_game_speed(6));
        assert_eq!(options.game_speed, 6);
        assert!(!options.apply_in_game_speed(7));
        assert_eq!(options.game_speed, 6);
    }

    #[test]
    fn stock_multiplayer_dialog_settings_match_hardcoded_defaults() {
        // The merged stock section equals every default, so the parse is a
        // no-op on the stock INI — the change is invisible in stock skirmishes
        // and only a mod that edits a key diverges.
        let ini = IniFile::from_str(
            "[MultiplayerDialogSettings]\n\
             Money=10000\nUnitCount=10\nTechLevel=10\nGameSpeed=1\n\
             AIDifficulty=0\nAIPlayers=0\n\
             BridgeDestruction=yes\nShroud=yes\nBases=yes\nTiberiumGrows=yes\n\
             Crates=yes\nHarvesterTruce=no\nMultiEngineer=no\nAllyChangeAllowed=yes\n\
             ShortGame=yes\nFogOfWar=no\nMCVRedeploys=yes\n",
        );
        let parsed = GameOptions::from_multiplayer_dialog_settings(&ini);
        let default = GameOptions::default();
        assert_eq!(parsed.starting_credits, default.starting_credits);
        assert_eq!(parsed.unit_count, default.unit_count);
        assert_eq!(parsed.tech_level, default.tech_level);
        assert_eq!(parsed.game_speed, default.game_speed);
        assert_eq!(parsed.bridges_destroyable, default.bridges_destroyable);
        assert_eq!(parsed.shroud, default.shroud);
        assert_eq!(parsed.bases, default.bases);
        assert_eq!(parsed.tiberium_grows, default.tiberium_grows);
        assert_eq!(parsed.crates, default.crates);
        assert_eq!(parsed.harvester_truce, default.harvester_truce);
        assert_eq!(parsed.multi_engineer, default.multi_engineer);
        assert_eq!(parsed.ally_change_allowed, default.ally_change_allowed);
        assert_eq!(parsed.short_game, default.short_game);
        assert_eq!(parsed.fog_of_war, default.fog_of_war);
        assert_eq!(parsed.mcv_redeploy, default.mcv_redeploy);
        // The stock section omits both these keys, so they keep the enabled
        // defaults rather than reading as false.
        assert_eq!(parsed.super_weapons, default.super_weapons);
        assert_eq!(parsed.build_off_ally, default.build_off_ally);
    }

    #[test]
    fn absent_section_keeps_all_defaults() {
        let ini = IniFile::from_str("[General]\nFoo=1\n");
        let parsed = GameOptions::from_multiplayer_dialog_settings(&ini);
        let default = GameOptions::default();
        assert_eq!(parsed.starting_credits, default.starting_credits);
        assert_eq!(parsed.tech_level, default.tech_level);
        assert!(parsed.bases);
        assert!(parsed.super_weapons);
    }

    #[test]
    fn modded_numeric_and_bool_keys_override_defaults() {
        let ini = IniFile::from_str(
            "[MultiplayerDialogSettings]\n\
             Money=7400\nUnitCount=4\nTechLevel=3\nGameSpeed=4\n\
             Bases=no\nCrates=no\nShortGame=no\nMCVRedeploys=no\n\
             TiberiumGrows=no\nFogOfWar=yes\n",
        );
        let parsed = GameOptions::from_multiplayer_dialog_settings(&ini);
        assert_eq!(parsed.starting_credits, 7400);
        assert_eq!(parsed.unit_count, 4);
        assert_eq!(parsed.tech_level, 3);
        // GameSpeed is stored as parsed; the display inversion is not baked in.
        assert_eq!(parsed.game_speed, 4);
        assert!(!parsed.bases);
        assert!(!parsed.crates);
        assert!(!parsed.short_game);
        assert!(!parsed.mcv_redeploy);
        assert!(!parsed.tiberium_grows);
        assert!(parsed.fog_of_war);
    }

    #[test]
    fn super_weapons_uses_allowed_suffix_key() {
        // The superweapons checkbox is backed by `SuperWeaponsAllowed`, not the
        // bare `SuperWeapons` key — the latter must be ignored.
        let wrong = IniFile::from_str("[MultiplayerDialogSettings]\nSuperWeapons=no\n");
        assert!(
            GameOptions::from_multiplayer_dialog_settings(&wrong).super_weapons,
            "the bare SuperWeapons key must not disable superweapons"
        );
        let right = IniFile::from_str("[MultiplayerDialogSettings]\nSuperWeaponsAllowed=no\n");
        assert!(!GameOptions::from_multiplayer_dialog_settings(&right).super_weapons);
    }

    #[test]
    fn build_off_ally_key_overrides_default() {
        let ini = IniFile::from_str("[MultiplayerDialogSettings]\nBuildOffAlly=no\n");
        assert!(!GameOptions::from_multiplayer_dialog_settings(&ini).build_off_ally);
    }

    #[test]
    fn normalized_animation_rates_match_original_execution() {
        // Original5FB2E0 executes independently for all56 supplied controls;
        // neither the table nor the formula is duplicated in this expectation.
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/fv_cell_attack/speed_normalize.json",
        ))
        .unwrap();
        let rows = native["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 56);
        for row in rows {
            let options = GameOptions {
                game_speed: row["stored_speed"].as_i64().unwrap() as i32,
                ..GameOptions::default()
            };
            assert_eq!(
                options.normalized_anim_delay(row["input_delay"].as_u64().unwrap() as u16),
                row["returned_eax"].as_u64().unwrap() as u16,
                "{row}"
            );
        }
    }
}
