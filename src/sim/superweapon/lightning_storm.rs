//! Lightning Storm state machine — bolt generation and area damage.
//!
//! Only one storm can be active globally at a time. The storm has a deferment
//! countdown before bolts begin, then generates center + scatter bolts each
//! tick for the configured duration.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/components, sim/combat.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_aoe::{
    AoELayerContext, TerrainCollectionView, apply_aoe_damage_with_terrain_and_scenario,
    bridge_adjusted_impact_z,
};
use crate::sim::intern::InternedId;
use crate::sim::world::{SimSoundEvent, Simulation};

/// Lightning storm bolt animation names (WeatherConBolts from art.ini).
const BOLT_ANIMS: &[&str] = &["WCLBOLT1", "WCLBOLT2", "WCLBOLT3"];

/// Maximum retry attempts for scatter bolt placement (avoid infinite loop).
const MAX_SCATTER_RETRIES: u32 = 10;

/// Rust-native representation of the explicit post-duration ending turn.
/// `-1` remains the native infinite-duration sentinel.
const ENDING_DURATION_SENTINEL: i32 = i32::MIN;

/// Active lightning storm state.
///
/// Global — only one storm at a time (per original engine).
/// Stored as `Simulation.lightning_storm: Option<LightningStormState>`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LightningStormState {
    /// House that launched the storm.
    pub owner: InternedId,
    /// Storm center cell X.
    pub target_rx: u16,
    /// Storm center cell Y.
    pub target_ry: u16,
    /// Ticks remaining before bolts begin (deferment countdown).
    pub deferment_remaining: i32,
    /// Ticks remaining for active bolt generation.
    pub duration_remaining: i32,
    /// Ticks until next center bolt.
    pub center_bolt_timer: i32,
    /// Ticks until next scatter bolt.
    pub scatter_bolt_timer: i32,
    /// Last bolt cell X (for separation enforcement).
    pub last_bolt_rx: u16,
    /// Last bolt cell Y (for separation enforcement).
    pub last_bolt_ry: u16,
}

/// The storm actually begins — the non-deferred half of
/// `LightningStorm::Start @ 0x00539EB0`, which flips the sky and plays the
/// `StormSound` cue at `0x0053A044` in that order. Every path that reaches the
/// beginning goes through here so the lighting and the cue cannot separate:
/// in gamemd they are two statements of one straight-line block, reached only
/// once the deferment countdown is zero.
fn begin(sim: &mut Simulation) {
    sim.select_lighting_profile(crate::sim::scenario_session::ScenarioLightingProfile::Ion);
    sim.sound_events.push(SimSoundEvent::LightningStormBegan);
}

/// Start a new lightning storm. An overlapping invocation retargets the one
/// global storm without creating a second queued lifetime.
pub fn start(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    target_rx: u16,
    target_ry: u16,
    sw_type: InternedId,
) -> bool {
    if let Some(storm) = sim.lightning_storm.as_mut() {
        storm.owner = owner;
        storm.target_rx = target_rx;
        storm.target_ry = target_ry;
        let begins_now = if storm.deferment_remaining > 0 {
            let requested_deferment = rules.general.lightning_deferment;
            if requested_deferment <= storm.deferment_remaining {
                storm.deferment_remaining = requested_deferment;
            }
            storm.duration_remaining = rules.general.lightning_storm_duration;
            log::info!("Deferred Lightning Storm retargeted to ({target_rx}, {target_ry})");
            storm.deferment_remaining <= 0
        } else {
            log::info!("Active Lightning Storm retargeted to ({target_rx}, {target_ry})");
            false
        };
        if begins_now {
            begin(sim);
        }
        return true;
    }

    let state = LightningStormState {
        owner,
        target_rx,
        target_ry,
        deferment_remaining: rules.general.lightning_deferment,
        duration_remaining: rules.general.lightning_storm_duration,
        center_bolt_timer: rules.general.lightning_hit_delay,
        scatter_bolt_timer: rules.general.lightning_scatter_delay,
        last_bolt_rx: target_rx,
        last_bolt_ry: target_ry,
    };

    let starts_active = state.deferment_remaining <= 0;
    sim.lightning_storm = Some(state);
    if starts_active {
        // `LightningDeferment=0` is the only way the launch call reaches the
        // cue: `Start`'s `if (param_2 != 0)` early return is skipped and the
        // whole block runs inside `SuperClass::Launch` itself. Native plays it
        // before the case-2 EVA line at `0x006CCD81`, hence this order.
        begin(sim);
    }

    // `EVA_LightningStormCreated` — `0x006CCD81`, played by case 2 after
    // `LightningStorm::Start` returns, deferred or not.
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx: target_rx,
        ry: target_ry,
    });

    log::info!(
        "Lightning Storm started at ({}, {}) by '{}', deferment={} duration={}",
        target_rx,
        target_ry,
        sim.interner.resolve(owner),
        rules.general.lightning_deferment,
        rules.general.lightning_storm_duration,
    );

    true
}

