//! Parachute descent — per-tick altitude integrator for paradropped infantry.
//!
//! Mirrors the gamemd descent block byte-exact:
//! - rate accumulates by `-1` per tick (integer DEC, not float)
//! - clamps to `Rules.ParachuteMaxFallRate` (default `-3`)
//! - Z integrates as `altitude += rate` per tick (integer leptons)
//! - first tick has `rate == 0` → no movement (3-tick ramp: 0,-1,-2,-3,-3,...)
//! - landing on `altitude <= 0` (inclusive bound)
//! - the infantry keeps its base locomotor and body sequence during descent
//!
//! Sibling of `droppod_movement` and follows the same shape: an `Option<State>`
//! field on `GameEntity`, a `begin_*` entry, a `tick_*` per-tick driver, and
//! cleanup when the object-level falling state lands.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/game_entity, sim/entity_store, sim/locomotor.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::entity_store::EntityStore;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

/// Per-entity parachute descent state. Set by [`begin_parachute_descent`],
/// cleared on landing. This mirrors gamemd's object-level falling state:
/// normal paradropped infantry keep their base locomotor and body animation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParachuteDescentState {
    /// Descent rate in leptons/tick. Negative = falling.
    /// Starts at 0; decrements by 1 per tick; clamps to `Rules.ParachuteMaxFallRate`.
    pub rate: i32,
    /// Current altitude in leptons. Decreases by `rate` each tick.
    pub altitude: SimFixed,
}

/// Begin parachute descent for an entity. Returns `true` on success.
///
/// - Initializes state with `rate = 0` (the 3-tick ramp begins on the first tick).
///
/// The entity must already exist in the EntityStore. Caller is responsible
/// for positioning the entity at the desired horizontal coord; `drop_altitude`
/// controls the starting Z.
pub fn begin_parachute_descent(
    entities: &mut EntityStore,
    entity_id: u64,
    drop_altitude: SimFixed,
) -> bool {
    let Some(entity) = entities.get_mut(entity_id) else {
        return false;
    };

    // Transfer coordinate ownership from a previous ground/tube pose to this
    // altitude integrator. Consumers prefer exact Z when present; retaining it
    // would freeze a previously moved passenger at its old surface throughout
    // descent. The production drop attaches this state before Reveal.
    entity.position.exact_z_leptons = None;
    entity.parachute_state = Some(ParachuteDescentState {
        rate: 0,
        altitude: drop_altitude,
    });

    entity.push_debug_event(
        0,
        DebugEventKind::SpecialMovementStart {
            kind: "Parachute".into(),
        },
    );
    true
}

/// Leptons the canopy is constructed above the falling object
/// (`ADD EDI, 0x4B` at `0x005F5AAA`).
const PARACHUTE_ANIM_Z_LIFT_LEPTONS: i32 = 0x4B;

/// `AnimClass` draw flags of the canopy (`PUSH 0x600`, `0x005F5ACF`).
const PARACHUTE_ANIM_DRAW_FLAGS: u32 = 0x600;

