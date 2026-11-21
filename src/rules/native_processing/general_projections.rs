//! `ProcessedRulesLayers` RulesClass scalar and `[General]` list accessors,
//! plus the hand-off of the INI and construction trace.

use super::ProcessedRulesLayers;
#[cfg(any(test, feature = "test-support"))]
use super::RulesTypeFamily;
use super::{CountryProjection, GeneralTypeLists, NativeTypeConstructionTrace, SideProjection};
use crate::rules::color_add::ColorAddSelectors;
use crate::rules::color_add::ColorAddTable;
use crate::rules::crate_rules::CrateRules;
use crate::rules::ini_parser::IniFile;
use crate::rules::missile_spawn::MissileSpawnRules;
use crate::rules::object_type::ObjectCategory;
use crate::rules::powerups::PowerupTable;
use crate::rules::prerequisite::Prerequisite;
use crate::rules::ruleset::DetailRules;
use crate::rules::ruleset::GeneralBuildingTypes;
use crate::rules::ruleset::PrismSupportRules;
use crate::util::native_x87::NativeF64Bits;

impl ProcessedRulesLayers {
    pub(crate) fn select_anim_rules(&self) -> (&str, &str, &str, &[String]) {
        let state = &self
            .native_type_construction_trace
            .registry_state()
            .select_anim;
        (
            &state.lightning_warhead,
            &state.weather_con_bolt_explosion,
            &state.weapon_nullify_anim,
            &state.splash_list,
        )
    }

    pub(crate) fn wind_direction(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_wind_direction
    }

