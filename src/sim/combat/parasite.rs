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
//! Each Foot's parasite fields are one [`FootParasite`]: the owner holds its
//! [`ParasiteState`], the victim the back-link to its eater.
//!
//! Forced releases ported: Sonic hits, third-party damage suppression and
//! heals (FootClass::ReceiveDamage `0x004D7330`), repair-depot steps (Radio
//! 0x1C `0x006F4D70`), Iron Curtain (FootClass::IronCurtain `0x004DEAE0`) and
//! the Teleport warp step (`0x007195BF`). Victim restrictions ported: transport
//! loading, tank bunkers, DeploysInto, retaliation against the eater.
//!
//! Every release unlimbos the owner through the class Unlimbo
//! (`Simulation::unlimbo_class_entry`). The AttachTo refusal and ExitUnit
//! raise no `0x00A8E7AC`, so the owner's class `Can_Enter_Cell`
//! (`0x005F4F1B..0x005F4F44`) decides, and an owner it refuses is deleted;
//! an owner still holding its victim is admitted at the victim's cell
//! (`0x0073F530`, `0x0051C259`). PointerExpired raises it around its Unlimbo
//! (`0x0062A2F8..0x0062A311`, cleared at `0x0062A372..0x0062A379` and
//! `0x0062A45C..0x0062A462`), which skips that query.
//!
//! RESIDUALS (each needs a mechanism VERA does not have yet):
//! - A placed owner's sight: native follows the Unlimbo with ReReveal
//!   (AttachTo refusal, `0x0062AA51`) or UpdateReveal (ExitUnit `0x0062A7A8`,
//!   PointerExpired `0x0062A40B`), and the fog border `0x00567DA0`
//!   (presentation; `0x0062AA91`, `0x0062A44B`). VERA's vision frame pass
//!   admits the placed owner in their place (the residual in `sim::vision`),
//!   so after the AttachTo refusal it holds sight where native's ReReveal has
//!   just released the reveal Unlimbo made (`0x006F6D27`), until its next
//!   cell arrival. Trigger: a jump whose host is lost in flight. Effect: the
//!   returned drone sees around its launch cell before it moves again.
//!   Frequency: rare.
//! - GetReleaseCoords' PlaceInfantryInCell finding no spot writes the zero
//!   coordinate (`0x0089E778`), which a Unit owner turns into map cell
//!   (0,0)'s GetCoords (`0x0062AE09..0x0062AE3E`); that is no longer the
//!   invalid `0x00AC4948`, so PointerExpired unlimboes the owner there under
//!   `0x00A8E7AC`, outside the map. VERA has no off-map Unlimbo and deletes
//!   the owner. Trigger: a drone's victim dies where the drone cannot take
//!   its place (in the air, on a building or terrain object, on water) and
//!   the cell ahead of the victim holds a vehicle or a closed object.
//!   Effect: the drone is lost instead of standing off the map's corner.
//!   Frequency: rare. Risk: the owner's unit count only.
//! - Paralysis in some movers: Fly `0x004CCD00`/`0x004CF96E`, Mech Move_To
//!   `0x005B0099` and Tunnel Move_To `0x00728B10` refuse a paralyzed Foot;
//!   VERA's ports of those do not read [`FootParasite::is_paralyzed`] yet
//!   (Drive/Ship Move_To and fresh Process_Movement, Hover, Teleport and the
//!   player-control virtual +0xA0 `0x00700C90` do). Trigger: a successful
//!   release paralyzes the owner for 3x ROF (a drone knocked off a Chrono
//!   Miner's warp: 180 frames). Effect: such a released owner can be given a
//!   new destination at once. Frequency: rare. Risk: movement timing only.
//!   Owner: those locomotors' ports.
//! - Grinder (UnitClass::PerCellProcess `0x0073A13E..0x0073A181`): credits the
//!   eater's refund, then suppression 50 + ExitUnit. VERA has no grinding.
//! - Sonic weapons may target an allied infected Foot (What_Action_OnObject
//!   `0x00700377`); the Sonic hit already ejects any eater (`0x004D7345`), but
//!   the order needs the cursor/command legality VERA does not port yet.

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::movement::{ground_pose, target_cell_coord};
use crate::sim::timer::CdTimer;
use crate::sim::world::{FrameEffects, Simulation, UninitContext};
use crate::util::direction_tables::CELL_DELTAS;

#[path = "parasite_grapple.rs"]
mod grapple;

/// Foot+698 launch lock: `TechnoClass::Fire @ 0x006FF81F` stores
/// `Frame + 0x14`; GetFireError `0x006FCAE1` refuses parasite shots before it.
pub(crate) const LAUNCH_LOCK_FRAMES: u32 = 0x14;

/// Forced releases (repair `Receive_Radio 0x1C`, heal, Iron Curtain, grinder)
/// arm the suppression timer with 50 frames before ExitUnit.
pub(crate) const FORCED_RELEASE_SUPPRESSION_FRAMES: i32 = 50;

