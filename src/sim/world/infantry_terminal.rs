//! An object's terminal lifetime ([`TerminalLifetime`]): the dying mark and
//! the Infantry terminal policy, separate from sprite animation progress.
//!
//! VERA-internal compatibility policy, gamemd equivalent UNCHECKED. Preserve
//! the existing effect recipe and delivery order while correcting indefinite
//! retention of effect-only corpses. Native InfantryClass::ReceiveDamage
//! (0x00517FA0) has additional selectors/particle operations not implemented here.

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::SequenceKind;
use crate::sim::movement::infantry_action::DO_PARADROP;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::FrameEffects;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum InfantryDeathSequence {
    Die1,
    Die2,
}

impl InfantryDeathSequence {
    fn for_inf_death(inf_death: u8) -> Option<Self> {
        match inf_death {
            1 => Some(Self::Die1),
            2 => Some(Self::Die2),
            _ => None,
        }
    }
    fn animation(self) -> SequenceKind {
        match self {
            Self::Die1 => SequenceKind::Die1,
            Self::Die2 => SequenceKind::Die2,
        }
    }
}

/// An object past its killing hit, or its UnInit, that is still in the
/// store. VERA-only: native has no such field. Natively the object is at
/// Health 0 running its death sequence (Infantry `Do_Action(11/12)` from
/// `InfantryClass::ReceiveDamage` `0x0051850F`/`0x005185DF`/`0x0051863F`, or
/// a Building's or Unit's death anim), or IsAlive (`+0x90`) is cleared by
/// `ObjectClass::UnInit @ 0x005F65F0` until the pending delete drains it.
///
/// Writers: the death recipes (`begin` from the receiver's non-Infantry SHP
/// death and from UnInit, `begin_infantry` from the Infantry death arm),
/// PostMortem's restore (`restore`, `0x00701F60`) and a Crash that keeps a
/// dying infantryman (`keep_crashing`, `0x005185F1`). Dying objects are
/// excluded from targeting, pathfinding and selection, and stay cell-marked
/// until their terminal UnInit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TerminalLifetime {
    dying: bool,
    /// The Infantry policy for the remaining Logic visits.
    infantry: Option<InfantryTerminal>,
}

impl TerminalLifetime {
    /// The object is dying or dead.
    pub(crate) const fn is_dying(self) -> bool {
        self.dying
    }

    /// The Infantry terminal policy, while one is retained.
    pub(crate) const fn infantry_terminal(self) -> Option<InfantryTerminal> {
        self.infantry
    }

    /// Mark the object dying: a non-Infantry SHP death sequence, or UnInit.
    pub(crate) fn begin(&mut self) {
        self.dying = true;
    }

    /// Mark an infantryman dying, with the policy for its remaining visits
    /// when it has one.
    pub(crate) fn begin_infantry(&mut self, terminal: Option<InfantryTerminal>) {
        self.dying = true;
        if terminal.is_some() {
            self.infantry = terminal;
        }
    }

    /// PostMortem's restore (`0x00701F60`): alive again at Health 1.
    pub(crate) fn restore(&mut self) {
        self.dying = false;
    }

    /// A Crash kept the dying infantryman (`0x005185F1`): alive at Health 0,
    /// falling, with no terminal policy.
    pub(crate) fn keep_crashing(&mut self) {
        self.dying = false;
        self.infantry = None;
    }

    /// UnInit consumes the Infantry policy before its pointer expiry.
    pub(crate) fn end_infantry_policy(&mut self) {
        self.infantry = None;
    }

    /// Install a fixture's dying mark.
    #[cfg(test)]
    pub(crate) fn set_dying_for_test(&mut self, dying: bool) {
        self.dying = dying;
    }

