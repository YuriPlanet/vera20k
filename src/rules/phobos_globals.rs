//! Phobos `RulesExt::LoadFromINI` globals, read from `[CombatDamage]`,
//! `[AudioVisual]` and `[General]` on every rules pass
//! (`Ext/Rules/Body.cpp:230-232`, `:291-304`, `:470-471`, `:629-632` and the
//! shrapnel block).
//!
//! Phobos reads these in its own extension reader, once per `Process`, with the
//! current field as the default; a later layer changes only the keys it names.
//! `ProcessedRulesLayers::ini()` is already that merged state, so one read here
//! is equivalent.
//!
//! The globals are `Valueable`; the booleans construct `false`
//! (`Ext/Rules/Body.h:464-465`, `:968-969`, `:729`, `:731`) except
//! `AutoDeath.AllowLimboed` (`:1027`),
//! `AffectsInvokerOnly.IgnoreInvokerState` (`:923`),
//! `Shrapnel.ObeyWarheadTriggerConditions` (`:972`),
//! `Vertical.AircraftFix` (`:990`),
//! `CanTarget.IronCurtained` (`Ext/Rules/Body.h:833`),
//! `AutoTarget.IronCurtained` (`:834`),
//! `DrainMoneyDisplay.OnTarget.UseDisplayIncome` (`:732`) and both
//! `Explodes.*` globals (`:885-886`), which construct `true`; the
//! `AffectedHouse` globals construct `All` (`:728`, `:730`).
//! The type-level keys are `Nullable`, and their consumer passes the
//! global through `Valueable<T>::operator const T&`, so an unset type key
//! falls back to that global's value. A `Valueable` global has no unset state
//! and no `none`/`<default>` clearing token; do not give these reads
//! `Nullable` semantics.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::mission_data::MissionType;
use crate::rules::phobos_shields::AffectedHouse;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

/// `Leptons(-1)`'s constructor value for saves that predate the field.
fn minus_one_i32() -> i32 {
    -1
}

/// A `Valueable<bool>` global constructed `true`, for serde defaults.
fn true_bool() -> bool {
    true
}

/// The `Team` constructor default for `[AudioVisual] RevealHouses=`.
fn reveal_houses_default() -> AffectedHouse {
    AffectedHouse::TEAM
}

/// A `Valueable<float>` global constructed 1.0, for serde defaults.
fn one_f32() -> crate::util::native_x87::NativeF32Bits {
    crate::util::native_x87::NativeF32Bits::ONE
}

/// A `Valueable<float>` global constructed 0.0, for serde defaults.
fn zero_f32() -> NativeF32Bits {
    NativeF32Bits::POSITIVE_ZERO
}

/// A `Valueable<double>` global constructed 0.0, for serde defaults.
fn zero_f64() -> NativeF64Bits {
    NativeF64Bits::POSITIVE_ZERO
}

/// The Phobos `[General] BunkerStateUpdateDelay=` constructor value.
fn fifteen() -> i32 {
    15
}

/// `section.read_phobos_double` kept as raw bits so `PhobosGlobals` stays
/// `Eq + Hash`; the parser and its failed-read fallback are unchanged.
fn read_double_bits(section: &IniSection, key: &str, current: NativeF64Bits) -> NativeF64Bits {
    NativeF64Bits::from_bits(
        section
            .read_phobos_double(key, f64::from_bits(current.bits()))
            .to_bits(),
    )
}

/// `[General] SinkSpeed=`'s constructor value (`Ext/Rules/Body.h:901`).
fn default_sink_speed() -> i32 {
    5
}

/// `[General] ParadropMission=`'s constructor value, `Guard`
/// (`Ext/Rules/Body.h:934`).
fn default_paradrop_mission() -> MissionType {
    MissionType::Guard
}

/// `[General] AIParadropMission=`'s constructor value, `Hunt`
/// (`Ext/Rules/Body.h:935`).
fn default_ai_paradrop_mission() -> MissionType {
    MissionType::Hunt
}

/// `[General] DamagedSpeed=`'s constructor value, 0.75
/// (`Ext/Rules/Body.h:840`; the stock Drive/Ship damaged-mover factor at
/// `0x7E7FC0`).
fn default_damaged_speed() -> NativeF64Bits {
    NativeF64Bits::from_bits(0x3fe8_0000_0000_0000)
}

/// `[General] CrushSlowdownMultiplier=`'s constructor value, 0.2
/// (`Ext/Rules/Body.h:715`; the same binary64 as the stock Drive/Ship cap).
fn crush_slowdown_default() -> NativeF64Bits {
    NativeF64Bits::from_bits(0x3fc9_9999_9999_999a)
}

/// The `Nullable<double>` counterpart of [`read_double_bits`].
fn read_nullable_double_bits(
    section: &IniSection,
    key: &str,
    current: Option<NativeF64Bits>,
) -> Option<NativeF64Bits> {
    section
        .read_phobos_nullable_double(key, current.map(|value| f64::from_bits(value.bits())))
        .map(|value| NativeF64Bits::from_bits(value.to_bits()))
}

