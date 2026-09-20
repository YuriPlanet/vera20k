//! The `ReceiveDamage` decloak and the damage outcomes that never reach it.
//!
//! `TechnoClass::ReceiveDamage @ 0x00701900` calls virtual `+0xFC`
//! (`StartUncloaking(0) @ 0x00703850`) at `0x0070281D`. Two native conditions
//! keep receivers away from it, and these tests pin both negative halves:
//!
//! 1. Every defensive gate returns ABOVE the `uVar7 =
//!    ObjectClass__ReceiveDamage(this)` join — the `TypeImmune`
//!    (`type+0xC8C`) same-type/same-owner arm, `vt+0x160` (IronCurtain /
//!    ForceShield), `vt+0x1D4` (warping in), the `AffectsAllies=no`
//!    (`warhead+0x179`) allied arm, and the accepted Psychedelic arm.
//! 2. After the join, `Health == 0` overwrites the switch selector with 4
//!    (`0x00702035`) and the jump table at `0x00702D24` sends case 4 to the
//!    death handler at `0x00702050`, which returns without reaching
//!    `0x0070281D` — so a corpse never surfaces, whatever ObjectClass
//!    returned.
//!
//! `[DLPH] TypeImmune=yes` in `rulesmd.ini` and DLPH is one of the four stock
//! `Cloakable=yes` types, so a player's own Dolphins splashing each other is
//! the ordinary trigger for (1); a multi-record blast on an already-killed
//! Dolphin is the trigger for (2).

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::cloak_disguise::CloakRuntime;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;
use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};

const TICK: u64 = 100;
const CLOAKING_STAGES: i32 = 9;

fn dolphin_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\nCloakingStages=9\n\
         [AudioVisual]\nCloakSound=NavalUnitEmerge\n\
         [VehicleTypes]\n0=DLPH\n1=DEST\n\
         [DLPH]\nStrength=600\nArmor=none\nSpeed=4\nCloakable=yes\nCloakingSpeed=1\n\
         TypeImmune=yes\nPrimary=SonicZap\n\
         [DEST]\nStrength=600\nArmor=none\nSpeed=6\nPrimary=SonicZap\n\
         Secondary=AllyBlindZap\n\
         [SonicZap]\nDamage=40\nROF=20\nRange=8\nWarhead=SonicWH\n\
         [AllyBlindZap]\nDamage=40\nROF=20\nRange=8\nWarhead=AllyBlindWH\n\
         [SonicWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [AllyBlindWH]\nAffectsAllies=no\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("dolphin damage fixture")
}

fn live(mut entity: GameEntity) -> GameEntity {
    entity.lifecycle.in_limbo = false;
    entity.in_playfield = true;
    entity
}

/// Entity 1 is the attacker (`attacker_type`), entity 2 a fully cloaked DLPH
/// one cell east. Both belong to `attacker_owner` / `Soviet` respectively.
fn store(attacker_type: &str, attacker_owner: &str) -> EntityStore {
    let mut store = EntityStore::new();
    store.insert(live(GameEntity::test_default(
        1,
        attacker_type,
        attacker_owner,
        10,
        10,
    )));
    let mut victim = live(GameEntity::test_default(2, "DLPH", "Soviet", 11, 10));
    let mut cloak = CloakRuntime::new(0, CLOAKING_STAGES);
    cloak.establish_unlimbo_fully_cloaked();
    victim.cloak = Some(cloak);
    store.insert(victim);
    store
}

struct Landed {
    cloak_state: i32,
    sounds: Vec<SimSoundEvent>,
    victim_hp: i32,
}

/// One `commit_damage_events` transaction: entity 1 hits entity 2 for 40 with
/// `warhead`, at the ordinary area distance zero.
fn hit(
    entities: &mut EntityStore,
    rules: &RuleSet,
    warhead: &str,
    alliances: &HouseAllianceMap,
) -> Landed {
    hit_records(entities, rules, warhead, alliances, &[40])
}

/// One `commit_damage_events` transaction carrying `damages.len()` ordered
/// records against entity 2 — the same shape Apply_area_damage produces for a
/// multi-record blast, and the shape a `DeathWeapon` cascade produces when a
/// later record lands on a receiver an earlier one already drove to zero.
fn hit_records(
    entities: &mut EntityStore,
    rules: &RuleSet,
    warhead: &str,
    alliances: &HouseAllianceMap,
    damages: &[i32],
) -> Landed {
    let mut interner = test_interner();
    let soviet = interner.intern("Soviet");
    let allies = interner.intern("Americans");
    let warhead_ref = interner.intern(warhead);
    let attacker_owner = entities.get(1).map(|attacker| attacker.owner);

    let mut houses = BTreeMap::from([
        (soviet, HouseState::new(soviet, 0, None, false, 0, 10)),
        (allies, HouseState::new(allies, 1, None, false, 0, 10)),
    ]);
    let house_order = [soviet, allies];
    let mut occupancy = OccupancyGrid::new();
    let mut main_rng = SimRng::new(11);
    let mut scenario_rng = SimRng::new(13);
    let mut handled_deaths = Vec::new();
    let mut hooks = None;
    let mut collected: Vec<SimSoundEvent> = Vec::new();
    let mut sound_sink: Option<&mut Vec<SimSoundEvent>> = Some(&mut collected);

    let records: Vec<EntityDamageEvent> = damages
        .iter()
        .map(|damage| EntityDamageEvent::area(2, *damage, 0, 1, attacker_owner, warhead_ref))
        .collect();

    let _ = commit_damage_events(
        &records,
        entities,
        &mut occupancy,
        rules,
        &mut interner,
        &mut houses,
        &house_order,
        alliances,
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        TICK,
        &mut hooks,
        &mut sound_sink,
    );

    let victim = entities.get(2).expect("victim survives the fixture");
    Landed {
        cloak_state: victim.cloak.as_ref().map_or(-1, |cloak| cloak.state),
        sounds: collected,
        victim_hp: victim.health.current,
    }
}