    /// ReadGeneral670DE9/670E30/670E57, current scalar defaults across
    /// layers and Type resets (original gamemd).
    pub(crate) fn repair_service_inputs(&self) -> (i32, f64, f64) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_infantry_repair_step,
            f64::from_bits(state.rules_unit_repair_rate.bits()),
            f64::from_bits(state.rules_infantry_repair_rate.bits()),
        )
    }

    /// Rules6660AE/ReadGeneral671858: retained scalar, surviving Type resets.
    pub(crate) fn charge_to_drain_ratio(&self) -> NativeF64Bits {
        self.native_type_construction_trace
            .registry_state()
            .rules_charge_to_drain_ratio
    }

    pub(crate) fn gravity(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_gravity
    }

    /// Rules+1144 survives Type reset and supplies each admitted Terrain
    /// reader's fallback when Strength is -1 (ReadGeneral671DD2..671DEB).
    pub(crate) fn tree_strength(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_tree_strength
    }

    pub(crate) fn threat_coefficient_defaults(
        &self,
    ) -> super::threat_coefficients::ThreatCoefficients {
        self.native_registry_state()
            .rules_threat_coefficient_defaults
    }

    pub(crate) fn techno_threat_coefficients(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            super::threat_coefficients::ThreatCoefficients,
        ),
    > {
        let state = self.native_registry_state();
        state.techno_type_order.iter().map(|&(family, index)| {
            let member = &state.families[&family][index];
            (
                family.object_category().expect("TechnoType family"),
                member.native_stored_id.as_str(),
                member.threat_coefficients,
            )
        })
    }

    pub(crate) fn detail(&self) -> DetailRules {
        self.native_type_construction_trace
            .registry_state()
            .rules_detail
    }

    /// Rules+1668/+1670 after the reached AudioVisual passes, including cold
    /// startup and Type reset. Retain raw literal/default and authored f32-
    /// widened doubles; only the consuming projection truncates to frames.
    /// Original controls: trigger_map_lighting.json rate_controls (+meta).
    pub(crate) fn ambient_change_inputs(&self) -> (NativeF64Bits, NativeF64Bits) {
        let registry = self.native_type_construction_trace.registry_state();
        (
            registry.rules_ambient_change_rate,
            registry.rules_ambient_change_step,
        )
    }

    /// ReadAudioVisual66A679..66A693/66B49E..66B4BE retains these across
    /// Process and Type resets; the Rules registry remains their sole owner.
    pub(crate) fn shroud_growth_inputs(&self) -> (bool, NativeF64Bits) {
        let registry = self.native_type_construction_trace.registry_state();
        (registry.rules_shroud_grow, registry.rules_shroud_rate)
    }

    /// Rules+1648 after the reached ReadAudioVisual66B4CA..66B4E4 passes.
    pub(crate) fn fog_rate(&self) -> NativeF64Bits {
        self.native_type_construction_trace
            .registry_state()
            .rules_fog_rate
    }

    /// Immutable projection of the retained global land-speed table.
    pub(crate) fn terrain_rules(&self) -> &crate::rules::terrain_rules::TerrainRules {
        &self.native_registry_state().rules_land
    }

    /// ReadAudioVisual669A60/669A99/669FC3 fixed-catalog IDs. These scalar
    /// Rules fields survive TypeReset6686C0 and Rules Save674730/Load675210.
    pub(crate) fn bomb_ticking_sound(&self) -> Option<&str> {
        self.native_registry_state()
            .rules_global_sounds
            .bomb_ticking()
    }

    pub(crate) fn bomb_attach_sound(&self) -> Option<&str> {
        self.native_registry_state()
            .rules_global_sounds
            .bomb_attach()
    }

    pub(crate) fn psychic_reveal_activate_sound(&self) -> Option<&str> {
        self.native_registry_state()
            .rules_global_sounds
            .psychic_reveal()
    }

    /// ReadAudioVisual66B673..66B74E: current Unit, Infantry, Aircraft
    /// extra body light, in signed milliunits, retained with RulesClass.
    pub(crate) fn class_extra_lights(&self) -> [i32; 3] {
        self.native_registry_state().rules_class_extra_lights
    }

    /// ReadAudioVisual6693AC/6693F6/66AA93 fixed Voc bindings. RulesClass
    /// keeps these scalars through type reset, RMG and Save674730/Load675210.
    pub(crate) fn ui_sounds(&self) -> [Option<&str>; 3] {
        self.native_registry_state().rules_global_sounds.ui_sounds()
    }

    /// ReadAudioVisual669AE3/669FFC/66A0C1 retains these fixed Voc IDs.
    pub(crate) fn mind_control_sounds(&self) -> [Option<&str>; 3] {
        let sounds = &self.native_registry_state().rules_global_sounds;
        [
            sounds.mind_control(),
            sounds.mind_cleared(),
            sounds.mastermind_overload(),
        ]
    }

    /// Final Difficulty readers after the ordered Process passes. Keeping the
    /// typed rows preserves literal f64 defaults separately from ReadDouble's
    /// widened f32 authored values (ReadDifficulty @ 0x0066D270).
    pub(crate) fn difficulty_rows(&self) -> &[crate::rules::ruleset::DifficultyRules; 3] {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_difficulty_rows
    }

    /// Country+C0 after each reached HouseType ReadINI51193D. The typed
    /// value preserves catalog availability at the actual read, unlike final
    /// projected Color strings resolved against later additions.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn country_color_scheme(&self, country: &str) -> Option<i32> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Country)?
            .iter()
            .find(|member| member.native_stored_id.eq_ignore_ascii_case(country))
            .map(|member| member.country_color_scheme)
    }

    /// Ordered live Country identities, Color+C0 and Side+BC. Side672440
    /// writes BC before HouseType511850 can override it in ReadTypeData.
    /// A constructor-only Country retains BC=-1 (not an invented side), an
    /// empty ParentCountry and empty veteran lists.
    pub(crate) fn country_projection(&self) -> impl Iterator<Item = CountryProjection<'_>> {
        self.native_type_construction_trace
            .registry_state()
            .country_projection()
    }

    pub(crate) fn side_projection(&self) -> impl Iterator<Item = SideProjection<'_>> {
        self.native_type_construction_trace
            .registry_state()
            .side_projection()
    }

    pub(crate) fn prism_support(&self) -> PrismSupportRules {
        self.native_type_construction_trace
            .registry_state()
            .rules_prism_support
    }

    /// ReadAI67399E..6739C5's retained Rules+1150 after all reached passes.
    pub(crate) fn patrol_scan_minutes(&self) -> NativeF64Bits {
        self.native_type_construction_trace
            .registry_state()
            .rules_patrol_scan_minutes
    }

    pub(crate) fn ai_hate_delays(&self) -> &[i32] {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_ai_hate_delays
    }

    /// Rules+147C/+14BC after ordered General/AudioVisual reads. Original
    /// ctor665650 and read blocks66E5D5/66B385 establish null/1024 defaults,
    /// empty retention and exact-case keys; the existing factory owns identity.
    pub(crate) fn drop_zone_controls(&self) -> (Option<&str>, i32) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_drop_zone_anim.as_deref(),
            state.rules_drop_zone_radius,
        )
    }

    /// `[General] Behind=` (Rules `+0xB8`) after the ordered General reads
    /// (`0x0066E4F5..0x0066E513`): the constructor's null (`0x00665765`),
    /// empty retention and the AnimType factory's identity.
    pub(crate) fn behind(&self) -> Option<&str> {
        self.native_type_construction_trace
            .registry_state()
            .rules_behind
            .as_deref()
    }

    /// `[General] PrismType=` (Rules `+0x498`), the stored ID of its type.
    pub(crate) fn prism_type(&self) -> Option<&str> {
        self.native_type_construction_trace
            .registry_state()
            .rules_prism_type
            .as_deref()
    }

    /// Phobos `[AudioVisual] TimerBlinkColorScheme=`, the runtime ColorScheme
    /// index the ordered passes resolved.
    #[cfg(feature = "phobos")]
    pub(crate) fn timer_blink_color_scheme(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_timer_blink_color_scheme
    }

    /// Phobos `[AudioVisual] MissingCameo=`, retained with the Rules scalars
    /// rather than read from the compatibility projection.
    #[cfg(feature = "phobos")]
    pub(crate) fn missing_cameo(&self) -> &str {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_missing_cameo
    }

    /// `[General] PrerequisiteProcAlternate=` (Rules `+0x400`), the stored ID
    /// of its UnitType.
    pub(crate) fn prerequisite_proc_alternate(&self) -> Option<&str> {
        self.native_type_construction_trace
            .registry_state()
            .rules_prerequisite_proc_alternate
            .as_deref()
    }

    /// The six `[General] Prerequisite*` lists after the last pass, in
    /// [`PrerequisiteGroup::ALL`] order.
    pub(crate) fn prerequisite_lists(&self) -> &[Vec<Prerequisite>; 6] {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_prerequisite_lists
    }

    /// Each TechnoType's `Prerequisite=` and `PrerequisiteOverride=` after
    /// the last pass that read them.
    pub(crate) fn prerequisite_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, &[Prerequisite], &[Prerequisite])> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        member.prerequisite.as_slice(),
                        member.prerequisite_override.as_slice(),
                    )
                })
            })
    }

    /// `[General]` gates, WallTower and power plants (Rules `+0x86C..+0x87C`,
    /// `+0x89C..+0x8A8`), stored IDs.
    pub(crate) fn building_types(&self) -> &GeneralBuildingTypes {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_building_types
    }

    /// Retained ColorAdd prefix/table and AudioVisual indexes, not a text merge.
    pub(crate) fn color_add_state(&self) -> (&ColorAddTable, ColorAddSelectors) {
        let state = self.native_type_construction_trace.registry_state();
        (&state.rules_color_add, state.rules_color_add_selectors)
    }

    pub(crate) fn projectile_rule_controls(&self) -> (f64, i32, [u8; 3]) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_missile_rot_var,
            state.rules_safety_altitude,
            state.rules_line_trail_override,
        )
    }

    /// Rules+310, constructor665ABE=15; CombatDamage66C914..66C92E.
    pub(crate) fn mind_control_attack_line_frames(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_mind_control_attack_line_frames
    }

    /// Rules+314/+318, retained ReadCombatDamage66C973..66C9AC scalars.
    pub(crate) fn drain_money_controls(&self) -> (i32, i32) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_drain_money_frame_delay,
            state.rules_drain_money_amount,
        )
    }

    /// Rules+D6C, retained ReadGeneral66FC36..66FC50 boolean.
    pub(crate) fn attack_cursor_on_disguise(&self) -> bool {
        self.native_type_construction_trace
            .registry_state()
            .rules_attack_cursor_on_disguise
    }

    /// Rules+17EF, retained ReadGeneral66EB03..66EB1C boolean.
    pub(crate) fn allow_shrouded_subterannean_moves(&self) -> bool {
        self.native_type_construction_trace
            .registry_state()
            .rules_allow_shrouded_subterannean_moves
    }

    /// Rules+1866, retained by AudioVisual66B7A7..66B7D2 through the
    /// current-RGB default of ReadColorRGB474B50.
    pub(crate) fn chrono_beam_color(&self) -> [u8; 3] {
        self.native_type_construction_trace
            .registry_state()
            .rules_chrono_beam_color
    }

    /// RulesClass's retained ObjectAI fall-rate limits, with and without a
    /// parachute respectively (`+0x7B8`, `+0x7BC`).
    pub(crate) fn fall_rate_controls(&self) -> (i32, i32) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_parachute_max_fall_rate,
            state.rules_no_parachute_max_fall_rate,
        )
    }

    /// Process-resident Rules+14F8/+14FC/+1500, Allied/Soviet/Third order.
    pub(crate) fn survivor_divisors(&self) -> [i32; 3] {
        self.native_type_construction_trace
            .registry_state()
            .rules_survivor_divisors
    }

    /// Retained AI.CompEasyBonus at Rules+17E3 (ReadAI673D18).
    pub(crate) fn comp_easy_bonus(&self) -> bool {
        self.native_type_construction_trace
            .registry_state()
            .rules_comp_easy_bonus
    }

    /// Retained General.MaximumCheerRate at Rules+580 (ReadGeneral66EBD4).
    pub(crate) fn maximum_cheer_rate(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_maximum_cheer_rate
    }

    /// Rules+1800, ctor6677A3=300, General671E3B..671E81.
    pub(crate) fn talk_bubble_time(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_talk_bubble_time
    }

    pub(crate) fn spotlight(&self) -> crate::rules::ruleset::SpotlightRules {
        self.native_type_construction_trace
            .registry_state()
            .rules_spotlight
    }

    /// Rules+490, +298, +29C, +FF0, retained across ordered Process calls.
    pub(crate) fn ion_blast_inputs(&self) -> (i32, &str, &str, &str) {
        let state = self.native_type_construction_trace.registry_state();
        (
            state.rules_ion_cannon_damage,
            &state.rules_ion_blast_anim,
            &state.rules_ion_beam_anim,
            &state.rules_ion_cannon_warhead,
        )
    }

    /// Consume only the typed-reader compatibility projection and deliberately
    /// discard the native constructor/registry receipt.
    ///
    /// Gameplay-equivalent fresh loads must use
    /// [`Self::into_ini_and_native_type_construction_trace`] instead.
    pub(crate) fn into_projection_discarding_native_receipt(self) -> IniFile {
        self.ini
    }

    pub fn content_hash(&self) -> u64 {
        self.content_hash
    }

    pub fn crate_rules(&self) -> &CrateRules {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_crate
    }

    pub fn powerups(&self) -> &PowerupTable {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_powerups
    }

    /// The three `RocketStruct` blocks and their warheads after every pass.
    pub fn missile_spawn(&self) -> &MissileSpawnRules {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_missile_spawn
    }

    pub(super) fn general_type_lists(&self) -> &GeneralTypeLists {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_general_type_lists
    }

    pub(crate) fn metallic_debris(&self) -> &[String] {
        &self.general_type_lists().metallic_debris
    }

    pub(crate) fn bridge_explosions(&self) -> &[String] {
        &self.general_type_lists().bridge_explosions
    }

    pub(crate) fn weather_con_clouds(&self) -> &[String] {
        &self.general_type_lists().weather_con_clouds
    }

    pub(crate) fn weather_con_bolts(&self) -> &[String] {
        &self.general_type_lists().weather_con_bolts
    }

    /// The four `*ParaDropInf=` lists as stored type IDs: American, Allied,
    /// Soviet, Yuri.
    pub(crate) fn paradrop_infantry(&self) -> &[Vec<String>; 4] {
        &self.general_type_lists().paradrop_infantry
    }

    pub(crate) fn anim_to_infantry(&self) -> &[String] {
        &self.general_type_lists().anim_to_infantry
    }

    pub(crate) fn default_mirage_disguises(&self) -> &[String] {
        &self.general_type_lists().default_mirage_disguises
    }

    pub(crate) fn infantry_disguises(&self) -> &[Option<String>; 3] {
        &self.general_type_lists().infantry_disguises
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn native_type_construction_trace(&self) -> &NativeTypeConstructionTrace {
        &self.native_type_construction_trace
    }

    pub(crate) fn into_ini_and_native_type_construction_trace(
        self,
    ) -> (IniFile, NativeTypeConstructionTrace) {
        (self.ini, self.native_type_construction_trace)
    }
}
