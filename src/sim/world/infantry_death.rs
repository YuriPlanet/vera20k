//! An infantryman's death: the arm of `InfantryClass::ReceiveDamage`'s death
//! ladder (`0x00517FA0`) that takes him, its effects
//! ([`InfantryDeathPostlude`]), and the dead body his death sequence leaves
//! ([`Simulation::leave_dead_body`]).
//!
//! The Die1/Die2 arms, NotHuman and a refused InfDeath 9 call
//! `Do_Action(11/12, 1, 0)` and return at `0x005185E5` without UnInit. The
//! corpse keeps IsAlive (`+0x90`) at Health 0 and has no state of its own:
//! `InfantryClass::AI` (`0x0051BAB0`) visits it like any infantryman, the
//! Health reset (`0x0051BC57`) and the mission handlers (`MissionClass::AI`,
//! `0x005B30A7`) passing it by, until `DoType_Sequencer` (`0x00520AE0`)
//! leaves its body (`0x00520CA4`) and UnInits it (`0x00520CAD`). The other
//! arms build their anim and UnInit him at once (`0x00518B9A`).
//!
//! RNG on a corpse's visits: the Iron Curtain or airstrike tint draw on the
//! Scenario stream (`0x006F9EAF`), and PlaceInfantryInCell's draw (`0x00481180`)
//! in the target-reach step of a CloseRange=, Teleport or Jumpjet corpse
//! holding a re-acquired target. Both are the ordinary visit's own draws.
//!
//! Native InfantryClass::ReceiveDamage has additional selectors/particle
//! operations not implemented here.

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::SequenceKind;
use crate::sim::intern::StringInterner;
use crate::sim::movement::infantry_action::DO_PARADROP;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding::PathGrid;
use crate::sim::rng::SimRng;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectSubstrate;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum InfantryDeathSequence {
    Die1,
    Die2,
}

impl InfantryDeathSequence {
    /// The sequence arms of the InfDeath table (`0x00518D58`, dispatched on
    /// `InfDeath - 1` over 0..9 unsigned at `0x005185C8`): 1 plays Die1 and
    /// 2 Die2 (`Do_Action(11/12, 1, 0)`, `0x005185DF`, `0x0051863F`), with
    /// no anim. The arms 3..=10 build an anim and play no sequence
    /// ([`inf_death_spawns_anim`]); any other value, the warhead default 0
    /// included, takes neither (`0x005185CC` is an unsigned `ja`). No arm
    /// reaches Die3..Die5 (DoType `0x0D`..`0x0F`), and the selection draws no
    /// RNG: `Do_Action`'s randomStart, which gates the frame-start draw, is 0.
    pub(crate) fn for_inf_death(inf_death: i32) -> Option<Self> {
        match inf_death {
            1 => Some(Self::Die1),
            2 => Some(Self::Die2),
            _ => None,
        }
    }
    pub(crate) fn animation(self) -> SequenceKind {
        match self {
            Self::Die1 => SequenceKind::Die1,
            Self::Die2 => SequenceKind::Die2,
        }
    }
}

/// Whether the InfDeath table's arm (`0x00518D58`) for this InfDeath builds
/// an anim (3..=10) instead of playing a sequence; exclusive with
/// [`InfantryDeathSequence::for_inf_death`].
fn inf_death_spawns_anim(inf_death: i32) -> bool {
    (3..=10).contains(&inf_death)
}

