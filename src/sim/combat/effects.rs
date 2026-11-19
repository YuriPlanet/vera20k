//! The ordered accumulator weapon emission and the receivers fill
//! ([`DeathEffects`]), and the records it carries to the world.

use super::*;
use crate::sim::projectile::ProjectileCoord;

/// Explosion animation to spawn at a world position (deferred to caller
/// which has access to `Simulation` for AnimClass construction).
pub struct ExplosionEffect {
    pub shp_name: InternedId,
    pub rx: u16,
    pub ry: u16,
    /// Sub-cell impact X in leptons. Preserves the CoordStruct-level impact
    /// point for warhead AnimList placement.
    pub sub_x: SimFixed,
    /// Sub-cell impact Y in leptons.
    pub sub_y: SimFixed,
    pub z: u8,
    /// Exact absolute Z in leptons; the anim is constructed there (its Middle
    /// height gate reads it). `z` is the level byte of the same point.
    pub world_z: i32,
    /// `AnimClass +0x100` draw Z adjust passed to the constructor. The stock
    /// combat row uses [`crate::sim::anim_class::COMBAT_EXPLOSION_Z_ADJUST`];
    /// the Phobos Bullet tail substitutes the warhead's `AnimZAdjust`.
    pub z_adjust: i32,
    /// Phobos' `0x469C46` tail assigns the invoker House to every animation it
    /// constructs (`pAnim->Owner = pInvoker`). `None` keeps the constructor's
    /// unowned state.
    pub owner_house: Option<InternedId>,
    /// A death producer's own constructor call (`Death_Explosion`, the
    /// Aircraft death arm, `DestructionEffects`, the infantry InfDeath arms):
    /// `AnimClass(type, coord, delay, 1, 0x600, 0, 0)` at an exact
    /// coordinate. `None` rows construct with the warhead impact's
    /// `(0, 1, 0x2600, -15)` at the impact's cell, sub-cell and exact
    /// `world_z`. The TechnoClass debris anims are death constructions at
    /// `center + 0x14 Z` (`0x007024AA`, `0x00702566`) that already took their
    /// constructor draws.
    pub death: Option<destruction_effects::DeathAnimSpawn>,
}

/// Inputs from the two existing Spotlight5FF250 producer families. These
/// are ordered presentation facts, never simulation objects or attachments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatLightRequest {
    /// Damage helper48A620: IC/ForceShield, Bright bullets and other impacts.
    Impact {
        target_id: Option<u64>,
        damage: i32,
        warhead_ref: InternedId,
        coord: ProjectileCoord,
        force_create: bool,
        flags: u32,
    },
    /// SparkAI62EC53 directly constructs a persistent flags0 Spotlight. The
    /// app applies the client Options Detail gate; simulation never reads it.
    Spark {
        coord: ProjectileCoord,
        base_size: i32,
    },
}

/// One Bullet detonation's screen-shake writes, in native X-then-Y order.
/// `None` leaves that axis' prior process-owned value untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenShakeUpdate {
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// Phobos `[Warhead] ShakeIsLocal=` (`Ext/WarheadType/Body.cpp:184`, falling
    /// back to `[General] ShakeIsLocal` at `Ext/Rules/Body.cpp:491`): skip the
    /// writes when this detonation is outside the local tactical view
    /// (`Ext/Bullet/Hooks.DetonateLogics.cpp:36-39`).
    ///
    /// The native test runs against `TacticalClass::CoordsToClient`, i.e. the
    /// client's own camera, so the simulation cannot make the decision without
    /// diverging between peers. VERA keeps the draw order intact (the sim writes
    /// stay deterministic) and lets the presentation owner drop the update.
    /// Residual: native's `SkipShaking (0x469130)` jump may also skip the two
    /// `RandomRanged` draws; VERA keeps them for cross-client determinism.
    pub local_only: bool,
    /// The detonation coordinate the gate projects. `ProjectileCoord`.
    pub coord: ProjectileCoord,
}