impl crate::sim::world::Simulation {
    /// Construct the falling object's canopy.
    ///
    /// gamemd-derived, the virtual `ObjectClass::Paradrop @ 0x005F5940`. It
    /// places the object itself (`Unlimbo` through `vtable+0xD8` at
    /// `0x005F5A3D`, then the coordinate through `vtable+0x1B4`) and then, at
    /// `0x005F5A9D..0x005F5B03`, for anything but a bullet it copies the
    /// object's coordinate, adds 75 leptons of Z, constructs
    /// `AnimClass(Rules+0xBBC, &coord, delay 0, loopCount 1, drawFlags 0x600,
    /// zAdjust 0, reverse 0)`, stores it at `Object+0x88` and makes the object
    /// its owner (`AnimClass::SetOwnerObject @ 0x00424B50`), so the canopy
    /// rides the descent. `Rules+0xBBC` is `[General] Parachute=`.
    ///
    /// VERA keeps no `Object+0x88`: the canopy is found again as the owner's
    /// attached anim of the parachute type.
    ///
    /// RESIDUAL: a bullet takes `Rules+0xBB8` (`BombParachute=`) at the
    /// uncopied coordinate instead; VERA has no parachuted bullets.
    ///
    /// DRIFT: after the attach native copies the owner's drawer (`vtable+0x1E4`)
    /// to the anim's `+0xD4` and the owner cell's ground Z adjust (`+0x10A`) to
    /// `+0xFC`, the pair `set_cell_anim_draw_authority` models for cell anims.
    /// They are not stored here; the presentation consequence is recorded at
    /// `build_parachute_instances`.
    pub(crate) fn attach_parachute_anim(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        owner_id: u64,
    ) -> Option<crate::sim::anim_class::AnimId> {
        let type_name = self
            .interner
            .intern(rules.general.parachute_shp.as_deref()?);
        let mut coord = self.anim_owner_coords(owner_id)?;
        coord.z = coord.z.wrapping_add(PARACHUTE_ANIM_Z_LIFT_LEPTONS);
        let (rx, ry, sub_x, sub_y, level) = coord.to_cell_sub_z();
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
            delay: 0,
            loop_count: 1,
            draw_flags: PARACHUTE_ANIM_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            ..crate::sim::components::AnimClassSpawnDescriptor::new(
                type_name, rx, ry, sub_x, sub_y, level,
            )
        };
        match self.spawn_anim_at_world(rules, descriptor, coord) {
            Ok(anim_id) => {
                self.set_anim_owner_object(anim_id, Some(owner_id), rules);
                Some(anim_id)
            }
            Err(error) => {
                // An art type that never bound draws nothing natively either.
                log::debug!("parachute anim did not construct: {error}");
                None
            }
        }
    }

    /// The landing edge of the canopy.
    ///
    /// gamemd-derived, `ObjectClass::AI @ 0x005F3E70`: when the falling
    /// object's height reaches zero it clears the in-air byte and, if
    /// `Object+0x88` is set, zeroes the anim's remaining-loops byte
    /// (`MOV byte ptr [EAX+0x195], 0` at `0x005F3F9D`). The anim is not
    /// removed: with a count of zero `AnimClass::AI` tests the frame against
    /// the type's end frame (`0x004246E4`) and completes there (`0x0042475A`),
    /// so the canopy plays on from its loop into the rest of its frames and
    /// leaves at the last one. Retail `PARACH.SHP` has 140 frames and loops
    /// 20..39, so that is about a hundred frames of the canopy collapsing on
    /// the landed object.
    pub(crate) fn wind_down_parachute_anim(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        owner_id: u64,
    ) {
        let Some(type_name) = rules.general.parachute_shp.as_deref() else {
            return;
        };
        let Some(type_id) = self.interner.get(type_name) else {
            return;
        };
        for anim in self.substrate.anims.values_mut() {
            if anim.owner_entity == Some(owner_id) && anim.type_id == type_id {
                anim.runtime.loop_remaining = 0;
            }
        }
    }
}

/// Per-tick advance for all entities with `parachute_state`.
///
/// Wired into `World::advance_tick` Phase 2 immediately after
/// `tick_droppod_movement`.
///
/// Per-tick algorithm (mirrors gamemd's descent block):
/// 1. Integrate Z FIRST: `altitude += rate` (rate is negative; first tick rate=0 → no move)
/// 2. Landing check: `altitude <= 0` → mark for cleanup (altitude clamped to exactly 0)
/// 3. Rate update: `rate -= 1`, clamp to `parachute_max_fall_rate`
///
/// Where the falling body is *drawn* is not this pass's business — the altitude
/// here is the only input `render::locomotor_visual` needs.
///
/// Cleanup (per landed entity):
/// - clear `parachute_state`
#[cfg(test)]
pub fn tick_parachute_descent(
    entities: &mut EntityStore,
    parachute_max_fall_rate: i32,
    sim_tick: u64,
) {
    let keys = entities.keys_sorted();
    tick_parachute_descent_in_order(entities, &keys, parachute_max_fall_rate, sim_tick);
}

