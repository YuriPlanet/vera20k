//! Per-object tactical draw state shared by SHP and voxel instance builders.
//!
//! The YR draw path resolves visibility, translucency selection and house remap
//! before choosing an SHP or voxel rasterizer. Keep that decision in
//! one CPU descriptor so the two atlas paths cannot disagree.

use crate::sim::game_entity::GameEntity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObserverDrawContext {
    /// The observer's house considers the object's real owner allied.
    pub owner_is_allied: bool,
    /// Positive sensor/detection result supplied by observer gameplay state.
    pub detects_cloak: bool,
    /// Screen-only fully-cloaked allowance requires both alliance directions.
    pub owner_is_mutually_allied: bool,
    pub observer_present: bool,
    /// Current production Rules value, not a second retained cloak clock.
    pub cloaking_stages: i32,
    pub invisible: bool,
    pub is_campaign: bool,
    /// Original House50B6F0, distinct from either alliance direction.
    pub owner_is_current_player: bool,
    /// Object+1BC returned a Cell for the disguise observer query.
    pub disguise_cell_present: bool,
    /// Original Cell4870F0 for this viewer at that Cell.
    pub detects_disguise: bool,
    /// The selected body type's encoding; screen-pick callers leave it unset.
    pub drawn_voxel: Option<bool>,
}

impl Default for ObserverDrawContext {
    fn default() -> Self {
        Self {
            owner_is_allied: false,
            detects_cloak: false,
            owner_is_mutually_allied: false,
            observer_present: false,
            cloaking_stages: crate::rules::ruleset::GeneralRules::default().cloaking_stages,
            invisible: false,
            is_campaign: false,
            owner_is_current_player: false,
            disguise_cell_present: false,
            detects_disguise: false,
            drawn_voxel: None,
        }
    }
}

/// `fx_flags` bit assignments consumed by the sprite shaders.
pub const FX_CLOAK: u32 = 1 << 0;
/// Explicit residual: no dedicated YR EMP material mutation is proven.
pub const FX_EMP: u32 = 1 << 1;
pub const FX_WARP: u32 = 1 << 3;
/// Explicit residual: no dedicated YR mirror material mutation is proven.
pub const FX_MIRROR: u32 = 1 << 4;
pub const FX_DISGUISE: u32 = 1 << 5;
/// The instance is a ground shadow stencil (`VxlLayer::Shadow`): the voxel
/// sprite shader ignores the palette and darkens whatever is beneath, the way
/// the native shadow blitter (`Blitter_selector(0x2001)`) does.
pub const FX_SHADOW: u32 = 1 << 6;
/// Voxel body waterline clip. `sinking_row` carries Techno+3CA's retained
/// world row; ordinary alpha remains in its own lane.
pub(crate) const FX_SINKING_CLIP: u32 = 1 << 7;

/// Resolved native visual character; the simulation owns the query and clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloakDrawInput {
    pub character: u8,
    pub progress: i32,
    pub native_offset_words: i32,
    pub voxel: bool,
}

/// Authoritative producer values needed by YR disguise shimmer selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisguiseDrawInput {
    pub active: bool,
    pub owner_is_current_player: bool,
    pub cell_present: bool,
    pub detected: bool,
    /// Techno+1EC/+1F4, not the re-disguise timer at+1E0/+1E8.
    pub blink_active: bool,
    /// Native timestamp field used by `GetDisguiseFlags`.
    pub start_frame: u32,
}

impl DisguiseDrawInput {
    /// Techno70EE30: shared Unit/Infantry real-type observer decision.
    /// The signed remainder is significant across the native DWORD boundary.
    pub(crate) fn shows_real_type(self, current_frame: u32) -> bool {
        if self.blink_active && !self.owner_is_current_player {
            return true;
        }
        if self.owner_is_current_player && self.active {
            return !(72..120).contains(&self.phase(current_frame));
        }
        !self.active || !self.cell_present || self.detected
    }