/// One smudge producer payload. Production commits it synchronously through
/// the world receiver; phase-level combat fixtures retain it in their result as
/// a test adapter.
#[derive(Debug, Clone)]
pub enum SmudgeSpawnRequest {
    /// `AnimClass::Middle @ 0x00424F00` past its height gate: the anim's
    /// coordinate (vt+0x48) and its marks. The anim runtime emits it at Start
    /// or at the anim's middle frame.
    AnimMiddle {
        coord: crate::sim::smudge_grid::SimCoord,
        marks: smudge_dispatch::AnimMiddleMarks,
    },
    /// Emitted once per >=2x2 building destruction (DestructionEffects path).
    BuildingCenter {
        rx: u16,
        ry: u16,
        building_z: i32,
        foundation_w: u16,
        foundation_h: u16,
    },
    /// One foundation cell's mark, committed by `SpawnSurvivors` after that
    /// cell's survivor roll (never for a building owing no survivor).
    BuildingSurvivor { cell_rx: u16, cell_ry: u16 },
}

/// One admitted per-cell tiberium reduction: the inline dispatch's request,
/// and the record the callback-disabled receiver fixture defers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TiberiumReductionRequest {
    pub rx: u16,
    pub ry: u16,
    pub amount: i32,
}

/// An admitted building/harvester warning from damage or a first Temporal head.
///
/// Damage producers do not test the attacker's house:
/// `BuildingClass::ReceiveDamage @ 0x00442230`'s retaliation block calls
/// `HouseClass::NotifyUnderAttack` for a living building hit by a source
/// object with a non-zero result, unless its type is `Insignificant=`
/// (`+0x232`) or 1x1 with `UndeploysInto=` (vt+0x80, `0x00465D40`)
/// ([`crate::sim::world::Simulation::building_hit_response`]); own-fire on an
/// own building announces. `UnitClass::ReceiveDamage 0x007384B9..0x00738530`
/// pings a `Harvester=` unit on any non-zero, non-fatal result with or
/// without a source. `TemporalClass::InitiateWarp 0x0071AFBC..0x0071B0CA`
/// admits the same types when its first head attaches. The House dispatcher
/// owns their warning subtype; producers retain the victim type even if later
/// damage delivery removes the object before dispatch.
#[derive(Debug, Clone, Copy)]
pub struct UnderAttackEvent {
    pub rx: u16,
    pub ry: u16,
    /// The VICTIM's owner 鈥?the player whose radar/EVA should react.
    pub owner: InternedId,
    pub victim_type: InternedId,
    /// The victim house's own radar/EVA line. Ares
    /// HouseClass_BuildingUnderAttack_Malicious (10053980, hook 0x4F94A5)
    /// skips it for a building hit by a `Malicious=no` warhead; the ally
    /// line and the House Tags still follow.
    pub own_alert: bool,
    pub line: UnderAttackLine,
}

/// Which native call raises an [`UnderAttackEvent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderAttackLine {
    /// `HouseClass::NotifyUnderAttack @ 0x004F93E0` for a building: the
    /// victim house's own line, which picks the ore-miner cue for a deployed
    /// miner and the base cue otherwise, and, when `allies`, the line its
    /// human allies hear. Ares announceAttack (10062150) calls it only after
    /// testing Owner == PlayerPtr, so on the victim's own client alone, where
    /// the own branch runs: no ally hears it.
    House { allies: bool },
    /// `CreateRadarEvent(4)` and `EVA_OreMinerUnderAttack` called by the
    /// producer itself (Unit `0x00738530`, Temporal `0x0071B04C`, Ares
    /// announceAttack 10062150), with no siren, ally line or House Tags.
    OreMiner,
}

/// `TechnoClass::Death_Announcement @ 0x004D98C0` input: a non-building
/// techno was killed at a `ReceiveDamage` kill site (`AircraftClass
/// 0x00416613`, infantry `0x005180F4`, `UnitClass 0x00737DC9/0x00737E39/
/// 0x00737E68`, all vtable slot `+0x3B8`) and its type is not `Spawned=`.
/// The world applies the owner-is-human gate and the radar type-7 dedupe.
#[derive(Debug, Clone)]
pub struct UnitLostEvent {
    pub rx: u16,
    pub ry: u16,
    pub owner: InternedId,
    /// The dead techno's interned type, for the optional detailed EVA text;
    /// a transient fact, never a state input.
    pub type_id: InternedId,
    /// The Ares announcement, when Ares plays; `None` is the stock event.
    pub ares: Option<AresUnitLost>,
}

