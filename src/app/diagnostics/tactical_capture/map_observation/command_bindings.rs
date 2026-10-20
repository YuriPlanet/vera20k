//! Profile-only names and actor selectors, resolved at ordinary input issue.
//! Command serde remains the sole command schema and simulation owns all IDs.

use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::world::Simulation;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const MAX_SYMBOLS_PER_COMMAND: usize = 256;
const MAX_NAME_BYTES: usize = 256;

#[derive(Debug, Clone)]
pub(super) struct MapScheduledCommand {
    pub(super) issue_after_step: u32,
    pub(super) owner: String,
    /// Canonical Command, with zero placeholders only when `symbolic` is set.
    /// A symbolic command must pass through `resolve` before input scheduling.
    pub(super) payload: Command,
    pub(super) symbolic: Option<SymbolicPayload>,
}

#[derive(Debug, Clone)]
pub(super) struct SymbolicPayload {
    source: Value,
    count: usize,
}

impl<'de> Deserialize<'de> for MapScheduledCommand {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            issue_after_step: u32,
            owner: String,
            payload: Value,
        }
        let wire = Wire::deserialize(deserializer)?;
        let (payload, count) = substitute_symbols(wire.payload.clone(), |_, _| Ok(json!(0)))
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            issue_after_step: wire.issue_after_step,
            owner: wire.owner,
            payload,
            symbolic: (count != 0).then_some(SymbolicPayload {
                source: wire.payload,
                count,
            }),
        })
    }
}

impl Serialize for MapScheduledCommand {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut wire = serializer.serialize_struct("MapScheduledCommand", 3)?;
        wire.serialize_field("issue_after_step", &self.issue_after_step)?;
        wire.serialize_field("owner", &self.owner)?;
        if let Some(symbolic) = &self.symbolic {
            wire.serialize_field("payload", &symbolic.source)?;
        } else {
            wire.serialize_field("payload", &self.payload)?;
        }
        wire.end()
    }
}

impl MapScheduledCommand {
    pub(super) fn symbolic_count(&self) -> usize {
        self.symbolic.as_ref().map_or(0, |payload| payload.count)
    }

    pub(super) fn resolve(
        &self,
        sim: &Simulation,
        rules: &RuleSet,
    ) -> Result<(Command, Option<BTreeMap<String, Value>>)> {
        let Some(symbolic) = &self.symbolic else {
            return Ok((self.payload.clone(), None));
        };
        let mut bindings = BTreeMap::new();
        let (command, count) = substitute_symbols(symbolic.source.clone(), |pointer, symbol| {
            let (id, binding) = match symbol {
                Symbol::RuleType { name, super_weapon } => {
                    let category = if super_weapon {
                        ensure!(
                            rules.super_weapon_order.iter().any(|id| id == name)
                                && rules.super_weapon(name).is_some(),
                            "superweapon type {name:?} at {pointer} is absent from loaded rules"
                        );
                        "SuperWeapon"
                    } else {
                        production_type_names(rules).into_iter()
                            .find(|(_, names)| names.iter().any(|id| id == name))
                            .map(|(category, _)| category)
                            .with_context(|| format!(
                                "production type {name:?} at {pointer} is absent from loaded rules"))?
                    };
                    let id = sim.interner().get(name).with_context(|| {
                        format!("rule type {name:?} at {pointer} was not preinterned")
                    })?;
                    (
                        json!(id.index()),
                        json!({"type_id": name,
                        "interned_id": id.index(), "category": category}),
                    )
                }
                Symbol::Actor(selector) => {
                    let mut matches = sim.entities().iter_sorted().filter(|(_, entity)| {
                        entity.is_active()
                            && entity.is_alive()
                            && !entity.lifecycle.in_limbo()
                            && sim.interner().resolve(entity.type_ref()) == selector.type_id
                            && sim.interner().resolve(entity.owner()) == selector.owner
                            && selector.cell.is_none_or(|cell| {
                                cell == [entity.position.rx(), entity.position.ry()]
                            })
                    });
                    let (id, entity) = matches.next().with_context(|| format!(
                        "actor selector {selector:?} at {pointer} matched no living non-limbo actor"))?;
                    if let Some((other_id, _)) = matches.next() {
                        bail!(
                            "actor selector {selector:?} at {pointer} is ambiguous: \
                            actors {id} and {other_id} both match; specify an unambiguous cell"
                        );
                    }
                    (
                        json!(id),
                        json!({"stable_id": id,
                        "owner": sim.interner().resolve(entity.owner()),
                        "type_id": sim.interner().resolve(entity.type_ref()),
                        "cell": [entity.position.rx(), entity.position.ry()]}),
                    )
                }
            };
            bindings.insert(pointer.to_owned(), binding);
            Ok(id)
        })?;
        ensure!(
            count == symbolic.count && bindings.len() == count,
            "symbolic command binding count changed during resolution"
        );
        Ok((command, Some(bindings)))
    }
}