/// Process the active lightning storm for one tick.
/// Called from `tick_active_superweapon_effects()` each tick, which is what
/// `World::advance_tick` reaches in production.
pub fn process(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) {
    // Phase 1: deferment countdown.
    let activates_now = match sim.lightning_storm.as_mut() {
        None => return,
        Some(storm) if storm.deferment_remaining > 0 => {
            storm.deferment_remaining -= 1;
            if storm.deferment_remaining > 0 {
                return;
            }
            true
        }
        Some(_) => false,
    };
    if activates_now {
        // `LightningStorm::Process @ 0x0053AAAD` decrements the countdown and
        // at zero re-enters `Start` with `param_2` cleared (`0x0053AAC8 XOR
        // EDX,EDX`), so this is the frame that runs the non-deferred block —
        // Ion lighting and `StormSound`. On stock data (`LightningDeferment=
        // 250`) this is the only frame the cue is ever heard on.
        begin(sim);
        // Native Process calls Start and returns on the countdown-zero frame;
        // expiry and bolt cadence begin on the next object tick.
        return;
    }

    // Phase 2: active storm. A positive countdown owns exactly that many
    // complete processing turns and cleanup occurs on the following turn.
    // The native -1 sentinel stays active indefinitely.
    let duration = sim
        .lightning_storm
        .as_ref()
        .expect("storm remains present after deferment processing")
        .duration_remaining;
    if duration == ENDING_DURATION_SENTINEL {
        log::info!("Lightning Storm ended");
        sim.lightning_storm = None;
        sim.select_lighting_profile(crate::sim::scenario_session::ScenarioLightingProfile::Normal);
        return;
    }
    if duration == 0 {
        // Native first enters its ending state and returns. With no modeled
        // cloud objects, the following Process is the earliest cleanup turn.
        sim.lightning_storm
            .as_mut()
            .expect("storm remains present while entering ending state")
            .duration_remaining = ENDING_DURATION_SENTINEL;
        return;
    }

    let storm = sim
        .lightning_storm
        .as_mut()
        .expect("active storm remains present");

    // Extract storm fields for bolt generation (avoid borrow conflict).
    let target_rx = storm.target_rx;
    let target_ry = storm.target_ry;
    let last_rx = storm.last_bolt_rx;
    let last_ry = storm.last_bolt_ry;
    let owner = storm.owner;

    // Center bolt
    storm.center_bolt_timer -= 1;
    let spawn_center = storm.center_bolt_timer <= 0;
    if spawn_center {
        storm.center_bolt_timer = rules.general.lightning_hit_delay;
    }

    // Scatter bolt
    storm.scatter_bolt_timer -= 1;
    let spawn_scatter = storm.scatter_bolt_timer <= 0;
    if spawn_scatter {
        storm.scatter_bolt_timer = rules.general.lightning_scatter_delay;
    }

    let spread = rules.general.lightning_cell_spread;
    let separation = rules.general.lightning_separation;

    if spawn_center {
        spawn_bolt(sim, rules, target_rx, target_ry, owner, overlay_registry);
    }

    if spawn_scatter {
        let (rx, ry) = pick_scatter_cell(
            sim, target_rx, target_ry, last_rx, last_ry, spread, separation,
        );
        spawn_bolt(sim, rules, rx, ry, owner, overlay_registry);
        // Update last bolt position on the storm state.
        if let Some(ref mut storm) = sim.lightning_storm {
            storm.last_bolt_rx = rx;
            storm.last_bolt_ry = ry;
        }
    }

    if let Some(storm) = sim.lightning_storm.as_mut()
        && storm.duration_remaining > 0
    {
        storm.duration_remaining -= 1;
    }
}

/// Pick a random cell within `spread` of the storm center, enforcing
/// `separation` manhattan distance from the last bolt.
fn pick_scatter_cell(
    sim: &mut Simulation,
    center_rx: u16,
    center_ry: u16,
    last_rx: u16,
    last_ry: u16,
    spread: i32,
    separation: i32,
) -> (u16, u16) {
    let diameter = (spread * 2 + 1) as u32;
    for _ in 0..MAX_SCATTER_RETRIES {
        // Random offset within [-spread, +spread] for both axes.
        let dx = sim.superweapon_rng().next_range_u32(diameter) as i32 - spread;
        let dy = sim.superweapon_rng().next_range_u32(diameter) as i32 - spread;
        let rx = (center_rx as i32 + dx).max(0) as u16;
        let ry = (center_ry as i32 + dy).max(0) as u16;

        // Check manhattan distance from last bolt.
        let manhattan = (rx as i32 - last_rx as i32).abs() + (ry as i32 - last_ry as i32).abs();
        if manhattan >= separation {
            return (rx, ry);
        }
    }
    // Fallback: use the last attempted position (avoids infinite loop).
    let dx = sim.superweapon_rng().next_range_u32(diameter) as i32 - spread;
    let dy = sim.superweapon_rng().next_range_u32(diameter) as i32 - spread;
    (
        (center_rx as i32 + dx).max(0) as u16,
        (center_ry as i32 + dy).max(0) as u16,
    )
}