    /// Install a fixture's Infantry policy.
    #[cfg(test)]
    pub(crate) fn set_infantry_terminal_for_test(&mut self, terminal: Option<InfantryTerminal>) {
        self.infantry = terminal;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum InfantryTerminal {
    RetireNextVisit,
    Sequence(InfantryDeathSequence),
    /// From the arm's choice to its UnInit in
    /// [`InfantryDeathPostlude::commit`], or to DamageConsequences for a
    /// callback-disabled receiver fixture.
    AwaitingConsequences,
}

enum ReceiverDeathRecipe {
    Sequence,
    ExternalAnim(u8),
    Cleanup,
    /// DropAsBomb water landing (`0x005181B5..0x005182AF`): Wake at
    /// Location and SplashList[0] three leptons higher, then the common
    /// Crashable/UnInit tail (`0x005185F1`).
    WaterSplash,
    /// A `JumpJet=` type's InfantryExplode (the InfDeath=3 construction,
    /// `0x0051831D..0x0051835D`) over a crash its `FootClass::Crash`
    /// accepted: the infantryman stays, alive at Health 0.
    CrashExplode,
    /// InfDeath 9's InfantryMutate after its admission
    /// ([`Simulation::infantry_mutation_admits`]), then UnInit.
    Mutate,
}

/// Captured concrete-receiver inputs after recursive DeathWeapon damage;
/// consuming this postlude cannot forget the
/// matching lifetime decision or split effect/smudge selection between callers.
#[must_use]
pub(crate) struct InfantryDeathPostlude {
    id: u64,
    /// The Location (`+0x9C`) every anim arm builds at.
    location: crate::sim::anim_class::AnimWorldCoord,
    /// The house InfDeath 8 and 9 give their anim: the killing source's
    /// owner, else the receiver's house argument.
    house: Option<crate::sim::intern::InternedId>,
    recipe: ReceiverDeathRecipe,
}

impl InfantryDeathPostlude {
    /// The arm's anim: `AnimClass(type, &Location, 0, 1, 0x600, 0, 0)`
    /// inline (e.g. InfDeath 3 at `0x00518693`, 9 at `0x00518A8B`), the
    /// death producers' constructor ([`Simulation::emit_death_anim`]); then
    /// UnInit for an anim arm (`0x005185F1`, the `Crashable=` test, then
    /// `0x00518B9A`, or `0x0051861D` after a refused Crash) and for an
    /// InfDeath with neither anim nor sequence.
    ///
    /// The UnInit runs here, inside the receiver, as natively: the next
    /// record of the same Apply_area_damage finds this infantryman unmarked
    /// from its cell spot. A later InfDeath 9 victim in the cell can then
    /// take the spot (PlaceInfantryInCell in its admission). `uninit` is
    /// `None` only for a callback-disabled receiver fixture, which leaves the
    /// UnInit to DamageConsequences (`immediate_uninit_ids`).
    pub(crate) fn commit(
        self,
        world: &mut Simulation,
        rules: &RuleSet,
        effects: &mut crate::sim::combat::DeathEffects,
        uninit: Option<super::UninitContext<'_>>,
    ) {
        use crate::sim::combat::destruction_effects::{DeathAnimFollowUp, DeathAnimSpawn};
        if matches!(self.recipe, ReceiverDeathRecipe::WaterSplash) {
            world.emit_death_anim(
                rules,
                &mut effects.explosion_effects,
                &rules.general.wake.name,
                DeathAnimSpawn::at(self.location, 0),
            );
            if let Some(splash) = rules.combat_damage.splash_list.first() {
                world.emit_death_anim(
                    rules,
                    &mut effects.explosion_effects,
                    splash,
                    DeathAnimSpawn::at(
                        crate::sim::anim_class::AnimWorldCoord {
                            z: self.location.z.wrapping_add(3),
                            ..self.location
                        },
                        0,
                    ),
                );
            }
            // Native5185F1 tests Crashable after both constructors, even
            // when the selected anim arm preceded the JumpJet type gate.
            // A grounded object's Crash refuses; at height1..10 it can retain
            // the object instead of UnInit. Reuse FootClass's one Crash owner.
            if let Some(context) = uninit
                && world
                    .substrate
                    .entities
                    .get(self.id)
                    .and_then(|entity| world.object_type(entity.type_ref(), rules))
                    .is_some_and(|object| object.crashable)
                && world.foot_crash(self.id, None, rules, context.registry(), context.effects())
            {
                let entity = world.substrate.entities.get_mut(self.id).unwrap();
                entity.death.keep_crashing();
                return;
            }
        }
        let anim = match self.recipe {
            ReceiverDeathRecipe::ExternalAnim(inf_death) => Some((
                inf_death,
                self.house
                    .filter(|_| inf_death == VIRUS_INF_DEATH)
                    .map(DeathAnimFollowUp::Owner),
            )),
            ReceiverDeathRecipe::CrashExplode => Some((INFANTRY_EXPLODE_INF_DEATH, None)),
            ReceiverDeathRecipe::Mutate => Some((
                INFANTRY_MUTATE_INF_DEATH,
                Some(DeathAnimFollowUp::Mutate(self.house)),
            )),
            ReceiverDeathRecipe::Sequence
            | ReceiverDeathRecipe::Cleanup
            | ReceiverDeathRecipe::WaterSplash => None,
        };
        if let Some((inf_death, follow_up)) = anim
            && let Some(name) = rules.general.infantry_death_anim(inf_death)
        {
            world.emit_death_anim(
                rules,
                &mut effects.explosion_effects,
                name,
                DeathAnimSpawn {
                    follow_up,
                    ..DeathAnimSpawn::at(self.location, 0)
                },
            );
        }
        if matches!(
            self.recipe,
            ReceiverDeathRecipe::ExternalAnim(_)
                | ReceiverDeathRecipe::Mutate
                | ReceiverDeathRecipe::Cleanup
                | ReceiverDeathRecipe::WaterSplash
        ) {
            match uninit {
                Some(context) => world.uninit_with_context(self.id, context),
                None => effects.immediate_uninit_ids.push(self.id),
            }
        }
    }
}

/// The InfDeath arm whose anim is `[General] InfantryExplode=`: a `JumpJet=`
/// infantryman's death builds it whatever the warhead (`0x00518313`).
const INFANTRY_EXPLODE_INF_DEATH: u8 = 3;

/// The InfDeath arm whose anim is `[General] InfantryVirus=` and takes an
/// owner (`0x00518826..0x005188A9`).
const VIRUS_INF_DEATH: u8 = 8;

/// The InfDeath arm whose anim is `[General] InfantryMutate=`
/// (`0x005188AE..0x00518B2C`).
const INFANTRY_MUTATE_INF_DEATH: u8 = 9;

/// The InfDeath an infantryman dies with while its action is still Paradrop
/// (`+0x6C4` 0x21), whatever the warhead's (`0x0051836F..0x0051842F`): a
/// paratrooper shot in its fall, where Do_Action refuses every death
/// sequence (`0x0051D722`), explodes and is UnInit.
///
/// RESIDUAL: with an `InfDeath=8` warhead (retail `Virus` and `VirusGas`, by
/// the production reader) native first builds InfantryVirus at the Location,
/// spawns one gas particle of its `SpawnsParticle=` type there into
/// GasCloudSys (`0x0062E430`) and deletes the anim (`0x0051837E..0x0051842C`);
/// VERA ports no gas particles (`AnimClass::Middle`'s RESIDUAL). Trigger: a
/// Virus shot or gas kill on a falling paratrooper. Effect: no gas puff over
/// its explosion, and the draws of both constructions are skipped: the
/// particle's draws Scenario Random once (`0x0062B842` or `0x0062B870`) and
/// again for a type whose `+0x314` is 1 (`0x0062B7F2`). Frequency: rare.
const PARADROP_INF_DEATH: u8 = INFANTRY_EXPLODE_INF_DEATH;

/// The arm of `InfantryClass::ReceiveDamage`'s death ladder (`0x00517FA0`)
/// that takes a killed infantryman, as far as VERA ports the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfantryDeathArm {
    /// Low DropAsBomb over water, before JumpJet, Paradrop and NotHuman.
    WaterSplash(crate::sim::anim_class::AnimWorldCoord),
    /// A `JumpJet=` type (`+0xD94`, `0x00518313`): InfantryExplode whatever
    /// the warhead, then a `Crashable=` one's crash.
    JumpJetExplode { crashable: bool },
    /// A `NotHuman=` type (`+0xEAD`, `0x005184F7`): Do_Action Die1
    /// (`0x0051850F`) whatever the InfDeath.
    NotHuman,
    /// The InfDeath table (`0x00518D58`) with this InfDeath.
    Table(u8),
}

impl Simulation {
    /// The arm that takes infantryman `entity` when a warhead with
    /// `inf_death` kills it, in the ladder's order:
    /// low DropAsBomb over water; [`InfantryDeathArm::JumpJetExplode`] for a
    /// `JumpJet=` type;
    /// [`PARADROP_INF_DEATH`] replaces the InfDeath while its action is still
    /// Paradrop; [`InfantryDeathArm::NotHuman`] for a `NotHuman=` type;
    /// else the table.
    ///
    /// RESIDUAL: two rungs between the Paradrop and NotHuman ones are not
    /// ported, both dormant on retail data: a killing building whose type
    /// sets `LaserFence=` (`+0x16BF`) forces InfDeath 5
    /// (`0x00518434..0x0051845B`), and a type with `DeathAnims=` (`+0xE7C`)
    /// builds the list's InfDeath entry, else its first
    /// (`0x00518460..0x005184F2`). Retail sets neither key.
    ///
    /// RESIDUAL: two rungs ahead of the JumpJet one are not ported, dormant
    /// on retail. An ignoreDefenses kill of a `Cyborg=` (`+0xEAC`)
    /// type raises a flag (`0x0051815B`) that takes the Die1, Die2,
    /// NotHuman and refused InfDeath 9 arms on to UnInit (`0x005185E5`); a
    /// prone Cyborg builds InfantryExplode (`0x005182B4`). Retail sets no
    /// `Cyborg=`. The preceding low DropAsBomb water arm is ported here
    /// (`0x005181B5..0x005182AF`; original `tools.native_inspect` instructions).
    ///
    /// RESIDUAL: a `NotHuman=` infantryman killed with InfDeath 8 natively
    /// also builds InfantryVirus at its Location, spawns one gas particle
    /// of its `SpawnsParticle=` type there into GasCloudSys (`0x0062E430`)
    /// and deletes the anim (`0x00518515..0x005185C6`), the Paradrop arm's
    /// sequence; VERA ports no gas particles (`AnimClass::Middle`'s
    /// RESIDUAL). Trigger: a Virus or VirusGas kill of a dog or another
    /// animal. Effect: no gas puff over its Die1, and the anim's and the
    /// particle's Scenario draws are skipped (as [`PARADROP_INF_DEATH`]
    /// records). Frequency: rare.
    fn infantry_death_arm(
        &self,
        entity: &crate::sim::game_entity::GameEntity,
        inf_death: u8,
        rules: &RuleSet,
    ) -> InfantryDeathArm {
        if let Some(location) = self.dropped_bomb_water_death_location(entity.stable_id()) {
            return InfantryDeathArm::WaterSplash(location);
        }
        let (jumpjet, crashable, not_human) = self
            .object_type(entity.type_ref(), rules)
            .map_or((false, false, false), |object| {
                (object.jumpjet, object.crashable, object.not_human)
            });
        if jumpjet {
            return InfantryDeathArm::JumpJetExplode { crashable };
        }
        let paradrop = entity
            .mission_leaf
            .as_infantry()
            .is_some_and(|leaf| leaf.doing() == DO_PARADROP);
        if not_human {
            return InfantryDeathArm::NotHuman;
        }
        InfantryDeathArm::Table(if paradrop {
            PARADROP_INF_DEATH
        } else {
            inf_death
        })
    }