    fn phase(self, current_frame: u32) -> i32 {
        current_frame
            .wrapping_sub(self.start_frame)
            .wrapping_add(64) as i32
            % 256
    }

    /// Techno70ED80's added flags. Its caller separately admits only the
    /// current player's active disguise at7062F5..70631D.
    fn selector_bits(self, current_frame: u32) -> u8 {
        if self.blink_active && !self.owner_is_current_player {
            0
        } else {
            selector_bits_for_percent(disguise_phase_percent(self.phase(current_frame)))
        }
    }
}

/// Producer-owned inputs to the common YR object draw resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrawStateInput {
    pub cloak: Option<CloakDrawInput>,
    pub disguise: Option<DisguiseDrawInput>,
    pub warp_out: bool,
    pub warp_in: bool,
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
/// those selector bits for diagnostics; `fx_params.z` is unused (1.0);
/// `fx_params.w` optionally overrides the zdepth-atlas scale, with zero retaining
/// the terrain default.
/// For voxel unit bodies carrying the composite bridge-split flag instead,
/// `fx_params.w` carries the tactical scissor height in world pixels (zero
/// means full viewport). This shader-specific transport does not alter effects.
/// `sinking_row` is the retained world row a voxel draw with FX_SINKING_CLIP
/// clips at. An effect that changes an object's light, such as the Iron
/// Curtain's tint, scales the intensity its `PaletteLight` carries, as the
/// native draws scale theirs.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawState {
    pub remap_row: u32,
    pub fx_flags: u32,
    pub fx_params: [f32; 4],
    pub sinking_row: f32,
    /// Techno70BE50 signed packed-surface displacement, derived from GameEntity.
    pub native_offset_words: i32,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            remap_row: 0,
            fx_flags: 0,
            fx_params: [1.0, 0.0, 1.0, 0.0],
            sinking_row: 0.0,
            native_offset_words: 0,
        }
    }
}

impl DrawState {
    /// Original Convert selector bits, shared by lowering and the compositor.
    pub(crate) fn native_selector_bits(&self) -> u32 {
        self.fx_params[1] as u32
    }