/// What Ares' announcement adds to a [`UnitLostEvent`] (FootClass_Destroyed
/// 10049080, hook 0x4D98C0; BuildingClass_Destroyed_UnitLost 10013F10, hook
/// 0x44D760).
#[derive(Debug, Clone)]
pub struct AresUnitLost {
    /// The victim type's `EVA.Lost`, spoken whether or not the client admits
    /// the radar event.
    pub eva: String,
    /// A Foot creates the radar event (FootClass_Destroyed 10049080); a
    /// building does not (BuildingClass_Destroyed_UnitLost 10013F10).
    pub radar_event: bool,
}

/// One ordered accumulator for weapon emission and recursive receiver effects.
/// DamageConsequences consumes its deferred work at the world delivery boundary.
#[derive(Default)]
pub(crate) struct DeathEffects {
    /// Fatal receivers, including SHP deaths that remain represented for animation.
    pub(crate) despawned_ids: Vec<u64>,
    /// Remaining world UnInit requests; distinct from all fatal receiver IDs.
    pub(crate) immediate_uninit_ids: Vec<u64>,
    pub(crate) structure_destroyed: bool,
    pub(crate) explosion_effects: Vec<ExplosionEffect>,
    /// Already-constructed, identified `VoxelAnimClass` pieces retained only
    /// by callback-disabled damage fixtures. Live Bullet and Techno constructors
    /// register synchronously; delivering these packets never allocates IDs.
    pub(crate) voxel_debris: Vec<crate::sim::voxel_anim::VoxelDebrisSpawn>,
    pub(crate) combat_light_requests: Vec<CombatLightRequest>,
    /// Receipt of synchronous Apply_area_damage bridge callbacks. Gameplay
    /// already ran before the detonation's animation and cluster successor.
    pub(crate) bridge_state_changed: bool,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) wall_mutations: Vec<WallMutation>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) cell_target_detaches: Vec<combat_aoe::CellTargetDetach>,
    /// The receiver fixture's deferred reductions. Production reduces each
    /// admitted cell inline, before the cell's damage.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) tiberium_reduction_requests: Vec<TiberiumReductionRequest>,
    pub(crate) death_sounds: Vec<(String, crate::sim::anim_class::AnimWorldCoord)>,
    pub(crate) smudge_spawn_requests: Vec<SmudgeSpawnRequest>,
    pub(crate) rad_detonations: Vec<crate::sim::radiation::RadDetonation>,
    pub(crate) under_attack_events: Vec<UnderAttackEvent>,
    /// `Death_Announcement` inputs from this tick's damage kills; the world
    /// applies the human-owner gate and the radar type-7 dedupe.
    pub(crate) unit_lost_events: Vec<UnitLostEvent>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) receiver_stage_trace: Vec<ReceiverStageTrace>,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReceiverStageTrace {
    HouseThreat { target_id: u64, delta: i32 },
    PostMortem { target_id: u64 },
    ShouldRetaliate { target_id: u64 },
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn tiberium_reduction_amount(
    base_damage: i32,
    affect_resource: bool,
    warhead: &WarheadType,
) -> Option<i32> {
    if !affect_resource || !warhead.tiberium {
        return None;
    }
    let amount = base_damage / 10;
    (amount > 0).then_some(amount)
}

impl DeathEffects {
    pub(crate) fn append(&mut self, mut other: Self) {
        self.despawned_ids.append(&mut other.despawned_ids);
        self.immediate_uninit_ids
            .append(&mut other.immediate_uninit_ids);
        self.structure_destroyed |= other.structure_destroyed;
        self.explosion_effects.append(&mut other.explosion_effects);
        self.voxel_debris.append(&mut other.voxel_debris);
        self.combat_light_requests
            .append(&mut other.combat_light_requests);
        self.bridge_state_changed |= other.bridge_state_changed;
        #[cfg(any(test, feature = "test-support"))]
        self.wall_mutations.append(&mut other.wall_mutations);

        #[cfg(any(test, feature = "test-support"))]
        self.cell_target_detaches
            .append(&mut other.cell_target_detaches);
        #[cfg(any(test, feature = "test-support"))]
        self.tiberium_reduction_requests
            .append(&mut other.tiberium_reduction_requests);
        self.death_sounds.append(&mut other.death_sounds);
        self.smudge_spawn_requests
            .append(&mut other.smudge_spawn_requests);
        self.rad_detonations.append(&mut other.rad_detonations);
        self.under_attack_events
            .append(&mut other.under_attack_events);
        self.unit_lost_events.append(&mut other.unit_lost_events);
        #[cfg(any(test, feature = "test-support"))]
        self.receiver_stage_trace
            .append(&mut other.receiver_stage_trace);
    }
}

/// `SpotlightFlags::NoColor` (`Misc/Hooks.LightEffects.cpp:80`,
/// `GeneralDefinitions.h:614..621`): bit 0, the black lane of the light's
/// channel math in [`crate::render::combat_light`]. The fps admission reads
/// `flags & 0xE` separately, which this bit does not bypass.
#[cfg(feature = "phobos")]
pub const NO_COLOR_SPOTLIGHT_FLAGS: u32 = 0x1;

/// Phobos `CombatLightChance=`'s probability gate at the native `Flash`
/// creation point: `0x48A620` draws one `Randomizer::Global.RandomDouble()`
/// with the warhead (`Misc/Hooks.LightEffects.cpp:69-74`) and, when the
/// authored chance is below that draw, returns before the per-warhead
/// `CombatLightDetailLevel`/`CLDisable` reads and before the detail, fps and
/// `Bright` tests. So the draw precedes those tests and a suppressed flash
/// still spends it; `force_create` bypasses `Bright` only, never this gate.
/// `Flash` without a warhead (the bridge and trigger rows) has no chance read.
///
/// Sim-scale deviation: the draw is made only when the authored value is below
/// 1.0. Retail rules never set the key and the constructor value is 1.0, so
/// drawing for every stock explosion would shift the process Main886B88 stream
/// that the original-execution fixtures compare against while changing no
/// visible flash; a rule set that authors the key keeps native draw count and
/// order.
#[cfg(not(feature = "phobos"))]
pub(crate) fn combat_light_chance_admits(
    _world: &crate::sim::world::Simulation,
    _rules: &crate::rules::ruleset::RuleSet,
    _warhead: crate::sim::intern::InternedId,
) -> bool {
    true
}

#[cfg(feature = "phobos")]
pub(crate) fn combat_light_chance_admits(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    warhead_ref: InternedId,
) -> bool {
    let Some(warhead) = rules.warhead(sim.interner.resolve(warhead_ref)) else {
        return true;
    };
    let chance = warhead.phobos_combat_light_chance;
    chance >= 1.0 || chance >= sim.main_rng().random_double()
}

#[cfg(all(test, feature = "phobos"))]
mod combat_light_chance_tests {
    use super::combat_light_chance_admits;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::world::Simulation;

    fn rules_with(body: &str) -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Warheads]\n0=WH\n[WH]\n{body}"
        )))
        .unwrap()
    }

    /// Retail `[Warheads]` leaves the key unset; the constructor's 1.0 admits
    /// and, under the recorded deviation, spends no draw.
    #[test]
    fn an_unset_chance_admits_without_touching_main() {
        let rules = rules_with("");
        let mut sim = Simulation::new();
        let warhead = sim.interner.intern("WH");
        let before = sim.main_rng().state();
        assert!(combat_light_chance_admits(&sim, &rules, warhead));
        assert_eq!(sim.main_rng().state(), before);
    }

    /// `CombatLightChance=0` never admits even though it still takes its draw;
    /// `=1` is the admitting boundary.
    #[test]
    fn boundary_chances_gate_the_flash_and_keep_the_draw() {
        let zero = rules_with("CombatLightChance=0\n");
        let mut sim = Simulation::new();
        let warhead = sim.interner.intern("WH");
        let before = sim.main_rng().state();
        assert!(!combat_light_chance_admits(&sim, &zero, warhead));
        assert_ne!(sim.main_rng().state(), before, "the gate draws once");

        let one = rules_with("CombatLightChance=1\n");
        let mut sim = Simulation::new();
        let warhead = sim.interner.intern("WH");
        let before = sim.main_rng().state();
        assert!(combat_light_chance_admits(&sim, &one, warhead));
        assert_eq!(sim.main_rng().state(), before);
    }
}