/// `[CombatDamage]` globals this port reads.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PhobosGlobals {
    /// `Shrapnel.AffectsGround=`, default `no`: admit shrapnel when the impact
    /// cell holds no ground object (`Ext/Bullet/Hooks.cpp:276`).
    pub shrapnel_affects_ground: bool,
    /// `Shrapnel.AffectsBuildings=`, default `no`: admit shrapnel when the
    /// impact cell's first ground object is a building (`:270`).
    pub shrapnel_affects_buildings: bool,
    /// `AutoDeath.TechnosExist.AllowLimboed=`, default `no`: count limboed
    /// instances in `AutoDeath.TechnosExist=` scans
    /// (`Ext/Rules/Body.cpp:632`, consumed at `Ext/Techno/Body.Update.cpp:250`).
    pub auto_death_technos_exist_allow_limboed: bool,
    /// `AutoDeath.TechnosDontExist.AllowLimboed=`, default `no` (`:631`,
    /// consumed at `:239`).
    pub auto_death_technos_dont_exist_allow_limboed: bool,
    /// `Shrapnel.IgnoreHitBuildings=`, default `no`: a building already hit by
    /// one of this emission's children is skipped for the rest
    /// (`Ext/Bullet/Hooks.cpp:295,303,348`).
    pub shrapnel_ignore_hit_buildings: bool,
    /// `Shrapnel.UseWeaponTargeting=`, default `no`: replace the ally test with
    /// the child weapon's own target eligibility (`:309-342`).
    pub shrapnel_use_weapon_targeting: bool,
    /// `Shrapnel.ObeyWarheadTriggerConditions=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:557`, constructor `Ext/Rules/Body.h:972`): a
    /// bullet whose type leaves the key unset applies the warhead's health,
    /// veterancy, neutral and invoker conditions before its whole body
    /// (`Ext/Bullet/Hooks.DetonateLogics.cpp:22-29`); failing them keeps only
    /// the impact extras.
    #[serde(default = "true_bool")]
    pub shrapnel_obey_warhead_trigger_conditions: bool,
    /// `Overload.ParticleSysCount=`, default 5 (`Ext/Rules/Body.h:670`):
    /// the number of spark systems a damaging overload row throws.
    pub overload_particle_sys_count: i32,
    /// `AffectsInvokerOnly.IgnoreInvokerState=`, default **`yes`**
    /// (`Ext/Rules/Body.h:923`): the fallback for a warhead whose own key is
    /// unset when its damage has no invoker.
    pub affects_invoker_only_ignore_invoker_state: bool,
    /// `[AudioVisual] ShowDesignatorRange=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:176`, constructor `Ext/Rules/Body.h:223,760`): the
    /// rules half of the armed superweapon's designator/inhibitor range rings
    /// (`Tactical_SuperLinesCircles_ShowDesignatorRange @ 0x6DBE74`). The
    /// `[Phobos]` client switch and the armed `[SuperWeaponType]` switch must
    /// also be on.
    #[serde(default = "true_bool")]
    pub show_designator_range: bool,
    /// `[AudioVisual] SuperWeaponTimer.Percentage=`, `Valueable<bool>` default
    /// **no** (`Ext/Rules/Body.cpp:144`, `Ext/Rules/Body.h:605`): the fallback
    /// for a Super whose `ShowTimer.Percentage=` is cleared; a timed Super
    /// then prints its elapsed recharge share as `%.2f%%`
    /// (`TacticalClass_Render_DrawSuperTimer_PercentageTimer @ 0x6D4A10`,
    /// `Misc/Hooks.UI.cpp:610-631`).
    pub super_weapon_timer_percentage: bool,
    /// `[AudioVisual] ToolTip.Background.Color=`, `Valueable<ColorStruct>`
    /// default black (`Ext/Rules/Body.cpp:169`, constructor `Body.h:722`):
    /// the tooltip's translucent backing colour
    /// (`CCToolTip_Draw2_FillRect @ 0x478FDC`, `Misc/PhobosToolTip.cpp:434-437`).
    pub tooltip_background_color: [u8; 3],
    /// `[AudioVisual] ToolTip.Background.Opacity=`, `Valueable<int>` default
    /// 100 (`Body.cpp:170`, `Body.h:723`): `FillRectTrans`'s 0..100 percent.
    pub tooltip_background_opacity: i32,
    /// `[AudioVisual] ToolTip.Background.BlurSize=`, `Valueable<float>`
    /// default 0.0 (`Body.cpp:171`, `Body.h:724`): the radius the client's
    /// `ToolTipBlur` switch blurs the backing by. VERA reads it for parity and
    /// leaves the blur pass a residual.
    pub tooltip_background_blur_size: crate::util::native_x87::NativeF32Bits,
    /// `[AudioVisual] AnimRemapDefaultColorScheme=`, default **0**
    /// (`ValueableIdx<ColorScheme>`, `Ext/Rules/Body.h:750`, read at
    /// `Ext/Rules/Body.cpp:174`): the `ColorScheme::Array` index an ownerless
    /// `AltPalette` anim draws with (`0x4232E2`, `Ext/Anim/Hooks.cpp:635`).
    /// `AltPalette.ApplyLighting` steps that index by one.
    pub anim_remap_default_color_scheme: i32,
    /// `[AudioVisual] WarheadParticleAlphaImageIsLightFlash=`, default **no**
    /// (`Ext/Rules/Body.cpp:403`, `Ext/Rules/Body.h:819`): the fallback for a
    /// warhead whose own `Particle.AlphaImageIsLightFlash=` is unset.
    pub warhead_particle_alpha_image_is_light_flash: bool,
    /// `[AudioVisual] LightFlashAlphaImageDetailLevel=`, default **0**
    /// (`Ext/Rules/Body.cpp:406`, `Ext/Rules/Body.h:822`): a light-flash
    /// particle AlphaImage above `GameOptions.DetailLevel` is not created
    /// (`0x005F5053`).
    pub light_flash_alpha_image_detail_level: i32,
    /// `[AudioVisual] FiringAnim.Update=`, default **no**
    /// (`Ext/Rules/Body.cpp:548`, `Ext/Rules/Body.h:964`): the fallback for
    /// a weapon whose own `Anim.Update=` is unset.
    pub firing_anim_update: bool,
    /// `[General] ExtraRange.TargetMoving` (`Valueable<Leptons>`, default 0,
    /// `Ext/Rules/Body.cpp:511`): added to a weapon's range while the target's
    /// locomotor really moves (`Ext/Techno/Hooks.WeaponRange.cpp:157-162`).
    pub extra_range_target_moving: i32,
    /// `ExtraRange.TargetMoving.CloseRangeOnly=` (default false,
    /// `Ext/Rules/Body.cpp:512`): only a `CloseRange=yes` firer takes the
    /// target-moving bonus.
    pub extra_range_target_moving_close_range_only: bool,
    /// `ExtraRange.FirerMoving=` (default 0, `Ext/Rules/Body.cpp:513`): added
    /// while the firer's locomotor really moves (`:164-168`).
    pub extra_range_firer_moving: i32,
    /// `ExtraRange.Prefiring=` (default 0, `Ext/Rules/Body.cpp:514`): added
    /// while the firer is pre-firing (`:152-155`).
    pub extra_range_prefiring: i32,
    /// `ExtraRange.Prefiring.IncludeBurst=` (`Valueable<bool>` default true,
    /// `Ext/Rules/Body.cpp:515`): a burst index other than zero also counts
    /// as pre-firing (`Ext/Techno/Hooks.WeaponRange.cpp:79-80`).
    pub extra_range_prefiring_include_burst: bool,
    /// `[AudioVisual] Pips.Generic.Size` (`Valueable<Point2D>` default
    /// `{4,0}`, `Ext/Rules/Body.cpp:160`, `Body.h:620`): the per-slot X/Y
    /// advance every non-Ammo pip arm uses
    /// (`TechnoClass_DrawPips_Sizes @ 0x709B2E`,
    /// `Ext/Techno/Hooks.Pips.cpp:156..180`).
    pub pips_generic_size: [i32; 2],
    /// `Pips.Generic.Buildings.Size=` (default `{4,2}`, `Body.h:621`).
    pub pips_generic_buildings_size: [i32; 2],
    /// `Pips.Ammo.Size=` (default `{4,0}`, `Ext/Rules/Body.cpp:162`,
    /// `Body.h:622`): the Ammo arm's advance, overridden per type by
    /// `AmmoPipSize`.
    pub pips_ammo_size: [i32; 2],
    /// `Pips.Ammo.Buildings.Size=` (default `{4,2}`, `Body.h:623`).
    pub pips_ammo_buildings_size: [i32; 2],
    /// `[AudioVisual] HeightShadowScaling=`, default `no` (`Ext/Rules/Body.cpp:186`,
    /// constructor `Ext/Rules/Body.h:630`): a drawing unit scales its voxel
    /// shadow by its live height (`Ext/TechnoType/Hooks.MatrixOp.cpp:677`).
    /// `Ext/Rules/Body.cpp:187` forces it off when `AirShadowBaseScale > 0.98`.
    pub height_shadow_scaling: bool,
    /// `[AudioVisual] HeightShadowScaling.MinScale=`, default `0.0` (`:189`).
    /// Raw bits keep `PhobosGlobals` `Eq + Hash`.
    pub height_shadow_scaling_min_scale: NativeF64Bits,
    /// `Ext/Rules/Body.cpp:183-184`: `-ln(min(AirShadowBaseScale, 1.0))`, the
    /// exponent base `Pade2_2` consumes; default `0.693376137`
    /// (`Ext/Rules/Body.h:632`, the log of a half-size shadow).
    pub air_shadow_base_scale_log: NativeF64Bits,
    /// `[AudioVisual] DrawTurretShadow=`, default `no` (`Ext/Rules/Body.cpp:173`,
    /// `Valueable<bool>` constructor `Ext/Rules/Body.h:737`): the fallback for a
    /// drawing unit's ART `TurretShadow=`
    /// (`Ext/TechnoType/Hooks.MatrixOp.cpp:803`).
    pub draw_turret_shadow: bool,
    /// `AutoDeath.OnOwnerChange.IgnoreRevertOnExit=`, default `no`
    /// (`Ext/Rules/Body.cpp:630`).
    pub auto_death_on_owner_change_ignore_revert_on_exit: bool,
    /// `AutoDeath.AllowLimboed=`, **default `yes`**: Phobos constructs it
    /// `true` (`Ext/Rules/Body.h:1027`) and `Ext/Rules/Body.cpp:629` reads it.
    pub auto_death_allow_limboed: bool,
    /// `[General] Convert.ResetMindControl=`, default `no`
    /// (`Ext/Rules/Body.cpp:222`, constructor `Ext/Rules/Body.h:666`): the
    /// fallback for a type's `Convert.ResetMindControl` when it is unset.
    pub convert_reset_mind_control: bool,
    /// `[General] BuildingGuardRetryDelay=`, default unset
    /// (`Ext/Rules/Body.cpp:579`): the fallback armed building guard recheck
    /// delay for a BuildingType whose `GuardRetryDelay` is unset
    /// (`Ext/Building/Hooks.cpp:1288`).
    pub building_guard_retry_delay: Option<crate::rules::ini_value::PartialIntPair>,
    /// `[General] ParadropEndDelay=`, default 5 (`Ext/Rules/Body.cpp:523`,
    /// constructor `Ext/Rules/Body.h:937`): game frames an overflying paradrop
    /// plane waits after its last passenger, the fallback for an AircraftType's
    /// own `ParadropEndDelay` (`Ext/Aircraft/Hooks.cpp:1278`).
    #[serde(default = "default_paradrop_end_delay")]
    pub paradrop_end_delay: i32,
    /// `[General] Explodes.KillPassengers=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:470`, constructor `Ext/Rules/Body.h:885`): the
    /// fallback for a type's key; `no` leaves a destroyed object's passengers
    /// out of the death arm's kill.
    #[serde(default = "true_bool")]
    pub explodes_kill_passengers: bool,
    /// `[General] Explodes.DuringBuildup=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:471`, constructor `Ext/Rules/Body.h:886`): the
    /// fallback for a BuildingType's key; `no` makes a building destroyed
    /// while under Construction or Selling behave as if `Explodes=` were
    /// false.
    #[serde(default = "true_bool")]
    pub explodes_during_buildup: bool,
    /// `[General] NotHuman.RandomDeathSequence=`, default `no`
    /// (`Ext/Rules/Body.cpp:621`, constructor `Ext/Rules/Body.h:1016`): the
    /// fallback for an InfantryType's key.
    pub not_human_random_death_sequence: bool,
    /// `[General] KeepRange.AllowAI=`, default `no` (`Ext/Rules/Body.cpp:269`,
    /// constructor `Ext/Rules/Body.h:187`): the fallback for a weapon's key.
    pub keep_range_allow_ai: bool,
    /// `[General] KeepRange.AllowPlayer=`, default `no` (`:270`, `Body.h:188`).
    pub keep_range_allow_player: bool,
    /// `[General] AIGuardAreaTargetingDelay=` (`Nullable<int>`, unset;
    /// `Ext/Rules/Body.cpp:430`, `Body.h:315`, constructor `:826`): the
    /// AI-controlled Area Guard scan-delay override
    /// (`TechnoClass_TargetAndEstimateDamage_TargetingDelay @ 0x70982C`); a
    /// type key wins over this, and a human-controlled house ignores both.
    #[serde(default)]
    pub ai_guard_area_targeting_delay: Option<i32>,
    /// `[General] AINormalTargetingDelay=` (`Nullable<int>`, unset;
    /// `Ext/Rules/Body.cpp:428`, `Body.h:313,824`): the AI-controlled ordinary
    /// scan-delay override; unset keeps `RulesClass::NormalTargetingDelay`
    /// (`Ext/Techno/Hooks.Targeting.cpp:12-45`, hook `0x70982C`).
    #[serde(default)]
    pub ai_normal_targeting_delay: Option<i32>,
    /// `[General] PlayerNormalTargetingDelay=` (`Nullable<int>`, unset;
    /// `Ext/Rules/Body.cpp:429`, `Body.h:314,825`): the human-controlled
    /// ordinary override (`0x70982C`).
    #[serde(default)]
    pub player_normal_targeting_delay: Option<i32>,
    /// `[General] PlayerGuardAreaTargetingDelay=` (`Nullable<int>`, unset;
    /// `Ext/Rules/Body.cpp:431`, `Body.h:316,827`): the human-controlled Area
    /// Guard override (`0x70982C`).
    #[serde(default)]
    pub player_guard_area_targeting_delay: Option<i32>,
    /// `[General] AIAttackMoveTargetingDelay=` (`Nullable<int>`, unset;
    /// `Ext/Rules/Body.cpp:432`, `Body.h:317,828`): the AI-controlled
    /// attack-move override (`0x70982C`, `MegaMissionIsAttackMove` arm).
    #[serde(default)]
    pub ai_attack_move_targeting_delay: Option<i32>,
    /// `[General] PlayerAttackMoveTargetingDelay=` (`Nullable<int>`, unset;
    /// `Ext/Rules/Body.cpp:433`, `Body.h:318,829`): the human-controlled
    /// attack-move override (`0x70982C`).
    #[serde(default)]
    pub player_attack_move_targeting_delay: Option<i32>,
    /// `[General] DistributeTargetingFrame=` (`Valueable<bool>`, default
    /// **false**; `Ext/Rules/Body.cpp:434`, `Body.h:319,830`): the global
    /// fallback of the TechnoType `DistributeTargetingFrame` override
    /// (`Ext/TechnoType/Body.cpp:1080`); when the consumer gate allows it, a
    /// fresh techno's targeting timer is re-armed at `45 + RandomFactor`
    /// (`Ext/Techno/Hooks.cpp:323-326`, hook `0x6F42F7`).
    #[serde(default)]
    pub distribute_targeting_frame: bool,
    /// `[General] DistributeTargetingFrame.AIOnly=` (`Valueable<bool>`,
    /// default **true**; `Ext/Rules/Body.cpp:435`, `Body.h:320,831`): with it
    /// on, a human-controlled house keeps the vanilla 45-frame construction
    /// anchor even when the distribute switch itself is on
    /// (`Ext/Techno/Hooks.cpp:323-326`).
    #[serde(default)]
    pub distribute_targeting_frame_ai_only: bool,
    /// `[AudioVisual] ToolTip.Background.BlurSize=` (`Valueable<float>`,
    /// default **0.0**; `Ext/Rules/Body.cpp:171`, `Body.h:202,724`): the
    /// fallback of a Side's `Nullable<float>` in the tooltip blur
    /// (`Misc/PhobosToolTip.cpp:439`, hook `0x478FDC`); native's
    /// `CLOSE_ENOUGH(blurSize, 0)` early return keeps a zero a no-op.
    #[serde(default = "zero_f32")]
    pub tool_tip_background_blur_size: NativeF32Bits,
    /// `[Basic] RepairBaseNodes=` (`Valueable<bool>`, default **false**;
    /// `Ext/Rules/Body.cpp:399`, `Body.h:302,817`): the fallback of a map
    /// house's `RepairBaseNodes` difficulty slots (`Ext/House/Body.cpp:626`);
    /// the resolved flag lets `BuildingClass::Unlimbo` mark a campaign AI
    /// building AI-repairable (`Ext/Building/Hooks.cpp:316-331`, hook
    /// `0x440B4F`).
    #[serde(default)]
    pub repair_base_nodes: bool,
    /// `[General] CrushSlowdownMultiplier=` (`Valueable<double>`, default
    /// 0.2; `Ext/Rules/Body.cpp:273`, `Body.h:191,715`): the cap for the
    /// Drive/Ship crush slowdown (`0x4B1150`, `0x6A0813`).
    #[serde(default = "crush_slowdown_default")]
    pub crush_slowdown_multiplier: NativeF64Bits,
    /// `[General] SkipCrushSlowdown=` (`Valueable<bool>`, default **false**;
    /// `Ext/Rules/Body.cpp:274`, `Body.h:192,716`): the fallback for a
    /// UnitType's `Nullable<bool>`; Drive and Ship skip the crush slowdown arm
    /// when it is set (`0x4B1146`/`0x6A0809`, `Ext/Unit/Hooks.Crushing.cpp:104-109`).
    #[serde(default)]
    pub skip_crush_slowdown: bool,
    /// `[General] AircraftFiringForceScatter=` (`Valueable<bool>`, default
    /// **true**; `Ext/Rules/Body.cpp:473`, `Body.h:383,888`): the fallback for
    /// an AircraftType's `FiringForceScatter`; false stops an attacking
    /// aircraft from forcing the target cell to scatter
    /// (`Ext/Aircraft/Hooks.cpp:324,341-355`).
    #[serde(default = "aircraft_firing_force_scatter_default")]
    pub aircraft_firing_force_scatter: bool,
    /// `[General] BunkerStateUpdateDelay=` (`Valueable<int>`, default **15**;
    /// `Ext/Rules/Body.cpp:617`, `Body.h:515,1012`): the fallback for a
    /// BuildingType's nullable; the Mission_Repair return while a tank bunker
    /// installs (`0x44C976`, `Ext/Building/Hooks.cpp:1474-1485`).
    #[serde(default = "fifteen")]
    pub bunker_state_update_delay: i32,
    /// `[General] ChronoSpherePreDelay=` (`Valueable<int>`, default **60**;
    /// `Ext/Rules/Body.cpp:120`, `Body.h:39,579`): the fallback for a
    /// TechnoType's nullable; the frames state 0 of the Chronosphere warp
    /// waits (`0x007197DF`, `Ext/TechnoType/Hooks.Teleport.cpp:154-163`).
    #[serde(default = "sixty")]
    pub chrono_sphere_pre_delay: i32,
    /// `[General] ChronoSphereDelay=` (`Valueable<int>`, default **0**;
    /// `Ext/Rules/Body.cpp:121`, `Body.h:40,580`): the fallback for a
    /// TechnoType's nullable; the extra frames a chrono-sphered object stays
    /// warped out after the warp-in (`Ext/TechnoType/Hooks.Teleport.cpp:165-188`,
    /// decremented by `Ext/Foot/Body.cpp:149-164`).
    #[serde(default)]
    pub chrono_sphere_delay: i32,
    /// `[General] MindControl.ThreatDelay=` (`Valueable<int>`, default **0**;
    /// `Ext/Rules/Body.cpp:380`, `Body.h:285,802`): the fallback for a
    /// warhead's `MindControl.ThreatDelay=`; frames a fresh capture keeps its
    /// new pawn shielded from its former allies' retaliation and
    /// auto-targeting (`Ext/Techno/Hooks.Misc.cpp:606-641`).
    #[serde(default)]
    pub mind_control_threat_delay: i32,
    /// `[JumpjetControls] RotateOnCrash=` (`Valueable<bool>`, default **true**;
    /// `Ext/Rules/Body.cpp:1355`, `Body.h:56,596`): the fallback for a
    /// TechnoType's nullable `JumpjetRotateOnCrash=`; the Jumpjet crash's
    /// facing spin (hook `0x54CB0E`).
    #[serde(default = "truth")]
    pub jumpjet_rotate_on_crash: bool,
    /// `[General] JumpjetClimbIgnoreBuilding=` (`Valueable<bool>`, default
    /// **false**; `Ext/Rules/Body.cpp:338`, `Body.h:244,768`): the fallback for
    /// a TechnoType's nullable `JumpjetClimbIgnoreBuilding=`; the jumpjet floor
    /// reference then ignores a building's top (hooks `0x54D820`/`0x54D859`/
    /// `0x54D8EA`, `Ext/Unit/Hooks.Jumpjet.cpp:439-457`).
    #[serde(default)]
    pub jumpjet_climb_ignore_building: bool,
    /// `[General] Sinkable=` (`Nullable<bool>`, unset; `Ext/Rules/Body.cpp:
    /// 484`, `Body.h:394,899`): the fallback for a UnitType's `Sinkable`;
    /// unset keeps the stock type-property gate.
    #[serde(default)]
    pub sinkable: Option<bool>,
    /// `[General] SinkSpeed=` (`Valueable<int>`, default 5;
    /// `Ext/Rules/Body.cpp:486`, `Body.h:396,901`): leptons per frame a unit
    /// sinks. A UnitType's nullable overrides it (`0x7364DC`).
    #[serde(default = "default_sink_speed")]
    pub sink_speed: i32,
    /// `[General] ParadropMission=` (`Valueable<Mission>`, Guard;
    /// `Ext/Rules/Body.cpp:520`, `Body.h:431,934`): the mission a
    /// human-owned paradropped `FootClass` queues after SpawnParachuted; a
    /// TechnoType's nullable overrides it (`Ext/Techno/Hooks.cpp:1883`).
    #[serde(default = "default_paradrop_mission")]
    pub paradrop_mission: MissionType,
    /// `[General] AIParadropMission=` (`Valueable<Mission>`, Hunt;
    /// `Ext/Rules/Body.cpp:521`, `Body.h:432,935`): the same for an
    /// AI-owned paradropped `FootClass` (`Ext/Techno/Hooks.cpp:1885`).
    #[serde(default = "default_ai_paradrop_mission")]
    pub ai_paradrop_mission: MissionType,
    /// `[General] AircraftWeapon.KickOutPassengers=` (`Valueable<bool>`,
    /// true; `Ext/Rules/Body.cpp:272`, `Body.h:190,714`): the fallback for a
    /// weapon's `KickOutPassengers` in `AircraftClass::Fire_At`'s passenger
    /// drop (`AircraftClass_Fire_KickOutPassengers @ 0x415EEE`).
    #[serde(default = "true_bool")]
    pub aircraft_weapon_kick_out_passengers: bool,
    /// `[General] SecondaryFireSequenceLandOnly=` (`Valueable<bool>`, true;
    /// `Ext/Rules/Body.cpp:623`, `Body.h:521,1018`): the fallback for the
    /// InfantryType art key; yes remaps an in-water SecondaryFire/Prone
    /// request to WetAttack (stock), no keeps its land record.
    #[serde(default = "true_bool")]
    pub secondary_fire_sequence_land_only: bool,
    /// `[General] DamagedSpeed=` (`Valueable<double>`, 0.75;
    /// `Ext/Rules/Body.cpp:414`, `Body.h:332,840`): the Drive/Ship
    /// below-Condition-Yellow speed factor; a UnitType's nullable overrides
    /// it (`LocomotionClass_Process_DamagedSpeedMultiplier @ 0x4B3DF0`).
    #[serde(default = "default_damaged_speed")]
    pub damaged_speed: NativeF64Bits,
    /// `[General] ArtImageSwap=`, default `no` (`Phobos.INI.cpp:262`):
    /// Phobos' ART `Image=` redirect for TechnoTypes. VERA's body SHP source
    /// already applies that redirect, so the SHP UnitType turret file name
    /// (`RuleSet::unit_turret_shp_id`) tracks the same base and this flag
    /// changes no name; it is read for parity only.
    #[serde(default)]
    pub art_image_swap: bool,
    /// `[General] KeepRange.EarlyStopFrame=`, default 0 (`:271`, `Body.h:189`).
    pub keep_range_early_stop: i32,
    /// `[AudioVisual] Parasite.GrappleAnim=`, `NullableIdx<AnimTypeClass>`
    /// read by declared-name `FindIndex` (`Ext/Rules/Body.cpp:425`): the
    /// fallback for a warhead's key; `SQDG` applies when both are unset.
    pub parasite_grapple_anim: Option<String>,
    /// `[General] Parasite.AllowWaterExit=`, `Nullable<bool>`
    /// (`Ext/Rules/Body.cpp:426`): the fallback for a UnitType's key.
    pub parasite_allow_water_exit: Option<bool>,
    /// `[CombatDamage] Parasite.DisableParticleSystem=`, default `no`
    /// (`Ext/Rules/Body.cpp:495`, constructor `Ext/Rules/Body.h:405`).
    pub parasite_disable_particle_system: bool,
    /// `[General] Cloak.KickOutParasite=`, default `no`
    /// (`Ext/Rules/Body.cpp:289`, constructor `Ext/Rules/Body.h:1056`): the
    /// fallback for a TechnoType's key.
    pub cloak_kick_out_parasite: bool,
    /// `[AudioVisual] CloakAnims=`, `ValueableVector<AnimTypeClass*>`
    /// (`Ext/Rules/Body.cpp:287`): the cloaking anim list fallback.
    #[serde(default)]
    pub cloak_anims: Vec<String>,
    /// `[AudioVisual] DecloakAnims=` (`Ext/Rules/Body.cpp:288`).
    #[serde(default)]
    pub decloak_anims: Vec<String>,
    /// `[General] Sinkable.SquidGrab=`, default `yes`
    /// (`Ext/Rules/Body.cpp:485`, constructor `Ext/Rules/Body.h:900`): the
    /// fallback for a UnitType's key.
    pub sinkable_squid_grab: bool,
    /// `[AudioVisual] DrainMoneyDisplay=`, default `no`: the drainer-side
    /// flying credit label (`Ext/Rules/Body.cpp:301`, consumed at
    /// `Ext/Techno/Hooks.cpp:199`).
    pub drain_money_display: bool,
    /// Phobos `[AudioVisual] WalkLocomotorMakesWake=` /
    /// `DriveLocomotorMakesWake=` / `HoverLocomotionClassMakesWake=` /
    /// `ShipLocomotionClassMakesWake=` (`Ext/Rules/Body.cpp:543-546`,
    /// constructors `Ext/Rules/Body.h:960-963` = no/yes/yes/yes): the
    /// per-locomotor fallback for a TechnoType's `MakesWake=`. The Walk arm's
    /// consumer site is not ported (VERA spawns no infantry movement wakes).
    #[serde(default)]
    pub walk_locomotor_makes_wake: bool,
    #[serde(default = "true_bool")]
    pub drive_locomotor_makes_wake: bool,
    #[serde(default = "true_bool")]
    pub hover_locomotor_makes_wake: bool,
    #[serde(default = "true_bool")]
    pub ship_locomotor_makes_wake: bool,
    /// `[AudioVisual] DrainMoneyDisplay.Houses=`, default `All`
    /// (`Ext/Rules/Body.h:730`): the label's House filter.
    pub drain_money_display_houses: AffectedHouse,
    /// `[AudioVisual] DrainMoneyDisplay.OnTarget=`, default `no`
    /// (`Ext/Rules/Body.h:731`): the drained object's own label.
    pub drain_money_display_on_target: bool,
    /// `[AudioVisual] DrainMoneyDisplay.OnTarget.UseDisplayIncome=`, default
    /// `yes` (`Ext/Rules/Body.h:732`): the target label takes the building's
    /// `DisplayIncome.Houses`/`.Offset` instead of the drain keys.
    pub drain_money_display_on_target_use_display_income: bool,
    /// `[AudioVisual] DisplayIncome.Houses=`, default `All`
    /// (`Ext/Rules/Body.h:728`): fallback for a building's own
    /// `DisplayIncome.Houses` in the drain target label.
    pub display_income_houses: AffectedHouse,
    /// `[General] ExtendedAircraftMissions=`, default `no`
    /// (`Ext/Rules/Body.cpp:191`, constructor `Ext/Rules/Body.h:97`): the
    /// master switch an AircraftType's own `ExtendedAircraftMissions=`
    /// overrides, and the fallback for its `SmoothMoving`, `EarlyDescend`,
    /// `RearApproach` and `FastScramble` keys
    /// (`rules::phobos_extended_aircraft_missions`).
    pub extended_aircraft_missions: bool,
    /// `[General] AmphibiousEnter=`, default `no`
    /// (`Ext/Rules/Body.cpp:197`, constructor `Ext/Rules/Body.h:654`): the
    /// fallback for a transport type's key; `yes` admits boarding while the
    /// transport sits on water (`Ext/Techno/Hooks.Transport.cpp:252`,
    /// `0x73796B`).
    pub amphibious_enter: bool,
    /// `[General] AmphibiousUnload=`, default `no`
    /// (`Ext/Rules/Body.cpp:198`, constructor `Ext/Rules/Body.h:655`): the
    /// fallback for a transport type's key; water unload then asks the
    /// passenger's own land row instead of refusing (`CanUnloadNow`,
    /// `Ext/Techno/Hooks.Transport.cpp:384`, hooks `0x7400B5`, `0x70107A`,
    /// `0x73D769`, `0x73D7AB`, `0x740C9C`, `0x73DAD8`).
    pub amphibious_unload: bool,
    /// `[General] ExtendedAircraftMissions.UnlandDamage=`, default `0`
    /// (`Ext/Rules/Body.cpp:192`, `Ext/Rules/Body.h:98`): the damage an
    /// aircraft that cannot land takes, or the kill threshold when negative.
    pub extended_aircraft_missions_unland_damage: i32,
    /// `[General] AmbientDamage.IgnoreTarget=`, default `no`
    /// (`Ext/Rules/Body.cpp:268`, constructor `Ext/Rules/Body.h:710`): the
    /// fallback for an unset weapon key; `yes` leaves the primary target out
    /// of the railgun/blast-wave ambient damage.
    #[serde(default)]
    pub ambient_damage_ignore_target: bool,
    /// `[General] Terrain.IsPassable=`, default `no`
    /// (`Ext/Rules/Body.cpp:479`, constructor `Ext/Rules/Body.h:894`): an
    /// ordinary terrain type with no `IsPassable=` key contributes no
    /// occupation bit.
    #[serde(default)]
    pub terrain_is_passable: bool,
    /// `[General] Tibtree.IsPassable=`, default `no` (`:480`, `:895`): the
    /// same for a `SpawnsTiberium=` type.
    #[serde(default)]
    pub tibtree_is_passable: bool,
    /// `[General] Terrain.CanBeBuiltOn=`, default `no`
    /// (`Ext/Rules/Body.cpp:481`, constructor `Ext/Rules/Body.h:896`): a
    /// building may replace an ordinary terrain type with the resolved key.
    #[serde(default)]
    pub terrain_can_be_built_on: bool,
    /// `[General] Tibtree.CanBeBuiltOn=`, default `no` (`:482`, `:897`): the
    /// same for a `SpawnsTiberium=` type.
    #[serde(default)]
    pub tibtree_can_be_built_on: bool,
    /// `[General] ApplyModifiersOnNegativeDamage=`, default `no`
    /// (`Ext/Rules/Body.cpp:492`, constructor `Ext/Rules/Body.h:907`): the
    /// fallback for an unset warhead key; a negative packet keeps the stock
    /// early-out at `0x4891AF` unless this or the type key is set.
    #[serde(default)]
    pub apply_modifiers_on_negative_damage: bool,
    /// `[General] ForceWeapon.InRange.TechnoOnly=`, default **`yes`**
    /// (`Ext/Rules/Body.h:674`): the per-type fallback for the in-range
    /// force-weapon gate.
    pub force_weapon_in_range_techno_only: bool,
    /// `[General] ForceWeapon.InRange.ApplyRangeModifiers=`, default `no`
    /// (`Ext/Rules/Body.h:675`).
    pub force_weapon_in_range_apply_range_modifiers: bool,
    /// `[General] ForceAAWeapon.InRange.ApplyRangeModifiers=`, default `no`
    /// (`Ext/Rules/Body.cpp:232`, constructor `Body.h:676`).
    pub force_aa_weapon_in_range_apply_range_modifiers: bool,
    /// `[CombatDamage] PenetratesTransport.Level=`, default 10
    /// (`Ext/Rules/Body.h:945`): the fallback for a transport type whose own
    /// level is unset.
    pub penetrates_transport_level: i32,
    /// `[CombatDamage] DriverKilled.KeptPassengers=`, default `no`
    /// (`Ext/Rules/Body.cpp:533`, `Body.h:949`): the fallback for a vehicle
    /// type whose own key is unset; `yes` keeps the passengers of a
    /// driver-killed vehicle owned by a neutral house aboard
    /// (`Ext/Techno/Body.cpp:930-1017`).
    #[serde(default)]
    pub driver_killed_kept_passengers: bool,
    /// `[CombatDamage] DriverKilled.KillPassengers=`, default `no`
    /// (`Ext/Rules/Body.cpp:534`, `Body.h:950`): the fallback that destroys
    /// each departing passenger instead of ejecting it.
    #[serde(default)]
    pub driver_killed_kill_passengers: bool,
    /// `[General] BuildLimitGroup.ContentIfAnyMatch=`, default `no`
    /// (`Ext/Rules/Body.cpp:223`, `Body.h:130,667`): fallback for an unset
    /// type-level `BuildLimitGroup.ContentIfAnyMatch`; `yes` makes any one
    /// group member reaching its limit block the whole group.
    #[serde(default)]
    pub build_limit_group_content_if_any_match: bool,
    /// `[General] BuildLimitGroup.NotBuildableIfQueueMatch=`, default `no`
    /// (`Ext/Rules/Body.cpp:224`, `Body.h:131,668`): fallback for an unset
    /// type-level `BuildLimitGroup.NotBuildableIfQueueMatch`; `yes` blocks
    /// even when only queued (not owned) copies reach the limit.
    #[serde(default)]
    pub build_limit_group_not_buildable_if_queue_match: bool,
    /// `[General] AttackMove.Aggressive=`, default `no`
    /// (`Ext/Rules/Body.cpp:377`, `Body.h:282,800`): fallback for an unset
    /// `TechnoType` `AttackMove.Aggressive`; `yes` lets an attack-mover treat
    /// an unarmed enemy building as a legal auto-target.
    #[serde(default)]
    pub attack_move_aggressive: bool,
    /// `[General] AttackMove.UpdateTarget=`, default `no`
    /// (`Ext/Rules/Body.cpp:378`, `Body.h:283,801`): fallback for an unset
    /// `TechnoType` `AttackMove.UpdateTarget`; `yes` lets the arrival handler
    /// drop an unusable held target and rescan.
    #[serde(default)]
    pub attack_move_update_target: bool,
    /// `[General] ApproachTarget.StopWhenInRange=`, default `no`
    /// (`Ext/Rules/Body.cpp:642-643`, `Body.h:537,1036`): fallback for an
    /// unset `TechnoType` `ApproachTarget.StopWhenInRange`; `yes` stops a
    /// close-enough, really moving foot on its approach.
    #[serde(default)]
    pub approach_target_stop_when_in_range: bool,
    /// `[General] AttackMove.IgnoreWeaponCheck=`, default `no`
    /// (`Ext/Rules/Body.cpp:423`, `Body.h:341,853`): global only — drops the
    /// native "`Primary=` must name a weapon" test from
    /// `TechnoTypeClass::Can_Attack_Move @ 0x00711E90`.
    #[serde(default)]
    pub attack_move_ignore_weapon_check: bool,
    /// `[General] Spawner.AttackImmediately=`, default `no`
    /// (`Ext/Rules/Body.cpp:208`, `Body.h:115,652`): fallback for an unset
    /// type-level key; `yes` sends a fresh child straight at the target.
    #[serde(default)]
    pub spawner_attack_immediately: bool,
    /// `[General] Spawner.UseTurretFacing=`, default `no`
    /// (`Ext/Rules/Body.cpp:209`, `Body.h:116,653`): launch facing uses the
    /// owner's turret instead of its body.
    #[serde(default)]
    pub spawner_use_turret_facing: bool,
    /// `[General] Spawner.RecycleRange=`, `Valueable<Leptons>`, default
    /// `Leptons(-1)` (`Ext/Rules/Body.cpp:210`, `Body.h:117,654`): cells ×
    /// 256; a negative value selects the legacy dock test.
    #[serde(default = "minus_one_i32")]
    pub spawner_recycle_range: i32,
    /// `[General] Spawner.RecycleOnTurret=`, default `no`
    /// (`Ext/Rules/Body.cpp:211`, `Body.h:118,655`): `RecycleCoord` rotates
    /// with the turret.
    #[serde(default)]
    pub spawner_recycle_on_turret: bool,
    /// `[General] Promote.IncludeSpawns=`, default `no`
    /// (`Ext/Rules/Body.cpp:212`, `Body.h:119,656`): fallback for an unset
    /// type-level `Promote.IncludeSpawns`.
    #[serde(default)]
    pub promote_include_spawns: bool,
    /// `[AudioVisual] Promote.VeteranAnimation=`,
    /// `ValueableVector<AnimTypeClass*>`, constructed empty
    /// (`Ext/Rules/Body.cpp:316`, `Body.h:239,748`): fallback for a type with
    /// no veteran list.
    #[serde(default)]
    pub promote_veteran_animation: Vec<String>,
    /// `[AudioVisual] Promote.EliteAnimation=`, constructed empty
    /// (`Ext/Rules/Body.cpp:317`, `Body.h:240,749`).
    #[serde(default)]
    pub promote_elite_animation: Vec<String>,
    /// `[General] OpenTopped.IgnoreRangefinding=`, default `no`
    /// (`Ext/Rules/Body.cpp:455`, `Body.h:870`): the per-type fallback for
    /// skipping the passenger-min weapon-range clamp in `WeaponRange`.
    #[serde(default)]
    pub open_topped_ignore_rangefinding: bool,
    /// `[General] OpenTopped.AllowFiringIfDeactivated=`, default **`yes`**
    /// (`Body.cpp:456`, `Body.h:871`).
    #[serde(default = "true_bool")]
    pub open_topped_allow_firing_if_deactivated: bool,
    /// `[General] OpenTopped.AllowFiringIfAttackedByLocomotor=`, default
    /// **`yes`** (`Body.cpp:457`, `Body.h:872`).
    #[serde(default = "true_bool")]
    pub open_topped_allow_firing_if_attacked_by_locomotor: bool,
    /// `[General] OpenTopped.ShareTransportTarget=`, default **`yes`**
    /// (`Body.cpp:458`, `Body.h:873`).
    #[serde(default = "true_bool")]
    pub open_topped_share_transport_target: bool,
    /// `[CombatDamage] ProjectileInterceptable=`, default `no`
    /// (`Ext/Rules/Body.cpp:497`, `Body.h:912`): the fallback for a bullet
    /// type's `Interceptable`.
    #[serde(default)]
    pub projectile_interceptable: bool,
    /// `[CombatDamage] Interceptor.GuardRange.IsCylindrical=`, default `no`
    /// (`Ext/Rules/Body.cpp:498`, `Body.h:913`).
    #[serde(default)]
    pub interceptor_guard_range_is_cylindrical: bool,
    /// `[CombatDamage] AllowWeaponSelectAgainstWalls=`, default `no`
    /// (`Ext/Rules/Body.cpp:256`, `Body.h:174,702`): the per-type fallback
    /// for picking a secondary wall-breaking weapon against a wall overlay
    /// (`Ext/Techno/WeaponHelpers.cpp:196-236`).
    #[serde(default)]
    pub allow_weapon_select_against_walls: bool,
    /// `[CombatDamage] Interceptor.ApplyFirepowerMult=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:499`, `Body.h:914`).
    #[serde(default = "true_bool")]
    pub interceptor_apply_firepower_mult: bool,
    /// `[CombatDamage] ReturnWeapon.ApplyFirepowerMult=`, default `no`
    /// (`Ext/Rules/Body.cpp:559`, constructor `Ext/Rules/Body.h:973`): the
    /// fallback a `BulletType`'s unset `ReturnWeapon.ApplyFirepowerMult=`
    /// keeps, scaling the returned shot's `Damage=` by the dying bullet's
    /// `FirepowerMult` (`Ext/Bullet/Hooks.DetonateLogics.cpp:490`).
    #[serde(default)]
    pub return_weapon_apply_firepower_mult: bool,
    /// `[General] OpenTopped.UseTransportRangeModifiers=`, default `no`
    /// (`Body.cpp:459`, `Body.h:874`). Also read into `AttachEffectGlobals`.
    #[serde(default)]
    pub open_topped_use_transport_range_modifiers: bool,
    /// `[General] OpenTopped.CheckTransportDisableWeapons=`, default `no`
    /// (`Body.cpp:460`, `Body.h:875`). Also read into `AttachEffectGlobals`.
    #[serde(default)]
    pub open_topped_check_transport_disable_weapons: bool,
    /// `[General] OpenTopped.DecloakToFire=`, default `no`
    /// (`Body.cpp:461`, `Body.h:876`). The docs at
    /// `docs/New-or-Enhanced-Logics.md:1912` claim `yes`; the constructor is
    /// `{ false }`, so the source wins.
    #[serde(default)]
    pub open_topped_decloak_to_fire: bool,
    /// `[General] OpenTopped.FireWhileMoving=`, default **`yes`**
    /// (`Body.cpp:462`, `Body.h:877`).
    #[serde(default = "true_bool")]
    pub open_topped_fire_while_moving: bool,
    /// `[CombatDamage] OpenTransport.RangeBonus=`, default 0
    /// (`Ext/Rules/Body.cpp:463`, constructor `Ext/Rules/Body.h:878`): the
    /// rider-side fallback, added to the carrier's bonus.
    #[serde(default)]
    pub open_transport_range_bonus: i32,
    /// `[CombatDamage] OpenTransport.DamageMultiplier=`, default 1.0
    /// (`:464`, `Body.h:879`), a `Valueable<float>`.
    #[serde(default = "one_f32")]
    pub open_transport_damage_multiplier: crate::util::native_x87::NativeF32Bits,
    /// `[General] OpenTransport.FireWhileMoving=`, default `yes`
    /// (`:465`, `Body.h:880`).
    #[serde(default = "true_bool")]
    pub open_transport_fire_while_moving: bool,
    /// `[General] NoRearm.UnderEMP=`, default `no` (`Ext/Rules/Body.cpp:383`,
    /// constructor `Ext/Rules/Body.h:804`): the fallback for an unset type key;
    /// `yes` holds a running rearm timer while the techno is EMP'd or
    /// deactivated.
    #[serde(default)]
    pub no_rearm_under_emp: bool,
    /// `[General] NoRearm.Temporal=`, default `no` (`Ext/Rules/Body.cpp:384`,
    /// constructor `Body.h:805`): the same hold while a Temporal weapon warps
    /// the techno.
    #[serde(default)]
    pub no_rearm_temporal: bool,
    /// `[General] NoReload.UnderEMP=`, default `no` (`Ext/Rules/Body.cpp:385`,
    /// constructor `Body.h:806`): the ammo-reload counterpart of
    /// `no_rearm_under_emp`.
    #[serde(default)]
    pub no_reload_under_emp: bool,
    /// `[General] NoReload.Temporal=`, default `no` (`Ext/Rules/Body.cpp:386`,
    /// constructor `Body.h:807`): the ammo-reload counterpart of
    /// `no_rearm_temporal`.
    #[serde(default)]
    pub no_reload_temporal: bool,
    /// `[General] NoTurret.TrackTarget=`, default `no`
    /// (`Ext/Rules/Body.cpp:393`, constructor `Body.h:812`): the fallback for
    /// an unset UnitType key; `yes` lets a turretless Unit in REARM keep
    /// checking its facing and turn toward the target while cooling down.
    #[serde(default)]
    pub no_turret_track_target: bool,
    /// `[General] OmniFire.TurnToTarget=`, default `no`
    /// (`Ext/Rules/Body.cpp:267`, constructor `Body.h:709`): the fallback for
    /// an unset WeaponType key; `yes` turns an omni-fire unit's hull toward
    /// its target while the fire error is OK or REARM.
    #[serde(default)]
    pub omni_fire_turn_to_target: bool,
    /// `[General] BuildingStartFacing=`, default 0
    /// (`Ext/Rules/Body.cpp:626`, constructor `Body.h:1024`): the fallback for
    /// an unset BuildingType `StartFacing`, on a 256-point circle.
    #[serde(default)]
    pub building_start_facing: i32,
    /// `[General] BuildingStartFacing.Random=`, default `no`
    /// (`Ext/Rules/Body.cpp:627`, constructor `Body.h:1025`): the fallback for
    /// an unset BuildingType `StartFacing.Random`.
    #[serde(default)]
    pub building_start_facing_random: bool,
    /// `[General] Passengers.SyncOwner=`, default `no`
    /// (`Ext/Rules/Body.cpp:467`, constructor `Ext/Rules/Body.h:882`): the
    /// fallback for an unset transport-type key; `yes` re-houses the cargo
    /// when the transport's house changes.
    #[serde(default)]
    pub passengers_sync_owner: bool,
    /// `[General] Passengers.SyncOwner.RevertOnExit=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:468`, constructor `Body.h:883`): the fallback for
    /// an unset transport-type key; `yes` puts an exiting cargo member back
    /// on the house it held when it boarded.
    #[serde(default = "true_bool")]
    pub passengers_sync_owner_revert_on_exit: bool,
    /// `[General] AlternateFLH.OnTurret=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:218`, constructor `Ext/Rules/Body.h:662`): the
    /// fallback for an unset TechnoType ART `AlternateFLH.OnTurret`; `no`
    /// keeps an alternate FLH hull-relative while a turret fires.
    #[serde(default = "true_bool")]
    pub alternate_flh_on_turret: bool,
    /// `[General] AlternateFLH.ApplyVehicle=`, default `no`
    /// (`Ext/Rules/Body.cpp:219`, constructor `Ext/Rules/Body.h:663`): the
    /// fallback for an unset carrier ART `AlternateFLH.ApplyVehicle`; `yes`
    /// lets a Unit passenger fire from the open-topped carrier's AlternateFLH.
    #[serde(default)]
    pub alternate_flh_apply_vehicle: bool,
    /// `[General] Vertical.AircraftFix=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:581`, constructor `Ext/Rules/Body.h:990`): the
    /// fallback for an unset BulletType `Vertical.AircraftFix`; `yes` keeps a
    /// `Vertical=` bullet's FireAt velocity instead of the aircraft's ROT
    /// course (`0x415F25`, `Misc/Hooks.BugFixes.cpp:2423`).
    #[serde(default = "true_bool")]
    pub vertical_aircraft_fix: bool,
    /// `[General] BuildingRadioLink.SyncOwner=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:509`, constructor `Ext/Rules/Body.h:413,920`):
    /// whether `BuildingClass::ChangeOwner`'s docked take-along segment
    /// (`0x00448559..0x004486C8`) swaps a docked Foot to the building's new
    /// owner; the type key overrides it per building
    /// (`Ext/Building/Hooks.cpp:1013-1018`, hook `0x4485DB`).
    #[serde(default = "true_bool")]
    pub building_radio_link_sync_owner: bool,
    /// `[AudioVisual] IvanIconVisibility=` (`AffectedHouse`, default
    /// `Owner`, `Ext/Rules/Body.cpp:448`, `Ext/Rules/Body.h:358,863`): the
    /// bomb-icon visibility gate (`Ext/WeaponType/Hooks.Bomb.cpp:38-77`,
    /// hook `0x438D44`); a weapon's `IvanBomb.Visibility=` overrides it.
    #[serde(default)]
    pub ivan_icon_visibility: crate::rules::phobos_shields::AffectedHouse,
    /// `[CombatDamage] Arcing.AllowElevationInaccuracy=` (`Valueable<bool>`,
    /// default **`true`**, `Ext/Rules/Body.cpp:477`, `Ext/Rules/Body.h:387`,
    /// `:892`): the global fallback of the BulletType key; `no` makes every
    /// arcing launch run `BulletExt::ApplyArcingFix`.
    #[serde(default = "true_bool")]
    pub arcing_allow_elevation_inaccuracy: bool,
    /// `[General] Debris.Conventional=`, default `no`
    /// (`Ext/Rules/Body.cpp:494`, constructor `Ext/Rules/Body.h:909`): the
    /// WarheadType `Debris.Conventional=` fallback; true skips a water
    /// cell's bullet debris block (`0x469D1A`).
    #[serde(default)]
    pub debris_conventional: bool,
    /// `[CombatDamage] Temporal.ApplyMultiplier=` / `.ApplyVersus=`
    /// (`Valueable<bool>`, constructors `false`, `Ext/Rules/Body.h:494-495`,
    /// `:991-992`; reads `Ext/Rules/Body.cpp:583-584`): the global fallbacks
    /// of the WarheadType keys that extend the temporal warp-per-step
    /// (`Ext/Techno/Hooks.cpp:2359-2379`).
    #[serde(default)]
    pub temporal_apply_multiplier: bool,
    #[serde(default)]
    pub temporal_apply_versus: bool,
    /// `[CombatDamage] CanTarget.IronCurtained=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:438`, constructor `Ext/Rules/Body.h:833`): the
    /// human fallback for an unset WeaponType `CanTarget.IronCurtained`; `no`
    /// refuses a shot at an iron-curtained target
    /// (`Ext/Techno/Hooks.Firing.cpp:367`).
    #[serde(default = "true_bool")]
    pub can_target_iron_curtained: bool,
    /// `[CombatDamage] CanTargetAI.IronCurtained=`, default `no` (`:437`,
    /// constructor `Body.h:832`): the computer-house fallback, preserving the
    /// stock AI refusal.
    #[serde(default)]
    pub can_target_ai_iron_curtained: bool,
    /// `[CombatDamage] AutoTarget.IronCurtained=`, default **`yes`** (`:439`,
    /// constructor `Body.h:834`): the human fallback for an unset WeaponType
    /// `AutoTarget.IronCurtained` in retaliation and auto-target
    /// (`Hooks.Firing.cpp:1246`, `Hooks.Targeting.cpp:63`).
    #[serde(default = "true_bool")]
    pub auto_target_iron_curtained: bool,
    /// `[General] CylinderRangefinding=`, default `no`
    /// (`Ext/Rules/Body.cpp:525`, constructor `Body.h:442`): the fallback for
    /// an unset weapon `CylinderRangefinding`; `yes` measures direct range in
    /// the plan (dx²+dy²) for every techno, not only aircraft
    /// (`Ext/Techno/Hooks.Firing.cpp:1265-1274`).
    #[serde(default)]
    pub cylinder_rangefinding: bool,
    /// `[General] RecountBurst=`, default `no` (`Ext/Rules/Body.cpp:382`,
    /// constructor `Ext/Rules/Body.h:803`): a weapon change or an idle timeout
    /// recounts the retained burst index and re-arms the shot
    /// (`Ext/Techno/Body.Update.cpp:544-566`,
    /// `Ext/Techno/Hooks.Firing.cpp:609-633`).
    #[serde(default)]
    pub recount_burst: bool,
    /// `[General] AISuperWeaponDelay=` (`Nullable<int>`, game frames,
    /// `Ext/Rules/Body.cpp:127`, decl `Ext/Rules/Body.h:45`): when set, the
    /// strategy-side `AI_TryFireSW` call is skipped and a per-house timer
    /// gates the per-frame call; `<= 0` tries every frame
    /// (`Ext/House/Hooks.cpp:508-541`). Unset keeps the stock cadence.
    #[serde(default)]
    pub ai_superweapon_delay: Option<i32>,
    /// `[AudioVisual] RevealHouses=` (`Valueable<AffectedHouse>`, default
    /// `Team`, `Ext/Rules/Body.cpp:711`, constructor `Body.h:1063`): which
    /// players a techno's sight reveals to when neither its type nor its
    /// country overrides (`Ext/Techno/Hooks.cpp:2562-2596`).
    #[serde(default = "reveal_houses_default")]
    pub reveal_houses: AffectedHouse,
    /// `[CombatDamage] BerzerkTargeting=`, default `all` (`:420`, constructor
    /// `Body.h:846`, `Valueable<AffectedHouse>`): the houses a berzerk firer
    /// may target (`Ext/Techno/Hooks.Firing.cpp:370`).
    #[serde(default)]
    pub berzerk_targeting: AffectedHouse,
    /// `[General] Strafing.SimulateBurst=`, default `no`
    /// (`Ext/Rules/Body.cpp:264`, constructor `Ext/Rules/Body.h:706`): the
    /// fallback for an unset weapon `Strafing.SimulateBurst`; `yes` alternates
    /// the firing offset across a strafing round's single shots
    /// (`Ext/Aircraft/Body.cpp:22,26-27`).
    #[serde(default)]
    pub strafing_simulate_burst: bool,
    /// `[General] Strafing.UseAmmoPerShot=`, default `no`
    /// (`Ext/Rules/Body.cpp:265`, constructor `Body.h:707`): the fallback for
    /// an unset weapon key; `yes` debits one Ammo after every strafing shot
    /// instead of once per round (`Ext/Aircraft/Body.cpp:36-45`).
    #[serde(default)]
    pub strafing_use_ammo_per_shot: bool,
    /// `[General] Strafing.TargetCell=`, default `no`
    /// (`Ext/Rules/Body.cpp:266`, constructor `Body.h:708`): the fallback for
    /// an unset weapon key; `yes` turns the round's Target into its cell after
    /// the release (`Ext/Aircraft/Hooks.cpp:234-235`).
    #[serde(default)]
    pub strafing_target_cell: bool,
    /// `[General] ExtraThreat.IsThreat=`, default 0.0
    /// (`Ext/Rules/Body.cpp:535`, constructor `Ext/Rules/Body.h:952`): the
    /// fallback for an unset TechnoType key.
    #[serde(default = "zero_f64")]
    pub extra_threat_is_threat: NativeF64Bits,
    /// `[General] ExtraThreat.InRange=`, default 0.0 (`:536`, `:953`).
    #[serde(default = "zero_f64")]
    pub extra_threat_in_range: NativeF64Bits,
    /// `[General] ExtraThreatCoefficient.InRangeDistance=`, default 0.0
    /// (`:537`, `:954`).
    #[serde(default = "zero_f64")]
    pub extra_threat_coefficient_in_range_distance: NativeF64Bits,
    /// `[General] ExtraThreatCoefficient.Facing=`, default 0.0 (`:538`,
    /// `:955`).
    #[serde(default = "zero_f64")]
    pub extra_threat_coefficient_facing: NativeF64Bits,
    /// `[General] ExtraThreatCoefficient.DistanceToLastTarget=`, default 0.0
    /// (`:539`, `:956`).
    #[serde(default = "zero_f64")]
    pub extra_threat_coefficient_distance_to_last_target: NativeF64Bits,
    /// `[AudioVisual] CombatAlert=`, the master switch, default **`no`**
    /// (`Ext/Rules/Body.cpp:355`, constructor `Ext/Rules/Body.h:783`).
    #[serde(default)]
    pub combat_alert: bool,
    /// `[AudioVisual] CombatAlert.Default=`, `Nullable<bool>`
    /// (`Ext/Rules/Body.cpp:356`, constructor `Body.h:784` unset): the
    /// fallback for an unset TechnoType `CombatAlert`; unset in turn admits
    /// `!Insignificant && !Spawned` (`Ext/Techno/Hooks.ReceiveDamage.cpp:101`).
    #[serde(default)]
    pub combat_alert_default: Option<bool>,
    /// `[AudioVisual] CombatAlert.IgnoreBuilding=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:357`, constructor `Body.h:785`): an ordinary
    /// building is skipped unless its type is vehicle-like.
    #[serde(default = "true_bool")]
    pub combat_alert_ignore_building: bool,
    /// `[AudioVisual] CombatAlert.SuppressIfInScreen=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:358`, constructor `Body.h:786`). Presentation-only:
    /// the simulation has no tactical view, so VERA cannot apply it
    /// (`docs/phobos-combat-alert.md` records the residual).
    #[serde(default = "true_bool")]
    pub combat_alert_suppress_if_in_screen: bool,
    /// `[AudioVisual] CombatAlert.Interval=`, `Valueable<int>` frames,
    /// default 150 (`Ext/Rules/Body.cpp:359`, constructor `Body.h:787`).
    #[serde(default = "combat_alert_interval_default")]
    pub combat_alert_interval: i32,
    /// `[AudioVisual] CombatAlert.SuppressIfAllyDamage=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:360`, constructor `Body.h:788`).
    #[serde(default = "true_bool")]
    pub combat_alert_suppress_if_ally_damage: bool,
    /// `[AudioVisual] CombatAlert.MakeAVoice=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:361`, constructor `Body.h:789`): `no` keeps the
    /// radar ping but plays no voice or EVA line
    /// (`Ext/Techno/Hooks.ReceiveDamage.cpp:125`).
    #[serde(default = "true_bool")]
    pub combat_alert_make_a_voice: bool,
    /// `[AudioVisual] CombatAlert.UseFeedbackVoice=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:362`, constructor `Body.h:790`): the fallback for
    /// the type's key.
    #[serde(default = "true_bool")]
    pub combat_alert_use_feedback_voice: bool,
    /// `[AudioVisual] CombatAlert.UseAttackVoice=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:363`, constructor `Body.h:791`).
    #[serde(default = "true_bool")]
    pub combat_alert_use_attack_voice: bool,
    /// `[AudioVisual] CombatAlert.UseEVA=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:364`, constructor `Body.h:792`).
    #[serde(default = "true_bool")]
    pub combat_alert_use_eva: bool,
    /// `[General] DestroyAnim.Random=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:220`, constructor `Body.h:664`): a destroyed
    /// unit with a `DestroyAnim=` list picks an entry at random, otherwise
    /// the facing-based entry (`Ext/AnimType/Body.cpp:21-43`).
    #[serde(default = "true_bool")]
    pub destroy_anim_random: bool,
    /// `[AudioVisual] VisualScatter.Min=`, `Valueable<Leptons>` default
    /// **`8`** (`Ext/Rules/Body.cpp:325`, constructor `Ext/Rules/Body.h:758`):
    /// the low end of the radius a `VisualScatter=` weapon's drawn laser, bolt
    /// or beam may end away from its real target
    /// (`Ext/Techno/Hooks.WeaponEffects.cpp:234-235`).
    #[serde(default = "visual_scatter_min_default")]
    pub visual_scatter_min: i32,
    /// `[AudioVisual] VisualScatter.Max=`, `Valueable<Leptons>` default
    /// **`32`** (`Ext/Rules/Body.cpp:326`, constructor `Ext/Rules/Body.h:759`).
    #[serde(default = "visual_scatter_max_default")]
    pub visual_scatter_max: i32,
    /// `[General] AutoTarget.NoThreatBuildings=`, default `no`
    /// (`Ext/Rules/Body.cpp:517`, constructor `Ext/Rules/Body.h:931`): whether
    /// a **human** scanner may auto-target an enemy building that is unarmed
    /// or poses no threat (`Ext/Techno/Hooks.cpp:1850-1852`). A weapon's own
    /// `AttackNoThreatBuildings=` overrides it.
    #[serde(default)]
    pub auto_target_no_threat_buildings: bool,
    /// `[General] AutoTargetAI.NoThreatBuildings=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:518`, constructor `Ext/Rules/Body.h:932`): the
    /// computer's half of the same gate.
    #[serde(default = "true_bool")]
    pub auto_target_ai_no_threat_buildings: bool,
    /// `[CombatDamage] DamageOwnerMultiplier=`, `Valueable<double>` default
    /// **`1.0`** (`Ext/Rules/Body.cpp:340`, constructor `Ext/Rules/Body.h:770`):
    /// the fallback for a warhead's own `DamageOwnerMultiplier=`
    /// (`Ext/Techno/Hooks.ReceiveDamage.cpp:73-74`).
    #[serde(default = "one_f64")]
    pub damage_owner_multiplier: NativeF64Bits,
    /// `[CombatDamage] DamageAlliesMultiplier=`, default **`1.0`**
    /// (`Ext/Rules/Body.cpp:341`, constructor `Ext/Rules/Body.h:771`).
    #[serde(default = "one_f64")]
    pub damage_allies_multiplier: NativeF64Bits,
    /// `[CombatDamage] DamageEnemiesMultiplier=`, default **`1.0`**
    /// (`Ext/Rules/Body.cpp:342`, constructor `Ext/Rules/Body.h:772`); also
    /// the last fallback of the two berserk chains.
    #[serde(default = "one_f64")]
    pub damage_enemies_multiplier: NativeF64Bits,
    /// `[CombatDamage] DamageOwnerMultiplier.NotAffectsEnemies=`,
    /// `Nullable<double>` (`Ext/Rules/Body.cpp:343`, `Body.h:773`): the owner
    /// column of a warhead whose `AffectsEnemies=no` when the warhead names
    /// no owner multiplier of its own.
    #[serde(default)]
    pub damage_owner_multiplier_not_affects_enemies: Option<NativeF64Bits>,
    /// `[CombatDamage] DamageAlliesMultiplier.NotAffectsEnemies=`
    /// (`Ext/Rules/Body.cpp:344`, `Body.h:774`).
    #[serde(default)]
    pub damage_allies_multiplier_not_affects_enemies: Option<NativeF64Bits>,
    /// `[CombatDamage] DamageOwnerMultiplier.Berzerk=`,
    /// `Nullable<double>` (`Ext/Rules/Body.cpp:345`, `Body.h:775`).
    #[serde(default)]
    pub damage_owner_multiplier_berzerk: Option<NativeF64Bits>,
    /// `[CombatDamage] DamageAlliesMultiplier.Berzerk=`
    /// (`Ext/Rules/Body.cpp:346`, `Body.h:776`).
    #[serde(default)]
    pub damage_allies_multiplier_berzerk: Option<NativeF64Bits>,
    /// `[CombatDamage] DamageEnemiesMultiplier.Berzerk=`
    /// (`Ext/Rules/Body.cpp:347`, `Body.h:777`).
    #[serde(default)]
    pub damage_enemies_multiplier_berzerk: Option<NativeF64Bits>,
    /// `[AudioVisual] RemoveMindControl.Silent=`, default `no`
    /// (`Ext/Rules/Body.cpp:597`, constructor `Ext/Rules/Body.h:998`): a
    /// warhead's `RemoveMindControl=` frees its victims without the
    /// `MindClearedSound` (`Ext/CaptureManager/Body.cpp:141`).
    #[serde(default)]
    pub remove_mind_control_silent: bool,
    /// `[CombatDamage] MergeBuildingDamage=`, default `no`
    /// (`Ext/Rules/Body.cpp:503`, constructor `Ext/Rules/Body.h:918`): one
    /// area detonation damages a building once, with its per-cell distance
    /// multipliers summed (`Ext/WarheadType/Hooks.cpp:412-487`).
    #[serde(default)]
    pub merge_building_damage: bool,
    /// `[General] ShakeIsLocal=`, `Valueable<bool>` default `no`
    /// (`Ext/Rules/Body.cpp:491`, constructor `Ext/Rules/Body.h:401,906`):
    /// the fallback for a warhead that leaves `ShakeIsLocal=` unset. When on,
    /// a detonation outside the local tactical view skips its screen-shake
    /// writes (`Ext/Bullet/Hooks.DetonateLogics.cpp:36-39`).
    #[serde(default)]
    pub shake_is_local: bool,
    /// `[General] RadarJamHouses=`, `Nullable<AffectedHouse>` default
    /// **`Enemies`** (`Ext/Rules/Body.cpp:213`, constructor
    /// `Ext/Rules/Body.h:657`): which Houses a `RadarJamRadius=` jammer may
    /// jam (`New/Entity/Ares/RadarJammerClass.cpp:17-18`).
    #[serde(default = "radar_jam_houses_default")]
    pub radar_jam_houses: crate::rules::phobos_shields::AffectedHouse,
    /// `[General] RadarJamDelay=`, `Valueable<int>` default **`30`**
    /// (`Ext/Rules/Body.cpp:214`, constructor `Ext/Rules/Body.h:658`): frames
    /// between two jammer scans (`RadarJammerClass.cpp:10`).
    #[serde(default = "radar_jam_delay_default")]
    pub radar_jam_delay: i32,
    /// `[General] NoQueueUpToEnter=`, default `no`
    /// (`Ext/Rules/Body.cpp:197`, constructor `Ext/Rules/Body.h:640`).
    #[serde(default)]
    pub no_queue_up_to_enter: bool,
    /// `[General] NoQueueUpToEnter.BoardDistance=`, `Valueable<int>` default
    /// **`384`** leptons (`Ext/Rules/Body.cpp:198`, constructor
    /// `Ext/Rules/Body.h:641`).
    #[serde(default = "no_queue_board_distance_default")]
    pub no_queue_up_to_enter_board_distance: i32,
    /// `[General] NoQueueUpToEnter.Buildings=`, `Nullable<bool>`
    /// (`Ext/Rules/Body.cpp:200`, constructor `Ext/Rules/Body.h:643`).
    #[serde(default)]
    pub no_queue_up_to_enter_buildings: Option<bool>,
    /// `[General] NoQueueUpToUnload=`, default `no`
    /// (`Ext/Rules/Body.cpp:199`, constructor `Ext/Rules/Body.h:642`): a
    /// transport whose type or this global enables it unloads the whole cargo
    /// within one frame (`Ext/Techno/Hooks.Transport.cpp:427-466`).
    #[serde(default)]
    pub no_queue_up_to_unload: bool,
    /// `[General] NoQueueUpToUnload.Buildings=`, `Nullable<bool>`
    /// (`Ext/Rules/Body.cpp:201`, constructor `Ext/Rules/Body.h:644`): the
    /// building-type fallback for the same key (`0x44DCB1`).
    #[serde(default)]
    pub no_queue_up_to_unload_buildings: Option<bool>,
    /// `[General] ReloadInTransport=`, default `no`
    /// (`Ext/Rules/Body.cpp:454`, constructor `Ext/Rules/Body.h:869`): a
    /// passenger whose type enables it reloads while inside a transport
    /// (`Ext/Techno/Hooks.Transport.cpp:70-165`).
    #[serde(default)]
    pub reload_in_transport: bool,
    /// `[General] InfantryGainSelfHealCap=`, `Nullable<int>`
    /// (`Ext/Rules/Body.cpp:111`, `Body.h:30,570`): the cap on the infantry
    /// self-heal count; unset means no cap.
    #[serde(default)]
    pub infantry_gain_self_heal_cap: Option<i32>,
    /// `[General] UnitsGainSelfHealCap=`, `Nullable<int>`
    /// (`Ext/Rules/Body.cpp:112`, `Body.h:31,571`).
    #[serde(default)]
    pub units_gain_self_heal_cap: Option<i32>,
    /// `[General] GainSelfHealAllowMultiplayPassive=`, default `yes`
    /// (`Ext/Rules/Body.cpp:113`, `Body.h:32,572`).
    #[serde(default = "true_bool")]
    pub gain_self_heal_allow_multiplay_passive: bool,
    /// `[General] GainSelfHealFromPlayerControl=`, default `no`
    /// (`Ext/Rules/Body.cpp:114`, `Body.h:33,573`).
    #[serde(default)]
    pub gain_self_heal_from_player_control: bool,
    /// `[General] GainSelfHealFromAllies=`, default `no`
    /// (`Ext/Rules/Body.cpp:115`, `Body.h:34,574`).
    #[serde(default)]
    pub gain_self_heal_from_allies: bool,
    /// `[AudioVisual] AircraftDockingDir.DefaultToPoseDir=`, default `yes`
    /// (`Ext/Rules/Body.cpp:634`, `Body.h:533,1032`): a docked aircraft's
    /// landing direction falls through to the type/pose dir instead of the
    /// building's facing (`AircraftExt::GetLandingDir`).
    #[serde(default = "true_bool")]
    pub aircraft_docking_dir_default_to_pose_dir: bool,
    /// `[AudioVisual] PoseDir.Production=`, `Nullable<int>`
    /// (`Ext/Rules/Body.cpp:635`, `Body.h:534,1033`): the production
    /// landing direction; unset keeps `[General] PoseDir=`.
    #[serde(default)]
    pub pose_dir_production: Option<i32>,
    /// `[AudioVisual] PoseDir.Field=`, `Nullable<int>`
    /// (`Ext/Rules/Body.cpp:636`, `Body.h:535,1034`): a raw field landing
    /// direction; unset keeps `PoseDir * 32`. Only its low byte is used.
    #[serde(default)]
    pub pose_dir_field: Option<i32>,
    /// `[General] VeteranReload=`, default **`1.0`**
    /// (`Ext/Rules/Body.cpp:388`, constructor `Ext/Rules/Body.h:808`).
    #[serde(default = "one_f64")]
    pub veteran_reload: NativeF64Bits,
    /// `[General] VeteranEmptyReload=`, `Nullable<double>`
    /// (`Ext/Rules/Body.cpp:389`, constructor `Ext/Rules/Body.h:809`).
    #[serde(default)]
    pub veteran_empty_reload: Option<NativeF64Bits>,
    /// `[AudioVisual] IsVoiceCreatedGlobal=`, default `no`
    /// (`Ext/Rules/Body.cpp:306`, constructor `Ext/Rules/Body.h:738`): every
    /// produced object plays its `VoiceCreated=`.
    #[serde(default)]
    pub is_voice_created_global: bool,
    /// `[General] UseDisguiseMovementSpeed=`, `Nullable<bool>`
    /// (`Ext/Rules/Body.cpp:221`): a disguised object that names no own
    /// answer moves at its disguise's speed.
    #[serde(default)]
    pub use_disguise_movement_speed: Option<bool>,
    /// `[General] HoverDrownable=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:475`, constructor `Ext/Rules/Body.h:890`): a hover
    /// vehicle over deep water drowns unless its type says otherwise.
    #[serde(default = "true_bool")]
    pub hover_drownable: bool,
    /// `[General] OnlyUseLandSequences=`, default `no`
    /// (`Ext/Rules/Body.cpp:622`, constructor `Ext/Rules/Body.h:1017`): the
    /// fallback an `InfantryType`'s unset art `OnlyUseLandSequences=` keeps,
    /// making infantry play their land sequences while in water
    /// (`Ext/Techno/Hooks.Misc.cpp:890`).
    #[serde(default)]
    pub only_use_land_sequences: bool,
    /// `[General] AnimCraterDestroyTiberium=`, default **`yes`**
    /// (`Ext/Rules/Body.cpp:418`, constructor `Ext/Rules/Body.h:844`): the
    /// fallback an AnimType's unset art `Crater.DestroyTiberium=` keeps for
    /// `0x4250E1`'s `CellClass::Reduce_Tiberium(6)` call
    /// (`Ext/Anim/Hooks.cpp:707`).
    #[serde(default = "true_bool")]
    pub anim_crater_destroy_tiberium: bool,
    /// `[General] HarvesterDumpAmount=`, default **0.0**
    /// (`Ext/Rules/Body.cpp:110`, constructor `Ext/Rules/Body.h:569`), a
    /// `Valueable<float>`: ore credited per unload tick instead of the full
    /// cargo (`Ext/Techno/Hooks.cpp:434-460`); 0 keeps the stock full-cargo
    /// dump.
    #[serde(default = "zero_f32")]
    pub harvester_dump_amount: NativeF32Bits,
    /// `[General] HarvesterScanAfterUnload=`, default `no`
    /// (`Ext/Rules/Body.cpp:416`, constructor `Ext/Rules/Body.h:842`): a
    /// harvester released from a refinery scans for ore in the same mission
    /// step (`Ext/TechnoType/Hooks.cpp:...`); off keeps the delayed scan.
    #[serde(default)]
    pub harvester_scan_after_unload: bool,
    /// `[AudioVisual] CombatLightDetailLevel=`, default **0**
    /// (`Ext/Rules/Body.cpp:404`, constructor `Ext/Rules/Body.h:820`):
    /// selects which combat-light descriptors a light flash draws
    /// (`Ext/Render/Hooks.cpp`).
    #[serde(default)]
    pub combat_light_detail_level: i32,
    /// `[AudioVisual] CombatLightDetailLevel.CheckColored=`, default **false**
    /// (`Ext/Rules/Body.cpp:405`, constructor `Ext/Rules/Body.h:821`): a light
    /// whose colour channels are partly disabled (`bitmask & 0xF != 0`) still
    /// flashes while this is off, and is refused while it is on
    /// (`DoFlash_CombatLightOptions @ 0x48A62E`,
    /// `Misc/Hooks.LightEffects.cpp:66-96`).
    #[serde(default)]
    pub combat_light_detail_level_check_colored: bool,
    /// `[General] SubterraneanSpeed=` (`Ext/Rules/Body.cpp:125`), read through
    /// `INI_EX::ReadSpeed` into internal 256ths, constructor **19**
    /// (`Ext/Rules/Body.h:583`): the fallback for a TechnoType's own key.
    #[serde(default = "subterranean_speed_default")]
    pub subterranean_speed: i32,
    /// `[General] SubterraneanHeight=`, default **-256**
    /// (`Ext/Rules/Body.cpp:126`, constructor `Ext/Rules/Body.h:584`): the
    /// fallback for a TechnoType's own key.
    #[serde(default = "subterranean_height_default")]
    pub subterranean_height: i32,
    /// `[General] VeteranRange=`, default **1.0**
    /// (`Ext/Rules/Body.cpp:390`, constructor `Ext/Rules/Body.h:810`): the
    /// fallback for a TechnoType's own key.
    #[serde(default = "one_f64")]
    pub veteran_range: NativeF64Bits,
    /// `[AudioVisual] DisplayIncome=`, default `no`
    /// (`Ext/Rules/Body.cpp:291`, constructor `Ext/Rules/Body.h:725`): a
    /// building that leaves the key unset still shows no income label.
    #[serde(default)]
    pub display_income: bool,
    /// `[AudioVisual] DisplayIncome.Delay=`, default **15**
    /// (`Ext/Rules/Body.cpp:292-297`, constructor `Ext/Rules/Body.h:726`):
    /// frames between the accumulated-income labels; a read of 0 is forced to
    /// 1 because 0 crashes the native label.
    #[serde(default = "display_income_delay_default")]
    pub display_income_delay: i32,
    /// `[General] SetRecruitableOnLiberate=`, default **-1**
    /// (`Ext/Rules/Body.cpp:309`, constructor `Ext/Rules/Body.h:741`): a team
    /// type's own unset key falls back here; -1 keeps the stock behaviour
    /// (`Ext/Team/Body.cpp:...`).
    #[serde(default = "minus_one_i32")]
    pub set_recruitable_on_liberate: i32,
    /// `[AudioVisual] NoAlphaImageOnBuildup=`, default `no`
    /// (`Ext/Rules/Body.cpp:645`, constructor `Ext/Rules/Body.h:1038`): the
    /// fallback for a BuildingType's own `Nullable<bool>`.
    #[serde(default)]
    pub no_alpha_image_on_buildup: bool,
    /// `[General] ParadropDelay=`, default **5**
    /// (`Ext/Rules/Body.cpp:522`, constructor `Ext/Rules/Body.h:936`): the
    /// fallback for an AircraftType's own `Nullable<int>`
    /// (`Ext/Aircraft/Hooks.cpp:1274`).
    #[serde(default = "default_paradrop_delay")]
    pub paradrop_delay: i32,
    /// `[General] AircraftSpawnFromEdge=`, default `owner`
    /// (`Ext/Rules/Body.cpp:193`, constructor `Ext/Rules/Body.h:636`): the
    /// fallback for an AircraftType's own `Nullable<EdgeType>`
    /// (`Ext/Aircraft/Body.cpp:88`).
    #[serde(default)]
    pub aircraft_spawn_from_edge: crate::rules::phobos_aircraft_edge::PhobosEdgeType,
    /// `[General] AircraftRetreatToEdge=`, default `owner`
    /// (`Ext/Rules/Body.cpp:194`, constructor `Ext/Rules/Body.h:637`): the
    /// fallback for an AircraftType's own `RetreatToEdge=` (`Ext/Aircraft/Body.cpp:89`).
    #[serde(default)]
    pub aircraft_retreat_to_edge: crate::rules::phobos_aircraft_edge::PhobosEdgeType,
}