#[test]
fn healing_overflow_stays_on_techno_nonzero_continuation_and_uncloaks() {
    // Object5F546A wraps INT_MAX-(-1) to INT_MIN and returns0; Techno70202E
    // checks exact0, so its real surviving tail still calls StartUncloaking.
    let rules = dolphin_rules();
    let mut entities = store("DEST", "Americans");
    let victim = entities.get_mut(2).unwrap();
    victim.health.current = i32::MAX;
    victim.estimated_health = crate::sim::estimated_health::EstimatedHealth::from_raw(-1234);
    let landed = hit_records(
        &mut entities,
        &rules,
        "SonicWH",
        &HouseAllianceMap::new(),
        &[-1],
    );
    assert_eq!(landed.victim_hp, i32::MIN);
    assert_eq!(landed.cloak_state, 3, "negative HP must still uncloak");
    let victim = entities.get(2).unwrap();
    assert!(victim.lifecycle.object_alive);
    assert!(!victim.dying);
    assert_eq!(victim.estimated_health.get(), -1234);
}

/// Positive control. Without it the three negative tests below would also pass
/// against a hook that never fires.
#[test]
fn an_ordinary_hostile_hit_surfaces_a_fully_cloaked_dolphin() {
    let rules = dolphin_rules();
    let mut entities = store("DEST", "Americans");
    let landed = hit(&mut entities, &rules, "SonicWH", &HouseAllianceMap::new());

    assert_eq!(landed.victim_hp, 60, "the hit lands for its 40");
    assert_eq!(
        landed.cloak_state, 3,
        "0x0070281D reaches vt+0xFC for a surviving receiver"
    );
    assert!(
        matches!(
            landed.sounds.as_slice(),
            [SimSoundEvent::CloakSound { sound_id, rx: 11, ry: 10, .. }]
                if sound_id == "NavalUnitEmerge"
        ),
        "StartUncloaking(0) owns one positional CloakSound, got {:?}",
        landed.sounds
    );
}

/// `TypeImmune` returns `0` from `TechnoClass::ReceiveDamage` well above the
/// `ObjectClass__ReceiveDamage` join, so `+0xFC` is never reached.
/// `[DLPH] TypeImmune=yes` makes this the routine case for a Dolphin pod.
#[test]
fn a_type_immune_sibling_splash_leaves_a_cloaked_dolphin_submerged() {
    let rules = dolphin_rules();
    let mut entities = store("DLPH", "Soviet");
    let landed = hit(&mut entities, &rules, "SonicWH", &HouseAllianceMap::new());

    assert_eq!(landed.victim_hp, 100, "TypeImmune nullifies the damage");
    assert_eq!(
        landed.cloak_state, 2,
        "native returns at type+0xC8C, above the vt+0xFC call"
    );
    assert!(
        landed.sounds.is_empty(),
        "no CloakSound: {:?}",
        landed.sounds
    );
}

/// `vt+0x160` (IronCurtain / ForceShield) writes `*damage = 0` and returns, so
/// the protected object never delegates to `ObjectClass::ReceiveDamage`.
#[test]
fn an_iron_curtained_cloaked_dolphin_stays_submerged() {
    let rules = dolphin_rules();
    let mut entities = store("DEST", "Americans");
    entities.get_mut(2).unwrap().invulnerability = Some(InvulnerabilityState {
        start_frame: TICK as u32 - 10,
        duration_frames: 100,
        kind: InvulnKind::IronCurtain,
    });
    let landed = hit(&mut entities, &rules, "SonicWH", &HouseAllianceMap::new());

    assert_eq!(landed.victim_hp, 100, "IronCurtain nullifies the damage");
    assert_eq!(landed.cloak_state, 2, "native returns at vt+0x160");
    assert!(
        landed.sounds.is_empty(),
        "no CloakSound: {:?}",
        landed.sounds
    );
}

