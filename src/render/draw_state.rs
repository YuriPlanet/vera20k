//! Per-object tactical draw state shared by SHP and voxel instance builders.
//!
//! The YR draw path resolves visibility, translucency selection, effect brightness,
//! and house remap before choosing an SHP or voxel rasterizer. Keep that decision in
//! one CPU descriptor so the two atlas paths cannot disagree.

use crate::sim::game_entity::GameEntity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ObserverDrawContext {
    /// The observer's house considers the object's real owner allied.
    pub owner_is_allied: bool,
    /// Positive sensor/detection result supplied by observer gameplay state.
    pub detects_cloak: bool,
}

/// `fx_flags` bit assignments consumed by the sprite shaders.
pub const FX_CLOAK: u32 = 1 << 0;
/// Explicit residual: no dedicated YR EMP material mutation is proven.
pub const FX_EMP: u32 = 1 << 1;
pub const FX_INVULNERABILITY: u32 = 1 << 2;
pub const FX_WARP: u32 = 1 << 3;
/// Explicit residual: no dedicated YR mirror material mutation is proven.
pub const FX_MIRROR: u32 = 1 << 4;
pub const FX_DISGUISE: u32 = 1 << 5;
/// The instance is a ground shadow stencil (`VxlLayer::Shadow`): the voxel
/// sprite shader ignores the palette and darkens whatever is beneath, the way
/// the native shadow blitter (`Blitter_selector(0x2001)`) does.
pub const FX_SHADOW: u32 = 1 << 6;

/// Native cloak state values consumed by YR draw selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloakDrawPhase {
    Cloaking,
    FullyCloaked,
    Uncloaking,
}

/// Authoritative producer values needed by YR cloak draw selection.
///
/// The simulation does not yet own these fields. Keeping the input separate avoids
/// reconstructing gameplay state from render flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloakDrawInput {
    pub phase: CloakDrawPhase,
    pub depth: u32,
    pub cloaking_stages: u32,
    pub late_visible: bool,
    pub force_visible_call: bool,
    pub visible_to_observer: bool,
}

/// Authoritative producer values needed by YR disguise shimmer selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisguiseDrawInput {
    pub active: bool,
    pub observer_is_allied: bool,
    /// Native timestamp field used by `GetDisguiseFlags`.
    pub start_frame: u32,
}

/// The YR invulnerability state consumed by
/// `TechnoClass::GetInvulnerabilityTintIntensity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvulnerabilityTintInput {
    pub mode: u8,
    pub elapsed_ticks: u32,
    /// Current object/cell brightness in the native 0..2000 scale.
    pub intensity: u32,
}

/// Producer-owned inputs to the common YR object draw resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrawStateInput {
    pub cloak: Option<CloakDrawInput>,
    pub disguise: Option<DisguiseDrawInput>,
    pub warp_out: bool,
    pub warp_in: bool,
    pub invulnerability: Option<InvulnerabilityTintInput>,
}

/// CPU draw admission plus the GPU-ready material state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawDecision {
    /// A fully cloaked object that is hidden from the observer emits no base draw.
    pub visible: bool,
    pub state: DrawState,
}

/// GPU-ready visual state resolved from one tactical object.
///
/// `remap_row` is the palette-ramp selection. `fx_params.x` is the final alpha
/// multiplier selected by the native translucency bits; `fx_params.y` preserves
/// those selector bits for diagnostics; `fx_params.z` is the 0..2 brightness scalar;
/// `fx_params.w` optionally overrides the zdepth-atlas scale, with zero retaining
/// the terrain default.
/// For voxel unit bodies carrying the composite bridge-split flag instead,
/// `fx_params.w` carries the tactical scissor height in world pixels (zero
/// means full viewport). This shader-specific transport does not alter effects.
/// `effect_tint` carries the scalar as RGB so SHP and voxel shaders apply the same
/// native brightness channel after their normal palette/light work. The layout is
/// part of `SpriteInstance`'s vertex ABI.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawState {
    pub remap_row: u32,
    pub fx_flags: u32,
    pub fx_params: [f32; 4],
    pub effect_tint: [f32; 4],
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            remap_row: 0,
            fx_flags: 0,
            fx_params: [1.0, 0.0, 1.0, 0.0],
            effect_tint: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