    /// InfDeath 9's admission (`0x005188AE..0x00518A3E`), in order:
    /// 1. UnmarkCellOccupancy (vt+0xF4 = `0x00521850`) at the Location.
    /// 2. Whether the cell under GetCoords (each axis divided by 256 toward
    ///    zero, `0x005657A0`) holds a building (WhatAmI 6) on its ground
    ///    list (`+0xE4`).
    /// 3. A cell whose land row gives Foot no speed (`Ground[+0xEC]` at
    ///    `0x0089EA40`, `FCOMP 0.0`) refuses, unless the infantryman is
    ///    OnBridge (`+0x8C`).
    /// 4. PlaceInfantryInCell (`0x00481180`) on the Location's ground plane
    ///    must find a spot (an answer other than the empty coordinate
    ///    `0x00A8F200`); then a building from step 2 refuses.
    /// 5. Admitted: MarkCellOccupancy (vt+0xF0 = `0x005217C0`) again.
    ///
    /// A refusal leaves the infantryman unmarked (`0x00518B31`).
    ///
    /// Native execution: `tools/superweapon_oracle.py` section
    /// `infantry_mutate_death`, replayed in
    /// `superweapon::genetic_converter_tests`.
    fn infantry_mutation_admits(&mut self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
        let location = crate::sim::components::DriveCoord {
            x,
            y,
            z: crate::sim::movement::ground_pose::object_world_z_leptons(
                entity,
                self.resolved_terrain.as_ref(),
            ),
        };
        let on_bridge = entity.on_bridge;
        self.object_raw_receiver_at(id, location, false);
        let (rx, ry) = ((x / 256) as u16, (y / 256) as u16);
        let building = self
            .substrate
            .occupancy
            .get(rx, ry)
            .is_some_and(|cell| cell.has_building_on(MovementLayer::Ground));
        let footless = self
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(rx, ry))
            .is_some_and(|cell| {
                !crate::sim::pathfinding::cell_entry::speed_type_allows_cell(
                    cell,
                    crate::rules::locomotor_type::SpeedType::Foot,
                )
            });
        if footless && !on_bridge {
            return false;
        }
        if self
            .place_infantry_in_ground_cell(rules, location)
            .is_none()
            || building
        {
            return false;
        }
        self.object_raw_receiver_at(id, location, true);
        true
    }

