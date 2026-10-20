use super::*;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::superweapon::{SuperWeaponInstance, superweapon_views_for_owner};
use crate::sim::world::Simulation;
use crate::ui::gadget::focus::FocusState;
use crate::ui::gadget::list::{GadgetList, GadgetSpec};
use crate::ui::gadget::tick::{GadgetInput, TickOutput, tick};
use crate::ui::gadget::{self, GadgetRect, ListId};
use crate::ui::sidebar::{self, SidebarAction, SidebarChromeLayoutSpec, SidebarItem, SidebarTab};

fn sidebar_item(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    section: &str,
) -> SidebarItem {
    let views = superweapon_views_for_owner(sim, rules, &owner);
    let cameos = sidebar::cameo_order::CameoStrips::layout_fixture(&[], &[], &views);
    let view = sidebar::build_sidebar_view_with_spec(
        SidebarChromeLayoutSpec::default(),
        800.0,
        600.0,
        SidebarTab::Defense,
        0,
        0,
        0,
        None,
        &[],
        &[],
        &[],
        None,
        &[],
        Some(&sim.interner),
        &views,
        &SidebarGadgetState::default(),
        None,
        None,
        None,
        None,
        [None; 2],
        &cameos,
    );
    view.items
        .into_iter()
        .find(|item| item.super_weapon_section.as_deref() == Some(section))
        .expect("visible Super cameo")
}

/// Same GadgetSpec::cameo/tick/hit_test_item used by the retained driver.
/// Unavailable drawing does not disable the native press event.
fn gadget_press(item: &SidebarItem, right: bool) -> SidebarAction {
    let mut list = GadgetList::new(ListId(1));
    let r = item.rect;
    list.add_tail(GadgetSpec::cameo(
        GadgetRect::new(r.x as i32, r.y as i32, r.w as i32, r.h as i32),
        super::super::gadget_input::ID_CAMEO_BASE,
    ));
    let input = GadgetInput {
        queued_key: if right {
            gadget::KEY_RMB_DOWN
        } else {
            gadget::KEY_LMB_DOWN
        },
        event_x: r.x as i32 + 2,
        event_y: r.y as i32 + 2,
        mouse_x: r.x as i32 + 2,
        mouse_y: r.y as i32 + 2,
        left_held: !right,
        right_held: right,
        ..Default::default()
    };
    let result = tick(
        &mut list,
        &mut FocusState::default(),
        &input,
        &mut TickOutput::default(),
    );
    assert_eq!(
        result & !(gadget::RESULT_BUTTON | gadget::RESULT_RIGHT),
        super::super::gadget_input::ID_CAMEO_BASE
    );
    sidebar::hit_test_item(item, result & gadget::RESULT_RIGHT != 0, false)
}

#[test]
fn native_superweapon_cameo_controls_match_modes_and_requests() {
    let data = crate::test_fixture::json("tools/sidebar_oracle/superweapon_click.json");
    for row in data["cases"].as_array().unwrap() {
        let supplied = &row["input"];
        // The original CanFire-only ungranted/ready contrast is not a visible
        // cameo producer. The core comparison covers that literal body.
        if supplied["granted"] == false {
            continue;
        }
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[SuperWeaponTypes]\n0=SUPER\n[SUPER]\nType=MultiMissile\nAction={}\nUseChargeDrain={}\n",
            if supplied["action"] == 0 { "None" } else { "Nuke" },
            supplied["drain"].as_bool().unwrap_or(false)
        ))).unwrap();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let id = sim.interner.intern("SUPER");
        let mut instance = SuperWeaponInstance::new(id, owner, 0);
        instance.is_active = true;
        instance.is_ready = supplied["ready"].as_bool().unwrap_or(true);
        instance.is_suspended = supplied["held"].as_bool().unwrap_or(false);
        instance.charge_drain_state = supplied["state"].as_i64().unwrap_or(0) as i32;
        sim.super_weapons
            .entry(owner)
            .or_default()
            .insert(id, instance);
        let item = sidebar_item(&sim, &rules, owner, "SUPER");
        let right = supplied["right"].as_bool().unwrap_or(false);
        let SidebarAction::SuperWeaponPress {
            section,
            right: routed_right,
        } = gadget_press(&item, right)
        else {
            panic!("Super press was lost");
        };
        assert_eq!(section, "SUPER");
        assert_eq!(right, routed_right);
        let mut input = MatchInputState::new(Default::default());
        let prior = supplied["selected"].as_i64().unwrap_or(-1);
        if prior == -1 {
            input.begin_building_placement("BUILDING", 7, owner);
        } else {
            input.targeting_mode = Some(TargetingMode::SuperWeapon(
                if prior == 0 { "SUPER" } else { "OTHER" }.to_owned(),
            ));
        }
        let mut gadgets = SidebarGadgetState::default();
        gadgets.repair_mode_on = supplied["repair"].as_bool().unwrap_or(true);
        gadgets.sell_mode_on = supplied["sell"].as_bool().unwrap_or(true);
        let views = superweapon_views_for_owner(&sim, &rules, &owner);
        let result = press(
            &mut input,
            &mut gadgets,
            views.first(),
            rules.super_weapon("SUPER").unwrap().action.as_deref(),
            right,
        );
        let selected = input
            .targeting_mode
            .as_ref()
            .and_then(TargetingMode::as_super_weapon)
            .map_or(-1, |section| if section == "SUPER" { 0 } else { 5 });
        let expected = &row["expected"];
        assert_eq!(
            selected,
            expected["selected"].as_i64().unwrap(),
            "{supplied}"
        );
        assert_eq!(
            gadgets.repair_mode_on,
            expected["repair"].as_bool().unwrap(),
            "{supplied}"
        );
        assert_eq!(
            gadgets.sell_mode_on,
            expected["sell"].as_bool().unwrap(),
            "{supplied}"
        );
        if prior == -1 {
            assert_eq!(
                input
                    .targeting_mode
                    .as_ref()
                    .and_then(TargetingMode::as_building_placement)
                    .is_some(),
                expected["placement"][0] != 0,
                "{supplied}"
            );
        }
        let emitted = expected["emitted"].as_array().unwrap();
        assert_eq!(
            result.gui_sound(),
            emitted.iter().any(|e| e["kind"] == "GUI"),
            "{supplied}"
        );
        assert_eq!(
            result == SuperWeaponClick::Target,
            emitted.iter().any(|e| e["name"] == "EVA_SelectTarget"),
            "{supplied}"
        );
        assert_eq!(
            matches!(result, SuperWeaponClick::Launch(_)),
            expected["event_count"] == 1,
            "{supplied}"
        );
    }
}

