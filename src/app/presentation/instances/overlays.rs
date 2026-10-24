//! Overlay, AnimClass, and fog snapshot instance builders.
//!
//! Generates SpriteInstances for map overlays (ore/gems, bridges, terrain objects),
//! the simulation's AnimClass objects, and fog-of-war building snapshots.
//! Split from `presentation::instances` to keep files under the 600-line limit.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use std::collections::{BTreeMap, HashMap};

use crate::app::AppState;
use crate::map::terrain::{self, TILE_HEIGHT, TILE_WIDTH};
use crate::render::batch::SpriteInstance;
use crate::render::bridge_atlas::is_high_bridge_body_identity;
use crate::render::native_z::{self, ZGradient, pack_z_gradient};
use crate::render::overlay_atlas::{CRATE_BODY_FRAME, OverlaySpriteKey};
use crate::render::sprite_atlas::ShpSpriteKey;
use crate::render::tactical_draw_plan::{RenderZPolicy, SpriteEncoding};
use crate::rules::art_data::{AnimTypeRuntimeConfig, anim_translucency_source_alpha};
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::overlay_types::OverlayTypeFlags;
use crate::rules::overlay_types::is_bridge_overlay_index;

use super::helpers::{
    ANIM_DRAW_DEPTH_BIAS_PX, apply_shape_z_adjust, compute_sprite_depth_params, in_view,
    projection_admitted,
};

/// Map terrain entries remain the render metadata source, but once rules-backed
/// simulation authority exists the live cell index decides whether an instance
/// still exists. This keeps loading/fallback screens static without letting a
/// destroyed runtime object remain visible forever.
#[cfg(test)]
fn terrain_object_is_render_visible(
    object: &crate::map::overlay::TerrainObject,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    authority: Option<&crate::sim::terrain_object::TerrainObjectStore>,
) -> bool {
    let (Some(rules), Some(terrain_objects)) = (rules, authority) else {
        return true;
    };
    if rules
        .terrain_object_type_case_insensitive(&object.name)
        .is_none()
    {
        return true;
    }
    let Some(stable_id) = terrain_objects.id_at((object.rx, object.ry)) else {
        return false;
    };
    terrain_objects
        .get(stable_id)
        .is_some_and(|terrain| terrain.is_live() && terrain.cell() == (object.rx, object.ry))
}

/// Body frame the native overlay draw selects for a non-bridge overlay cell.
///
/// `Crate=yes` overlays take a dedicated branch that hardcodes frame 0; every
/// other overlay draws its cell overlay-data byte directly (ore density, wall
/// `damage << 4 | connectivity`).
fn overlay_body_frame(is_crate: bool, overlay_data: u8) -> u8 {
    if is_crate {
        CRATE_BODY_FRAME
    } else {
        overlay_data
    }
}

/// The ordinary overlay pass must not own any identity whose live numeric ID
/// dispatches through native high-bridge Mark, even when layered rules rename
/// its art away from the stock BRIDGE1/2 family.
fn ordinary_overlay_accepts_identity(overlay_id: u8, name: &str) -> bool {
    !is_high_bridge_body_identity(overlay_id, name)
}

/// `CellClass::DrawOverlay_Body @ 0x0047F6A0` returns before drawing for two
/// literal live identities (`CMP [Cell+0x44],0xA7` at `0x0047F6AC`, `0xB2` at
/// `0x0047F6B7`); retail `[OverlayTypes]` declares `USELESS` and
/// `VEINHOLEDUMMY` there.
fn body_draw_skips_live_identity(overlay_id: u8) -> bool {
    matches!(overlay_id, 0xA7 | 0xB2)
}

/// Ordinary overlay bodies are SHP draws, not TMP tiles. The active path is
/// `Cell_ContentRendering @ 0x006D6D10 -> DrawOverlay_Body @ 0x0047F6A0`.
/// Every body requests 0x4E00; selector 0x00490B90's +0xBC family tests and
/// writes Z (same opaque leaf as buildings). Wall (+0x2A8) forces gradient 2
/// with `-15 * level - 2`; ordinary non-walls use DrawFlat (+0x2B3), adding
/// -15 only for upright non-rock art. Tiberium's flat branch uses gradient 0.
///
/// Sloped resources and literal legacy identity7E also select the raw
/// optional surface owned by OverlayAtlas. IsVeins/IsVeinholeMonster do
/// not disable native depth. Rubble swaps in its building's art but keeps the
/// ordinary flags-derived Z: 0047F6A0's IsRubble arm computes level,
/// gradient and DrawFlat term exactly as the ordinary arm does.
struct OverlayBodyDepth {
    render_z: RenderZPolicy,
    z_adjust: f32,
    gradient: ZGradient,
    shape: Option<crate::render::overlay_atlas::OverlayShapeRequest>,
}

fn ordinary_overlay_z(
    overlay_id: u8,
    flags: Option<&OverlayTypeFlags>,
    slope: u8,
    level: u8,
) -> Option<OverlayBodyDepth> {
    let Some(flags) = flags else {
        return Some(OverlayBodyDepth {
            render_z: RenderZPolicy::None,
            z_adjust: 0.0,
            gradient: ZGradient::Flat,
            shape: None,
        });
    };
    let legacy_veins = !flags.tiberium && !flags.wall && overlay_id == 0x7E;
    let shape = if (flags.tiberium || legacy_veins) && slope != 0 {
        // Only AA1060..AA106C are native optional-shape pointers. Higher
        // slopes index unrelated globals; skip that malformed draw safely.
        if !(1..=4).contains(&slope) {
            return None;
        }
        Some(crate::render::overlay_atlas::OverlayShapeRequest {
            slope,
            offset: if legacy_veins { [30, -2] } else { [0, 0] },
        })
    } else {
        None
    };
    let (gradient, class_term) = if flags.tiberium || legacy_veins {
        (ZGradient::Flat, -2)
    } else if flags.wall {
        (ZGradient::Vertical, -2)
    } else {
        (
            if flags.draw_flat {
                ZGradient::Flat
            } else {
                ZGradient::Vertical
            },
            if flags.draw_flat || flags.is_a_rock {
                -2
            } else {
                -17
            },
        )
    };
    Some(OverlayBodyDepth {
        render_z: RenderZPolicy::ReadWrite,
        z_adjust: native_z::ground_anchored_z_adjust(i32::from(level), class_term) as f32,
        gradient,
        shape,
    })
}