    /// Select the represented concrete recipe after recursive DeathWeapon
    /// damage. Effects remain in the consuming postlude; Do_Action owns the
    /// class sequence admission and shared Stage independently of drawing.
    ///
    /// The arm is [`Self::infantry_death_arm`]'s. A `JumpJet=` type's
    /// `Crashable=` one (`+0xD95`, `0x005185F1`) crashes after its
    /// InfantryExplode (`Crash(NULL)`, `0x0051860B`): accepted, it stays alive
    /// at Health 0 and falls (`world::jumpjet_cruise`,
    /// `movement::infantry_action`); refused on the ground, it is UnInit. An
    /// infantryman flown by the Jumpjet locomotor first runs the arm's
    /// Stop_Driver (`0x005180FE`) and Stun (`0x00518108`), which stop its
    /// locomotor with Scenario draws.
    ///
    /// RESIDUAL: native tests `Crashable=` after every anim arm, not only the
    /// JumpJet one. Trigger: a `Crashable=` type without `JumpJet=`, which
    /// retail lacks (its Crashable infantry, JUMPJET and LUNR, are JumpJet
    /// types by the production reader). Effect: it would crash instead of
    /// being UnInit.
    ///
    /// RESIDUAL: a walker skips that Stop_Driver and Stun. Trigger: every
    /// infantry death. Effect: none observable: its Walk stop and Stun repeat
    /// the death arm's, and its death sequence or removal overwrites the Doing
    /// and cell-entry byte they write.
    ///
    /// Infantry fire and direct mission FireAt now commit in the shooter's
    /// Logic visit. RESIDUAL (other firing lanes): a shot still delivered by
    /// the frame-end combat pass after every object's turn can engage the
    /// Rocketeer's crash latch one frame after native when that shooter was
    /// ahead in Logic order. The fall and landing's Scenario draws then move
    /// by a frame. Frequency follows kills through those deferred lanes.
    ///
    /// Each exact-zero callback in the fall reaches the shared immediate
    /// RecordKill702D40 owner again. Retained attribution does not suppress a
    /// later native callback or defer its House statistics until UnInit.
    ///
    /// `house` is the owner InfDeath 8 and 9 give their anim.
    pub(crate) fn begin_infantry_receiver_death(
        &mut self,
        id: u64,
        inf_death: u8,
        house: Option<crate::sim::intern::InternedId>,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: super::FrameEffects<'_>,
    ) -> InfantryDeathPostlude {
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(crate::sim::movement::infantry_action::uses_jumpjet_locomotor)
        {
            if let Err(cause) = self.infantry_stop_driver(id, rules, overlay_registry, effects) {
                log::debug!("infantry {id} death Stop_Driver: {cause}");
            }
            self.techno_death_stun(
                id,
                super::UninitContext::new(Some(rules), overlay_registry).with_effects(effects),
            );
        }
        // `InfantryClass::ReceiveDamage 0x0051810E..0x0051812E`, before the
        // death ladder: Queue_Mission(-1) (refused), Queue_Mission(Guard),
        // Commence. The corpse sits on Guard until it is removed.
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard),
            );
        }
        let now = self.session.binary_frame;
        let _ = self.mission_commence_exact(id, now);
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("fatal Infantry retained for postlude");
        debug_assert_eq!(entity.category(), EntityCategory::Infantry);
        let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
        let mut location = crate::sim::anim_class::AnimWorldCoord {
            x,
            y,
            z: crate::sim::movement::ground_pose::object_world_z_leptons(
                entity,
                self.resolved_terrain.as_ref(),
            ),
        };
        let recipe = match self.infantry_death_arm(entity, inf_death, rules) {
            InfantryDeathArm::WaterSplash(raw_location) => {
                location = raw_location;
                ReceiverDeathRecipe::WaterSplash
            }
            InfantryDeathArm::JumpJetExplode { crashable } => {
                if crashable && self.foot_crash(id, None, rules, overlay_registry, effects) {
                    ReceiverDeathRecipe::CrashExplode
                } else {
                    ReceiverDeathRecipe::ExternalAnim(INFANTRY_EXPLODE_INF_DEATH)
                }
            }
            InfantryDeathArm::NotHuman => {
                self.begin_infantry_death_sequence(id, InfantryDeathSequence::Die1, rules, effects);
                ReceiverDeathRecipe::Sequence
            }
            InfantryDeathArm::Table(INFANTRY_MUTATE_INF_DEATH) => {
                if self.infantry_mutation_admits(id, rules) {
                    ReceiverDeathRecipe::Mutate
                } else {
                    self.begin_infantry_death_sequence(
                        id,
                        InfantryDeathSequence::Die2,
                        rules,
                        effects,
                    );
                    ReceiverDeathRecipe::Sequence
                }
            }
            InfantryDeathArm::Table(inf_death) => {
                if let Some(sequence) = InfantryDeathSequence::for_inf_death(inf_death) {
                    self.begin_infantry_death_sequence(id, sequence, rules, effects);
                    ReceiverDeathRecipe::Sequence
                } else if crate::sim::animation::inf_death_spawns_anim(inf_death) {
                    ReceiverDeathRecipe::ExternalAnim(inf_death)
                } else {
                    ReceiverDeathRecipe::Cleanup
                }
            }
        };
        if !matches!(
            recipe,
            ReceiverDeathRecipe::Sequence | ReceiverDeathRecipe::CrashExplode
        ) {
            let entity = self.substrate.entities.get_mut(id).unwrap();
            entity
                .death
                .begin_infantry(Some(InfantryTerminal::AwaitingConsequences));
        }
        InfantryDeathPostlude {
            id,
            location,
            house,
            recipe,
        }
    }

    /// Complete the represented raw-kill handoff. Aircraft retirement
    /// supplies no class action. Bridge fallout uses ReceiveDamage.
    /// No ReceiveDamage effects are introduced on these compatibility paths.
    /// Returns false for other categories, whose existing lifetime stays local.
    #[cfg(test)]
    pub(crate) fn begin_raw_infantry_death(&mut self, id: u64) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if entity.category() != EntityCategory::Infantry {
            return false;
        }
        entity.health.set_for_test(0);
        // Legacy raw coordinate scans can revisit an UnInit'd object before
        // pending deletion drains it. Preserve their HP write, but never give
        // a completed lifetime another scheduled terminal action.
        let terminal = entity
            .lifecycle
            .object_alive()
            .then_some(InfantryTerminal::RetireNextVisit);
        entity.death.begin_infantry(terminal);
        true
    }

    /// Native Do_Action51D6F0 retains progress for an unchanged action. A
    /// captured receiver can write a different action after UnInit, without
    /// restoring Logic membership or canceling its pending deletion.
    pub(crate) fn begin_infantry_death_sequence(
        &mut self,
        id: u64,
        sequence: InfantryDeathSequence,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        //ReceiveDamage51850F/5185DF/51863F dispatches DoAction(11/12,1,0).
        // Its unchanged/absent-sequence refusal retains the whole Stage;
        // otherwise the one class action owner performs every restart.
        let action = i32::from(crate::rules::infantry_sequence::action_id(
            sequence.animation(),
        ));
        if let Err(cause) = self.infantry_do_action(id, action, true, rules, effects) {
            log::debug!("infantry {id} death Do_Action: {cause}");
        }
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        debug_assert_eq!(entity.category(), EntityCategory::Infantry);
        let terminal = entity
            .lifecycle
            .object_alive()
            .then_some(InfantryTerminal::Sequence(sequence));
        entity.death.begin_infantry(terminal);
    }

    /// Consume this object's terminal Logic visit, including eventual UnInit.
    /// The ordinary sequencer owns every completed class action, including
    /// corpse creation and UnInit; this host never advances a second clock.
    ///
    /// A Die1/Die2 corpse still runs its Techno AI subset each visit (see
    /// `techno_ai::dying_infantry_techno_ai`); when its sequence completes,
    /// `InfantryClass 0x00520BC6` leaves a corpse anim, then UnInit.
    pub(super) fn visit_infantry_terminal(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        ctx: super::techno_ai::ObjectAiCtx<'_>,
    ) -> super::techno_ai::ObjectAiOutcome {
        let Some(entity) = self.substrate.entities.get(id) else {
            return Default::default();
        };
        let Some(terminal) = entity.death.infantry_terminal() else {
            return Default::default();
        };
        debug_assert!(entity.death.is_dying() && entity.category() == EntityCategory::Infantry);
        let mut outcome = super::techno_ai::ObjectAiOutcome::visited();
        match terminal {
            InfantryTerminal::AwaitingConsequences => return outcome,
            InfantryTerminal::Sequence(_) => {
                let Some(rules) = rules else {
                    return outcome;
                };
                super::techno_ai::dying_infantry_techno_ai(self, id, rules, ctx);
                match self.infantry_action_turn(id, rules, ctx.overlay_registry, ctx.effects) {
                    Ok(changed) => outcome.bridge_state_changed = changed,
                    Err(cause) => log::debug!("infantry {id} terminal action turn: {cause}"),
                }
                return outcome;
            }
            InfantryTerminal::RetireNextVisit => {}
        }
        if let Some(rules) = rules {
            self.uninit_with_context(
                id,
                super::UninitContext::new(Some(rules), ctx.overlay_registry)
                    .with_effects(ctx.effects),
            );
        } else {
            self.uninit(id);
        }
        outcome
    }

    /// `InfantryClass 0x00520BC6..0x00520CA4`, a Die1..Die5 sequence's
    /// completion: the type's `DeadBodies=`, else `[General] DeadBodies=`
    /// unless the type is `NotHuman=`, one Scenario `Random() % count`
    /// (`0x00520C11`/`0x00520C6B`), and `AnimClass(body, GetCoords, 0, 1,
    /// 0x600, 0, 0)` (`0x00520CA4`), before the UnInit. Returns the corpse
    /// anim it built.
    ///
    /// Native execution: `tools/spatial_oracle/infantry_death_completion.py`
    /// ([`dead_body_tests`]).
    pub(crate) fn leave_dead_body(
        &mut self,
        id: u64,
        rules: &RuleSet,
    ) -> Option<crate::sim::intern::InternedId> {
        let entity = self.substrate.entities.get(id)?;
        let object = self.object_type(entity.type_ref(), rules)?;
        let bodies = if !object.dead_bodies.is_empty() {
            &object.dead_bodies
        } else if !object.not_human {
            &rules.general.dead_bodies
        } else {
            return None;
        };
        if bodies.is_empty() {
            return None;
        }
        let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let body = self.pick_death_anim(bodies).to_string();
        let body = self.interner.intern(&body);
        self.admit_death_anim(
            rules,
            body,
            crate::sim::combat::destruction_effects::DeathAnimSpawn::at(
                crate::sim::anim_class::AnimWorldCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                },
                0,
            ),
        );
        Some(body)
    }
}