/// `RadarJamHouses=`'s constructor value (`Ext/Rules/Body.h:657`).
fn radar_jam_houses_default() -> crate::rules::phobos_shields::AffectedHouse {
    crate::rules::phobos_shields::AffectedHouse::ENEMIES
}

/// `RadarJamDelay=`'s constructor value (`Ext/Rules/Body.h:658`).
fn radar_jam_delay_default() -> i32 {
    30
}

/// `NoQueueUpToEnter.BoardDistance=`'s constructor value
/// (`Ext/Rules/Body.h:641`).
fn no_queue_board_distance_default() -> i32 {
    384
}

/// `1.0`, the constructor value of the three plain `[CombatDamage]` damage
/// multipliers (`Ext/Rules/Body.h:770-772`) and `[General] VeteranReload=`
/// (`Ext/Rules/Body.h:808`), stored as bits so `PhobosGlobals` keeps `Eq`.
fn one_f64() -> NativeF64Bits {
    NativeF64Bits::ONE
}

/// `AircraftFiringForceScatter=`'s constructor value
/// (`Ext/Rules/Body.h:888`): vanilla forces the scatter.
fn aircraft_firing_force_scatter_default() -> bool {
    true
}

/// `ChronoSpherePreDelay=`'s constructor value (`Ext/Rules/Body.h:579`).
fn sixty() -> i32 {
    60
}

