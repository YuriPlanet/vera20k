//! Original MGTK ReceiveDamage737C90 through the production damage and HP owners.
//!
//! Comparisons stop at the health commit: packet, result, HP and retained
//! disguise fields. The lethal native row also executes later death effects;
//! their lifecycle changes and RNG draws are outside this comparison.

use super::{
    EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, receiver_health, resolve_receive_damage,
};
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::sim::cloak_disguise::DisguiseRuntime;
use crate::sim::components::DriveCoord;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::{StringInterner, test_interner};
use crate::sim::timer::CdTimer;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn corpus() -> Value {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/mirage_disguise.json",
    ))
    .expect("original Mirage disguise corpus");
    assert_eq!(native["schema_version"], 1);
    assert_eq!(
        native["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    native
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn word(value: &Value) -> u32 {
    u32::try_from(value.as_u64().unwrap()).unwrap()
}

fn restore_disguise(native: &Value, interner: &mut StringInterner) -> DisguiseRuntime {
    let type_id = native["type"].as_str().map(|name| interner.intern(name));
    let house_pointer = native["house_pointer"].as_str().unwrap();
    let house = (house_pointer != "0x0").then(|| interner.intern(house_pointer));
    // Supply the observed native prior through the existing restore boundary.
    // In particular, raw=false may retain a type/house identity. Reconstructing
    // that state with acquire/clear would invoke the consequence under test.
    // The unnamed +1E4 stack word is deliberately absent from Rust state.
    serde_json::from_value(json!({
        "disguised": native["disguised"] == 1,
        "disguise_creation_frame": native["creation_frame"],
        "disguise_type": type_id,
        "disguised_as_house": house,
        "reveal_timer": CdTimer::from_raw(
            int(&native["reveal_start"]), int(&native["reveal_duration"])
        ),
    }))
    .expect("native disguise prior fits the retained-state schema")
}

fn assert_identity(
    state: &DisguiseRuntime,
    expected: &Value,
    interner: &StringInterner,
    context: &str,
) {
    assert_eq!(
        json!(u8::from(state.is_disguised())),
        expected["disguised"],
        "{context}"
    );
    assert_eq!(
        json!(state.creation_frame()),
        expected["creation_frame"],
        "{context}"
    );
    assert_eq!(
        json!(state.type_id().map(|id| interner.resolve(id))),
        expected["type"],
        "{context}: retained type identity"
    );
    assert_eq!(
        state.house().map_or("0x0", |id| interner.resolve(id)),
        expected["house_pointer"].as_str().unwrap(),
        "{context}: retained house identity"
    );
}

fn assert_disguise(
    state: &DisguiseRuntime,
    expected: &Value,
    interner: &StringInterner,
    context: &str,
) {
    assert_identity(state, expected, interner, context);
    assert_eq!(
        state.reveal_timer().start_frame(),
        int(&expected["reveal_start"]),
        "{context}"
    );
    assert_eq!(
        state.reveal_timer().duration(),
        int(&expected["reveal_duration"]),
        "{context}"
    );
}

fn rules_for_control(root: &IniFile, art: &IniFile, input: &Value) -> RuleSet {
    let mut layers = RulesLayerStack::new(root.clone());
    // The native controls change only these two type bytes. Keep the real
    // MGTK/AP values and use the production reader for the supplied gates.
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(&format!(
            "[MGTK]\nCanDisguise={}\nPermaDisguise={}\n",
            word(&input["can_disguise"]),
            word(&input["perma_disguise"]),
        )),
    );
    let processed = layers.process_with_fixed_art(art).unwrap();
    RuleSet::from_processed_rules(&processed).unwrap()
}

#[test]
fn damage_receiver_matches_original_packet_result_and_disguise_consequence() {
    let Some(raw) = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini") else {
        return;
    };
    let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
        return;
    };
    let native = corpus();
    assert_eq!(
        crate::util::sha256::sha256_hex(&raw),
        native["retail"]["layers"][0]["sha256"],
        "physical MGTK and AP source identity"
    );
    let root = IniFile::from_bytes(&raw).unwrap();
    let rows = native["damage"].as_array().unwrap();
    assert!(rows.iter().any(|row| row["name"] == "lethal_ignore"));
    for row in rows {
        let context = row["name"].as_str().unwrap();
        let input = &row["input"];
        let before = &row["before"];
        let after = &row["after"];
        let frame = word(&row["frame"]);
        let rules = rules_for_control(&root, &art, input);
        let object_type = rules.object("MGTK").unwrap();
        assert_eq!(
            json!(u8::from(object_type.can_disguise)),
            input["can_disguise"]
        );
        assert_eq!(
            json!(u8::from(object_type.perma_disguise)),
            input["perma_disguise"]
        );
        let owner_type = input["owner_type"].as_str().unwrap();
        assert_eq!(
            rules
                .country_armor_mult_for_type(owner_type, object_type)
                .to_bits(),
            word(&input["house_type_armor_units_bits"]),
            "{context}: actual native HouseType divisor"
        );

        let mut target = GameEntity::test_default(1, "MGTK", owner_type, 0, 0);
        let mut interner = test_interner();
        target.health.current = int(&before["health"]);
        target.lifecycle.object_alive = before["alive"] == 1;
        target.lifecycle.in_limbo = before["limbo"] == 1;
        target.veterancy_raw = NativeF32Bits::from_bits(word(&input["veterancy_bits"]));
        target.armor_multiplier = NativeF64Bits::from_bits(
            u64::from_str_radix(
                input["armor_multiplier_bits"]
                    .as_str()
                    .unwrap()
                    .trim_start_matches("0x"),
                16,
            )
            .unwrap(),
        );
        crate::sim::movement::ground_pose::put_location(
            &mut target.position,
            DriveCoord {
                x: int(&before["xyz"][0]),
                y: int(&before["xyz"][1]),
                z: int(&before["xyz"][2]),
            },
        );
        target.disguise = Some(restore_disguise(before, &mut interner));
        assert_disguise(
            target.disguise.as_ref().unwrap(),
            before,
            &interner,
            context,
        );
        let owner = target.owner();
        let houses = BTreeMap::from([(owner, HouseState::new(owner, 0, Some(owner), false, 0, 0))]);
        let mut entities = EntityStore::new();
        entities.insert(target);
        let warhead = interner.intern(input["warhead"].as_str().unwrap());
        // The original receiver controls have no attacker or sourceHouse.
        // Preserve that ABI instead of introducing a retaliation source.
        assert_eq!(row["args"][3], 0, "{context}: attacker");
        assert_eq!(row["args"][6], 0, "{context}: sourceHouse");
        let event = EntityDamageEvent::direct_receiver(
            1,
            int(&input["damage"]),
            int(&row["args"][1]),
            RAD_NO_ATTACKER,
            None,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: input["ignore_defenses"].as_bool().unwrap(),
                arg6: row["args"][5] != 0,
            },
        );
        let alliances = HouseAllianceMap::new();
        let resolved = resolve_receive_damage(
            &event,
            &entities,
            &rules,
            &interner,
            &houses,
            &alliances,
            false,
            u64::from(frame),
            None,
        )
        .expect("physical MGTK/AP receiver resolves");
        let receipt = receiver_health::commit_receiver_health(
            &event,
            &mut entities,
            &rules,
            &interner,
            &alliances,
            None,
            None,
            Some(resolved),
            frame,
        )
        .expect("ordinary Object/Techno health commit");
        let target = entities.get(1).unwrap();
        assert_eq!(
            receipt.state as i32,
            int(&row["returned_eax"]),
            "{context}: native result"
        );
        assert_eq!(
            target.health.current,
            int(&after["health"]),
            "{context}: committed HP"
        );
        assert_disguise(target.disguise.as_ref().unwrap(), after, &interner, context);
        let native_packet = int(&row["modified_damage"]);
        if let Some((packet, _, _)) = receipt.positive_postlude {
            // This is the Object owner's final packet, including lethal
            // overkill truncation. Never derive it from authored damage.
            assert_eq!(packet, native_packet, "{context}: final damage packet");
        } else {
            // Zero/healing do not rewrite the prepared packet. The shared
            // signed Object receiver's separate corpus covers that boundary.
            assert!(
                native_packet <= 0,
                "{context}: missing positive packet receipt"
            );
            assert_eq!(
                resolved.outcome.post_object_damage,
                Some(native_packet),
                "{context}"
            );
        }
        if row["name"] == "lethal_ignore" {
            assert!(
                receipt.entered_techno_death,
                "{context}: staged death continuation"
            );
            assert_eq!(
                target.disguise.as_ref().unwrap(),
                &restore_disguise(before, &mut interner),
                "result4 skips every disguise/reveal mutation"
            );
        }
    }
}

#[test]
fn damage_clear_dispatch_matches_original_unit_and_nonpermanent_identity_retention() {
    let native = corpus();
    let damage = native["damage"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "five_ignore")
        .unwrap();
    for (name, category) in [
        ("unit", EntityCategory::Unit),
        ("infantry", EntityCategory::Infantry),
        ("building", EntityCategory::Structure),
        ("aircraft", EntityCategory::Aircraft),
    ] {
        let row = native["clear"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap();
        let mut interner = StringInterner::new();
        let mut state = restore_disguise(&row["before"], &mut interner);
        // These original direct Clear calls establish only class-specific
        // identity retention. Compare that part of the shared damage writer;
        // its timer write is covered by the actual MGTK damage rows above.
        // Building/Aircraft used supplied vtable-shaped native buffers; these
        // rows do not establish their class construction or full receivers.
        // The permanent Infantry branch is a separate, unported mechanism.
        state.receive_damage_reveal(
            word(&damage["frame"]),
            int(&damage["modified_damage"]),
            category,
        );
        assert_identity(&state, &row["after"], &interner, name);
    }
}
