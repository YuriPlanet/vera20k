//! One concrete `ReceiveDamage` call: its value ABI ([`EntityDamageEvent`], and
//! [`TerrainDamageEvent`] for a TerrainClass receiver), the Building wrapper
//! prelude and Infantry heads around the shared receiver, and the post-mortem
//! delay and Iron Curtain isolation facts the world receiver reads from it.

use super::*;

/// InfantryClass::ReceiveDamage's prone head (`0x00517FC1..0x00517FEF`): a
/// prone infantryman's positive raw damage, unless defenses are ignored,
/// becomes `ftol(fild damage * fmul qword [warhead+0xF8])`, at least 1.
/// Math__ftol (`0x007C5F00`) stores a qword and the head keeps EAX, so a
/// product beyond 32 bits wraps before the clamp, and an infinite product
/// (`1e39%`) converts to the indefinite qword, whose low dword 0 the clamp
/// makes 1. Native rows: tools/rules_oracle/read_double_percent (group B).
pub(super) fn infantry_prone_raw_damage(
    target: &GameEntity,
    warhead: &WarheadType,
    damage: i32,
    ignore_defenses: bool,
) -> i32 {
    use crate::util::native_x87::MaskedX87Chop53 as X87;
    if target.category != EntityCategory::Infantry
        || !infantry::is_prone_for_damage(target)
        || damage <= 0
        || ignore_defenses
    {
        return damage;
    }
    let multiplier = X87::load_f64(NativeF64Bits::from_bits(warhead.prone_damage_f64.to_bits()));
    X87::ftol_i32_low_masked(X87::mul(X87::load_i32(damage), multiplier)).max(1)
}

/// InfantryClass::ReceiveDamage's next head (`0x00517FF5..0x00518010`): an
/// `InfDeath=9` warhead (`+0x120`) does no damage to an infantryman above the
/// ground or its deck (GetHeight, vt+0x1C8, `ObjectClass::GetHeight @
/// 0x005F5F40`), such as a paratrooper in its fall or a Rocketeer in flight.
/// Retail's two InfDeath 9 warheads are the Genetic Mutator's.
fn infantry_spared_mutation(
    target: &GameEntity,
    warhead: &WarheadType,
    terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    const MUTATE_INF_DEATH: u8 = 9;
    target.category == EntityCategory::Infantry
        && warhead.inf_death == MUTATE_INF_DEATH
        && crate::sim::movement::air_movement::current_fly_height(target, terrain) > 0
}

/// Sentinel attacker id for sourceless damage (the radiation field). Stable
/// entity ids start at 1, so 0 is never a live attacker; the receiver asks no
/// retaliation for it.
pub(crate) const RAD_NO_ATTACKER: u64 = 0;

/// The two receiver booleans carried by one native concrete `ReceiveDamage`
/// call. Their semantic names are intentionally limited to the verified ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReceiverCallFlags {
    pub(crate) ignore_defenses: bool,
    pub(crate) arg6: bool,
}

/// One ordered damage call. Area and direct-receiver records retain the raw
/// signed damage, native lepton distance, and concrete receiver flags until
/// dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntityDamageEvent {
    pub(crate) target_id: u64,
    pub(crate) damage: i32,
    pub(crate) attacker_id: u64,
    /// ReceiveDamage's sourceHouse ABI argument captured when the detonation
    /// enters Apply_area_damage. This remains valid if the source object is
    /// uninitialized by an earlier ordered receiver record.
    pub(crate) source_house: Option<InternedId>,
    pub(crate) warhead_ref: InternedId,
    pub(crate) distance_leptons: i32,
    pub(crate) receiver_flags: ReceiverCallFlags,
    /// This record belongs to an Apply_area_damage transaction whose captured
    /// CellSpread is at most 0.5. The receiver commit uses this transient fact
    /// to reproduce the native near-center Iron Curtain isolation scan; it is
    /// deliberately false for direct-receiver calls.
    pub(crate) near_center_ic_isolation_eligible: bool,
}

