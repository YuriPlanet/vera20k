//! The `Ammo.*` deploy reads (`Ext/UnitType/Body.cpp:45-49`, constructors
//! `Ext/UnitType/Body.h:30-34`).
use super::PhobosAmmoDeploy;
use crate::rules::ini_parser::IniFile;

#[test]
fn defaults_match_the_constructor() {
    let set = PhobosAmmoDeploy::default();
    assert_eq!(set.add_on_deploy, 0);
    assert_eq!(
        (
            set.auto_deploy_minimum,
            set.auto_deploy_maximum,
            set.deploy_unlock_minimum,
            set.deploy_unlock_maximum
        ),
        (-1, -1, -1, -1)
    );
    assert!(
        set.has_ammo_to_deploy(0),
        "an unset DeployUnlock band admits every count"
    );
    assert_eq!(set.auto_deploy_band(), None);
    assert_eq!(set.ammo_after_deploy(5, 10), 5, "no add keeps the count");
}

#[test]
fn reads_the_five_keys_and_their_bands() {
    let set = PhobosAmmoDeploy::from_section(
        IniFile::from_str(
            "[D]\nAmmo.AddOnDeploy=-3\nAmmo.AutoDeployMinimumAmount=1\n\
             Ammo.AutoDeployMaximumAmount=4\nAmmo.DeployUnlockMinimumAmount=2\n\
             Ammo.DeployUnlockMaximumAmount=6\n",
        )
        .section_or_empty("D"),
    );
    assert_eq!(set.add_on_deploy, -3);
    assert_eq!((set.auto_deploy_minimum, set.auto_deploy_maximum), (1, 4));
    assert_eq!(
        (set.deploy_unlock_minimum, set.deploy_unlock_maximum),
        (2, 6)
    );
    // HandleOnDeployAmmoChange clamps into [0, type Ammo].
    assert_eq!(set.ammo_after_deploy(8, 10), 5);
    assert_eq!(set.ammo_after_deploy(1, 10), 0);
    assert_eq!(set.ammo_after_deploy(9, 10), 6);
    // A type without `Ammo=` keeps the constructor's -1: native's ordered
    // clamp answers 0 below zero and -1 above it, without panicking.
    assert_eq!(set.ammo_after_deploy(4, -1), -1);
    assert_eq!(set.ammo_after_deploy(2, -1), 0);
    // HasAmmoToDeploy admits only inside the unlock band.
    assert!(set.has_ammo_to_deploy(2) && set.has_ammo_to_deploy(6));
    assert!(!set.has_ammo_to_deploy(1) && !set.has_ammo_to_deploy(7));
    assert_eq!(set.auto_deploy_band(), Some((1, 4)));
}
