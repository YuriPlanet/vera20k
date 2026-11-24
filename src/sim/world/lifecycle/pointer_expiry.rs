//! Pointer-expiry broadcast (`0x007258D0`): expiry-listener index and
//! roster, targeting sweeps, per-entity and anim expiry forwards, and
//! projectile target loss.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileTarget;
use crate::sim::world::expiry_listeners::{ExpiryListeners, ListenerStores};
use crate::sim::world::substrate::ObjectKind;
use crate::sim::world::{FrameEffects, Simulation};

#[cfg(any(test, feature = "test-support"))]
use super::LifecycleTestEvent;
use super::{
    LifecycleOutput, NULL_TARGET_CELL_SENTINEL, PointerExpiryControl, TargetingSweepReceipt,
    UninitContext, object_get_coords_cell,
};

impl Simulation {
    /// What `listener` holds that its pointer-expiry forwards can change:
    /// `name` gets each id a field [`Self::notify_entity_pointer_expired`] or
    /// a forward compares (repeats possible), and the result says whether it
    /// holds an object a forward reaches whatever expires (an airstrike
    /// manager, its parasite). A superset of each forward's own test, so
    /// passing over a listener that is not the expiring object, names nothing
    /// expiring and returns false matches visiting it. A field or forward
    /// added to the broadcast must be added here too.
    /// VERA-only: feeds the expiry-listener index (`ExpiryListeners`) so the
    /// `0x007258D0` broadcast skips entities; the listener's fields are the truth.
    pub(in crate::sim::world) fn entity_expiry_holds(
        listener: &GameEntity,
        mut name: impl FnMut(u64),
    ) -> bool {
        listener.held_anims().for_each(&mut name);
        for target in [
            listener.attack_target.target(),
            listener.seldom.suspended_attack_target.target(),
            listener.archive_target(),
        ] {
            if let Some(TargetKind::Entity(id)) = target {
                name(id);
            }
        }
        listener
            .seldom
            .radio_contacts
            .iter_live()
            .for_each(&mut name);
        listener
            .building
            .tesla_charge
            .chargers()
            .iter()
            .copied()
            .for_each(&mut name);
        if let Some(cargo) = listener.passenger_role().cargo() {
            cargo.passengers().iter().copied().for_each(&mut name);
        }
        listener
            .passenger_role()
            .occupants()
            .ids()
            .iter()
            .copied()
            .for_each(&mut name);
        let navigation = &listener.navigation;
        navigation
            .suspended_nav_com()
            .iter()
            .chain(navigation.nav_com().iter())
            .chain(navigation.nav_queue().iter())
            .chain(navigation.nav_list().iter())
            .filter_map(|target| target.object_id())
            .for_each(&mut name);
        [
            listener.unit_follower(),
            listener.passenger_role().inside_transport_id(),
            listener.passenger_role().transporter(),
            listener.pending_entry(),
            listener.seldom.spawn_owner_id.owner(),
            listener.seldom.fire_particle_system.system(),
            listener.seldom.spark_particle_system.system(),
            listener.seldom.railgun_particle_system.system(),
            listener.seldom.damage_smoke_system_id.system(),
            listener.ammo().and_then(|ammo| ammo.dock()),
            listener
                .building
                .pending_c4_detonation
                .pending()
                .and_then(|pending| pending.source_entity_id()),
            listener.lift.target(),
            listener.lift.source(),
            listener.seldom.drop_credit.manipulated_by(),
            listener.building.building_light.light(),
            listener.building.natural_particle_system.system(),
            listener.building.destroy_smoke_particle_system.system(),
        ]
        .into_iter()
        .flatten()
        .for_each(&mut name);
        listener
            .seldom
            .mega_mission
            .referenced_ids()
            .for_each(&mut name);
        listener.distributed_fire_references().for_each(&mut name);
        // The SpawnManager, CaptureManager and TemporalClass forwards act on
        // the listener's own expiry and on the ids they hold.
        if let Some(manager) = &listener.spawn_manager {
            manager.expiry_names().for_each(&mut name);
        }
        if let Some(manager) = &listener.capture_manager {
            manager.victims().for_each(&mut name);
        }
        listener
            .temporal
            .warp_target()
            .into_iter()
            .for_each(&mut name);
        listener.seldom.airstrike.owns_manager() || listener.parasite.eater().is_some()
    }

    /// Whether the pointer-expiry forwards an entity listener receives can
    /// change anything for `expired_id`: the listener is the expiring object,
    /// or [`Self::entity_expiry_holds`] names `expired_id` or says it acts on
    /// every expiry.
    pub(in crate::sim::world) fn entity_expiry_listener_acts(
        listener: &GameEntity,
        listener_id: u64,
        expired_id: u64,
    ) -> bool {
        let mut names = listener_id == expired_id;
        Self::entity_expiry_holds(listener, |id| names |= id == expired_id) || names
    }

    /// Each RNG stream's cursor, which every draw moves.
    #[cfg(any(test, feature = "test-support"))]
    fn rng_cursors(&self) -> [(u8, i32, i32); 3] {
        [
            self.scenario_rng.logical_state(),
            self.main_rng.logical_state(),
            self.mapgen_rng.logical_state(),
        ]
        .map(|view| (view.disabled, view.index_a, view.index_b))
    }

    /// Represented entries in global ObjectClass construction order, each
    /// with the store it lives in. Stable IDs are monotonic and never reused,
    /// so merging the separate Rust stores by ID reproduces the native
    /// registration order without walking holes left by already-finalized
    /// objects, and an ID's store never changes.
    ///
    /// gamemd-derived: active YR `ObjectClass` construction/destruction at
    /// `0x005F3900` / `0x005F3B80` maintains the listener roster in object
    /// construction order. Of the entities, it lists `entities` (ascending).
    /// Test builds visit this roster; production builds visit the listeners
    /// the expiry index finds, in the same order.
    #[cfg(any(test, feature = "test-support"))]
    fn removal_listener_order(&self, entities: Vec<u64>) -> Vec<(u64, ObjectKind)> {
        let mut listeners = Vec::new();
        for kind in ObjectKind::ALL {
            match kind {
                ObjectKind::Entity => listeners.extend(entities.iter().map(|&id| (id, kind))),
                ObjectKind::Anim => {
                    listeners.extend(self.substrate.anims.iter().map(|(&id, _)| (id, kind)));
                }
                ObjectKind::ParticleSystem => listeners.extend(
                    self.substrate
                        .particle_systems
                        .iter()
                        .map(|(&id, _)| (id, kind)),
                ),
                ObjectKind::Projectile => {
                    listeners.extend(self.projectiles.iter().map(|(&id, _)| (id, kind)));
                }
                ObjectKind::Wave => {
                    listeners.extend(self.waves.iter().map(|(&id, _)| (id, kind)));
                }
                ObjectKind::Terrain => {
                    listeners.extend(self.terrain_objects.objects().keys().map(|&id| (id, kind)));
                }
                ObjectKind::Smudge => {
                    if let Some(grid) = &self.smudge_grid {
                        listeners.extend(grid.objects().map(|(id, _)| (id, kind)));
                    }
                }
                // ObjectClass's PointerExpired (`0x005F5230`), which these
                // inherit, clears nothing these stores hold. A Spotlight's
                // (`0x00436A00`) runs apart, for every expiry.
                ObjectKind::VoxelAnim | ObjectKind::Particle | ObjectKind::Spotlight => {}
            }
        }
        // Each store yields its IDs in order: the run-adaptive stable sort
        // merges those runs instead of sorting from scratch.
        listeners.sort_by_key(|&(stable_id, _)| stable_id);
        debug_assert!(
            listeners.windows(2).all(|pair| pair[0].0 != pair[1].0),
            "object stable ID exists in more than one represented store"
        );
        listeners
    }