/// The Chrono Warp's release of a Naval eater (`SuperClass::Launch` case 4,
/// `0x006CC78F..0x006CC7B7`): suppression 500, then ExitUnit.
pub(crate) const CHRONO_WARP_SUPPRESSION_FRAMES: i32 = 500;

/// Retained ParasiteClass instance fields. The owner (`+0x24`) is the entity
/// storing this value.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ParasiteState {
    /// `+0x28` Victim (FootClass).
    victim: Option<u64>,
    /// `+0x2C` suppression timer. Armed by third-party damage and forced
    /// releases; a release while it runs deletes the owner with its host.
    suppression: CdTimer,
    /// `+0x38` damage-delivery timer (bite cadence).
    damage: CdTimer,
    /// Parasite+44..54: retained animation and manager-controlled progress.
    /// Load6295B0 preserves these while restarting only the two timers.
    grapple: grapple::GrappleState,
}

impl std::hash::Hash for ParasiteState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.victim.hash(state);
        self.suppression.hash(state);
        self.damage.hash(state);
        // Preserve the established non-grapple hash while including every
        // nondefault field named by native ComputeCRC6294D0.
        if self.grapple != Default::default() {
            b"parasite-grapple-v371".hash(state);
            self.grapple.hash(state);
        }
    }
}

impl ParasiteState {
    /// `ParasiteClass(owner) @ 0x006292B0`: both timers start at the current
    /// frame with zero duration (already expired); no victim.
    fn constructed(frame: u32) -> Self {
        Self {
            victim: None,
            suppression: CdTimer::started(frame as i32, 0),
            damage: CdTimer::started(frame as i32, 0),
            grapple: Default::default(),
        }
    }

    /// `+0x28` Victim, which the owner's own `Can_Enter_Cell` also reads
    /// (Unit `0x0073F530`, Infantry `0x0051C259`).
    pub(crate) fn victim(&self) -> Option<u64> {
        self.victim
    }

    pub(crate) fn grapple_anim(&self) -> Option<u64> {
        self.grapple.anim
    }

    #[cfg(test)]
    pub(crate) fn suppression(&self) -> &CdTimer {
        &self.suppression
    }

    fn suppress(&mut self, frame: u32, duration: i32) {
        self.suppression = CdTimer::started(frame as i32, duration);
    }

    /// ParasiteClass::Load `0x006295DB..0x006295F3`: both timers restart at
    /// the load frame with zero duration.
    fn restart_timers_after_load(&mut self, frame: u32) {
        self.suppression = CdTimer::started(frame as i32, 0);
        self.damage = CdTimer::started(frame as i32, 0);
    }
}

/// A Foot's parasite fields: the ParasiteClass it owns as a parasite, the
/// eater attached to it as a victim, the launch lock, its paralysis and the
/// reselect memo.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct FootParasite {
    /// Foot+69C `ParasiteImUsing`: this owner's ParasiteClass, allocated by
    /// `TechnoClass::Init_Managers @ 0x006F4145` for a weapon-0 Parasite
    /// warhead (attack dog, Terror Drone). Hashed and persisted (v193).
    class: Option<Box<ParasiteState>>,
    /// Foot+694 `ParasiteEatingMe`: the owner whose parasite is attached to
    /// this victim (AttachTo `0x0062AB2B`). Hashed and persisted (v193).
    eater: Option<u64>,
    /// Foot+698: frame before which parasite shots at this Foot are refused
    /// (`TechnoClass::Fire @ 0x006FF81F`, GetFireError `0x006FCAE1`).
    launch_lock: u32,
    /// Foot+6A0 ParalysisTimer. FootClass ctor `0x004D33FC` starts it at the
    /// construction frame with zero duration; ParasiteClass arms it (bites,
    /// release) and clears it, as does FootClass::IronCurtain `0x004DEAE0`.
    /// The Squid grapple refreshes the host timer and clears the released
    /// owner's timer (`0x0062987A`, `0x0062989E`, `0x00629CFF`).
    paralysis: CdTimer,
    /// Techno+432 ReselectIfLimboed memo (`TechnoClass::Fire @ 0x006FF79C`),
    /// consumed by a successful parasite release. Process-local like
    /// `selected`: written only for the local player's selection, so it is
    /// saved but never folded into the peer hash.
    limbo_reselect: bool,
}

impl FootParasite {
    /// The FootClass constructor's fields: paralysis started at
    /// `construction_frame` with zero duration (`0x004D33FC`).
    pub(crate) fn new(construction_frame: u32) -> Self {
        Self {
            paralysis: CdTimer::started(construction_frame as i32, 0),
            ..Self::default()
        }
    }

    /// This owner's ParasiteClass, when it has one.
    pub(crate) fn class(&self) -> Option<&ParasiteState> {
        self.class.as_deref()
    }

    /// The owner whose parasite is attached to this victim.
    pub(crate) fn eater(&self) -> Option<u64> {
        self.eater
    }

    /// The frame before which parasite shots at this Foot are refused.
    pub(crate) fn launch_lock(&self) -> u32 {
        self.launch_lock
    }

