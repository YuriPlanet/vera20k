//! Bounded comparison of the production classifier's Unit post-latch tail.
//! The oracle supplies the preceding accumulator/latch. These fixtures realize
//! them with a bridge-list infantry victim and (for codes5..7) a real blocker.
//! Listed native Units are ignored by that earlier walk so the comparison does
//! not claim parity for the still-approximate object-walk producer. The tail
//! must independently find them in GROUND order, including ignored/self IDs.

use super::*;
use crate::sim::occupancy::CellListInsertion;
use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
use serde_json::Value;

const CELL: (u16, u16) = (5, 5);
const MOVER: u64 = 1;
const VICTIM: u64 = 100;

struct Fixture {
    entities: EntityStore,
    occupancy: OccupancyGrid,
    identity: CellOccupationGrid,
    raw: RawCellOccupationGrid,
    ignored: BTreeSet<u64>,
    alliances: HouseAllianceMap,
    layers: CanEnterLayerContext,
    capability: bump_crush::CrushCapability,
}

impl Fixture {
    fn new() -> Self {
        let mut entities = EntityStore::new();
        let mut mover = GameEntity::test_default(MOVER, "GTNK", "Americans", 4, 5);
        mover.category = EntityCategory::Unit;
        mover.regular_crusher = true;
        mover.lifecycle.object_alive = true;
        mover.lifecycle.in_limbo = false;
        entities.insert(mover);
        let mut victim = GameEntity::test_default(VICTIM, "E1", "VictimHouse", 5, 5);
        victim.category = EntityCategory::Infantry;
        victim.crushable = true;
        victim.sub_cell = Some(2);
        victim.lifecycle.object_alive = true;
        victim.lifecycle.in_limbo = false;
        entities.insert(victim);
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            5,
            5,
            VICTIM,
            MovementLayer::Bridge,
            Some(2),
            CellListInsertion::AppendBuilding,
        );
        Self {
            entities,
            occupancy,
            identity: CellOccupationGrid::new(),
            raw: RawCellOccupationGrid::new(),
            ignored: BTreeSet::new(),
            alliances: HouseAllianceMap::new(),
            layers: CanEnterLayerContext {
                terrain_layer: MovementLayer::Bridge,
                object_list_layer: MovementLayer::Bridge,
                occupancy_bits_layer: MovementLayer::Ground,
            },
            capability: bump_crush::CrushCapability::new(true, false),
        }
    }

    fn classify(&self) -> CellEntryResult {
        classify_occupied_cell_with_layers_and_ignored_and_occupation(
            CELL,
            self.layers,
            MOVER,
            self.capability,
            "Americans",
            LocomotorKind::Drive,
            false,
            Some(&self.ignored),
            &self.occupancy,
            &self.identity,
            &self.raw,
            100,
            &self.entities,
            &self.alliances,
            &crate::sim::intern::test_interner(),
        )
    }

    fn insert(&mut self, entity: GameEntity, layer: MovementLayer) {
        let id = entity.stable_id();
        let sub = entity.sub_cell;
        self.entities.insert(entity);
        self.occupancy
            .add(5, 5, id, layer, sub, CellListInsertion::AppendBuilding);
    }

    fn state(&self) -> Vec<(u64, i32, bool, bool)> {
        self.entities
            .iter_sorted()
            .map(|(id, entity)| {
                (
                    id,
                    entity.health.current,
                    entity.lifecycle.object_alive,
                    entity.lifecycle.in_limbo,
                )
            })
            .collect()
    }
}

fn flag(node: &Value, key: &str) -> bool {
    node[key].as_bool().unwrap_or(false)
}