/// `JumpjetRotateOnCrash=`'s constructor value (`Ext/Rules/Body.h:596`).
fn truth() -> bool {
    true
}

/// `CombatAlert.Interval=`'s constructor value (`Ext/Rules/Body.h:787`).
fn combat_alert_interval_default() -> i32 {
    150
}

/// `VisualScatter.Min=`'s constructor value (`Ext/Rules/Body.h:758`).
fn visual_scatter_min_default() -> i32 {
    8
}

/// `VisualScatter.Max=`'s constructor value (`Ext/Rules/Body.h:759`).
fn visual_scatter_max_default() -> i32 {
    32
}

/// `[General] SubterraneanSpeed=`'s constructor value
/// (`Ext/Rules/Body.h:583`).
fn subterranean_speed_default() -> i32 {
    19
}

/// `[General] SubterraneanHeight=`'s constructor value
/// (`Ext/Rules/Body.h:584`).
fn subterranean_height_default() -> i32 {
    -256
}

/// `[AudioVisual] DisplayIncome.Delay=`'s constructor value
/// (`Ext/Rules/Body.h:726`).
fn display_income_delay_default() -> i32 {
    15
}

/// `[General] ParadropDelay=`'s constructor value (`Ext/Rules/Body.h:936`).
fn default_paradrop_delay() -> i32 {
    5
}

