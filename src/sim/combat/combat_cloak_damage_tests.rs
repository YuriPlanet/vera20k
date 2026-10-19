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

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::cloak_disguise::CloakRuntime;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;

const TICK: u64 = 100;

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
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.set_in_playfield(true);
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
    let mut cloak = CloakRuntime::new(0);
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
    let attacker_owner = entities.get(1).map(|attacker| attacker.owner());

    let mut houses = [
        HouseState::new(soviet, 0, None, false, 0, 10),
        HouseState::new(allies, 1, None, false, 0, 10),
    ]
    .into_iter()
    .collect::<crate::sim::houses::Houses>();
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
        cloak_state: victim.cloak.as_ref().map_or(-1, |cloak| cloak.state()),
        sounds: collected,
        victim_hp: victim.health.current(),
    }
}

fn cloak_sounds(sounds: &[SimSoundEvent]) -> Vec<&SimSoundEvent> {
    sounds
        .iter()
        .filter(|sound| matches!(sound, SimSoundEvent::CloakSound { .. }))
        .collect()
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
    entities
        .get_mut(2)
        .expect("victim present")
        .health
        .set_for_test(0);
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