/// `AffectsAllies=no` (`warhead+0x179`) with a present source and an allied
/// owner returns above the join too. A house is always allied with itself, and
/// an explicit two-house alliance behaves the same way.
#[test]
fn an_affects_allies_no_warhead_leaves_an_allied_cloaked_dolphin_submerged() {
    let rules = dolphin_rules();

    let mut own = store("DEST", "Soviet");
    let landed = hit(&mut own, &rules, "AllyBlindWH", &HouseAllianceMap::new());
    assert_eq!(landed.victim_hp, 100);
    assert_eq!(landed.cloak_state, 2, "same house is allied with itself");
    assert!(
        landed.sounds.is_empty(),
        "no CloakSound: {:?}",
        landed.sounds
    );

    let alliances: HouseAllianceMap = BTreeMap::from([
        (
            "AMERICANS".to_string(),
            BTreeSet::from(["SOVIET".to_string()]),
        ),
        (
            "SOVIET".to_string(),
            BTreeSet::from(["AMERICANS".to_string()]),
        ),
    ]);
    let mut allied = store("DEST", "Americans");
    let landed = hit(&mut allied, &rules, "AllyBlindWH", &alliances);
    assert_eq!(landed.victim_hp, 100);
    assert_eq!(landed.cloak_state, 2, "native returns at warhead+0x179");
    assert!(
        landed.sounds.is_empty(),
        "no CloakSound: {:?}",
        landed.sounds
    );

    // Control: the SAME warhead against a non-allied owner must still land.
    // Without this the two assertions above would also hold if `AllyBlindWH`
    // failed to resolve and the record were dropped before the receiver.
    let mut hostile = store("DEST", "Americans");
    let landed = hit(
        &mut hostile,
        &rules,
        "AllyBlindWH",
        &HouseAllianceMap::new(),
    );
    assert_eq!(landed.victim_hp, 60, "the warhead resolves and does damage");
    assert_eq!(landed.cloak_state, 3);
}

fn cloak_sounds(sounds: &[SimSoundEvent]) -> Vec<&SimSoundEvent> {
    sounds
        .iter()
        .filter(|sound| matches!(sound, SimSoundEvent::CloakSound { .. }))
        .collect()
}

/// A receiver already at zero health takes native's DEATH branch, not the
/// surviving tail, so `+0xFC` is never reached for it.
///
/// Read from the disassembly — the decompiler's `switch (uVar7)` rendering of
/// this dispatch is the cross-assignment trap and says the opposite:
///
/// ```text
/// 0070202e MOV  EAX,[ESI+0x6C]   ; this->Health, AFTER the ObjectClass join
/// 00702031 TEST EAX,EAX
/// 00702033 JNZ  0x00702040
/// 00702035 MOV  EDI,0x4          ; overwrites the ObjectClass result code
/// 00702049 JMP  [EDI*4 + 0x702D24]
/// ```
///
/// The table at `0x00702D24` is `[0x007027F7, 0x00702713, 0x00702695,
/// 0x007027F7, 0x00702050]`; case 4 (`0x00702050`) returns at `0x00702692`
/// (`RET 0x1C`) without passing `0x0070281D`. `ObjectClass::ReceiveDamage @
/// 0x005F5390` does return 0 (case 0) for a corpse, but the health test above
/// discards that.
///
/// The production trigger is an ordinary multi-record blast: a `DeathWeapon`
/// cascade, a Demo Truck or an Ivan cluster whose earlier record already drove
/// the receiver to zero inside the SAME `commit_damage_events` transaction.
#[test]
fn a_second_record_of_the_same_blast_leaves_a_dead_cloaked_dolphin_submerged() {
    let rules = dolphin_rules();
    let mut entities = store("DEST", "Americans");
    // The 400 kills the 100-HP Dolphin; the 40 then lands on the corpse.
    let landed = hit_records(
        &mut entities,
        &rules,
        "SonicWH",
        &HouseAllianceMap::new(),
        &[400, 40],
    );

    assert_eq!(landed.victim_hp, 0, "the first record kills the Dolphin");
    assert_eq!(
        landed.cloak_state, 2,
        "Health == 0 forces case 4 at 0x00702035; neither record reaches vt+0xFC"
    );
    assert!(
        cloak_sounds(&landed.sounds).is_empty(),
        "a corpse emits no CloakSound: {:?}",
        landed.sounds
    );
}

/// The same rule with the death machinery out of the way: a receiver that is
/// already a corpse when the transaction opens. `receive_damage` returns
/// `reached_survivor_postlude: true` with `Unaffected` here (native does reach
/// the join — `ObjectClass::ReceiveDamage` returns 0), so only the post-join
/// health test at `0x00702031` keeps `+0xFC` away from it.
#[test]
fn a_hit_on_an_already_dead_cloaked_dolphin_never_reaches_the_uncloak() {
    let rules = dolphin_rules();
    let mut entities = store("DEST", "Americans");
    entities.get_mut(2).expect("victim present").health.current = 0;
    let landed = hit(&mut entities, &rules, "SonicWH", &HouseAllianceMap::new());

    assert_eq!(landed.victim_hp, 0, "the kernel is skipped for a corpse");
    assert_eq!(
        landed.cloak_state, 2,
        "native takes the death branch, not the surviving tail"
    );
    assert!(
        cloak_sounds(&landed.sounds).is_empty(),
        "no CloakSound: {:?}",
        landed.sounds
    );
}