/// `[General] ParadropEndDelay=`'s constructor value (`Ext/Rules/Body.h:937`).
fn default_paradrop_end_delay() -> i32 {
    5
}

impl Default for PhobosGlobals {
    fn default() -> Self {
        Self {
            shrapnel_affects_ground: false,
            shrapnel_affects_buildings: false,
            auto_death_technos_exist_allow_limboed: false,
            auto_death_technos_dont_exist_allow_limboed: false,
            shrapnel_ignore_hit_buildings: false,
            shrapnel_use_weapon_targeting: false,
            shrapnel_obey_warhead_trigger_conditions: true,
            projectile_interceptable: false,
            interceptor_guard_range_is_cylindrical: false,
            allow_weapon_select_against_walls: false,
            interceptor_apply_firepower_mult: true,
            return_weapon_apply_firepower_mult: false,
            overload_particle_sys_count: 5,
            affects_invoker_only_ignore_invoker_state: true,
            show_designator_range: true,
            super_weapon_timer_percentage: false,
            tooltip_background_color: [0, 0, 0],
            tooltip_background_opacity: 100,
            tooltip_background_blur_size: crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO,
            anim_remap_default_color_scheme: 0,
            warhead_particle_alpha_image_is_light_flash: false,
            light_flash_alpha_image_detail_level: 0,
            firing_anim_update: false,
            extra_range_target_moving: 0,
            extra_range_target_moving_close_range_only: false,
            extra_range_firer_moving: 0,
            extra_range_prefiring: 0,
            extra_range_prefiring_include_burst: true,
            pips_generic_size: [4, 0],
            pips_generic_buildings_size: [4, 2],
            pips_ammo_size: [4, 0],
            pips_ammo_buildings_size: [4, 2],
            height_shadow_scaling: false,
            height_shadow_scaling_min_scale: NativeF64Bits::from_bits(0.0f64.to_bits()),
            air_shadow_base_scale_log: NativeF64Bits::from_bits(0.693_376_137f64.to_bits()),
            draw_turret_shadow: false,
            auto_death_on_owner_change_ignore_revert_on_exit: false,
            auto_death_allow_limboed: true,
            convert_reset_mind_control: false,
            building_guard_retry_delay: None,
            paradrop_end_delay: 5,
            explodes_kill_passengers: true,
            explodes_during_buildup: true,
            not_human_random_death_sequence: false,
            keep_range_allow_ai: false,
            keep_range_allow_player: false,
            art_image_swap: false,
            ai_guard_area_targeting_delay: None,
            ai_normal_targeting_delay: None,
            player_normal_targeting_delay: None,
            player_guard_area_targeting_delay: None,
            ai_attack_move_targeting_delay: None,
            player_attack_move_targeting_delay: None,
            distribute_targeting_frame: false,
            distribute_targeting_frame_ai_only: true,
            tool_tip_background_blur_size: NativeF32Bits::POSITIVE_ZERO,
            repair_base_nodes: false,
            crush_slowdown_multiplier: crush_slowdown_default(),
            skip_crush_slowdown: false,
            aircraft_firing_force_scatter: aircraft_firing_force_scatter_default(),
            bunker_state_update_delay: fifteen(),
            chrono_sphere_pre_delay: sixty(),
            chrono_sphere_delay: 0,
            mind_control_threat_delay: 0,
            jumpjet_rotate_on_crash: truth(),
            jumpjet_climb_ignore_building: false,
            sinkable: None,
            sink_speed: 5,
            paradrop_mission: MissionType::Guard,
            ai_paradrop_mission: MissionType::Hunt,
            aircraft_weapon_kick_out_passengers: true,
            secondary_fire_sequence_land_only: true,
            damaged_speed: default_damaged_speed(),
            keep_range_early_stop: 0,
            parasite_grapple_anim: None,
            parasite_allow_water_exit: None,
            parasite_disable_particle_system: false,
            cloak_kick_out_parasite: false,
            cloak_anims: Vec::new(),
            decloak_anims: Vec::new(),
            sinkable_squid_grab: true,
            drain_money_display: false,
            drain_money_display_houses: AffectedHouse::ALL,
            drain_money_display_on_target: false,
            drain_money_display_on_target_use_display_income: true,
            display_income_houses: AffectedHouse::ALL,
            extended_aircraft_missions: false,
            extended_aircraft_missions_unland_damage: 0,
            amphibious_enter: false,
            amphibious_unload: false,
            ambient_damage_ignore_target: false,
            terrain_is_passable: false,
            tibtree_is_passable: false,
            terrain_can_be_built_on: false,
            tibtree_can_be_built_on: false,
            apply_modifiers_on_negative_damage: false,
            force_weapon_in_range_techno_only: true,
            force_weapon_in_range_apply_range_modifiers: false,
            force_aa_weapon_in_range_apply_range_modifiers: false,
            penetrates_transport_level: 10,
            driver_killed_kept_passengers: false,
            driver_killed_kill_passengers: false,
            build_limit_group_content_if_any_match: false,
            build_limit_group_not_buildable_if_queue_match: false,
            attack_move_aggressive: false,
            attack_move_update_target: false,
            approach_target_stop_when_in_range: false,
            attack_move_ignore_weapon_check: false,
            spawner_attack_immediately: false,
            spawner_use_turret_facing: false,
            spawner_recycle_range: -1,
            spawner_recycle_on_turret: false,
            promote_include_spawns: false,
            promote_veteran_animation: Vec::new(),
            promote_elite_animation: Vec::new(),
            open_topped_ignore_rangefinding: false,
            open_topped_allow_firing_if_deactivated: true,
            open_topped_allow_firing_if_attacked_by_locomotor: true,
            open_topped_share_transport_target: true,
            open_topped_use_transport_range_modifiers: false,
            open_topped_check_transport_disable_weapons: false,
            open_topped_decloak_to_fire: false,
            open_topped_fire_while_moving: true,
            open_transport_range_bonus: 0,
            open_transport_damage_multiplier: crate::util::native_x87::NativeF32Bits::ONE,
            open_transport_fire_while_moving: true,
            no_rearm_under_emp: false,
            no_rearm_temporal: false,
            no_reload_under_emp: false,
            no_reload_temporal: false,
            no_turret_track_target: false,
            omni_fire_turn_to_target: false,
            building_start_facing: 0,
            building_start_facing_random: false,
            passengers_sync_owner: false,
            passengers_sync_owner_revert_on_exit: true,
            alternate_flh_on_turret: true,
            alternate_flh_apply_vehicle: false,
            vertical_aircraft_fix: true,
            building_radio_link_sync_owner: true,
            ivan_icon_visibility: crate::rules::phobos_shields::AffectedHouse::OWNER,
            arcing_allow_elevation_inaccuracy: true,
            debris_conventional: false,
            temporal_apply_multiplier: false,
            temporal_apply_versus: false,
            can_target_iron_curtained: true,
            can_target_ai_iron_curtained: false,
            auto_target_iron_curtained: true,
            cylinder_rangefinding: false,
            recount_burst: false,
            ai_superweapon_delay: None,
            reveal_houses: AffectedHouse::TEAM,
            berzerk_targeting: AffectedHouse::ALL,
            strafing_simulate_burst: false,
            strafing_use_ammo_per_shot: false,
            strafing_target_cell: false,
            extra_threat_is_threat: NativeF64Bits::POSITIVE_ZERO,
            extra_threat_in_range: NativeF64Bits::POSITIVE_ZERO,
            extra_threat_coefficient_in_range_distance: NativeF64Bits::POSITIVE_ZERO,
            extra_threat_coefficient_facing: NativeF64Bits::POSITIVE_ZERO,
            extra_threat_coefficient_distance_to_last_target: NativeF64Bits::POSITIVE_ZERO,
            combat_alert: false,
            combat_alert_default: None,
            combat_alert_ignore_building: true,
            combat_alert_suppress_if_in_screen: true,
            combat_alert_interval: combat_alert_interval_default(),
            combat_alert_suppress_if_ally_damage: true,
            combat_alert_make_a_voice: true,
            combat_alert_use_feedback_voice: true,
            combat_alert_use_attack_voice: true,
            combat_alert_use_eva: true,
            destroy_anim_random: true,
            visual_scatter_min: visual_scatter_min_default(),
            visual_scatter_max: visual_scatter_max_default(),
            auto_target_no_threat_buildings: false,
            auto_target_ai_no_threat_buildings: true,
            damage_owner_multiplier: NativeF64Bits::ONE,
            damage_allies_multiplier: NativeF64Bits::ONE,
            damage_enemies_multiplier: NativeF64Bits::ONE,
            damage_owner_multiplier_not_affects_enemies: None,
            damage_allies_multiplier_not_affects_enemies: None,
            damage_owner_multiplier_berzerk: None,
            damage_allies_multiplier_berzerk: None,
            damage_enemies_multiplier_berzerk: None,
            remove_mind_control_silent: false,
            merge_building_damage: false,
            shake_is_local: false,
            radar_jam_houses: radar_jam_houses_default(),
            radar_jam_delay: radar_jam_delay_default(),
            no_queue_up_to_enter: false,
            no_queue_up_to_enter_board_distance: no_queue_board_distance_default(),
            no_queue_up_to_enter_buildings: None,
            no_queue_up_to_unload: false,
            no_queue_up_to_unload_buildings: None,
            reload_in_transport: false,
            infantry_gain_self_heal_cap: None,
            units_gain_self_heal_cap: None,
            gain_self_heal_allow_multiplay_passive: true,
            gain_self_heal_from_player_control: false,
            gain_self_heal_from_allies: false,
            aircraft_docking_dir_default_to_pose_dir: true,
            pose_dir_production: None,
            pose_dir_field: None,
            veteran_reload: NativeF64Bits::ONE,
            veteran_empty_reload: None,
            is_voice_created_global: false,
            use_disguise_movement_speed: None,
            hover_drownable: true,
            only_use_land_sequences: false,
            anim_crater_destroy_tiberium: true,
            harvester_dump_amount: NativeF32Bits::POSITIVE_ZERO,
            harvester_scan_after_unload: false,
            combat_light_detail_level: 0,
            combat_light_detail_level_check_colored: false,
            subterranean_speed: subterranean_speed_default(),
            subterranean_height: subterranean_height_default(),
            veteran_range: NativeF64Bits::ONE,
            display_income: false,
            display_income_delay: display_income_delay_default(),
            set_recruitable_on_liberate: -1,
            no_alpha_image_on_buildup: false,
            walk_locomotor_makes_wake: false,
            drive_locomotor_makes_wake: true,
            hover_locomotor_makes_wake: true,
            ship_locomotor_makes_wake: true,
            paradrop_delay: default_paradrop_delay(),
            aircraft_spawn_from_edge: crate::rules::phobos_aircraft_edge::PhobosEdgeType::Owner,
            aircraft_retreat_to_edge: crate::rules::phobos_aircraft_edge::PhobosEdgeType::Owner,
        }
    }
}