    /// Resolve YR object draw state without inferring any producer-owned gameplay state.
    ///
    /// Original locations: `TechnoClass::DrawVoxel @ 0x00706640` and
    /// `TeleportLocomotionClass::ILocomotion::Process @ 0x007192f0`.
    pub fn resolve(input: DrawStateInput, current_frame: u32, remap_row: u32) -> DrawDecision {
        let mut state = Self {
            remap_row,
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
            .filter(|disguise| disguise.active && disguise.owner_is_current_player)
            .map(|disguise| disguise.selector_bits(current_frame))
            .unwrap_or(0);

        if disguise_selector != 0 {
            state.fx_flags |= FX_DISGUISE;
        }

        if cloak_flag {
            state.fx_flags |= FX_CLOAK;
            state.native_offset_words = input.cloak.map_or(0, |cloak| cloak.native_offset_words);
        }
        let selector_bits = cloak_selector | warp_selector | disguise_selector;
        state.fx_params[0] = opacity_for_selector_bits(selector_bits);
        state.fx_params[1] = selector_bits as f32;

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
        let (warp_out, warp_in) = (entity.is_warped_out(), entity.is_warping_in());
        let character = entity.visual_character(
            observer.cloaking_stages,
            observer.invisible,
            crate::sim::cloak_disguise::VisualCharacterQuery::screen(
                true,
                observer.observer_present,
                observer.detects_cloak,
                observer.owner_is_mutually_allied,
                observer.is_campaign,
                false,
            ),
        );
        let voxel = observer.drawn_voxel.unwrap_or(entity.is_voxel);
        Self::resolve(
            DrawStateInput {
                cloak: Some(CloakDrawInput {
                    character,
                    progress: entity.cloak.as_ref().map_or(0, |cloak| cloak.depth as i32),
                    native_offset_words: if character == 4 && voxel {
                        entity.native_cloak_offset_words()
                    } else {
                        0
                    },
                    voxel,
                }),
                disguise: Self::disguise_input(entity, observer),
                warp_out,
                warp_in,
            },
            current_frame,
            remap_row,
        )
    }

    pub(crate) fn disguise_input(
        entity: &GameEntity,
        observer: ObserverDrawContext,
    ) -> Option<DisguiseDrawInput> {
        entity.disguise.as_ref().map(|disguise| DisguiseDrawInput {
            active: disguise.is_disguised(),
            owner_is_current_player: observer.owner_is_current_player,
            cell_present: observer.disguise_cell_present,
            detected: observer.detects_disguise,
            // The separate Techno blink producer is not retained yet. The
            // stationary stock Mirage path starts with an inactive timer;
            // do not alias its gameplay re-disguise timer to this input.
            blink_active: false,
            start_frame: disguise.creation_frame(),
        })
    }

    pub(crate) fn draws_disguise(
        entity: &GameEntity,
        current_frame: u32,
        observer: ObserverDrawContext,
    ) -> bool {
        Self::disguise_input(entity, observer)
            .is_some_and(|input| !input.shows_real_type(current_frame))
    }
}

const TRANSLUCENCY_25: u8 = 0b010;
const TRANSLUCENCY_50: u8 = 0b100;
const TRANSLUCENCY_75: u8 = TRANSLUCENCY_25 | TRANSLUCENCY_50;

/// `GetDisguiseFlags @ 0x0070ed80`'s 256-frame shimmer leaf.
pub fn disguise_phase_percent(phase: i32) -> u8 {
    match phase % 256 {
        64..=67 | 76..=79 | 112..=115 | 124..=127 => 25,
        68..=75 | 116..=123 => 50,
        _ => 0,
    }
}

fn cloak_selector(cloak: Option<CloakDrawInput>) -> Option<(u8, bool)> {
    let Some(cloak) = cloak else {
        return Some((0, false));
    };
    // Unit73B21F..73B259/Techno706640 consume the query, whereas SHP705E45
    // does not add the displacement bit. See the executed selector corpus in
    // tools/procedural_drawing_oracle/translucent_blitter_a.md.
    match cloak.character {
        0 => Some((0, false)),
        1 => Some((TRANSLUCENCY_25, true)),
        2 | 3 => Some((TRANSLUCENCY_50, true)),
        4 => Some((
            if cloak.progress == 0 {
                TRANSLUCENCY_25
            } else {
                TRANSLUCENCY_50
            } | if cloak.voxel { 8 } else { 0 },
            true,
        )),
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
    fn cloak_material_selection_matches_executed_original_unit_controls() {
        let original: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/translucent_blitter_a.json",
        ))
        .unwrap();
        for control in original["cloak_transition"].as_array().unwrap() {
            let character = control["character"].as_u64().unwrap() as u8;
            let decision = DrawState::resolve(
                DrawStateInput {
                    cloak: Some(CloakDrawInput {
                        character,
                        progress: control["depth"].as_i64().unwrap() as i32,
                        native_offset_words: control["offset_words"].as_i64().unwrap() as i32,
                        voxel: true,
                    }),
                    ..DrawStateInput::default()
                },
                0,
                0,
            );
            assert_eq!(decision.visible, character != 5, "{control}");
            if decision.visible {
                assert_eq!(
                    decision.state.native_selector_bits(),
                    control["final_flags"].as_u64().unwrap() as u32 & 0xEu32,
                    "{control}"
                );
            }
        }
    }