    /// The detach-time targeting sweep: release every object currently shooting
    /// at `detach_id`, which is leaving play *while still alive*.
    ///
    /// This is not the pointer-expiry broadcast. The detaching object survives —
    /// it is being sold, changing owner, teleporting, or being detached by area
    /// damage — so nothing else nulls the references pointed at it, and this is
    /// the only route by which a live-but-detached target releases its
    /// attackers. In an ordinary skirmish it fires tens of times per match:
    /// every building sale, every engineer capture, every mind-control or
    /// Psychic Beacon owner change, every Chrono Legionnaire or Chronosphere
    /// teleport.
    ///
    /// Three clauses are NOT copies of the pointer-expiry sweep and are
    /// reproduced verbatim because each is observable:
    ///
    /// 1. **Descending stable-ID iteration.** The native walk runs the global
    ///    techno vector from its last entry down to its first. When two objects
    ///    share the detaching target, the higher ID's class target/destination
    ///    setters run first. Restore does not itself dispatch mission AI.
    /// 2. **Restore runs first, with no suspended-mission pre-check.** The
    ///    expiry sweep asks whether a mission is suspended and clears the target
    ///    before restoring; this one restores unconditionally and clears after.
    /// 3. **The target clear is conditional on the Restore not having replaced
    ///    the target.** A successful Restore re-installs the archived target, and
    ///    the null-out then does not run.
    ///
    /// The native sweep has callers in area damage, building sale, owner change,
    /// building placement and teleportation. The represented area-damage,
    /// cliff, owner-change, bridge, wall-sale and teleport (`0x007193C7`)
    /// callers share this owner.
    ///
    /// After the Restore an aircraft now on Patrol restarts it
    /// (`0x0070D513..0x0070D532`, [`crate::sim::aircraft::restart_patrol_after_restore`]).
    pub(crate) fn stop_all_targeting_on_detach(
        &mut self,
        detach_id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        self.stop_all_targeting_target(TargetKind::Entity(detach_id), rules, effects);
    }

    /// AreaDamage, cliff, bridge, foundation and wall-sale callers share the
    /// 0x0070D4A0 sweep with a CellClass pointer. This is not PointerExpired:
    /// Restore precedes the conditional target clear, in descending Techno order.
    pub(crate) fn stop_all_targeting_cell(
        &mut self,
        rx: u16,
        ry: u16,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) -> TargetingSweepReceipt {
        self.stop_all_targeting_target(TargetKind::Cell(rx, ry), rules, effects)
    }

    fn stop_all_targeting_target(
        &mut self,
        target: TargetKind,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) -> TargetingSweepReceipt {
        // VERA-only: the sweep releases a batched target scan's pick as it
        // does a TarCom (`techno_ai::scan_batch`).
        if let TargetKind::Entity(target_id) = target {
            self.release_batched_scan_pick(target_id);
        }
        #[cfg(any(test, feature = "test-support"))]
        let mut cell_detaches = Vec::new();
        let mut listeners = EntityStore::descending_ids(|_| true);
        while let Some(listener_id) = listeners.next(&self.substrate.entities) {
            if let TargetKind::Entity(target_id) = target
                && self.airstrike_detach_preserves_target(listener_id, target_id)
            {
                continue;
            }
            if !self.listener_targets(listener_id, target) {
                continue;
            }

            // Native Restore cannot fail. VERA's preflight can refuse it
            // (see the expiry receiver's Restore below); the sweep carries on.
            let restored = self
                .mission_restore_represented(listener_id, rules, effects)
                .unwrap_or_else(|error| {
                    log::error!("detach sweep: listener {listener_id} kept its override: {error}");
                    false
                });
            // `0x0070D513..0x0070D532`, whatever the Restore did.
            if let Some(listener) = self.substrate.entities.get_mut(listener_id) {
                crate::sim::aircraft::restart_patrol_after_restore(listener);
            }

            let target_cleared = self.listener_targets(listener_id, target);
            if target_cleared {
                self.assign_target_represented(listener_id, None, rules, effects)
                    .expect("detach sweep listener remains present for the target clear");
            }

            let _ = restored;
            #[cfg(any(test, feature = "test-support"))]
            if matches!(target, TargetKind::Cell(..)) {
                cell_detaches.push(crate::sim::combat::combat_aoe::CellTargetDetach {
                    listener_id,
                    restored,
                    cleared: target_cleared,
                });
            }
            #[cfg(any(test, feature = "test-support"))]
            if let TargetKind::Entity(detach_id) = target {
                self.trace_lifecycle_for_test(LifecycleTestEvent::DetachTargetingSweepVisited {
                    detach_id,
                    listener_id,
                    restored,
                    target_cleared,
                });
            }
        }
        self.team_script_vm
            .detach_target(target.into(), self.resolved_terrain.as_ref());
        TargetingSweepReceipt {
            #[cfg(any(test, feature = "test-support"))]
            cell_detaches,
        }
    }

    /// Match the retained target identity, preserving Cell/Techno distinction.
    /// The detach sweep `0x0070D4A0`'s TarCom test `CMP [ESI+0x2B4]` (`0x0070D539`).
    fn listener_targets(&self, listener_id: u64, target: TargetKind) -> bool {
        self.substrate
            .entities
            .get(listener_id)
            .and_then(|listener| listener.attack_target.target())
            .is_some_and(|held| {
                //70D539 compares pointers, including fixed512 aliases and
                //the one shared Dummy. Reading an already held Cell never
                //restamps that Dummy's current coordinate.
                if let (TargetKind::Cell(..), TargetKind::Cell(..), Some(terrain)) =
                    (held, target, self.resolved_terrain.as_ref())
                {
                    held.cell_identity(terrain) == target.cell_identity(terrain)
                } else {
                    held == target
                }
            })
    }

    /// Infantry PerCell repair519D17..519D36 calls each registered Infantry
    /// +28(hut,false) in descending construction order. The hut survives;
    /// this is not the global ObjectClass removal broadcast. Actual Infantry
    /// +28 is51AA10 -> Foot4D9960 -> Techno7077C0. Its extra +6C0 clear
    /// compares an InfantryType pointer with the hut and cannot match.
    pub(crate) fn expire_infantry_bridge_hut_targets(
        &mut self,
        hut_id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        // Infantry ctor517B34 appends to its type registry. Represented
        // PointerExpired receivers do not construct/delete registry entries;
        // stable construction IDs therefore preserve its descending cursor.
        let mut listeners =
            EntityStore::descending_ids(|category| category == EntityCategory::Infantry);
        while let Some(listener_id) = listeners.next(&self.substrate.entities) {
            let Some(hut) = self.substrate.entities.get(hut_id) else {
                return;
            };
            let facts = (
                object_get_coords_cell(hut),
                hut.lifecycle.object_alive,
                hut.health.current(),
                hut.mission.current().known() == Some(crate::sim::mission::MissionType::Selling),
                hut.owner(),
            );
            self.notify_entity_pointer_expired(
                listener_id,
                hut_id,
                facts.0,
                facts.1,
                facts.2,
                facts.3,
                Some(facts.4),
                PointerExpiryControl::DetachAll,
                rules,
                effects,
            );
            // Techno707B24 forwards this manager independently of control.
            crate::sim::spawn_manager::notify_pointer_expired(
                self,
                listener_id,
                hut_id,
                rules,
                effects,
            );
        }
    }

    /// Techno70F770 and the identical PointerExpired7079D1..7A28 arm
    /// shorten the passive targeting timer (+180/+188), never weapon rearm.
    /// The native priority bracket A8E7AC suppresses both the draw and write.
    /// Original controls: infantry_deploy_action.json's reload rows.
    pub(crate) fn shorten_passive_scan_timer(&mut self, id: u64) -> bool {
        let now = self.session.binary_frame();
        if self.object_placement_scope_active()
            || self
                .substrate
                .entities
                .get(id)
                .is_none_or(|entity| entity.passive_scan.timer().remaining(now) <= 10)
        {
            return false;
        }
        let delay = self.scenario_rng.next_range_u32_inclusive(4, 8);
        self.substrate
            .entities
            .get_mut(id)
            .expect("queried passive timer owner remains present")
            .passive_scan
            .arm_timer(now, delay);
        true
    }

