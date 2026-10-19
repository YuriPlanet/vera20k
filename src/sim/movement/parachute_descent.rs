//! ObjectClass falling: parachutes and dropped bombs share one state owner.
//!
//! As in gamemd, a falling object's height is its Location Z
//! (`position.exact_z_leptons`). `ObjectClass::AI`'s falling block moves it
//! ([`Simulation::advance_fall`](crate::sim::world::Simulation::advance_fall)):
//! - each frame the Z moves by the FallRate: `Location.Z = GetZ() + FallRate`
//! - the FallRate starts at 0 and drops by one a frame (integer DEC) to
//!   `Rules.ParachuteMaxFallRate` (default `-3`), so it ramps 0,-1,-2,-3,-3,...
//! - the fall grounds when GetHeight is at most 0, and SetHeight(0) puts the
//!   object on the ground or deck
//! - the infantry keeps its base locomotor and body sequence during descent
//!
//! Free fall uses trunc(FallRate - 1.4), capped by NoParachuteMaxFallRate.
//! Landing retains the rate and parachute/bomb bytes; a bomb then enters
//! the synchronous C4 receiver with its current Health and both force flags.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/game_entity, sim/entity_store, sim/locomotor.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::sim::entity_store::EntityStore;
use crate::sim::world::FrameEffects;

/// Begin parachute descent for an entity at world Z `drop_z` (leptons).
/// Returns `true` on success.
///
/// `ObjectClass::Paradrop @ 0x005F5940` raises the falling byte (`+0x8D`,
/// `0x005F5965`) and places the object at the drop coordinate: Unlimbo
/// (`vt+0xD8`), then SetLocation (`vt+0x1B4`, `0x005F5A50`). The FallRate
/// starts at 0, so the ramp begins on the first frame.
///
/// The caller positions the entity's XY. Reveal keeps this Z as the Unlimbo
/// coordinate's.
pub fn begin_parachute_descent(entities: &mut EntityStore, entity_id: u64, drop_z: i32) -> bool {
    let Some(entity) = entities.get_mut(entity_id) else {
        return false;
    };

    entity.begin_parachute_fall(drop_z);
    true
}

/// Leptons the canopy is constructed above the falling object
/// (`ADD EDI, 0x4B` at `0x005F5AAA`).
const PARACHUTE_ANIM_Z_LIFT_LEPTONS: i32 = 0x4B;

/// `AnimClass` draw flags of the canopy (`PUSH 0x600`, `0x005F5ACF`).
const PARACHUTE_ANIM_DRAW_FLAGS: u32 = 0x600;

