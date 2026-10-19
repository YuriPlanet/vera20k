use super::*;
use crate::rules::ini_parser::IniFile;

#[test]
fn can_fire_matches_original_sidebar_controls() {
    let data = crate::test_fixture::json("tools/sidebar_oracle/superweapon_click.json");
    for row in data["cases"].as_array().unwrap() {
        let input = &row["input"];
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[SuperWeaponTypes]\n0=SUPER\n[SUPER]\nType=MultiMissile\nUseChargeDrain={}\n",
            input["drain"].as_bool().unwrap_or(false)
        )))
        .unwrap();
        let id = InternedId::from_index(0);
        let mut instance = SuperWeaponInstance::new(id, id, 0);
        instance.is_active = input["granted"].as_bool().unwrap_or(true);
        instance.is_ready = input["ready"].as_bool().unwrap_or(true);
        instance.is_suspended = input["held"].as_bool().unwrap_or(false);
        instance.charge_drain_state = input["state"].as_i64().unwrap_or(0) as i32;
        assert_eq!(
            instance.can_fire(rules.super_weapon("SUPER").unwrap()),
            row["expected"]["can_fire"].as_bool().unwrap(),
            "{input}"
        );
    }
}
