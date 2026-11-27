//! Unit tests of the rules set, one child module per subject; the shared
//! imports and the minimal rules fixture live here.

use super::*;
use crate::rules::native_processing::RulesLayerKind;

/// Build a minimal rules.ini string for testing.
fn make_test_rules() -> String {
    "\
[InfantryTypes]
0=E1
1=E2

[General]
BuildSpeed=0.75
MultipleFactory=0.7
LowPowerPenaltyModifier=1.25
MinLowPowerProductionSpeed=0.4
MaxLowPowerProductionSpeed=0.85

[VehicleTypes]
0=MTNK

[AircraftTypes]

[BuildingTypes]
0=GAPOWR

[E1]
Name=GI
Cost=200
Strength=125
Armor=flak
Speed=4
Primary=M60
BuildTimeMultiplier=1.15

[E2]
Name=Conscript
Cost=100
Strength=100
Armor=flak
Speed=4
Primary=INTL

[MTNK]
Name=Grizzly
Cost=700
Strength=300
Armor=heavy
Speed=6
Primary=105mm
Secondary=MachGun

[GAPOWR]
Name=Power Plant
Cost=800
Strength=750
Power=200
Foundation=2x2

[M60]
Damage=25
ROF=20
Range=5
Warhead=SA

[INTL]
Damage=20
ROF=20
Range=4.75
Warhead=SA

[105mm]
Damage=65
ROF=50
Range=5.75
Speed=40
Projectile=InvisibleLow
Warhead=AP
Burst=2

[MachGun]
Damage=20
ROF=15
Range=5
Projectile=InvisibleLow
Warhead=SA

[InvisibleLow]
AA=no
AG=yes

[SA]
Verses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%
CellSpread=0

[AP]
Verses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%
CellSpread=0
"
    .to_string()
}

mod config_hash_tests;
mod countries_tests;
mod general_tests;
mod types_tests;