impl EntityDamageEvent {
    pub(crate) fn area(
        target_id: u64,
        raw_damage: i32,
        distance_leptons: i32,
        attacker_id: u64,
        source_house: Option<InternedId>,
        warhead_ref: InternedId,
    ) -> Self {
        Self {
            target_id,
            damage: raw_damage,
            attacker_id,
            source_house,
            warhead_ref,
            distance_leptons,
            receiver_flags: ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
            near_center_ic_isolation_eligible: false,
        }
    }

    pub(crate) fn direct_receiver(
        target_id: u64,
        raw_damage: i32,
        distance_leptons: i32,
        attacker_id: u64,
        source_house: Option<InternedId>,
        warhead_ref: InternedId,
        receiver_flags: ReceiverCallFlags,
    ) -> Self {
        Self {
            target_id,
            damage: raw_damage,
            attacker_id,
            source_house,
            warhead_ref,
            distance_leptons,
            receiver_flags,
            near_center_ic_isolation_eligible: false,
        }
    }
}

/// One captured TerrainClass receiver in a fixed Apply_area_damage transaction.
///
/// Transient only: stable identity, cell, distance, and isolation scope are
/// captured during collection so dispatch never rescans a later world state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainDamageEvent {
    pub stable_id: u64,
    pub rx: u16,
    pub ry: u16,
    pub damage: i32,
    pub distance_leptons: i32,
    pub warhead_ref: InternedId,
    /// True when the parent AoE used native binary32 CellSpread <= 0.5.
    /// Terrain cannot arm IC isolation, but an armed transaction skips it.
    pub near_center_ic_isolation_eligible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BaseDefenseResponseCallSite {
    BuildingPrelude,
    ProtectedTechno,
}

/// [`resolve_receive_damage`]'s result for one receiver record.
#[derive(Debug, Clone, Copy)]
pub(super) struct ResolvedReceiveDamage {
    pub(super) outcome: damage::DamageOutcome,
    pub(super) invulnerability_impact: Option<CombatLightRequest>,
}