/// `CellClass::DrawOverlay_Body @ 0x0047F6A0`'s IsRubble arm: the cell draws
/// its recorded building type's rubble frame (`Cell+0x40`, `0x0045F160`), and
/// nothing when it has none or the type's image carries no rubble.
fn rubble_sprite_key(
    live_cell: Option<&crate::sim::overlay_grid::OverlayCell>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> Option<OverlaySpriteKey> {
    let type_id = live_cell?.rubble_building_type?;
    let art = rules?.building_rubble_art(interner?.resolve(type_id))?;
    Some(crate::render::overlay_atlas::rubble_key(
        &art.shp,
        art.body_frame,
    ))
}

/// Resolve the CellClass overlay identity/data used by the tactical overlay
/// draw. Low-bridge damage and repair mutate the identity in place, so a map
/// pack entry is only the iteration anchor once live cell authority exists.
pub(super) fn overlay_render_identity(
    static_overlay_id: u8,
    static_overlay_data: u8,
    live_cell: Option<&crate::sim::overlay_grid::OverlayCell>,
) -> Option<(u8, u8)> {
    let Some(live_cell) = live_cell else {
        return Some((static_overlay_id, static_overlay_data));
    };
    if is_bridge_overlay_index(static_overlay_id) {
        return live_cell
            .overlay_id
            .map(|overlay_id| (overlay_id, live_cell.overlay_data));
    }
    (live_cell.overlay_id == Some(static_overlay_id))
        .then_some((static_overlay_id, live_cell.overlay_data))
}

/// Choose the display-only identity for a resource cell.
///
/// Active YR `CellClass__DrawOverlay_Body @ 0x0047F6A0` keeps the Cell's
/// overlay identity/data as resource state but selects a coordinate-derived
/// image for the resolved slope. Missing metadata/non-resource overlays keep
/// the live identity. Native-invalid slope selectors are skipped safely.
fn overlay_display_identity(
    live_overlay_id: u8,
    overlay_data: u8,
    rx: u16,
    ry: u16,
    slope_type: Option<u8>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    tiberium_types: Option<&crate::rules::tiberium_type::TiberiumTypeRegistry>,
) -> Option<(u8, u8)> {
    let (Some(overlay_registry), Some(tiberium_types)) = (overlay_registry, tiberium_types) else {
        return Some((live_overlay_id, overlay_data));
    };
    if slope_type.is_none()
        || !overlay_registry
            .flags(live_overlay_id)
            .is_some_and(|flags| flags.tiberium)
    {
        return Some((live_overlay_id, overlay_data));
    }

    let slope = slope_type.unwrap_or(0);
    overlay_registry
        .tiberium_display_overlay_id(tiberium_types, live_overlay_id, rx, ry, slope)
        .or_else(|| (slope == 0).then_some(live_overlay_id))
        .map(|display| (display, overlay_data))
}

/// Borrowed inputs to the one retained dynamic-effect traversal. App startup,
/// platform windows and unrelated presentation resources are not draw inputs.
pub(crate) struct DisplayEffectInputs<'a> {
    pub interpolation: Option<&'a crate::app::presentation::interpolation::RenderInterpolation>,
    pub sim: &'a crate::sim::world::Simulation,
    pub rules: &'a crate::rules::ruleset::RuleSet,
    pub camera: [f32; 2],
    pub render_size: [u32; 2],
    pub tactical: [u32; 4],
    pub zoom: f32,
    pub axis: crate::render::batch::DepthAxis,
    pub atlas: Option<&'a crate::render::sprite_atlas::SpriteAtlas>,
    pub detail: &'a std::cell::RefCell<crate::app::presentation::detail::DetailState>,
    pub detail_level: u32,
    pub lighting: &'a crate::map::lighting::CellLightGrid,
    pub retained_anim_intensities: Option<&'a BTreeMap<u64, i32>>,
    pub house_colors: &'a crate::map::houses::HouseColorMap,
    pub hidden_super_anims: &'a std::collections::BTreeSet<crate::sim::anim_class::AnimId>,
    /// The local Show Hidden option (`Options+0x1F`).
    pub show_hidden: bool,
    pub local_owner: Option<crate::sim::intern::InternedId>,
    pub observe_particles: bool,
}