/// The same rules-owned production families feed the public discovery table
/// and symbolic type lookup. No new interner entries are created here.
pub(super) fn production_type_names(rules: &RuleSet) -> [(&'static str, &[String]); 4] {
    [
        ("Infantry", &rules.infantry_ids),
        ("Unit", &rules.vehicle_ids),
        ("Aircraft", &rules.aircraft_ids),
        ("Structure", &rules.building_ids),
    ]
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorSelector {
    type_id: String,
    owner: String,
    #[serde(default, deserialize_with = "super::deserialize_present")]
    cell: Option<[u16; 2]>,
}

enum Symbol<'a> {
    RuleType { name: &'a str, super_weapon: bool },
    Actor(ActorSelector),
}

fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name.len() <= MAX_NAME_BYTES,
        "symbolic names must contain 1..256 UTF-8 bytes"
    );
    Ok(())
}

fn actor_symbol(value: &Value) -> Result<Symbol<'_>> {
    let selector: ActorSelector =
        serde_json::from_value(value.clone()).context("invalid actor selector")?;
    validate_name(&selector.type_id)?;
    validate_name(&selector.owner)?;
    Ok(Symbol::Actor(selector))
}

// Symbols decorate only existing Command fields. After substitution the one
// Command deserializer checks variant identity, argument types and widths;
// canonical equality rejects any extra field it would otherwise discard.
fn substitute_symbols(
    mut value: Value,
    mut resolve: impl FnMut(&str, Symbol<'_>) -> Result<Value>,
) -> Result<(Command, usize)> {
    let mut count = 0;
    let mut bind = |pointer: &str, symbol: Symbol<'_>| -> Result<Value> {
        count += 1;
        ensure!(
            count <= MAX_SYMBOLS_PER_COMMAND,
            "command exceeds 256 symbolic bindings"
        );
        resolve(pointer, symbol)
    };
    if let Some(variants) = value.as_object_mut().filter(|object| object.len() == 1) {
        let (variant, arguments) = variants.iter_mut().next().expect("one variant");
        if let Some(arguments) = arguments.as_object_mut() {
            for (field, argument) in arguments {
                let pointer = format!("/{variant}/{field}");
                if matches!(field.as_str(), "type_id" | "sw_type_id") && argument.is_string() {
                    let name = argument.as_str().expect("string checked");
                    validate_name(name)?;
                    *argument = bind(
                        &pointer,
                        Symbol::RuleType {
                            name,
                            super_weapon: field == "sw_type_id",
                        },
                    )?;
                } else if matches!(
                    field.as_str(),
                    "entity_id"
                        | "attacker_id"
                        | "target_id"
                        | "engineer_id"
                        | "target_building_id"
                        | "transport_id"
                ) && argument.is_object()
                {
                    *argument = bind(&pointer, actor_symbol(argument)?)?;
                } else if variant == "MegaMission"
                    && field == "destination"
                    && let Some(entity) = argument
                        .as_object_mut()
                        .and_then(|destination| destination.get_mut("Entity"))
                        .filter(|entity| entity.is_object())
                {
                    *entity = bind(&format!("{pointer}/Entity"), actor_symbol(entity)?)?;
                } else if ((variant == "Select" && field == "entity_ids")
                    || (variant == "SetRally" && field == "producer_ids"))
                    && let Some(elements) = argument.as_array_mut()
                {
                    for (index, element) in elements.iter_mut().enumerate() {
                        if element.is_object() {
                            *element = bind(&format!("{pointer}/{index}"), actor_symbol(element)?)?;
                        }
                    }
                }
            }
        }
    }
    let command: Command = serde_json::from_value(value.clone())?;
    ensure!(
        value == serde_json::to_value(&command)?,
        "command payload has unrecognized or noncanonical fields"
    );
    Ok((command, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    fn rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=400\n\
             [BuildingTypes]\n0=GAPOWR\n[GAPOWR]\nStrength=750\n\
             [SuperWeaponTypes]\n0=IronCurtainSpecial\n\
             [IronCurtainSpecial]\nType=IronCurtain\n",
        ))
        .unwrap()
    }

    fn scheduled(payload: Value) -> MapScheduledCommand {
        serde_json::from_value(json!({"issue_after_step": 2,
            "owner": "Computer1", "payload": payload}))
        .unwrap()
    }

    fn add_actor(sim: &mut Simulation, id: u64, cell: [u16; 2]) {
        let owner = sim.intern("Computer1");
        let type_ref = sim.intern("MTNK");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            id,
            cell[0],
            cell[1],
            0,
            0,
            owner,
            Health::new(400),
            type_ref,
            EntityCategory::Unit,
            0,
            4,
            true,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        sim.entities_mut_for_test().insert(entity);
    }

    #[test]
    fn symbolic_rule_names_follow_each_loaded_interner_and_reject_unlisted_strings() {
        let rules = rules();
        let mut first = Simulation::new();
        first.intern_rule_type_ids(&rules);
        let mut second = Simulation::new();
        second.intern("ORCA");
        second.intern_rule_type_ids(&rules);
        let command = scheduled(json!({"LaunchSuperWeapon": {
            "sw_type_id": "IronCurtainSpecial", "target_rx": 3, "target_ry": 4}}));
        let (first_command, first_bindings) = command.resolve(&first, &rules).unwrap();
        let (second_command, second_bindings) = command.resolve(&second, &rules).unwrap();
        assert_ne!(
            first_command, second_command,
            "adding a type must not silently launch a different numeric type"
        );
        assert_eq!(
            first_command,
            Command::LaunchSuperWeapon {
                sw_type_id: first.interner().get("IronCurtainSpecial").unwrap(),
                target_rx: 3,
                target_ry: 4,
            }
        );
        assert_eq!(
            second_command,
            Command::LaunchSuperWeapon {
                sw_type_id: second.interner().get("IronCurtainSpecial").unwrap(),
                target_rx: 3,
                target_ry: 4,
            }
        );
        assert_eq!(
            first_bindings.unwrap()["/LaunchSuperWeapon/sw_type_id"]["category"],
            "SuperWeapon"
        );
        assert_eq!(
            second_bindings.unwrap()["/LaunchSuperWeapon/sw_type_id"]["type_id"],
            "IronCurtainSpecial"
        );
        let production = scheduled(json!({"QueueProduction": {"type_id": "MTNK"}}));
        let (resolved, bindings) = production.resolve(&second, &rules).unwrap();
        assert_eq!(
            resolved,
            Command::QueueProduction {
                type_id: second.interner().get("MTNK").unwrap(),
            }
        );
        assert_eq!(
            bindings.unwrap()["/QueueProduction/type_id"]["category"],
            "Unit"
        );
        for payload in [
            json!({"QueueProduction": {"type_id": "ORCA"}}),
            json!({"QueueProduction": {"type_id": "IronCurtainSpecial"}}),
            json!({"LaunchSuperWeapon": {"sw_type_id": "MTNK", "target_rx": 3, "target_ry": 4}}),
        ] {
            assert!(scheduled(payload).resolve(&second, &rules).is_err());
        }
        let mut absent = Simulation::new();
        absent.intern("Computer1");
        assert!(production.resolve(&absent, &rules).is_err());
        assert!(
            absent.interner().get("MTNK").is_none(),
            "resolution must never intern"
        );
    }

    #[test]
    fn actor_selectors_require_exactly_one_live_actor_and_resolve_at_issue_time() {
        let rules = rules();
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        let command = scheduled(json!({"Stop": {"entity_id": {
            "type_id": "MTNK", "owner": "Computer1"}}}));
        assert!(command.resolve(&sim, &rules).is_err(), "not produced yet");
        add_actor(&mut sim, 17, [10, 11]);
        assert_eq!(
            command.resolve(&sim, &rules).unwrap().0,
            Command::Stop { entity_id: 17 }
        );
        add_actor(&mut sim, 28, [12, 13]);
        assert!(
            command
                .resolve(&sim, &rules)
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
        let narrowed = scheduled(json!({"Stop": {"entity_id": {
            "type_id": "MTNK", "owner": "Computer1", "cell": [12, 13]}}}));
        let (resolved, bindings) = narrowed.resolve(&sim, &rules).unwrap();
        assert_eq!(resolved, Command::Stop { entity_id: 28 });
        assert_eq!(
            bindings.unwrap()["/Stop/entity_id"],
            json!({
            "stable_id": 28, "type_id": "MTNK", "owner": "Computer1", "cell": [12, 13]})
        );
        sim.entities_mut_for_test()
            .get_mut(28)
            .unwrap()
            .position
            .set_rx_for_test(14);
        assert!(
            narrowed.resolve(&sim, &rules).is_err(),
            "cell is current, not initial"
        );
        sim.entities_mut_for_test()
            .get_mut(28)
            .unwrap()
            .health
            .set_for_test(0);
        assert_eq!(
            command.resolve(&sim, &rules).unwrap().0,
            Command::Stop { entity_id: 17 }
        );
        sim.entities_mut_for_test()
            .get_mut(17)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(true);
        assert!(command.resolve(&sim, &rules).is_err());
        sim.entities_mut_for_test()
            .get_mut(17)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(false);
        sim.entities_mut_for_test()
            .get_mut(17)
            .unwrap()
            .death
            .set_dying_for_test(true);
        assert!(command.resolve(&sim, &rules).is_err());
    }

    #[test]
    fn selectors_bind_each_supported_actor_list_element_without_altering_numeric_arguments() {
        let rules = rules();
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        add_actor(&mut sim, 17, [10, 11]);
        let selector = json!({"type_id": "MTNK", "owner": "Computer1"});
        for (payload, pointer, expected) in [
            (
                json!({"Select": {"entity_ids": [7, selector.clone()], "additive": true}}),
                "/Select/entity_ids/1",
                Command::Select {
                    entity_ids: vec![7, 17],
                    additive: true,
                },
            ),
            (
                json!({"SetRally": {"producer_ids": [selector], "rx": 25, "ry": 26}}),
                "/SetRally/producer_ids/0",
                Command::SetRally {
                    producer_ids: vec![17],
                    rx: 25,
                    ry: 26,
                },
            ),
        ] {
            let command = scheduled(payload.clone());
            let wire = serde_json::to_value(&command).unwrap();
            assert_eq!(wire["payload"], payload);
            let (resolved, bindings) = command.resolve(&sim, &rules).unwrap();
            assert_eq!(resolved, expected);
            let bindings = bindings.unwrap();
            assert_eq!(bindings.len(), 1);
            assert_eq!(bindings[pointer]["stable_id"], 17);
        }
    }

    /// An Enter order's object rides the MEGAMISSION's Destination token, so
    /// a selector there binds as an actor ID field does.
    #[test]
    fn a_megamission_destination_binds_an_actor_selector() {
        let rules = rules();
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        add_actor(&mut sim, 17, [10, 11]);
        let payload = json!({"MegaMission": {"entity_id": 7, "mission": "Enter",
            "target": null, "destination": {"Entity": {
            "type_id": "MTNK", "owner": "Computer1"}}}});
        let command = scheduled(payload.clone());
        assert_eq!(command.symbolic_count(), 1);
        assert_eq!(serde_json::to_value(&command).unwrap()["payload"], payload);
        let (resolved, bindings) = command.resolve(&sim, &rules).unwrap();
        assert_eq!(
            resolved,
            Command::clicked_mission(
                7,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(17),
            )
        );
        assert_eq!(
            bindings.unwrap()["/MegaMission/destination/Entity"]["stable_id"],
            17
        );
    }

    #[test]
    fn numeric_payloads_keep_historical_roundtrip_and_have_no_bindings() {
        let value = json!({"issue_after_step": 2, "owner": "Computer1",
            "payload": {"Move": {"entity_id": 19, "target_rx": 20, "target_ry": 21}}});
        let command: MapScheduledCommand = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(command.symbolic_count(), 0);
        assert_eq!(serde_json::to_value(&command).unwrap(), value);
        let (resolved, bindings) = command.resolve(&Simulation::new(), &rules()).unwrap();
        assert_eq!(resolved, command.payload);
        assert!(bindings.is_none());
        for payload in [
            json!({"Stop": {"entity_id": 1, "ignored": true}}),
            json!({"Stop": {"entity_id": 1.0}}),
            json!({"Select": {"entity_ids": [1.0], "additive": false}}),
        ] {
            assert!(
                serde_json::from_value::<MapScheduledCommand>(json!({
                "issue_after_step": 2, "owner": "Computer1", "payload": payload}))
                .is_err()
            );
        }
    }

    #[test]
    fn selectors_reject_ignored_null_unbounded_and_wrongly_typed_arguments() {
        let selector = json!({"type_id": "MTNK", "owner": "Computer1"});
        let mut invalid_selectors = vec![
            json!({"type_id": "MTNK"}),
            json!({"type_id": "", "owner": "Computer1"}),
            json!({"type_id": "MTNK", "owner": "Computer1", "cell": null}),
            json!({"type_id": "MTNK", "owner": "Computer1", "cell": [10.0, 11]}),
            json!({"type_id": "MTNK", "owner": "Computer1", "cell": [10, 11], "nearest": true}),
        ];
        invalid_selectors.push(json!({"type_id": "MTNK", "owner": "x".repeat(257)}));
        invalid_selectors.push(json!({"type_id": "MTNK", "owner": "é".repeat(129)}));
        for selector in invalid_selectors {
            assert!(
                serde_json::from_value::<MapScheduledCommand>(json!({
                "issue_after_step": 0, "owner": "Computer1",
                "payload": {"Stop": {"entity_id": selector}}}))
                .is_err()
            );
        }
        for payload in [
            json!({"Select": {"entity_ids": vec![selector; 257], "additive": false}}),
            json!({"Move": {"entity_id": 1, "target_rx": "MTNK", "target_ry": 2}}),
            json!({"QueueProduction": {"type_id": "", "ignored": true}}),
        ] {
            assert!(
                serde_json::from_value::<MapScheduledCommand>(json!({
                "issue_after_step": 0, "owner": "Computer1", "payload": payload}))
                .is_err()
            );
        }
    }
}