    /// The broadcast passes over listeners that
    /// [`Self::entity_expiry_listener_acts`] rejects, so a field compared here
    /// must be named or flagged by [`Self::entity_expiry_holds`] too. That
    /// enumeration may read only the listener: the expiry index re-reads an
    /// entity only when the store hands that entity out mutably.
    pub(in crate::sim::world) fn notify_entity_pointer_expired(
        &mut self,
        listener_id: u64,
        expired_id: u64,
        // The expiring object's `ObjectClass::GetCoords` cell, which
        // `TechnoClass::PointerExpired` hands to `SensorCountForHouse` when it
        // computes `allowClear` on the `Detach_All` control.
        expired_get_coords_cell: Option<(u16, u16)>,
        expired_object_alive: bool,
        expired_health: i32,
        expired_is_selling: bool,
        expired_owner: Option<InternedId>,
        control: PointerExpiryControl,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        // ObjectClass::PointerExpired5F5268 clears +88 regardless of control.
        if self
            .substrate
            .entities
            .get(listener_id)
            .is_some_and(|listener| listener.parachute_animation() == Some(expired_id))
        {
            self.substrate
                .entities
                .get_mut(listener_id)
                .unwrap()
                .parachute_animation_expired(expired_id);
        }
        let Some(listener) = self.substrate.entities.get(listener_id) else {
            return;
        };
        let current_target_matches = listener
            .attack_target
            .target()
            .is_some_and(|target| matches!(target, TargetKind::Entity(id) if id == expired_id));
        let listener_owner = listener.owner();
        let mission_is_suspended =
            listener.mission.suspended() != crate::sim::mission::MissionId::NONE;
        // What the radio and cargo clears below would change. A listener is
        // handed out mutably only when something changes: every hand-out
        // enters the store's touch logs, and most listeners hold no
        // reference to the expiring object.
        let drops_contact =
            control == PointerExpiryControl::Uninit && listener.has_live_contact_with(expired_id);
        // `0x007077D8..0x007077E9`: CargoClass::Remove runs only when the
        // `removed` argument is set, so a cloaking rider stays aboard.
        let drops_passenger = control == PointerExpiryControl::Uninit
            && listener
                .passenger_role()
                .cargo()
                .is_some_and(|cargo| cargo.passengers().contains(&expired_id));
        // `0x007077EE..0x007077FD`, after the radio and cargo, whatever the
        // control: a child whose SpawnOwner (`+0x2D4`) expires forgets it.
        let drops_spawn_owner = listener.seldom.spawn_owner_id.owner() == Some(expired_id);

        // `TechnoClass::PointerExpired @ 0x007077C0`, the `allowClear` local
        // (`[ESP+0x24]`): it starts true and is cancelled only on the
        // `Detach_All` control, when the expiring object is a Techno and the
        // RECEIVER's own house already holds a sensor count on that object's
        // cell — `0x00707994 CALL 0x004870D0`. The Techno half of that guard is
        // `AbstractFlags +0x14` bit 0, ORed in for every object by
        // `TechnoClass__Constructor @ 0x006F3228` and therefore true of every
        // `GameEntity`. Terrain retains the Object flags without that bit,
        // so its expiry never asks the sensor guard. `allowClear` gates both the
        // Target (`+0x2B4`) clear at `0x007079AD` and the `+0x2B8` clear at
        // `0x00707A7A`.
        let allow_clear = match control {
            PointerExpiryControl::Uninit => true,
            PointerExpiryControl::DetachAll => {
                !self.substrate.entities.contains(expired_id)
                    || !expired_get_coords_cell.is_some_and(|(rx, ry)| {
                        self.fog.has_sensor_for_house(listener_owner, rx, ry)
                    })
            }
        };
        // `0x007079B7..0x007079CB`: on the same control the Target clear is
        // additionally skipped when the expiring object's `GetOwner` (vslot
        // `+0x3C`) equals the receiver's `+0x21C`, so a diving submarine never
        // drops its own house's shooters.
        let same_owner_exempt =
            control == PointerExpiryControl::DetachAll && expired_owner == Some(listener_owner);
        let clears_current_target = current_target_matches && allow_clear && !same_owner_exempt;

        // `0x007079D1..0x00707A0D`: the re-arm sits INSIDE the guarded arm,
        // ahead of `Assign_Target(NULL)`, so a receiver that keeps its target
        // spends no Scenario draw. An already-expired timer (`elapsed >=
        // duration`) skips the block entirely, which `remaining()`'s clamp to 0
        // reproduces.
        if clears_current_target {
            self.shorten_passive_scan_timer(listener_id);
        }
        if (drops_contact || drops_passenger || drops_spawn_owner)
            && let Some(listener) = self.substrate.entities.get_mut(listener_id)
        {
            // `RadioClass::PointerExpired @ 0x0065AAC0` nulls matching sparse
            // slots in place, but ONLY on a nonzero control:
            //
            // ```
            // 0065aac1 MOV EBX,[ESP+0xc]      ; EBX = the control argument
            // 0065aae6 MOV EDX,[EAX + ECX*4]  ; slot
            // 0065aaec CMP EDX,EDI            ; slot == expired ?
            // 0065aaee JNZ 0065aafa
            // 0065aaf0 TEST BL,BL             ; control
            // 0065aaf2 JZ   0065aafa          ; control 0 skips the clear
            // 0065aaf4 MOV dword ptr [EAX],0x0
            // ```
            //
            // So `Detach_All(false)` — the dive — leaves radio contacts intact,
            // and only UnInit breaks them. A diving submarine therefore keeps
            // its naval-yard repair link and its transport link.
            if drops_contact {
                listener.clear_live_contact_with(expired_id);
            }

            // TechnoClass removes an expiring passenger from its CargoClass before
            // clearing its target/archive/manager reference family.
            if drops_passenger && let Some(cargo) = listener.passenger_role_mut().cargo_mut() {
                let _ = cargo.disembark(expired_id);
            }
            if drops_spawn_owner {
                listener.seldom.spawn_owner_id.forget();
            }
        }

        if clears_current_target {
            self.assign_target_represented(listener_id, None, rules, effects)
                .expect("expiry listener remains present");
            if mission_is_suspended {
                // Native Restore (`0x00707A51`) cannot fail. VERA's is one
                // transaction whose preflight refuses when a class setter's
                // inputs are missing: an archived NavCom naming no object, no
                // overlay table for a Teleport destination. Residual: the
                // listener then keeps its override mission with the target
                // cleared, until its next mission change. Rare; the broadcast
                // carries on instead of stopping the game.
                if let Err(error) = self.mission_restore_represented(listener_id, rules, effects) {
                    log::error!("expiry: listener {listener_id} kept its override: {error}");
                }
                // `0x00707A51..0x00707A6C`.
                if let Some(listener) = self.substrate.entities.get_mut(listener_id) {
                    crate::sim::aircraft::restart_patrol_after_restore(listener);
                }
            }
        }

        // Decide every clear from a read, then hand the listener out mutably
        // only when one applies (see `drops_contact`).
        let Some(listener) = self.substrate.entities.get(listener_id) else {
            return;
        };
        // `+0x2B8` is cleared on an exact match under `allowClear` alone — the
        // same-owner exemption applies only to `+0x2B4`.
        let clear_suspended_target = allow_clear
            && matches!(
                listener.seldom.suspended_attack_target.target(),
                Some(TargetKind::Entity(id)) if id == expired_id
            );
        // Techno707AC2..707AD7 clears eight retained effect pointers on an
        // exact match, including damage-Smoke+310. It precedes the control
        // gate707AE7 and is independent of the target's sensor/owner guards.
        let clear_damage_smoke =
            listener.seldom.damage_smoke_system_id.system() == Some(expired_id);
        let clear_fire_system = listener.seldom.fire_particle_system.system() == Some(expired_id);
        let clear_spark_system = listener.seldom.spark_particle_system.system() == Some(expired_id);
        let clear_railgun_system =
            listener.seldom.railgun_particle_system.system() == Some(expired_id);
        // A Building's natural `+0x30C` and destroy-smoke `+0x320` links are
        // two more of those eight.
        let clear_natural_system =
            listener.building.natural_particle_system.system() == Some(expired_id);
        let clear_destroy_smoke =
            listener.building.destroy_smoke_particle_system.system() == Some(expired_id);
        // Techno707AE7..707B03 clears ArchiveTarget+218 on control1. The
        // later Foot4D99F1..4D99FC clears a still-matching archive on both
        // controls, independent of sensors and same-owner target exemptions.
        // Only Unit/Infantry/Aircraft inherit that additional Foot clear.
        // Native execution: spatial_oracle/foot_archive_expiry.{json,md}.
        let foot_receiver = matches!(
            listener.category(),
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        );
        let clear_archive_target = (control == PointerExpiryControl::Uninit || foot_receiver)
            && listener.archive_target() == Some(TargetKind::Entity(expired_id));

        // FootClass clears SuspendedNavCom first, then its current/aux target,
        // and removes every matching queue entry. Cell targets are unaffected.
        //
        // `FootClass::PointerExpired @ 0x004D9960` carries its OWN copy of the
        // `allowClear` Boolean, computed by a SECOND `SensorCountForHouse`
        // call — `0x004D9A57 CALL 0x004870D0`, at the expiring object's
        // `GetCoords` cell (`0x004D9A3D` vslot `+0x48`) for the RECEIVER's house
        // (`param_1[0x87] + 0x30`) — under the same control-0 / nonnull /
        // `+0x14` bit-0 guard as the Techno body. It gates the `+0x5A0`/`+0x5A4`
        // PAIR clear below; the `+0x5A8` clear above it is unguarded. The two
        // Booleans are equal in value because both read the receiver's house at
        // the same cell, so the single `allow_clear` local models both.
        let expired_nav = NavTargetRef::object(expired_id);
        let clear_suspended_nav_com = listener.navigation.suspended_nav_com() == Some(expired_nav);
        let current_nav_matches = listener.navigation.nav_com() == Some(expired_nav);
        let retain_capture_nav = current_nav_matches
            && listener.category() == EntityCategory::Infantry
            && listener.seldom.removal_facts.occupier()
            && listener.mission.current().known()
                == Some(crate::sim::mission::MissionType::Capture)
            && expired_object_alive
            && expired_health > 0
            && !expired_is_selling;
        //4D9A0F gates the pair on current NavCom identity;4D9ABD then
        //clears both fields, irrespective of the auxiliary pointer's value.
        let clear_nav_com = current_nav_matches && !retain_capture_nav && allow_clear;
        let prune_nav_queue = listener.navigation.nav_queue().contains(&expired_nav);
        // `0x004D9B43..0x004D9B95`: every navigation list entry naming the
        // expiring object goes, before the NavQueue's.
        let prune_nav_list = listener.navigation.nav_list().contains(&expired_nav);

        // Techno707AE7..707AF5 clears private pending+500 only on control1.
        // Non-destructive DetachAll(false) retains this independent entry;
        // radio contact expiry owns the admitted visit.
        let clear_dock =
            control == PointerExpiryControl::Uninit && listener.pending_entry() == Some(expired_id);
        // Aircraft PointerExpired clears its dock (`+0x6CC`) for either
        // control (`0x0041B673..0x0041B67F`).
        let clear_airfield = listener
            .ammo()
            .is_some_and(|ammo| ammo.dock() == Some(expired_id));

        // Steam gamemd, TechnoPointerExpired7078B2..7078DB:
        // Transporter+11C survives a live hull's ordinary Limbo announcement,
        // for either control. Clear only on Object+90==0, exact Health==0 or
        // ScenarioInit(A8E7AC)!=0 (`0x007078D5`). Negative live HP is not
        // this zero test. VERA's carrier relation follows the same test.
        let hull_gone =
            !expired_object_alive || expired_health == 0 || self.object_placement_scope_active();
        let clear_transporter =
            hull_gone && listener.passenger_role().transporter() == Some(expired_id);
        let clear_passenger_role =
            hull_gone && listener.passenger_role().inside_transport_id() == Some(expired_id);
        let clear_c4_source = listener
            .building
            .pending_c4_detonation
            .pending()
            .as_ref()
            .is_some_and(|pending| pending.source_entity_id() == Some(expired_id));
        // `0x004D9AC9..0x004D9B43`, on either control: an attack-move's
        // MegaTarget becomes a MegaDestination at its GetCoords cell, and its
        // MegaDestination is dropped.
        let expire_mega_pointer = listener.seldom.mega_mission.references(expired_id);
        // Building44EA77 removes a charger for either control, irrespective
        // of the base target exemptions. Only later AI changes the weapon latch.
        let clear_tesla_charger = listener
            .building
            .tesla_charge
            .chargers()
            .contains(&expired_id);
        // BuildingPointerExpired44EAB8..44EB02 removes a matching Occupant
        // only inside ScenarioInit(A8E7AC), after inherited Techno cleanup.
        // It neither depends on removed/control nor rewrites the fire cursor.
        let clear_occupant = listener.category() == EntityCategory::Structure
            && self.object_placement_scope_active()
            && listener
                .passenger_role()
                .occupants()
                .ids()
                .contains(&expired_id);
        // BuildingPointerExpired44E932..44E93A clears its owned child for
        // either control, before that storage is physically removed.
        let clear_spotlight = listener.category() == EntityCategory::Structure
            && listener.building.building_light.light() == Some(expired_id);
        if !(clear_suspended_target
            || clear_fire_system
            || clear_spark_system
            || clear_railgun_system
            || clear_natural_system
            || clear_destroy_smoke
            || clear_damage_smoke
            || clear_archive_target
            || clear_suspended_nav_com
            || clear_nav_com
            || prune_nav_list
            || prune_nav_queue
            || clear_dock
            || clear_airfield
            || clear_transporter
            || clear_passenger_role
            || clear_c4_source
            || expire_mega_pointer
            || clear_tesla_charger
            || clear_spotlight
            || clear_occupant)
        {
            return;
        }

        let listener = self
            .substrate
            .entities
            .get_mut(listener_id)
            .expect("expiry listener read above");
        if clear_suspended_target {
            listener.seldom.suspended_attack_target.clear();
        }
        if clear_damage_smoke {
            listener.seldom.damage_smoke_system_id.forget();
        }
        if clear_fire_system {
            listener.seldom.fire_particle_system.forget();
        }
        if clear_spark_system {
            listener.seldom.spark_particle_system.forget();
        }
        if clear_railgun_system {
            listener.seldom.railgun_particle_system.forget();
        }
        if clear_natural_system {
            listener.building.natural_particle_system.forget();
        }
        if clear_destroy_smoke {
            listener.building.destroy_smoke_particle_system.forget();
        }
        if clear_archive_target {
            listener.set_archive_target(None);
        }
        if clear_suspended_nav_com {
            listener.navigation.clear_suspended_nav_com();
        }
        if clear_nav_com {
            listener.navigation.expire_nav_com();
        }
        if prune_nav_list {
            listener
                .navigation
                .retain_nav_list(|target| *target != expired_nav);
        }
        if prune_nav_queue {
            listener
                .navigation
                .retain_nav_queue(|target| *target != expired_nav);
        }
        if clear_dock {
            // `0x00707AF5`.
            listener.pending_entry.expire(expired_id);
        }
        if clear_airfield && let Some(ammo) = listener.ammo_mut() {
            ammo.set_dock(None);
        }
        if clear_transporter {
            listener.passenger_role_mut().clear_transporter();
        }
        if clear_passenger_role {
            listener.passenger_role_mut().clear_carrier_relation();
        }
        if clear_c4_source {
            listener.building.pending_c4_detonation.forget_source();
        }
        if expire_mega_pointer {
            listener
                .seldom
                .mega_mission
                .pointer_expired(expired_id, expired_get_coords_cell);
        }
        if clear_tesla_charger {
            listener.building.tesla_charge.pointer_expired(expired_id);
        }
        if clear_spotlight {
            listener.building.building_light.take();
        }
        if clear_occupant {
            listener
                .passenger_role_mut()
                .occupants_mut()
                .remove_expired(expired_id);
        }
    }

