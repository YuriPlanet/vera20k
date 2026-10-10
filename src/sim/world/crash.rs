//! A crashing object: `FootClass::Crash`, the Fly and Jumpjet impacts, and the
//! crash-only work of the object's own AI (the crash voice/sound edge and the
//! red-health smoke).
//!
//! A shot-down aircraft is not removed at the killing hit. After the Techno
//! death arm, `AircraftClass::ReceiveDamage @ 0x004165C0` announces the loss,
//! builds its `Explosion=` anim and calls Crash (vtable `+0x3DC`,
//! `0x00416694`); only a refused Crash (the object is on the ground) UnInits
//! it there (`0x004166A3`). A crashing object stays alive with Health 0 and
//! the latch `FootClass+0x425`, spins by three Scenario draws, and falls under
//! `FlyLocomotionClass::Process` until the frame its height reaches zero,
//! where it fires its death weapon, plays the impact sound and UnInits.
//!
//! A `Crashable=` Unit crashes the same way from `UnitClass::ReceiveDamage @
//! 0x00737C90` (`0x00738457..0x00738475`), after its `Death_Explosion`, its
//! passengers and its crew; a Jumpjet falls under the locomotor's State 5
//! (`movement::jumpjet_movement::jumpjet_flight`) and its impact notice finishes it
//! ([`Simulation::jumpjet_crash_impact`]). A `Crashable=` `JumpJet=`
//! infantryman crashes from `InfantryClass::ReceiveDamage` (`0x005185F1`,
//! `world::infantry_terminal`) and falls and lands in its AirDeath actions
//! (`movement::infantry_action`).
//!
//! While it falls a wreck keeps the AI its native IsAlive (`+0x90`) gates: the
//! Techno body with its passive scan, and a Unit's fire update, so a guarding
//! wreck can pick up a target and shoot on the way down. Its Health of 0 stops
//! only the mission handlers (`MissionClass::AI @ 0x005B30A7`) and its self-heal
//! (`world::techno_ai`).
//!
//! Native evidence: `tools/spatial_oracle/aircraft_crash.{py,json}` runs Crash,
//! the per-frame fall to the impact, the crashing RockingUpdate and the smoke
//! block on the original executable; `jumpjet_crash.{py,json}` runs the
//! Jumpjet kill and fall to its notice.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::RockingState;
use crate::sim::movement::air_movement::current_fly_height;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{NativeF64Bits, X87Chop53};

use super::{Simulation, UninitContext};

/// `0x007E9280`, the spread of the sideways spin rate.
const SIDEWAYS_SPREAD: NativeF64Bits = NativeF64Bits::from_bits(0x3fc3_3333_3333_3333);
/// `0x007E3860`, 0.1: the sideways rate's floor and the forwards rate's spread.
const SPIN_TENTH: NativeF64Bits = NativeF64Bits::from_bits(0x3fb9_9999_9999_999a);

/// The spin rates Crash stores as f32 (`fstp dword`, chop): `r / 0x7FFFFFFE`
/// (`0x007E3570`) times the spread, plus the floor.
fn spin_rate(draw: i32, spread: NativeF64Bits, floor: Option<NativeF64Bits>) -> SimFixed {
    let unit = X87Chop53::mul(
        X87Chop53::load_i32(draw),
        X87Chop53::load_f64(crate::sim::rng::RANDOM_RANGED_UNIT_SCALE)
            .expect("the RandomRanged unit scale is a finite constant"),
    );
    let mut value = X87Chop53::mul(
        unit,
        X87Chop53::load_f64(spread).expect("the spin spread is a finite constant"),
    );
    if let Some(floor) = floor {
        value = X87Chop53::add(
            value,
            X87Chop53::load_f64(floor).expect("the spin floor is a finite constant"),
        );
    }
    let stored = X87Chop53::store_f32(value).expect("a spin rate is a finite f32");
    SimFixed::from_num(f32::from_bits(stored.bits()))
}