impl DrawState {
    /// Resolve YR object draw state without inferring any producer-owned gameplay state.
    ///
    /// Original locations: `TechnoClass::DrawVoxel @ 0x00706640`,
    /// `TechnoClass::GetInvulnerabilityTintIntensity @ 0x0070e380`, and
    /// `TeleportLocomotionClass::ILocomotion::Process @ 0x007192f0`.
    pub fn resolve(input: DrawStateInput, current_frame: u32, remap_row: u32) -> DrawDecision {
        let mut state = Self {
            remap_row,
            fx_params: [1.0, 0.0, 1.0, 0.0],
            effect_tint: [1.0, 1.0, 1.0, 1.0],
            ..Self::default()
        };

        let Some((cloak_selector, cloak_flag)) = cloak_selector(input.cloak) else {
            return DrawDecision {
                visible: false,
                state,
            };
        };

        let warp_selector = if input.warp_out || input.warp_in {
            state.fx_flags |= FX_WARP;
            TRANSLUCENCY_50
        } else {
            0
        };
        let disguise_selector = input
            .disguise
            .filter(|disguise| disguise.active && disguise.observer_is_allied)
            .map(|disguise| {
                let phase = current_frame
                    .wrapping_sub(disguise.start_frame)
                    .wrapping_add(64)
                    % 256;
                selector_bits_for_percent(disguise_phase_percent(phase))
            })
            .unwrap_or(0);

        if disguise_selector != 0 {
            state.fx_flags |= FX_DISGUISE;
        }

        if cloak_flag {
            state.fx_flags |= FX_CLOAK;
        }
        let selector_bits = cloak_selector | warp_selector | disguise_selector;
        state.fx_params[0] = opacity_for_selector_bits(selector_bits);
        state.fx_params[1] = selector_bits as f32;

        if let Some(invulnerability) = input.invulnerability {
            state.fx_flags |= FX_INVULNERABILITY;
            let brightness = invulnerability_tint_intensity(
                invulnerability.mode,
                invulnerability.elapsed_ticks,
                invulnerability.intensity,
            ) as f32
                / 1000.0;
            state.fx_params[2] = brightness;
            state.effect_tint = [brightness, brightness, brightness, 1.0];
        }

        DrawDecision {
            visible: true,
            state,
        }
    }

    /// Adapt simulation-owned producers without inventing missing native fields.
    pub fn for_entity(
        entity: &GameEntity,
        current_frame: u32,
        remap_row: u32,
        observer: ObserverDrawContext,
    ) -> DrawDecision {
        let (warp_out, warp_in) = entity
            .teleport_state
            .as_ref()
            .map(|teleport| (teleport.warp_out_active(), teleport.warp_in_active()))
            .unwrap_or_default();
        Self::resolve(
            DrawStateInput {
                cloak: entity.cloak.as_ref().and_then(|cloak| {
                    let phase = match cloak.visual_phase? {
                        crate::sim::cloak_disguise::CloakVisualPhase::Cloaking => {
                            CloakDrawPhase::Cloaking
                        }
                        crate::sim::cloak_disguise::CloakVisualPhase::FullyCloaked => {
                            CloakDrawPhase::FullyCloaked
                        }
                        crate::sim::cloak_disguise::CloakVisualPhase::Uncloaking => {
                            CloakDrawPhase::Uncloaking
                        }
                    };
                    Some(CloakDrawInput {
                        phase,
                        depth: cloak.depth,
                        cloaking_stages: cloak.cloaking_stages,
                        late_visible: cloak.late_visible,
                        force_visible_call: cloak.force_visible_call,
                        visible_to_observer: observer.owner_is_allied || observer.detects_cloak,
                    })
                }),
                disguise: entity.disguise.as_ref().map(|disguise| DisguiseDrawInput {
                    active: disguise.disguised,
                    observer_is_allied: observer.owner_is_allied,
                    start_frame: disguise.disguise_creation_frame,
                }),
                warp_out,
                warp_in,
                // The current invulnerability producer has no authoritative YR
                // +0x1A4 mode byte, so tint remains an explicit residual.
                invulnerability: None,
                ..DrawStateInput::default()
            },
            current_frame,
            remap_row,
        )
    }
}

const TRANSLUCENCY_25: u8 = 0b010;
const TRANSLUCENCY_50: u8 = 0b100;
const TRANSLUCENCY_75: u8 = TRANSLUCENCY_25 | TRANSLUCENCY_50;

/// `GetInvulnerabilityTintIntensity @ 0x0070e380`.
pub fn invulnerability_tint_intensity(mode: u8, elapsed_ticks: u32, intensity: u32) -> u32 {
    let t = elapsed_ticks as i64;
    let scale = match mode {
        1 => (12 - t) * 256 / 6,
        2 | 8 => 512,
        // Modes 3..5 are retained from the closed formula but are not asserted as
        // exact across every native call path because the RE evidence records minor
        // divider variance there.
        3 => (461 * t + 1020) / 10,
        4 => (1024 - 77 * t) / 8,
        5 => (77 * t + 816) / 16,
        6 => 51,
        7 => (3072 - 461 * t) / 6,
        9 => (t + 20) * 256 / 20,
        _ => return intensity,
    };
    ((intensity as i64 * scale) >> 8).min(2000).max(0) as u32
}

