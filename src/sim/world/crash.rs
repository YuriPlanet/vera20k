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
//! (`movement::jumpjet_flight`) and its impact notice finishes it
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
    /// campaign maps only. `IKnowWhatImDoing` is raised only around building
    /// placement scopes that never reach a crash, so VERA reads it as zero.
    pub(crate) fn foot_crash(&mut self, id: u64, attacker: Option<u64>, rules: &RuleSet) -> bool {
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
            if let Some(attacker) = attacker {
                crate::sim::combat::award_kill_experience(
                    &mut self.substrate.entities,
                    rules,
                    &self.interner,
                    &self.house_alliances,
                    attacker,
                    id,
                );
            }
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                crate::sim::combat::record_kill_credit(entity, killer, rules, &self.interner);
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
        self.techno_death_stun(id, UninitContext::with_rules(rules));
        self.kill_passengers(id, attacker, rules);
        if category != EntityCategory::Infantry {
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
        self.detach_all_pointer_expired(id, rules);
        true
    }

    /// `FlyLocomotionClass::Process`'s fall block (`0x004CD67F..0x004CD7A4`)
    /// for a dead Fly: above the ground its fall counter grows by one
    /// (`advance_fall`) and it drops by the new counter, XY unchanged, when its
    /// cell passes `MapClass::In_Bounds` (`0x004CD748`); otherwise only the
    /// counter moves. Returns whether its height then reached zero, the
    /// impact (`0x004CD7A4`); the rest of Process (the paid step and the
    /// height step) runs only when it did not.
    ///
    /// RESIDUAL: the same block drops an unpowered living Fly by three a frame
    /// and its impact (`0x004CD8A9..0x004CD9C0`) kills it with the C4 warhead,
    /// an explosion anim and area damage. Nothing in VERA powers an aircraft
    /// off (native: `Power_Off`, reached by EMP), so the arm is dormant.
    ///
    /// RESIDUAL: `AircraftClass::AI` UnInits an aircraft outside the playfield
    /// or `In_Bounds` when its map-leave predicate (vt+0x4DC, `0x0041B890`)
    /// answers true (`0x00414F47..0x00414FD1`); VERA ports neither. Trigger: a
    /// wreck of a plane that leaves the map (paradrop, spy, cargo: the
    /// predicate's `+0x3D4` latch) drifting out of bounds while it falls.
    /// Effect: it keeps drifting where native removes it; a shot-down combat
    /// aircraft (latch clear) drifts natively too. Frequency: rare.
    pub(super) fn fly_crash_fall(&mut self, id: u64) -> bool {
        let terrain = self.resolved_terrain.as_ref();
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.health.current != 0 || current_fly_height(entity, terrain) == 0 {
            return false;
        }
        let xy = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
        // `0x004CD70C..0x004CD736`: the cell of the unchanged XY, divided
        // toward zero.
        let cell = ((xy[0] / 256) as i16, (xy[1] / 256) as i16);
        let placed = self.map_cell_in_bounds(cell);
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        let Some(runtime) = entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.fly_runtime_mut())
        else {
            return false;
        };
        let counter = runtime.advance_fall(true);
        if placed && let Some(z) = entity.position.exact_z_leptons {
            entity.position.exact_z_leptons = Some(z.wrapping_sub(counter));
        }
        let height = current_fly_height(entity, self.resolved_terrain.as_ref());
        if let Some(locomotor) = entity.locomotor.as_mut() {
            locomotor.altitude = SimFixed::saturating_from_num(height);
        }
        height <= 0
    }

    /// The dead Fly's impact, `FlyLocomotionClass::Process 0x004CD7AA..
    /// 0x004CD8A8`: out of the AircraftTracker (`0x004CD7B3`), `SetHeight(0)`
    /// (`0x004CD7BF`), `Fire_Death_Weapon(0)` (`0x004CD809`) with no
    /// `Explodes=` gate, the impact cue by the impact cell's LandType
    /// (`0x004CD818..0x004CD891`), then UnInit (`0x004CD89B`). The object's
    /// own AI returns right after (`FootClass::AI 0x004DA87E`).
    pub(crate) fn fly_crash_impact(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        if !self.substrate.entities.contains(id) {
            return;
        }
        self.aircraft_tracker_remove(id);
        self.set_object_height(id, 0);
        self.fire_death_weapon(id, rules, overlay_registry);
        self.play_crash_impact_sound(id, rules);
        // `FootClass::~FootClass` releases the crash sound the object holds
        // (`0x004D3677`): a one-shot plays out.
        self.sound_events
            .push(super::SimSoundEvent::ObjectSoundReleased { owner: id });
        self.release_move_sound(id);
        self.uninit_with_rules(id, rules);
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
    /// instead: [`Simulation::infantry_crash_impact`].
    pub(crate) fn jumpjet_crash_impact(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
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
            return;
        }
        self.set_object_height(id, 0);
        if balloon_hover {
            self.fire_death_weapon(id, rules, overlay_registry);
        } else {
            self.unit_death_explosion_now(rules, id);
        }
        self.sound_events
            .push(super::SimSoundEvent::ObjectSoundReleased { owner: id });
        self.release_move_sound(id);
        self.uninit_with_rules(id, rules);
    }

    /// `TechnoClass::Fire_Death_Weapon @ 0x0070D690` with no extra damage: a
    /// real bullet of the chosen weapon (`CreateBullet @ 0x0046B050`, target
    /// and owner the object itself) detonates at the object's Location
    /// (`vt+0x48`, `0x0070D77C`; `DetonateAtCoord @ 0x004690B0`).
    fn fire_death_weapon(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let current_weapon = crate::sim::combat::combat_weapon::current_weapon(entity, object);
        let impact = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let Some((damage, warhead, weapon)) = crate::sim::combat::fire_death_weapon_payload(
            rules,
            object,
            current_weapon,
            &mut self.interner,
        ) else {
            return;
        };
        let detonation = crate::sim::projectile::ProjectileDetonation {
            projectile_id: id,
            source_id: id,
            target: crate::sim::projectile::ProjectileTarget::Entity(id),
            impact: crate::sim::projectile::ProjectileCoord {
                x: impact.x,
                y: impact.y,
                z: impact.z,
            },
            payload: crate::sim::projectile::ProjectilePayload {
                base_damage: damage,
                warhead,
                weapon,
            },
            reason: crate::sim::projectile::ProjectileDetonationReason::DeathWeapon,
        };
        self.commit_logic_projectile_detonations(rules, overlay_registry, &[detonation]);
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
            self.release_move_sound(id);
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
                let sound_id = self.interner.intern(&sound);
                self.sound_events
                    .push(super::SimSoundEvent::AnimationStarted {
                        anim_id: id,
                        sound_id,
                        world,
                    });
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
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
            crate::sim::combat::destruction_effects::DeathAnimSpawn {
                coord: crate::sim::anim_class::AnimWorldCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                },
                delay: 0,
                draws: None,
            },
        );
    }

    /// `FootClass::KillPassengers @ 0x00707CB0`: until the cargo is empty, the
    /// head passenger leaves its Team, is removed from the cargo, has its own
    /// passengers killed (credited to itself, `0x00707CF5`), records its kill
    /// by `attacker` (vtable `+0xE0`) and is UnInit.
    ///
    /// RESIDUAL: the Team removal (`TeamClass::Remove_Member @ 0x006EA870`) is
    /// not ported, as for every other VERA death (`object_destroy_callback`).
    pub(crate) fn kill_passengers(
        &mut self,
        transport: u64,
        attacker: Option<u64>,
        rules: &RuleSet,
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
            if let Some(entity) = self.substrate.entities.get_mut(passenger)
                && matches!(
                    entity.passenger_role,
                    crate::sim::passenger::PassengerRole::Inside { transport_id }
                        if transport_id == transport
                )
            {
                entity.passenger_role = crate::sim::passenger::PassengerRole::None;
            }
            self.kill_passengers(passenger, Some(passenger), rules);
            self.record_kill_and_uninit(passenger, attacker, rules);
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
    ) {
        let killer = attacker.and_then(|a| self.substrate.entities.get(a).map(|k| k.owner()));
        if let Some(attacker) = attacker {
            crate::sim::combat::award_kill_experience(
                &mut self.substrate.entities,
                rules,
                &self.interner,
                &self.house_alliances,
                attacker,
                victim,
            );
        }
        if let Some(entity) = self.substrate.entities.get_mut(victim) {
            entity.health.current = 0;
            crate::sim::combat::record_kill_credit(entity, killer, rules, &self.interner);
        }
        self.uninit_with_rules(victim, rules);
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