/// Traverse dynamic effect bodies in actual Display order. Anim and particle
/// consumers share the mutative55AF60 FPS latch; grouping them by class changes
/// native admission when the unsigned minimum/buffer arithmetic wraps.
/// Prepared building slots keep their binding/light owner, but query and append
/// only at their own encounter, even when preparation produced no visible piece.
pub(crate) fn build_display_effect_instances(
    input: &DisplayEffectInputs<'_>,
    ground_objects: &mut Vec<
        crate::app::presentation::render::draw_plan_lowering::PlannedObjectInstance,
    >,
    ground_order: &crate::app::presentation::render::draw_plan_lowering::NativeDisplayOrder,
    mut prepared_building_anims: BTreeMap<
        crate::sim::anim_class::AnimId,
        crate::app::presentation::render::draw_plan_lowering::PlannedObjectInstance,
    >,
) -> Option<Vec<super::particles::ParticleDrawObservation>> {
    let mut observations = input.observe_particles.then(Vec::new);
    let sim = input.sim;
    let context = super::particles::ParticleDrawContext::new(
        input.camera,
        input.zoom,
        input.tactical,
        input.axis,
    );
    // Particles have independent AIR registrations, including live bodies
    // detached from a System. The store owns their lifetime; Display supplies
    // every encounter, never this frame-local lookup's iteration/hash order.
    let particles: HashMap<_, _> = sim
        .particle_systems()
        .display_particles()
        .map(|(system, particle)| (particle.stable_id(), (system, particle)))
        .collect();
    let z2 = input.zoom;
    let (cam_x, cam_y, sw, sh) = (
        input.camera[0],
        input.camera[1],
        input.render_size[0] as f32 / z2,
        input.render_size[1] as f32 / z2,
    );
    let projection_viewport = [input.tactical[2] as f32 / z2, input.tactical[3] as f32 / z2];
    let parachute_type = input
        .rules
        .general
        .parachute_shp
        .as_deref()
        .and_then(|name| sim.interner().get(name));
    let local_owner = input.local_owner;
    for &stable_id in sim.display_layers().ordered_ids() {
        if let Some(light) = sim.building_spotlight(stable_id, input.rules) {
            super::particles::build_spotlight_instance(
                input,
                stable_id,
                light,
                &context,
                ground_objects,
                ground_order,
            );
            continue;
        }
        if let Some(system) = sim.particle_systems().get(stable_id) {
            super::particles::build_particle_system_instance(
                input,
                system,
                &context,
                ground_objects,
                ground_order,
            );
            continue;
        }
        if let Some(&(system, particle)) = particles.get(&stable_id) {
            super::particles::build_particle_instance(
                input,
                system,
                particle,
                &context,
                ground_objects,
                ground_order,
                &mut observations,
            );
            continue;
        }
        let Some(atlas) = input.atlas else {
            continue;
        };
        let Some(anim) = sim.anim(stable_id) else {
            continue;
        };
        // Paradrop creates a retained Anim, whose GetCoords resolves its
        // owner-relative75-lepton lift. Draw it at its own Display encounter.
        let is_parachute =
            parachute_type.is_some_and(|ty| parachute_canopy_owner(anim, ty).is_some());
        let type_name: &str = sim.interner().resolve(anim.type_id());
        let config = input.rules.art().anim_runtime_config(type_name);
        if config
            .is_some_and(|config| !config.art_body_read && config.raw_shp_frame_count.is_none())
        {
            continue;
        }
        // AnimDrawIfVisible422C70 rejects delay+184 before Object5F4B10's
        // Tactical6D2140 projection gate and the mutative DrawIt55AF60 query.
        // Whole-slot controls: spatial_oracle/building_anim_detail.json.
        if anim.runtime().delay_remaining != 0 {
            continue;
        }
        // AnimGetCoords422BE0 resolves owner-relative coordinates. Admission
        // uses the same painted point as the existing body emitter, without
        // image offsets or the parent Building's viewport/body visibility.
        let Some(anim_coord) = sim.anim_absolute_coord(stable_id) else {
            continue;
        };
        let (mut center_x, mut center_y, rx, ry, _, mut lift_px) =
            anim_world_render_coords(anim_coord);
        let mut drawn_coord = [anim_coord.x, anim_coord.y, anim_coord.z];
        let is_building_anim = anim.is_building_anim();
        // Slot preparation uses the committed coordinate; retain that geometry.
        // Free Anims keep their existing sampled point for admission and drawing.
        if !is_building_anim && let Some(interpolation) = input.interpolation {
            use crate::app::presentation::interpolation::{PointKind, PointPose};
            let sample = interpolation.sample_point(
                PointKind::Anim,
                stable_id,
                PointPose::new(
                    [anim_coord.x, anim_coord.y, anim_coord.z],
                    anim.type_id().index(),
                    anim.owner_entity(),
                ),
                sim.session().binary_frame(),
            );
            [center_x, center_y] = sample.screen;
            lift_px = sample.lift.round() as i32;
            drawn_coord = sample.coord;
        }
        if !projection_admitted([center_x, center_y], input.camera, projection_viewport) {
            continue;
        }
        if !crate::sim::anim_class::anim_draw_detail_visible(
            crate::sim::anim_class::AnimDrawDetailInput {
                frame_rate_below_minimum: {
                    let mut detail = input.detail.borrow_mut();
                    detail.frame_rate() < detail.minimum_frame_rate()
                },
                type_detail_level: config.map_or(0, |value| value.detail_level),
                game_detail_level: input.detail_level as i32,
                // `AnimClass::AI` rewrites a PsiWarning anim's hidden byte
                // every frame from the local player's detection.
                hidden: match config.is_some_and(|config| config.psi_warning) {
                    true => !local_owner.is_some_and(|viewer| {
                        sim.psi_warning_detected_by(viewer, anim, input.rules)
                    }),
                    // ... and the `[General] Behind=` anim's from the local
                    // Show Hidden option (`0x00423B62..0x00423B88`).
                    false
                        if input
                            .rules
                            .general
                            .behind
                            .as_deref()
                            .is_some_and(|behind| behind.eq_ignore_ascii_case(type_name)) =>
                    {
                        !input.show_hidden
                    }
                    // A Super's ChronoPlacement anim: the client's byte
                    // (`match_runtime::super_selection`).
                    false => {
                        anim.draw_runtime().hidden || input.hidden_super_anims.contains(&stable_id)
                    }
                },
                special_hidden: anim.draw_runtime().special_hidden,
                // The native special-hide type bit remains an explicit residual.
                type_special_hide: false,
            },
        ) {
            continue;
        }
        // AnimDrawIt423014..423026 applies the fog byte after its detail
        // queries, and only when AnimType.ShouldFogRemove is set.
        if config.is_none_or(|config| config.should_fog_remove)
            && local_owner.is_some_and(|viewer| sim.fog().is_object_fogged(viewer, stable_id))
        {
            continue;
        }
        if is_building_anim {
            // DrawIt queries detail before hidden/image/material rejection.
            // An absent prepared piece must not suppress that query.
            if let Some(parent) = prepared_building_anims.remove(&stable_id) {
                ground_objects.push(parent);
            }
            continue;
        }
        // DrawIt423055..42305D resolves the ordinary source as AC + AnimType.Start,
        // just as the building-slot caller. Special draw arms retain their
        // existing frame selection until their own renderer is ported, and so
        // does a HasBounce ground shadow ahead of them, which natively takes
        // AC + Start in every arm (4233CD). Residual: no retail Tiled or RING1
        // type sets Bouncer=, IsMeteor= or Start=.
        let start = if super::anim_draw::ordinary_anim_type(type_name, config) {
            config.map_or(0, |config| config.start)
        } else {
            0
        };
        let Ok(frame) = u16::try_from(anim.runtime().current_frame.wrapping_add(start)) else {
            continue;
        };
        let remap_color =
            crate::render::sprite_atlas::anim_remap_color(anim, sim.interner(), input.house_colors);
        let palette_light = crate::app::presentation::lighting::anim_palette_light(
            input.lighting,
            (rx, ry),
            config,
            anim.use_cell_drawer(),
            anim.remap().as_ref(),
            input
                .retained_anim_intensities
                .and_then(|samples| samples.get(&stable_id))
                .copied()
                .unwrap_or(1000),
        );
        let key = ShpSpriteKey {
            palette_context: if anim.use_cell_drawer() {
                crate::render::sprite_atlas::ShpPaletteContext::AnimCell
            } else if remap_color.is_some() {
                crate::render::sprite_atlas::anim_remap_context(anim)
            } else {
                crate::render::sprite_atlas::ShpPaletteContext::GlobalAnim
            },
            type_id: type_name.to_string(),
            facing: 0,
            frame,
            house_color: remap_color.unwrap_or(HouseColorIndex(0)),
        };
        let entry = atlas.get(&key);
        // DrawIt422CA0's D4 instance Convert carries its palette/FC into
        // the ordinary native blitter. Sparse FC absence means ctor1000;
        // it is not a reason to choose the compatibility material.
        let instance_scheme = remap_color.is_some()
            && !anim.use_cell_drawer()
            && entry.is_some_and(|entry| entry.source_palette != 0)
            && super::anim_draw::ordinary_anim_type(type_name, config);
        let tint = if is_parachute || instance_scheme {
            [1.0; 3]
        } else {
            input.lighting.anim_tint_at((rx, ry), config)
        };
        let Some(flags) = anim_instance_flags(
            config,
            anim.draw_flags(),
            anim.runtime().current_frame,
            i32::from(
                presentation_anim_frame_count(&atlas.active_anim_frame_counts, type_name)
                    .unwrap_or(0),
            ),
            input.detail_level as i32,
            anim.draw_runtime(),
        ) else {
            continue;
        };
        // Ordinary DrawIt422CA0 -> 4AED70 carries Convert's selector into
        // the packed RGB565 blitter, rather than alpha-blending the source.
        // Original full draw controls: spatial_oracle/anim_translucency.
        let (alpha, draw_state) = if (remap_color.is_none() || instance_scheme)
            && super::anim_draw::ordinary_anim_type(type_name, config)
        {
            (
                1.0,
                crate::render::draw_state::DrawState::from_native_selector(0, (flags & 6) as u8),
            )
        } else {
            // Separate native draw arms and remapped compatibility sources
            // without native palette metadata retain their existing material.
            (anim_translucency_source_alpha(flags), Default::default())
        };
        let (origin_y, world_height) = (input.axis.origin_y, input.axis.world_height);
        let fire_depth = super::helpers::compute_sprite_depth_params_lifted(
            origin_y,
            world_height,
            center_y,
            lift_px,
        );
        debug_assert!(!anim.terrain_attached() || anim.use_cell_drawer());
        // Prepare the ordinary body coordinate/material. The shared DrawIt
        // pass builder owns Flat's separate Z term and the optional shadow.
        let instance = SpriteInstance {
            depth: apply_shape_z_adjust(
                fire_depth,
                anim.z_adjust() + ANIM_DRAW_DEPTH_BIAS_PX,
                world_height,
            ),
            tint,
            palette_light,
            alpha,
            draw_state,
            // YDrawOffset is baked into the atlas offset, so it must also
            // ride the Z term to keep Z on the un-offset row, as natively.
            z_adjust: super::helpers::lifted_z_adjust(
                lift_px,
                anim.z_adjust() + config.map_or(0, |c| c.y_draw_offset) + ANIM_DRAW_DEPTH_BIAS_PX,
            ),
            z_gradient: crate::render::native_z::pack_z_gradient(
                if config.is_some_and(|c| c.flat) {
                    crate::render::native_z::ZGradient::Flat
                } else {
                    crate::render::native_z::ZGradient::Vertical
                },
                false,
            ),
            ..Default::default()
        };
        if let Some(parent) = ground_order.object_draw(anim.stable_id(), SpriteEncoding::Plain) {
            ground_objects.push(
                crate::app::presentation::render::draw_plan_lowering::PlannedObjectInstance::object(
                    parent,
                    super::anim_draw::anim_draw_pieces(
                        atlas,
                        super::anim_draw::AnimDrawInput {
                            key: &key,
                            entry,
                            point: [center_x, center_y],
                            body: instance,
                            config,
                            flags,
                            lift_px,
                            axis: input.axis,
                            viewport: [cam_x, cam_y, sw, sh],
                            legacy_policy: parent.policy.render_z,
                            bounce_drop_px: super::anim_draw::anim_bounce_drop_px(
                                sim,
                                anim,
                                drawn_coord,
                            ),
                        },
                    ),
                ),
            );
        }
    }
    observations
}

/// Source-pixel weight one animation frame draws with.
///
/// gamemd picks a blitter family per draw from the art type's `Translucency=` /
/// `Translucent=` keys ([`crate::sim::anim_class::anim_translucency_selection`]);
/// this returns the weight the family gives the incoming sprite pixel. An art type
/// the registry does not know draws opaque, matching the resolver's own
/// no-keys result.
#[cfg(test)]
fn anim_instance_alpha(
    config: Option<&AnimTypeRuntimeConfig>,
    current_frame: i32,
    shp_frame_count: i32,
) -> f32 {
    anim_instance_alpha_with_flags(
        config,
        0,
        current_frame,
        shp_frame_count,
        2,
        crate::sim::anim_class::AnimDrawRuntime::default(),
    )
    .unwrap_or(0.0)
}

/// `AnimClass::DrawIt`'s translucency selection (`0x00423061`) as a source
/// alpha, or `None` where it skips the draw. Free anims and building slot
/// anims both draw through it.
#[cfg(test)]
pub(super) fn anim_instance_alpha_with_flags(
    config: Option<&AnimTypeRuntimeConfig>,
    base_flags: u32,
    current_frame: i32,
    shp_frame_count: i32,
    game_detail_level: i32,
    draw_runtime: crate::sim::anim_class::AnimDrawRuntime,
) -> Option<f32> {
    anim_instance_flags(
        config,
        base_flags,
        current_frame,
        shp_frame_count,
        game_detail_level,
        draw_runtime,
    )
    .map(anim_translucency_source_alpha)
}

