//! Ares TechnoClass_GetBuildTime10041AB0, TechnoTypeClass_GetBuildSpeed
//! 10041D40 and HouseClass_GetCostMult10041780 through the production
//! kernels and Cost_Of.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::HouseCostFactors;

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\nBuildSpeed=0.5\nWallBuildSpeedCoefficient=0.5\n\
         LowPowerPenaltyModifier=2.0\nMinLowPowerProductionSpeed=-5\n\
         [BuildingTypes]\n0=FASTWALL\n1=WALL\n[InfantryTypes]\n0=E1\n1=PLAIN\n2=CHEAP\n\
         [FASTWALL]\nStrength=100\nWall=yes\nCost=100\nBuildTime.Cost=4000\nBuildTime.Speed=0.25\n\
         [WALL]\nStrength=100\nWall=yes\nCost=2000\n\
         [E1]\nStrength=100\nCost=1000\nBuildTime.LowPowerPenalty=0\nBuildTime.MaxLowPower=0.5\n\
         [PLAIN]\nStrength=100\nCost=1000\n\
         [CHEAP]\nStrength=100\nCost=1000\nFactoryPlant.Multiplier=0.5\n",
    ))
    .expect("fixture rules")
}

fn inputs(rules: &RuleSet, object: &ObjectType, output: i32, drain: i32) -> TimeToBuildInputs {
    TimeToBuildInputs {
        cost: object.cost,
        build_speed: rules.production.build_speed,
        country_multiplier: NativeF32Bits::ONE,
        build_time_multiplier: object.build_time_multiplier,
        power_output: output,
        power_drain: drain,
        low_power_penalty: rules.production.low_power_penalty_modifier,
        min_low_power_speed: rules.production.min_low_power_production_speed,
        max_low_power_speed: rules.production.max_low_power_production_speed,
        factory_count: 1,
        multiple_factory: rules.production.multiple_factory,
        wall: object.wall_build_speed_applies(),
        wall_coefficient: rules.production.wall_build_speed_coefficient,
    }
}

/// BuildTime.Cost and BuildTime.Speed replace Cost= and BuildSpeed= (4000 x
/// 0.25 / 1000 x 900), and a set BuildTime.Speed exempts a wall from
/// WallBuildSpeedCoefficient; an ordinary wall (2000 x 0.5) still takes it.
#[test]
fn build_time_overrides_replace_cost_and_speed_and_exempt_walls() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules();
    for (name, base, full) in [("FASTWALL", 900, 900), ("WALL", 900, 450)] {
        let object = rules.object(name).unwrap();
        assert_eq!(type_build_time(object, &rules), base, "{name}");
        let inputs = inputs(&rules, object, 100, 0);
        assert_eq!(
            ares_time_to_build(&inputs, object.ares_production()),
            full,
            "{name}"
        );
    }
}

/// A speed below 0.01 is raised to 0.01, where the stock kernel only
/// replaces exactly zero; BuildTime.LowPowerPenalty and BuildTime.MaxLowPower
/// replace the [General] values.
#[test]
fn low_power_speed_floor_and_overrides() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules();
    let plain = rules.object("PLAIN").unwrap();
    // Half power: 1 - 0.5 x 2.0 is zero; a tenth: -0.8.
    let half = inputs(&rules, plain, 50, 100);
    let tenth = inputs(&rules, plain, 10, 100);
    let floored = ares_time_to_build(&half, plain.ares_production());
    assert!(floored > 0);
    assert_eq!(ares_time_to_build(&tenth, plain.ares_production()), floored);
    assert!(
        time_to_build(&tenth) < 0,
        "the stock kernel divides by -0.8"
    );
    // No penalty, so a tenth of the power keeps speed 1.0 until the 0.5 cap.
    let e1 = rules.object("E1").unwrap();
    let tenth = inputs(&rules, e1, 10, 100);
    assert_eq!(ares_time_to_build(&tenth, e1.ares_production()), 900);
}

/// FactoryPlant.Multiplier scales the House's FactoryPlant discount:
/// 1 - (1 - 0.75) x 0.5 prices a 1000-credit type at 875, the default
/// multiplier at the stock 750.
#[test]
fn factory_plant_multiplier_scales_the_discount() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules();
    let mut factors = HouseCostFactors {
        country: [NativeF32Bits::ONE; 5],
        factory_plant: [NativeF32Bits::ONE; 5],
    };
    factors.factory_plant[0] = NativeF32Bits::from_bits(0.75f32.to_bits());
    assert_eq!(
        rules.cost_of(rules.object("CHEAP").unwrap(), Some(&factors)),
        875
    );
    assert_eq!(
        rules.cost_of(rules.object("E1").unwrap(), Some(&factors)),
        750
    );
}