#[cfg(test)]
mod dead_body_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use serde_json::Value;

    /// Every `infantry_death_completion.json` Die1..Die5 row that completes
    /// (stage at or past a nonzero count) replays through
    /// `Simulation::leave_dead_body` from the same Scenario seed: the corpse it
    /// picks (the type's list, else Rules', none for NotHuman) and the RNG
    /// state after.
    #[test]
    fn leave_dead_body_matches_the_original() {
        let rows: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/infantry_death_completion.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 68);
        let mut compared = 0;
        for row in &rows {
            let input = &row["input"];
            let int = |key: &str| input[key].as_i64().unwrap();
            if !(0xB..=0xF).contains(&int("doing"))
                || int("count") <= 0
                || int("stage") < int("count")
                || int("alloc") == 0
            {
                continue;
            }
            let type_bodies: Vec<String> = (0..int("type_bodies"))
                .map(|n| format!("TBODY{n}"))
                .collect();
            let ini = format!(
                "[General]\nDeadBodies=RBODY0,RBODY1,RBODY2,RBODY3,RBODY4,RBODY5\n\
                 [InfantryTypes]\n0=DOOMED\n[DOOMED]\nStrength=100\nNotHuman={}\n{}",
                if int("not_human") != 0 { "yes" } else { "no" },
                if type_bodies.is_empty() {
                    String::new()
                } else {
                    format!("DeadBodies={}\n", type_bodies.join(","))
                },
            );
            let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
            let mut sim = Simulation::with_seed(int("seed") as u64);
            let mut entity =
                crate::sim::game_entity::GameEntity::test_default(1, "DOOMED", "Americans", 5, 5);
            entity.set_type_ref_for_test(sim.interner.intern("DOOMED"));
            entity.set_category_for_test(EntityCategory::Infantry);
            sim.substrate.entities.insert(entity);
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_before"].as_str().unwrap()
            );

            let picked = sim
                .leave_dead_body(1, &rules)
                .map(|body| sim.interner.resolve(body).to_string());
            let native = row["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["call"] == "anim")
                .map(|event| {
                    let anim = event["anim"].as_i64().unwrap();
                    assert_eq!(event["rest"], serde_json::json!([0, 1, 0x600, 0, 0]));
                    if anim >= 0x2F00 {
                        format!("TBODY{}", anim - 0x2F00)
                    } else {
                        format!("RBODY{}", anim - 0x2E00)
                    }
                });
            assert_eq!(picked, native, "{input}");
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "{input}"
            );
            compared += 1;
        }
        assert_eq!(compared, 61);
    }
}