fn receiver_effect_coord(
    target: &GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> ProjectileCoord {
    let x = i32::from(target.position.rx)
        .wrapping_mul(256)
        .wrapping_add(target.position.sub_x.to_num::<i32>());
    let y = i32::from(target.position.ry)
        .wrapping_mul(256)
        .wrapping_add(target.position.sub_y.to_num::<i32>());
    let z = object_world_z_leptons(target, terrain);
    ProjectileCoord::new(x, y, z)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuildingReceivePrelude {
    Continue,
    Respond,
    ReturnZero,
}

/// Execute the BuildingClass wrapper work which natively precedes the shared
/// Techno receiver.
///
/// gamemd-derived: `BuildingClass__ReceiveDamage @ 0x00442230` returns zero at
/// `0x00442262` for disallowed self damage. A non-null attacker then writes the
/// victim owner's `House+0x54D8` at `0x0044229C`, before Building immunity,
/// the already-dead gate, and `TechnoClass__ReceiveDamage @ 0x00442425`.
pub(super) fn apply_building_receive_prelude(
    event: &EntityDamageEvent,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    houses: &mut BTreeMap<InternedId, HouseState>,
    current_tick: u64,
) -> BuildingReceivePrelude {
    let Some(target) = entities.get(event.target_id) else {
        return BuildingReceivePrelude::Continue;
    };
    if target.category != EntityCategory::Structure {
        return BuildingReceivePrelude::Continue;
    }
    let Some(target_type) = rules.object(interner.resolve(target.type_ref())) else {
        return BuildingReceivePrelude::Continue;
    };

    if event.attacker_id == event.target_id && !target_type.damage_self {
        return BuildingReceivePrelude::ReturnZero;
    }
    if event.attacker_id != RAD_NO_ATTACKER
        && !target_type.is_1x1_with_undeploy()
        && let Some(owner) = houses.get_mut(&target.owner())
    {
        owner
            .strategy_emergency
            .note_building_attack(current_tick as u32 as i32);
    }

    BuildingReceivePrelude::Respond
}

/// Build the native ReceiveDamage value ABI for one ordered area or direct
/// receiver record and run the shared receiver exactly once. The returned
/// signed HP delta is the only health input consumed by `commit_damage_events`.
pub(super) fn resolve_receive_damage(
    event: &EntityDamageEvent,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    houses: &BTreeMap<InternedId, HouseState>,
    alliances: &HouseAllianceMap,
    scenario_no_damage: bool,
    current_tick: u64,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<ResolvedReceiveDamage> {
    let distance_leptons = event.distance_leptons;
    let receiver_flags = event.receiver_flags;
    let target = entities.get(event.target_id)?;
    // Building442230 returns before Techno for Health0, after its wrapper
    // response (already executed by commit_entities). Infantry/Unit instead
    // delegate and can re-enter Techno's fatal tail at zero Health.
    if target.category == EntityCategory::Structure && target.health.current == 0 {
        return None;
    }
    let warhead = rules.warhead(interner.resolve(event.warhead_ref))?;
    let target_type = rules.object(interner.resolve(target.type_ref()));
    let source = (event.attacker_id != RAD_NO_ATTACKER)
        .then(|| entities.get(event.attacker_id))
        .flatten();
    let source_house = event
        .source_house
        .or_else(|| source.map(|entity| entity.owner()));
    // InfantryClass mutates the positive raw i32 before forwarding to the
    // shared Techno receiver. Its sign is therefore Techno's original-sign
    // snapshot used by the IC/FS gate below.
    let receiver_input = if infantry_spared_mutation(target, warhead, terrain) {
        0
    } else {
        infantry_prone_raw_damage(
            target,
            warhead,
            event.damage,
            receiver_flags.ignore_defenses,
        )
    };

    let allied = |asker: InternedId, other: InternedId| {
        crate::map::houses::is_allied_with(
            alliances,
            interner.resolve(asker),
            interner.resolve(other),
        )
    };
    let attacker_is_allied = source_house.is_some_and(|owner| allied(owner, target.owner()));
    let source_house_is_allied = source_house.is_some_and(|owner| allied(target.owner(), owner));

    let target_is_building = target.category == EntityCategory::Structure;
    let target_view = damage::TargetDamageView {
        armor: damage::ArmorClass(
            target_type
                .map(|object| armor_index(&object.armor))
                .unwrap_or(0) as u8,
        ),
        current_hp: target.health.current,
        object_immune: target_type.is_some_and(|object| object.immune),
    };
    let type_immune = target_type.is_some_and(|object| object.type_immune)
        && source.is_some_and(|source| {
            source.type_ref() == target.type_ref() && source.owner() == target.owner()
        });
    // 701900 jumps to701BF6 when ignoreDefenses is set, before either linked
    // bunker branch. BlowUpBridge's forced C4 receiver must bypass both arms.
    let bunker_blocked = !receiver_flags.ignore_defenses
        && if target_is_building && target.bunker_occupant.is_some() {
            // Linked Building branch is intentionally the inverse of the installed
            // non-Building branch in TechnoClass::ReceiveDamage.
            warhead.penetrates_bunker
        } else {
            target.bunker_link.installed_in().is_some() && !warhead.penetrates_bunker
        };
    let active_invulnerability = target.invulnerability.as_ref().filter(|_| {
        crate::sim::superweapon::invulnerability::is_invulnerable(
            target.invulnerability.as_ref(),
            current_tick as u32,
        )
    });
    let gates = damage::ImmunityInputs {
        ignore_defenses: receiver_flags.ignore_defenses,
        attacker_present: event.attacker_id != RAD_NO_ATTACKER,
        type_immune,
        // IC/FS checks original sign and precedes WarpingOut. Warping does not
        // share the negative/healing exemption; both honor ignoreDefenses.
        invulnerable: !receiver_flags.ignore_defenses
            && receiver_input >= 0
            && active_invulnerability.is_some(),
        // `+0x270` (vtable `+0x1D4`, `0x00701AB1`): a Chrono teleport's
        // warp-out or a Temporal warp.
        warping_out: !receiver_flags.ignore_defenses && target.is_warped_out(),
        bunker_blocked,
        radiation_immune: warhead.radiation
            && target_type.is_some_and(|object| object.immune_to_radiation),
        psychic_immune: warhead.psychic_damage
            && target_type.is_some_and(|object| object.immune_to_psionic_weapons),
        poison_immune: warhead.poison && target_type.is_some_and(|object| object.immune_to_poison),
        affects_allies: warhead.affects_allies,
        attacker_is_allied,
        source_house_is_allied,
        psychedelic: warhead.psychedelic,
        psionics_immune: target_type.is_some_and(|object| object.immune_to_psionics),
        target_is_building,
    };
    // `HouseClass::GetArmorMultForType @ 0x0050BD30` on the target's owner:
    // its HouseType's per-category float. The difficulty and country
    // `Armor=` product (`House+0x1A0`) is never read here.
    let house_type_armor = houses.get(&target.owner()).map_or(1.0, |house| {
        target_type.map_or(1.0, |object| {
            rules.country_armor_mult_for_type(interner.resolve(house.house_type_id()), object)
        })
    });
    let rank_armor = target_type
        .is_some_and(|object| {
            super::veterancy::has_weapon_ability(
                target.veterancy.rank_kind(),
                object,
                crate::rules::object_type::Ability::Stronger,
            )
        })
        .then(|| {
            crate::util::native_x87::NativeF64Bits::from_bits(rules.general.veteran_armor.to_bits())
        });
    let divisors = damage::DefenceDivisors {
        house_type_armor: crate::util::native_x87::NativeF32Bits::from_bits(
            house_type_armor.to_bits(),
        ),
        unit_armor: target.armor_multiplier,
        rank_armor,
    };
    // `arg6` stays on the ordered call: its Unit-class consumer is the crew
    // block, which the concrete death reads from the killing event.
    let outcome = damage::receive::receive_damage(
        receiver_input,
        warhead.cell_spread_f64,
        warhead.percent_at_max_f64,
        &warhead.verses_f64,
        &target_view,
        &divisors,
        &gates,
        distance_leptons,
        scenario_no_damage,
        rules.combat_damage.max_damage,
    );
    let invulnerability_impact = outcome.invulnerability_impact_damage.map(|damage| {
        let flags = match active_invulnerability
            .expect("receiver gate retained active state")
            .kind
        {
            crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain => 1,
            crate::sim::superweapon::invulnerability::InvulnKind::ForceShield => 6,
        };
        CombatLightRequest::Impact {
            target_id: Some(target.stable_id()),
            damage,
            warhead_ref: event.warhead_ref,
            coord: receiver_effect_coord(target, terrain),
            force_create: true,
            flags,
        }
    });
    Some(ResolvedReceiveDamage {
        outcome,
        invulnerability_impact,
    })
}

/// Inspect the value left in WaveClass::DamageArea's shared per-cell damage
/// local by one concrete Techno receiver. The receiver commit immediately
/// following this call runs the same pure ABI resolver against the same live
/// state; exposing the pointer result here avoids fabricating immutable damage
/// inputs for later occupants.
pub(crate) fn wave_post_object_damage(
    event: &EntityDamageEvent,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    houses: &BTreeMap<InternedId, HouseState>,
    alliances: &HouseAllianceMap,
    scenario_no_damage: bool,
    current_tick: u64,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<i32> {
    resolve_receive_damage(
        event,
        entities,
        rules,
        interner,
        houses,
        alliances,
        scenario_no_damage,
        current_tick,
        terrain,
    )
    .and_then(|resolved| resolved.outcome.post_object_damage)
}

/// TechnoClass::ReceiveDamage builds the anger-node increment from the final
/// ObjectClass damage packet, not from raw weapon damage. The x87 keeps the
/// division result live, multiplies by the type's virtual cost, then Math__ftol
/// returns an i64 whose low dword is passed to HouseClass.
pub(super) fn receiver_anger_delta(final_damage: i32, strength: i32, cost: i32) -> i32 {
    if strength == 0 {
        return 0;
    }
    let ratio = X87Chop53::div(
        X87Chop53::load_i32(final_damage),
        X87Chop53::load_i32(strength),
    )
    .expect("live Techno strength is nonzero");
    X87Chop53::ftol_i64(X87Chop53::mul(ratio, X87Chop53::load_i32(cost)))
        .expect("i32 damage/cost ratio fits native ftol i64") as i32
}

/// TechnoClass::ReceiveDamage PostMortem interpolation at 0x00701ED0.
/// `CellSpread` and `DelayKillAtMax` are native binary32 inputs; every other
/// operand is signed i32 and the two ftol results use their low dword.
fn postmortem_delay_duration(warhead: &WarheadType, distance_leptons: i32) -> i32 {
    let base = X87Chop53::load_i32(warhead.delay_kill_frames);
    let at_max = X87Chop53::load_f32(NativeF32Bits::from_bits(
        (warhead.delay_kill_at_max_f64 as f32).to_bits(),
    ))
    .expect("DelayKillAtMax parser retains a finite native f32");
    let slope = X87Chop53::sub(X87Chop53::mul(at_max, base), base);
    let denominator = cell_spread::whole_cells(warhead.cell_spread_f64).wrapping_shl(8);
    let Ok(slope_per_lepton) = X87Chop53::div(slope, X87Chop53::load_i32(denominator)) else {
        // Masked x87 divide-by-zero/non-finite conversion yields the integer
        // indefinite qword; Math__ftol returns its low dword, which is zero.
        return 0;
    };
    let delay = X87Chop53::add(
        base,
        X87Chop53::mul(X87Chop53::load_i32(distance_leptons), slope_per_lepton),
    );
    X87Chop53::ftol_i32_low_masked(delay)
}

pub(super) fn postmortem_duration_for_event(
    event: &EntityDamageEvent,
    target: &GameEntity,
    rules: &RuleSet,
    interner: &StringInterner,
    state: damage::DamageState,
) -> Option<i32> {
    if state != damage::DamageState::Dead || target.category != EntityCategory::Structure {
        return None;
    }
    let warhead = rules.warhead(interner.resolve(event.warhead_ref))?;
    let object = rules.object(interner.resolve(target.type_ref()))?;
    (warhead.causes_delay_kill && object.eligible_for_delay_kill)
        .then(|| postmortem_delay_duration(warhead, event.distance_leptons))
}

pub(super) fn has_active_area_invulnerability(entity: &GameEntity, current_tick: u64) -> bool {
    // Every GameEntity is a TechnoClass-derived object, so the native
    // AbstractFlags +0x14 bit-0 identity test is inherent in this store. The
    // virtual +0x160 result is the existing passive IC/FS timer predicate.
    crate::sim::superweapon::invulnerability::is_invulnerable(
        entity.invulnerability.as_ref(),
        current_tick as u32,
    )
}

fn event_arms_ic_isolation(
    event: &EntityDamageEvent,
    entities: &EntityStore,
    current_tick: u64,
) -> bool {
    event.near_center_ic_isolation_eligible
        && event.distance_leptons < 85
        && entities.get(event.target_id).is_some_and(|target| {
            has_active_area_invulnerability(target, current_tick)
                && target.invulnerability.as_ref().is_some_and(|state| {
                    state.kind == crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain
                })
        })
}

pub(super) fn near_center_ic_isolation_armed(
    damage_events: &[EntityDamageEvent],
    entities: &EntityStore,
    current_tick: u64,
) -> bool {
    damage_events
        .iter()
        .any(|event| event_arms_ic_isolation(event, entities, current_tick))
}

pub(super) fn area_near_center_ic_isolation_armed(
    receivers: &[combat_aoe::AreaDamageReceiver],
    entities: &EntityStore,
    current_tick: u64,
) -> bool {
    receivers.iter().any(|receiver| {
        matches!(receiver, combat_aoe::AreaDamageReceiver::Entity(event)
            if event_arms_ic_isolation(event, entities, current_tick))
    })
}