/// Keep DrawIt's selected flags for the body material and its later shadow.
pub(super) fn anim_instance_flags(
    config: Option<&AnimTypeRuntimeConfig>,
    base_flags: u32,
    current_frame: i32,
    shp_frame_count: i32,
    game_detail_level: i32,
    draw_runtime: crate::sim::anim_class::AnimDrawRuntime,
) -> Option<u32> {
    let result = crate::sim::anim_class::anim_translucency_selection(
        crate::sim::anim_class::AnimTranslucencyInput {
            base_flags,
            forced_translucent: draw_runtime.forced_translucent,
            double_thick: config.is_some_and(|value| value.double_thick),
            translucency_detail_level: config.map_or(0, |value| value.translucency_detail_level),
            // Draw-time detail is presentation state and does not enter simulation.
            game_detail_level,
            translucent_ramp: config.is_some_and(|value| value.translucent),
            current_frame,
            end_frame: config.map_or(shp_frame_count, |value| value.loaded_end(shp_frame_count)),
            explicit_translucency: config.map_or(0, |value| value.translucency),
            instance_ramp: i32::from(draw_runtime.translucency_ramp),
        },
    );
    result.draw.then_some(result.flags)
}

pub(super) fn presentation_anim_frame_count(
    frame_counts: &HashMap<String, u16>,
    type_name: &str,
) -> Option<u16> {
    frame_counts.get(type_name).copied().or_else(|| {
        let canonical = type_name.to_ascii_uppercase();
        frame_counts.get(&canonical).copied()
    })
}

/// The object a canopy hangs on: an owner-attached anim of the
/// `[General] Parachute=` type.
fn parachute_canopy_owner(
    anim: &crate::sim::anim_class::AnimObject,
    parachute_type: crate::sim::intern::InternedId,
) -> Option<u64> {
    (anim.type_id() == parachute_type)
        .then_some(anim.owner_entity())
        .flatten()
}

/// Screen position, cell, height level and the exact pixel lift of an anim.
///
/// The sprite sits at the anim's exact Z (a muzzle or an airburst is not on a
/// level boundary). The lift is the same `AdjustForZ(z)` the projection
/// subtracted, so depth can cancel exactly what was drawn, as
/// `AnimClass::DrawIt @ 0x00422CA0` does.
pub(super) fn anim_world_render_coords(
    world: crate::sim::anim_class::AnimWorldCoord,
) -> (f32, f32, u16, u16, u8, i32) {
    let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
    let (screen_x, screen_y) =
        crate::util::lepton::lepton_to_screen_exact_z(rx, ry, sub_x, sub_y, world.z);
    let lift_px = crate::util::native_x87::adjust_for_z_standard(world.z);
    (screen_x, screen_y, rx, ry, z, lift_px)
}

/// The Convert and scalar 0047F6A0 passes for one overlay body, in its branch
/// order: ordinary cell Convert/common; resource (+0x2A9) and veins use global
/// ordinary Convert, with resource brightness fixed at 1000. Walls (OverlayType+0x2A8,
/// 0047F96A..0047F9C1) take the current house's ColorScheme Convert
/// (A83D4C+0x16054 -> B054D4[]+0x30C) at the cell's +0x10A scalar. Every
/// Convert reads A, so unexplored shroud selects palette row 0 (black).
///
/// RESIDUAL: that scheme's house remap of wall art is absent from this RGBA
/// atlas; walls keep their unremapped colours. Trigger: every wall draw.
/// Effect: remap-range wall pixels show source colours. Presentation only.
fn ordinary_overlay_palette_light(
    flags: Option<&OverlayTypeFlags>,
    light_grid: &crate::map::lighting::CellLightGrid,
    cell: (u16, u16),
) -> crate::render::palette_light::PaletteLight {
    use crate::render::palette_light::PaletteLight;
    match flags {
        Some(f) if f.tiberium => PaletteLight::plain(53, 1000),
        Some(f) if f.wall => {
            crate::app::presentation::lighting::body_palette_light(light_grid, cell, 0)
        }
        Some(f) if f.is_veins => PaletteLight::plain(
            53,
            light_grid
                .cell_light_at(cell)
                .map_or(1000, |l| l.common_scalar),
        ),
        _ => PaletteLight::cell(light_grid, cell, false),
    }
}

/// Borrowed inputs for the ordinary CellClass overlay pass. Live identity and
/// frame stay owned by OverlayGrid; the source entries only retain draw order.
struct CellOverlayInputs<'a> {
    entries: &'a [crate::map::overlay::OverlayEntry],
    atlas: &'a crate::render::overlay_atlas::OverlayAtlas,
    names: &'a std::collections::BTreeMap<u8, String>,
    live_grid: Option<&'a crate::sim::overlay_grid::OverlayGrid>,
    registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    /// Rules and interner resolving a rubble cell's recorded building type.
    rules: Option<&'a crate::rules::ruleset::RuleSet>,
    interner: Option<&'a crate::sim::intern::StringInterner>,
    tiberium_types: Option<&'a crate::rules::tiberium_type::TiberiumTypeRegistry>,
    terrain: Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid>,
    lighting: &'a crate::map::lighting::CellLightGrid,
    camera: [f32; 2],
    viewport: [f32; 2],
    origin_y: f32,
    world_height: f32,
}

