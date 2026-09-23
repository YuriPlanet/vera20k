//! ParasiteClass: attack dogs and Terror Drones (`Parasite=` warheads).
//!
//! Native owner: one `ParasiteClass` (vtable `0x007EF890`, size `0x58`) per
//! non-building Techno whose weapon-0 warhead is a Parasite, allocated by
//! `TechnoClass::Init_Managers @ 0x006F40FC..0x006F414E` into Foot+69C. The
//! firer's `LimboLaunch=` shot limbos it inside the bullet
//! (`TechnoClass::Fire @ 0x006FF749..0x006FF872`); the Parasite detonation arm
//! (`BulletClass::DetonateAtCoord @ 0x004693D3`) calls AttachTo `0x0062A980`.
//! While attached the victim holds the owner in Foot+694 and runs the class AI
//! `0x00629FD0` at the tail of its own `FootClass::AI` (`0x004DAEE1`).
//! Release: ExitUnit `0x0062A4A0` (forced) or PointerExpired `0x0062A260`
//! (the victim expired). A victim's death releases its eater inside the
//! killing hit: ObjectClass::ReceiveDamage's exact-zero Destroy (`0x005F57AF`)
//! runs Detach_All(1), whose announce visits the victim itself; its
//! FootClass::PointerExpired forwards from Foot+694 to the eater's
//! ParasiteClass (`0x004D99AA..0x004D99C6`, `Simulation::object_destroy_callback`).
//! Ghidra plates on those addresses carry the evidence; retail data:
//! `[ParasiteDog]`, `[Parasite]`, `[BadTeeth]`, `[DroneJump]`.
//!
//! The owner entity holds [`ParasiteState`]; the victim holds only the
//! back-link `GameEntity::parasite_eating_me`.
//!
//! Forced releases ported: Sonic hits, third-party damage suppression and
//! heals (FootClass::ReceiveDamage `0x004D7330`), repair-depot steps (Radio
//! 0x1C `0x006F4D70`), Iron Curtain (FootClass::IronCurtain `0x004DEAE0`) and
//! the Teleport warp step (`0x007195BF`). Victim restrictions ported: transport
//! loading, tank bunkers, DeploysInto, retaliation against the eater.
//!
//! RESIDUALS (each needs a mechanism VERA does not have yet):
//! - Area Guard after a release: Enter_Idle_Mode (Infantry `0x0051CD3E..`,
//!   Unit `0x00738B67..`) picks Area Guard for DefaultToGuardArea types
//!   (DOG/ADOG/DRON and 8 more) and, IQ-gated, for AI houses; the shared
//!   selector (`queue_foot_enter_idle_mode`) still picks Guard. Effect: a
//!   released dog or drone guards in place instead of chasing nearby targets.
//!   Frequency: every release. Owner: the Enter_Idle_Mode selector port.
//! - Unlimbo's Can_Enter_Cell (`0x005F4F1B..0x005F4F44`) runs on the unbracketed
//!   AttachTo-refusal and ExitUnit releases and deletes an owner it refuses
//!   (Unit/Infantry skip occupants of the attached victim's cell,
//!   `0x0073F530`, `0x0051C259`). VERA places the owner. Trigger: a launch cell
//!   taken during the jump, or a NearbyLocation cell only the victim can use.
//!   Frequency: rare. Owner: the CanEnterCell consolidation (Drive/Ship port).
//! - Paralysis in the movers: Drive/Ship Set_Destination and Process_Movement
//!   (`0x004AFD5E`, `0x004B2759`, `0x0069F46E`, `0x006A1DA9`), Fly
//!   `0x004CCD00`/`0x004CF96E`, Hover `0x00514DB9`/`0x00514FD2`, Teleport
//!   `0x00718120`, `0x005B0099`, `0x00728B10`, and the player-control virtual
//!   +0xA0 (`0x00700C90`) refuse a paralyzed Foot. Trigger: a successful
//!   release paralyzes the owner for 3x ROF (a drone knocked off a Chrono
//!   Miner's warp: 180 frames). Effect: VERA's released drone moves and takes
//!   orders at once. Frequency: rare. Risk: movement timing only. Owner: the
//!   Drive/Ship Process path port, which holds those locomotors.
//! - Grinder (UnitClass::PerCellProcess `0x0073A13E..0x0073A181`): credits the
//!   eater's refund, then suppression 50 + ExitUnit. VERA has no grinding.
//! - ChronoWarp (SuperClass::Launch case 4 `0x006CC763`): a Naval eater gets
//!   suppression 500 + ExitUnit. VERA has no Chronosphere superweapon.
//! - Magnetron lift (`0x00710026`): a Naval eater exits. VERA does not port the
//!   IsLocomotor detonation arm.
//! - Sonic weapons may target an allied infected Foot (What_Action_OnObject
//!   `0x00700377`); the Sonic hit already ejects any eater (`0x004D7345`), but
//!   the order needs the cursor/command legality VERA does not port yet.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::movement::ground_pose;
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;

/// Foot+698 launch lock: `TechnoClass::Fire @ 0x006FF81F` stores
/// `Frame + 0x14`; GetFireError `0x006FCAE1` refuses parasite shots before it.
pub(crate) const LAUNCH_LOCK_FRAMES: u32 = 0x14;

/// Forced releases (repair `Receive_Radio 0x1C`, heal, Iron Curtain, grinder)
/// arm the suppression timer with 50 frames before ExitUnit.
pub(crate) const FORCED_RELEASE_SUPPRESSION_FRAMES: i32 = 50;

/// Bridge deck height `DAT_00AC497C`, added to release coordinates on a deck.
const BRIDGE_HEIGHT_LEPTONS: i32 = 4 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;