impl PhobosGlobals {
    pub fn from_ini(ini: &IniFile) -> Self {
        let mut globals = section_settings(ini.section_or_empty("CombatDamage"));
        // `RulesExt::LoadFromINIFile` reads this block in the same pass
        // (`Ext/Rules/Body.cpp:291-304`); the two sections share no key, so
        // their relative order does not matter.
        let audio_visual = ini.section_or_empty("AudioVisual");
        // `Ext/Rules/Body.cpp:176`: the designator/inhibitor range-ring rules
        // switch, read in the same `[AudioVisual]` pass as the income labels.
        globals.show_designator_range =
            audio_visual.read_bool("ShowDesignatorRange", globals.show_designator_range);
        globals.super_weapon_timer_percentage = audio_visual.read_bool(
            "SuperWeaponTimer.Percentage",
            globals.super_weapon_timer_percentage,
        );
        globals.tooltip_background_color = audio_visual
            .read_phobos_color("ToolTip.Background.Color", globals.tooltip_background_color);
        globals.tooltip_background_opacity = audio_visual.read_ares_int(
            "ToolTip.Background.Opacity",
            globals.tooltip_background_opacity,
        );
        let tooltip_blur = audio_visual.read_phobos_double(
            "ToolTip.Background.BlurSize",
            f64::from(f32::from_bits(globals.tooltip_background_blur_size.bits())),
        );
        globals.tooltip_background_blur_size =
            crate::util::native_x87::NativeF32Bits::from_bits((tooltip_blur as f32).to_bits());
        // `Ext/Rules/Body.cpp:174`, same `[AudioVisual]` pass.
        globals.anim_remap_default_color_scheme = audio_visual.read_int(
            "AnimRemapDefaultColorScheme",
            globals.anim_remap_default_color_scheme,
        );
        // `Ext/Rules/Body.cpp:403,406`, same `[AudioVisual]` pass.
        globals.warhead_particle_alpha_image_is_light_flash = audio_visual.read_bool(
            "WarheadParticleAlphaImageIsLightFlash",
            globals.warhead_particle_alpha_image_is_light_flash,
        );
        globals.light_flash_alpha_image_detail_level = audio_visual.read_int(
            "LightFlashAlphaImageDetailLevel",
            globals.light_flash_alpha_image_detail_level,
        );
        // `Ext/Rules/Body.cpp:160-163`: the four pip-size pairs, `Point2D`
        // values (Read2Integers); the constructor defaults reproduce the
        // stock 4 / (building ? 2 : 0) advance.
        globals.pips_generic_size =
            audio_visual.read_phobos_int_pair("Pips.Generic.Size", globals.pips_generic_size);
        globals.pips_generic_buildings_size = audio_visual.read_phobos_int_pair(
            "Pips.Generic.Buildings.Size",
            globals.pips_generic_buildings_size,
        );
        globals.pips_ammo_size =
            audio_visual.read_phobos_int_pair("Pips.Ammo.Size", globals.pips_ammo_size);
        globals.pips_ammo_buildings_size = audio_visual
            .read_phobos_int_pair("Pips.Ammo.Buildings.Size", globals.pips_ammo_buildings_size);
        globals.firing_anim_update =
            audio_visual.read_bool("FiringAnim.Update", globals.firing_anim_update);
        // `Ext/Rules/Body.cpp:291-297`: the income-label switch and cadence. A
        // read `DisplayIncome.Delay=0` would crash the native label, so the
        // reader forces it to 1.
        globals.display_income = audio_visual.read_bool("DisplayIncome", globals.display_income);
        globals.display_income_delay =
            audio_visual.read_int("DisplayIncome.Delay", globals.display_income_delay);
        if globals.display_income_delay == 0 {
            globals.display_income_delay = 1;
        }
        // `Ext/Rules/Body.cpp:404`: which combat-light descriptors a flash
        // draws (`CombatLightDetailLevel.CheckColored=` is the sibling key).
        globals.combat_light_detail_level =
            audio_visual.read_int("CombatLightDetailLevel", globals.combat_light_detail_level);
        // `Ext/Rules/Body.cpp:405`: whether a partly-disabled colour still
        // flashes; the warhead's own key overrides it.
        globals.combat_light_detail_level_check_colored = audio_visual.read_bool(
            "CombatLightDetailLevel.CheckColored",
            globals.combat_light_detail_level_check_colored,
        );
        // `Ext/Rules/Body.cpp:645`: the `[AudioVisual]` fallback a BuildingType's
        // `NoAlphaImageOnBuildup=` overrides.
        globals.no_alpha_image_on_buildup =
            audio_visual.read_bool("NoAlphaImageOnBuildup", globals.no_alpha_image_on_buildup);
        // `Ext/Rules/Body.cpp:182-189`: the height-scaled shadow block. The
        // base scale is stored as `-ln(min(v, 1))`, and a base above 0.98
        // switches `HeightShadowScaling` off no matter what the key says.
        globals.height_shadow_scaling =
            audio_visual.read_bool("HeightShadowScaling", globals.height_shadow_scaling);
        globals.height_shadow_scaling_min_scale = read_double_bits(
            &audio_visual,
            "HeightShadowScaling.MinScale",
            globals.height_shadow_scaling_min_scale,
        );
        let air_shadow_base_scale =
            audio_visual.read_phobos_nullable_double("AirShadowBaseScale", None);
        if let Some(base) = air_shadow_base_scale.filter(|base| *base > 0.0) {
            // `libm` keeps the log identical on every target.
            globals.air_shadow_base_scale_log =
                NativeF64Bits::from_bits((-libm::log(base.min(1.0))).to_bits());
        }
        if air_shadow_base_scale.is_some_and(|base| base > 0.98) {
            globals.height_shadow_scaling = false;
        }
        // `Ext/Rules/Body.cpp:173`: the ART `TurretShadow=` fallback.
        globals.draw_turret_shadow =
            audio_visual.read_bool("DrawTurretShadow", globals.draw_turret_shadow);
        globals.parasite_grapple_anim = audio_visual
            .read_type_name("Parasite.GrappleAnim", 0x100)
            .map(str::to_owned);
        globals.drain_money_display =
            audio_visual.read_bool("DrainMoneyDisplay", globals.drain_money_display);
        // `Ext/Rules/Body.cpp:634-636`: the aircraft landing-direction
        // globals, all `[AudioVisual]`.
        globals.aircraft_docking_dir_default_to_pose_dir = audio_visual.read_bool(
            "AircraftDockingDir.DefaultToPoseDir",
            globals.aircraft_docking_dir_default_to_pose_dir,
        );
        globals.pose_dir_production = audio_visual
            .read_phobos_nullable_int("PoseDir.Production", globals.pose_dir_production);
        globals.pose_dir_field =
            audio_visual.read_phobos_nullable_int("PoseDir.Field", globals.pose_dir_field);
        globals.walk_locomotor_makes_wake =
            audio_visual.read_bool("WalkLocomotorMakesWake", globals.walk_locomotor_makes_wake);
        globals.drive_locomotor_makes_wake = audio_visual.read_bool(
            "DriveLocomotorMakesWake",
            globals.drive_locomotor_makes_wake,
        );
        globals.hover_locomotor_makes_wake = audio_visual.read_bool(
            "HoverLocomotionClassMakesWake",
            globals.hover_locomotor_makes_wake,
        );
        globals.ship_locomotor_makes_wake = audio_visual.read_bool(
            "ShipLocomotionClassMakesWake",
            globals.ship_locomotor_makes_wake,
        );
        globals.drain_money_display_houses = crate::rules::phobos_shields::read_affected_house(
            audio_visual,
            "DrainMoneyDisplay.Houses",
            globals.drain_money_display_houses,
        );
        globals.drain_money_display_on_target = audio_visual.read_bool(
            "DrainMoneyDisplay.OnTarget",
            globals.drain_money_display_on_target,
        );
        globals.drain_money_display_on_target_use_display_income = audio_visual.read_bool(
            "DrainMoneyDisplay.OnTarget.UseDisplayIncome",
            globals.drain_money_display_on_target_use_display_income,
        );
        globals.display_income_houses = crate::rules::phobos_shields::read_affected_house(
            audio_visual,
            "DisplayIncome.Houses",
            globals.display_income_houses,
        );
        globals.reveal_houses = crate::rules::phobos_shields::read_affected_house(
            audio_visual,
            "RevealHouses",
            globals.reveal_houses,
        );
        // `[AudioVisual] Promote.VeteranAnimation=` / `.EliteAnimation=`
        // (`Ext/Rules/Body.cpp:316-317`): `ValueableVector<AnimTypeClass*>`
        // reads, so an absent or empty key keeps the list and any other value
        // replaces it. Names stay until a consumer resolves them against the
        // AnimType registry (`has_anim_type`).
        let keep = |name: &str| Some(name.to_owned());
        globals.promote_veteran_animation =
            crate::rules::native_processing::read_valueable_type_vector(
                audio_visual,
                "Promote.VeteranAnimation",
                &globals.promote_veteran_animation,
                keep,
            );
        globals.promote_elite_animation =
            crate::rules::native_processing::read_valueable_type_vector(
                audio_visual,
                "Promote.EliteAnimation",
                &globals.promote_elite_animation,
                keep,
            );
        // `Ext/Rules/Body.cpp:325-326`: the `VisualScatter=` radius pair,
        // read just ahead of the `CombatAlert*` block (`:355-364`).
        // `Ext/Rules/Body.cpp:597`: an `[AudioVisual]` read, not CombatDamage.
        globals.remove_mind_control_silent = audio_visual.read_bool(
            "RemoveMindControl.Silent",
            globals.remove_mind_control_silent,
        );
        globals.visual_scatter_min =
            audio_visual.read_int("VisualScatter.Min", globals.visual_scatter_min);
        globals.visual_scatter_max =
            audio_visual.read_int("VisualScatter.Max", globals.visual_scatter_max);
        // `[AudioVisual] CombatAlert*` (`Ext/Rules/Body.cpp:355-364`): the
        // master switch, the nullable default and interval, and the three
        // type-key fallbacks. `Valueable` reads from the constructor values;
        // `CombatAlert.Default` is `Nullable<bool>` and stays unset until a
        // layer names it.
        globals.combat_alert = audio_visual.read_bool("CombatAlert", globals.combat_alert);
        globals.combat_alert_default = audio_visual
            .read_phobos_nullable_bool("CombatAlert.Default", globals.combat_alert_default);
        globals.combat_alert_ignore_building = audio_visual.read_bool(
            "CombatAlert.IgnoreBuilding",
            globals.combat_alert_ignore_building,
        );
        globals.combat_alert_suppress_if_in_screen = audio_visual.read_bool(
            "CombatAlert.SuppressIfInScreen",
            globals.combat_alert_suppress_if_in_screen,
        );
        globals.combat_alert_interval =
            audio_visual.read_ares_int("CombatAlert.Interval", globals.combat_alert_interval);
        globals.combat_alert_suppress_if_ally_damage = audio_visual.read_bool(
            "CombatAlert.SuppressIfAllyDamage",
            globals.combat_alert_suppress_if_ally_damage,
        );
        globals.combat_alert_make_a_voice =
            audio_visual.read_bool("CombatAlert.MakeAVoice", globals.combat_alert_make_a_voice);
        globals.combat_alert_use_feedback_voice = audio_visual.read_bool(
            "CombatAlert.UseFeedbackVoice",
            globals.combat_alert_use_feedback_voice,
        );
        globals.combat_alert_use_attack_voice = audio_visual.read_bool(
            "CombatAlert.UseAttackVoice",
            globals.combat_alert_use_attack_voice,
        );
        globals.combat_alert_use_eva =
            audio_visual.read_bool("CombatAlert.UseEVA", globals.combat_alert_use_eva);
        // `Ext/Rules/Body.cpp:306`: an `[AudioVisual]` read, not General.
        globals.is_voice_created_global =
            audio_visual.read_bool("IsVoiceCreatedGlobal", globals.is_voice_created_global);
        // `[General]` force-weapon defaults (`Ext/Rules/Body.cpp:230-232`);
        // the three sections share no key, so their order does not matter.
        let general = ini.section_or_empty("General");
        // `Ext/Rules/Body.cpp:511-515`: `Valueable<Leptons>` keys, parsed as
        // cells × 256 (`Parser<Leptons>`); a failed read keeps the default.
        globals.extra_range_target_moving = general
            .read_phobos_leptons("ExtraRange.TargetMoving", globals.extra_range_target_moving);
        globals.extra_range_target_moving_close_range_only = general.read_bool(
            "ExtraRange.TargetMoving.CloseRangeOnly",
            globals.extra_range_target_moving_close_range_only,
        );
        globals.extra_range_firer_moving =
            general.read_phobos_leptons("ExtraRange.FirerMoving", globals.extra_range_firer_moving);
        globals.extra_range_prefiring =
            general.read_phobos_leptons("ExtraRange.Prefiring", globals.extra_range_prefiring);
        globals.extra_range_prefiring_include_burst = general.read_bool(
            "ExtraRange.Prefiring.IncludeBurst",
            globals.extra_range_prefiring_include_burst,
        );
        // `Ext/Rules/Body.cpp:222`: a `[General]` read, not CombatDamage.
        globals.convert_reset_mind_control = general.read_bool(
            "Convert.ResetMindControl",
            globals.convert_reset_mind_control,
        );
        // `Ext/Rules/Body.cpp:579`: the armed building guard recheck fallback
        // for a BuildingType that leaves `GuardRetryDelay` unset.
        globals.building_guard_retry_delay = general.read_phobos_nullable_partial_int_pair(
            "BuildingGuardRetryDelay",
            globals.building_guard_retry_delay,
        );
        globals.bunker_state_update_delay =
            general.read_int("BunkerStateUpdateDelay", globals.bunker_state_update_delay);
        // `Ext/Rules/Body.cpp:491`: `[General] ShakeIsLocal=`, a
        // `Valueable<bool>` default `no` (constructor `Ext/Rules/Body.h:906`).
        // It is the fallback for a warhead's own `ShakeIsLocal=`.
        globals.shake_is_local = general.read_bool("ShakeIsLocal", globals.shake_is_local);
        // `Ext/Rules/Body.cpp:521`: `Valueable<Mission>`; a named mission
        // replaces the constructor's `Hunt`, any other text keeps it
        // (`Utilities/TemplateDef.h:510-528`).
        globals.ai_paradrop_mission = general
            .read_name("AIParadropMission", 0x80)
            .and_then(crate::rules::mission_data::MissionType::from_map_name)
            .unwrap_or(globals.ai_paradrop_mission);
        // `Ext/Rules/Body.cpp:523`: `Valueable<int>`, the same read.
        globals.paradrop_end_delay =
            general.read_int("ParadropEndDelay", globals.paradrop_end_delay);
        // `Ext/Rules/Body.cpp:470-471`, both constructed `true`.
        globals.explodes_kill_passengers =
            general.read_bool("Explodes.KillPassengers", globals.explodes_kill_passengers);
        globals.explodes_during_buildup =
            general.read_bool("Explodes.DuringBuildup", globals.explodes_during_buildup);
        globals.ambient_damage_ignore_target = general.read_bool(
            "AmbientDamage.IgnoreTarget",
            globals.ambient_damage_ignore_target,
        );
        // `Ext/Rules/Body.cpp:479-480`: the terrain passability fallbacks,
        // both constructed false.
        globals.terrain_is_passable =
            general.read_bool("Terrain.IsPassable", globals.terrain_is_passable);
        globals.tibtree_is_passable =
            general.read_bool("Tibtree.IsPassable", globals.tibtree_is_passable);
        globals.terrain_can_be_built_on =
            general.read_bool("Terrain.CanBeBuiltOn", globals.terrain_can_be_built_on);
        globals.tibtree_can_be_built_on =
            general.read_bool("Tibtree.CanBeBuiltOn", globals.tibtree_can_be_built_on);
        // `Ext/Rules/Body.cpp:492`: the negative-damage modifier fallback,
        // constructed false.
        globals.apply_modifiers_on_negative_damage = general.read_bool(
            "ApplyModifiersOnNegativeDamage",
            globals.apply_modifiers_on_negative_damage,
        );
        // `Ext/Rules/Body.cpp:418`: the AnimType ART key's `[General]`
        // fallback, constructed true.
        globals.anim_crater_destroy_tiberium = general.read_bool(
            "AnimCraterDestroyTiberium",
            globals.anim_crater_destroy_tiberium,
        );
        // `Ext/Rules/Body.cpp:525`: the weapon key's `[General]` fallback.
        globals.cylinder_rangefinding =
            general.read_bool("CylinderRangefinding", globals.cylinder_rangefinding);
        globals.debris_conventional =
            general.read_bool("Debris.Conventional", globals.debris_conventional);
        globals.force_weapon_in_range_techno_only = general.read_bool(
            "ForceWeapon.InRange.TechnoOnly",
            globals.force_weapon_in_range_techno_only,
        );
        // `Ext/Rules/Body.cpp:197-200`: the boarding-queue pair and its
        // Building override, all `[General]`.
        globals.no_queue_up_to_enter =
            general.read_bool("NoQueueUpToEnter", globals.no_queue_up_to_enter);
        globals.no_queue_up_to_enter_board_distance = general.read_int(
            "NoQueueUpToEnter.BoardDistance",
            globals.no_queue_up_to_enter_board_distance,
        );
        globals.no_queue_up_to_enter_buildings = general.read_phobos_nullable_bool(
            "NoQueueUpToEnter.Buildings",
            globals.no_queue_up_to_enter_buildings,
        );
        // `Ext/Rules/Body.cpp:199-201`: the unload-queue global and its
        // Building override.
        globals.no_queue_up_to_unload =
            general.read_bool("NoQueueUpToUnload", globals.no_queue_up_to_unload);
        globals.no_queue_up_to_unload_buildings = general.read_phobos_nullable_bool(
            "NoQueueUpToUnload.Buildings",
            globals.no_queue_up_to_unload_buildings,
        );
        globals.reload_in_transport =
            general.read_bool("ReloadInTransport", globals.reload_in_transport);
        // `Ext/Rules/Body.cpp:111-115`: the gained-self-heal caps and gates.
        // Each key stays literal on its reader line: the v3 scanner attributes
        // a read by the literal argument of the call on that line.
        let infantry_cap = globals.infantry_gain_self_heal_cap;
        globals.infantry_gain_self_heal_cap =
            general.read_phobos_nullable_int("InfantryGainSelfHealCap", infantry_cap);
        globals.units_gain_self_heal_cap = general
            .read_phobos_nullable_int("UnitsGainSelfHealCap", globals.units_gain_self_heal_cap);
        let allow_passive = globals.gain_self_heal_allow_multiplay_passive;
        globals.gain_self_heal_allow_multiplay_passive =
            general.read_bool("GainSelfHealAllowMultiplayPassive", allow_passive);
        let from_player_control = globals.gain_self_heal_from_player_control;
        globals.gain_self_heal_from_player_control =
            general.read_bool("GainSelfHealFromPlayerControl", from_player_control);
        globals.gain_self_heal_from_allies =
            general.read_bool("GainSelfHealFromAllies", globals.gain_self_heal_from_allies);
        // `Ext/Rules/Body.cpp:213-214`: the Ares radar-jam scan's House filter
        // (`Valueable<AffectedHouse>`, default Enemies) and interval.
        globals.radar_jam_houses = crate::rules::phobos_shields::read_affected_house(
            general,
            "RadarJamHouses",
            globals.radar_jam_houses,
        );
        globals.radar_jam_delay = general.read_int("RadarJamDelay", globals.radar_jam_delay);
        // `Ext/Rules/Body.cpp:221`: a nullable `[General]` read.
        globals.use_disguise_movement_speed = general.read_phobos_nullable_bool(
            "UseDisguiseMovementSpeed",
            globals.use_disguise_movement_speed,
        );
        // `Ext/Rules/Body.cpp:388-389`.
        globals.veteran_reload = read_double_bits(general, "VeteranReload", globals.veteran_reload);
        globals.veteran_empty_reload =
            read_nullable_double_bits(general, "VeteranEmptyReload", globals.veteran_empty_reload);
        // `Ext/Rules/Body.cpp:475`, a `Valueable<bool>` default `yes`.
        globals.hover_drownable = general.read_bool("HoverDrownable", globals.hover_drownable);
        globals.only_use_land_sequences =
            general.read_bool("OnlyUseLandSequences", globals.only_use_land_sequences);
        // `Ext/Rules/Body.cpp:110`, `:125-126`, `:390`, `:416`, `:309`: the
        // harvester dump/scan, subterranean and veteran-range fallbacks, plus
        // the team-type recruitable override.
        let dump_amount = f32::from_bits(globals.harvester_dump_amount.bits());
        globals.harvester_dump_amount = NativeF32Bits::from_bits(
            general
                .read_float("HarvesterDumpAmount", dump_amount)
                .to_bits(),
        );
        globals.harvester_scan_after_unload = general.read_bool(
            "HarvesterScanAfterUnload",
            globals.harvester_scan_after_unload,
        );
        // `INI_EX::ReadSpeed` (`Utilities/INIParser.h:155-166`): a non-negative
        // double becomes `min(Floor(min(v,100)*256/100), 255)`; a negative or
        // absent read keeps the current value.
        globals.subterranean_speed = general.read_double_with(
            "SubterraneanSpeed",
            globals.subterranean_speed,
            |current, parsed| {
                if parsed >= 0.0 {
                    (((parsed.min(100.0) * 256.0) / 100.0) as i32).min(255)
                } else {
                    current
                }
            },
        );
        globals.subterranean_height =
            general.read_int("SubterraneanHeight", globals.subterranean_height);
        globals.veteran_range = read_double_bits(general, "VeteranRange", globals.veteran_range);
        globals.set_recruitable_on_liberate = general.read_int(
            "SetRecruitableOnLiberate",
            globals.set_recruitable_on_liberate,
        );
        // `Ext/Rules/Body.cpp:218-219`, `yes`/`no` constructor defaults.
        globals.alternate_flh_on_turret =
            general.read_bool("AlternateFLH.OnTurret", globals.alternate_flh_on_turret);
        globals.alternate_flh_apply_vehicle = general.read_bool(
            "AlternateFLH.ApplyVehicle",
            globals.alternate_flh_apply_vehicle,
        );
        globals.destroy_anim_random =
            general.read_bool("DestroyAnim.Random", globals.destroy_anim_random);
        globals.not_human_random_death_sequence = general.read_bool(
            "NotHuman.RandomDeathSequence",
            globals.not_human_random_death_sequence,
        );
        globals.cloak_kick_out_parasite =
            general.read_bool("Cloak.KickOutParasite", globals.cloak_kick_out_parasite);
        globals.cloak_anims = audio_visual
            .read_list("CloakAnims", 0x800)
            .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
            .unwrap_or(globals.cloak_anims);
        globals.decloak_anims = audio_visual
            .read_list("DecloakAnims", 0x800)
            .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
            .unwrap_or(globals.decloak_anims);
        globals.sinkable_squid_grab =
            general.read_bool("Sinkable.SquidGrab", globals.sinkable_squid_grab);
        globals.keep_range_allow_ai =
            general.read_bool("KeepRange.AllowAI", globals.keep_range_allow_ai);
        globals.keep_range_allow_player =
            general.read_bool("KeepRange.AllowPlayer", globals.keep_range_allow_player);
        globals.art_image_swap = general.read_bool("ArtImageSwap", globals.art_image_swap);
        globals.ai_guard_area_targeting_delay = general.read_phobos_nullable_int(
            "AIGuardAreaTargetingDelay",
            globals.ai_guard_area_targeting_delay,
        );
        globals.ai_normal_targeting_delay = general
            .read_phobos_nullable_int("AINormalTargetingDelay", globals.ai_normal_targeting_delay);
        globals.player_normal_targeting_delay = general.read_phobos_nullable_int(
            "PlayerNormalTargetingDelay",
            globals.player_normal_targeting_delay,
        );
        globals.player_guard_area_targeting_delay = general.read_phobos_nullable_int(
            "PlayerGuardAreaTargetingDelay",
            globals.player_guard_area_targeting_delay,
        );
        globals.ai_attack_move_targeting_delay = general.read_phobos_nullable_int(
            "AIAttackMoveTargetingDelay",
            globals.ai_attack_move_targeting_delay,
        );
        globals.player_attack_move_targeting_delay = general.read_phobos_nullable_int(
            "PlayerAttackMoveTargetingDelay",
            globals.player_attack_move_targeting_delay,
        );
        globals.distribute_targeting_frame = general.read_bool(
            "DistributeTargetingFrame",
            globals.distribute_targeting_frame,
        );
        globals.distribute_targeting_frame_ai_only = general.read_bool(
            "DistributeTargetingFrame.AIOnly",
            globals.distribute_targeting_frame_ai_only,
        );
        // `Ext/Rules/Body.cpp:399`: a `[Basic]` read in the same pass.
        globals.repair_base_nodes = ini
            .section_or_empty("Basic")
            .read_bool("RepairBaseNodes", globals.repair_base_nodes);
        // `Ext/Rules/Body.cpp:171`: an `[AudioVisual]` `Valueable<float>`.
        globals.tool_tip_background_blur_size = NativeF32Bits::from_bits(
            audio_visual
                .read_float(
                    "ToolTip.Background.BlurSize",
                    f32::from_bits(globals.tool_tip_background_blur_size.bits()),
                )
                .to_bits(),
        );
        globals.crush_slowdown_multiplier = read_double_bits(
            &general,
            "CrushSlowdownMultiplier",
            globals.crush_slowdown_multiplier,
        );
        globals.skip_crush_slowdown =
            general.read_bool("SkipCrushSlowdown", globals.skip_crush_slowdown);
        globals.aircraft_firing_force_scatter = general.read_bool(
            "AircraftFiringForceScatter",
            globals.aircraft_firing_force_scatter,
        );
        globals.bunker_state_update_delay =
            general.read_int("BunkerStateUpdateDelay", globals.bunker_state_update_delay);
        globals.chrono_sphere_pre_delay =
            general.read_int("ChronoSpherePreDelay", globals.chrono_sphere_pre_delay);
        globals.chrono_sphere_delay =
            general.read_int("ChronoSphereDelay", globals.chrono_sphere_delay);
        globals.mind_control_threat_delay =
            general.read_int("MindControl.ThreatDelay", globals.mind_control_threat_delay);
        globals.jumpjet_rotate_on_crash = ini
            .section_or_empty("JumpjetControls")
            .read_bool("RotateOnCrash", globals.jumpjet_rotate_on_crash);
        globals.jumpjet_climb_ignore_building = general.read_bool(
            "JumpjetClimbIgnoreBuilding",
            globals.jumpjet_climb_ignore_building,
        );
        globals.sinkable = general.read_phobos_nullable_bool("Sinkable", globals.sinkable);
        globals.sink_speed = general.read_int("SinkSpeed", globals.sink_speed);
        globals.paradrop_mission = general.read_string_with(
            "ParadropMission",
            2048,
            globals.paradrop_mission,
            |current, value| MissionType::from_map_name(value).unwrap_or(current),
        );
        globals.ai_paradrop_mission = general.read_string_with(
            "AIParadropMission",
            2048,
            globals.ai_paradrop_mission,
            |current, value| MissionType::from_map_name(value).unwrap_or(current),
        );
        // `Ext/Rules/Body.cpp:522`: the `[General]` fallback an AircraftType's
        // `ParadropDelay=` overrides (`Ext/Aircraft/Hooks.cpp:1274`).
        globals.paradrop_delay = general.read_ares_int("ParadropDelay", globals.paradrop_delay);
        globals.paradrop_end_delay =
            general.read_ares_int("ParadropEndDelay", globals.paradrop_end_delay);
        // `Ext/Rules/Body.cpp:193`: the `[General]` fallback an AircraftType's
        // `SpawnFromEdge=` overrides (`Ext/Aircraft/Body.cpp:88`).
        globals.aircraft_spawn_from_edge = general.read_string_with(
            "AircraftSpawnFromEdge",
            32,
            globals.aircraft_spawn_from_edge,
            |current, value| {
                crate::rules::phobos_aircraft_edge::PhobosEdgeType::parse(value).unwrap_or(current)
            },
        );
        // `Ext/Rules/Body.cpp:194`: the `[General]` fallback an AircraftType's
        // `RetreatToEdge=` overrides (`Ext/Aircraft/Body.cpp:89`, `0x415A7A`).
        globals.aircraft_retreat_to_edge = general.read_string_with(
            "AircraftRetreatToEdge",
            32,
            globals.aircraft_retreat_to_edge,
            |current, value| {
                crate::rules::phobos_aircraft_edge::PhobosEdgeType::parse(value).unwrap_or(current)
            },
        );
        globals.aircraft_weapon_kick_out_passengers = general.read_bool(
            "AircraftWeapon.KickOutPassengers",
            globals.aircraft_weapon_kick_out_passengers,
        );
        globals.secondary_fire_sequence_land_only = general.read_bool(
            "SecondaryFireSequenceLandOnly",
            globals.secondary_fire_sequence_land_only,
        );
        globals.damaged_speed = read_double_bits(general, "DamagedSpeed", globals.damaged_speed);
        globals.keep_range_early_stop =
            general.read_ares_int("KeepRange.EarlyStopFrame", globals.keep_range_early_stop);
        globals.parasite_allow_water_exit = general.read_phobos_nullable_bool(
            "Parasite.AllowWaterExit",
            globals.parasite_allow_water_exit,
        );
        globals.force_weapon_in_range_apply_range_modifiers = general.read_bool(
            "ForceWeapon.InRange.ApplyRangeModifiers",
            globals.force_weapon_in_range_apply_range_modifiers,
        );
        globals.force_aa_weapon_in_range_apply_range_modifiers = general.read_bool(
            "ForceAAWeapon.InRange.ApplyRangeModifiers",
            globals.force_aa_weapon_in_range_apply_range_modifiers,
        );
        globals.build_limit_group_content_if_any_match = general.read_bool(
            "BuildLimitGroup.ContentIfAnyMatch",
            globals.build_limit_group_content_if_any_match,
        );
        globals.build_limit_group_not_buildable_if_queue_match = general.read_bool(
            "BuildLimitGroup.NotBuildableIfQueueMatch",
            globals.build_limit_group_not_buildable_if_queue_match,
        );
        globals.attack_move_aggressive =
            general.read_bool("AttackMove.Aggressive", globals.attack_move_aggressive);
        globals.attack_move_update_target =
            general.read_bool("AttackMove.UpdateTarget", globals.attack_move_update_target);
        globals.approach_target_stop_when_in_range = general.read_bool(
            "ApproachTarget.StopWhenInRange",
            globals.approach_target_stop_when_in_range,
        );
        globals.attack_move_ignore_weapon_check = general.read_bool(
            "AttackMove.IgnoreWeaponCheck",
            globals.attack_move_ignore_weapon_check,
        );
        globals.spawner_attack_immediately = general.read_bool(
            "Spawner.AttackImmediately",
            globals.spawner_attack_immediately,
        );
        globals.spawner_use_turret_facing =
            general.read_bool("Spawner.UseTurretFacing", globals.spawner_use_turret_facing);
        // `Valueable<Leptons>`: cells × 256 (F2I); a failed read keeps -1.
        globals.spawner_recycle_range =
            general.read_phobos_leptons("Spawner.RecycleRange", globals.spawner_recycle_range);
        globals.spawner_recycle_on_turret =
            general.read_bool("Spawner.RecycleOnTurret", globals.spawner_recycle_on_turret);
        globals.promote_include_spawns =
            general.read_bool("Promote.IncludeSpawns", globals.promote_include_spawns);
        // The eight `[General] OpenTopped.*` `Valueable<bool>` switches
        // (`Ext/Rules/Body.cpp:455-462`); `Valueable` has no unset state, so
        // these are plain `read_bool` reads from the constructor defaults.
        globals.open_topped_ignore_rangefinding = general.read_bool(
            "OpenTopped.IgnoreRangefinding",
            globals.open_topped_ignore_rangefinding,
        );
        globals.open_topped_allow_firing_if_deactivated = general.read_bool(
            "OpenTopped.AllowFiringIfDeactivated",
            globals.open_topped_allow_firing_if_deactivated,
        );
        globals.open_topped_allow_firing_if_attacked_by_locomotor = general.read_bool(
            "OpenTopped.AllowFiringIfAttackedByLocomotor",
            globals.open_topped_allow_firing_if_attacked_by_locomotor,
        );
        globals.open_topped_share_transport_target = general.read_bool(
            "OpenTopped.ShareTransportTarget",
            globals.open_topped_share_transport_target,
        );
        globals.open_topped_use_transport_range_modifiers = general.read_bool(
            "OpenTopped.UseTransportRangeModifiers",
            globals.open_topped_use_transport_range_modifiers,
        );
        globals.open_topped_check_transport_disable_weapons = general.read_bool(
            "OpenTopped.CheckTransportDisableWeapons",
            globals.open_topped_check_transport_disable_weapons,
        );
        globals.open_topped_decloak_to_fire = general.read_bool(
            "OpenTopped.DecloakToFire",
            globals.open_topped_decloak_to_fire,
        );
        globals.open_topped_fire_while_moving = general.read_bool(
            "OpenTopped.FireWhileMoving",
            globals.open_topped_fire_while_moving,
        );
        globals.open_transport_fire_while_moving = general.read_bool(
            "OpenTransport.FireWhileMoving",
            globals.open_transport_fire_while_moving,
        );
        // The four `NoRearm.*` / `NoReload.*` `Valueable<bool>` globals
        // (`Ext/Rules/Body.cpp:383-386`); `Valueable` has no unset state, so
        // these are plain `read_bool` reads from the constructor defaults.
        globals.no_rearm_under_emp =
            general.read_bool("NoRearm.UnderEMP", globals.no_rearm_under_emp);
        globals.no_rearm_temporal =
            general.read_bool("NoRearm.Temporal", globals.no_rearm_temporal);
        globals.no_reload_under_emp =
            general.read_bool("NoReload.UnderEMP", globals.no_reload_under_emp);
        globals.no_reload_temporal =
            general.read_bool("NoReload.Temporal", globals.no_reload_temporal);
        // `Valueable<bool>` (`Ext/Rules/Body.cpp:393`): no unset state.
        globals.no_turret_track_target =
            general.read_bool("NoTurret.TrackTarget", globals.no_turret_track_target);
        globals.omni_fire_turn_to_target =
            general.read_bool("OmniFire.TurnToTarget", globals.omni_fire_turn_to_target);
        globals.recount_burst = general.read_bool("RecountBurst", globals.recount_burst);
        globals.ai_superweapon_delay =
            general.read_phobos_nullable_int("AISuperWeaponDelay", globals.ai_superweapon_delay);
        // The StartFacing pair (`Ext/Rules/Body.cpp:626-627`): a Valueable<int>
        // and a Valueable<bool>, both read with their constructor value.
        globals.building_start_facing =
            general.read_int("BuildingStartFacing", globals.building_start_facing);
        globals.building_start_facing_random = general.read_bool(
            "BuildingStartFacing.Random",
            globals.building_start_facing_random,
        );
        // The two `Passengers.SyncOwner` `Valueable<bool>` globals
        // (`Ext/Rules/Body.cpp:467-468`); `Valueable` has no unset state, so
        // these are plain `read_bool` reads from the constructor defaults.
        globals.passengers_sync_owner =
            general.read_bool("Passengers.SyncOwner", globals.passengers_sync_owner);
        globals.passengers_sync_owner_revert_on_exit = general.read_bool(
            "Passengers.SyncOwner.RevertOnExit",
            globals.passengers_sync_owner_revert_on_exit,
        );
        globals.vertical_aircraft_fix =
            general.read_bool("Vertical.AircraftFix", globals.vertical_aircraft_fix);
        globals.building_radio_link_sync_owner = general.read_bool(
            "BuildingRadioLink.SyncOwner",
            globals.building_radio_link_sync_owner,
        );
        globals.ivan_icon_visibility = audio_visual
            .read_name("IvanIconVisibility", 0x20)
            .and_then(crate::rules::phobos_shields::AffectedHouse::parse)
            .unwrap_or(globals.ivan_icon_visibility);
        // `Ext/Rules/Body.cpp:517-518`, both `[General]` `Valueable<bool>`:
        // a human scanner keeps `no`, the computer's scanners keep `yes`.
        globals.auto_target_no_threat_buildings = general.read_bool(
            "AutoTarget.NoThreatBuildings",
            globals.auto_target_no_threat_buildings,
        );
        globals.auto_target_ai_no_threat_buildings = general.read_bool(
            "AutoTargetAI.NoThreatBuildings",
            globals.auto_target_ai_no_threat_buildings,
        );
        // The `ExtendedAircraftMissions` pair (`Ext/Rules/Body.cpp:191-192`).
        // The switch is a `Valueable<bool>`; UnlandDamage a `Valueable<int>`,
        // both read with their constructor value as the default.
        globals.extended_aircraft_missions = general.read_bool(
            "ExtendedAircraftMissions",
            globals.extended_aircraft_missions,
        );
        globals.extended_aircraft_missions_unland_damage = general.read_int(
            "ExtendedAircraftMissions.UnlandDamage",
            globals.extended_aircraft_missions_unland_damage,
        );
        // The two `Amphibious*` `Valueable<bool>` globals
        // (`Ext/Rules/Body.cpp:197-198`, constructor `Body.h:654-655`).
        globals.amphibious_enter = general.read_bool("AmphibiousEnter", globals.amphibious_enter);
        globals.amphibious_unload =
            general.read_bool("AmphibiousUnload", globals.amphibious_unload);
        // The three `[General] Strafing.*` `Valueable<bool>` fallbacks
        // (`Ext/Rules/Body.cpp:264-266`, constructor `Body.h:706-708`).
        globals.strafing_simulate_burst =
            general.read_bool("Strafing.SimulateBurst", globals.strafing_simulate_burst);
        globals.strafing_use_ammo_per_shot = general.read_bool(
            "Strafing.UseAmmoPerShot",
            globals.strafing_use_ammo_per_shot,
        );
        globals.strafing_target_cell =
            general.read_bool("Strafing.TargetCell", globals.strafing_target_cell);
        // `Ext/Rules/Body.cpp:535-539`: five `Valueable<double>` fallbacks,
        // all constructed `0.0` (`Ext/Rules/Body.h:952-956`).
        // `Valueable<double>` through `parser.ReadDouble` (the installed Ares
        // Parser<double>), not the stock `%f` ReadDouble.
        globals.extra_threat_is_threat =
            general.read_ares_double_bits("ExtraThreat.IsThreat", globals.extra_threat_is_threat);
        globals.extra_threat_in_range =
            general.read_ares_double_bits("ExtraThreat.InRange", globals.extra_threat_in_range);
        globals.extra_threat_coefficient_in_range_distance = general.read_ares_double_bits(
            "ExtraThreatCoefficient.InRangeDistance",
            globals.extra_threat_coefficient_in_range_distance,
        );
        globals.extra_threat_coefficient_facing = general.read_ares_double_bits(
            "ExtraThreatCoefficient.Facing",
            globals.extra_threat_coefficient_facing,
        );
        globals.extra_threat_coefficient_distance_to_last_target = general.read_ares_double_bits(
            "ExtraThreatCoefficient.DistanceToLastTarget",
            globals.extra_threat_coefficient_distance_to_last_target,
        );
        globals
    }