    /// ObjectClass::Detach_From_All_Lists represented listener broadcast.
    ///
    /// The callback pass runs before removing the target from its store.
    /// A never-Fired Bullet is still in Limbo; ordinary Object UnInit reaches
    /// this boundary while its target is alive and marked. Listener UnInit may
    /// queue deletion, but final erasure waits for the common drain, retaining
    /// the construction-order cursor through nested callbacks.
    ///
    /// gamemd-derived: active YR `DispatchPointerExpiredCleanup @ 0x007258D0`
    /// is called directly by `ObjectClass__UnInit @ 0x005F65F0` and by
    /// `ObjectClass::Detach_All @ 0x005F5280` (the killing hit's Destroy, the
    /// virtual Conceal path and the Stun).
    /// Every listener uses the live MapClass authority, including when a receiver
    /// has temporarily lent that grid through UninitContext. The remaining arms
    /// read the object's liveness, health and mission from the same world.
    pub(crate) fn notify_pointer_expired(&mut self, expired_id: u64, context: UninitContext<'_>) {
        // A VoxelAnim (WhatAmI 0x29, `0x0074AB20`) takes the generic arm at
        // `0x00725925` too, which reaches every particle system, so an
        // AttachedSystem's owner hears of the anim's death.
        if !self.substrate.spotlights.contains_key(expired_id)
            && !self.substrate.entities.contains(expired_id)
            && !self.substrate.voxel_anims.contains_key(expired_id)
            && !self.substrate.particle_systems.contains_key(expired_id)
            && self
                .substrate
                .particle_systems
                .detached_particle(expired_id)
                .is_none()
            && !self.terrain_objects.objects().contains_key(&expired_id)
            && self.projectiles.get(expired_id).is_none()
            && self
                .smudge_grid
                .as_ref()
                .and_then(|grid| grid.object(expired_id))
                .is_none()
        {
            return;
        }

        #[cfg(any(test, feature = "test-support"))]
        if let Some(tree) = self.terrain_objects.objects().get(&expired_id) {
            self.trace_lifecycle_for_test(LifecycleTestEvent::TerrainExpiryBoundary {
                stable_id: expired_id,
                cell: tree.cell(),
                indexed: self.terrain_objects.cell_index().get(&tree.cell()) == Some(&expired_id),
                in_logic: tree.in_logic_vector(),
                live: tree.is_live(),
            });
        }

        // gamemd-derived: the House pointer-expiry handler reached by this
        // synchronous broadcast stable-removes a BuildConst Building while it
        // is still alive, marked, and resolvable. Keeping the House mutation
        // at this callback boundary also covers direct UnInit/destruction;
        // Conceal's already-limbo return never reaches it.
        Self::remove_build_const_from_owner(&self.substrate, &mut self.houses, expired_id);
        Self::remove_house_base_membership(&self.substrate, &mut self.houses, expired_id);
        // `TechnoClass::PointerExpired @ 0x0070785F..0x0070792A` (and the
        // death arm of `ReceiveDamage @ 0x0070206A`): both halves of a drain
        // link drop when either object expires.
        crate::sim::credit_income::clear_drain_links_on_expiry(self, expired_id, context.rules);
        self.broadcast_pointer_expired(expired_id, PointerExpiryControl::Uninit, context);
    }