/// Spawn a single lightning bolt at the given cell: visual effect + area damage.
fn spawn_bolt(
    sim: &mut Simulation,
    rules: &RuleSet,
    rx: u16,
    ry: u16,
    owner: InternedId,
    overlay_registry: Option<&OverlayTypeRegistry>,
) {
    let handles = sim.rule_handles;
    // 1. Pick a random bolt animation.
    let anim_idx = sim
        .superweapon_rng()
        .next_range_u32(BOLT_ANIMS.len() as u32) as usize;
    let anim_name = BOLT_ANIMS[anim_idx];
    // `LightningStorm::GroundStrike @ 0x0053A300` constructs the bolt at the
    // cell's centre coordinate with the row `(type, &coord, 0, 1, 0x600, 0, 0)`
    // (`0x0053A387`), the same row as the superweapon invoke animations.
    super::spawn_cell_anim(sim, rules, anim_name, rx, ry, false);

    // 2. Apply area damage via lightning warhead.
    let warhead_id = &rules.general.lightning_warhead;
    if let Some(warhead) = rules.warhead(warhead_id) {
        let warhead_ref = sim.interner.intern(warhead_id);
        let impact_z = bridge_adjusted_impact_z(sim.resolved_terrain.as_ref(), rx, ry);
        let air_impact = crate::sim::combat::combat_aoe::air_impact_from_layer_z(
            sim.resolved_terrain.as_ref(),
            rx,
            ry,
            crate::util::lepton::CELL_CENTER_LEPTON,
            crate::util::lepton::CELL_CENTER_LEPTON,
            impact_z,
        );
        let world_z_leptons = air_impact
            .map(|impact| impact.z_leptons)
            .unwrap_or_else(|| {
                impact_z.wrapping_mul(crate::util::lepton::LEPTONS_PER_LEVEL as i32)
            });

        // GroundStrike selects and starts its explosion AnimClass before it
        // enters Apply_area_damage; the anim's scorch or crater is its own
        // Middle, at its middle frame.
        let mut explosions: Vec<crate::sim::combat::ExplosionEffect> = Vec::new();
        crate::sim::combat::emit_warhead_detonation_effects(
            warhead,
            rules.general.lightning_damage,
            rx,
            ry,
            crate::util::lepton::CELL_CENTER_LEPTON,
            crate::util::lepton::CELL_CENTER_LEPTON,
            crate::sim::combat::impact_z_byte(impact_z),
            world_z_leptons,
            &mut sim.interner,
            &mut explosions,
        );
        // The strike's explosion is the ordinary warhead `AnimList=` pick with
        // the ordinary row (`0x0053A50E`: drawFlags 0x2600, the `0x0048ACE0`
        // zAdjust), so it takes the combat explosion constructor.
        for fx in &explosions {
            sim.spawn_combat_explosion_anim(
                rules,
                fx.shp_name,
                fx.rx,
                fx.ry,
                fx.sub_x,
                fx.sub_y,
                fx.z,
                fx.world_z,
            );
        }

        let scenario_no_damage = sim.session.no_damage;
        let binary_frame = sim.session.binary_frame;
        let spread_enabled = sim.production.ore_growth_config.spreads;
        let mut cell_prelude = crate::sim::world::simulation_area_damage_cell_prelude(
            rules,
            warhead,
            rules.general.lightning_damage,
            true,
            scenario_no_damage,
            &mut sim.production.ore_growth_state,
            &sim.production.tiberium_spawning_terrain_cells,
            &sim.production.terrain_object_cells,
            binary_frame,
            spread_enabled,
            &mut sim.radar_terrain_dirty_cells,
            &mut sim.radar_terrain_dirty_generation,
            &mut sim.tactical_dirty_cells,
            &mut sim.terrain_costs,
            &mut sim.zone_grid,
            &mut sim.path_grid,
            sim.bridge_state.as_ref(),
            sim.playfield_bounds,
        );
        let terrain_objects = TerrainCollectionView {
            objects: &sim.production.terrain_objects,
            cells: &sim.production.terrain_object_cells,
        };
        let aoe = apply_aoe_damage_with_terrain_and_scenario(
            &mut sim.substrate.entities,
            rx,
            ry,
            rules.general.lightning_damage,
            warhead,
            rules,
            &sim.interner,
            handles,
            (
                crate::sim::combat::RAD_NO_ATTACKER,
                Some(owner),
                warhead_ref,
            ),
            AoELayerContext {
                occupancy: Some(&sim.substrate.occupancy),
                terrain: sim.resolved_terrain.as_mut(),
                overlay_grid: sim.overlay_grid.as_mut(),
                overlay_registry,
                scenario_rng: Some(&mut sim.scenario_rng),
                air_impact,
                impact_z,
            },
            Some(terrain_objects),
            scenario_no_damage,
            Some(&mut cell_prelude as &mut dyn crate::sim::combat::combat_aoe::AoECellPrelude),
        );
        let receivers = aoe.receivers;
        drop(cell_prelude);

        // IonWH has Wall=yes in active retail. The borrowed cell prelude has
        // already published every native tactical/radar callback inline.

        // GroundStrike enters the ordinary ReceiveDamage transaction for each
        // hit before returning. In particular, a fatal carrier detonates its
        // DeathWeapon (and mutates walls/RNG/targets) before the next bolt or
        // LogicClass visit.
        sim.commit_noncombat_aoe_receivers(rules, overlay_registry, &receivers);
    } else {
        log::warn!("Lightning warhead '{}' not found in rules", warhead_id);
    }

    // 3. Sound event for the bolt strike.
    sim.sound_events
        .push(SimSoundEvent::SuperWeaponStrike { rx, ry });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts};
    use crate::map::entities::EntityCategory;
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::CellListInsertion;
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::rng::SimRng;
    use crate::sim::scenario_session::ScenarioLightingProfile;
    use crate::sim::world::Simulation;

    fn lighting_timing_rules(deferment: i32, duration: i32, rate: &str) -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\n\
             LightningDeferment={deferment}\n\
             LightningStormDuration={duration}\n\
             LightningHitDelay=1000\n\
             LightningScatterDelay=1000\n\
             AmbientChangeRate={rate}\n\
             AmbientChangeStep=.2\n"
        )))
        .expect("lighting timing rules should parse")
    }

    #[test]
    fn gsi_04_20_deferment_activation_and_cleanup_follow_pre_ore_ambient_rung() {
        let rules = lighting_timing_rules(1, 2, ".0012");
        assert_eq!(rules.general.ambient_change_interval_frames, 1);
        let mut sim = Simulation::with_seed(0x420);
        let owner = sim.interner.intern("Americans");
        let heights = std::collections::BTreeMap::new();
        let rng_before = sim.scenario_rng.state();

        let sw_test = sim.interner.intern("SWTEST");
        assert!(start(&mut sim, &rules, owner, 8, 9, sw_test));
        assert_eq!(
            sim.session.lighting.selected_profile,
            ScenarioLightingProfile::Normal,
            "a deferred request must not select Ion"
        );

        sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
        assert_eq!(
            sim.session.lighting.selected_profile,
            ScenarioLightingProfile::Ion,
            "the decrement-to-zero frame activates the storm"
        );
        assert_eq!(sim.session.lighting.target_ambient, 87);
        assert_eq!(
            sim.session.lighting.current_ambient, 100,
            "the pre-ore ambient rung already ran before activation"
        );

        sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
        assert!(sim.lightning_storm.is_some());
        assert_eq!(sim.session.lighting.current_ambient, 87);
        assert_eq!(
            sim.lightning_storm
                .as_ref()
                .expect("first active duration turn")
                .duration_remaining,
            1
        );

        sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
        assert!(sim.lightning_storm.is_some());
        assert_eq!(
            sim.lightning_storm
                .as_ref()
                .expect("storm remains through both duration turns")
                .duration_remaining,
            0
        );
        assert_eq!(
            sim.session.lighting.selected_profile,
            ScenarioLightingProfile::Ion
        );

        sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
        assert_eq!(
            sim.lightning_storm
                .as_ref()
                .expect("explicit ending turn retains the storm")
                .duration_remaining,
            ENDING_DURATION_SENTINEL
        );
        assert_eq!(
            sim.session.lighting.selected_profile,
            ScenarioLightingProfile::Ion,
            "the explicit ending turn retains Ion lighting"
        );

        sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
        assert!(sim.lightning_storm.is_none());
        assert_eq!(
            sim.session.lighting.selected_profile,
            ScenarioLightingProfile::Normal
        );
        assert_eq!(sim.session.lighting.target_ambient, 100);
        assert_eq!(
            sim.session.lighting.current_ambient, 87,
            "cleanup selects Normal after this frame's ambient rung"
        );

        sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
        assert_eq!(sim.session.lighting.current_ambient, 100);
        assert_eq!(sim.scenario_rng.state(), rng_before);
    }

    fn storm_began_count(sim: &Simulation) -> usize {
        sim.sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::LightningStormBegan))
            .count()
    }

    fn launch_announced_count(sim: &Simulation) -> usize {
        sim.sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::SuperWeaponLaunched { .. }))
            .count()
    }

    /// `StormSound` is deferred. `SuperClass::Launch @ 0x006CC390` case 2
    /// hands `LightningStorm::Start @ 0x00539EB0` the `[Rules+0x1794]`
    /// `LightningDeferment` value as `param_2`, and `Start` returns at
    /// `if (param_2 != 0) { arm the countdown; return; }` — before the cue at
    /// `0x0053A044`. `LightningStorm::Process @ 0x0053A6C0` decrements the
    /// countdown (`0x0053AAAD`) and at zero re-enters `Start` with `param_2`
    /// cleared (`0x0053AAC8 XOR EDX,EDX ; 0x0053AACA CALL 0x00539EB0`); that
    /// second entry is what plays it. Stock `rulesmd.ini:130` is
    /// `LightningDeferment=250`, so on retail data the cue never lands on the
    /// launch frame — only the EVA line does.
    #[test]
    fn the_storm_cue_lands_on_the_deferment_expiry_not_on_the_launch() {
        let rules = lighting_timing_rules(3, 2, ".2");
        let mut sim = Simulation::with_seed(0x422);
        let owner = sim.interner.intern("Americans");
        let sw_test = sim.interner.intern("LightningStormSpecial");

        assert!(start(&mut sim, &rules, owner, 8, 9, sw_test));
        assert_eq!(
            storm_began_count(&sim),
            0,
            "a deferred launch returns before the cue"
        );
        assert_eq!(
            launch_announced_count(&sim),
            1,
            "the EVA line is still spoken at launch (0x006CCD81)"
        );
        assert_eq!(
            sim.session.lighting.selected_profile,
            ScenarioLightingProfile::Normal
        );

        for frame in 1..=2 {
            process(&mut sim, &rules, None);
            assert_eq!(
                storm_began_count(&sim),
                0,
                "countdown frame {frame} has not reached zero"
            );
        }

        process(&mut sim, &rules, None);
        assert_eq!(
            storm_began_count(&sim),
            1,
            "the countdown-zero frame re-enters Start and plays the cue"
        );
        assert_eq!(
            sim.session.lighting.selected_profile,
            ScenarioLightingProfile::Ion,
            "the cue and the Ion flip are one block in Start"
        );

        process(&mut sim, &rules, None);
        assert_eq!(
            storm_began_count(&sim),
            1,
            "the non-deferred block runs once per storm"
        );
    }

    /// The one launch that does reach the cue: `LightningDeferment=0` skips
    /// `Start`'s early return, so the whole non-deferred block runs inside
    /// `SuperClass::Launch` itself, before the case-2 EVA line.
    #[test]
    fn a_zero_deferment_storm_plays_its_cue_on_the_launch_frame() {
        let rules = lighting_timing_rules(0, 2, ".2");
        let mut sim = Simulation::with_seed(0x423);
        let owner = sim.interner.intern("Americans");
        let sw_test = sim.interner.intern("LightningStormSpecial");

        assert!(start(&mut sim, &rules, owner, 8, 9, sw_test));
        assert_eq!(storm_began_count(&sim), 1);
        assert_eq!(launch_announced_count(&sim), 1);
        let cue_first = sim
            .sound_events
            .iter()
            .position(|event| matches!(event, SimSoundEvent::LightningStormBegan))
            < sim
                .sound_events
                .iter()
                .position(|event| matches!(event, SimSoundEvent::SuperWeaponLaunched { .. }));
        assert!(
            cue_first,
            "native calls Start before the EVA line at 0x006CCD81"
        );
    }

    #[test]
    fn gsi_04_20_minus_one_duration_remains_active_and_ion_selected() {
        let rules = lighting_timing_rules(0, -1, ".2");
        let mut sim = Simulation::with_seed(0x421);
        let owner = sim.interner.intern("Americans");

        let sw_test = sim.interner.intern("SWTEST");
        assert!(start(&mut sim, &rules, owner, 8, 9, sw_test));
        for _ in 0..4 {
            process(&mut sim, &rules, None);
            assert_eq!(
                sim.lightning_storm
                    .as_ref()
                    .expect("-1 storm remains active")
                    .duration_remaining,
                -1
            );
            assert_eq!(
                sim.session.lighting.selected_profile,
                ScenarioLightingProfile::Ion
            );
        }
    }

    #[test]
    fn gsi_04_20_deferred_retarget_preserves_earliest_countdown_and_rewrites_duration() {
        let first_rules = lighting_timing_rules(5, 20, ".2");
        let second_rules = lighting_timing_rules(9, 37, ".2");
        let mut sim = Simulation::with_seed(0x422);
        let first_owner = sim.interner.intern("Americans");
        let second_owner = sim.interner.intern("Soviet");

        let sw_test = sim.interner.intern("SWTEST");
        assert!(start(&mut sim, &first_rules, first_owner, 4, 5, sw_test));
        sim.lightning_storm
            .as_mut()
            .expect("deferred storm")
            .deferment_remaining = 3;
        let sw_test = sim.interner.intern("SWTEST");
        assert!(start(
            &mut sim,
            &second_rules,
            second_owner,
            17,
            19,
            sw_test
        ));

        let storm = sim.lightning_storm.as_ref().expect("one deferred storm");
        assert_eq!(storm.owner, second_owner);
        assert_eq!((storm.target_rx, storm.target_ry), (17, 19));
        assert_eq!(storm.deferment_remaining, 3);
        assert_eq!(storm.duration_remaining, 37);
        assert_eq!(
            sim.session.lighting.selected_profile,
            ScenarioLightingProfile::Normal
        );
    }

    #[test]
    fn gsi_04_20_active_storm_start_retargets_without_a_queued_lifetime() {
        let rules = lighting_timing_rules(0, 20, ".2");
        let mut sim = Simulation::with_seed(1);
        let first_owner = sim.interner.intern("Americans");
        let second_owner = sim.interner.intern("Soviet");

        let sw_test = sim.interner.intern("SWTEST");
        assert!(start(&mut sim, &rules, first_owner, 4, 5, sw_test));
        assert_eq!(
            sim.session.lighting.selected_profile,
            ScenarioLightingProfile::Ion
        );
        let duration = sim.lightning_storm.as_ref().unwrap().duration_remaining;

        let sw_test = sim.interner.intern("SWTEST");
        assert!(start(&mut sim, &rules, second_owner, 17, 19, sw_test));
        let storm = sim
            .lightning_storm
            .as_ref()
            .expect("one storm remains active");
        assert_eq!(storm.owner, second_owner);
        assert_eq!((storm.target_rx, storm.target_ry), (17, 19));
        assert_eq!(storm.duration_remaining, duration);
    }

    #[test]
    fn gsi_04_20_static_normal_map_does_not_advance_lighting_or_rng() {
        let rules = lighting_timing_rules(250, 180, ".2");
        let mut sim = Simulation::with_seed(0x42);
        let lighting_before = sim.session.lighting;
        let rng_before = sim.scenario_rng.state();
        let heights = std::collections::BTreeMap::new();

        for _ in 0..400 {
            sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
        }

        assert_eq!(sim.session.lighting, lighting_before);
        assert_eq!(sim.scenario_rng.state(), rng_before);
    }

    fn registry_only_warhead_lightning_test_setup() -> (Simulation, RuleSet) {
        // LWH is declared only by [Warheads]; no object or ordinary weapon
        // points to it. This mirrors stock IonWH's Lightning Storm route.
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=DUMMY\n\n\
             [VehicleTypes]\n\n\
             [AircraftTypes]\n\n\
             [BuildingTypes]\n0=GAPOWR\n\n\
             [Warheads]\n0=LWH\n\n\
             [DUMMY]\nStrength=100\nArmor=none\nSpeed=4\n\n\
             [GAPOWR]\nStrength=200\nArmor=wood\n\n\
             [General]\nLightningDamage=100\nLightningWarhead=LWH\n\n\
             [LWH]\nCellSpread=1\nPercentAtMax=1\nAnimList=EXPLOSION\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("lightning test rules should parse");
        let sim = Simulation::with_seed(1);
        (sim, rules)
    }

    #[test]
    fn gsi_04_11_registry_only_lightning_anim_smudge_is_not_deferred() {
        let (mut sim, mut rules) = registry_only_warhead_lightning_test_setup();
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
            "[EXPLOSION]
Rate=900
[WCLBOLT1]
Layer=ground
[WCLBOLT2]
Layer=ground
\n             [WCLBOLT3]
Layer=ground
",
        ));
        for name in ["EXPLOSION", "WCLBOLT1", "WCLBOLT2", "WCLBOLT3"] {
            art.bind_anim_frame_count_for_test(name, 10);
        }
        rules.art_registry = art;
        let owner = sim.interner.intern("Americans");

        spawn_bolt(&mut sim, &rules, 5, 5, owner, None);

        assert!(sim.pending_smudge_requests.is_empty());

        // Bolt and explosion are both real AnimClass instances at the struck
        // cell; no second animation lane receives either.
        let explosion_iid = sim.interner.intern("EXPLOSION");
        let at_cell = |anim: &crate::sim::anim_class::AnimObject| {
            let (rx, ry, ..) = anim.world_coord.to_cell_sub_z();
            (rx, ry) == (5, 5)
        };
        let anims: Vec<_> = sim.substrate.anims.iter().map(|(_, anim)| anim).collect();
        assert!(
            anims.iter().any(|anim| anim.type_id == explosion_iid
                && at_cell(anim)
                && anim.draw_flags == 0x2600),
            "the lightning warhead's AnimList pick takes the combat explosion row"
        );
        assert!(
            anims.iter().any(|anim| {
                sim.interner.resolve(anim.type_id).starts_with("WCLBOLT")
                    && at_cell(anim)
                    && anim.draw_flags == 0x600
            }),
            "the bolt takes the (0, 1, 0x600, 0, 0) row"
        );
    }

    #[test]
    fn gsi_04_11_lightning_anim_precedes_per_cell_ore_reduction() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=LWH\n\
             [OverlayTypes]\n0=ORE\n\
             [SmudgeTypes]\n0=CR1\n\
             [Tiberiums]\n0=Riparius\n\
             [General]\nLightningDamage=100\nLightningWarhead=LWH\n\
             [LWH]\nCellSpread=0\nAnimList=EXPLOSION\nTiberium=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [ORE]\nTiberium=yes\nChainReaction=yes\n\
             [Riparius]\nImage=1\nValue=25\n\
             [CR1]\nCrater=yes\nWidth=1\nHeight=1\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("ore-order lightning rules");
        rules.art_registry = ArtRegistry::from_ini(&IniFile::from_str(
            "[EXPLOSION]\nCrater=yes\nScorch=no\nFrameWidth=100\nFrameHeight=100\n",
        ));
        let overlay_registry = OverlayTypeRegistry::from_ini(&ini, None);
        let ore_id = overlay_registry.id_for_name("ORE").expect("ORE overlay id");

        let mut sim = Simulation::with_seed(1);
        let mut cells = Vec::new();
        for ry in 0..10 {
            for rx in 0..10 {
                let mut cell = test_terrain_cell(rx, ry);
                cell.filled_clear = true;
                cell.accepts_smudge = true;
                cell.allows_tiberium = true;
                cells.push(cell);
            }
        }
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(10, 10, cells));
        sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(10, 10));
        sim.production.ore_growth_state = crate::sim::ore_growth::OreGrowthState::new(10, 10);
        let mut overlay = OverlayGrid::new(10, 10);
        // Raw data 9 represents ten density units. The pre-AoE crater reduces
        // six but remains blocked by the surviving overlay; Damage=100 then
        // clears the four units left by AnimClass::Start.
        overlay.place_overlay(5, 5, ore_id, 9);
        sim.overlay_grid = Some(overlay);
        let owner = sim.interner.intern("Americans");

        spawn_bolt(&mut sim, &rules, 5, 5, owner, Some(&overlay_registry));

        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
            None,
            "the later GroundStrike area pass still clears the partially reduced ore"
        );
        assert!(
            sim.smudge_grid
                .as_ref()
                .unwrap()
                .cell(5, 5)
                .type_id
                .is_none(),
            "GroundStrike starts its crater Anim while dense ore still blocks placement"
        );
        assert!(sim.pending_smudge_requests.is_empty());

        let mut expected_rng = SimRng::new(1);
        let _ = expected_rng.next_range_u32(BOLT_ANIMS.len() as u32);
        assert_eq!(sim.scenario_rng.state(), expected_rng.state());
    }

    #[test]
    fn active_retail_lightning_wall_removal_publishes_navigation_and_radar_inline() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=IonWH\n\
             [OverlayTypes]\n0=TESTWALL\n\
             [General]\nLightningDamage=100\nLightningWarhead=IonWH\n\
             [IonWH]\nCellSpread=0\nWall=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=1\n",
        );
        let art = IniFile::from_str("[TESTWALL]\nDamageLevels=2\n");
        let rules = RuleSet::from_ini(&ini).expect("active-retail lightning wall rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));
        let wall_id = registry
            .id_for_name("TESTWALL")
            .expect("test wall overlay id");

        let mut cells = Vec::new();
        for ry in 0..12 {
            for rx in 0..12 {
                cells.push(test_terrain_cell(rx, ry));
            }
        }
        let mut terrain = ResolvedTerrainGrid::from_cells(12, 12, cells);
        let mut overlays = OverlayGrid::new(12, 12);
        overlays.place_overlay(5, 5, wall_id, 0);
        assert!(crate::sim::overlay_grid::recalc_overlay_passability(
            &mut overlays,
            &mut terrain,
            &registry,
            5,
            5,
        ));
        let _ = overlays.take_dirty_cells();

        let mut sim = Simulation::with_seed(1);
        sim.overlay_grid = Some(overlays);
        sim.resolved_terrain = Some(terrain);
        assert!(sim.rebuild_dynamic_navigation(&rules));
        assert!(!sim.path_grid().unwrap().is_walkable(5, 5));
        let ground_zone_before = sim
            .zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(crate::rules::locomotor_type::MovementZone::Normal))
            .expect("normal zone map")
            .zone_at(5, 5, MovementLayer::Ground);
        assert_eq!(
            ground_zone_before,
            crate::sim::pathfinding::zone_map::ZONE_INVALID
        );

        let owner = sim.interner.intern("Americans");
        spawn_bolt(&mut sim, &rules, 5, 5, owner, Some(&registry));

        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
            None
        );
        assert!(sim.path_grid().unwrap().is_walkable(5, 5));
        let ground_zone_after = sim
            .zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(crate::rules::locomotor_type::MovementZone::Normal))
            .expect("normal zone map")
            .zone_at(5, 5, MovementLayer::Ground);
        assert_ne!(
            ground_zone_after,
            crate::sim::pathfinding::zone_map::ZONE_INVALID
        );
        assert_eq!(
            sim.radar_terrain_dirty_cells,
            vec![
                (5, 5),
                (5, 3),
                (6, 4),
                (4, 4),
                (5, 4),
                (4, 6),
                (3, 5),
                (4, 5),
                (6, 6),
                (5, 7),
                (5, 6),
                (7, 5),
                (6, 5),
            ]
        );
        assert_eq!(sim.radar_terrain_dirty_generation, 13);
        assert_eq!(
            sim.tactical_dirty_cells,
            vec![
                (5, 5),
                (5, 3),
                (6, 4),
                (5, 5),
                (4, 4),
                (5, 4),
                (4, 4),
                (5, 5),
                (4, 6),
                (3, 5),
                (4, 5),
                (5, 5),
                (6, 6),
                (5, 7),
                (4, 6),
                (5, 6),
                (6, 4),
                (7, 5),
                (6, 6),
                (5, 5),
                (6, 5),
            ]
        );
    }

    #[test]
    fn lightning_bridge_strike_damages_only_bridge_layer() {
        let (mut sim, rules) = registry_only_warhead_lightning_test_setup();
        add_same_cell_bridge_targets(&mut sim, "DUMMY");
        let owner = sim.interner.intern("Americans");

        spawn_bolt(&mut sim, &rules, 5, 5, owner, None);

        assert_eq!(
            sim.substrate.entities.get(1).unwrap().health.current,
            100,
            "ground occupant under the bridge must not be hit by a deck strike"
        );
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().health.current,
            0,
            "bridge-deck occupant must be hit by a bridge-targeted Lightning strike"
        );
    }

    #[test]
    fn registry_only_warhead_lightning_strike_damages_building_and_sets_damage_state() {
        let (mut sim, rules) = registry_only_warhead_lightning_test_setup();
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("GAPOWR");
        let mut building = GameEntity::test_default(10, "GAPOWR", "Soviet", 5, 5);
        building.category = EntityCategory::Structure;
        building.lifecycle.in_limbo = false;
        building.lifecycle.cell_marked = true;
        building.owner = sim.interner.intern("Soviet");
        building.type_ref = type_ref;
        building.health = Health { current: 150 };
        sim.substrate.entities.insert(building);

        spawn_bolt(&mut sim, &rules, 5, 5, owner, None);

        let building = sim
            .substrate
            .entities
            .get(10)
            .expect("building remains in sim");
        assert_eq!(building.health.current, 50);
        assert!(matches!(
            building.health.compare_ratio(
                rules
                    .object(sim.interner.resolve(building.type_ref()))
                    .unwrap()
                    .strength,
                rules.general.condition_yellow,
            ),
            crate::util::native_x87::MaskedX87Ordering::Less
                | crate::util::native_x87::MaskedX87Ordering::Equal
        ));
    }

    #[test]
    fn gsi_04_07_damage_lightning_fatal_uses_inline_death_transaction() {
        fn run(carrier_hp: i32) -> (Simulation, u64) {
            let ini = IniFile::from_str(
                "[InfantryTypes]\n\
                 [VehicleTypes]\n0=BOOMER\n\
                 [AircraftTypes]\n\
                 [BuildingTypes]\n\
                 [Warheads]\n0=LightningWH\n1=WallWH\n\
                 [OverlayTypes]\n0=TESTWALL\n\
                 [BOOMER]\nStrength=101\nArmor=heavy\nExplodes=yes\nDeathWeapon=DeathBoom\n\
                 [DeathBoom]\nDamage=214\nWarhead=WallWH\n\
                 [General]\nLightningDamage=100\nLightningWarhead=LightningWH\n\
                 [LightningWH]\nCellSpread=1\nPercentAtMax=1\n\
                 Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [WallWH]\nCellSpread=0\nWall=yes\n\
                 Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
            );
            let art = IniFile::from_str("[TESTWALL]\nDamageLevels=2\n");
            let rules = RuleSet::from_ini(&ini).expect("lightning death transaction rules");
            let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));
            assert!(rules.warhead("LightningWH").is_some());
            assert!(rules.warhead("WallWH").is_some());
            assert_eq!(
                rules.object("BOOMER").unwrap().death_weapon.as_deref(),
                Some("DeathBoom")
            );
            let mut sim = Simulation::with_seed(1);
            let owner = sim.interner.intern("Americans");
            let mut carrier = GameEntity::test_default(10, "BOOMER", "Soviet", 5, 5);
            carrier.owner = sim.interner.intern("Soviet");
            carrier.type_ref = sim.interner.intern("BOOMER");
            carrier.health = Health {
                current: carrier_hp,
            };
            sim.substrate.entities.insert(carrier);
            let _ = sim.reveal(10);
            let mut overlays = OverlayGrid::new(12, 12);
            overlays.place_overlay(5, 5, 0, 0);
            sim.overlay_grid = Some(overlays);

            spawn_bolt(&mut sim, &rules, 5, 5, owner, Some(&registry));
            let rng_state = sim.scenario_rng.state();
            (sim, rng_state)
        }

        let (fatal, fatal_rng) = run(100);
        assert!(fatal.substrate.entities.get(10).is_some_and(|entity| {
            entity.health.current == 0 && entity.dying && !entity.in_logic_vector
        }));
        assert_eq!(
            fatal.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
            None
        );
        assert!(fatal.substrate.pending_delete.contains(&10));
        assert!(!fatal.live_object_order_snapshot().contains(&10));
        let mut expected_fatal_rng = SimRng::new(1);
        let _ = expected_fatal_rng.next_range_u32(BOLT_ANIMS.len() as u32);
        let _ = expected_fatal_rng.next_range_u32_inclusive(0, 400);
        assert_eq!(fatal_rng, expected_fatal_rng.state());

        let (boundary, boundary_rng) = run(101);
        assert_eq!(
            boundary
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(5, 5)
                .overlay_id,
            Some(0)
        );
        assert!(boundary.substrate.pending_delete.is_empty());
        assert!(boundary.live_object_order_snapshot().contains(&10));
        assert_eq!(
            boundary.substrate.entities.get(10).unwrap().health.current,
            1
        );
        let mut expected_boundary_rng = SimRng::new(1);
        let _ = expected_boundary_rng.next_range_u32(BOLT_ANIMS.len() as u32);
        assert_eq!(boundary_rng, expected_boundary_rng.state());
    }

    fn add_same_cell_bridge_targets(sim: &mut Simulation, type_name: &str) {
        let owner = sim.interner.intern("Soviet");
        let type_ref = sim.interner.intern(type_name);

        // Both stand in the cell's lists, so they are on the map: out of
        // limbo and marked (`+0x74`), which Apply_area_damage's dispatch reads.
        let mut ground = GameEntity::test_default(1, type_name, "Soviet", 5, 5);
        ground.owner = owner;
        ground.type_ref = type_ref;
        ground.health = Health { current: 100 };
        ground.lifecycle.in_limbo = false;
        ground.lifecycle.cell_marked = true;

        let mut bridge = GameEntity::test_default(2, type_name, "Soviet", 5, 5);
        bridge.owner = owner;
        bridge.type_ref = type_ref;
        bridge.health = Health { current: 100 };
        bridge.on_bridge = true;
        bridge.position.z = 4;
        bridge.lifecycle.in_limbo = false;
        bridge.lifecycle.cell_marked = true;

        sim.substrate.entities.insert(ground);
        sim.substrate.entities.insert(bridge);
        sim.substrate.occupancy.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        sim.substrate.occupancy.add(
            5,
            5,
            2,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        sim.resolved_terrain = Some(bridge_terrain());
    }

    fn bridge_terrain() -> ResolvedTerrainGrid {
        let mut cells = Vec::new();
        for ry in 0..10 {
            for rx in 0..10 {
                cells.push(test_terrain_cell(rx, ry));
            }
        }
        let idx = 5 * 10 + 5;
        cells[idx].bridge_facts = BridgeCellFacts {
            raw_flags: BRIDGE_FLAG_STRUCTURAL,
            ..BridgeCellFacts::default()
        };
        cells[idx].has_bridge_deck = true;
        cells[idx].bridge_walkable = true;
        cells[idx].bridge_deck_level = 4;
        ResolvedTerrainGrid::from_cells(10, 10, cells)
    }

    fn test_terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs: SpeedCostProfile::default(),
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            height_in_pixels: 0,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: 0,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: Default::default(),
            base_speed_costs: Default::default(),
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }
}