    /// `[General] ExtraThreat.IsThreat=`.
    pub fn extra_threat_is_threat(&self) -> f64 {
        f64::from_bits(self.extra_threat_is_threat.bits())
    }

    /// `[General] ExtraThreat.InRange=`.
    pub fn extra_threat_in_range(&self) -> f64 {
        f64::from_bits(self.extra_threat_in_range.bits())
    }

    /// `[General] ExtraThreatCoefficient.InRangeDistance=`.
    pub fn extra_threat_in_range_distance(&self) -> f64 {
        f64::from_bits(self.extra_threat_coefficient_in_range_distance.bits())
    }

    /// `[General] ExtraThreatCoefficient.Facing=`.
    pub fn extra_threat_coefficient_facing(&self) -> f64 {
        f64::from_bits(self.extra_threat_coefficient_facing.bits())
    }

    /// `[General] ExtraThreatCoefficient.DistanceToLastTarget=`.
    pub fn extra_threat_coefficient_distance_to_last_target(&self) -> f64 {
        f64::from_bits(self.extra_threat_coefficient_distance_to_last_target.bits())
    }

    /// `[CombatDamage] DamageOwnerMultiplier=`.
    pub fn damage_owner_multiplier(&self) -> f64 {
        f64::from_bits(self.damage_owner_multiplier.bits())
    }