    /// `DispatchPointerExpiredCleanup @ 0x007258D0` with control **0**, as
    /// `FootClass::Crash` calls it directly (`0x004DED4F`). `Detach_All(false)`
    /// ([`Self::detach_all`]) ends with the same announcement. It runs the same
    /// `TechnoClass::PointerExpired` body as the UnInit path over the same
    /// registered-object roster. The detaching object survives, so no
    /// House-side BuildConst removal happens here.
    ///
    /// **The non-Techno listeners on that roster are control-INSENSITIVE**, read
    /// this session and no longer a residual:
    ///
    /// * `WaveClass::PointerExpired @ 0x0075F610` (vtable `0x007F6BF4 + 0x28`)
    ///   and `ParticleSystemClass::PointerExpired @ 0x0062FE90` branch on the
    ///   control only inside the inherited `ObjectClass` call.
    /// * `AnimClass`'s `+0x28` override is `0x00425150` (vtable
    ///   `0x007E3354 + 0x28`; its only xref is that slot). It too forwards the
    ///   control to `ObjectClass::PointerExpired` and takes no further branch on
    ///   it.
    /// * `BulletClass::PointerExpired @ 0x004684E0` branches on the control only
    ///   for its trailing global-vector erase; the `+0x10C` target repair that
    ///   substitutes `Get_CellClass` at the expired object's last cell — the arm
    ///   `ProjectileStore::pointer_expired` receives — is unguarded.
    /// * `ObjectClass::PointerExpired @ 0x005F5230` gates the `+0x30`
    ///   successor relink on a nonzero control. Terrain's retained cell
    ///   subsequence follows that gate through the same listener index.
    ///
    /// So a torpedo already in flight at a diving submarine falling to the sub's
    /// last cell instead of tracking it is native behavior, not an approximation.
    ///
    /// The victim's FootClass parasite forward is control-insensitive too, so a
    /// cloaking host releases its parasite; the rules place the released owner.
    pub fn detach_all_pointer_expired(
        &mut self,
        expired_id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        self.announce_detach_all(expired_id, UninitContext::new(rules).with_effects(effects));
    }

    /// [`Self::detach_all_pointer_expired`] under a caller's context.
    pub(super) fn announce_detach_all(&mut self, expired_id: u64, context: UninitContext<'_>) {
        if self.substrate.entities.contains(expired_id) {
            self.broadcast_pointer_expired(expired_id, PointerExpiryControl::DetachAll, context);
        }
    }

    /// Teleport Process7193CC..7193F4 calls Bullet::LoseTarget468430 on every
    /// matching Bullet in reverse construction order, including non-Logic
    /// bullets. Stable IDs retain that construction order in the store.
    pub(crate) fn lose_projectile_targets(&mut self, target_id: u64, rules: &RuleSet) {
        if !self
            .projectiles
            .iter()
            .any(|(_, bullet)| bullet.target().object_id() == Some(target_id))
        {
            return;
        }
        // Every matching Bullet queries the same unchanged object. Resolve
        // once: no RNG/timer writes occur, and repeated dummy stamps agree.
        let replacement = self.projectile_lost_target(target_id, UninitContext::new(rules));
        self.projectiles.lose_targets(target_id, replacement);
    }

    /// Shared target repair in LoseTarget468430 and PointerExpired468517.
    /// Both read +48, pack signed cells, then test A8E7AC, IsInAir, and the
    /// empty-cell sentinel before Map5657A0. The map owns aliasing and Dummy.
    fn projectile_lost_target(
        &self,
        target_id: u64,
        context: UninitContext<'_>,
    ) -> ProjectileTarget {
        let entity = self.substrate.entities.get(target_id);
        let xy = entity
            .map(crate::sim::movement::ground_pose::object_center_xy)
            .or_else(|| {
                self.substrate
                    .particle_systems
                    .get(target_id)
                    .map(|system| [system.coords().x, system.coords().y])
            })
            .or_else(|| {
                self.terrain_objects.objects().get(&target_id).map(|tree| {
                    let coord = tree.world_coord();
                    [coord.x, coord.y]
                })
            })
            .or_else(|| {
                self.smudge_grid
                    .as_ref()?
                    .object(target_id)
                    .map(|smudge| [smudge.location().x, smudge.location().y])
            })
            .or_else(|| {
                self.spotlight_object_coord(target_id)
                    .map(|[x, y, _]| [x, y])
            });
        let Some([x, y]) = xy else {
            return ProjectileTarget::None;
        };
        let (rx, ry) = (
            crate::util::lepton::lepton_to_cell_packed(x),
            crate::util::lepton::lepton_to_cell_packed(y),
        );
        let terrain = context.terrain().or(self.resolved_terrain.as_ref());
        if self.object_placement_scope_active()
            || entity.is_some_and(|entity| {
                crate::sim::movement::air_movement::is_high_flying(
                    entity,
                    terrain,
                    Some((context.rules(), &self.interner)),
                )
            })
            || self
                .terrain_objects
                .objects()
                .get(&target_id)
                .is_some_and(|tree| tree.is_in_air(terrain))
            || (rx, ry) == NULL_TARGET_CELL_SENTINEL
        {
            return ProjectileTarget::None;
        }
        // Real worlds query MapClass and retain its returned identity.
        ProjectileTarget::looked_up_cell(terrain, rx as u16, ry as u16)
    }

    /// Anim4's dedicated holder announcement725A16..725A4C, before Anim
    /// conceal/removal (UnInit42561F, destructor422906). It does not enter
    /// the generic Object/bomb/factory/team broadcast. This dispatches the
    /// Bullet, Parasite and Ares EMP-holder subsets; Building receivers stay at their existing boundaries,
    /// including their distinct ordinary-UnInit versus scalar slot handling.
    pub(crate) fn notify_anim_pointer_expired(&mut self, expired_id: u64) {
        if !self.substrate.anims.contains_key(expired_id) {
            return;
        }
        // Only an Ares Anim shape joins these listeners
        // (`AlphaShapeClass_CTOR_Anims`); it touches no other listener's state.
        self.alpha_shapes.anim_pointer_expired(expired_id);
        // VERA-only: each callback below clears only its own entity's fields
        // that name the Anim (`GameEntity::held_anims`) and draws nothing, so
        // visiting just the entities the anim-holder index finds, ascending,
        // changes exactly what visiting every entity would.
        let holders = self.substrate.entities.take_anim_holders(expired_id);
        for owner in holders {
            if let Some(entity) = self.substrate.entities.get_mut(owner) {
                // Techno's Anim callback710455 reaches Object5F6DA0.
                entity.parachute_animation_expired(expired_id);
                entity.emp_anim_pointer_expired(expired_id);
                // Ares 10058770: only Ares code attaches an effect's Anim, so
                // only a holder of this one writes its Ares part.
                if entity
                    .attached_effect_anims()
                    .any(|anim| anim == expired_id)
                {
                    entity.attached_effect_anim_expired(expired_id);
                }
            }
            self.substrate
                .parasite_anim_pointer_expired(owner, expired_id);
        }
        let acting = self
            .expiry_listeners
            .projectiles_reached_by(&mut self.projectiles, expired_id);
        if crate::sim::touch_log::live_read_check_enabled() {
            let expected: Vec<u64> = self
                .projectiles
                .iter()
                .filter(|(_, bullet)| bullet.expiry_names().contains(&Some(expired_id)))
                .map(|(&id, _)| id)
                .collect();
            assert_eq!(
                acting, expected,
                "Anim expiry listeners diverged from a live read"
            );
        }
        for id in acting {
            // A held Anim cannot be a Techno/terrain target or firer. The
            // expired RTTI is Anim4, so the Scalable Bullet arm stays off.
            self.projectiles
                .pointer_expired(id, expired_id, ProjectileTarget::None, false)
                .expect("indexed Bullet remains stored during Anim expiry");
        }
    }