enum ReceiverDeathRecipe {
    Sequence,
    ExternalAnim(i32),
    /// An InfDeath off the table (`0x005185CC`), the warhead default 0
    /// included: neither sequence nor anim, then UnInit.
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
pub struct InfantryDeathPostlude {
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
            // the object instead of UnInit, alive at Health 0 and falling.
            // Reuse FootClass's one Crash owner.
            if let Some(context) = uninit
                && world
                    .substrate
                    .entities
                    .get(self.id)
                    .and_then(|entity| world.object_type(entity.type_ref(), rules))
                    .is_some_and(|object| object.crashable)
                && world.foot_crash(self.id, None, rules, context.registry(), context.effects())
            {
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
const INFANTRY_EXPLODE_INF_DEATH: i32 = 3;

/// The InfDeath arm whose anim is `[General] InfantryVirus=` and takes an
/// owner (`0x00518826..0x005188A9`).
const VIRUS_INF_DEATH: i32 = 8;

/// The InfDeath arm whose anim is `[General] InfantryMutate=`
/// (`0x005188AE..0x00518B2C`).
const INFANTRY_MUTATE_INF_DEATH: i32 = 9;

/// The InfDeath an infantryman dies with while its action is still Paradrop
/// (`+0x6C4` 0x21), whatever the warhead's (`0x0051836F..0x0051842F`): a
/// paratrooper shot in its fall, where Do_Action refuses every death
/// sequence (`0x0051D722`), explodes and is UnInit.
///
/// InfDeath8 first emits the same temporary-anim single particle as the
/// animal branch (51837E..51842C), before the ordinary explosion.
const PARADROP_INF_DEATH: i32 = INFANTRY_EXPLODE_INF_DEATH;

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
    Table(i32),
}

impl Simulation {
    /// Infantry51851E..5185C6 (also Paradrop51837E): construct InfantryVirus,
    /// spawn its explicit SpawnsParticle once, then scalar-delete the temporary
    /// animation. This is distinct from the human death animation's Middle.
    fn emit_inline_virus_particle(
        &mut self,
        rules: &RuleSet,
        location: crate::sim::anim_class::AnimWorldCoord,
    ) {
        let Some(name) = rules.general.infantry_death_anim(VIRUS_INF_DEATH) else {
            return;
        };
        let type_id = self.interner.intern(name);
        let Some(anim) = self.admit_death_anim(
            rules,
            type_id,
            crate::sim::combat::destruction_effects::DeathAnimSpawn::at(location, 0),
        ) else {
            return;
        };
        if let Some(kind) = self
            .anim(anim)
            .and_then(|anim| {
                rules
                    .art()
                    .anim_runtime_config(self.interner.resolve(anim.type_id()))
            })
            .and_then(|config| config.spawns_particle.as_deref())
            .and_then(|name| rules.p_type_id_by_name(name))
        {
            self.spawn_gas_particle(
                kind,
                glam::IVec3::new(location.x, location.y, location.z),
                rules,
            );
        }
        self.scalar_delete_anim(anim);
    }

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
    fn infantry_death_arm(
        &self,
        entity: &crate::sim::game_entity::GameEntity,
        inf_death: i32,
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
    fn infantry_mutation_admits(
        interner: &StringInterner,
        scenario_rng: &mut SimRng,
        substrate: &mut ObjectSubstrate,
        path_grid: &Option<Arc<PathGrid>>,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        id: u64,
        rules: &RuleSet,
    ) -> bool {
        let Some(entity) = substrate.entities.get(id) else {
            return false;
        };
        let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
        let location = crate::sim::components::DriveCoord {
            x,
            y,
            z: crate::sim::movement::ground_pose::object_world_z_leptons(
                entity,
                resolved_terrain.as_ref(),
            ),
        };
        let on_bridge = entity.on_bridge();
        Self::object_raw_receiver_at(substrate, path_grid, resolved_terrain, id, location, false);
        let (rx, ry) = ((x / 256) as u16, (y / 256) as u16);
        let building = substrate
            .occupancy
            .get(rx, ry)
            .is_some_and(|cell| cell.has_building_on(MovementLayer::Ground));
        let footless = resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(rx, ry))
            .is_some_and(|cell| {
                // The land row (`0x0089EA40`) gives Foot no speed; an
                // unbound row admits.
                cell.speed_costs
                    .cost_for_speed_type(crate::rules::locomotor_type::SpeedType::Foot)
                    == Some(0)
            });
        if footless && !on_bridge {
            return false;
        }
        if Self::place_infantry_in_ground_cell(
            interner,
            scenario_rng,
            substrate,
            resolved_terrain,
            rules,
            location,
        )
        .is_none()
            || building
        {
            return false;
        }
        Self::object_raw_receiver_at(substrate, path_grid, resolved_terrain, id, location, true);
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
    /// RESIDUAL: native tests `Crashable=` (`0x005185F1`) after every anim arm
    /// and after an InfDeath off the table (the warhead default 0 included),
    /// not only after the JumpJet one. Trigger: a `Crashable=` type without
    /// `JumpJet=`, which retail lacks (its Crashable infantry, JUMPJET and
    /// LUNR, are JumpJet types by the production reader). Effect: it would
    /// crash instead of being UnInit.
    ///
    /// RESIDUAL: a walker skips that Stop_Driver and Stun. Trigger: every
    /// infantry death. Effect: none observable: its Walk stop and Stun repeat
    /// the death arm's, and its death sequence or removal overwrites the Doing
    /// and cell-entry byte they write.
    ///
    /// Each exact-zero callback in the fall reaches the shared immediate
    /// RecordKill702D40 owner again. Retained attribution does not suppress a
    /// later native callback or defer its House statistics until UnInit.
    ///
    /// `house` is the owner InfDeath 8 and 9 give their anim.
    pub(crate) fn begin_infantry_receiver_death(
        &mut self,
        id: u64,
        inf_death: i32,
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
            self.stun(
                id,
                super::UninitContext::new(rules, overlay_registry).with_effects(effects),
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
        let now = self.session.binary_frame();
        let _ = self.substrate.mission_commence_exact(id, now);
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
        let arm = self.infantry_death_arm(entity, inf_death, rules);
        if inf_death == VIRUS_INF_DEATH
            && matches!(arm, InfantryDeathArm::Table(PARADROP_INF_DEATH))
        {
            self.emit_inline_virus_particle(rules, location);
        }
        let recipe = match arm {
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
                if inf_death == VIRUS_INF_DEATH {
                    self.emit_inline_virus_particle(rules, location);
                }
                ReceiverDeathRecipe::Sequence
            }
            InfantryDeathArm::Table(INFANTRY_MUTATE_INF_DEATH) => {
                if Self::infantry_mutation_admits(
                    &self.interner,
                    &mut self.scenario_rng,
                    &mut self.substrate,
                    &self.path_grid,
                    &self.resolved_terrain,
                    id,
                    rules,
                ) {
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
                } else if inf_death_spawns_anim(inf_death) {
                    ReceiverDeathRecipe::ExternalAnim(inf_death)
                } else {
                    ReceiverDeathRecipe::Cleanup
                }
            }
        };
        InfantryDeathPostlude {
            id,
            location,
            house,
            recipe,
        }
    }

    /// The Die1/Die2 arms' `Do_Action(11/12, 1, 0)` (`0x0051850F`,
    /// `0x005185DF`, `0x0051863F`), whose answer the arm ignores
    /// (`0x005185E5`). Accepted, it leaves the corpse its ordinary visit runs
    /// to completion (module docs). Refused, an unchanged action keeps its
    /// whole Stage, and a sequence with no frames (`0x0051D70F`) keeps the
    /// previous action: the next visit's Health reset (`0x0051BC57`) then
    /// raises the infantryman to Health 1, alive.
    ///
    /// A captured receiver can write a different action after UnInit,
    /// without restoring Logic membership or canceling its pending deletion.
    pub fn begin_infantry_death_sequence(
        &mut self,
        id: u64,
        sequence: InfantryDeathSequence,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let action = i32::from(crate::rules::infantry_sequence::action_id(
            sequence.animation(),
        ));
        if let Err(cause) = self.infantry_do_action(id, action, true, rules, effects) {
            log::debug!("infantry {id} death Do_Action: {cause}");
        }
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
        let body = crate::sim::combat::destruction_effects::pick_death_anim(
            &mut self.scenario_rng,
            bodies,
        )
        .to_string();
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
mod corpse_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// A GI-drawn E1 on the flat arena, with RadSite radiation (InfDeath 7,
    /// the `[General] InfantryNuked=` NUKEDIE anim).
    fn corpse_scene() -> (Simulation, RuleSet, u64) {
        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [E1]\nImage=GI\nStrength=100\nSpeed=4\n\
             [Warheads]\n0=RadSite\n\
             [RadSite]\nVerses=100%,100%,100%,50%,10%,10%,0%,0%,0%,100%,100%\n\
             InfDeath=7\nRadiation=yes\n\
             [Radiation]\nRadDurationMultiple=1\nRadApplicationDelay=16\nRadLevelMax=500\n\
             RadLevelDelay=90\nRadLevelFactor=.2\nRadSiteWarhead=RadSite\n\
             [General]\nInfantryNuked=NUKEDIE\n",
        );
        let art = IniFile::from_str(&format!(
            "{}\n[NUKEDIE]\nEnd=20\n",
            crate::rules::retail_ini_fixture::GI_ART_EXCERPT
        ));
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
        registry.bind_anim_frame_count_for_test("NUKEDIE", 20);
        rules.install_art_data(registry);
        rules.bind_animation_sequences(
            &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
        );
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let id = sim
            .spawn_object("E1", "Americans", 5, 5, 0, &rules)
            .unwrap();
        (sim, rules, id)
    }

    /// The killing hit's Die1 arm: Health 0, then `Do_Action(11, 1, 0)`.
    fn kill_with_die1(sim: &mut Simulation, rules: &RuleSet, id: u64) {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.health.set_for_test(0);
        sim.begin_infantry_death_sequence(
            id,
            InfantryDeathSequence::Die1,
            rules,
            FrameEffects::default(),
        );
    }

    /// FootClass::AI's radiation (`0x004DA554..0x004DA635`) has no Health
    /// gate, and TechnoClass::ReceiveDamage takes a Health-0 receiver to its
    /// death branch again (`0x0070202E`): a Die1 corpse standing in
    /// radiation dies anew at the next application, by RadSite's InfDeath 7,
    /// whose arm builds NUKEDIE and UnInits it before its sequence ends.
    #[test]
    fn a_corpse_in_radiation_dies_again_at_the_next_application() {
        let (mut sim, rules, id) = corpse_scene();
        sim.session.set_binary_frame_for_test(10);
        kill_with_die1(&mut sim, &rules, id);
        sim.radiation.apply_detonation(
            crate::sim::radiation::RadDetonation {
                rx: 5,
                ry: 5,
                rad_level: 500,
                spread: 2,
                rad_type: None,
                firing_house: None,
                invoker: None,
            },
            10,
            &sim.session.theater(),
            &rules,
            sim.resolved_terrain.as_ref(),
            &mut sim.lighting_sources,
        );
        let nukedie = |sim: &Simulation| {
            sim.anims()
                .filter(|(_, anim)| sim.interner.resolve(anim.type_id()) == "NUKEDIE")
                .count()
        };
        for frame in 10..16 {
            sim.session.set_binary_frame_for_test(frame);
            sim.advance_tick(&[], Some(&rules), None, None, 100);
            assert!(
                sim.substrate
                    .entities
                    .get(id)
                    .is_some_and(|corpse| corpse.lifecycle.object_alive()),
                "frame {frame}: no application yet"
            );
        }
        assert_eq!(nukedie(&sim), 0);
        sim.session.set_binary_frame_for_test(16);
        sim.advance_tick(&[], Some(&rules), None, None, 100);
        assert!(
            sim.substrate
                .entities
                .get(id)
                .is_none_or(|corpse| !corpse.lifecycle.object_alive())
        );
        assert_eq!(nukedie(&sim), 1);
    }

    /// The InfDeath table (`0x00518D58`) dispatches on `InfDeath - 1` over
    /// 0..9 unsigned: 0, negatives and values above 10 take neither a
    /// sequence nor an anim, 1 and 2 are its only sequence arms, and 3..=10
    /// build an anim; no arm reaches Die3..Die5.
    #[test]
    fn inf_death_table_arms_are_exclusive() {
        let sequence = |inf_death| {
            InfantryDeathSequence::for_inf_death(inf_death).map(InfantryDeathSequence::animation)
        };
        for off_table in [0, -1, 11] {
            assert_eq!(sequence(off_table), None);
            assert!(!inf_death_spawns_anim(off_table));
        }
        assert_eq!(sequence(1), Some(SequenceKind::Die1));
        assert_eq!(sequence(2), Some(SequenceKind::Die2));
        assert!(!inf_death_spawns_anim(1));
        assert!(!inf_death_spawns_anim(2));
        for inf_death in 3..=10 {
            assert_eq!(sequence(inf_death), None);
            assert!(inf_death_spawns_anim(inf_death));
        }
    }

    /// Retail `[CometWH]` (the Prism Tank's beam and its shrapnel fragments)
    /// and `[GrandCannonWH]` name no `InfDeath=`, so the production reader
    /// keeps the WarheadTypeClass constructor's 0 (`0x0075CF14`). Killed by
    /// either through the production receiver, an E1 takes no table arm
    /// (`0x005185C8`: `InfDeath - 1` is above 9 unsigned) and is UnInit inside
    /// the killing call (`0x005185F1` -> `0x00518B9A`): no death sequence, no
    /// anim, no corpse. `[SA]`'s `InfDeath=1` still leaves the Die1 corpse.
    #[test]
    fn retail_warheads_without_inf_death_remove_the_infantryman_at_once() {
        let Some(mut rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
            return;
        };
        rules.bind_animation_sequences(
            &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
        );
        for (warhead, inf_death) in [("CometWH", 0), ("GrandCannonWH", 0), ("SA", 1)] {
            assert_eq!(
                rules.warhead(warhead).unwrap().inf_death,
                inf_death,
                "{warhead}"
            );
            let mut sim = Simulation::new();
            crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
            let id = sim
                .spawn_object("E1", "Americans", 5, 5, 0, &rules)
                .unwrap();
            let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
                id,
                100_000,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                sim.interner.intern(warhead),
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: false,
                    arg6: false,
                },
            );
            sim.commit_noncombat_aoe_hits(&rules, None, &[hit], FrameEffects::default());
            let entity = sim.substrate.entities.get(id).unwrap();
            assert_eq!(entity.health.current(), 0, "{warhead}");
            assert_eq!(sim.anims().count(), 0, "{warhead}: no InfDeath anim");
            let doing = entity.mission_leaf.as_infantry().unwrap().doing();
            if inf_death == 0 {
                assert!(!entity.lifecycle.object_alive(), "{warhead}: UnInit");
                assert!(sim.substrate.pending_delete.contains_object(&id));
                assert_ne!(doing, 11, "{warhead}: no Die1");
            } else {
                assert!(entity.lifecycle.object_alive(), "the Die1 corpse stays");
                assert!(!sim.substrate.pending_delete.contains_object(&id));
                assert_eq!(doing, 11);
            }
        }
    }

    /// The zero-count refusal (`0x0051D70F`) that leaves a killed
    /// infantryman to the Health reset (`0x0051BC57`) meets one retail type:
    /// every other `[InfantryTypes]` entry has Die1 and Die2 frames once
    /// `artmd.ini`'s sequences are bound. WEEDGUY has no ART and
    /// `TechLevel=-1` (rulesmd.ini keeps it to instantiate weapons), so only
    /// a map that places one can kill it into that revival.
    #[test]
    fn retail_infantry_without_die_frames_are_the_weedguy() {
        let Some(mut rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
            return;
        };
        rules.bind_animation_sequences(
            &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
        );
        assert!(!rules.infantry_ids.is_empty());
        let without: Vec<&String> = rules
            .infantry_ids
            .iter()
            .filter(|id| {
                [11, 12].into_iter().any(|action| {
                    rules
                        .animation_sequence(id)
                        .and_then(|set| set.infantry_action(action))
                        .is_none_or(|record| record.frames_per_facing == 0)
                })
            })
            .collect();
        assert_eq!(without, ["WEEDGUY"]);
        assert_eq!(rules.object("WEEDGUY").unwrap().tech_level, -1);
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