impl crate::sim::world::Simulation {
    /// ObjectAI5F3F11..5F405B inside MissionAI, before mission dispatch and
    /// the locomotor. The landing callback precedes canopy wind-down and the
    /// forced C4 receiver. No fall-specific RNG, timer or detach calls occur;
    /// the synchronous production receiver owns all ordinary death effects.
    pub(crate) fn object_ai_fall(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<(bool, bool), crate::sim::world::FrameAdvanceError> {
        if self
            .substrate
            .entities
            .get(id)
            .is_none_or(|e| !e.is_falling_down())
        {
            return Ok((false, false));
        }
        let old_layer = self.entity_display_layer(id, Some(rules));
        let landed = self.advance_fall(
            id,
            rules.general.parachute_max_fall_rate,
            Some(rules),
            registry,
            effects,
        );
        let mut changed = false;
        if landed {
            changed = self.per_cell_process(
                id,
                super::PerCellReason::Arrival,
                Some(rules),
                registry,
                effects,
            )?;
            self.wind_down_parachute_anim(rules, id);
            //5F3FA4..5F3FFA: PerCell can Limbo the object. The landing
            // frame updates and retains FallRate only after that callback.
            if let Some(entity) = self.substrate.entities.get_mut(id)
                && !entity.lifecycle.in_limbo()
            {
                entity.advance_fall_rate(
                    rules.general.parachute_max_fall_rate,
                    rules.general.no_parachute_max_fall_rate,
                );
            }
        }
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|e| e.lifecycle.in_limbo())
        {
            self.substrate.display.remove(id);
            return Ok((landed, changed));
        }
        let new_layer = self.entity_display_layer(id, Some(rules));
        if old_layer != new_layer
            && let Some(layer) = new_layer
        {
            //5F4001..5F400E: resubmit only when the live layer changed.
            self.submit_object_display(id, layer, Some(rules));
        }
        if let Some(health) = self
            .substrate
            .entities
            .get(id)
            .filter(|e| {
                !e.lifecycle.in_limbo()
                    && !e.is_falling_down()
                    && e.was_dropped_as_bomb()
                    && e.health.current() > 0
            })
            .map(|e| e.health.current())
        {
            //5F402B copies current Health+6C into a local packet (not Strength).
            let c4 = self.interner.intern(&rules.bridge_warheads.c4_name);
            let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
                id,
                health,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                c4,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            );
            self.commit_direct_damage_receiver(rules, registry, hit, effects);
        }
        Ok((landed, changed))
    }

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
        let owner_house = self.substrate.entities.get(owner_id)?.owner();
        let mut coord = self.anim_owner_coords(owner_id)?;
        coord.z = coord.z.wrapping_add(PARACHUTE_ANIM_Z_LIFT_LEPTONS);
        let (rx, ry, sub_x, sub_y, level) = coord.to_cell_sub_z();
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
            delay: 0,
            loop_count: 1,
            draw_flags: PARACHUTE_ANIM_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            owner: Some(owner_id),
            ..crate::sim::components::AnimClassSpawnDescriptor::new(
                type_name, rx, ry, sub_x, sub_y, level,
            )
        };
        match self.spawn_anim_at_world(rules, descriptor, coord) {
            Ok(anim_id) => {
                // Native Paradrop calls SetOwnerObject, then GetRemapColour
                // through the owner's vtable and stores that ConvertClass at
                // Anim+0xD4 (`ObjectClass::Paradrop @ 0x005F5940`,
                // `0x005F5AFE..0x005F5B19`). Freeze the same house selection
                // on the attached canopy after construction.
                self.set_anim_house_remap(anim_id, owner_house);
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

impl crate::sim::world::Simulation {
    /// `ObjectClass::AI`'s falling block (`0x005F3F11..0x005F3FFA`) for one
    /// object. Answers whether the fall grounded this frame.
    ///
    /// - `Location.Z = GetZ() + FallRate` (`0x005F3F2C..0x005F3F60`). On the
    ///   first frame the FallRate is 0, so the object hangs for a frame.
    /// - A marked object leaves its cell through its own Mark (`vt+0x124` at
    ///   `0x005F3F46`) before that write and marks again after it
    ///   (`0x005F3F58`). A paratrooper's layer is Ground at any height (Foot
    ///   `vt+0x78` = `0x004DB7E0` asks the locomotor; Walk's `0x0075C7E0`
    ///   returns Ground), so every fall frame prepends it to its cell's list
    ///   again and recalculates the cell.
    /// - GetHeight at most 0 (`0x005F3F6A`) grounds it: SetHeight(0)
    ///   (`0x005F3F7A`) puts it on the ground or deck, and the falling byte
    ///   clears (`0x005F3F86`).
    /// - An airborne object updates its retained rate here. On landing,
    ///   `object_ai_fall` first runs PerCell and canopy wind-down, then updates
    ///   the rate unless that callback put the object in Limbo (5F3FA4).
    ///
    /// Native rate controls are executed in drop_as_bomb.json.
    pub(crate) fn advance_fall(
        &mut self,
        stable_id: u64,
        max_fall_rate: i32,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        let Some(rate) = entity.is_falling_down().then(|| entity.fall_rate()) else {
            return false;
        };
        let z = crate::sim::movement::ground_pose::object_world_z_leptons(
            entity,
            self.resolved_terrain.as_ref(),
        )
        .wrapping_add(rate);
        let marked = entity.lifecycle.cell_marked();
        let context = crate::sim::world::UninitContext::new(rules, registry).with_effects(effects);
        if marked {
            self.unmark_entity_remove(stable_id, context);
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.position.put_z(z);
        }
        if marked {
            self.mark_entity_put(stable_id, context);
        }
        let terrain = self.resolved_terrain.as_ref();
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        if crate::sim::movement::air_movement::current_fly_height(entity, terrain) > 0 {
            if !entity.lifecycle.in_limbo() {
                entity.advance_fall_rate(
                    max_fall_rate,
                    rules.map_or(-100, |r| r.general.no_parachute_max_fall_rate),
                );
            }
            return false;
        }
        // SetHeight(0) (`0x005F3F7A`) runs before the falling byte clears
        // (`0x005F3F86`). Descent does not displace the locomotor, so there
        // is no piggyback to unwind here.
        self.set_object_height(stable_id, 0, rules, registry, effects);
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return true;
        };
        entity.finish_fall();
        true
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
        // Synthetic fixture supplies ART directly, without native read-admission replay.
        rules.install_art_fixture(art);

        let mut sim = Simulation::new();
        sim.interner = crate::sim::intern::test_interner();
        let id = sim.allocate_stable_id();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(id, "E1", "Americans", 10, 11));
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(false);
        assert!(begin_parachute_descent(
            &mut sim.substrate.entities,
            id,
            600
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
            anim.remap(),
            Some(crate::sim::anim_class::AnimRemap::House(
                sim.entities().get(id).unwrap().owner()
            )),
            "Paradrop copies the owner's ConvertClass to Anim+0xD4"
        );
        assert_eq!(
            anim.world_coord,
            AnimWorldCoord { x: 0, y: 0, z: 75 },
            "stored owner-relative: only the lift"
        );
        let body = sim.anim_owner_coords(id).unwrap();
        assert_eq!(body.z, 600, "the object hangs at its drop Z");
        assert_eq!(
            sim.anim_absolute_coord(anim_id),
            Some(AnimWorldCoord {
                z: body.z + 75,
                ..body
            })
        );

        // A few frames of descent: the canopy comes down with the object.
        for _ in 0..6 {
            sim.advance_fall(
                id,
                -3,
                None,
                None,
                crate::sim::world::FrameEffects::default(),
            );
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
