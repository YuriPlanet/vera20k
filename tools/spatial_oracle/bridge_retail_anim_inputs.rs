use serde_json::json;
use std::path::Path;
fn main() {
    let retail_root = std::env::var("RA2_DIR").expect("set RA2_DIR to the retail asset directory");
    let root = Path::new(&retail_root);
    let scenario = vera20k::headless_scenario::load(root, "Hills.mmx", 0x0B21_D6E5).unwrap();
    let art = scenario.runtime.resources.rules.art();
    let mut names: Vec<String> = [
        "DBRIS1LG", "DBRIS2LG", "DBRIS3LG", "DBRIS4LG", "DBRIS5LG", "DBRIS6LG", "DBRIS7LG",
        "DBRIS8LG", "DBRIS9LG", "DBRS10LG", "DBRIS1SM", "DBRIS2SM", "DBRIS3SM", "DBRIS4SM", "D",
        "TWLT026", "TWLT036", "TWLT050", "TWLT070",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    names.push(scenario.runtime.resources.rules.general.wake.name.clone());
    names.extend(
        scenario
            .runtime
            .resources
            .rules
            .combat_damage
            .splash_list
            .iter()
            .cloned(),
    );
    names.push("SMOKEY2".to_string());
    names.sort();
    names.dedup();
    let rows:Vec<_>=names.into_iter().map(|name|{
        let Some(c)=art.anim_runtime_config(&name) else {return json!({"name":name,"missing_runtime_config":true})};
        json!({"name":name,"art_body_read":c.art_body_read,"start":c.start,"loop_start":c.loop_start,"loop_end":c.loop_end,"end":c.end,"loop_count":c.loop_count,"raw_shp_frame_count":c.raw_shp_frame_count,"explicit_end":c.explicit_end,"explicit_loop_end":c.explicit_loop_end,"rate":c.rate_logic_frames,"random_rate":c.random_rate_logic_frames,"bouncer":c.bouncer,"damage_f64_bits":c.damage.bits(),"damage_radius":c.damage_radius,"warhead":c.warhead,"elasticity_f64_bits":c.elasticity.bits(),"min_z_vel_f64_bits":c.min_z_vel.bits(),"max_xy_vel_f64_bits":c.max_xy_vel.bits(),"bounce_anim":c.bounce_anim,"expire_anim":c.expire_anim,"spawns":c.spawns,"spawn_count":c.spawn_count,"trailer_anim":c.trailer_anim,"trailer_seperation":c.trailer_seperation,"report":c.report,"scorch":c.scorch,"crater":c.crater,"terrain_palette":art.get(&name).map(|e|e.terrain_palette),"full_config_debug":format!("{c:?}")})
    }).collect();
    println!("{}",serde_json::to_string_pretty(&json!({"source":"production headless Hills load, RuleSet ArtRegistry after asset binding; record the built commit alongside new output","metallic_names_source":"executed native ReadGeneral 128-byte list, not current GeneralRules list","he_wall":scenario.runtime.resources.rules.warhead("HE").unwrap().wall,"bridge_strength":scenario.runtime.resources.rules.bridge_rules.strength,"wake":scenario.runtime.resources.rules.general.wake.name,"splash_list":scenario.runtime.resources.rules.combat_damage.splash_list,"rows":rows})).unwrap());
}