/// `GetDisguiseFlags @ 0x0070ed80`'s 256-frame shimmer leaf.
pub fn disguise_phase_percent(phase: u32) -> u8 {
    match phase % 256 {
        64..=67 | 76..=79 | 112..=115 | 124..=127 => 25,
        68..=75 | 116..=123 => 50,
        _ => 0,
    }
}

/// `TechnoClass::VisualCharacter @ 0x00703860`'s transitional cloak phase.
pub fn cloak_phase_256(depth: u32, cloaking_stages: u32) -> u32 {
    depth.saturating_mul(256) / cloaking_stages.max(1)
}

/// `TechnoClass::VisualCharacter @ 0x00703860`'s drawn transitional band.
pub fn cloak_visual_character(
    depth: u32,
    cloaking_stages: u32,
    late_visible: bool,
    force_visible_call: bool,
) -> u8 {
    if depth == 0 {
        return 0;
    }
    match cloak_phase_256(depth, cloaking_stages) {
        0..=63 => 1,
        64..=127 => 2,
        128..=191 => 3,
        192..=254 if late_visible && !force_visible_call => 3,
        192..=254 => 4,
        _ => 5,
    }
}

fn cloak_selector(cloak: Option<CloakDrawInput>) -> Option<(u8, bool)> {
    let Some(cloak) = cloak else {
        return Some((0, false));
    };
    if cloak.phase == CloakDrawPhase::FullyCloaked && !cloak.visible_to_observer {
        return None;
    }
    if cloak.phase == CloakDrawPhase::FullyCloaked || cloak.depth == 0 {
        return Some((0, false));
    }

    let visual_character = cloak_visual_character(
        cloak.depth,
        cloak.cloaking_stages,
        cloak.late_visible,
        cloak.force_visible_call,
    );
    match visual_character {
        1 => Some((TRANSLUCENCY_25, true)),
        2..=4 => Some((TRANSLUCENCY_50, true)),
        _ => None,
    }
}

fn selector_bits_for_percent(percent: u8) -> u8 {
    match percent {
        25 => TRANSLUCENCY_25,
        50 => TRANSLUCENCY_50,
        _ => 0,
    }
}