/// Retained ParasiteClass instance fields. The owner (`+0x24`) is the entity
/// storing this value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ParasiteState {
    /// `+0x28` Victim (FootClass).
    victim: Option<u64>,
    /// `+0x2C` suppression timer. Armed by third-party damage and forced
    /// releases; a release while it runs deletes the owner with its host.
    suppression: CdTimer,
    /// `+0x38` damage-delivery timer (bite cadence).
    damage: CdTimer,
}

impl ParasiteState {
    /// `ParasiteClass(owner) @ 0x006292B0`: both timers start at the current
    /// frame with zero duration (already expired); no victim.
    pub(crate) fn constructed(frame: u32) -> Self {
        Self {
            victim: None,
            suppression: CdTimer::started(frame as i32, 0),
            damage: CdTimer::started(frame as i32, 0),
        }
    }

    #[cfg(test)]
    pub(crate) fn victim(&self) -> Option<u64> {
        self.victim
    }

    pub(crate) fn suppress(&mut self, frame: u32, duration: i32) {
        self.suppression = CdTimer::started(frame as i32, duration);
    }

    /// ParasiteClass::Load `0x006295DB..0x006295F3`: both timers restart at
    /// the load frame with zero duration.
    pub(crate) fn restart_timers_after_load(&mut self, frame: u32) {
        self.suppression = CdTimer::started(frame as i32, 0);
        self.damage = CdTimer::started(frame as i32, 0);
    }
}

/// Naval+Organic owners (retail `[SQD]`) run the grapple machine `0x006297F0`
/// from the class AI (`0x00629FE4..0x0062A00C`).
///
/// RESIDUAL (Squid grapple): the grapple state machine, its SQDG anim, wakes,
/// splashes, Culling and per-tick victim paralysis are not ported. Until they
/// are, such owners neither LimboLaunch nor attach (both gates below). Trigger:
/// a Giant Squid (SQD) firing SquidGrab/SquidGrabE. Effect: SQDJUMP is Inviso,
/// so VERA's instant-hit path deals ordinary ParasitePlus damage once per
/// weapon ROF (99) with the squid visible and the ship free, where native
/// grapples, paralyzes, rocks and damages the ship every 40 frames and culls it
/// when weak. Frequency: Yuri's Revenge naval games. Risk: naval balance.
/// (The instant-hit path also skips every special detonation arm; that gap is
/// shared with other Inviso special warheads and tracked separately.)
pub(crate) fn owner_uses_grapple(object: &ObjectType) -> bool {
    object.naval && object.organic
}

/// `TechnoClass::Init_Managers @ 0x006F40FC..0x006F412A`: non-buildings whose
/// weapon 0 (the elite weapon when elite, through `GetWeapon(0)`) carries a
/// Parasite warhead own a ParasiteClass.
pub(crate) fn type_allocates_parasite(
    rules: &RuleSet,
    object: &ObjectType,
    category: EntityCategory,
    veterancy: u16,
) -> bool {
    category != EntityCategory::Structure
        && weapon_zero(rules, object, veterancy)
            .and_then(|weapon| weapon.warhead.as_deref())
            .and_then(|warhead| rules.warhead(warhead))
            .is_some_and(|warhead| warhead.parasite)
}

fn weapon_zero<'r>(
    rules: &'r RuleSet,
    object: &ObjectType,
    veterancy: u16,
) -> Option<&'r crate::rules::weapon_type::WeaponType> {
    super::combat_weapon::weapon_for_index(object, veterancy, 0)
        .and_then(|(name, _)| rules.weapon(name))
}

/// Primary facing as the native 16-bit word (`FacingClass::Current`).
fn facing_word(entity: &crate::sim::game_entity::GameEntity, frame: u32) -> u16 {
    entity
        .body_facing
        .map_or(u16::from(entity.facing) << 8, |facing| {
            facing.current(frame)
        })
}

/// `((raw >> 12) + 1) >> 1 & 7`: the eight-way direction of a facing word.
fn dir8(raw: u16) -> usize {
    (((u32::from(raw) >> 12) + 1) >> 1) as usize & 7
}

/// Unlimbo DirType argument `((raw >> 7) + 1) >> 1 & 0xFF`.
fn dir_type(raw: u16) -> u8 {
    ((((u32::from(raw) >> 7) + 1) >> 1) & 0xFF) as u8
}

