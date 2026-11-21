//! Detach_All, building now-dead radio contacts, Stun (Foot and Techno),
//! lifecycle requests and the represented UnInit pre-hook.

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::lifecycle_request::LifecycleRequest;
use crate::sim::world::{FrameEffects, Simulation};

#[cfg(any(test, feature = "test-support"))]
use super::LifecycleTestEvent;
use super::{LifecycleOutput, UninitContext};

impl Simulation {
    /// `Detach_All(all)` (vtable `+0xDC`): the class prelude, then
    /// `ObjectClass::Detach_All @ 0x005F5280`'s Deselect and its expiry
    /// announcement (`DispatchPointerExpiredCleanup @ 0x007258D0` with `all`
    /// as the control), leaving Logic, occupancy and liveness intact.
    ///
    /// Callers:
    /// * `all`: the exact-zero Destroy callback of `ObjectClass::ReceiveDamage`
    ///   (`0x005F5765..0x005F57AF`), after the kill callback and before
    ///   TechnoClass's death arm, and every Stun (death, crash, sinking,
    ///   lift release and sale).
    /// * not `all`: `TechnoClass::StartCloaking @ 0x00703770` and the cloak's
    ///   1 → 2 completion (`0x006FBA98`).
    ///
    /// The class prelude:
    /// * `BuildingClass::Detach_All @ 0x0044EBF0`. With `all`, the production
    ///   prelude it shares with Conceal: abandon the separate Building
    ///   Factory, temporarily exclude the building, and cancel the House
    ///   product only without an alternative. Then RADIO OVER_OUT to every
    ///   contact. Without `all` (`0x0044EF29..0x0044EF69`), OVER_OUT to each
    ///   contact its owner is not allied with (`HouseClass::Is_Ally_ByObject
    ///   @ 0x004F9A90`), slot by slot.
    /// * `FootClass::Detach_All @ 0x004D9720`. With `all`, the object leaves
    ///   its team (`0x004D9744`) and sends OVER_OUT to contact 0. Without
    ///   `all` (`0x004D9762..0x004D9785`), contact 0 gets OVER_OUT only when
    ///   the owner is not allied with it.
    ///
    /// The Deselect (`0x005F52A8..0x005F52E9`) is unconditional with `all`.
    /// Without it, the object stays selected when its owner is the player's
    /// (`0x0050B6F0`, [`Self::house_is_human_player`]) or, for a Techno, the
    /// player senses its cell (vt+0x328, [`sensor_visible_to_player`]). Then
    /// the follow camera lets go of the object whatever the gate decided
    /// (`0x005F52EF..0x005F5306`, [`LifecycleOutput::FollowReleased`]).
    ///
    /// Other `Detach_All(false)` callers, all dormant in retail: Infantry
    /// PerCellProcess's one-cell UndeploysInto conversion (`0x00519A19`, on
    /// the Building), the vehicle thief (`0x00520423`, `0x00741868`) and the
    /// Tunnel locomotor (`0x00728F7B`, `0x00729465`). No retail building
    /// undeploys into a one-cell infantryman, no type sets `VehicleThief=`
    /// and none uses the Tunnel locomotor.
    ///
    /// The LineTrail teardown (`0x005F5284..0x005F529D`) has nothing to free:
    /// VERA constructs LineTrails only for bullets (`admit_projectile`).
    ///
    /// [`sensor_visible_to_player`]: super::techno_ai_cloak::sensor_visible_to_player
    pub(crate) fn detach_all(&mut self, stable_id: u64, all: bool, mut context: UninitContext<'_>) {
        let Some((category, owner)) = self
            .substrate
            .entities
            .get(stable_id)
            .map(|e| (e.category(), e.owner()))
        else {
            return;
        };
        // Ares BuildingClass_Disappear_PrismForward (10016470, hook
        // 0x44EBF0) takes a building out of its Prism network first.
        if context.rules.ares() && category == EntityCategory::Structure {
            self.substrate
                .ares_prism_remove_from_network(stable_id, true);
        }
        let allied_with = |sim: &Self, contact: u64| {
            sim.substrate.entities.get(contact).is_some_and(|contact| {
                crate::sim::combat::combat_weapon::is_ally_by_object(
                    Some(&sim.house_alliances),
                    &sim.interner,
                    owner,
                    contact.owner(),
                )
            })
        };
        match category {
            EntityCategory::Structure if all => {
                // Shared BuildingDetach44EBF0 production prelude.
                crate::sim::production::detach_building_production(
                    self,
                    context.rules(),
                    stable_id,
                    context.effects(),
                );
                crate::sim::radio::broadcast_break(
                    self,
                    stable_id,
                    context.rules(),
                    context.effects(),
                );
            }
            EntityCategory::Structure => {
                let capacity = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .map_or(0, |entity| entity.seldom.radio_contacts.capacity());
                for slot in 0..capacity {
                    if let Some(contact) = self
                        .substrate
                        .entities
                        .get(stable_id)
                        .and_then(|entity| entity.seldom.radio_contacts.slot(slot))
                        .filter(|&contact| !allied_with(self, contact))
                    {
                        crate::sim::radio::transmit(
                            self,
                            stable_id,
                            contact,
                            crate::sim::radio::RadioMessage::Break,
                            crate::sim::radio::RadioPayload::default(),
                            context.rules(),
                            context.effects(),
                        );
                    }
                }
            }
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft => {
                // The Foot prelude leaves the object's team first, unless an
                // object-placement scope is open (`[0x00A8E7AC]`,
                // `0x004D9736..0x004D9744`).
                if all && !self.object_placement_scope_active() {
                    self.leave_team(stable_id, false, context.rules(), context.effects());
                }
                if let Some(contact) = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .and_then(|entity| entity.seldom.radio_contacts.slot(0))
                    .filter(|&contact| all || !allied_with(self, contact))
                {
                    crate::sim::radio::transmit(
                        self,
                        stable_id,
                        contact,
                        crate::sim::radio::RadioMessage::Break,
                        crate::sim::radio::RadioPayload::default(),
                        context.rules(),
                        context.effects(),
                    );
                }
            }
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::DestroyRadioBreakCompleted { stable_id });

        let keeps_selection = !all
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| {
                    self.house_is_human_player(entity.owner())
                        || super::techno_ai_cloak::sensor_visible_to_player(self, stable_id)
                });
        if !keeps_selection {
            self.object_deselect(stable_id);
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::DestroyDeselected { stable_id });
        self.lifecycle_outputs
            .emit(LifecycleOutput::FollowReleased { stable_id });

        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::DestroyNotifyBoundary { stable_id });
        if all {
            self.notify_pointer_expired(stable_id, context);
        } else {
            self.announce_detach_all(stable_id, context);
        }
    }

    /// The Building NowDead contact loop (`BuildingClass::ReceiveDamage`,
    /// `0x00442511..0x004425F4`) over the contacts the building held before
    /// the hit: one at least 0x100 leptons from the building's `GetCoords`
    /// centre, on a building that is not a `Helipad=`, is sent RUN_AWAY (0x17),
    /// so a War Miner on a destroyed refinery's pad leaves its unload for
    /// Harvest (`radio::receive`), then clears its Unit+0x500 pending entry
    /// (`0x004425AA`). A nearer contact, or any contact of a helipad, takes
    /// `Strength * 10` C4 damage (`0x004425B6..0x004425EE`), with no source
    /// and both receiver flags set. Execute each receiver synchronously in
    /// the caller's transaction: its death effects may alter later contacts.
    /// Evidence: retail gamemd.exe instructions, inspect with
    /// `python -m tools.native_inspect disasm 0x442511 --bytes 0xf7`.
    pub(crate) fn building_now_dead_contacts(
        &mut self,
        building_id: u64,
        contacts: &[u64],
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        run: &mut crate::sim::combat::world_receiver::ReceiverRun<'_>,
    ) -> (
        crate::sim::combat::DeathEffects,
        Vec<crate::sim::combat::UnderAttackEvent>,
    ) {
        let mut death_effects = crate::sim::combat::DeathEffects::default();
        let mut under_attack_events = Vec::new();
        // `0x00442532`/`0x00442543`: both ends are GetCoords (vt+0x48).
        let Some((centre, helipad)) = self.substrate.entities.get(building_id).and_then(|b| {
            let object = self.object_type(b.type_ref(), rules)?;
            let centre = crate::sim::movement::ground_pose::object_get_coords(
                b,
                self.resolved_terrain.as_ref(),
            );
            Some((
                [
                    i64::from(centre.x),
                    i64::from(centre.y),
                    i64::from(centre.z),
                ],
                object.helipad,
            ))
        }) else {
            return (death_effects, under_attack_events);
        };
        for &contact in contacts {
            let Some(at) = self.substrate.entities.get(contact).map(|c| {
                crate::sim::movement::ground_pose::object_get_coords(
                    c,
                    self.resolved_terrain.as_ref(),
                )
            }) else {
                continue;
            };
            let d = [
                i64::from(at.x) - centre[0],
                i64::from(at.y) - centre[1],
                i64::from(at.z) - centre[2],
            ];
            // `0x00442586`: the truncated length against 0x100.
            let far = d.iter().map(|v| v * v).sum::<i64>() >= 0x100 * 0x100;
            if far && !helipad {
                #[cfg(any(test, feature = "test-support"))]
                self.trace_lifecycle_for_test(LifecycleTestEvent::BuildingNowDeadRunAway {
                    building_id,
                    contact_id: contact,
                });
                crate::sim::radio::transmit(
                    self,
                    building_id,
                    contact,
                    crate::sim::radio::RadioMessage::RunAway,
                    crate::sim::radio::RadioPayload::default(),
                    rules,
                    run.effects(),
                );
                if let Some(unit) = self.substrate.entities.get_mut(contact) {
                    // `0x004425AA`, after RUN_AWAY returns.
                    unit.pending_entry.clear();
                }
            } else {
                //4425BA gets the contact's type, not its current Health.
                //LEA/SHL at4425CC..4425D4 are wrapping i32 arithmetic.
                let Some(damage) = self.substrate.entities.get(contact).and_then(|entity| {
                    self.object_type(entity.type_ref(), rules)
                        .map(|object| object.strength.wrapping_mul(10))
                }) else {
                    continue;
                };
                let warhead = self.interner.intern(&rules.bridge_warheads.c4_name);
                let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                    contact,
                    damage,
                    0,
                    crate::sim::combat::RAD_NO_ATTACKER,
                    None,
                    warhead,
                    crate::sim::combat::ReceiverCallFlags {
                        ignore_defenses: true,
                        arg6: true,
                    },
                );
                let nested = crate::sim::combat::world_receiver::commit_entities(
                    self,
                    run,
                    &[event],
                    Some(false),
                    rules,
                    overlay_registry,
                    &mut under_attack_events,
                );
                death_effects.append(nested);
            }
        }
        (death_effects, under_attack_events)
    }

    /// Dispatch the virtual `Stun` slot to its native class layer. Foot
    /// objects take `FootClass::Stun @ 0x004D5660` before the shared
    /// `TechnoClass::Stun @ 0x006FCD40`; non-Foot Technos enter the Techno
    /// body directly. This is shared by death, sinking, crash, lift release
    /// and sale callers, so only the Foot override runs its driver prefix.
    pub(crate) fn stun(&mut self, stable_id: u64, context: UninitContext<'_>) {
        let Some(category) = self
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| entity.category())
        else {
            return;
        };
        if category.is_foot() {
            self.foot_stun(stable_id, context);
        } else {
            self.techno_stun(stable_id, context);
        }
    }

    /// `FootClass::Stun @ 0x004D5660`: Set_Destination(NULL, 1)
    /// (`0x004D5667`), Path[0] = -1 (`0x004D5673`), Stop_Driver through
    /// vtable `+0x500` (Unit/aircraft locomotor stop or Infantry
    /// `0x0051DAF0`), then the base Techno stun.
    fn foot_stun(&mut self, stable_id: u64, mut context: UninitContext<'_>) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let category = entity.category();
        debug_assert!(category.is_foot());
        self.assign_null_destination(
            stable_id,
            context.rules,
            context.registry(),
            context.effects(),
        );
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.clear_live_path_head();
        }
        if category == EntityCategory::Infantry {
            if let Err(cause) = self.infantry_stop_driver(
                stable_id,
                context.rules(),
                context.registry(),
                context.effects(),
            ) {
                log::debug!("infantry {stable_id} Stun Stop_Driver: {cause}");
            }
        } else if let Err(cause) = self.locomotor_stop_moving(
            stable_id,
            context.rules,
            context.registry(),
            context.effects(),
        ) {
            log::debug!("{stable_id} Stun Stop_Driver: {cause}");
        }
        self.techno_stun(stable_id, context);
    }

    /// `TechnoClass::Stun @ 0x006FCD40`: Assign_Target(NULL),
    /// Set_Destination(NULL, 1), RADIO OVER_OUT (code 3) to every contact,
    /// then its Foot-class gate. Techno ctor `0x006F2B40` ORs AbstractFlags
    /// bit 0 at `0x006F3228`; Foot ctor `0x004D31E0` ORs bit 2 (`4`) at
    /// `0x004D34DA`, so the native `+0x14 & 4` test identifies Foot, not an
    /// active-state predicate. A non-Foot object (`JZ 0x006FCD7B`) and a Foot
    /// with `+0x6AD == 0` run Kill_All_Spawns (`0x006B7100`), ClearAllTargets
    /// (`0x006B7BB0`) and Detach_All(1); only a lifted Foot skips them.
    /// Deselect follows on every class. Foot's constructor clears `+0x6AD` at
    /// `0x004D3414`; ImbueLocomotor sets the lift latch at `0x00710352`, and
    /// the Jumpjet landing paths clear it. A Building reaches this from
    /// `TechnoClass::ReceiveDamage`'s death arm (`0x00702206..0x00702210`),
    /// usually after the Destroy callback's own Detach_All(1)
    /// (`0x005F5765..0x005F57AF`), so it detaches a second time like native.
    /// A further hit on a building already at Health 0 skips Destroy
    /// (`0x005F5752..0x005F575F`) but still takes the death arm, so this is
    /// its only detach.
    fn techno_stun(&mut self, stable_id: u64, mut context: UninitContext<'_>) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let category = entity.category();
        let walking_infantry = category == EntityCategory::Infantry
            && entity.locomotor.as_ref().is_some_and(|locomotor| {
                locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Walk
            });
        if walking_infantry {
            let _ = self.assign_target_represented(
                stable_id,
                None,
                context.rules,
                &mut FrameEffects::default(),
            );
        } else if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            crate::sim::mission::concrete_effects::represented_assign_target(
                entity,
                None,
                context.rules.ares_form(),
            );
        }
        self.assign_null_destination(
            stable_id,
            context.rules,
            context.registry(),
            context.effects(),
        );
        if !self.substrate.entities.contains(stable_id) {
            return;
        }
        crate::sim::radio::broadcast_break(self, stable_id, context.rules(), context.effects());
        // `0x006FCD6B..0x006FCD79`: a lifted Foot keeps its spawns, their
        // targets and its listeners. The `+0x14 & 4` test only guards the
        // Foot-only `+0x6AD` read; every other object runs the block.
        let foot_lifted = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category().is_foot() && entity.lift.swap_active());
        if !foot_lifted {
            crate::sim::spawn_manager::kill_all_spawns_with_context(
                self,
                stable_id,
                context.reborrow(),
            );
            crate::sim::spawn_manager::clear_all_spawn_targets(
                self,
                stable_id,
                context.rules(),
                context.effects(),
            );
            self.detach_all(stable_id, true, context);
        }
        // Deselect (vt+0x150, `0x006FCDA5`).
        self.object_deselect(stable_id);
    }

    pub(crate) fn apply_lifecycle_request(
        &mut self,
        request: LifecycleRequest,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        match request {
            LifecycleRequest::Uninit {
                stable_id,
                reason: _,
            } => {
                self.uninit_with_context(stable_id, UninitContext::new(rules).with_effects(effects))
            }
        }
    }

    pub(super) fn run_represented_uninit_pre_hook(&mut self, stable_id: u64) {
        // Object UnInit5F6616 expires damage-fire owners before Building's
        // destructor43BDE0 destroys the remaining slot Anims. Do not run the
        // recovery path here: it converts coordinates and stops sounds early.
        // Ordinary21slots have no Anim owner+CC and survive this broadcast;
        // Building43BDC5 clears them only at deferred scalar destruction.
        Self::close_kill_record(&mut self.substrate, stable_id);
        crate::sim::docking::bunker_link::break_links_on_despawn(&mut self.substrate, stable_id);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::UninitClassPre { stable_id });
    }
}