fn opacity_for_selector_bits(bits: u8) -> f32 {
    match bits & (TRANSLUCENCY_25 | TRANSLUCENCY_50) {
        TRANSLUCENCY_25 => 0.75,
        TRANSLUCENCY_50 => 0.5,
        TRANSLUCENCY_75 => 0.25,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::InternedId;
    use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};

    fn entity() -> GameEntity {
        GameEntity::new_at_frame_zero_for_test(
            1,
            0,
            0,
            0,
            0,
            InternedId::from_index(0),
            Health { current: 100 },
            InternedId::from_index(0),
            EntityCategory::Unit,
            0,
            1,
            true,
        )
    }

    #[test]
    fn invulnerability_formula_matches_locked_yr_vectors() {
        assert_eq!(invulnerability_tint_intensity(6, 0, 1000), 199);
        assert_eq!(invulnerability_tint_intensity(2, 5, 1000), 2000);
        assert_eq!(invulnerability_tint_intensity(7, 2, 1000), 1398);
        assert_eq!(invulnerability_tint_intensity(1, 0, 1000), 2000);
        assert_eq!(invulnerability_tint_intensity(9, 0, 1000), 1000);
        assert_eq!(invulnerability_tint_intensity(0, 0, 777), 777);
    }

    #[test]
    fn disguise_formula_matches_locked_yr_vectors() {
        for (phase, expected) in [
            (0, 0),
            (64, 25),
            (70, 50),
            (78, 25),
            (100, 0),
            (120, 50),
            (126, 25),
            (200, 0),
        ] {
            assert_eq!(disguise_phase_percent(phase), expected);
        }
    }

    #[test]
    fn cloak_ramp_matches_locked_yr_vectors() {
        assert_eq!(cloak_phase_256(1, 9), 28);
        assert_eq!(cloak_phase_256(3, 9), 85);
        assert_eq!(cloak_phase_256(8, 9), 227);
        assert_eq!(cloak_visual_character(0, 9, false, false), 0);
        assert_eq!(cloak_visual_character(1, 9, false, false), 1);
        assert_eq!(cloak_visual_character(3, 9, false, false), 2);
        assert_eq!(cloak_visual_character(5, 9, false, false), 3);
        assert_eq!(cloak_visual_character(8, 9, false, false), 4);
        assert_eq!(cloak_visual_character(9, 9, false, false), 5);
        assert_eq!(cloak_visual_character(8, 9, true, false), 3);
        assert_eq!(cloak_visual_character(8, 9, true, true), 4);
    }

    #[test]
    fn fully_hidden_cloak_suppresses_the_base_draw() {
        let decision = DrawState::resolve(
            DrawStateInput {
                cloak: Some(CloakDrawInput {
                    phase: CloakDrawPhase::FullyCloaked,
                    depth: 9,
                    cloaking_stages: 9,
                    late_visible: false,
                    force_visible_call: false,
                    visible_to_observer: false,
                }),
                ..DrawStateInput::default()
            },
            0,
            3,
        );
        assert!(!decision.visible);
    }

    #[test]
    fn visible_cloak_warp_disguise_tint_and_remap_stay_independent() {
        let decision = DrawState::resolve(
            DrawStateInput {
                cloak: Some(CloakDrawInput {
                    phase: CloakDrawPhase::Cloaking,
                    depth: 1,
                    cloaking_stages: 9,
                    late_visible: false,
                    force_visible_call: false,
                    visible_to_observer: true,
                }),
                disguise: Some(DisguiseDrawInput {
                    active: true,
                    observer_is_allied: true,
                    start_frame: 0,
                }),
                warp_out: true,
                invulnerability: Some(InvulnerabilityTintInput {
                    mode: 6,
                    elapsed_ticks: 0,
                    intensity: 1000,
                }),
                ..DrawStateInput::default()
            },
            0,
            7,
        );

        assert!(decision.visible);
        assert_eq!(decision.state.remap_row, 7);
        assert_eq!(
            decision.state.fx_flags,
            FX_CLOAK | FX_WARP | FX_DISGUISE | FX_INVULNERABILITY
        );
        assert_eq!(decision.state.fx_params[0], 0.25);
        assert_eq!(decision.state.fx_params[1], 6.0);
        assert_eq!(decision.state.effect_tint, [0.199, 0.199, 0.199, 1.0]);
    }

    #[test]
    fn teleport_adapter_uses_distinct_producer_flags() {
        let mut entity = entity();
        entity.teleport_state = Some(TeleportState {
            phase: TeleportPhase::ChronoDelay,
            target_rx: 1,
            target_ry: 2,
            being_warped_ticks: 4,
        });
        let state = DrawState::for_entity(&entity, 45, 3, ObserverDrawContext::default()).state;
        assert_eq!(state.fx_flags, FX_WARP);
        assert_eq!(state.fx_params[0], 0.5);
    }

    #[test]
    fn invulnerability_without_native_mode_remains_an_explicit_residual() {
        let mut entity = entity();
        entity.invulnerability = Some(InvulnerabilityState {
            start_frame: 40,
            duration_frames: 20,
            kind: InvulnKind::IronCurtain,
        });
        let state = DrawState::for_entity(&entity, 45, 3, ObserverDrawContext::default()).state;
        assert_eq!(state.fx_flags, 0);
        assert_eq!(state.fx_params, [1.0, 0.0, 1.0, 0.0]);
        assert_eq!(state.effect_tint, [1.0; 4]);
    }

    #[test]
    fn expired_invulnerability_keeps_normal_draw_state() {
        let mut entity = entity();
        entity.invulnerability = Some(InvulnerabilityState {
            start_frame: 40,
            duration_frames: 5,
            kind: InvulnKind::ForceShield,
        });
        let state = DrawState::for_entity(&entity, 45, 2, ObserverDrawContext::default()).state;
        assert_eq!(state.fx_flags, 0);
        assert_eq!(state.effect_tint, [1.0; 4]);
    }

    #[test]
    fn gsi_13_09_zdepth_shader_reads_the_zdata_sign_from_fx_params_w() {
        // fx_params.w carries the Z-data sign (+1 tiles, -1 bridges); the
        // row adjustment remains in native units. Actual TMP/SHP/VXL GPU
        // regressions cover the shared u16 storage and depth admission.
        let shader = include_str!("zdepth_shader.wgsl");
        assert!(shader.contains("@location(9) fx_params: vec4f"));
        assert!(shader.contains("@location(11) z_adjust: f32"));
        assert!(shader.contains("select(1.0, -1.0, instance.fx_params.w < 0.0)"));
        assert!(shader.contains("input.canvas_top - (input.z_adjust + input.z_sign * z_byte)"));
        assert!(!shader.contains("0.0002"));
    }
}