#[path = "superweapon_cameo_retail_tests.rs"]
mod retail_tests;

#[test]
fn delivery_missing_or_custom_action_arms_the_real_cameo_targeting_path() {
    for action in ["", "Action=Custom\n"] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[SuperWeaponTypes]\n0=Delivery\n[Delivery]\nType=UnitDelivery\n{action}"
        )))
        .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let id = sim.interner.intern("Delivery");
        let mut instance = SuperWeaponInstance::new(id, owner, 0);
        instance.is_active = true;
        instance.is_ready = true;
        sim.super_weapons
            .entry(owner)
            .or_default()
            .insert(id, instance);
        let views = superweapon_views_for_owner(&sim, &rules, &owner);
        let mut input = MatchInputState::new(Default::default());
        let result = press(
            &mut input,
            &mut SidebarGadgetState::default(),
            views.first(),
            rules.super_weapon("Delivery").unwrap().action.as_deref(),
            false,
        );
        assert_eq!(result, SuperWeaponClick::Target, "{action}");
        assert_eq!(
            input
                .targeting_mode
                .as_ref()
                .and_then(TargetingMode::as_super_weapon),
            Some("Delivery")
        );
    }
}

#[test]
fn superweapon_money_cameo_refuses_before_target_or_direct_launch_but_allows_right_cancel() {
    for action in ["PsychicReveal", "None"] {
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(&format!(
                "[SuperWeaponTypes]\n0=Paid\n[Paid]\nType=PsychicReveal\nAction={action}\nMoney.Amount=-500\n"
            )), &IniFile::empty(),
        ).unwrap();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let id = sim.interner.intern("Paid");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 499, 10,
            ));
        let mut instance = SuperWeaponInstance::new(id, owner, 0);
        instance.is_active = true;
        instance.is_ready = true;
        sim.super_weapons
            .entry(owner)
            .or_default()
            .insert(id, instance);
        let mut input = MatchInputState::new(Default::default());
        input.begin_building_placement("BUILDING", 7, owner);
        let mut gadgets = SidebarGadgetState::default();
        gadgets.repair_mode_on = true;
        let views = superweapon_views_for_owner(&sim, &rules, &owner);
        assert!(views[0].can_fire, "native readiness remains independent");
        assert!(!views[0].can_afford);
        let result = press(&mut input, &mut gadgets, views.first(), Some(action), false);
        assert_eq!(result, SuperWeaponClick::Refused);
        assert!(result.gui_sound());
        assert!(
            input
                .targeting_mode
                .as_ref()
                .and_then(TargetingMode::as_building_placement)
                .is_some()
        );
        assert!(gadgets.repair_mode_on);
        input.targeting_mode = Some(TargetingMode::SuperWeapon("Paid".to_owned()));
        assert_eq!(
            press(&mut input, &mut gadgets, views.first(), Some(action), true),
            SuperWeaponClick::None
        );
        assert!(input.targeting_mode.is_none());
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .economy_mut_for_test()
            .add_credits(1);
        let views = superweapon_views_for_owner(&sim, &rules, &owner);
        let result = press(&mut input, &mut gadgets, views.first(), Some(action), false);
        assert_eq!(
            result,
            if action == "None" {
                SuperWeaponClick::Launch(id)
            } else {
                SuperWeaponClick::Target
            }
        );
    }
}