impl Simulation {
    /// `FootClass::Crash @ 0x004DEBB0`, the shared vtable `+0x3DC` body of
    /// Unit, Infantry and Aircraft. Returns false, and changes nothing, for an
    /// object on the ground (`GetHeight <= 0`, `0x004DEBB6`): its caller then
    /// UnInits it.
    ///
    /// A living object first runs the live prefix (`0x004DEBC4..0x004DEC72`):
    /// its Tag's events, `RecordKill(attacker)` (vtable `+0xE0`) and Health 0.
    /// Then, for every crash: the latch (`+0x425`), OVER_OUT to contact 0
    /// (vtable `+0x274(3)`), the Stun (vtable `+0x3A0`), KillPassengers
    /// (`0x00707CB0`), the three Scenario spin draws unless the object is
    /// Infantry or `Unsorted::IKnowWhatImDoing` (`[0x00A8E7AC]`) is raised,
    /// and `Detach_All(false)`'s announcement (`0x007258D0`, `dl = 0`).
    ///
    /// RESIDUAL: the live prefix raises the object's Tag events 0x26 and 0x29
    /// (`TagClass @ 0x006E53A0`); VERA attaches no Tags to objects. Trigger: a
    /// tagged campaign aircraft crashes while alive (AirportBound with no
    /// airfield left). Effect: the map's trigger never hears it. Frequency:
    /// campaign maps only. The shared A8E7AC caller scope also covers native
    /// runtime placement/destruction; a raised scope suppresses spin draws.
    pub(crate) fn foot_crash(
        &mut self,
        id: u64,
        attacker: Option<u64>,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if current_fly_height(entity, self.resolved_terrain.as_ref()) <= 0 {
            return false;
        }
        let category = entity.category;
        if entity.health.current > 0 {
            // `0x004DEC4C`: RecordKill(attacker) books the kill and the loss
            // for the killer's house (no attacker: the loss alone), then
            // `0x004DEC72` zeroes Health (Foot+6AD, a Magnetron lift, would
            // keep it; VERA never sets it).
            let killer = attacker.and_then(|a| self.substrate.entities.get(a).map(|k| k.owner()));
            self.record_the_kill(
                id,
                attacker,
                killer,
                crate::sim::combat::KillCallback::Terminal,
                rules,
            );
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.health.current = 0;
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.crashing = true;
        }
        crate::sim::radio::transmit_to_contact(
            self,
            id,
            crate::sim::radio::RadioMessage::Break,
            Some(rules),
        );
        self.techno_death_stun(id, UninitContext::new(Some(rules), registry));
        self.kill_passengers(id, attacker, rules, registry);
        if category != EntityCategory::Infantry && !self.object_placement_scope_active() {
            let sideways = self.scenario_rng.next_range_i32_inclusive(0, 0x7FFF_FFFE);
            let sign = self.scenario_rng.next_range_i32_inclusive(0, 1);
            let forwards = self.scenario_rng.next_range_i32_inclusive(0, 0x7FFF_FFFE);
            let mut vel_sideways = spin_rate(sideways, SIDEWAYS_SPREAD, Some(SPIN_TENTH));
            if sign == 0 {
                vel_sideways = -vel_sideways;
            }
            let vel_forwards = spin_rate(forwards, SPIN_TENTH, None);
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                let rocking = entity.rocking.get_or_insert_with(RockingState::default);
                rocking.vel_sideways = vel_sideways;
                rocking.vel_forwards = vel_forwards;
            }
        }
        self.detach_all_pointer_expired(id, rules, registry);
        true
    }

    /// The fall block (`0x004CD67F..0x004CD7A4`) of `0x004CD600`, which
    /// `FlyLocomotionClass::Process` (`0x004CCB40`) calls every frame, for a
    /// dead Fly: above the ground its fall counter grows by one
    /// (`advance_fall`) and it drops by the new counter, XY unchanged, when its
    /// cell passes `MapClass::In_Bounds` (`0x004CD748`); otherwise only the
    /// counter moves. Returns whether its height then reached zero, the
    /// impact (`0x004CD7A4`); the rest of the function (the paid step and the
    /// height step) runs only when it did not.
    ///
    /// The drop leaves the display, runs Mark(UP), `FootClass::SetLocation`
    /// (vt+0x1B4 = `0x004DB810`), Mark(DOWN) and re-enters the display
    /// (`0x004CD751..0x004CD792`). This comes before the movement's own Mark
    /// pair (`0x004CDA36`). The Mark(DOWN) marks the wreck whatever its state
    /// before, and on the impact frame its height of 0 or less makes its
    /// layer Ground (`0x004CFCF0`), so the wreck is in its cell's list for
    /// the impact.
    ///
    /// RESIDUAL: the same block drops an unpowered living Fly by three a frame
    /// and its impact (`0x004CD8A9..0x004CD9C0`) kills it with the C4 warhead,
    /// an explosion anim and area damage. Nothing in VERA powers an aircraft
    /// off (native: `Power_Off`, reached by EMP), so the arm is dormant.
    ///
    /// A falling wreck is still alive for its class AI, so one that drifts
    /// past the playfield or `In_Bounds` meets `AircraftClass::AI`'s removal
    /// after this Process (`aircraft::leave_map`, `0x00414F47..0x00414FD1`):
    /// the map-leave predicate decides, as natively, and a shot-down combat
    /// aircraft (its `+0x3D4` latch clear) keeps falling.
    pub(super) fn fly_crash_fall(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let terrain = self.resolved_terrain.as_ref();
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.health.current != 0 || current_fly_height(entity, terrain) == 0 {
            return false;
        }
        let location = crate::sim::movement::ground_pose::object_location(entity, terrain);
        // `0x004CD70C..0x004CD736`: the cell of the unchanged XY, divided
        // toward zero.
        let cell = ((location.x / 256) as i16, (location.y / 256) as i16);
        let placed = self.map_cell_in_bounds(cell);
        let Some(runtime) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.locomotor.as_mut())
            .and_then(|locomotor| locomotor.fly_runtime_mut())
        else {
            return false;
        };
        let counter = runtime.advance_fall(true);
        if placed {
            let context = UninitContext::new(rules, registry);
            self.substrate.display.remove(id);
            self.unmark_entity_remove(id, context);
            let dropped = crate::sim::components::DriveCoord {
                z: location.z.wrapping_sub(counter),
                ..location
            };
            crate::sim::movement::ground_pose::foot_set_location(
                &mut self.substrate.entities,
                id,
                dropped,
                rules,
                &self.interner,
            );
            self.mark_entity_put(id, context);
            self.submit_entity_display(id, rules, None);
        }
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        let height = current_fly_height(entity, self.resolved_terrain.as_ref());
        crate::sim::movement::ground_pose::mirror_height(entity, height);
        height <= 0
    }

    /// The dead Fly's impact, `0x004CD7AA..0x004CD8A8` in the same function:
    /// out of the AircraftTracker (`0x004CD7B3`), `SetHeight(0)`
    /// (`0x004CD7BF`), `Fire_Death_Weapon(0)` (`0x004CD809`) with no
    /// `Explodes=` gate, the impact cue by the impact cell's LandType
    /// (`0x004CD818..0x004CD891`), then UnInit (`0x004CD89B`). The object's
    /// own AI returns right after (`FootClass::AI 0x004DA87E`).
    /// Returns whether the death weapon changed a bridge.
    pub(crate) fn fly_crash_impact(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        if !self.substrate.entities.contains(id) {
            return false;
        }
        self.aircraft_tracker_remove(id);
        self.set_object_height(id, 0, Some(rules), overlay_registry);
        let bridge_state_changed = self.fire_death_weapon(id, rules, overlay_registry);
        self.play_crash_impact_sound(id, rules);
        self.uninit_with_context(id, UninitContext::new(Some(rules), overlay_registry));
        bridge_state_changed
    }

    /// A crashed Jumpjet's impact: State 5's owner work (`AircraftTracker::
    /// Remove @ 0x004135D0`, `0x0054D075`), then its `(0x117C, 0)` notice to
    /// `UnitClass`'s `INoticeSink` (`0x00746100`). A `Crashable=` type
    /// (`+0xD95`, `0x0074619D`) answers it (`0x007461A7..0x00746206`):
    /// `SetHeight(0)` (vtable `+0x1CC`), then `Fire_Death_Weapon(0)` for a
    /// `BalloonHover=` type (`0x007461EF`, the Kirov's bomb) or
    /// `UnitClass::Death_Explosion` once more for any other (`0x007461D1`),
    /// and UnInit (vtable `+0xF8`). The impact plays no sound; the crash
    /// sound the wreck holds plays out as it goes (`FootClass::~FootClass`,
    /// `0x004D3677`).
    ///
    /// An Infantry owner's notice (the Rocketeer's) keeps the infantryman
    /// instead: [`Simulation::infantry_crash_impact`]. Returns whether the
    /// death weapon changed a bridge.
    pub(crate) fn jumpjet_crash_impact(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        debug_assert_eq!(entity.category, EntityCategory::Unit);
        let (crashable, balloon_hover) = self
            .object_type(entity.type_ref(), rules)
            .map_or((false, false), |object| {
                (object.crashable, object.balloon_hover)
            });
        self.aircraft_tracker_remove(id);
        if !crashable {
            // The notice goes unanswered: the wreck rests in State 6.
            return false;
        }
        self.set_object_height(id, 0, Some(rules), overlay_registry);
        let bridge_state_changed = if balloon_hover {
            self.fire_death_weapon(id, rules, overlay_registry)
        } else {
            self.unit_death_explosion(rules, id, &mut Vec::new());
            false
        };
        self.uninit_with_context(id, UninitContext::new(Some(rules), overlay_registry));
        bridge_state_changed
    }

    /// `TechnoClass::Fire_Death_Weapon @ 0x0070D690` outside a damage
    /// transaction, its detonation committed on its own. Returns whether it
    /// changed a bridge.
    fn fire_death_weapon(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        crate::sim::combat::death_weapon_detonation(self, id, rules).is_some_and(|detonation| {
            self.commit_logic_projectile_detonations(rules, overlay_registry, &[detonation])
        })
    }

    /// `0x004CD818..0x004CD891`: over Water (LandType 2) the type's
    /// `ImpactWaterSound=` else `[AudioVisual] ImpactWaterSound=`, elsewhere
    /// `ImpactLandSound=` likewise, played at the object's Location with no
    /// sound handle.
    fn play_crash_impact_sound(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let water = self
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(entity.position.rx, entity.position.ry))
            .is_some_and(|cell| {
                cell.yr_cell_land_type == crate::rules::terrain_rules::LandType::Water.as_index()
            });
        let object = self.object_type(entity.type_ref(), rules);
        let sound = if water {
            object
                .and_then(|object| object.impact_water_sound.clone())
                .or_else(|| rules.general.impact_water_sound.clone())
        } else {
            object
                .and_then(|object| object.impact_land_sound.clone())
                .or_else(|| rules.general.impact_land_sound.clone())
        };
        if let Some(sound_id) = sound {
            let event = voc_at(entity, sound_id, None);
            self.sound_events.push(event);
        }
    }

    /// `FootClass::AI`'s crash edge (`0x004DACDD..0x004DADC2`): when the latch
    /// has risen since the last visit, the object's sound handle is released
    /// (`0x004DAD01`), `VoiceCrashing=` plays for a human player's object
    /// (`0x004DAD1F CALL 0x0050B6F0`) and `CrashingSound=` plays on the
    /// handle, following the object. The previous value is then kept.
    ///
    /// Nothing on the Fly path lowers the latch; besides the constructor only
    /// the Jumpjet Descend touchdown clears it (`0x0054CA12`, a Magnetron drop
    /// that lands).
    pub(crate) fn crash_edge_sounds(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.crashing == entity.crashing_seen {
            return;
        }
        if entity.crashing {
            // Foot4DAD01 releases the shared handle even if its MoveSound
            // latch was already cleared by the preceding tail.
            self.sound_events
                .push(super::SimSoundEvent::ObjectSoundReleased { owner: id });
            let entity = self.substrate.entities.get(id).expect("checked above");
            let owner = entity.owner();
            let object = self.object_type(entity.type_ref(), rules);
            let voice = object.and_then(|object| object.voice_crashing.clone());
            let crashing_sound = object.and_then(|object| object.crashing_sound.clone());
            let human = self.houses.get(&owner).is_some_and(|house| house.is_human);
            if let Some(voice) = voice
                && human
            {
                let event = voc_at(entity, voice, Some([owner, owner]));
                self.sound_events.push(event);
            }
            if let Some(sound) = crashing_sound {
                let world = Self::movement_sound_world(entity);
                self.sound_events
                    .push(super::SimSoundEvent::AnimationStarted {
                        anim_id: id,
                        sound_id: sound,
                        world,
                    });
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            // Falling crash edge4DADA9..4DADB7 leaves an active MoveSound's
            // shared handle alone; otherwise it releases the crash sound.
            if !entity.crashing && !entity.move_sound.is_active() {
                self.sound_events
                    .push(super::SimSoundEvent::ObjectSoundReleased { owner: id });
            }
            entity.crashing_seen = entity.crashing;
        }
    }

    /// `AircraftClass::AI`'s smoke (`0x00415085..0x0041512C`), after the
    /// ready/commence and ammo steps: an airborne aircraft below
    /// `ConditionRed=` (strictly, or with no Strength) takes one Scenario
    /// `RandomRanged(0, 99)` and, under 10 while alive or 80 when dead,
    /// builds `SGRYSMK1` at its Location (`AnimClass(type, coord, 0, 1,
    /// 0x600, 0, 0)`).
    pub(crate) fn aircraft_crash_smoke(&mut self, id: u64, rules: &RuleSet) {
        use crate::util::native_x87::MaskedX87Ordering::{Less, Unordered};
        let terrain = self.resolved_terrain.as_ref();
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        // `0x00414DAA`: an aircraft its own FootClass::AI removed never gets here.
        if entity.category != EntityCategory::Aircraft || !entity.lifecycle.object_alive {
            return;
        }
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let red = matches!(
            entity
                .health
                .compare_ratio(object.strength, rules.general.condition_red),
            Less | Unordered
        );
        if !red || current_fly_height(entity, terrain) <= 0 {
            return;
        }
        let threshold = if entity.health.current != 0 { 10 } else { 80 };
        let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        if self.scenario_rng.next_range_i32_inclusive(0, 99) >= threshold {
            return;
        }
        // `0x004150EF..0x00415102`: `AnimTypeClass::Find` of the literal.
        let type_id = self
            .interner
            .intern(crate::rules::effect_asset_catalog::AIRCRAFT_SMOKE_ANIM);
        self.admit_death_anim(
            rules,
            type_id,
            crate::sim::combat::destruction_effects::DeathAnimSpawn::at(
                crate::sim::anim_class::AnimWorldCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                },
                0,
            ),
        );
    }

    /// `FootClass::KillPassengers @ 0x00707CB0`: until the cargo is empty, the
    /// head passenger leaves its Team, is removed from the cargo, has its own
    /// passengers killed (credited to itself, `0x00707CF5`), records its kill
    /// by `attacker` (vtable `+0xE0`) and is UnInit.
    pub(crate) fn kill_passengers(
        &mut self,
        transport: u64,
        attacker: Option<u64>,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        loop {
            let Some(passenger) = self
                .substrate
                .entities
                .get_mut(transport)
                .and_then(|entity| entity.passenger_role.cargo_mut())
                .and_then(|cargo| cargo.unload_first())
                .map(|(passenger, _size)| passenger)
            else {
                return;
            };
            // `0x00707CE0`: `TeamClass::Remove_Member` (in limbo, so no idle
            // order).
            self.leave_team(passenger, false, Some(rules));
            if let Some(entity) = self.substrate.entities.get_mut(passenger)
                && matches!(
                    entity.passenger_role,
                    crate::sim::passenger::PassengerRole::Inside { transport_id, .. }
                        if transport_id == transport
                )
            {
                entity.passenger_role = crate::sim::passenger::PassengerRole::None;
            }
            self.kill_passengers(passenger, Some(passenger), rules, registry);
            self.record_kill_and_uninit(passenger, attacker, rules, registry);
        }
    }

    /// A passenger dying with its transport: `RecordKill(attacker)` (vtable
    /// `+0xE0`) then UnInit (vtable `+0xF8`), in KillPassengers
    /// (`0x00707CFF`/`0x00707D09`) and in a dying unit's refused escape
    /// (`UnitClass::ReceiveDamage 0x00738188..0x0073819B`).
    pub(crate) fn record_kill_and_uninit(
        &mut self,
        victim: u64,
        attacker: Option<u64>,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let killer = attacker.and_then(|a| self.substrate.entities.get(a).map(|k| k.owner()));
        if let Some(entity) = self.substrate.entities.get_mut(victim) {
            entity.health.current = 0;
        }
        self.record_the_kill(
            victim,
            attacker,
            killer,
            crate::sim::combat::KillCallback::Terminal,
            rules,
        );
        self.uninit_with_context(victim, UninitContext::new(Some(rules), registry));
    }
}

/// `VocClass::PlayAt @ 0x007509E0` of a rules-named sound at the object's
/// Location, with no handle.
fn voc_at(
    entity: &crate::sim::game_entity::GameEntity,
    sound_id: String,
    audible_to: Option<[crate::sim::intern::InternedId; 2]>,
) -> super::SimSoundEvent {
    let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    super::SimSoundEvent::VocAt {
        sound_id,
        audible_to,
        rx: entity.position.rx,
        ry: entity.position.ry,
        sub_x: entity.position.sub_x,
        sub_y: entity.position.sub_y,
        world_z_leptons: location.z,
    }
}