    /// `[CombatDamage] DamageAlliesMultiplier=`.
    pub fn damage_allies_multiplier(&self) -> f64 {
        f64::from_bits(self.damage_allies_multiplier.bits())
    }

    /// `[CombatDamage] DamageEnemiesMultiplier=`.
    pub fn damage_enemies_multiplier(&self) -> f64 {
        f64::from_bits(self.damage_enemies_multiplier.bits())
    }

    /// `[CombatDamage] DamageOwnerMultiplier.NotAffectsEnemies=`.
    pub fn damage_owner_multiplier_not_affects_enemies(&self) -> Option<f64> {
        self.damage_owner_multiplier_not_affects_enemies
            .map(|value| f64::from_bits(value.bits()))
    }

    /// `[CombatDamage] DamageAlliesMultiplier.NotAffectsEnemies=`.
    pub fn damage_allies_multiplier_not_affects_enemies(&self) -> Option<f64> {
        self.damage_allies_multiplier_not_affects_enemies
            .map(|value| f64::from_bits(value.bits()))
    }

    /// `[CombatDamage] DamageOwnerMultiplier.Berzerk=`.
    pub fn damage_owner_multiplier_berzerk(&self) -> Option<f64> {
        self.damage_owner_multiplier_berzerk
            .map(|value| f64::from_bits(value.bits()))
    }

    /// `[CombatDamage] DamageAlliesMultiplier.Berzerk=`.
    pub fn damage_allies_multiplier_berzerk(&self) -> Option<f64> {
        self.damage_allies_multiplier_berzerk
            .map(|value| f64::from_bits(value.bits()))
    }

    /// `[CombatDamage] DamageEnemiesMultiplier.Berzerk=`.
    pub fn damage_enemies_multiplier_berzerk(&self) -> Option<f64> {
        self.damage_enemies_multiplier_berzerk
            .map(|value| f64::from_bits(value.bits()))
    }

    /// `[General] VeteranReload=`.
    pub fn veteran_reload(&self) -> f64 {
        f64::from_bits(self.veteran_reload.bits())
    }

    /// `[General] VeteranEmptyReload=`.
    pub fn veteran_empty_reload(&self) -> Option<f64> {
        self.veteran_empty_reload
            .map(|value| f64::from_bits(value.bits()))
    }
}

fn section_settings(section: &crate::rules::ini_parser::IniSection) -> PhobosGlobals {
    let defaults = PhobosGlobals::default();
    PhobosGlobals {
        shrapnel_affects_ground: section
            .read_bool("Shrapnel.AffectsGround", defaults.shrapnel_affects_ground),
        shrapnel_affects_buildings: section.read_bool(
            "Shrapnel.AffectsBuildings",
            defaults.shrapnel_affects_buildings,
        ),
        auto_death_technos_exist_allow_limboed: section.read_bool(
            "AutoDeath.TechnosExist.AllowLimboed",
            defaults.auto_death_technos_exist_allow_limboed,
        ),
        auto_death_technos_dont_exist_allow_limboed: section.read_bool(
            "AutoDeath.TechnosDontExist.AllowLimboed",
            defaults.auto_death_technos_dont_exist_allow_limboed,
        ),
        shrapnel_ignore_hit_buildings: section.read_bool(
            "Shrapnel.IgnoreHitBuildings",
            defaults.shrapnel_ignore_hit_buildings,
        ),
        shrapnel_use_weapon_targeting: section.read_bool(
            "Shrapnel.UseWeaponTargeting",
            defaults.shrapnel_use_weapon_targeting,
        ),
        shrapnel_obey_warhead_trigger_conditions: section.read_bool(
            "Shrapnel.ObeyWarheadTriggerConditions",
            defaults.shrapnel_obey_warhead_trigger_conditions,
        ),
        projectile_interceptable: section
            .read_bool("ProjectileInterceptable", defaults.projectile_interceptable),
        interceptor_guard_range_is_cylindrical: section.read_bool(
            "Interceptor.GuardRange.IsCylindrical",
            defaults.interceptor_guard_range_is_cylindrical,
        ),
        allow_weapon_select_against_walls: section.read_bool(
            "AllowWeaponSelectAgainstWalls",
            defaults.allow_weapon_select_against_walls,
        ),
        interceptor_apply_firepower_mult: section.read_bool(
            "Interceptor.ApplyFirepowerMult",
            defaults.interceptor_apply_firepower_mult,
        ),
        return_weapon_apply_firepower_mult: section.read_bool(
            "ReturnWeapon.ApplyFirepowerMult",
            defaults.return_weapon_apply_firepower_mult,
        ),
        can_target_iron_curtained: section.read_bool(
            "CanTarget.IronCurtained",
            defaults.can_target_iron_curtained,
        ),
        arcing_allow_elevation_inaccuracy: section.read_bool(
            "Arcing.AllowElevationInaccuracy",
            defaults.arcing_allow_elevation_inaccuracy,
        ),
        temporal_apply_multiplier: section.read_bool(
            "Temporal.ApplyMultiplier",
            defaults.temporal_apply_multiplier,
        ),
        temporal_apply_versus: section
            .read_bool("Temporal.ApplyVersus", defaults.temporal_apply_versus),
        can_target_ai_iron_curtained: section.read_bool(
            "CanTargetAI.IronCurtained",
            defaults.can_target_ai_iron_curtained,
        ),
        auto_target_iron_curtained: section.read_bool(
            "AutoTarget.IronCurtained",
            defaults.auto_target_iron_curtained,
        ),
        berzerk_targeting: crate::rules::phobos_shields::read_affected_house(
            section,
            "BerzerkTargeting",
            defaults.berzerk_targeting,
        ),
        overload_particle_sys_count: section.read_int(
            "Overload.ParticleSysCount",
            defaults.overload_particle_sys_count,
        ),
        affects_invoker_only_ignore_invoker_state: section.read_bool(
            "AffectsInvokerOnly.IgnoreInvokerState",
            defaults.affects_invoker_only_ignore_invoker_state,
        ),
        auto_death_on_owner_change_ignore_revert_on_exit: section.read_bool(
            "AutoDeath.OnOwnerChange.IgnoreRevertOnExit",
            defaults.auto_death_on_owner_change_ignore_revert_on_exit,
        ),
        auto_death_allow_limboed: section
            .read_bool("AutoDeath.AllowLimboed", defaults.auto_death_allow_limboed),
        penetrates_transport_level: section.read_ares_int(
            "PenetratesTransport.Level",
            defaults.penetrates_transport_level,
        ),
        driver_killed_kept_passengers: section.read_bool(
            "DriverKilled.KeptPassengers",
            defaults.driver_killed_kept_passengers,
        ),
        parasite_disable_particle_system: section.read_bool(
            "Parasite.DisableParticleSystem",
            defaults.parasite_disable_particle_system,
        ),
        open_transport_range_bonus: section.read_int(
            "OpenTransport.RangeBonus",
            defaults.open_transport_range_bonus,
        ),
        open_transport_damage_multiplier: crate::util::native_x87::NativeF32Bits::from_bits(
            section
                .read_float(
                    "OpenTransport.DamageMultiplier",
                    f32::from_bits(defaults.open_transport_damage_multiplier.bits()),
                )
                .to_bits(),
        ),
        driver_killed_kill_passengers: section.read_bool(
            "DriverKilled.KillPassengers",
            defaults.driver_killed_kill_passengers,
        ),
        // `Ext/Rules/Body.cpp:340-347`: the eight `[CombatDamage]` damage
        // multipliers, read in this order. The three plain values are
        // `Valueable<double>` 1.0; the five nullable columns stay unset until
        // a layer names them.
        damage_owner_multiplier: read_double_bits(
            section,
            "DamageOwnerMultiplier",
            defaults.damage_owner_multiplier,
        ),
        damage_allies_multiplier: read_double_bits(
            section,
            "DamageAlliesMultiplier",
            defaults.damage_allies_multiplier,
        ),
        damage_enemies_multiplier: read_double_bits(
            section,
            "DamageEnemiesMultiplier",
            defaults.damage_enemies_multiplier,
        ),
        damage_owner_multiplier_not_affects_enemies: read_nullable_double_bits(
            section,
            "DamageOwnerMultiplier.NotAffectsEnemies",
            defaults.damage_owner_multiplier_not_affects_enemies,
        ),
        damage_allies_multiplier_not_affects_enemies: read_nullable_double_bits(
            section,
            "DamageAlliesMultiplier.NotAffectsEnemies",
            defaults.damage_allies_multiplier_not_affects_enemies,
        ),
        damage_owner_multiplier_berzerk: read_nullable_double_bits(
            section,
            "DamageOwnerMultiplier.Berzerk",
            defaults.damage_owner_multiplier_berzerk,
        ),
        damage_allies_multiplier_berzerk: read_nullable_double_bits(
            section,
            "DamageAlliesMultiplier.Berzerk",
            defaults.damage_allies_multiplier_berzerk,
        ),
        damage_enemies_multiplier_berzerk: read_nullable_double_bits(
            section,
            "DamageEnemiesMultiplier.Berzerk",
            defaults.damage_enemies_multiplier_berzerk,
        ),
        // `Ext/Rules/Body.cpp:503`: the merge switch, a `Valueable<bool>`
        // default `no`.
        merge_building_damage: section
            .read_bool("MergeBuildingDamage", defaults.merge_building_damage),
        // The `[AudioVisual]` block belongs to `from_ini`; a `CombatDamage`
        // section leaves these at their constructor values.
        ..defaults
    }
}

#[cfg(test)]
#[path = "phobos_globals_tests.rs"]
mod tests;