/// Cell_ContentRendering @ 0x006D6D10 -> DrawOverlay_Body @ 0x0047F6A0 for
/// every cell whose overlay is not a high bridge; the cell body sweep
/// ([`super::submit_cell_body_sweep`]) orders them. Both the app and
/// offscreen retail witness use this one identity/frame/geometry owner.
/// Native 6D6D10 checks map bounds and art/clip rectangles, not exploration;
/// the 47FB90/47FDE0 rectangles and 47F6A0/47F510 draws never read Cell+12C/+130.
/// The shared ABuffer supplies per-pixel shroud independently of cell admission.
fn build_cell_overlay_bodies(input: &CellOverlayInputs<'_>, bodies: &mut Vec<super::CellBody>) {
    // Overlay entries from [OverlayPack]. `YR TacticalClass::Draw` keeps walls
    // in the fixed cell overlay family, not the `LayerClass` object sort.
    for entry in input.entries {
        let Some(static_name) = input.names.get(&entry.overlay_id) else {
            continue;
        };

        // High-bridge bodies are emitted by `instances::bridges` reading
        // live CellClass fields post-tick. Skip them here so they don't double-
        // render via the static map overlay list.
        if !ordinary_overlay_accepts_identity(entry.overlay_id, static_name) {
            continue;
        }

        // Low bridges remain in the ordinary overlay pass, but CellClass's
        // live identity—not the map-pack seed—selects damaged/collapsed art.
        let live_overlay_cell = input.live_grid.map(|grid| grid.cell(entry.rx, entry.ry));
        let Some((live_overlay_id, live_overlay_data)) =
            overlay_render_identity(entry.overlay_id, entry.frame, live_overlay_cell)
        else {
            continue;
        };
        if body_draw_skips_live_identity(live_overlay_id) {
            continue;
        }
        let overlay_registry = input.registry;
        let overlay_flags = overlay_registry.and_then(|reg| reg.flags(live_overlay_id));
        let slope_type = input
            .terrain
            .and_then(|terrain| terrain.cell(entry.rx, entry.ry))
            .map(|cell| cell.slope_type);
        let Some((display_overlay_id, live_overlay_data)) = overlay_display_identity(
            live_overlay_id,
            live_overlay_data,
            entry.rx,
            entry.ry,
            slope_type,
            overlay_registry,
            input.tiberium_types,
        ) else {
            continue;
        };
        let name = if display_overlay_id == live_overlay_id {
            input.names.get(&live_overlay_id).cloned()
        } else {
            overlay_registry
                .and_then(|registry| registry.name(display_overlay_id).map(str::to_owned))
        };
        let Some(name) = name else {
            continue;
        };
        if !ordinary_overlay_accepts_identity(live_overlay_id, &name) {
            continue;
        }

        // The live identity owns resource type, wall/crate flags, and bridge
        // state even when a flat resource cell selects another display image.
        let is_crate: bool = overlay_flags.map(|f| f.crate_type).unwrap_or(false);
        let render_frame: u8 = overlay_body_frame(is_crate, live_overlay_data);
        let rubble_key = if overlay_flags.is_some_and(|f| f.is_rubble) {
            let Some(key) = rubble_sprite_key(live_overlay_cell, input.rules, input.interner)
            else {
                continue;
            };
            Some(key)
        } else {
            None
        };

        // FA2 IsoView.cpp:5955-5956: track overlays render +CellHeight (15px) lower.
        let track_y_offset: f32 = if overlay_flags.map(|f| f.track).unwrap_or(false) {
            15.0
        } else {
            0.0
        };

        let z: u8 = input
            .terrain
            .and_then(|terrain| terrain.cell(entry.rx, entry.ry))
            .map_or(0, |cell| cell.level);
        let (screen_x, screen_y) = terrain::iso_to_screen(entry.rx, entry.ry, z);
        let live_flags = input
            .terrain
            .and_then(|terrain| terrain.cell(entry.rx, entry.ry))
            .map_or(0, |cell| cell.bridge_facts.raw_flags);
        let screen_y: f32 = screen_y
            + track_y_offset
            + super::bridges::cell_overlay_y_offset(live_overlay_id, live_flags, live_overlay_data);

        // No LocalSize gate: gamemd draws overlays on border filler cells like
        // any other; camera/geometry admission precedes per-pixel shroud.
        // Rubble draws a whole building's art from one cell, so its margin
        // grows to the sprite.
        let cull_margin: f32 = rubble_key
            .as_ref()
            .and_then(|key| input.atlas.get(key))
            .map_or(120.0, |spr| {
                120.0f32.max(spr.pixel_size[0]).max(spr.pixel_size[1])
            });
        if !in_view(
            screen_x,
            screen_y,
            cull_margin,
            cull_margin,
            input.camera[0],
            input.camera[1],
            input.viewport[0],
            input.viewport[1],
            cull_margin,
        ) {
            continue;
        }

        // Exactly the frame the cell names, or nothing. Overlay draw:
        // `CellClass__DrawOverlay_Body @ 0x0047F6A0` blits `Cell+0x11E` with no
        // substitution, so a cell whose frame is empty art draws nothing — the
        // authored state for a low bridge's flanking columns (overlay data 0 and
        // 2, art only in frame 1). Collapsing to frame 0 here resurrected them
        // as phantom deck slabs. Frame selection lives in
        // `render::overlay_atlas::resolve_body_frame`.
        let key = rubble_key.unwrap_or_else(|| OverlaySpriteKey {
            name: name.clone(),
            frame: i32::from(render_frame),
        });
        let Some(spr) = input.atlas.get(&key) else {
            continue;
        };
        let depth_z: u8 = z;
        let depth: f32 =
            compute_sprite_depth_params(input.origin_y, input.world_height, screen_y, depth_z);
        let Some(body_z) = ordinary_overlay_z(
            live_overlay_id,
            overlay_flags,
            slope_type.unwrap_or(0),
            depth_z,
        ) else {
            continue;
        };
        let tint: [f32; 3] = input.lighting.overlay_tint_at((entry.rx, entry.ry));
        let palette_light =
            ordinary_overlay_palette_light(overlay_flags, input.lighting, (entry.rx, entry.ry));
        let mut instance = SpriteInstance {
            position: [
                screen_x + TILE_WIDTH / 2.0 + spr.offset_x,
                screen_y + TILE_HEIGHT / 2.0 + spr.offset_y,
            ],
            size: spr.pixel_size,
            uv_origin: spr.uv_origin,
            uv_size: spr.uv_size,
            depth,
            tint,
            palette_light,
            alpha: 1.0,
            z_adjust: body_z.z_adjust,
            z_gradient: pack_z_gradient(body_z.gradient, false),
            ..Default::default()
        };
        if body_z.shape.is_some_and(|shape| {
            !input
                .atlas
                .apply_zshape(shape, spr, input.camera, input.viewport, &mut instance)
        }) {
            continue;
        }
        bodies.push(super::CellBody {
            cell: (entry.rx, entry.ry),
            draw: super::CellBodyDraw {
                atlas: super::CellBodyAtlas::Overlay,
                render_z: body_z.render_z,
            },
            instance,
        });
    }

    if std::env::var("RA2_DEBUG_BRIDGE_RENDER_BUCKETS").is_ok() {
        log::debug!("Cell overlay bodies: {}", bodies.len());
    }
}

/// Build the ordinary overlay bodies for the cell body sweep, and the
/// terrain objects.
///
/// High-bridge bodies and every overlay shadow come from `instances::bridges`.
/// Low bridges (LOBRDG*) are ordinary bodies on the regular overlay atlas.
pub(crate) fn build_overlay_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
    bodies: &mut Vec<super::CellBody>,
    ground_objects: &mut Vec<
        crate::app::presentation::render::draw_plan_lowering::PlannedObjectInstance,
    >,
    ground_order: &crate::app::presentation::render::draw_plan_lowering::NativeDisplayOrder,
) {
    let atlas = match &state.match_state.match_presentation.overlay_atlas {
        Some(a) => a,
        None => return,
    };
    let (cam_x, cam_y) = (
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    );
    let (origin_y, world_height) = state
        .match_state
        .match_presentation
        .terrain_grid
        .as_ref()
        .map(|g| (g.origin_y, g.world_height))
        .unwrap_or((0.0, 1.0));

    build_cell_overlay_bodies(
        &CellOverlayInputs {
            entries: state.match_state.match_presentation.overlays.as_slice(),
            atlas,
            names: &state.match_state.match_presentation.overlay_names,
            live_grid: state
                .match_state
                .sim_runtime
                .as_ref()
                .and_then(|runtime| runtime.simulation.overlay_grid()),
            registry: state.overlay_registry(),
            rules: state.rules(),
            interner: state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|runtime| runtime.simulation.interner()),
            tiberium_types: state.rules().map(|rules| &rules.tiberium_types),
            terrain: state
                .match_state
                .sim_runtime
                .as_ref()
                .and_then(|runtime| runtime.simulation.resolved_terrain()),
            lighting: state.match_state.match_presentation.lighting.grid(),
            camera: [cam_x, cam_y],
            viewport: [sw, sh],
            origin_y,
            world_height,
        },
        bodies,
    );

    // Terrain objects from [Terrain] section.
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    for obj in sim.terrain_objects().objects().values() {
        if !obj.is_live() {
            continue;
        }
        let name = sim.interner().resolve(obj.type_ref());
        // Ordinary native Ground6D97D0 -> Terrain71CC50/71C1B0 admits live
        // static art independently of anchor exploration; ABuffer clips pixels.

        // Terrain Render71CD30 reads the retained Location through GetCoords
        // 5F65A0; DrawIt71C1B0 applies AdjustForZ to that retained height.
        // Original call-chain outputs: tools/spatial_oracle/terrain_render.json.
        // a later map/deck change must not move a stationary tree's artwork.
        let coord = obj.world_coord();
        let point = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
        let lift_px = crate::util::native_x87::adjust_for_z_standard(coord.z);
        let screen_x = point.0 - TILE_WIDTH / 2.0;
        let screen_y = point.1 - TILE_HEIGHT / 2.0;
        if !in_view(
            screen_x, screen_y, 120.0, 120.0, cam_x, cam_y, sw, sh, 120.0,
        ) {
            continue;
        }

        let terrain_type = state
            .rules()
            .and_then(|rules| rules.terrain_object_type_case_insensitive(name));
        let is_animated = terrain_type.is_some_and(|terrain_type| terrain_type.is_animated);
        let spawns_tiberium = terrain_type.is_some_and(|terrain_type| terrain_type.spawns_tiberium);
        // Terrain DrawIt71C208 selects the Stage written by Terrain AI.
        // Wall-clock presentation cannot advance or restart a spawner.
        let stage = is_animated.then(|| {
            sim.terrain_objects()
                .animations()
                .get(&obj.stable_id())
                .map_or(0, |animation| animation.current_frame())
        });
        let frame = terrain_draw_frame(stage, obj.health());
        let (body, shadow) = atlas.terrain_pieces(name, frame);
        if body.is_none() && shadow.is_none() {
            continue;
        }

        let depth = super::helpers::compute_sprite_depth_params_lifted(
            origin_y,
            world_height,
            screen_y,
            lift_px,
        );
        let tint: [f32; 3] = state
            .match_state
            .match_presentation
            .lighting
            .grid()
            .terrain_object_tint_for_type(obj.cell(), spawns_tiberium);
        // Terrain DrawIt 0071C286..0071C304: SpawnsTiberium selects the
        // global ordinary Convert/top; ordinary trees select cell Convert/common.
        let light_grid = state.match_state.match_presentation.lighting.grid();
        let palette_light = if spawns_tiberium {
            crate::render::palette_light::PaletteLight::plain(
                53,
                light_grid
                    .cell_light_at(obj.cell())
                    .map_or(1000, |l| l.top_scalar),
            )
        } else {
            crate::render::palette_light::PaletteLight::cell(light_grid, obj.cell(), false)
        };

        let Some(mut parent) = ground_order.object_draw(obj.stable_id(), SpriteEncoding::Terrain)
        else {
            continue;
        };
        use crate::app::presentation::render::draw_plan_lowering::{
            ObjectPieceInstance, ObjectTexture, PlannedObjectInstance,
        };
        use crate::render::tactical_draw_plan::RenderZPolicy;
        use crate::render::terrain_draw::TerrainPiece;
        // Terrain DrawIt 0071C304/0071C34E preserves one draw point for
        // body and shadow. Each SHP's own integer canvas/frame offsets
        // then locate the stored rectangle.
        let point = [
            screen_x + TILE_WIDTH / 2.0,
            screen_y + TILE_HEIGHT / 2.0 - if spawns_tiberium { 16.0 } else { 0.0 },
        ];
        // DrawIt flags 2E00 read Z only for an animated type; 4E00 also write.
        let render_z = if is_animated {
            RenderZPolicy::ReadOnly
        } else {
            RenderZPolicy::ReadWrite
        };
        parent.policy.render_z = render_z;
        let pieces = [(TerrainPiece::Body, body), (TerrainPiece::Shadow, shadow)]
            .into_iter()
            .filter_map(|(piece, sprite)| {
                Some(ObjectPieceInstance {
                    target: ObjectTexture::TerrainShp(piece),
                    render_z,
                    instance: native_terrain_instance(
                        sprite?,
                        piece,
                        point,
                        lift_px,
                        depth,
                        tint,
                        palette_light,
                    ),
                })
            })
            .collect();
        ground_objects.push(PlannedObjectInstance::object(parent, pieces));
    }
}