    /// The ParalysisTimer.
    pub(crate) fn paralysis(&self) -> CdTimer {
        self.paralysis
    }

    /// `FootClass::IsParalyzed @ 0x004DE770` (vtable +0x380): the Foot+6A0
    /// ParalysisTimer    /// has time left. Readers: GetFireError `0x006FC623`/`0x006FCCD5`,
    /// CloakingTick `0x006FB775`, ShouldUncloak `0x006FBCC0`, and the
    /// locomotor movement setters.
    pub(crate) fn is_paralyzed(&self, frame: u32) -> bool {
        self.paralysis.remaining(frame as i32) != 0
    }

    /// `TechnoClass::Init_Managers @ 0x006F4145`: allocate this owner's
    /// ParasiteClass ([`type_allocates_parasite`]).
    pub(crate) fn allocate_class(&mut self, frame: u32) {
        self.class = Some(Box::new(ParasiteState::constructed(frame)));
    }

    fn class_mut(&mut self) -> Option<&mut ParasiteState> {
        self.class.as_deref_mut()
    }

    /// ParalysisTimer = {Frame, 0}: a release's victim reset
    /// (`0x0062A862`) and FootClass::IronCurtain `0x004DEAE0`.
    pub(crate) fn end_paralysis(&mut self, frame: u32) {
        self.paralysis = CdTimer::started(frame as i32, 0);
    }

    /// ParasiteClass::Load `0x006295DB..0x006295F3` for this owner's class.
    pub(crate) fn restart_timers_after_load(&mut self, frame: u32) {
        if let Some(class) = self.class_mut() {
            class.restart_timers_after_load(frame);
        }
    }
}

#[cfg(test)]
impl FootParasite {
    pub(crate) fn set_eater_for_test(&mut self, eater: Option<u64>) {
        self.eater = eater;
    }

    pub(crate) fn set_paralysis_for_test(&mut self, paralysis: CdTimer) {
        self.paralysis = paralysis;
    }
}

/// Naval+Organic owners (retail `[SQD]`) run the grapple machine `0x006297F0`
/// from the class AI (`0x00629FE4..0x0062A00C`).
///
/// This predicate selects the separate grapple progression; launch,
/// relationship, admission and release remain the same shared owners.
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

/// `((raw >> 12) + 1) >> 1 & 7`: the eight-way direction of a facing word.
fn dir8(raw: u16) -> usize {
    (((u32::from(raw) >> 12) + 1) >> 1) as usize & 7
}