#[test]
fn runtime_unit_crush_tail_matches_original_continuation_corpus() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/cell_entry_crush_tail.json"
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 68);
    let mut compared = 0;
    for case in cases {
        let row = &case["input"];
        let code = row["running_code"].as_u64().unwrap();
        // This runtime object walk has no producer for native code1 or wall4;
        // codes2/3 require other locomotor/gate fixtures. The native corpus
        // retains all seven nonzero accumulator cases without asserting their
        // producers are reproduced by this bounded comparison.
        if !matches!(code, 0 | 5 | 6 | 7) {
            continue;
        }
        let mut f = Fixture::new();
        f.capability.omni_crusher = flag(row, "omni");
        f.entities.get_mut(MOVER).unwrap().omni_crusher = f.capability.omni_crusher;
        f.raw
            .mark_ground(5, 5, row["raw_ground"].as_u64().unwrap() as u8);
        f.raw
            .mark_deck(5, 5, row["raw_deck"].as_u64().unwrap() as u8);
        if row["bits_layer"] == "deck" {
            f.layers.occupancy_bits_layer = MovementLayer::Bridge;
        }
        let mut next_id = 10;
        for (key, layer) in [
            ("ground", MovementLayer::Ground),
            ("deck", MovementLayer::Bridge),
        ] {
            for node in row[key].as_array().unwrap() {
                if flag(node, "self") {
                    f.occupancy
                        .add(5, 5, MOVER, layer, None, CellListInsertion::AppendBuilding);
                    continue;
                }
                let owner = if flag(node, "allied") {
                    "French"
                } else {
                    "Soviets"
                };
                if flag(node, "allied") {
                    f.alliances
                        .entry("AMERICANS".into())
                        .or_default()
                        .insert("FRENCH".into());
                }
                if flag(node, "reverse_allied") {
                    f.alliances
                        .entry("SOVIETS".into())
                        .or_default()
                        .insert("AMERICANS".into());
                }
                let mut entity = GameEntity::test_default(next_id, "HTNK", owner, 5, 5);
                entity.category = if node["category"] == "infantry" {
                    entity.sub_cell = Some(0);
                    EntityCategory::Infantry
                } else {
                    EntityCategory::Unit
                };
                entity.crushable = flag(node, "crushable");
                entity.omni_crush_resistant = flag(node, "resistant");
                entity.lifecycle.object_alive = true;
                entity.lifecycle.in_limbo = false;
                entity.invulnerability = match node["timer"].as_str() {
                    Some("active") => Some(InvulnerabilityState {
                        start_frame: 90,
                        duration_frames: 20,
                        kind: InvulnKind::IronCurtain,
                    }),
                    Some("expired") => Some(InvulnerabilityState {
                        start_frame: 90,
                        duration_frames: 10,
                        kind: InvulnKind::ForceShield,
                    }),
                    _ => None,
                };
                f.insert(entity, layer);
                f.ignored.insert(next_id);
                next_id += 1;
            }
        }
        if code != 0 {
            let mut blocker = GameEntity::test_default(
                200,
                "HTNK",
                if code == 5 { "Soviets" } else { "Americans" },
                5,
                5,
            );
            blocker.category = if code == 7 {
                EntityCategory::Structure
            } else {
                EntityCategory::Unit
            };
            f.insert(blocker, MovementLayer::Bridge);
        }
        let before = f.state();
        let result = f.classify();
        assert_eq!(
            u64::from(result.yr_code()),
            case["result"].as_u64().unwrap(),
            "{}: {result:?}",
            row["name"]
        );
        if result.yr_code() == 0 {
            assert!(
                matches!(&result, CellEntryResult::Crushable { victims } if victims.contains(&VICTIM)),
                "{}",
                row["name"]
            );
        }
        assert_eq!(
            f.state(),
            before,
            "admission must not kill: {}",
            row["name"]
        );
        assert!(
            f.occupancy
                .get(5, 5)
                .unwrap()
                .iter_layer(MovementLayer::Bridge)
                .any(|o| o.entity_id == VICTIM)
        );
        assert_eq!(case["object_and_cell_memory_unchanged"], true);
        compared += 1;
    }
    assert_eq!(compared, 64);
}

#[test]
fn raw_self_claim_is_not_subtracted_and_identity_claims_do_not_supply_raw_bits() {
    let mut f = Fixture::new();
    f.identity
        .mark_vehicle_on_layer(5, 5, MOVER, MovementLayer::Ground);
    f.raw.mark_ground(5, 5, 0x20);
    assert_eq!(f.classify(), CellEntryResult::TemporaryOccupation);
    f.raw.clear_ground(5, 5, 0x20);
    f.identity
        .mark_vehicle_on_layer(5, 5, 999, MovementLayer::Ground);
    assert!(matches!(f.classify(), CellEntryResult::Crushable { .. }));
}

#[test]
fn raw_vehicle_only_runs_the_same_central_classifier_without_an_object_blocker() {
    let mut f = Fixture::new();
    f.occupancy = OccupancyGrid::new();
    f.raw.mark_ground(5, 5, 0x20);
    assert_eq!(f.classify(), CellEntryResult::TemporaryOccupation);
    f.raw.clear_ground(5, 5, 0x20);
    assert_eq!(f.classify(), CellEntryResult::Clear);
}

#[test]
fn infantry_mover_does_not_acquire_the_unit_crush_latch() {
    let mut f = Fixture::new();
    f.entities.get_mut(MOVER).unwrap().category = EntityCategory::Infantry;
    assert_eq!(
        f.classify(),
        CellEntryResult::OccupiedEnemy { blocker_id: VICTIM }
    );
    assert!(f.entities.get(VICTIM).unwrap().is_object_alive());
}

#[test]
fn unignored_higher_blocker_codes_survive_mixed_crush_and_raw_vehicle_occupation() {
    for (category, owner, code) in [
        (EntityCategory::Unit, "Soviets", 5),
        (EntityCategory::Unit, "Americans", 6),
        (EntityCategory::Structure, "Americans", 7),
    ] {
        let mut f = Fixture::new();
        f.raw.mark_ground(5, 5, 0x20);
        let mut blocker = GameEntity::test_default(200, "HTNK", owner, 5, 5);
        blocker.category = category;
        f.insert(blocker, MovementLayer::Bridge);
        let before = f.state();
        assert_eq!(f.classify().yr_code(), code);
        assert_eq!(f.state(), before);
    }
}