/// Terrain DrawIt (0x71C1B0) frame: an IsAnimated type's stage, else frame 1
/// below 2 health, else 0. Its crumbling arm never draws: Take_Damage
/// (0x71B920) starts crumbling on a lethal hit and uninits the object in the
/// same call.
fn terrain_draw_frame(stage: Option<i32>, health: i32) -> i32 {
    // Original71C208 keeps the whole signed Stage; frame bounds belong to
    // the image/atlas, not the cell overlay-data byte's0..255 domain.
    stage.unwrap_or_else(|| i32::from(health < 2))
}

/// Ordinary Terrain DrawIt 0071C304/0071C34E: both stored frames share one
/// projected draw point. The body takes gradient 2 and class Z -12, the
/// shadow gradient 0 and -3.
fn native_terrain_instance(
    sprite: &crate::render::overlay_atlas::OverlaySpriteEntry,
    piece: crate::render::terrain_draw::TerrainPiece,
    point: [f32; 2],
    lift_px: i32,
    depth: f32,
    tint: [f32; 3],
    palette_light: crate::render::palette_light::PaletteLight,
) -> SpriteInstance {
    let (gradient, class_z) = match piece {
        crate::render::terrain_draw::TerrainPiece::Body => (2, -12),
        crate::render::terrain_draw::TerrainPiece::Shadow => (0, -3),
    };
    SpriteInstance {
        position: [point[0] + sprite.offset_x, point[1] + sprite.offset_y],
        size: sprite.pixel_size,
        uv_origin: sprite.uv_origin,
        uv_size: sprite.uv_size,
        depth,
        tint,
        palette_light,
        alpha: 1.0,
        z_gradient: gradient,
        z_adjust: super::helpers::lifted_z_adjust(lift_px, class_z),
        ..Default::default()
    }
}

/// Sample the upstream polygon without rebuilding its native shape. History
/// uses world-projected points, so camera movement never becomes wave motion.
fn sample_weapon_wave_draw(
    mut draw: crate::render::wave_geometry::WaveDraw,
    id: u64,
    cache: &crate::app::presentation::interpolation::RenderInterpolation,
    frame: u32,
    camera: [i32; 2],
) -> crate::render::wave_geometry::WaveDraw {
    use crate::app::presentation::interpolation::{PointKind, PointPose};
    use crate::render::wave_geometry::WavePolygon;
    let sample = |index, p: [i32; 2]| {
        let [x, y, _] = cache
            .sample_draw_point(
                PointKind::Wave(index),
                id,
                PointPose::new([p[0], p[1], 0], 0, None),
                frame,
            )
            .coord;
        [x - camera[0], y - camera[1]]
    };
    let points: &mut [[i32; 2]] = match &mut draw.polygon {
        WavePolygon::Magnetic(points) => points,
        WavePolygon::Sonic(points) => points,
    };
    for (i, p) in points.iter_mut().enumerate() {
        *p = sample(i as u8, *p);
    }
    draw.source_point = sample(6, draw.source_point);
    draw.source_z = cache
        .sample_draw_point(
            PointKind::Wave(7),
            id,
            PointPose::new([0, 0, draw.source_z], 0, None),
            frame,
        )
        .coord[2];
    draw
}

/// Attach native Sonic/magnetic destination edits to retained Display parents.
pub(crate) fn build_weapon_wave_instances(
    state: &AppState,
    objects: &mut Vec<crate::app::presentation::render::draw_plan_lowering::PlannedObjectInstance>,
    order: &crate::app::presentation::render::draw_plan_lowering::NativeDisplayOrder,
) {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let observer = crate::app::input::commands::preferred_local_owner(state)
        .as_deref()
        .and_then(|owner| sim.interner().get(owner));
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ]
    .map(|value| value.floor() as i32);
    let cache = &state.match_state.match_presentation.interpolation;
    for (id, draw) in
        crate::app::presentation::fire_effects::build_weapon_wave_draws(sim, observer, [0, 0])
    {
        let draw = sample_weapon_wave_draw(draw, id, cache, sim.session().binary_frame(), camera);
        use crate::app::presentation::render::draw_plan_lowering::{
            ObjectBodyDraw, PlannedObjectInstance,
        };
        let Some(parent) = order.object_draw(id, SpriteEncoding::Plain) else {
            continue;
        };
        let mut planned = PlannedObjectInstance::object(parent, Vec::new());
        planned.body_draw = Some(ObjectBodyDraw::Surface(
            crate::render::surface_line::SurfaceDraw::Wave(draw),
        ));
        objects.push(planned);
    }
    // Live BuildingLightRuntime producers are intentionally not lowered here:
    // native DrawExtras first uses SpotlightClass type 16's zero-blend
    // shape-blitter/light-mask path, which this renderer does not yet expose.
    // A white quad or alpha approximation would invent visible behavior.
}

#[cfg(test)]
#[path = "ore_gpu_tests.rs"]
mod ore_gpu_tests;

#[cfg(test)]
#[path = "ordinary_bridge_gpu_tests.rs"]
mod ordinary_bridge_gpu_tests;