pub(crate) fn tick_parachute_descent_in_order(
    entities: &mut EntityStore,
    entity_order: &[u64],
    parachute_max_fall_rate: i32,
    sim_tick: u64,
) {
    let mut finished: Vec<u64> = Vec::new();

    for &id in entity_order {
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        let Some(ref mut state) = entity.parachute_state else {
            continue;
        };

        // Integrate Z FIRST. On the very first tick `rate == 0`, so altitude
        // doesn't change yet — that produces the 3-tick ramp 0,-1,-2,-3.
        state.altitude += SimFixed::from_num(state.rate);

        // Landing on `altitude <= 0` (inclusive). Clamp to exactly SIM_ZERO
        // so render position never shows the unit below ground for a frame.
        if state.altitude <= SIM_ZERO {
            state.altitude = SIM_ZERO;
            finished.push(id);
        } else {
            // Integer DEC, then clamp toward the more-negative bound.
            state.rate = (state.rate - 1).max(parachute_max_fall_rate);
        }
    }

    // Cleanup landed entities: clear descent state first, so anything watching
    // the landing transition sees a coherent snapshot. Descent does not displace
    // the locomotor, so there is no piggyback to unwind here.
    for id in finished {
        if let Some(entity) = entities.get_mut(id) {
            entity.parachute_state = None;
            entity.push_debug_event(sim_tick as u32, DebugEventKind::SpecialMovementEnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
    use crate::sim::animation::{Animation, SequenceKind};
    use crate::sim::entity_store::EntityStore;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotion::LocomotorSlot;
    use crate::sim::movement::locomotor::{GroundMovePhase, LocomotorState, MovementLayer};
    use crate::util::fixed_math::{SIM_ONE, SIM_ZERO};

    /// Mirrors the helper used in droppod_movement.rs tests.
    fn make_walk_loco() -> LocomotorState {
        LocomotorState {
            kind: LocomotorKind::Walk,
            slot: LocomotorSlot::from_kind(LocomotorKind::Walk),
            powered: true,
            piggyback: None,
            runtime_payload: crate::sim::movement::locomotion::LocomotorRuntimePayload::for_kind(
                LocomotorKind::Walk,
                0,
            ),
            layer: MovementLayer::Ground,
            phase: GroundMovePhase::Idle,

            speed_multiplier: SIM_ONE,
            speed_fraction: SIM_ONE,
            fly_current_speed: SIM_ZERO,
            altitude: SIM_ZERO,

            balloon_hover: false,
            hover_attack: false,
            speed_type: SpeedType::Foot,
            movement_zone: MovementZone::Normal,
            rot: 0,
            air_progress: SIM_ZERO,
            infantry_wobble_phase: 0.0,
            subcell_dest: None,
            hover_throttle: crate::util::fixed_math::SIM_ZERO,
            hover_speed_request: crate::util::fixed_math::SIM_ZERO,
            hover_bob_offset: crate::util::fixed_math::SIM_ZERO,
        }
    }

    fn drop_altitude_1200() -> SimFixed {
        SimFixed::from_num(1200)
    }

    /// Build an infantry entity with a Walk locomotor and a Stand animation,
    /// inserted into `entities`. Returns the entity id.
    fn insert_test_infantry(entities: &mut EntityStore, id: u64) -> u64 {
        let mut e = GameEntity::test_default(id, "E1", "Americans", 10, 10);
        e.locomotor = Some(make_walk_loco());
        e.animation = Some(Animation::new(SequenceKind::Stand));
        entities.insert(e);
        id
    }

    #[test]
    fn test_begin_attaches_state_and_keeps_locomotor_identity() {
        let mut entities = EntityStore::new();
        let id = insert_test_infantry(&mut entities, 1);
        entities.get_mut(id).unwrap().position.exact_z_leptons = Some(52);

        assert!(begin_parachute_descent(
            &mut entities,
            id,
            drop_altitude_1200()
        ));

        let entity = entities.get(id).expect("should exist");
        assert_eq!(
            entity.position.exact_z_leptons, None,
            "descent must take over a previously exact ground coordinate"
        );
        let state = entity
            .parachute_state
            .as_ref()
            .expect("parachute state must be attached");
        assert_eq!(
            state.rate, 0,
            "rate must start at 0 (3-tick ramp begins next tick)"
        );
        assert_eq!(state.altitude, drop_altitude_1200());

        let loco = entity.locomotor.as_ref().expect("has loco");
        assert!(
            !loco.is_overridden(),
            "ordinary paradropped infantry keep their base locomotor"
        );
        assert_eq!(loco.kind, LocomotorKind::Walk);
        assert_eq!(
            loco.layer,
            MovementLayer::Ground,
            "object-level falling state must not rewrite locomotor layer"
        );
    }

    #[test]
    fn test_body_sequence_preserved_on_begin() {
        let mut entities = EntityStore::new();
        let id = insert_test_infantry(&mut entities, 1);

        begin_parachute_descent(&mut entities, id, drop_altitude_1200());

        let anim = entities
            .get(id)
            .expect("alive")
            .animation
            .as_ref()
            .expect("has anim");
        assert_eq!(
            anim.sequence,
            SequenceKind::Stand,
            "normal paradrops render the attached PARACH anim, not body Paradrop frames"
        );
    }

    #[test]
    fn test_begin_works_without_locomotor() {
        // Mirrors test_droppod_without_loco_still_works.
        let mut entities = EntityStore::new();
        let mut e = GameEntity::test_default(1, "E1", "Americans", 5, 5);
        // No locomotor.
        e.animation = Some(Animation::new(SequenceKind::Stand));
        entities.insert(e);

        assert!(begin_parachute_descent(
            &mut entities,
            1,
            drop_altitude_1200()
        ));

        let entity = entities.get(1).expect("alive");
        assert!(entity.parachute_state.is_some());
    }

    #[test]
    fn test_begin_returns_false_for_missing_entity() {
        let mut entities = EntityStore::new();
        assert!(!begin_parachute_descent(
            &mut entities,
            999,
            drop_altitude_1200()
        ));
    }

    // -------------------------------------------------------------------
    // Parity tests — verify the descent state machine matches gamemd
    // exactly. See JUMPJET_LOCOMOTION_CLASS_GHIDRA_REPORT.md Round 4 §R4.7.
    // -------------------------------------------------------------------

    /// Default INI value per `[General] ParachuteMaxFallRate=-3`.
    const RULES_PARACHUTE_MAX_FALL_RATE: i32 = -3;
    /// Set up an entity at id=1, attach a parachute descent at the given altitude.
    /// Returns the id.
    fn setup_parachuting_entity(entities: &mut EntityStore, drop_altitude: SimFixed) -> u64 {
        let id = insert_test_infantry(entities, 1);
        begin_parachute_descent(entities, id, drop_altitude);
        id
    }

    #[test]
    fn test_3tick_rate_ramp() {
        // Rate sequence over 6 ticks must be exactly [0, -1, -2, -3, -3, -3].
        // Sample BEFORE each tick (= rate-in for that tick).
        let mut entities = EntityStore::new();
        let id = setup_parachuting_entity(&mut entities, drop_altitude_1200());

        let mut observed: Vec<i32> = Vec::new();
        for _ in 0..6 {
            let entity = entities.get(id).expect("alive");
            let state = entity.parachute_state.as_ref().expect("descending");
            observed.push(state.rate);
            tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
        }

        assert_eq!(
            observed,
            vec![0, -1, -2, -3, -3, -3],
            "3-tick ramp must be 0,-1,-2,-3,-3,-3 (NOT instant -3)"
        );
    }

    #[test]
    fn test_descent_distance_first_4_ticks() {
        // Total descent over the first N ticks (deltas vs initial):
        //   tick 1: 0  (rate was 0 at integration)
        //   tick 2: 1  (rate was -1 at integration)
        //   tick 3: 3  (rate was -2 at integration)
        //   tick 4: 6  (rate was -3 at integration)
        let mut entities = EntityStore::new();
        let id = setup_parachuting_entity(&mut entities, drop_altitude_1200());

        let initial_altitude = entities
            .get(id)
            .unwrap()
            .parachute_state
            .as_ref()
            .unwrap()
            .altitude;

        let expected_deltas: [i32; 4] = [0, 1, 3, 6];
        for (i, expected_delta) in expected_deltas.iter().enumerate() {
            tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
            let altitude = entities
                .get(id)
                .unwrap()
                .parachute_state
                .as_ref()
                .unwrap()
                .altitude;
            let descent = initial_altitude - altitude;
            let expected = SimFixed::from_num(*expected_delta);
            assert_eq!(
                descent,
                expected,
                "after tick {} descent should be {} leptons (got {})",
                i + 1,
                expected_delta,
                descent
            );
        }
    }

    #[test]
    fn test_steady_state_rate() {
        // After enough ticks past the ramp, rate stays clamped at -3.
        let mut entities = EntityStore::new();
        let id = setup_parachuting_entity(&mut entities, drop_altitude_1200());

        for _ in 0..10 {
            tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
        }
        let rate = entities
            .get(id)
            .unwrap()
            .parachute_state
            .as_ref()
            .unwrap()
            .rate;
        assert_eq!(
            rate, RULES_PARACHUTE_MAX_FALL_RATE,
            "steady-state rate must equal ParachuteMaxFallRate"
        );
    }

    #[test]
    fn test_landing_inclusive_zero() {
        // Drop altitude = 6 leptons → tick 4 integrates altitude = 0 → landing
        // triggers (inclusive bound). `parachute_state` must be cleared.
        let mut entities = EntityStore::new();
        let id = setup_parachuting_entity(&mut entities, SimFixed::from_num(6));

        for _ in 0..4 {
            tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
        }

        let entity = entities.get(id).expect("alive");
        assert!(
            entity.parachute_state.is_none(),
            "landing at altitude == 0 must trigger cleanup"
        );
    }

    #[test]
    fn test_landing_clamps_to_zero_no_overshoot() {
        // Drop altitude = 5 leptons. The ramp is 0,-1,-2,-3, so three ticks
        // leave altitude at 2 and the fourth integrates to -1 — below ground.
        // That tick must clamp to SIM_ZERO and land rather than leave the unit
        // underground.
        let mut entities = EntityStore::new();
        let id = setup_parachuting_entity(&mut entities, SimFixed::from_num(5));

        for _ in 0..3 {
            tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
        }
        let state = entities
            .get(id)
            .expect("alive")
            .parachute_state
            .as_ref()
            .expect("still descending after three ticks");
        assert_eq!(state.altitude, SimFixed::from_num(2));
        assert_eq!(state.rate, -3, "the next tick would integrate to -1");

        tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
        assert!(
            entities.get(id).expect("alive").parachute_state.is_none(),
            "the overshooting tick must clamp and land, not leave the unit falling"
        );
    }

    #[test]
    fn test_clamp_at_max_fall_rate_default() {
        // Rate must never exceed (be more-negative than) ParachuteMaxFallRate.
        let mut entities = EntityStore::new();
        let id = setup_parachuting_entity(&mut entities, drop_altitude_1200());

        for _ in 0..50 {
            tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
            if let Some(state) = entities.get(id).and_then(|e| e.parachute_state.as_ref()) {
                assert!(
                    state.rate >= RULES_PARACHUTE_MAX_FALL_RATE,
                    "rate {} must not exceed (more-negative than) max {}",
                    state.rate,
                    RULES_PARACHUTE_MAX_FALL_RATE
                );
            }
        }
    }

    #[test]
    fn test_clamp_with_custom_max_fall_rate() {
        // Mod-friendliness: a non-default `parachute_max_fall_rate` must be
        // respected. With max = -1, rate ramp is 0 → -1 → -1 → -1.
        let mut entities = EntityStore::new();
        let id = setup_parachuting_entity(&mut entities, drop_altitude_1200());

        let custom_max: i32 = -1;
        let mut observed: Vec<i32> = Vec::new();
        for _ in 0..4 {
            let rate = entities
                .get(id)
                .unwrap()
                .parachute_state
                .as_ref()
                .unwrap()
                .rate;
            observed.push(rate);
            tick_parachute_descent(&mut entities, custom_max, 0);
        }
        assert_eq!(
            observed,
            vec![0, -1, -1, -1],
            "with max=-1, rate must clamp at -1 after the first decrement"
        );
    }

    #[test]
    fn test_body_sequence_preserved_on_landing() {
        // Normal paradropped infantry do not switch to the body Paradrop
        // sequence, so landing should not rewrite the body animation either.
        let mut entities = EntityStore::new();
        let id = setup_parachuting_entity(&mut entities, SimFixed::from_num(6));

        assert_eq!(
            entities
                .get(id)
                .unwrap()
                .animation
                .as_ref()
                .unwrap()
                .sequence,
            SequenceKind::Stand
        );

        for _ in 0..4 {
            tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
        }

        let anim = entities.get(id).unwrap().animation.as_ref().unwrap();
        assert_eq!(
            anim.sequence,
            SequenceKind::Stand,
            "landing must preserve the unchanged body sequence"
        );
    }

    #[test]
    fn test_body_sequence_preserved_if_externally_changed() {
        // If some other system changed the sequence during descent (e.g., a
        // death anim took over), don't overwrite on landing.
        let mut entities = EntityStore::new();
        let id = setup_parachuting_entity(&mut entities, SimFixed::from_num(6));

        // Mid-descent, externally change to Die1 (simulating shot down in air).
        for _ in 0..2 {
            tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
        }
        entities
            .get_mut(id)
            .unwrap()
            .animation
            .as_mut()
            .unwrap()
            .switch_to(SequenceKind::Die1);

        // Continue ticking through landing.
        for _ in 0..4 {
            tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
        }

        let anim = entities.get(id).unwrap().animation.as_ref().unwrap();
        assert_eq!(
            anim.sequence,
            SequenceKind::Die1,
            "must NOT overwrite Die1 with Stand on landing"
        );
    }

    #[test]
    fn test_locomotor_identity_preserved_through_landing() {
        let mut entities = EntityStore::new();
        let id = setup_parachuting_entity(&mut entities, SimFixed::from_num(6));

        {
            let loco = entities.get(id).unwrap().locomotor.as_ref().unwrap();
            assert!(!loco.is_overridden());
            assert_eq!(loco.kind, LocomotorKind::Walk);
        }

        for _ in 0..4 {
            tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
        }

        let loco = entities.get(id).unwrap().locomotor.as_ref().unwrap();
        assert!(
            !loco.is_overridden(),
            "object-level falling must not leave a locomotor override"
        );
        assert_eq!(
            loco.kind,
            LocomotorKind::Walk,
            "must preserve base Walk locomotor"
        );
    }

    #[test]
    fn test_works_without_animation() {
        // begin and tick must not panic when entity.animation is None.
        let mut entities = EntityStore::new();
        let mut e = GameEntity::test_default(1, "E1", "Americans", 5, 5);
        e.locomotor = Some(make_walk_loco());
        e.animation = None;
        entities.insert(e);

        assert!(begin_parachute_descent(
            &mut entities,
            1,
            SimFixed::from_num(6)
        ));
        for _ in 0..10 {
            tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);
        }
        let entity = entities.get(1).unwrap();
        assert!(
            entity.parachute_state.is_none(),
            "should land cleanly without an animation field"
        );
    }

    #[test]
    fn test_zero_event_stamp_still_advances_one_native_frame() {
        // The final argument only stamps debug events. Admission and pausing
        // belong to the global frame pacer, so every call advances one frame.
        let mut entities = EntityStore::new();
        let id = setup_parachuting_entity(&mut entities, drop_altitude_1200());
        let initial_alt = entities
            .get(id)
            .unwrap()
            .parachute_state
            .as_ref()
            .unwrap()
            .altitude;
        let initial_rate = entities
            .get(id)
            .unwrap()
            .parachute_state
            .as_ref()
            .unwrap()
            .rate;

        tick_parachute_descent(&mut entities, RULES_PARACHUTE_MAX_FALL_RATE, 0);

        let after = entities.get(id).unwrap().parachute_state.as_ref().unwrap();
        assert_eq!(
            after.altitude, initial_alt,
            "the first frame integrates the initial zero fall rate"
        );
        assert_eq!(
            after.rate,
            initial_rate - 1,
            "a zero event stamp must not suppress the native-frame update"
        );
    }
}

#[cfg(test)]
mod canopy_tests {
    use super::*;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::anim_class::AnimWorldCoord;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    fn fixture() -> (Simulation, RuleSet, u64) {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[General]\nParachute=PARACH\n[E1]\nStrength=125\n",
        ))
        .expect("rules");
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(
            "[PARACH]\nRate=900\nLoopStart=2\nLoopEnd=5\nLoopCount=-1\n",
        ));
        art.bind_anim_frame_count_for_test("PARACH", 6);
        rules.merge_art_data(&art);
        rules.art_registry = art;

        let mut sim = Simulation::new();
        sim.interner = crate::sim::intern::test_interner();
        let id = sim.allocate_stable_id();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(id, "E1", "Americans", 10, 11));
        assert!(begin_parachute_descent(
            &mut sim.substrate.entities,
            id,
            SimFixed::from_num(600)
        ));
        (sim, rules, id)
    }

    /// `0x005F5A9D..0x005F5B03`: the object's coordinate plus 75 leptons of Z,
    /// the constructor row, then `SetOwnerObject(object)`.
    #[test]
    fn the_canopy_is_built_75_leptons_above_the_falling_object_and_rides_it() {
        let (mut sim, rules, id) = fixture();
        let anim_id = sim.attach_parachute_anim(&rules, id).expect("canopy");
        let anim = sim.anim(anim_id).unwrap();
        assert_eq!(sim.interner.resolve(anim.type_id), "PARACH");
        assert_eq!((anim.draw_flags, anim.z_adjust), (0x600, 0));
        assert_eq!(anim.owner_entity, Some(id));
        assert_eq!(
            anim.world_coord,
            AnimWorldCoord { x: 0, y: 0, z: 75 },
            "stored owner-relative: only the lift"
        );
        let body = sim.anim_owner_coords(id).unwrap();
        assert_eq!(body.z, 600, "the object hangs at its drop altitude");
        assert_eq!(
            sim.anim_absolute_coord(anim_id),
            Some(AnimWorldCoord {
                z: body.z + 75,
                ..body
            })
        );

        // A few frames of descent: the canopy comes down with the object.
        for tick in 0..6 {
            tick_parachute_descent(&mut sim.substrate.entities, -3, tick);
        }
        let lower = sim.anim_owner_coords(id).unwrap().z;
        assert!(lower < 600);
        assert_eq!(sim.anim_absolute_coord(anim_id).unwrap().z, lower + 75);
    }

    /// `0x005F3F9D`: landing zeroes the canopy's remaining loops; it is not
    /// removed, it plays to its loop end and leaves on its own.
    #[test]
    fn landing_winds_the_canopy_down_instead_of_removing_it() {
        let (mut sim, rules, id) = fixture();
        let anim_id = sim.attach_parachute_anim(&rules, id).expect("canopy");
        assert_eq!(sim.anim(anim_id).unwrap().runtime.loop_remaining, u8::MAX);

        sim.wind_down_parachute_anim(&rules, id);

        let anim = sim.anim(anim_id).expect("still in the store");
        assert_eq!(anim.runtime.loop_remaining, 0);
        assert!(!anim.runtime.inactive);
        assert_eq!(anim.owner_entity, Some(id));
    }

    #[test]
    fn no_parachute_type_no_canopy() {
        let (mut sim, mut rules, id) = fixture();
        rules.general.parachute_shp = None;
        assert_eq!(sim.attach_parachute_anim(&rules, id), None);
        sim.wind_down_parachute_anim(&rules, id);
        assert_eq!(sim.substrate.anims.iter().count(), 0);
    }
}
