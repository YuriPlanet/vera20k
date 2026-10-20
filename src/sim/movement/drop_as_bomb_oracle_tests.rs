//! Native DropAsBomb/ObjectAI fall corpus checks.
//!
//! The corpus is produced by `hover_speed_altitude.py --drop-as-bomb` and its
//! native sidecar pins the retail executable. These checks exercise the shared
//! object fall state and x87 free-fall conversion without duplicating the
//! production implementation. The caller module includes this file under
//! `#[cfg(test)]`.

#[cfg(test)]
mod tests {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::{FrameEffects, Simulation};

    fn vectors() -> Vec<serde_json::Value> {
        serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/drop_as_bomb.json",
        ))
        .expect("drop-as-bomb native corpus")
    }

    fn int(row: &serde_json::Value, key: &str, default: i32) -> i32 {
        row.get(key)
            .and_then(serde_json::Value::as_i64)
            .map_or(default, |value| value as i32)
    }

    #[test]
    fn production_object_ai_free_fall_matches_native_rate_and_caps() {
        for row in vectors().into_iter().filter(|row| {
            row["input"]["kind"] == "fall"
                && matches!(
                    row["input"]["name"].as_str().unwrap(),
                    "free_rate_0"
                        | "free_rate_-1"
                        | "free_rate_-2"
                        | "free_rate_-98"
                        | "free_rate_-99"
                        | "free_rate_-100"
                        | "free_rate_2"
                        | "custom_free_cap"
                        | "unmarked"
                        | "dead_grounding"
                        | "negative_health"
                        | "parachute_landing"
                        | "not_falling_bomb"
                        | "limbo_keeps_rate"
                )
        }) {
            let input = &row["input"];
            let mut rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
            rules.general.parachute_max_fall_rate = int(input, "parachute_cap", -3);
            rules.general.no_parachute_max_fall_rate = int(input, "free_cap", -100);
            let mut sim = Simulation::new();
            sim.interner = crate::sim::intern::test_interner();
            let mut entity = GameEntity::test_default(1, "E1", "Americans", 10, 10);
            let z = int(input, "z", 416);
            if input["parachuted"].as_i64() == Some(1) {
                entity.begin_parachute_fall(z);
            } else {
                entity.position.set_exact_z_leptons_for_test(Some(z));
                entity.begin_bomb_fall();
            }
            entity.set_fall_rate_for_test(int(input, "rate", 0));
            if int(input, "falling", 1) == 0 {
                entity.finish_fall();
            }
            entity
                .lifecycle
                .set_in_limbo_for_test(int(input, "limbo", 0) != 0);
            entity.health.set_for_test(int(input, "health", 100));
            sim.substrate.entities.insert(entity);
            sim.object_ai_fall(1, &rules, None, FrameEffects::default())
                .expect("production object falling AI");
            let entity = sim
                .substrate
                .entities
                .get(1)
                .expect("fall control retained");
            assert_eq!(
                entity.fall_rate(),
                int(&row, "rate", 0),
                "{}",
                input["name"]
            );
            assert_eq!(
                entity.position.exact_z_leptons(),
                Some(int(&row, "z", 0)),
                "{}",
                input["name"]
            );
            assert_eq!(
                entity.is_falling_down(),
                row["falling"].as_bool().unwrap(),
                "{}",
                input["name"]
            );
        }
    }

    #[test]
    fn production_drop_preserves_native_rate_z_and_lifecycle_bits() {
        let vectors = vectors();
        let row = vectors
            .iter()
            .find(|row| row["input"]["name"] == "standing_rate_kept")
            .unwrap();
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "E1", "Americans", 10, 10);
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.position.set_exact_z_leptons_for_test(Some(416));
        entity.set_on_bridge(true);
        entity.set_fall_rate_for_test(int(&row["input"], "rate", 0));
        sim.substrate.entities.insert(entity);
        sim.drop_as_bomb(1, crate::sim::world::UninitContext::default());
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(entity.is_falling_down(), row["falling"].as_bool().unwrap());
        assert_eq!(entity.was_dropped_as_bomb(), row["bomb"].as_bool().unwrap());
        assert_eq!(entity.on_bridge(), row["bridge"].as_bool().unwrap());
        assert_eq!(entity.position.exact_z_leptons(), Some(int(row, "z", 0)));
        assert_eq!(entity.fall_rate(), int(row, "rate", 0));
        assert!(!entity.is_parachuted());
    }

    #[test]
    fn retained_fall_rate_and_bomb_state_survive_snapshot_and_affect_hash() {
        use crate::sim::house_state::HouseState;
        use crate::sim::rng::SimRng;
        use crate::sim::snapshot::GameSnapshot;

        let mut sim = Simulation::new();
        sim.interner = crate::sim::intern::test_interner();
        sim.scenario_rng = SimRng::new(0);
        let house = sim.interner.intern("Americans");
        sim.houses
            .register(HouseState::new(house, 0, None, true, 0, 10));
        let id = sim.allocate_stable_id();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(id, "E1", "Americans", 10, 10));
        let plain = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_fall_rate_for_test(-17);
        let rate_only = sim.state_hash();
        assert_ne!(plain, rate_only, "retained inactive rate affects the hash");
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.begin_bomb_fall();
        entity.finish_fall();
        assert_ne!(
            rate_only,
            sim.state_hash(),
            "inactive bomb provenance affects the hash"
        );

        let bytes = GameSnapshot::save(&sim, 0, 0, "retained object fall", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        let entity = restored.substrate.entities.get(id).unwrap();
        assert!(!entity.is_falling_down());
        assert!(!entity.is_parachuted());
        assert!(entity.was_dropped_as_bomb());
        assert_eq!(entity.fall_rate(), -17);
        assert_eq!(restored.state_hash(), sim.state_hash());

        restored
            .substrate
            .entities
            .get_mut(id)
            .unwrap()
            .begin_bomb_fall();
        assert_eq!(
            restored.substrate.entities.get(id).unwrap().fall_rate(),
            -17,
            "a later DropAsBomb resumes the saved rate"
        );
    }
}