#[cfg(test)]
#[path = "terrain_render_tests.rs"]
mod terrain_render_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn render_interpolation_preserves_both_native_wave_shapes_and_camera_independence() {
        use crate::app::presentation::interpolation::RenderInterpolation;
        use crate::render::wave_geometry::{WaveClipPath, WaveDraw, WavePolygon};
        use crate::sim::world::Simulation;
        let six = [
            [100, 200],
            [104, 198],
            [112, 206],
            [110, 208],
            [106, 210],
            [102, 205],
        ];
        for polygon in [
            WavePolygon::Magnetic(std::array::from_fn(|i| six[i])),
            WavePolygon::Sonic(six),
        ] {
            let old = WaveDraw {
                polygon,
                source_point: [101, 201],
                source_z: 0,
                phase: 3,
                direction: 2,
                clip_path: WaveClipPath::Fast,
            };
            let mut sim = Simulation::new();
            sim.session_mut_for_test().set_binary_frame_for_test(1);
            let mut cache = RenderInterpolation::new(true);
            cache.observe(&sim, 0, Some(32.), false);
            super::sample_weapon_wave_draw(old, 7, &cache, 1, [7, 11]);
            let mut current = old;
            let points: &mut [[i32; 2]] = match &mut current.polygon {
                WavePolygon::Magnetic(points) => points,
                WavePolygon::Sonic(points) => points,
            };
            for point in points {
                point[0] += 12;
                point[1] += 8;
            }
            current.source_point = [113, 209];
            current.source_z = 104;
            current.phase = 4;
            sim.session_mut_for_test().set_binary_frame_for_test(2);
            cache.observe(&sim, 32, Some(32.), false);
            super::sample_weapon_wave_draw(current, 7, &cache, 2, [30, -4]);
            cache.observe(&sim, 48, Some(32.), false);
            let mid = super::sample_weapon_wave_draw(current, 7, &cache, 2, [30, -4]);
            for (actual, previous) in mid.polygon.points().iter().zip(old.polygon.points()) {
                assert_eq!(*actual, [previous[0] + 6 - 30, previous[1] + 4 + 4]);
            }
            assert_eq!(mid.source_point, [101 + 6 - 30, 201 + 4 + 4]);
            assert_eq!(mid.source_z, 52);
            assert_eq!(mid.phase, 4, "native lifetime is not interpolated");
            assert_eq!(mid.direction, current.direction);
            assert_eq!(mid.clip_path, current.clip_path);
            cache.set_enabled(false);
            let exact = super::sample_weapon_wave_draw(current, 7, &cache, 2, [0, 0]);
            assert_eq!(exact, current, "OFF must preserve native geometry exactly");
        }
    }

    /// The sprite is projected from the anim's exact Z. Half a level up lands
    /// between the two level rows; the old level-byte projection drew it on the
    /// lower row, and drew a 104-frame level-1 coordinate on row 0.
    #[test]
    fn anim_sprite_is_projected_at_its_exact_height() {
        let at = |z: i32| {
            super::anim_world_render_coords(crate::sim::anim_class::AnimWorldCoord {
                x: 10 * 256 + 128,
                y: 12 * 256 + 128,
                z,
            })
        };
        let (ground, half, level_one) = (at(0), at(52), at(104));
        assert_eq!((ground.0, ground.4), (level_one.0, 0));
        assert_eq!(level_one.4, 1, "104 leptons is height level 1");
        assert!(
            level_one.1 < half.1 && half.1 < ground.1,
            "screen Y rises with exact Z: {} < {} < {}",
            level_one.1,
            half.1,
            ground.1
        );
        // The lift handed to depth is exactly what the projection subtracted,
        // on and between levels, so the depth row is the ground row.
        assert_eq!(ground.5, 0);
        for lifted in [half, level_one] {
            assert_eq!(lifted.1 + lifted.5 as f32, ground.1);
        }
        assert_eq!(level_one.5, 15, "one level is one 15 px height step");
    }

    use super::{
        anim_instance_alpha, body_draw_skips_live_identity, ordinary_overlay_z,
        overlay_display_identity, overlay_render_identity, terrain_object_is_render_visible,
    };
    use crate::map::overlay::TerrainObject;
    use crate::render::native_z::ZGradient;
    use crate::render::tactical_draw_plan::RenderZPolicy;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::overlay_types::OverlayTypeFlags;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::tiberium_type::TiberiumTypeRegistry;
    use crate::sim::intern::StringInterner;
    use crate::sim::overlay_grid::OverlayCell;
    use crate::sim::terrain_object::{
        TerrainObjectLifecycle, TerrainObjectState, TerrainObjectStore,
    };

    /// The two identities 0047F6A0 never draws, read from retail through the
    /// production registry.
    #[test]
    fn body_draw_skips_the_two_retail_dummy_identities() {
        let Some((ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art_ini));
        for (overlay_id, name) in [(0xA7, "USELESS"), (0xB2, "VEINHOLEDUMMY")] {
            assert_eq!(registry.name(overlay_id), Some(name));
            assert!(body_draw_skips_live_identity(overlay_id), "{name}");
        }
        for drawn in [0xA6, 0xA8, 0xB1, 0xB3, 0xEF] {
            assert!(!body_draw_skips_live_identity(drawn), "{drawn:#x}");
        }
    }

    #[test]
    fn gsi_04_10_render_visibility_distinguishes_unregistered_live_and_destroyed() {
        let ini = IniFile::from_str(
            "[General]\n\
             FixtureOnly=1\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n0=DUMMY\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n0=TREE01\n\
             [DUMMY]\nStrength=100\n\
             [TREE01]\nFixtureOnly=1\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules");
        let registered = TerrainObject {
            rx: 4,
            ry: 7,
            name: "TREE01".to_string(),
        };
        let unregistered = TerrainObject {
            rx: 8,
            ry: 9,
            name: "MAPONLY".to_string(),
        };
        let mut terrain_objects = TerrainObjectStore::default();

        assert!(terrain_object_is_render_visible(&registered, None, None));
        assert!(terrain_object_is_render_visible(
            &unregistered,
            Some(&rules),
            Some(&terrain_objects),
        ));
        assert!(!terrain_object_is_render_visible(
            &registered,
            Some(&rules),
            Some(&terrain_objects),
        ));

        let mut interner = StringInterner::default();
        let stable_id = 1;
        terrain_objects
            .cell_index_mut_for_test()
            .insert((4, 7), stable_id);
        terrain_objects.objects_mut_for_test().insert(stable_id, {
            let mut terrain =
                TerrainObjectState::for_test(stable_id, interner.intern("TREE01"), 4, 7);
            terrain.set_health_for_test(10);
            terrain.set_max_health_for_test(10);
            terrain.set_occupation_bits_for_test(7);
            terrain
        });
        assert!(terrain_object_is_render_visible(
            &registered,
            Some(&rules),
            Some(&terrain_objects),
        ));

        terrain_objects
            .objects_mut_for_test()
            .get_mut(&stable_id)
            .unwrap()
            .set_lifecycle_for_test(TerrainObjectLifecycle::Destroyed);
        assert!(!terrain_object_is_render_visible(
            &registered,
            Some(&rules),
            Some(&terrain_objects),
        ));
    }

    #[test]
    fn ordinary_overlay_depth_uses_native_wall_and_nonwall_call_arguments() {
        // Parsed default matters: +0x2B3 starts true, but +0x2A8 Wall forces
        // gradient 2 even if DrawFlat is omitted or explicitly true.
        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str(
                "[OverlayTypes]\n0=WALL\n1=FLAT\n2=UPRIGHT\n3=ROCK\n4=ORE\n\
                 [WALL]\nWall=yes\n[FLAT]\n[UPRIGHT]\nDrawFlat=no\n\
                 [ROCK]\nDrawFlat=no\nIsARock=yes\n[ORE]\nTiberium=yes\nDrawFlat=no\n",
            ),
            None,
        );
        for (id, gradient, class_term) in [
            (0, ZGradient::Vertical, -2),
            (1, ZGradient::Flat, -2),
            (2, ZGradient::Vertical, -17),
            (3, ZGradient::Vertical, -2),
            (4, ZGradient::Flat, -2),
        ] {
            for level in [0, 1, 4, 14] {
                let depth = ordinary_overlay_z(id, registry.flags(id), 0, level).unwrap();
                assert_eq!(depth.render_z, RenderZPolicy::ReadWrite);
                assert_eq!(
                    depth.gradient, gradient,
                    "ordinary overlays have no BUILDNGZ"
                );
                assert_eq!(depth.z_adjust as i32, class_term - i32::from(level) * 15);
            }
        }
    }

    #[test]
    fn retail_slope_bodies_use_read_write_depth() {
        // Cell47F943 (sloped ore) and literal7E both request flags4E00.
        // DUMMYOLD is the retail identity7E, independent of vein flags.
        let Some((ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art_ini));
        for name in ["TIB01", "DUMMYOLD"] {
            let id = registry.id_for_name(name).expect("retail overlay");
            assert!(!body_draw_skips_live_identity(id));
            for slope in 0..=4 {
                let depth = ordinary_overlay_z(id, registry.flags(id), slope, 5).unwrap();
                assert_eq!(
                    depth.render_z,
                    RenderZPolicy::ReadWrite,
                    "{name}, slope{slope}"
                );
                assert_eq!(
                    depth.shape.map(|shape| shape.offset),
                    (slope != 0).then_some(if id == 0x7E { [30, -2] } else { [0, 0] }),
                    "{name}, slope{slope}"
                );
            }
        }
    }

    #[test]
    fn vein_flags_do_not_bypass_depth_or_select_a_shape() {
        // The ordinary47FB6B arm does not test these legacy INI flags.
        // Supplied flags cover custom overlays; VEINHOLE is not a retail
        // OverlayTypes entry, and the retail VEINHOLEDUMMY body is skipped.
        for flags in [
            OverlayTypeFlags {
                is_veins: true,
                ..Default::default()
            },
            OverlayTypeFlags {
                is_veinhole_monster: true,
                ..Default::default()
            },
        ] {
            for slope in 0..=4 {
                let depth = ordinary_overlay_z(0, Some(&flags), slope, 5).unwrap();
                assert_eq!(depth.render_z, RenderZPolicy::ReadWrite);
                assert!(depth.shape.is_none());
            }
        }
    }

    /// Walls and fences (Wall=yes overlays) draw through a native Convert,
    /// so unexplored shroud's A=2 stores black instead of a dimmed silhouette.
    #[test]
    fn wall_overlay_convert_reads_shroud_a_like_other_overlays() {
        let grid = crate::map::lighting::CellLightGrid::new();
        let wall = OverlayTypeFlags {
            wall: true,
            ..Default::default()
        };
        let light = super::ordinary_overlay_palette_light(Some(&wall), &grid, (3, 4));
        assert_eq!(
            (light.rows(), light.brightness()),
            (53, 1000),
            "current-house ColorScheme Convert at the cell scalar"
        );
        let stone = [150, 110, 90];
        // SHROUD frame 15 stores A=2 in every unexplored pixel.
        assert_eq!(light.rgb565(stone, 100, 2), 0, "unexplored shroud");
        assert_ne!(light.rgb565(stone, 100, 127), 0, "explored ground");
        for other in [
            OverlayTypeFlags::default(),
            OverlayTypeFlags {
                tiberium: true,
                ..Default::default()
            },
        ] {
            let light = super::ordinary_overlay_palette_light(Some(&other), &grid, (3, 4));
            assert_eq!(light.rgb565(stone, 100, 2), 0);
        }
    }

    /// 0047F6A0's IsRubble arm: only the cell holding a rubble type draws,
    /// and only when that type has bound rubble art.
    #[test]
    fn rubble_draws_only_from_the_cell_recording_a_type_with_art() {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n0=RUIN\n1=BARE\n",
        ))
        .unwrap();
        rules.set_building_rubble_art_for_test(
            "RUIN",
            crate::rules::building_rubble::RubbleArt {
                shp: "ruin.shp".into(),
                body_frame: 3,
                shadow_frame: 7,
            },
        );
        let mut interner = crate::sim::intern::StringInterner::new();
        let ruin = interner.intern("RUIN");
        let bare = interner.intern("BARE");
        let cell = |rubble_building_type| OverlayCell {
            overlay_id: Some(crate::sim::overlay_grid::RUBBLE_OVERLAY_INDEX),
            overlay_data: 0,
            wall_owner: None,
            rubble_building_type,
        };
        let key = |live: OverlayCell| {
            super::rubble_sprite_key(Some(&live), Some(&rules), Some(&interner))
        };
        assert_eq!(
            key(cell(Some(ruin))),
            Some(crate::render::overlay_atlas::rubble_key("RUIN.SHP", 3))
        );
        assert_eq!(key(cell(None)), None, "other foundation cells");
        assert_eq!(key(cell(Some(bare))), None, "no rubble art bound");
    }

    #[test]
    fn gsi_04_13_low_overlay_renderer_follows_live_identity_through_terminal_collapse() {
        let mut live = OverlayCell {
            overlay_id: Some(0x50),
            overlay_data: 0xA5,
            wall_owner: None,
            rubble_building_type: None,
        };

        assert_eq!(overlay_render_identity(0x4A, 7, None), Some((0x4A, 7)));
        assert_eq!(
            overlay_render_identity(0x4A, 7, Some(&live)),
            Some((0x50, 0xA5)),
            "first-damaged low bridge must select the live overlay variant"
        );

        live.overlay_id = Some(0x64);
        assert_eq!(
            overlay_render_identity(0x4A, 7, Some(&live)),
            Some((0x64, 0xA5)),
            "terminal collapse remains a drawable overlay identity"
        );

        live.overlay_id = Some(0x4D);
        assert_eq!(
            overlay_render_identity(0x4A, 7, Some(&live)),
            Some((0x4D, 0xA5)),
            "repair art must follow the live healthy variant"
        );

        live.overlay_id = None;
        assert_eq!(overlay_render_identity(0x4A, 7, Some(&live)), None);

        live.overlay_id = Some(6);
        assert_eq!(
            overlay_render_identity(5, 3, Some(&live)),
            None,
            "ordinary overlays retain exact identity matching"
        );
    }

    #[test]
    fn resource_display_selection_preserves_live_density_for_flat_and_slope_cells() {
        let mut text = String::from(
            "[Tiberiums]\n0=Riparius\n1=Cruentus\n\
             [Riparius]\nImage=1\n\
             [Cruentus]\nImage=2\n\
             [OverlayTypes]\n",
        );
        let mut resource_names = Vec::new();
        for overlay_id in 0..=121 {
            let name = match overlay_id {
                27..=38 => format!("GEM{:02}", overlay_id - 26),
                102..=121 => format!("TIB{:02}", overlay_id - 101),
                _ => format!("FILL{overlay_id:03}"),
            };
            text.push_str(&format!("{overlay_id}={name}\n"));
            if matches!(overlay_id, 27..=38 | 102..=121) {
                resource_names.push(name);
            }
        }
        for name in resource_names {
            text.push_str(&format!("[{name}]\nTiberium=yes\n"));
        }
        let ini = IniFile::from_str(&text);
        let overlays = OverlayTypeRegistry::from_ini(&ini, None);
        let tiberiums = TiberiumTypeRegistry::from_ini(&ini);
        let tib12 = overlays.id_for_name("TIB12").expect("TIB12");
        let tib01 = overlays.id_for_name("TIB01").expect("TIB01");
        let tib05 = overlays.id_for_name("TIB05").expect("TIB05");
        let tib15 = overlays.id_for_name("TIB15").expect("TIB15");
        let gem12 = overlays.id_for_name("GEM12").expect("GEM12");
        let gem01 = overlays.id_for_name("GEM01").expect("GEM01");
        let non_tiberium = overlays.id_for_name("FILL000").expect("FILL000");

        assert_eq!(
            overlay_display_identity(tib12, 8, 4, 7, Some(0), Some(&overlays), Some(&tiberiums),),
            Some((tib05, 8))
        );
        assert_eq!(
            overlay_display_identity(gem12, 11, 0, 7, Some(0), Some(&overlays), Some(&tiberiums),),
            Some((gem01, 11))
        );
        assert_eq!(
            overlay_display_identity(tib12, 8, 4, 7, Some(2), Some(&overlays), Some(&tiberiums),),
            Some((tib15, 8)),
            "slope2 selects its first extra image without changing live density"
        );
        assert_eq!(
            overlay_display_identity(
                non_tiberium,
                9,
                4,
                7,
                Some(0),
                Some(&overlays),
                Some(&tiberiums),
            ),
            Some((non_tiberium, 9)),
            "non-resource overlays remain unchanged"
        );
        assert_eq!(
            overlay_display_identity(
                tib12,
                8,
                u16::MAX,
                1,
                Some(0),
                Some(&overlays),
                Some(&tiberiums),
            ),
            Some((tib01.checked_sub(1).expect("registered base - 1"), 8)),
            "signed base-relative display selection must not change live density state"
        );
    }

    #[test]
    fn translucent_yes_divides_the_loaded_end_and_wins_over_translucency() {
        // AnimClass::DrawIt tests Type+0x36A (`Translucent=`) before Type+0x2EC
        // (`Translucency=`), and its ladder divides End (Type+0x2C0,
        // `0x004230AC`), which a Shadow=yes SHP halves and `End=` overrides.
        let ini = IniFile::from_str(
            "[SMKPUFF]\n\
             Translucent=yes\n\
             Translucency=50\n\
             [SHADOWED]\n\
             Translucent=yes\n\
             Shadow=yes\n\
             [CAPPED]\n\
             Translucent=yes\n\
             End=10\n",
        );
        let mut reg = ArtRegistry::from_ini(&ini);
        reg.bind_anim_frame_count_for_test("SMKPUFF", 10);

        let smoke = reg.anim_runtime_config("SMKPUFF");
        assert_eq!(anim_instance_alpha(smoke, 0, 10), 1.0);
        assert_eq!(anim_instance_alpha(smoke, 9, 10), 0.25);

        let shadowed = reg.anim_runtime_config("SHADOWED");
        assert_eq!(anim_instance_alpha(shadowed, 2, 20), 1.0);
        assert_eq!(anim_instance_alpha(shadowed, 5, 20), 0.5);

        let capped = reg.anim_runtime_config("CAPPED");
        assert_eq!(anim_instance_alpha(capped, 5, 100), 0.5);
    }
}
