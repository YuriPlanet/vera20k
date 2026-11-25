//! Dump the role weights the army picker reads, for the types a house can buy.

use vera20k_core::rules::object_type::ObjectCategory;
use vera20k_core::rules::unit_role::{ROLES, RoleTable};

fn main() {
    let Some(retail) = vera20k_core::rules::retail_ini_fixture::retail_battle_rules() else {
        eprintln!("the retail rules did not load");
        std::process::exit(2);
    };
    let rules = retail.rules;
    let table = RoleTable::build(&rules);
    let mut rows: Vec<(i32, String, Vec<f64>, f64)> = Vec::new();
    for ty in rules.all_objects() {
        // Aircraft too: `is_army` admits them and `RoleTable::build` scores them,
        // so omitting them here made the one type that can actually break a wall
        // invisible in the table it was chosen from. The `ORCA` reads 168 permille
        // against concrete and never appeared in this dump.
        if !matches!(
            ty.category,
            ObjectCategory::Infantry | ObjectCategory::Vehicle | ObjectCategory::Aircraft
        ) {
            continue;
        }
        let Some(profile) = table.profile(&ty.id) else {
            continue;
        };
        rows.push((
            ty.cost,
            ty.id.to_ascii_uppercase(),
            profile.weights.0.to_vec(),
            profile.dps,
        ));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let header = ROLES
        .iter()
        .map(|role| format!("{role:?}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("{:<10}{:>5}  {header}   dps", "type", "cost");
    for (cost, id, weights, dps) in &rows {
        let cells = weights
            .iter()
            .map(|w| format!("{w:.3}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!("{id:<10}{cost:>5}  {cells}  {dps:6.1}");
    }
}