/// `MapCoord_StepByDir` offsets, `g_DirectionOffsets @ 0x0089F688`.
const ADJACENT: [(i16, i16); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

fn cell_centre(cell: (i16, i16)) -> DriveCoord {
    DriveCoord {
        x: i32::from(cell.0) * 256 + 128,
        y: i32::from(cell.1) * 256 + 128,
        z: 0,
    }
}

impl Simulation {
    /// `CanInfect @ 0x0062A8E0` for AttachTo, through the same
    /// [`ParasiteVictimFacts`](super::combat_weapon::ParasiteVictimFacts)
    /// GetFireError reads.
    pub(crate) fn parasite_can_infect(
        &self,
        owner: u64,
        victim: Option<u64>,
        rules: &RuleSet,
    ) -> bool {
        let Some((victim_entity, victim_object)) = victim
            .and_then(|id| self.substrate.entities.get(id))
            .and_then(|entity| Some((entity, self.object_type(entity.type_ref(), rules)?)))
        else {
            return false;
        };
        let naval_owner = self
            .substrate
            .entities
            .get(owner)
            .and_then(|o| self.object_type(o.type_ref(), rules))
            .is_some_and(|object| object.naval);
        super::combat_weapon::ParasiteVictimFacts::of(
            victim_entity,
            victim_object,
            self.resolved_terrain.as_ref(),
        )
        .admits(naval_owner)
    }

    /// `AttachTo @ 0x0062A980`, reached only from the Parasite detonation arm
    /// with the bullet Target filtered to FootClass (`0x00469406`).
    pub(crate) fn parasite_attach(&mut self, owner: u64, victim: Option<u64>, rules: &RuleSet) {
        let frame = self.session.binary_frame;
        // Squid residual: a visible grapple owner must not attach while its
        // grapple is unported (every release path would fail its reveal).
        if self
            .substrate
            .entities
            .get(owner)
            .and_then(|o| self.object_type(o.type_ref(), rules))
            .is_some_and(owner_uses_grapple)
        {
            return;
        }
        let Some(state) = self
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|o| o.parasite.as_deref_mut())
        else {
            return;
        };
        // 0x0062A9D8: the first bite lands on the victim's next AI visit.
        state.damage = CdTimer::started(frame as i32, 0);
        if !self.parasite_can_infect(owner, victim, rules) {
            self.parasite_return_to_launch_cell(owner, rules);
            return;
        }
        let victim = victim.expect("CanInfect admitted a victim");
        // 0x0062AAD9..0x0062AB24: the OWNER's locomotor Force_Track(-1, victim
        // XYZ). Drive resets its track (and returns with the owner in limbo);
        // Walk binds the base no-op 0x0055AC10.
        let victim_coord = ground_pose::position_world_coord(
            &self.substrate.entities.get(victim).unwrap().position,
        );
        self.force_drive_track(owner, -1, victim_coord);
        self.substrate
            .entities
            .get_mut(victim)
            .unwrap()
            .parasite_eating_me = Some(owner);
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|o| o.parasite.as_deref_mut())
        {
            state.victim = Some(victim);
        }
    }

    /// AttachTo refusal `0x0062AA02..0x0062AAD6`: Unlimbo at the centre of the
    /// owner's last map cell (Foot+55C, its launch cell), facing 0; failure
    /// deletes it. This path neither reselects nor rejoins a team.
    fn parasite_return_to_launch_cell(&mut self, owner: u64, rules: &RuleSet) {
        let Some(cell) = self
            .substrate
            .entities
            .get(owner)
            .map(|o| o.navigation.neighbor_state.cell())
        else {
            return;
        };
        if !self.parasite_unlimbo_owner(owner, cell_centre(cell), 0, false, rules) {
            self.uninit_with_rules(owner, rules);
            return;
        }
        self.parasite_released_owner_orders(owner, false, rules);
    }

    /// Release tail shared by AttachTo refusal, ExitUnit and PointerExpired:
    /// VERA has no planning path (Foot vtable +0x4AC is false), so the owner
    /// clears its target and destination, then Enter_Idle_Mode(0,1). Only
    /// ExitUnit (`0x0062A771`) and PointerExpired (`0x0062A3D4`) also clear the
    /// archive target; the AttachTo refusal (`0x0062AA96..0x0062AAC9`) keeps it.
    fn parasite_released_owner_orders(&mut self, owner: u64, clear_archive: bool, rules: &RuleSet) {
        if let Some(entity) = self.substrate.entities.get_mut(owner) {
            if clear_archive {
                entity.base_defense_response.set_archive_target(None);
            }
            crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
            crate::sim::mission::concrete_effects::represented_assign_destination_mode_one(
                entity, None,
            );
            entity.movement_target = None;
        }
        crate::sim::world::queue_foot_enter_idle_mode(self, owner, rules);
    }

    /// `+0x432` reselect memo, consumed only by successful releases
    /// (`0x0062A71C`, `0x0062A37F`) for the local player's house.
    fn parasite_reselect(&mut self, owner: u64) {
        let current = self.session.current_house;
        if let Some(entity) = self.substrate.entities.get_mut(owner)
            && entity.limbo_reselect
            && current.is_some_and(|house| house == entity.owner())
        {
            entity.limbo_reselect = false;
            entity.selected = true;
        }
    }

    /// ParasiteClass AI `0x00629FD0`, run at the tail of the victim's
    /// `FootClass::AI` (`0x004DAEE1..0x004DAEF3`).
    pub(crate) fn parasite_ai_for_victim(
        &mut self,
        victim: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        let frame = self.session.binary_frame;
        let Some(owner) = self
            .substrate
            .entities
            .get(victim)
            .and_then(|v| v.parasite_eating_me)
        else {
            return;
        };
        let Some(owner_entity) = self.substrate.entities.get(owner) else {
            return;
        };
        let Some(state) = owner_entity.parasite.as_deref() else {
            return;
        };
        let Some(victim) = state.victim else {
            return;
        };
        let Some(owner_object) = self.object_type(owner_entity.type_ref(), rules) else {
            return;
        };
        if owner_uses_grapple(owner_object) {
            return;
        }
        let Some(weapon) = weapon_zero(rules, owner_object, owner_entity.veterancy) else {
            return;
        };
        let Some(warhead_name) = weapon.warhead.as_deref() else {
            return;
        };
        let Some(warhead) = rules.warhead(warhead_name) else {
            return;
        };
        if !state.damage.expired(frame as i32) {
            return;
        }
        let (rof, damage, paralyzes) = (weapon.rof, weapon.damage, warhead.paralyzes);
        let anims = weapon.anim.clone();
        let warhead_id = self.interner.intern(warhead_name);
        self.substrate
            .entities
            .get_mut(owner)
            .unwrap()
            .parasite
            .as_deref_mut()
            .unwrap()
            .damage = CdTimer::started(frame as i32, rof);
        let Some(victim_entity) = self.substrate.entities.get_mut(victim) else {
            return;
        };
        victim_entity.paralysis_timer = CdTimer::started(frame as i32, paralyzes);
        let infantry = victim_entity.category == EntityCategory::Infantry;
        let location = ground_pose::position_world_coord(&victim_entity.position);
        let raw = facing_word(victim_entity, frame);
        let (amount, ignore_defenses) = if infantry {
            // 0x0062A0AF: an Infantry victim takes its own current Health,
            // ignoring defenses: one bite kills unless ReceiveDamage refuses.
            (victim_entity.health.current, true)
        } else {
            if let Some(system) = rules
                .combat_damage
                .default_spark_system
                .as_deref()
                .and_then(|name| rules.ps_type_id_by_name(name))
            {
                self.spawn_particle_system(
                    system,
                    glam::IVec3::new(location.x, location.y, location.z),
                    None,
                    None,
                    glam::IVec3::ZERO,
                    None,
                    rules,
                );
            }
            if let Some(name) = anims.get(dir8(raw)).filter(|_| !anims.is_empty()) {
                let world = crate::sim::anim_class::AnimWorldCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                };
                let (rx, ry, sx, sy, z) = world.to_cell_sub_z();
                let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
                    loop_count: 1,
                    draw_flags: 0x600,
                    ..crate::sim::components::AnimClassSpawnDescriptor::new(
                        self.interner.intern(name),
                        rx,
                        ry,
                        sx,
                        sy,
                        z,
                    )
                };
                if let Err(error) = self.spawn_anim_at_world(rules, descriptor, world) {
                    log::debug!("parasite bite anim [{name}] did not construct: {error}");
                }
            }
            // 0x0062A17F: the rock's lateral sign. TechnoClass::Rock
            // (vtable +0x3D8) itself belongs to the rocking subsystem, which
            // has no producer or renderer yet; the Scenario draw is kept.
            let _ = self.scenario_rng.next_range_i32_inclusive(0, 1);
            (damage, false)
        };
        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            victim,
            amount,
            0,
            owner,
            None,
            warhead_id,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses,
                arg6: true,
            },
        );
        self.commit_direct_damage_receiver(rules, overlay_registry, event);
    }

    /// ExitUnit `0x0062A4A0`: forced release from the victim.
    pub(crate) fn parasite_exit_unit(&mut self, owner: u64, rules: &RuleSet) {
        let frame = self.session.binary_frame;
        let Some(owner_entity) = self.substrate.entities.get(owner) else {
            return;
        };
        let Some(victim) = owner_entity.parasite.as_deref().and_then(|s| s.victim) else {
            return;
        };
        let Some(owner_object) = self.object_type(owner_entity.type_ref(), rules) else {
            return;
        };
        let naval = owner_object.naval;
        let suppressed = owner_entity
            .parasite
            .as_deref()
            .is_some_and(|s| s.suppression.remaining(frame as i32) != 0);
        let (speed_type, movement_zone) = (owner_object.speed_type, owner_object.movement_zone);
        let rof = weapon_zero(rules, owner_object, owner_entity.veterancy).map_or(0, |w| w.rof);
        let Some(victim_entity) = self.substrate.entities.get(victim) else {
            return;
        };
        let raw = facing_word(victim_entity, frame);
        let released = if dir8(raw) <= 2 {
            raw.wrapping_add(0x3FFF)
        } else {
            raw.wrapping_sub(0x3FFF)
        };
        let victim_cell = (
            victim_entity.position.rx as i16,
            victim_entity.position.ry as i16,
        );
        if !naval && suppressed {
            // 0x0062A89B: the owner dies with its host.
            self.substrate
                .entities
                .get_mut(owner)
                .unwrap()
                .health
                .current = 0;
            self.detach_parasite_victim(owner, victim, frame);
            self.uninit_with_rules(owner, rules);
            return;
        }
        let requested = if naval {
            let (dx, dy) = ADJACENT[dir8(released)];
            (
                victim_cell.0.wrapping_add(dx),
                victim_cell.1.wrapping_add(dy),
            )
        } else {
            victim_cell
        };
        let cell = if self.cell_clear_for(requested, speed_type, movement_zone, rules) {
            Some(requested)
        } else {
            self.techno_nearby_location(victim, rules)
        };
        let placed = cell.is_some_and(|cell| {
            self.parasite_can_place_at_victim(owner, victim, rules)
                && self.parasite_unlimbo_owner(
                    owner,
                    cell_centre(cell),
                    dir_type(released),
                    false,
                    rules,
                )
        });
        if placed {
            self.parasite_reselect(owner);
            self.parasite_released_owner_orders(owner, true, rules);
            if let Some(entity) = self.substrate.entities.get_mut(owner) {
                entity.paralysis_timer = CdTimer::started(frame as i32, rof.wrapping_mul(3));
            }
        } else {
            self.substrate
                .entities
                .get_mut(owner)
                .unwrap()
                .health
                .current = 0;
            self.uninit_with_rules(owner, rules);
        }
        self.detach_parasite_victim(owner, victim, frame);
    }

    /// Victim-side reset shared by every release: ParalysisTimer {Frame, 0},
    /// Foot+694 = 0, Parasite.Victim = 0 (`0x0062A862..0x0062A88F`).
    fn detach_parasite_victim(&mut self, owner: u64, victim: u64, frame: u32) {
        if let Some(entity) = self.substrate.entities.get_mut(victim) {
            entity.paralysis_timer = CdTimer::started(frame as i32, 0);
            if entity.parasite_eating_me == Some(owner) {
                entity.parasite_eating_me = None;
            }
        }
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|o| o.parasite.as_deref_mut())
        {
            state.victim = None;
        }
    }

    /// ParasiteClass::PointerExpired `0x0062A260`, forwarded by the victim's
    /// `FootClass::PointerExpired @ 0x004D99AA..0x004D99C6` while the owner
    /// is alive. Returns without effect unless `expired` is the victim.
    pub(crate) fn parasite_pointer_expired(
        &mut self,
        owner: u64,
        expired: u64,
        rules: Option<&RuleSet>,
    ) {
        let frame = self.session.binary_frame;
        let Some(state) = self
            .substrate
            .entities
            .get(owner)
            .and_then(|o| o.parasite.as_deref())
        else {
            return;
        };
        if state.victim != Some(expired) {
            return;
        }
        let victim = expired;
        let suppressed = state.suppression.remaining(frame as i32) != 0;
        let Some(rules) = rules else {
            // A rules-less fixture cannot run Unlimbo; drop the link only.
            self.detach_parasite_victim(owner, victim, frame);
            return;
        };
        // Owner vtable +0x160 (0x0041BF40): Iron Curtain protects it here.
        let owner_protected = self.substrate.entities.get(owner).is_some_and(|o| {
            crate::sim::superweapon::invulnerability::is_invulnerable(
                o.invulnerability.as_ref(),
                frame,
            )
        });
        if suppressed && !owner_protected {
            if let Some(entity) = self.substrate.entities.get_mut(victim) {
                entity.parasite_eating_me = None;
            }
            self.clear_parasite_victim(owner);
            self.uninit_with_rules(owner, rules);
            return;
        }
        let victim_raw = self
            .substrate
            .entities
            .get(victim)
            .map_or(0, |v| facing_word(v, frame));
        let coords = self.parasite_release_coords(owner, victim, rules);
        // 0x0062A2ED..0x0062A303: the 0xA8E7AC bracket around Unlimbo.
        let placed = coords.is_some_and(|coords| {
            self.parasite_unlimbo_owner(owner, coords, dir_type(victim_raw), true, rules)
        });
        if placed {
            self.parasite_reselect(owner);
            self.parasite_released_owner_orders(owner, true, rules);
        } else {
            self.substrate
                .entities
                .get_mut(owner)
                .unwrap()
                .health
                .current = 0;
            self.uninit_with_rules(owner, rules);
        }
        self.clear_parasite_victim(owner);
    }

    fn clear_parasite_victim(&mut self, owner: u64) {
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|o| o.parasite.as_deref_mut())
        {
            state.victim = None;
        }
    }

    /// GetReleaseCoords `0x0062AC30`.
    fn parasite_release_coords(
        &mut self,
        owner: u64,
        victim: u64,
        rules: &RuleSet,
    ) -> Option<DriveCoord> {
        let victim_entity = self.substrate.entities.get(victim)?;
        let owner_is_unit = self
            .substrate
            .entities
            .get(owner)
            .is_some_and(|o| o.category == EntityCategory::Unit);
        let victim_on_bridge = victim_entity.on_bridge;
        if self.parasite_can_place_at_victim(owner, victim, rules) {
            let victim_entity = self.substrate.entities.get(victim)?;
            let mut coords = ground_pose::position_world_coord(&victim_entity.position);
            if owner_is_unit {
                let cell = ((coords.x / 256) as i16, (coords.y / 256) as i16);
                coords = cell_centre(cell);
                coords.z = self.cell_ground_z(cell);
                if victim_on_bridge {
                    coords.z = coords.z.wrapping_add(BRIDGE_HEIGHT_LEPTONS);
                }
            }
            self.substrate.entities.get_mut(owner)?.on_bridge = victim_on_bridge;
            return Some(coords);
        }
        let frame = self.session.binary_frame;
        let raw = facing_word(victim_entity, frame);
        let victim_cell = (
            victim_entity.position.rx as i16,
            victim_entity.position.ry as i16,
        );
        let (dx, dy) = ADJACENT[dir8(raw)];
        let adjacent = (
            victim_cell.0.wrapping_add(dx),
            victim_cell.1.wrapping_add(dy),
        );
        let naval_owner = self
            .substrate
            .entities
            .get(owner)
            .and_then(|o| self.object_type(o.type_ref(), rules))
            .is_some_and(|o| o.naval);
        if !naval_owner && self.land_blocks_release(adjacent, victim_cell) {
            return None;
        }
        // CellClass::PlaceInfantryInCell 0x00481180 from the adjacent cell's
        // centre, for every owner class; Unit owners then take the centre.
        let occupancy = self
            .substrate
            .occupancy
            .get(adjacent.0 as u16, adjacent.1 as u16);
        let centre = crate::util::fixed_math::SimFixed::from_num(128);
        let spot = crate::sim::movement::bump_crush::allocate_sub_cell_with_preference(
            occupancy,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            centre,
            centre,
            &mut self.scenario_rng,
        )?;
        let (sub_x, sub_y) = crate::util::lepton::subcell_lepton_offset(Some(spot));
        let mut coords = if owner_is_unit {
            cell_centre(adjacent)
        } else {
            DriveCoord {
                x: i32::from(adjacent.0) * 256 + sub_x.to_num::<i32>(),
                y: i32::from(adjacent.1) * 256 + sub_y.to_num::<i32>(),
                z: 0,
            }
        };
        coords.z = self.cell_ground_z(adjacent);
        let bridge = self.cell_has_bridge(adjacent);
        let on_bridge = if victim_on_bridge {
            if bridge {
                coords.z = coords.z.wrapping_add(BRIDGE_HEIGHT_LEPTONS);
            }
            bridge
        } else if bridge {
            // 0x0062AE63: only a deck below the victim's own height counts.
            let victim_z = self.cell_ground_z(victim_cell);
            if victim_z > coords.z {
                coords.z = coords.z.wrapping_add(BRIDGE_HEIGHT_LEPTONS);
                true
            } else {
                false
            }
        } else {
            false
        };
        self.substrate.entities.get_mut(owner)?.on_bridge = on_bridge;
        Some(coords)
    }

    /// CanPlaceAtVictim `0x0062AB40`.
    fn parasite_can_place_at_victim(&self, owner: u64, victim: u64, rules: &RuleSet) -> bool {
        let Some(victim_entity) = self.substrate.entities.get(victim) else {
            return false;
        };
        // Victim vtable +0x54 (IsInAir).
        if crate::sim::movement::air_movement::current_fly_height(
            victim_entity,
            self.resolved_terrain.as_ref(),
        ) > 0
        {
            return false;
        }
        let cell = (
            victim_entity.position.rx as i16,
            victim_entity.position.ry as i16,
        );
        let owner_entity = self.substrate.entities.get(owner);
        let naval = owner_entity
            .and_then(|o| self.object_type(o.type_ref(), rules))
            .is_some_and(|o| o.naval);
        if !naval {
            if owner_entity.is_some_and(|o| o.category == EntityCategory::Unit)
                && self
                    .production
                    .terrain_object_cells
                    .contains_key(&(cell.0 as u16, cell.1 as u16))
            {
                return false;
            }
            if self.land_blocks_release(cell, cell) {
                return false;
            }
        }
        self.substrate
            .occupancy
            .first_building_on_layer(
                cell.0 as u16,
                cell.1 as u16,
                crate::sim::movement::locomotor::MovementLayer::Ground,
            )
            .is_none()
    }

    /// Water/Beach/Rock land refuses a release unless the reference cell has a
    /// bridge flag or a bridge overlay (`0x0062ABB9..0x0062AC04`). Native reads
    /// the bridge tests from the victim's own cell even for the adjacent path.
    fn land_blocks_release(&self, cell: (i16, i16), bridge_reference: (i16, i16)) -> bool {
        use crate::rules::terrain_rules::LandType;
        let land = self
            .resolved_terrain
            .as_ref()
            .and_then(|t| t.cell(cell.0 as u16, cell.1 as u16))
            .map(|c| c.yr_cell_land_type);
        let restricted = land.is_some_and(|land| {
            land == LandType::Water.as_index()
                || land == LandType::Beach.as_index()
                || land == LandType::Rock.as_index()
        });
        if !restricted {
            return false;
        }
        let overlay = self.overlay_grid.as_ref().and_then(|grid| {
            grid.cell(bridge_reference.0 as u16, bridge_reference.1 as u16)
                .overlay_id
        });
        let overlay_bridge = overlay.is_some_and(|id| (0x4A..0x64).contains(&id))
            || overlay.is_some_and(|id| (0xCD..=0xE6).contains(&id));
        !(self.cell_has_bridge(bridge_reference) || overlay_bridge)
    }

    fn cell_has_bridge(&self, cell: (i16, i16)) -> bool {
        self.resolved_terrain.as_ref().is_some_and(|terrain| {
            terrain.native_cell_flags(terrain.native_cell_identity(cell)) & 0x100 != 0
        })
    }

    fn cell_ground_z(&self, cell: (i16, i16)) -> i32 {
        let centre = cell_centre(cell);
        ground_pose::ground_surface_z_at(
            [centre.x, centre.y],
            false,
            self.resolved_terrain.as_ref(),
            None,
        )
        .unwrap_or(0)
    }

    /// `CellClass::IsClearToMove @ 0x004834A0` with (SpeedType, 0, 0, zone,
    /// MovementZone, -1, 1) after `GetZoneID(cell, MovementZone, 0)`.
    fn cell_clear_for(
        &self,
        cell: (i16, i16),
        speed_type: crate::rules::locomotor_type::SpeedType,
        movement_zone: crate::rules::locomotor_type::MovementZone,
        rules: &RuleSet,
    ) -> bool {
        use crate::sim::cell_rect::{
            IsClearToMoveResult, LiveCellPassabilityQuery, evaluate_live_cell_passability,
        };
        let (Some(terrain), Some(zones)) =
            (self.resolved_terrain.as_ref(), self.zone_grid.as_ref())
        else {
            return false;
        };
        let target = (cell.0 as u16, cell.1 as u16);
        let Some(terrain_cell) = terrain.cell(target.0, target.1) else {
            return false;
        };
        let requested =
            zones.get_zone_id_native((i32::from(cell.0), i32::from(cell.1)), movement_zone, false);
        let actual = zones.get_zone_id_native(
            (i32::from(cell.0), i32::from(cell.1)),
            movement_zone,
            self.cell_has_bridge(cell),
        );
        let land = rules
            .terrain_rules
            .semantics_for_land_type(terrain_cell.yr_cell_land_type)
            .and_then(|row| row.cost_for_speed_type(speed_type))
            .is_some_and(|cost| cost > 0);
        let grid = self.path_grid_snapshot();
        matches!(
            evaluate_live_cell_passability(LiveCellPassabilityQuery {
                target,
                speed_type,
                movement_zone,
                requested_zone: requested.map(|zone| zone as i16),
                actual_zone: actual.map_or(-1, |zone| zone as i16),
                requested_layer: None,
                ignore_infantry: false,
                ignore_vehicles: false,
                land_passable: land,
                path_grid: grid.as_deref(),
                resolved_terrain: Some(terrain),
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
            }),
            IsClearToMoveResult::Clear { .. } | IsClearToMoveResult::ClearWinged
        )
    }

    /// TechnoClass NearbyLocation `0x00703590` (argument 0): FNPC seeded at the
    /// object's own cell with its SpeedType (Winged read as Track), its
    /// MovementZone, `GetZoneID(cell, MovementZone, OnBridge)` and OnBridge.
    fn techno_nearby_location(&self, id: u64, rules: &RuleSet) -> Option<(i16, i16)> {
        use crate::rules::locomotor_type::SpeedType;
        use crate::sim::find_nearby_cell::{
            NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs,
            find_nearby_passable_cell, map_owned_radius_cap,
        };
        let entity = self.substrate.entities.get(id)?;
        let object = self.object_type(entity.type_ref(), rules)?;
        let speed_type = if object.speed_type == SpeedType::Winged {
            SpeedType::Track
        } else {
            object.speed_type
        };
        let seed = (i32::from(entity.position.rx), i32::from(entity.position.ry));
        let zone = self.zone_grid.as_ref().and_then(|zones| {
            zones.get_zone_id_native(seed, object.movement_zone, entity.on_bridge)
        });
        let (width, height) = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(b, h)| (b.base, h))?;
        let grid = self.path_grid_snapshot();
        let cell = find_nearby_passable_cell(
            seed,
            &NearbyQuery {
                native_cells: None,
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: PassabilityArgs {
                    speed_type,
                    required_zone_id: zone,
                    movement_zone: object.movement_zone,
                    bridge_aware_zone: entity.on_bridge,
                },
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: true,
                check_height: false,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(width, height),
                target_cell: None,
                path_grid: grid.as_deref(),
                resolved_terrain: self.resolved_terrain.as_ref(),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: self.playfield_bounds,
            },
            self.session.binary_frame,
        )?;
        Some((cell.0 as i16, cell.1 as i16))
    }

    /// Unlimbo(coord, dir) for a released or returning owner. Infantry place
    /// through `PlaceInfantryInCell @ 0x00481180`: `guarded` is the
    /// `0x00A8E7AC` bracket PointerExpired holds, which selects the priority
    /// arm (no occupancy test, no RNG); otherwise the ordinary arm may draw.
    fn parasite_unlimbo_owner(
        &mut self,
        owner: u64,
        coord: DriveCoord,
        facing: u8,
        guarded: bool,
        rules: &RuleSet,
    ) -> bool {
        let rx = (coord.x / 256) as u16;
        let ry = (coord.y / 256) as u16;
        let Some(entity) = self.substrate.entities.get(owner) else {
            return false;
        };
        let infantry = entity.category == EntityCategory::Infantry;
        // Unlimbo receives the whole coordinate. A deck release from
        // GetReleaseCoords (`0x0062AC60..0x0062ACF6`) carries the deck height,
        // which the reveal adapter reads as `level + 4` with the owner's
        // OnBridge; cell-centre coordinates (ExitUnit, AttachTo refusal) are
        // ground, `CellClass::GetCoords @ 0x00486840`.
        let ground_level = self
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(rx, ry))
            .map_or(0, |cell| cell.level);
        let coord_level =
            u8::try_from(coord.z.max(0) / crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS)
                .unwrap_or(u8::MAX);
        let z = ground_level.max(coord_level);
        let sub_x = crate::util::fixed_math::SimFixed::from_num(coord.x.rem_euclid(256));
        let sub_y = crate::util::fixed_math::SimFixed::from_num(coord.y.rem_euclid(256));
        let (sub_cell, sub_x, sub_y) = if infantry {
            let spot = if guarded {
                crate::sim::movement::bump_crush::priority_sub_cell(sub_x, sub_y)
            } else {
                let occupancy = self.substrate.occupancy.get(rx, ry);
                let Some(spot) =
                    crate::sim::movement::bump_crush::allocate_sub_cell_with_preference(
                        occupancy,
                        crate::sim::movement::locomotor::MovementLayer::Ground,
                        None,
                        sub_x,
                        sub_y,
                        &mut self.scenario_rng,
                    )
                else {
                    return false;
                };
                spot
            };
            let (x, y) = crate::util::lepton::subcell_lepton_offset(Some(spot));
            (Some(spot), x, y)
        } else {
            (None, sub_x, sub_y)
        };
        if let Some(entity) = self.substrate.entities.get_mut(owner) {
            entity.sub_cell = sub_cell;
            entity.facing = facing;
            if let Some(body) = entity.body_facing.as_mut() {
                body.snap(u16::from(facing) << 8, self.session.binary_frame);
            }
        }
        matches!(
            self.try_reveal_entity_with_context(
                owner,
                crate::sim::world::RevealRequest {
                    position: crate::sim::world::RevealPosition {
                        rx,
                        ry,
                        z,
                        sub_x,
                        sub_y,
                    },
                    placement: crate::sim::world::PlacementEvidence::MarkSucceeded,
                    logic_eligible: true,
                },
                crate::sim::world::UninitContext::with_rules(rules),
            ),
            crate::sim::world::RevealOutcome::Revealed { .. }
        )
    }

    /// `TechnoClass::Fire @ 0x006FF749..0x006FF872`, the LimboLaunch block.
    /// The bullet is already built and launched; VERA's pending projectile
    /// keeps its source, which is what native's bullet Limbo/re-Init restores.
    pub(crate) fn parasite_limbo_launch(
        &mut self,
        firer: u64,
        target: super::TargetKind,
        weapon: &crate::rules::weapon_type::WeaponType,
        rules: &RuleSet,
    ) {
        let frame = self.session.binary_frame;
        let Some(entity) = self.substrate.entities.get(firer) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        if owner_uses_grapple(object) {
            return;
        }
        let target_id = match target {
            super::TargetKind::Entity(id) => Some(id),
            super::TargetKind::Cell(..) => None,
        };
        let target_entity = target_id.and_then(|id| self.substrate.entities.get(id));
        let target_infantry =
            target_entity.is_some_and(|target| target.category == EntityCategory::Infantry);
        // 0x006FF763..0x006FF79C: ReselectIfLimboed memo for a selected,
        // locally controlled firer jumping an Infantry target.
        let reselect = object.reselect_if_limboed
            && entity.selected
            && self.session.current_house == Some(entity.owner())
            && target_infantry;
        // RESIDUAL: 0x006FF7A3..0x006FF7E9 stores the Team (+0x434) for
        // RejoinTeamIfLimboed, and Limbo's Detach_All removes the member
        // (0x006EA870). VERA has no Team membership owner, so a jumping AI
        // dog or drone stays listed in its team while limboed and a
        // RejoinTeamIfLimboed=no drone is never dropped from it.
        if reselect {
            self.substrate
                .entities
                .get_mut(firer)
                .unwrap()
                .limbo_reselect = true;
        }
        self.techno_limbo_with_rules(firer, rules);
        let parasite = weapon
            .warhead
            .as_deref()
            .and_then(|name| rules.warhead(name))
            .is_some_and(|warhead| warhead.parasite);
        // 0x006FF809..0x006FF81F: lock a Foot target for 20 frames.
        if parasite
            && let Some(target) = target_id.and_then(|id| self.substrate.entities.get_mut(id))
            && matches!(
                target.category,
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
        {
            target.parasite_launch_lock = frame.wrapping_add(LAUNCH_LOCK_FRAMES);
        }
    }

    /// FootClass::PointerExpired `0x004D998C..0x004D99CD`, after the Techno
    /// body, for one listener.
    pub(crate) fn foot_parasite_pointer_expired(
        &mut self,
        listener: u64,
        expired: u64,
        rules: Option<&RuleSet>,
    ) {
        let Some(eater) = self
            .substrate
            .entities
            .get(listener)
            .and_then(|l| l.parasite_eating_me)
        else {
            return;
        };
        let eater_health = |sim: &Simulation, eater: u64| {
            sim.substrate
                .entities
                .get(eater)
                .map_or(0, |e| e.health.current)
        };
        // 0x004D998C..0x004D99A4: an expiring eater that is dead drops the
        // link (the scenario-ended arm has no represented state).
        if eater == expired && eater_health(self, eater) == 0 {
            self.substrate
                .entities
                .get_mut(listener)
                .unwrap()
                .parasite_eating_me = None;
        }
        // 0x004D99AA..0x004D99C6: forward to a live eater's ParasiteClass.
        if let Some(eater) = self
            .substrate
            .entities
            .get(listener)
            .and_then(|l| l.parasite_eating_me)
            && eater_health(self, eater) > 0
        {
            self.parasite_pointer_expired(eater, expired, rules);
        }
        // 0x004D99C9: the victim itself expiring clears its own link.
        if expired == listener
            && let Some(entity) = self.substrate.entities.get_mut(listener)
        {
            entity.parasite_eating_me = None;
        }
    }

    /// FootClass::ReceiveDamage `0x004D7330` prefix, ahead of the Techno
    /// receiver: a Sonic hit ejects the parasite (and drops the shooter's
    /// Target), a third party's raw damage above the eater type's
    /// SuppressionThreshold arms `2*damage - threshold` of suppression, and a
    /// heal (negative damage) arms 50 and ejects.
    pub(crate) fn foot_receive_damage_parasite_prefix(
        &mut self,
        victim: u64,
        source: Option<u64>,
        raw_damage: i32,
        warhead: crate::sim::intern::InternedId,
        rules: &RuleSet,
    ) {
        let frame = self.session.binary_frame;
        let Some(eater) = self
            .substrate
            .entities
            .get(victim)
            .and_then(|v| v.parasite_eating_me)
        else {
            return;
        };
        let sonic = rules
            .warhead(self.interner.resolve(warhead))
            .is_some_and(|w| w.sonic);
        if sonic {
            self.parasite_exit_unit(eater, rules);
            if let Some(source) = source.and_then(|id| self.substrate.entities.get_mut(id)) {
                crate::sim::mission::concrete_effects::represented_assign_target(source, None);
            }
            return;
        }
        let threshold = self
            .substrate
            .entities
            .get(eater)
            .and_then(|e| self.object_type(e.type_ref(), rules))
            .map_or(0, |o| o.suppression_threshold);
        if source != Some(eater)
            && raw_damage > threshold
            && let Some(state) = self
                .substrate
                .entities
                .get_mut(eater)
                .and_then(|e| e.parasite.as_deref_mut())
        {
            state.suppress(frame, raw_damage.wrapping_mul(2).wrapping_sub(threshold));
        }
        if raw_damage < 0 {
            self.parasite_force_release(eater, FORCED_RELEASE_SUPPRESSION_FRAMES, rules);
        }
    }

    /// The forced-release idiom of the repair, heal, Iron Curtain and grinder
    /// sites: arm the suppression timer, then ExitUnit.
    pub(crate) fn parasite_force_release(&mut self, eater: u64, suppression: i32, rules: &RuleSet) {
        let frame = self.session.binary_frame;
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(eater)
            .and_then(|e| e.parasite.as_deref_mut())
        {
            state.suppress(frame, suppression);
        }
        self.parasite_exit_unit(eater, rules);
    }
}

#[cfg(test)]
#[path = "parasite_tests.rs"]
mod tests;