/// Unlimbo DirType argument `((raw >> 7) + 1) >> 1 & 0xFF`.
fn dir_type(raw: u16) -> u8 {
    ((((u32::from(raw) >> 7) + 1) >> 1) & 0xFF) as u8
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
    pub(crate) fn parasite_attach(
        &mut self,
        owner: u64,
        victim: Option<u64>,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame;
        // AttachTo629A989..629A9D4 clears old grapple progress and its
        // dedicated animation before the damage timer and CanInfect query.
        self.reset_parasite_grapple(owner, Some(rules));
        let Some(state) = self
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|o| o.parasite.class_mut())
        else {
            return;
        };
        // 0x0062A9D8: the first bite lands on the victim's next AI visit.
        state.damage = CdTimer::started(frame as i32, 0);
        if !self.parasite_can_infect(owner, victim, rules) {
            self.parasite_return_to_launch_cell(owner, rules, effects);
            return;
        }
        let victim = victim.expect("CanInfect admitted a victim");
        // 0x0062AAD9..0x0062AB24: the OWNER's locomotor Force_Track(-1, victim
        // XYZ). Drive and Ship reset their track (and return with the owner in
        // limbo); Walk binds the base no-op 0x0055AC10. SQD uses this same
        // attachment; its victim AI selects the separate grapple progression.
        let victim_coord = ground_pose::position_world_coord(
            &self.substrate.entities.get(victim).unwrap().position,
        );
        self.force_track(
            owner,
            -1,
            victim_coord,
            Some(rules),
            effects.overlay_registry(),
            effects,
        );
        self.substrate
            .entities
            .get_mut(victim)
            .unwrap()
            .parasite
            .eater = Some(owner);
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|o| o.parasite.class_mut())
        {
            state.victim = Some(victim);
        }
    }

    /// AttachTo refusal `0x0062AA02..0x0062AAD6`: Unlimbo at the centre of the
    /// owner's last map cell (Foot+55C, its launch cell), facing 0; failure
    /// deletes it. This path neither reselects nor rejoins a team.
    fn parasite_return_to_launch_cell(
        &mut self,
        owner: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(cell) = self
            .substrate
            .entities
            .get(owner)
            .map(|o| o.navigation.neighbor_state.cell())
        else {
            return;
        };
        // 0x0062AA02..0x0062AA2E: Unlimbo at the cell's GetCoords, DirType 0.
        let coords = target_cell_coord(
            cell.0 as u16,
            cell.1 as u16,
            self.resolved_terrain
                .as_ref()
                .map(NativeCellQuery::canonical)
                .as_ref(),
        );
        if !self.parasite_unlimbo_owner(owner, coords, 0, rules, effects) {
            self.uninit_with_context(
                owner,
                UninitContext::with_rules(rules).with_effects(effects),
            );
            return;
        }
        self.parasite_released_owner_orders(owner, false, rules, effects);
    }

    /// Release tail shared by AttachTo refusal, ExitUnit and PointerExpired:
    /// unless the owner is on an attack-move (HaveMegaMission, vt+0x4AC at
    /// `0x0062A763`, `0x0062A3C6`, `0x0062AA9B`), it clears its target and
    /// destination (`vt+0x480(0, 1)` at `0x0062A78A`, `0x0062A3ED`,
    /// `0x0062AAB9`); then Enter_Idle_Mode(0,1), whose RefreshMegaMission
    /// resumes such an order. Only ExitUnit (`0x0062A771`) and PointerExpired
    /// (`0x0062A3D4`) also clear the archive target; the AttachTo refusal
    /// (`0x0062AA96..0x0062AAC9`) keeps it.
    fn parasite_released_owner_orders(
        &mut self,
        owner: u64,
        clear_archive: bool,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        if !self.mega_mission(owner).is_attack_move() {
            if let Some(entity) = self.substrate.entities.get_mut(owner) {
                if clear_archive {
                    entity.set_archive_target(None);
                }
                crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
            }
            self.assign_null_destination(owner, Some(rules), effects.overlay_registry(), effects);
        }
        crate::sim::world::enter_idle_mode(self, owner, rules, effects.overlay_registry(), effects);
    }

    /// A successful ExitUnit or PointerExpired release re-adds the owner to
    /// its team to rejoin (`+0x434`, `Add_Member(owner, 0)` at `0x0062A759`
    /// and `0x0062A3BC`), which asks `Can_Add`.
    fn parasite_rejoin_team(&mut self, owner: u64, rules: &RuleSet, effects: FrameEffects<'_>) {
        if let Some(team) = self.team_script_vm.team_to_rejoin(owner) {
            self.team_add_member(
                team,
                owner,
                false,
                rules,
                effects.overlay_registry(),
                effects,
            );
        }
    }

    /// `+0x432` reselect memo, consumed only by successful releases
    /// (`0x0062A71C`, `0x0062A37F`) for the local player's house.
    fn parasite_reselect(&mut self, owner: u64) {
        let current = self.session.current_house;
        if let Some(entity) = self.substrate.entities.get_mut(owner)
            && entity.parasite.limbo_reselect
            && current.is_some_and(|house| house == entity.owner())
        {
            entity.parasite.limbo_reselect = false;
            entity.selection.select();
        }
    }

    /// ParasiteClass AI `0x00629FD0`, run at the tail of the victim's
    /// `FootClass::AI` (`0x004DAEE1..0x004DAEF3`).
    pub(crate) fn parasite_ai_for_victim(
        &mut self,
        victim: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame;
        let Some(owner) = self
            .substrate
            .entities
            .get(victim)
            .and_then(|v| v.parasite.eater)
        else {
            return;
        };
        let Some(owner_entity) = self.substrate.entities.get(owner) else {
            return;
        };
        let Some(state) = owner_entity.parasite.class() else {
            return;
        };
        let Some(victim) = state.victim else {
            return;
        };
        let Some(owner_object) = self.object_type(owner_entity.type_ref(), rules) else {
            return;
        };
        if owner_uses_grapple(owner_object) {
            self.parasite_grapple(owner, victim, rules, overlay_registry, effects);
            return;
        }
        let Some(weapon) = weapon_zero(rules, owner_object, owner_entity.veterancy.rank()) else {
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
            .class_mut()
            .unwrap()
            .damage = CdTimer::started(frame as i32, rof);
        let Some(victim_entity) = self.substrate.entities.get_mut(victim) else {
            return;
        };
        victim_entity.parasite.paralysis = CdTimer::started(frame as i32, paralyzes);
        let infantry = victim_entity.category() == EntityCategory::Infantry;
        let location = ground_pose::position_world_coord(&victim_entity.position);
        let raw = victim_entity.body_facing_current(frame);
        let (amount, ignore_defenses) = if infantry {
            // 0x0062A0AF: an Infantry victim takes its own current Health,
            // ignoring defenses: one bite kills unless ReceiveDamage refuses.
            (victim_entity.health.current(), true)
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
            //62A173..62A203: one ranged side after the real constructors,
            // then a two-lepton impact offset. The wrapping first quarter
            // turn precedes MOVSX; the shared radians owner subtracts the
            // second0x3FFF after sign extension. Truncate Y before X.
            use crate::util::native_x87::{NativeF32Bits, X87Chop53 as X};
            let lateral = if self.scenario_rng.next_range_i32_inclusive(0, 1) == 0 {
                2
            } else {
                -2
            };
            let angle = X::load_f64(crate::sim::projectile::native_math::radians(
                raw.wrapping_sub(0x3fff),
            ))
            .expect("finite parasite bite facing");
            let trig = crate::map::retail_trig::TrigTable::embedded();
            let y = X::ftol_i32_low_masked(X::sub(
                X::load_i32(location.y),
                X::mul(trig.sin_from_table(angle), X::load_i32(lateral)),
            ));
            let x = X::ftol_i32_low_masked(X::add(
                X::mul(trig.cos_from_table(angle), X::load_i32(lateral)),
                X::load_i32(location.x),
            ));
            //62A21C: the victim's actual ApplyRocker70B280. Its single
            // owner retains loaded-model admission, Weight and rate stores.
            let request = self.substrate.entities.get(victim).and_then(|entity| {
                let handle = self.interner.type_handle(entity.type_ref(), rules)?;
                let object = rules.object_by_handle(handle);
                Some(crate::sim::rocking::RockerImpulse {
                    position: [location.x, location.y, location.z],
                    origin: [x, y, location.z],
                    body_facing: raw,
                    force: NativeF32Bits::from_bits(0x3fc0_0000),
                    weight: object.weight,
                    no_dampen: false,
                    has_loaded_voxel: rules.object_has_loaded_voxel(handle),
                })
            });
            if let Some(request) = request {
                crate::sim::rocking::apply_rocker_impulse(
                    &mut self.substrate.entities.get_mut(victim).unwrap().rocking,
                    request,
                );
            }
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
        self.commit_direct_damage_receiver(rules, overlay_registry, event, effects);
    }

    /// ExitUnit `0x0062A4A0`: forced release from the victim.
    pub(crate) fn parasite_exit_unit(
        &mut self,
        owner: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame;
        let Some(owner_entity) = self.substrate.entities.get(owner) else {
            return;
        };
        let Some(victim) = owner_entity.parasite.class().and_then(|s| s.victim) else {
            return;
        };
        let Some(owner_object) = self.object_type(owner_entity.type_ref(), rules) else {
            return;
        };
        let naval = owner_object.naval;
        let suppressed = owner_entity
            .parasite
            .class()
            .is_some_and(|s| s.suppression.remaining(frame as i32) != 0);
        let (speed_type, movement_zone) = (owner_object.speed_type, owner_object.movement_zone);
        let rof =
            weapon_zero(rules, owner_object, owner_entity.veterancy.rank()).map_or(0, |w| w.rof);
        let Some(victim_entity) = self.substrate.entities.get(victim) else {
            return;
        };
        let raw = victim_entity.body_facing_current(frame);
        let released = if dir8(raw) <= 2 {
            raw.wrapping_add(0x3FFF)
        } else {
            raw.wrapping_sub(0x3FFF)
        };
        let victim_cell = (
            victim_entity.position.rx() as i16,
            victim_entity.position.ry() as i16,
        );
        if !naval && suppressed {
            // 0x0062A89B: the owner dies with its host.
            self.substrate
                .entities
                .get_mut(owner)
                .unwrap()
                .health
                .set(0);
            self.detach_parasite_victim(owner, victim, frame);
            self.uninit_with_context(
                owner,
                UninitContext::with_rules(rules).with_effects(effects),
            );
            return;
        }
        let requested = if naval {
            let (dx, dy) = CELL_DELTAS[dir8(released)];
            (
                victim_cell.0.wrapping_add(dx as i16),
                victim_cell.1.wrapping_add(dy as i16),
            )
        } else {
            victim_cell
        };
        let cell = if self.cell_clear_for(requested, speed_type, movement_zone, rules) {
            Some(requested)
        } else {
            self.techno_nearby_location(victim, None, rules)
        };
        // 0x0062A5E8..0x0062A61F: Unlimbo at the chosen cell's GetCoords.
        let placed = cell.is_some_and(|cell| {
            let coords = target_cell_coord(
                cell.0 as u16,
                cell.1 as u16,
                self.resolved_terrain
                    .as_ref()
                    .map(NativeCellQuery::canonical)
                    .as_ref(),
            );
            self.parasite_can_place_at_victim(owner, victim, rules)
                && self.parasite_unlimbo_owner(owner, coords, dir_type(released), rules, effects)
        });
        if placed {
            self.parasite_reselect(owner);
            self.parasite_rejoin_team(owner, rules, effects);
            self.parasite_released_owner_orders(owner, true, rules, effects);
            if let Some(entity) = self.substrate.entities.get_mut(owner) {
                entity.parasite.paralysis = CdTimer::started(frame as i32, rof.wrapping_mul(3));
            }
            // ExitUnit62A7E0..62A84B, before clearing the victim fields.
            self.reset_parasite_grapple(owner, Some(rules));
        } else {
            self.substrate
                .entities
                .get_mut(owner)
                .unwrap()
                .health
                .set(0);
            self.uninit_with_context(
                owner,
                UninitContext::with_rules(rules).with_effects(effects),
            );
        }
        self.detach_parasite_victim(owner, victim, frame);
    }

    /// Victim-side reset shared by every release: ParalysisTimer {Frame, 0},
    /// Foot+694 = 0, Parasite.Victim = 0 (`0x0062A862..0x0062A88F`).
    fn detach_parasite_victim(&mut self, owner: u64, victim: u64, frame: u32) {
        if let Some(entity) = self.substrate.entities.get_mut(victim) {
            if let Some(rocking) = entity.rocking.as_mut() {
                rocking.set_angle_sideways(crate::util::fixed_math::SimFixed::ZERO);
            }
            entity.parasite.end_paralysis(frame);
            if entity.parasite.eater == Some(owner) {
                entity.parasite.eater = None;
            }
        }
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|o| o.parasite.class_mut())
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
        effects: FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame;
        let Some(state) = self
            .substrate
            .entities
            .get(owner)
            .and_then(|o| o.parasite.class())
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
                entity.parasite.eater = None;
            }
            self.clear_parasite_victim(owner);
            self.uninit_with_context(
                owner,
                UninitContext::with_rules(rules).with_effects(effects),
            );
            return;
        }
        let victim_raw = self
            .substrate
            .entities
            .get(victim)
            .map_or(0, |v| v.body_facing_current(frame));
        let coords = self.parasite_release_coords(owner, victim, rules);
        // 0x0062A2F8..0x0062A379: the 0xA8E7AC bracket around Unlimbo.
        let placed = coords.is_some_and(|coords| {
            self.with_object_placement_scope(|sim| {
                sim.parasite_unlimbo_owner(owner, coords, dir_type(victim_raw), rules, effects)
            })
        });
        if placed {
            self.parasite_reselect(owner);
            self.parasite_rejoin_team(owner, rules, effects);
            self.parasite_released_owner_orders(owner, true, rules, effects);
        } else {
            self.substrate
                .entities
                .get_mut(owner)
                .unwrap()
                .health
                .set(0);
            self.uninit_with_context(
                owner,
                UninitContext::with_rules(rules).with_effects(effects),
            );
        }
        self.clear_parasite_victim(owner);
    }

    fn clear_parasite_victim(&mut self, owner: u64) {
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|o| o.parasite.class_mut())
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
            .is_some_and(|o| o.category() == EntityCategory::Unit);
        let victim_on_bridge = victim_entity.on_bridge();
        if self.parasite_can_place_at_victim(owner, victim, rules) {
            let victim_entity = self.substrate.entities.get(victim)?;
            let mut coords = ground_pose::position_world_coord(&victim_entity.position);
            if owner_is_unit {
                coords = target_cell_coord(
                    (coords.x / 256) as u16,
                    (coords.y / 256) as u16,
                    self.resolved_terrain
                        .as_ref()
                        .map(NativeCellQuery::canonical)
                        .as_ref(),
                );
                if victim_on_bridge {
                    coords.z = coords
                        .z
                        .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
                }
            }
            self.substrate
                .entities
                .get_mut(owner)?
                .on_bridge
                .store(victim_on_bridge);
            return Some(coords);
        }
        let frame = self.session.binary_frame;
        let raw = victim_entity.body_facing_current(frame);
        let victim_cell = (
            victim_entity.position.rx() as i16,
            victim_entity.position.ry() as i16,
        );
        let (dx, dy) = CELL_DELTAS[dir8(raw)];
        let adjacent = (
            victim_cell.0.wrapping_add(dx as i16),
            victim_cell.1.wrapping_add(dy as i16),
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
        // CellClass::PlaceInfantryInCell 0x00481180 (`0x0062ADCF..0x0062ADE9`)
        // on the ground plane, at the adjacent cell's GetCoords, for every
        // owner class; Unit owners then take the centre (`0x0062AE09..0x0062AE3E`).
        let mut coords = target_cell_coord(
            adjacent.0 as u16,
            adjacent.1 as u16,
            self.resolved_terrain
                .as_ref()
                .map(NativeCellQuery::canonical)
                .as_ref(),
        );
        let spot = self.place_infantry_in_ground_cell(rules, coords)?;
        if !owner_is_unit {
            let (sub_x, sub_y) = crate::util::lepton::subcell_lepton_offset(Some(spot));
            coords.x = i32::from(adjacent.0) * 256 + sub_x.to_num::<i32>();
            coords.y = i32::from(adjacent.1) * 256 + sub_y.to_num::<i32>();
        }
        let bridge = self.cell_has_bridge(adjacent);
        let on_bridge = if victim_on_bridge {
            if bridge {
                coords.z = coords
                    .z
                    .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
            }
            bridge
        } else if bridge {
            // 0x0062AE63: only a deck below the victim's own height counts.
            let victim_z = target_cell_coord(
                victim_cell.0 as u16,
                victim_cell.1 as u16,
                self.resolved_terrain
                    .as_ref()
                    .map(NativeCellQuery::canonical)
                    .as_ref(),
            )
            .z;
            if victim_z > coords.z {
                coords.z = coords
                    .z
                    .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
                true
            } else {
                false
            }
        } else {
            false
        };
        self.substrate
            .entities
            .get_mut(owner)?
            .on_bridge
            .store(on_bridge);
        Some(coords)
    }

    /// CanPlaceAtVictim `0x0062AB40`.
    fn parasite_can_place_at_victim(&self, owner: u64, victim: u64, rules: &RuleSet) -> bool {
        let Some(victim_entity) = self.substrate.entities.get(victim) else {
            return false;
        };
        // Victim vtable +0x54 (IsInAir, `0x0062AB52`).
        if crate::sim::movement::air_movement::is_high_flying(
            victim_entity,
            self.resolved_terrain.as_ref(),
            Some((rules, &self.interner)),
        ) {
            return false;
        }
        let cell = (
            victim_entity.position.rx() as i16,
            victim_entity.position.ry() as i16,
        );
        let owner_entity = self.substrate.entities.get(owner);
        let naval = owner_entity
            .and_then(|o| self.object_type(o.type_ref(), rules))
            .is_some_and(|o| o.naval);
        if !naval {
            if owner_entity.is_some_and(|o| o.category() == EntityCategory::Unit)
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
        let overlay_bridge = overlay.is_some_and(|id| {
            use crate::sim::bridge_state::{ordinary, ramp_repair::Family};
            ordinary::standing(i32::from(id), Family::Low)
                || ordinary::standing(i32::from(id), Family::High)
        });
        !(self.cell_has_bridge(bridge_reference) || overlay_bridge)
    }

    fn cell_has_bridge(&self, cell: (i16, i16)) -> bool {
        self.resolved_terrain.as_ref().is_some_and(|terrain| {
            terrain.native_cell_flags(terrain.native_cell_identity(cell)) & 0x100 != 0
        })
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
        let requested = zones.get_zone_id_native(terrain, target, movement_zone, false);
        let actual =
            zones.get_zone_id_native(terrain, target, movement_zone, self.cell_has_bridge(cell));
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
                // `0x004834A0` compares the full GetZoneID DWORDs.
                requested_zone: requested,
                actual_zone: actual.unwrap_or(u32::MAX),
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

    /// Unlimbo(coord, dir) (vt+0xD8) for a released or returning owner: the
    /// class half ([`Simulation::unlimbo_class_entry`]: InfantryClass::Unlimbo
    /// `0x0051DFF0`'s PlaceInfantryInCell `0x00481180` for an infantryman,
    /// then Object's class `Can_Enter_Cell` `0x005F4F1B..0x005F4F49` unless
    /// the caller raised `0x00A8E7AC`), then the Reveal, which snaps the body
    /// to `facing` (`0x006F6DAA`).
    fn parasite_unlimbo_owner(
        &mut self,
        owner: u64,
        coord: DriveCoord,
        facing: u8,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        use crate::sim::world::{PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest};
        let rx = (coord.x / 256) as u16;
        let ry = (coord.y / 256) as u16;
        // The stored level beside the coordinate: a deck release from
        // GetReleaseCoords (`0x0062AC60..0x0062ACF6`) stands at `level + 4`
        // with the owner's OnBridge; a cell's GetCoords is its ground.
        let ground_level = self.terrain_cell_level(rx, ry).unwrap_or(0);
        let coord_level =
            u8::try_from(coord.z.max(0) / crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS)
                .unwrap_or(u8::MAX);
        let requested = RevealPosition::new(
            rx,
            ry,
            ground_level.max(coord_level),
            Some(coord.z),
            crate::util::fixed_math::SimFixed::from_num(coord.x.rem_euclid(256)),
            crate::util::fixed_math::SimFixed::from_num(coord.y.rem_euclid(256)),
        );
        let registry = effects.overlay_registry();
        let Some((position, placement)) = self.unlimbo_class_entry(
            owner,
            requested,
            PlacementEvidence::EvaluateMark,
            Some(rules),
            registry,
        ) else {
            return false;
        };
        matches!(
            self.try_reveal_entity_with_context(
                owner,
                RevealRequest {
                    position,
                    placement,
                    logic_eligible: true,
                },
                UninitContext::new(Some(rules), registry)
                    .with_effects(effects)
                    .with_unlimbo_facing(Some(facing)),
            ),
            RevealOutcome::Revealed { .. }
        )
    }

    /// `TechnoClass::Fire @ 0x006FF749..0x006FF872`, the LimboLaunch block,
    /// after the shot's `bullet` was launched.
    pub(crate) fn parasite_limbo_launch(
        &mut self,
        firer: u64,
        target: super::TargetKind,
        weapon: &crate::rules::weapon_type::WeaponType,
        bullet: Option<u64>,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame;
        let Some(entity) = self.substrate.entities.get(firer) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let target_id = match target {
            super::TargetKind::Entity(id) => Some(id),
            super::TargetKind::Cell(..) => None,
        };
        let target_entity = target_id.and_then(|id| self.substrate.entities.get(id));
        let target_infantry =
            target_entity.is_some_and(|target| target.category() == EntityCategory::Infantry);
        // 0x006FF763..0x006FF79C: ReselectIfLimboed memo for a selected,
        // locally controlled firer jumping an Infantry target.
        let reselect = object.reselect_if_limboed
            && entity.selection.is_selected()
            && self.session.current_house == Some(entity.owner())
            && target_infantry;
        if reselect {
            self.substrate
                .entities
                .get_mut(firer)
                .unwrap()
                .parasite
                .limbo_reselect = true;
        }
        // 0x006FF7A3..0x006FF7E9: a RejoinTeamIfLimboed= Foot jumping an
        // Infantry target remembers its team (+0x434) before the Limbo below
        // takes it off the team (0x004D9744); its release re-adds it
        // (`parasite_rejoin_team`).
        if object.rejoin_team_if_limboed && target_infantry {
            self.team_script_vm.remember_team_to_rejoin(firer);
        }
        self.techno_limbo_with_context(
            firer,
            UninitContext::new(Some(rules), registry).with_effects(effects),
        );
        let parasite = weapon
            .warhead
            .as_deref()
            .and_then(|name| rules.warhead(name))
            .is_some_and(|warhead| warhead.parasite);
        // 0x006FF809..0x006FF81F: lock a Foot target for 20 frames.
        if parasite
            && let Some(target) = target_id.and_then(|id| self.substrate.entities.get_mut(id))
            && matches!(
                target.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
        {
            target.parasite.launch_lock = frame.wrapping_add(LAUNCH_LOCK_FRAMES);
        }
        // 0x006FF825..0x006FF86C: the firer's Limbo detached the bullet's
        // Owner, so the bullet is Limboed, re-Constructed (`0x004664C0`) with
        // the firer, the same target, damage, warhead, speed and bright, and
        // re-fired (vtable `+0x1F0`) from the same launch coordinate and
        // velocity. The state it rebuilds is the launch's own, except the
        // Owner and the Logic slot, which moves to the tail. An Inviso one
        // (`[SQDJUMP]`) is re-placed on its target and keeps the OnBridge
        // flag FireAt copied (`0x006FF0B0`): Limbo `0x005F4D30`, Fire and
        // Unlimbo's own body never write `+0x8C`.
        if parasite && let Some(bullet) = bullet {
            let flat = self.substrate.display.layer_of(bullet)
                == Some(crate::sim::world::display_layers::DisplayLayer::SURFACE);
            if self.unregister_non_entity_object(bullet) {
                self.projectiles.set_owner(bullet, firer);
                self.construct_bullet_scheme(bullet, rules);
                let registered = self.register_projectile(bullet, flat);
                debug_assert!(registered);
            }
        }
    }

    /// FootClass::PointerExpired `0x004D998C..0x004D99CD`, after the Techno
    /// body, for one listener.
    pub(crate) fn foot_parasite_pointer_expired(
        &mut self,
        listener: u64,
        expired: u64,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) {
        let Some(eater) = self
            .substrate
            .entities
            .get(listener)
            .and_then(|l| l.parasite.eater)
        else {
            return;
        };
        let eater_health = |sim: &Simulation, eater: u64| {
            sim.substrate
                .entities
                .get(eater)
                .map_or(0, |e| e.health.current())
        };
        // 0x004D998C..0x004D99A4: an expiring eater that is dead drops the
        // link (the scenario-ended arm has no represented state).
        if eater == expired && eater_health(self, eater) == 0 {
            self.substrate
                .entities
                .get_mut(listener)
                .unwrap()
                .parasite
                .eater = None;
        }
        // 0x004D99AA..0x004D99C6: forward to a live eater's ParasiteClass.
        if let Some(eater) = self
            .substrate
            .entities
            .get(listener)
            .and_then(|l| l.parasite.eater)
            && eater_health(self, eater) > 0
        {
            self.parasite_pointer_expired(eater, expired, rules, effects);
        }
        // 0x004D99C9: the victim itself expiring clears its own link.
        if expired == listener
            && let Some(entity) = self.substrate.entities.get_mut(listener)
        {
            entity.parasite.eater = None;
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
        effects: FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame;
        let Some(eater) = self
            .substrate
            .entities
            .get(victim)
            .and_then(|v| v.parasite.eater)
        else {
            return;
        };
        let sonic = rules
            .warhead(self.interner.resolve(warhead))
            .is_some_and(|w| w.sonic);
        if sonic {
            self.parasite_exit_unit(eater, rules, effects);
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
                .and_then(|e| e.parasite.class_mut())
        {
            state.suppress(frame, raw_damage.wrapping_mul(2).wrapping_sub(threshold));
        }
        if raw_damage < 0 {
            self.parasite_force_release(eater, FORCED_RELEASE_SUPPRESSION_FRAMES, rules, effects);
        }
    }

    /// The forced-release idiom of the repair, heal, Iron Curtain and grinder
    /// sites: arm the suppression timer, then ExitUnit.
    pub(crate) fn parasite_force_release(
        &mut self,
        eater: u64,
        suppression: i32,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame;
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(eater)
            .and_then(|e| e.parasite.class_mut())
        {
            state.suppress(frame, suppression);
        }
        self.parasite_exit_unit(eater, rules, effects);
    }
}

#[cfg(test)]
#[path = "parasite_tests.rs"]
mod tests;
