use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::RulesLayerStack;
use crate::rules::ruleset::RuleSet;

fn rules(text: &str) -> RuleSet {
    let processed = RulesLayerStack::new(IniFile::from_str(text))
        .process()
        .unwrap();
    RuleSet::from_processed_rules(&processed).unwrap()
}

/// Ares 3.0 TechnoTypeExt (Ares.dll 0x1003E355..0x1003E380, 0x1003E6D5):
/// `Insignia=` sets every rank and `Insignia.<Rank>=` overrides one, each
/// value becoming `<value>.shp` whatever it ends with (0x10040640);
/// `Insignia.ShowEnemy` is unset until authored. RulesExt `[General]
/// EnemyInsignia` defaults to yes (constructor 0x1002A3DF).
#[test]
fn ares_insignia_reads_promotable_shp_names_and_enemy_visibility() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules(
        "[VehicleTypes]\n0=HERO\n1=MCV\n[InfantryTypes]\n0=SPY\n\
         [HERO]\nInsignia=hero\nInsignia.Elite=herostar\n\
         [MCV]\nInsignia.Veteran=dummy.shp\n\
         [SPY]\nInsignia.Rookie=spyins\nInsignia.ShowEnemy=yes\nInsigniaFrame.Rookie=2\n",
    );
    let insignia = |name| rules.object(name).unwrap().ares_insignia.clone();
    let hero = insignia("HERO");
    assert_eq!(
        hero.images.map(|history| history.last().cloned()),
        [
            Some("hero.shp".to_owned()),
            Some("hero.shp".to_owned()),
            Some("herostar.shp".to_owned()),
        ]
    );
    assert_eq!((hero.frames, hero.show_enemy), ([-1; 3], None));
    assert_eq!(insignia("MCV").images[1], ["dummy.shp.shp"]);
    let spy = insignia("SPY");
    assert_eq!((spy.frames, spy.show_enemy), ([2, -1, -1], Some(true)));
    assert!(rules.ares_enemy_insignia());
    let mo = self::rules("[General]\nEnemyInsignia=no\n");
    assert!(!mo.ares_enemy_insignia());
}