    /// The pointer-expiry listener index, with the stores it reads.
    fn expiry_listener_index(&mut self) -> (&mut ExpiryListeners, ListenerStores<'_>) {
        (
            &mut self.expiry_listeners,
            ListenerStores {
                entities: &mut self.substrate.entities,
                anims: &mut self.substrate.anims,
                particle_systems: &mut self.substrate.particle_systems,
                projectiles: &mut self.projectiles,
                waves: &mut self.waves,
                terrain: &mut self.terrain_objects,
            },
        )
    }

    fn broadcast_pointer_expired(
        &mut self,
        expired_id: u64,
        control: PointerExpiryControl,
        mut context: UninitContext<'_>,
    ) {
        // VERA-only: a batched target scan holds its pick across the turns
        // before its scanner's as a TarCom does (`techno_ai::scan_batch`).
        self.release_batched_scan_pick(expired_id);
        // `AlphaShapeClass::PointerExpired @ 0x00420E70` listens whatever the
        // control and touches no other listener's state, so its place among
        // the listeners changes nothing.
        self.alpha_shapes.pointer_expired(expired_id);
        // `0x0072590E..0x0072591F`: whatever the control, an expiring
        // ParticleSystem that is the gas cloud clears that global first.
        self.substrate
            .particle_systems
            .expire_gas_cloud_system(expired_id);
        // `DispatchPointerExpiredCleanup @ 0x007258D0` calls
        // `BombListClass::PointerGotInvalid @ 0x00439150` after its listener
        // loop (0x00725961) whatever the control, so a cloaking planter loses
        // its bombs' credit as a removed one does. No listener reads a planter,
        // so running it first changes nothing. Ares
        // AnnounceInvalidPointer_BombCloak (10056750, hook 0x725961) skips it
        // unless the object is removed: a cloaking planter keeps its credit.
        let announce_to_bombs = !context.rules.ares() || control == PointerExpiryControl::Uninit;
        if announce_to_bombs {
            Self::bomb_planter_expired(&mut self.substrate, &self.bombs, expired_id);
        }
        // The three listener families below compare the expired object with
        // what they hold and write only on a match. A team holds Technos and
        // the Terrain object that blocks its path, a factory its product and
        // a house the building it lost, so the end of any other object (a
        // Bullet, a Particle, a Smudge) matches nothing in them: its
        // broadcast skips their walks (VERA-only, results unchanged). A debug
        // build under the live-read check still walks, and asserts that.
        // RESIDUAL: restore checks no team target and no House+54E0, so a
        // save edited by hand can make one name such an object. Its end then
        // leaves the name where the walk cleared it, and a checked debug
        // build asserts. The game's own saves never hold one; such a team
        // would wait on an object that no longer exists.
        let techno = self.substrate.entities.contains(expired_id);
        let team_object = techno || self.terrain_objects.objects().contains_key(&expired_id);
        let checked = crate::sim::touch_log::live_read_check_enabled();
        // `TeamClass::PointerExpired @ 0x006EAE60`, each team a listener.
        if team_object || checked {
            let named = self
                .team_script_vm
                .pointer_expired(expired_id, control == PointerExpiryControl::Uninit);
            debug_assert!(
                team_object || !named,
                "a team named {expired_id}, which is neither a Techno nor a Terrain object"
            );
        }
        if techno || checked {
            // FactoryClass::PointerExpired4CA580 is a control-insensitive
            // listener in Dispatch7258D0. Its only mutation clears a matching
            // held product; no entity listener consumes that factory pointer.
            let mut named = self.production.factory_product_expired(expired_id);
            // `HouseClass::PointerExpired @ 0x004FB9B0`, each house a listener
            // (`0x004F5FFB`), forgets House+54E0 whatever the control. Phobos'
            // and Ares' `AnnounceInvalidPointer` hooks at this dispatcher's
            // entry (`0x7258D0`, resuming there) send a removed building to
            // every house's `HouseExt::OnDetach` / `InvalidatePointer` (Ares
            // `100052B0`), which clear the AI factory slots naming it; no
            // other listener reads them.
            let slot_detach = (context.rules.phobos() || context.rules.ares())
                && control == PointerExpiryControl::Uninit;
            for house in self.houses.values_mut() {
                named |= house.forget_lost_building(expired_id);
                if slot_detach {
                    named |= house
                        .ai_production_mut()
                        .forget_parallel_factory(expired_id);
                }
            }
            debug_assert!(
                techno || !named,
                "a factory or a house named {expired_id}, which is no Techno"
            );
        }
        let Some((
            expired_target_cell,
            expired_object_alive,
            expired_health,
            expired_is_selling,
            expired_owner,
        )) = self
            .substrate
            .entities
            .get(expired_id)
            .map(|expired| {
                (
                    object_get_coords_cell(expired),
                    expired.lifecycle.object_alive,
                    expired.health.current(),
                    expired.mission.current().known()
                        == Some(crate::sim::mission::MissionType::Selling),
                    Some(expired.owner()),
                )
            })
            .or_else(|| {
                let [x, y, _] = self.spotlight_object_coord(expired_id)?;
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(x))
                    .ok()
                    .zip(u16::try_from(crate::util::lepton::lepton_to_cell(y)).ok());
                // ObjectCtor5F390B supplies255; the class has no Techno owner.
                Some((cell, self.spotlight_alive(expired_id)?, 255, false, None))
            })
            .or_else(|| {
                let piece = self.substrate.voxel_anims.get(expired_id)?;
                let location = piece.world_coord();
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(location.x))
                    .ok()
                    .zip(u16::try_from(crate::util::lepton::lepton_to_cell(location.y)).ok());
                // Voxel749A11 / inherited ObjectUnInit5F65F0 announce
                // this retained Object body; OwnerHouse is no Techno flag.
                Some((cell, piece.lifecycle().object_alive(), 255, false, None))
            })
            .or_else(|| {
                let system = self.substrate.particle_systems.get(expired_id)?;
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(system.coords().x))
                    .ok()
                    .zip(
                        u16::try_from(crate::util::lepton::lepton_to_cell(system.coords().y)).ok(),
                    );
                // ObjectCtor5F390B..5F3919 supplies Health=255. A System
                // has no Techno flags, selling mission or Techno owner;
                // its retained coordinates and liveness are still readable
                // during ObjectLimbo's synchronous expiry announcement.
                Some((cell, system.alive(), 255, false, None))
            })
            .or_else(|| {
                let (particle, alive) = self
                    .substrate
                    .particle_systems
                    .detached_particle(expired_id)?;
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(particle.coords().x))
                    .ok()
                    .zip(
                        u16::try_from(crate::util::lepton::lepton_to_cell(particle.coords().y))
                            .ok(),
                    );
                Some((cell, alive, 255, false, None))
            })
            .or_else(|| {
                let bullet = self.projectiles.get(expired_id)?;
                let point = bullet.position();
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(point.x))
                    .ok()
                    .zip(u16::try_from(crate::util::lepton::lepton_to_cell(point.y)).ok());
                Some((cell, true, bullet.payload().base_damage, false, None))
            })
            .or_else(|| {
                let tree = self.terrain_objects.objects().get(&expired_id)?;
                let coord = tree.world_coord();
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(coord.x))
                    .ok()
                    .zip(u16::try_from(crate::util::lepton::lepton_to_cell(coord.y)).ok());
                Some((cell, tree.object_alive(), tree.health(), false, None))
            })
            .or_else(|| {
                let smudge = self.smudge_grid.as_ref()?.object(expired_id)?;
                let location = smudge.location();
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(location.x))
                    .ok()
                    .zip(u16::try_from(crate::util::lepton::lepton_to_cell(location.y)).ok());
                Some((cell, smudge.object_alive(), smudge.health(), false, None))
            })
        else {
            return;
        };

        // BuildingLight436A00; Ares1004EBC0 preserves a cloaking parent's
        // link on control0, while the target clear remains unconditional.
        self.substrate.spotlights_pointer_expired(
            expired_id,
            control == PointerExpiryControl::Uninit,
            context.rules.ares(),
        );

        #[cfg(any(test, feature = "test-support"))]
        if let Some(smudge) = self
            .smudge_grid
            .as_ref()
            .and_then(|grid| grid.object(expired_id))
        {
            let location = smudge.location();
            self.trace_lifecycle_for_test(LifecycleTestEvent::SmudgeExpiryBoundary {
                stable_id: expired_id,
                native_id: smudge.native_unique_id(),
                native_cursor: self.native_unique_ids.as_ref().unwrap().current_raw(),
                object_alive: smudge.object_alive(),
                in_limbo: smudge.in_limbo(),
                health: smudge.health(),
                location: [location.x, location.y, location.z],
                pending: self.substrate.pending_delete.object_ids(),
                generic_objects: self
                    .removal_listener_order(self.substrate.entities.keys_sorted())
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect(),
                rng: [
                    self.scenario_rng.native_state_hex(),
                    self.main_rng.native_state_hex(),
                    self.mapgen_rng.native_state_hex(),
                ],
            });
        }

        // `BulletClass::PointerExpired @ 0x004684E0` performs the packed
        // `MapClass::Get_CellClass @ 0x005657A0` lookup only for a matching
        // target. Its result pointer is stored at Bullet+0x10C: an allocated
        // slot therefore remains a stable Cell target, while a miss stores the
        // one shared dummy at `0x00ABDC50`. Later `BulletClass::AI @ 0x004666E0`
        // dispatches that live pointer and observes its most recent coord stamp.

        // Most listeners hold nothing this expiry changes, and the game
        // passes over them (below). The ones that do are found up front in
        // the expiry index. Production builds list only those. Test builds
        // list every listener, each with whether the index found it, and
        // check at each one's turn that a listener it left out changes
        // nothing.
        let (index, stores) = self.expiry_listener_index();
        let acting = index.reached_by(stores, expired_id);
        if crate::sim::touch_log::live_read_check_enabled() {
            let (_, stores) = self.expiry_listener_index();
            assert_eq!(
                acting,
                ExpiryListeners::asked_of_every(&stores, expired_id),
                "the expiry listeners of {expired_id} diverged from a live read"
            );
        }
        // Original4685CC gates the Bullet callback's scalable-list arm on
        // control!=0, expired RTTI8 and its current retained type. Read once,
        // before any listener runs: RuleSet/type remain immutable through this
        // broadcast, the expiring Bullet stays stored until dtor466581 (its
        // own registered subscriber is one of the Bullets), and Unit741439
        // only enrolls a fresh returned Bullet, never this old expired
        // identity. With the arm on and the Bullet enrolled, every present
        // Bullet subscriber removes one enrolled duplicate, so production
        // builds then visit every Bullet, not only those the index finds.
        // Then retain the proven lack of further matches.
        let mut scalable_expiry_pending = control == PointerExpiryControl::Uninit
            && self
                .projectiles
                .get(expired_id)
                .and_then(|bullet| bullet.payload().projectile_type())
                .and_then(|type_id| self.interner.projectile_type(type_id, context.rules()))
                .is_some_and(|kind| kind.scalable)
            && self.projectiles.scalable_enrolled(expired_id);
        #[cfg(any(test, feature = "test-support"))]
        let listeners: Vec<(u64, ObjectKind, bool)> = {
            let found: std::collections::BTreeSet<u64> = acting.iter().map(|&(id, _)| id).collect();
            self.removal_listener_order(self.substrate.entities.keys_sorted())
                .into_iter()
                .map(|(id, kind)| (id, kind, found.contains(&id)))
                .collect()
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let listeners: Vec<(u64, ObjectKind, bool)> = {
            let mut listeners: Vec<(u64, ObjectKind, bool)> = acting
                .into_iter()
                .map(|(id, kind)| (id, kind, true))
                .collect();
            if scalable_expiry_pending {
                listeners.extend(
                    self.projectiles
                        .iter()
                        .map(|(&id, _)| (id, ObjectKind::Projectile, false)),
                );
                // Stable: a Bullet the index found keeps its first entry.
                listeners.sort_by_key(|&(id, ..)| id);
                listeners.dedup_by_key(|&mut (id, ..)| id);
            }
            listeners
        };
        for (listener_id, kind, found) in listeners {
            // A callback may have removed a later listener; an ID never
            // moves to another store. An entity listener is read once, for
            // its presence and for whether it holds anything this expiry
            // changes.
            let mut entity_acts = false;
            let present = match kind {
                ObjectKind::Entity => {
                    self.substrate
                        .entities
                        .get(listener_id)
                        .is_some_and(|listener| {
                            entity_acts = Self::entity_expiry_listener_acts(
                                listener,
                                listener_id,
                                expired_id,
                            );
                            true
                        })
                }
                ObjectKind::Anim => self.substrate.anims.contains_key(listener_id),
                ObjectKind::ParticleSystem => {
                    self.substrate.particle_systems.contains_key(listener_id)
                }
                ObjectKind::Projectile => self.projectiles.get(listener_id).is_some(),
                ObjectKind::Wave => self.waves.get(listener_id).is_some(),
                ObjectKind::Terrain => self.terrain_objects.get(listener_id).is_some(),
                ObjectKind::Smudge => self
                    .smudge_grid
                    .as_ref()
                    .and_then(|grid| grid.object(listener_id))
                    .is_some(),
                // Not on the roster ([`ObjectKind`]'s pointer-expiry column).
                ObjectKind::VoxelAnim | ObjectKind::Particle | ObjectKind::Spotlight => false,
            };
            if !present {
                continue;
            }
            #[cfg(any(test, feature = "test-support"))]
            assert!(
                !entity_acts || found,
                "listener {listener_id} came to hold {expired_id} during its expiry broadcast; \
                 production builds leave it out"
            );

            #[cfg(any(test, feature = "test-support"))]
            if control == PointerExpiryControl::Uninit {
                let (target_alive, target_in_limbo) = self
                    .substrate
                    .entities
                    .get(expired_id)
                    .map(|target| (target.lifecycle.object_alive, target.lifecycle.in_limbo))
                    .or_else(|| {
                        self.substrate
                            .particle_systems
                            .get(expired_id)
                            .map(|system| (system.alive(), system.in_limbo()))
                    })
                    .or_else(|| {
                        self.substrate
                            .particle_systems
                            .detached_particle(expired_id)
                            .map(|(_, alive)| (alive, !alive))
                    })
                    .or_else(|| {
                        self.terrain_objects
                            .objects()
                            .get(&expired_id)
                            .map(|tree| (tree.object_alive(), !tree.is_live()))
                    })
                    .or_else(|| {
                        self.smudge_grid
                            .as_ref()?
                            .object(expired_id)
                            .map(|object| (object.object_alive(), object.in_limbo()))
                    })
                    .or_else(|| {
                        self.projectiles
                            .get(expired_id)
                            .map(|bullet| (true, !bullet.in_logic_vector()))
                    })
                    .or_else(|| {
                        self.spotlight_alive(expired_id)
                            .map(|alive| (alive, !alive))
                    })
                    .unwrap_or((false, true));
                self.trace_lifecycle_for_test(LifecycleTestEvent::UninitRemovalListenerVisited {
                    expired_id,
                    listener_id,
                    target_alive,
                    target_in_limbo,
                });
            }

            if kind == ObjectKind::Entity {
                // Most entity listeners hold nothing this expiry changes, and
                // the game passes over them. Test builds visit them as well,
                // and check below that their forwards handed out no entity and
                // drew no random number.
                if !entity_acts && !cfg!(any(test, feature = "test-support")) {
                    continue;
                }
                #[cfg(any(test, feature = "test-support"))]
                let passed_over = (!entity_acts)
                    .then(|| (self.substrate.entities.hand_outs(), self.rng_cursors()));
                self.notify_entity_pointer_expired(
                    listener_id,
                    expired_id,
                    expired_target_cell,
                    expired_object_alive,
                    expired_health,
                    expired_is_selling,
                    expired_owner,
                    control,
                    context.rules(),
                    context.effects(),
                );
                // `TechnoClass::PointerExpired` forwards to the listener's
                // SpawnManager: `0x00707B24 CALL 0x006B7C60`, gated only on
                // `+0x2D0 != 0`. This is the only mechanism that drops a
                // destroyed wing target, so without it a Carrier keeps sending
                // its Hornets at a corpse. The forward sits OUTSIDE the control
                // test, so it runs on a cloak dive as well as on UnInit. Ares
                // 3.0 TechnoClass_PointerGotInvalid_SpawnCloakOwner (1004A910,
                // hook 0x707B19) skips it for the manager's own owner unless
                // it is removed, so a cloaking carrier keeps its spawns.
                if !(context.rules.ares()
                    && control == PointerExpiryControl::DetachAll
                    && expired_id == listener_id)
                {
                    crate::sim::spawn_manager::notify_pointer_expired(
                        self,
                        listener_id,
                        expired_id,
                        context.rules(),
                        context.effects(),
                    );
                }
                // The CaptureManager forward — `0x00707B14 CALL 0x00471F90` —
                // sits inside the `if (control != 0)` block opened at
                // `0x00707AE7`, so a dive leaves mind-control links alone while
                // a death drops them. It changes nothing unless a node names
                // the expiring object, so only then is the listener handed out.
                if control == PointerExpiryControl::Uninit
                    && self
                        .substrate
                        .entities
                        .get(listener_id)
                        .and_then(|entity| entity.capture_manager.as_ref())
                        .is_some_and(|manager| manager.victims().any(|victim| victim == expired_id))
                    && let Some(manager) = self
                        .substrate
                        .entities
                        .get_mut(listener_id)
                        .and_then(|entity| entity.capture_manager.as_mut())
                {
                    manager.pointer_expired(expired_id);
                }
                // The TemporalClass forward (`0x00707B34` -> `0x0071AB60`)
                // follows, outside the control test like the SpawnManager's.
                self.temporal_pointer_expired(
                    listener_id,
                    expired_id,
                    context.rules(),
                    context.effects(),
                );
                self.airstrike_pointer_expired(
                    listener_id,
                    expired_id,
                    context.rules(),
                    context.effects(),
                );
                // Then the Magnetron links (`0x00707B64..0x00707BAC`), also
                // outside the control test.
                self.locomotor_link_pointer_expired(
                    listener_id,
                    expired_id,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
                // Techno707BCE..707CA4 first-removes one candidate/paired
                // score and one history occurrence, outside control/allowClear.
                // This must precede the subclass Foot parasite forward.
                if let Some(entity) = self.substrate.entities.get_mut_if(listener_id, |entity| {
                    entity
                        .distributed_fire_references()
                        .any(|id| id == expired_id)
                }) {
                    entity.distributed_fire_pointer_expired(expired_id);
                }
                // FootClass::PointerExpired 0x004D998C..0x004D99CD follows the
                // Techno body: the parasite link and its forward.
                self.foot_parasite_pointer_expired(
                    listener_id,
                    expired_id,
                    control == PointerExpiryControl::Uninit,
                    context.rules(),
                    context.effects(),
                );
                // UnitClass::PointerExpired7446E0 runs the full Foot forward
                // first, then clears matching +6C8 at7446FF for either control
                // value. It leaves the independent +6D0 byte alone (gamemd).
                if let Some(entity) = self.substrate.entities.get_mut_if(listener_id, |entity| {
                    entity.category() == EntityCategory::Unit
                        && entity.unit_follower() == Some(expired_id)
                }) {
                    entity.set_unit_follower(None);
                }
                #[cfg(any(test, feature = "test-support"))]
                if let Some(before) = passed_over {
                    assert_eq!(
                        before,
                        (self.substrate.entities.hand_outs(), self.rng_cursors()),
                        "the expiry of {expired_id} changed listener {listener_id}, which \
                         entity_expiry_listener_acts passes over"
                    );
                }
            } else if kind == ObjectKind::Anim {
                let changed = self.anim_pointer_expired(listener_id, expired_id, context.rules());
                debug_assert!(
                    found || !changed,
                    "the expiry of {expired_id} changed anim {listener_id}, which the expiry \
                     index passes over"
                );
            } else if kind == ObjectKind::ParticleSystem {
                let changed = self
                    .substrate
                    .particle_systems
                    .pointer_expired(listener_id, expired_id);
                debug_assert!(
                    found || !changed,
                    "the expiry of {expired_id} changed particle system {listener_id}, which the \
                     expiry index passes over"
                );
            } else if kind == ObjectKind::Projectile {
                let target_matches = self
                    .projectiles
                    .get(listener_id)
                    .is_some_and(|projectile| projectile.target().object_id() == Some(expired_id));
                let projectile_replacement_target = if target_matches {
                    self.projectile_lost_target(expired_id, context.reborrow())
                } else {
                    ProjectileTarget::None
                };
                let (changed, remaining) = self
                    .projectiles
                    .pointer_expired(
                        listener_id,
                        expired_id,
                        projectile_replacement_target,
                        scalable_expiry_pending,
                    )
                    .expect("present Bullet subscriber");
                scalable_expiry_pending = remaining;
                debug_assert!(
                    found || !changed,
                    "the expiry of {expired_id} changed bullet {listener_id}, which the expiry \
                     index passes over"
                );
                #[cfg(any(test, feature = "test-support"))]
                if let Some((source_id, target)) = self
                    .projectiles
                    .get(listener_id)
                    .map(|projectile| (projectile.source_id(), projectile.target()))
                {
                    self.trace_lifecycle_for_test(
                        LifecycleTestEvent::ProjectilePointerExpiredVisited {
                            expired_id,
                            projectile_id: listener_id,
                            expired_resolvable: self.classify_object(expired_id).is_some(),
                            projectile_resolvable: true,
                            source_id,
                            target,
                        },
                    );
                }
            } else if kind == ObjectKind::Terrain {
                // Both Terrain constructors call ObjectCtor5F3900, whose
                // 5F3A76..8B registers this listener and5F3B34 sets flag2;
                // Dispatch725938..95F reaches TerrainPointerExpired71CFD0.
                let changed = self.terrain_objects.pointer_expired(
                    listener_id,
                    expired_id,
                    control == PointerExpiryControl::Uninit,
                );
                debug_assert!(
                    found || !changed,
                    "Terrain expiry index missed listener {listener_id}"
                );
            } else if kind == ObjectKind::Wave {
                let (owner_cleared, target_cleared) = self
                    .waves
                    .pointer_expired(listener_id, expired_id)
                    .expect("Wave listener disappeared during expiry callback");
                debug_assert!(
                    found || !(owner_cleared || target_cleared),
                    "the expiry of {expired_id} changed wave {listener_id}, which the expiry \
                     index passes over"
                );
                if owner_cleared && self.active_wave_links.get(&expired_id) == Some(&listener_id) {
                    // TechnoClass keeps the Wave link through the dying/deferred
                    // interval. Once the exact owner pointer expires, retaining
                    // this Rust projection would serialize a link whose Wave
                    // owner is now null.
                    self.active_wave_links.remove(&expired_id);
                }
            } else {
                // The one roster kind with no callback: a smudge's
                // PointerExpired clears nothing it holds.
                debug_assert_eq!(
                    kind,
                    ObjectKind::Smudge,
                    "{listener_id} has no expiry callback"
                );
            }
        }
        // The kamikaze tracker's Remove (`0x00725972`) follows the listeners
        // and the BombList; a SpawnManager's slot guard reads the membership
        // before it.
        self.kamikaze.remove(expired_id);
        //725977..725993: DiskLaser4A7900 sees every expired pointer after
        // Kamikaze and before the Techno navigation handoff. It queues a shot
        // without clearing its retained pointers or deleting copied beams.
        self.detach_disk_lasers(expired_id);
        // Dispatch7258D0's Techno-bit gate reaches733160 at7259B1 after
        // Kamikaze Remove. Executed controls: input_oracle/selection_navigation.
        // All typed GameEntity classes carry Abstract+14's Techno bit. Smudge
        // does not, so it must not produce this handoff. It precedes native
        // Tactical/Logic expiry and eventual DisplayRemove/Alive writes.
        if expired_owner.is_some() {
            self.lifecycle_outputs
                .emit(LifecycleOutput::ObjectPointerExpired {
                    stable_id: expired_id,
                });
        }
    }
}