    #[test]
    fn shp_character_four_does_not_use_voxel_neighbor_material() {
        for (progress, expected) in [(0, 2), (1, 4)] {
            let decision = DrawState::resolve(
                DrawStateInput {
                    cloak: Some(CloakDrawInput {
                        character: 4,
                        progress,
                        native_offset_words: -1,
                        voxel: false,
                    }),
                    ..DrawStateInput::default()
                },
                0,
                0,
            );
            assert_eq!(decision.state.native_selector_bits(), expected);
        }
    }

    #[test]
    fn fully_hidden_cloak_suppresses_the_base_draw() {
        let decision = DrawState::resolve(
            DrawStateInput {
                cloak: Some(CloakDrawInput {
                    character: 5,
                    progress: 9,
                    native_offset_words: 0,
                    voxel: true,
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
                    character: 1,
                    progress: 1,
                    native_offset_words: 0,
                    voxel: true,
                }),
                disguise: Some(DisguiseDrawInput {
                    active: true,
                    owner_is_current_player: true,
                    cell_present: true,
                    detected: false,
                    blink_active: false,
                    start_frame: 0,
                }),
                warp_out: true,
                ..DrawStateInput::default()
            },
            0,
            7,
        );

        assert!(decision.visible);
        assert_eq!(decision.state.remap_row, 7);
        assert_eq!(decision.state.fx_flags, FX_CLOAK | FX_WARP | FX_DISGUISE);
        assert_eq!(decision.state.fx_params[0], 0.25);
        assert_eq!(decision.state.fx_params[1], 6.0);
    }

    #[test]
    fn teleport_adapter_uses_distinct_producer_flags() {
        let mut entity = entity();
        entity.install_teleport_state_for_test(Some(TeleportState::for_test(
            TeleportPhase::ChronoDelay,
            1,
            2,
            4,
        )));
        let state = DrawState::for_entity(&entity, 45, 3, ObserverDrawContext::default()).state;
        assert_eq!(state.fx_flags, FX_WARP);
        assert_eq!(state.fx_params[0], 0.5);
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

    #[test]
    fn mirage_observer_and_shimmer_match_original_executable() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/mirage_disguise.json",
        ))
        .unwrap();
        let rows = corpus["observer"].as_array().unwrap();
        assert!(rows.len() >= 31);
        for row in rows {
            let input = &row["input"];
            let frame = input["frame"].as_i64().unwrap() as u32;
            let timer = crate::sim::timer::CdTimer::from_raw(
                input["blink_start"].as_i64().unwrap() as i32,
                input["blink_duration"].as_i64().unwrap() as i32,
            );
            let disguise = DisguiseDrawInput {
                active: input["raw_disguised"].as_bool().unwrap(),
                owner_is_current_player: input["owner"].as_bool().unwrap(),
                cell_present: input["cell_present"].as_bool().unwrap(),
                detected: input["sensor_count"].as_i64().unwrap() > 0,
                blink_active: !timer.expired(frame as i32),
                start_frame: input["creation"].as_i64().unwrap() as u32,
            };
            assert_eq!(
                u64::from(disguise.shows_real_type(frame)),
                row["outputs"]["draw_actual"].as_u64().unwrap(),
                "{}",
                row["name"]
            );
            assert_eq!(
                256 | u64::from(disguise.selector_bits(frame)),
                row["outputs"]["flags_arg256"].as_u64().unwrap(),
                "{}",
                row["name"]
            );
            let decision = DrawState::resolve(
                DrawStateInput {
                    disguise: Some(disguise),
                    ..Default::default()
                },
                frame,
                0,
            );
            let admitted_flags = if disguise.active && disguise.owner_is_current_player {
                row["outputs"]["flags_arg256"].as_u64().unwrap() & 6
            } else {
                0
            };
            assert_eq!(
                u64::from(decision.state.native_selector_bits()),
                admitted_flags,
                "{}: caller gates the standalone flag helper",
                row["name"]
            );
            assert_eq!(
                row["rng_before"], row["rng_after"],
                "read-only native observer"
            );
        }
    }
}
