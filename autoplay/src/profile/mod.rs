//! Unit profiles: what a unit can do, derived from the rules.
//!
//! Every fact the policy knows about a type is computed here, from the same
//! readers the simulation uses. No type name appears in the policy, which is
//! what lets a mod's unit be used without a code change.
//!
//! The profile is integer-valued: the policy never compares floats, and a
//! damage figure is kept in permille so two builds rank targets identically on
//! every platform.

use std::collections::BTreeMap;

use vera20k_core::rules::object_type::{ObjectCategory, ObjectType};
use vera20k_core::rules::ruleset::RuleSet;

/// The armor registry size the verses table is indexed by, in the order
/// `Armor=` names resolve to (`rules::armor::armor_index`).
pub const ARMOR_SLOTS: usize = 11;

/// Frames in one second of the simulation, for turning a rate-of-fire into a
/// per-second figure.
const FRAMES_PER_SECOND: i64 = 15;

/// What a unit is for, independent of its name. A type may carry several.
///
/// `u32` because the technology buildings arrived: a laboratory, an ore
/// purifier, a superweapon and a war-factory upgrade each needed a bit of their
/// own, and fifteen bits does not leave room for four.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Roles(pub u32);

impl Roles {
    pub const HARVESTER: Self = Self(1 << 0);
    pub const ENGINEER: Self = Self(1 << 1);
    pub const AGENT: Self = Self(1 << 2);
    pub const SABOTEUR: Self = Self(1 << 3);
    pub const OCCUPIER: Self = Self(1 << 4);
    pub const TRANSPORT: Self = Self(1 << 5);
    pub const DEPLOYER: Self = Self(1 << 6);
    pub const REFINERY: Self = Self(1 << 7);
    pub const ARMED: Self = Self(1 << 8);
    pub const STRUCTURE: Self = Self(1 << 9);
    /// A structure whose factory produces infantry (`Factory=InfantryType`).
    pub const TRAINS_INFANTRY: Self = Self(1 << 10);
    /// A structure whose factory produces land vehicles (`Factory=UnitType`).
    pub const BUILDS_VEHICLES: Self = Self(1 << 11);
    /// A structure whose factory produces ships (`Factory=UnitType` and
    /// `Naval=yes`): a shipyard is not a war factory, and treating it as one
    /// leaves a base with no way to build a truck.
    pub const BUILDS_NAVAL: Self = Self(1 << 12);
    /// An armed unit whose best weapon reaches further than a rifle's: it can
    /// shoot from behind the front line.
    ///
    /// Derived from the weapon profile (`range_cells` against
    /// [`Roles::FRONT_RANGE_CELLS`]), never from a type name, so a mod's artillery
    /// is artillery. A unit is either ranged or a front-liner; the longer of its
    /// weapons decides.
    pub const RANGED: Self = Self(1 << 13);
    /// A laboratory: the building that unlocks the house's higher technology,
    /// and in retail YR also the one that grants the force shield.
    ///
    /// Measured from the retail rules, not from a name: `[GATECH]` carries
    /// `SuperWeapon=ForceShieldSpecial` and `[NATECH]`/`[YATECH]` carry the same
    /// key, so the laboratory *is* the shield's source. The house that never
    /// builds one loses both the shield and everything behind it — `[GACSPH]`
    /// requires `GATECH`, `[GAOREP]` requires `GATECH`, and the enemy's iron
    /// curtain requires `NATECH` on the other side.
    pub const LABORATORY: Self = Self(1 << 14);
    /// An ore purifier (`OrePurifier=yes`): every truck brings in more per trip.
    ///
    /// Only `[GAOREP]` carries the key in retail YR, and only the Allied side
    /// gets it, so a Soviet house simply has no such building to find and the
    /// role stays empty for it. That is the data, not a gap in the predicate.
    pub const ORE_PURIFIER: Self = Self(1 << 15);
    /// A building that declares a superweapon (`SuperWeapon=` or `SuperWeapon2=`).
    ///
    /// Retail attaches one to nine buildings plus the laboratories, and the
    /// houses that own them fire iron curtains, nukes, lightning storms and the
    /// chronosphere. A house with none of them has no answer to a house that has.
    pub const SUPERWEAPON: Self = Self(1 << 16);
    /// A radar: the building that satisfies a `RADAR` prerequisite group.
    ///
    /// `Radar=yes` in the retail rules. It sits between the war factory and the
    /// laboratory — `[GATECH]` lists `Prerequisite=GAWEAP,RADAR,GACNST` — and a
    /// house that never builds one can never reach the laboratory, the force
    /// shield, the purifier or anything behind them. Measured: the policy's build
    /// order went straight from four defences to a laboratory, the laboratory was
    /// refused on every term, and the house ended 1v7 matches with `lab=0`.
    pub const RADAR: Self = Self(1 << 17);
    /// A weapon reaching at least this many cells counts as ranged.
    ///
    /// A Grizzly's gun reaches four; a Prism Tank's reaches ten. Anything past
    /// six is shooting over the line in front of it rather than standing in it.
    pub const FRONT_RANGE_CELLS: i32 = 6;

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The roles as names, for the profile dump and the logs.
    pub fn names(self) -> Vec<&'static str> {
        const ALL: &[(&str, Roles)] = &[
            ("harvester", Roles::HARVESTER),
            ("engineer", Roles::ENGINEER),
            ("agent", Roles::AGENT),
            ("saboteur", Roles::SABOTEUR),
            ("occupier", Roles::OCCUPIER),
            ("transport", Roles::TRANSPORT),
            ("deployer", Roles::DEPLOYER),
            ("refinery", Roles::REFINERY),
            ("armed", Roles::ARMED),
            ("structure", Roles::STRUCTURE),
            ("trains-infantry", Roles::TRAINS_INFANTRY),
            ("builds-vehicles", Roles::BUILDS_VEHICLES),
            ("builds-naval", Roles::BUILDS_NAVAL),
            ("ranged", Roles::RANGED),
            ("laboratory", Roles::LABORATORY),
            ("ore-purifier", Roles::ORE_PURIFIER),
            ("superweapon", Roles::SUPERWEAPON),
            ("radar", Roles::RADAR),
        ];
        ALL.iter()
            .filter(|(_, role)| self.contains(*role))
            .map(|(name, _)| *name)
            .collect()
    }
}

/// One weapon, reduced to what a decision compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponProfile {
    pub id: String,
    pub damage: i32,
    pub rof: i32,
    pub burst: i32,
    /// Reach in cells, from the stored lepton range with the simulation's own
    /// conversion (a cell is 256 leptons).
    pub range_cells: i32,
    /// Damage per second against each armor class, in permille of one hit
    /// point. Index follows `Armor=` order.
    pub dps_permille: [i64; ARMOR_SLOTS],
    /// Splash radius in cells from the warhead.
    pub splash_cells: i32,
}

impl WeaponProfile {
    /// Whether this weapon does anything to that armor class.
    pub fn can_hurt(&self, armor: usize) -> bool {
        self.dps_permille.get(armor).is_some_and(|dps| *dps > 0)
    }

    /// The best damage this weapon does to anything, for ranking.
    pub fn best_dps(&self) -> i64 {
        self.dps_permille.iter().copied().max().unwrap_or(0)
    }
}

/// One object type, as the policy understands it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitProfile {
    pub id: String,
    pub category: ObjectCategory,
    pub cost: i32,
    pub strength: i32,
    /// The type's own armor, as an index into the verses table.
    pub armor: usize,
    /// The type's own armor by name, so a caller can ask what job answers
    /// it without re-reading the rules.
    pub armor_name: String,
    pub speed: i32,
    pub sight: i32,
    pub roles: Roles,
    pub weapons: Vec<WeaponProfile>,
    pub passengers: i32,
    pub ammo: i32,
    pub reload: i32,
    /// Power output for a structure (negative is drain).
    pub power: i32,
    /// `BuildLimit=`: a hero or a one-off.
    pub build_limit: i32,
}

impl UnitProfile {
    /// Derive one type's profile. Pure: the same rules always give the same
    /// profile.
    pub fn of(rules: &RuleSet, ty: &ObjectType) -> Self {
        let mut roles = Roles::default();
        if ty.harvester || ty.resource_gatherer {
            roles = roles.with(Roles::HARVESTER);
        }
        if ty.engineer {
            roles = roles.with(Roles::ENGINEER);
        }
        if ty.agent || ty.infiltrate {
            roles = roles.with(Roles::AGENT);
        }
        if ty.c4 {
            roles = roles.with(Roles::SABOTEUR);
        }
        if ty.occupier {
            roles = roles.with(Roles::OCCUPIER);
        }
        if ty.passengers > 0 {
            roles = roles.with(Roles::TRANSPORT);
        }
        // A maker carries `DeploysInto=`; a deployment-form unit carries
        // `UndeploysInto=` or `Deployer=`. All three mean "this unit becomes
        // something else where it stands".
        if ty.deployer || ty.deploys_into.is_some() || ty.undeploys_into.is_some() {
            roles = roles.with(Roles::DEPLOYER);
        }
        // Two keys, because retail uses both and they are not interchangeable.
        // `Refinery=` is the allied and soviet refinery; Yuri's ore refinery
        // carries no `Refinery=` at all and is declared by
        // `ResourceDestination=yes` instead (`[YAREFN]` in rulesmd.ini). A
        // predicate that read only `Refinery=` left the Yuri house with no
        // refinery it could ever see, so it never built one, never bought a
        // truck, and farmed bio reactors until the map ended.
        //
        // `ResourceDestination` alone is not enough, because the slave miner
        // carries it too: `[SMIN]` is the deployed refinery's vehicle form
        // (`DeploysInto=YAREFN`, and `[YAREFN]`'s `UndeploysInto=SMIN`), so the
        // flag names "this object receives ore" and only a structure can be the
        // place a house builds. The vehicle form is a harvester and is checked
        // as one.
        let receives_ore =
            ty.refinery || (ty.resource_destination && ty.category == ObjectCategory::Building);
        if receives_ore {
            roles = roles.with(Roles::REFINERY);
        }
        if ty.category == ObjectCategory::Building {
            roles = roles.with(Roles::STRUCTURE);
        }
        match ty.factory_type() {
            Some(vera20k_core::rules::object_type::FactoryType::InfantryType) => {
                roles = roles.with(Roles::TRAINS_INFANTRY);
            }
            Some(vera20k_core::rules::object_type::FactoryType::UnitType) => {
                roles = roles.with(if ty.naval {
                    Roles::BUILDS_NAVAL
                } else {
                    Roles::BUILDS_VEHICLES
                });
            }
            _ => {}
        }

        // The technology buildings. Each key is the one retail actually reads,
        // and each was checked against `ini/rulesmd.ini` rather than guessed:
        //
        // - a laboratory declares the force shield (`SuperWeapon=ForceShield
        //   Special` on `[GATECH]`/`[NATECH]`/`[YATECH]`), and the shield is
        //   also what gates the next tier — `[GACSPH]` and `[GAOREP]` both list
        //   `GATECH` in their prerequisites;
        // - an ore purifier is `OrePurifier=yes` (`[GAOREP]` only, Allied side);
        // - a superweapon building declares `SuperWeapon=` or `SuperWeapon2=`.
        //
        // There is no industrial plant to find: retail YR has none. `[GAYARD]`
        // and `[NAYARD]` look like factory upgrades and are shipyards
        // (`Name=Allied Shipyard`, `Name=Soviet Shipyard`), and every war
        // factory already carries `WeaponsFactory=yes`, so that key cannot
        // separate one from the other. The industrial plant is a Tiberian Sun
        // building that YR's rules do not ship.
        if ty.category == ObjectCategory::Building {
            let declares_superweapon = ty.super_weapon.is_some() || ty.super_weapon2.is_some();
            if declares_superweapon {
                roles = roles.with(Roles::SUPERWEAPON);
            }
            if ty.ore_purifier {
                roles = roles.with(Roles::ORE_PURIFIER);
            }
            // The radar. `Radar=yes` on `[AMRADR]`, `[NARADR]`, `[GAAIRC]` and
            // `[NAPSIS]`, which are exactly the four the retail rules list under
            // `[General] PrerequisiteRadar` — the group `[GATECH]` names.
            if ty.radar {
                roles = roles.with(Roles::RADAR);
            }
            // A laboratory is the structure whose only superweapon is the force
            // shield: retail gives it no other effect to declare, and a house
            // builds it to unlock the tier rather than to fire anything.
            if declares_superweapon
                && ty
                    .super_weapon
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case("ForceShieldSpecial"))
            {
                roles = roles.with(Roles::LABORATORY);
            }
        }

        // Only the slots native actually fires, and never a `NeverUse=yes`
        // entry.
        //
        // `TechnoClass::What_Weapon_Should_I_Use @ 0x006F3330` returns one
        // weapon index, and `weapon_damage_value` (`0x006F3970`) reads the
        // current gunner slot for a `TurretCount>0` non-gattling type and
        // slots 0..1 otherwise. The slots past those are not a second gun:
        // the IFV's seventeen `WeaponN=` entries are the crew weapons each
        // passenger turns its turret into, and summing them gave one transport
        // the firepower of a dozen types at once. Arm E at `0x006F33BF` never
        // selects a `NeverUse=yes` secondary, which is the scan-range
        // `VirtualScanner` every dog and engineer carries.
        let slots: Vec<usize> = if ty.is_gattling {
            // A gattling type steps through `stage*2` and `stage*2+1`, so what
            // it can bring is spread over every slot it declares.
            (0..ty.weapon_record_count()).collect()
        } else if ty.turret_count > 0 {
            vec![0]
        } else {
            vec![0, 1]
        };
        let mut weapons = Vec::new();
        for index in slots {
            let Some(name) = ty.weapon_at(index) else {
                continue;
            };
            if name.is_empty() || name.eq_ignore_ascii_case("none") {
                continue;
            }
            let Some(weapon) = rules.weapon(name) else {
                continue;
            };
            if weapon.never_use {
                continue;
            }
            let mut dps_permille = [0_i64; ARMOR_SLOTS];
            let verses = rules
                .warhead(weapon.warhead.as_deref().unwrap_or(""))
                .map(|warhead| warhead.verses_f64)
                .unwrap_or([1.0; ARMOR_SLOTS]);
            let rof = weapon.rof.max(1) as i64;
            let burst = weapon.burst.max(1) as i64;
            for (slot, multiplier) in verses.iter().enumerate().take(ARMOR_SLOTS) {
                let per_mille = (multiplier * 1000.0).round() as i64;
                dps_permille[slot] =
                    i64::from(weapon.damage) * burst * FRAMES_PER_SECOND * per_mille / rof / 1000;
            }
            let splash_cells = rules
                .warhead(weapon.warhead.as_deref().unwrap_or(""))
                .map(|warhead| warhead.cell_spread_f64.round() as i32)
                .unwrap_or(0);
            weapons.push(WeaponProfile {
                id: name.to_owned(),
                damage: weapon.damage,
                rof: weapon.rof,
                burst: weapon.burst,
                range_cells: weapon.range_leptons / 256,
                dps_permille,
                splash_cells,
            });
        }
        // `Armed` is the engine's own answer (`capabilities.rs`), not a second
        // reading of the weapon list: it skips `NeverUse=yes` entries and reads
        // the same slots the simulation does, so a mod's unit is armed here
        // exactly when it is armed in the fight.
        if vera20k_core::rules::capabilities::Capabilities::of(rules, ty)
            .has(vera20k_core::rules::capabilities::Tag::Armed)
        {
            roles = roles.with(Roles::ARMED);
        }
        // What a unit is for in a fight, off its best reach rather than its name:
        // a gun that reaches past the front line is a ranged unit, and one that
        // does not stands in it. A structure keeps out of this — a base defence's
        // reach says nothing about how an army should be composed.
        if roles.contains(Roles::ARMED)
            && ty.category != ObjectCategory::Building
            && weapons
                .iter()
                .any(|weapon| weapon.range_cells >= Roles::FRONT_RANGE_CELLS)
        {
            roles = roles.with(Roles::RANGED);
        }

        Self {
            id: ty.id.clone(),
            category: ty.category,
            cost: ty.cost,
            strength: ty.strength,
            armor: vera20k_core::rules::armor::armor_index(&ty.armor).index(),
            armor_name: ty.armor.clone(),
            speed: ty.speed,
            sight: ty.sight,
            roles,
            weapons,
            passengers: ty.passengers,
            ammo: ty.ammo,
            reload: ty.reload,
            power: ty.power,
            build_limit: ty.build_limit,
        }
    }

    /// The weapon the policy would use against that armor class: the one with
    /// the highest damage per second that can hurt it.
    pub fn best_weapon_against(&self, armor: usize) -> Option<&WeaponProfile> {
        self.weapons
            .iter()
            .filter(|weapon| weapon.can_hurt(armor))
            .max_by_key(|weapon| weapon.dps_permille[armor])
    }

    /// The damage this type puts into a target of that armor class, per second.
    ///
    /// The best of the weapons that can hurt it, not the sum: native fires one
    /// weapon at a time (`What_Weapon_Should_I_Use` answers one index), so a
    /// type with a rifle and a rocket launcher does not shoot both in the same
    /// second. Zero means no weapon can hurt that armor class at all, which is
    /// the answer a caller needs — a type that cannot hurt armour is not army
    /// against armour.
    pub fn damage_against(&self, armor: usize) -> i64 {
        self.weapons
            .iter()
            .filter(|weapon| weapon.can_hurt(armor))
            .map(|weapon| weapon.dps_permille[armor])
            .max()
            .unwrap_or(0)
    }

    /// Whether this unit can hurt an object with that armor at all.
    pub fn can_hurt(&self, armor: usize) -> bool {
        self.best_weapon_against(armor).is_some()
    }
}

/// Every type the rules describe, by id.
#[derive(Debug, Clone, Default)]
pub struct Profiles {
    by_id: BTreeMap<String, UnitProfile>,
}

impl Profiles {
    pub fn build(rules: &RuleSet) -> Self {
        let mut by_id = BTreeMap::new();
        for ty in rules.all_objects() {
            by_id.insert(ty.id.clone(), UnitProfile::of(rules, ty));
        }
        Self { by_id }
    }

    pub fn get(&self, id: &str) -> Option<&UnitProfile> {
        self.by_id.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &UnitProfile> {
        self.by_id.values()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The whole table as JSON, for review. Hand-written so the crate needs no
    /// serialization dependency for a diagnostic.
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n");
        for (index, profile) in self.by_id.values().enumerate() {
            if index > 0 {
                out.push_str(",\n");
            }
            out.push_str(&profile.to_json());
        }
        out.push_str("\n}\n");
        out
    }
}

impl UnitProfile {
    fn to_json(&self) -> String {
        let roles: Vec<String> = self
            .roles
            .names()
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect();
        let weapons: Vec<String> = self
            .weapons
            .iter()
            .map(|weapon| {
                let dps: Vec<String> = weapon
                    .dps_permille
                    .iter()
                    .map(|value| value.to_string())
                    .collect();
                format!(
                    "{{\"id\":\"{}\",\"damage\":{},\"rof\":{},\"burst\":{},\"range_cells\":{},\"splash_cells\":{},\"dps_permille\":[{}]}}",
                    weapon.id,
                    weapon.damage,
                    weapon.rof,
                    weapon.burst,
                    weapon.range_cells,
                    weapon.splash_cells,
                    dps.join(",")
                )
            })
            .collect();
        format!(
            "  \"{}\": {{\"category\":\"{}\",\"cost\":{},\"strength\":{},\"armor\":{},\"speed\":{},\"sight\":{},\"passengers\":{},\"ammo\":{},\"reload\":{},\"power\":{},\"build_limit\":{},\"roles\":[{}],\"weapons\":[{}]}}",
            self.id,
            category_name(self.category),
            self.cost,
            self.strength,
            self.armor,
            self.speed,
            self.sight,
            self.passengers,
            self.ammo,
            self.reload,
            self.power,
            self.build_limit,
            roles.join(","),
            weapons.join(",")
        )
    }
}

fn category_name(category: ObjectCategory) -> &'static str {
    match category {
        ObjectCategory::Infantry => "infantry",
        ObjectCategory::Vehicle => "vehicle",
        ObjectCategory::Aircraft => "aircraft",
        ObjectCategory::Building => "building",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retail() -> Option<RuleSet> {
        Some(vera20k_core::rules::retail_ini_fixture::retail_battle_rules()?.rules)
    }

    #[test]
    fn every_type_gets_a_profile() {
        let Some(rules) = retail() else {
            return;
        };
        let profiles = Profiles::build(&rules);
        assert!(profiles.len() > 100, "the rules describe few types");
        for profile in profiles.iter() {
            assert!(!profile.id.is_empty());
        }
    }

    #[test]
    fn a_tank_is_armed_and_its_weapon_hurts_armor() {
        let Some(rules) = retail() else {
            return;
        };
        let profiles = Profiles::build(&rules);
        let Some(grizzly) = profiles.get("MTNK") else {
            return;
        };
        assert!(grizzly.roles.contains(Roles::ARMED), "a tank is armed");
        assert!(
            grizzly.can_hurt(vera20k_core::rules::armor::armor_index("heavy").index()),
            "a tank's gun hurts heavy armor"
        );
        assert!(grizzly.cost > 0);
    }

    /// The attack dog's only real weapon is its teeth, and a `ParasiteDog`
    /// warhead is 0% against every vehicle armour. It used to score anti-armour
    /// damage off the `VirtualScanner` every dog carries as `Secondary=` — a
    /// `NeverUse=yes` scan range that native never selects to fire.
    #[test]
    fn a_dog_cannot_hurt_armour() {
        let Some(rules) = retail() else {
            return;
        };
        let profiles = Profiles::build(&rules);
        let Some(dog) = profiles.get("ADOG") else {
            return;
        };
        for armor in ["light", "medium", "heavy"] {
            let at = vera20k_core::rules::armor::armor_index(armor).index();
            assert!(!dog.can_hurt(at), "a dog does nothing to {armor} armour");
            assert_eq!(dog.damage_against(at), 0);
        }
        assert!(!dog.weapons.iter().any(|w| w.id == "VirtualScanner"));
    }

    /// A turreted type's profile is the gun it points, not the whole weapon
    /// array: `TurretCount>0` takes `What_Weapon_Should_I_Use`'s gunner slot
    /// (arm A), and the IFV's seventeen entries are the crew weapons each
    /// passenger turns its turret into.
    #[test]
    fn a_turreted_type_is_profiled_by_its_own_gun() {
        let Some(rules) = retail() else {
            return;
        };
        let profiles = Profiles::build(&rules);
        let Some(ifv) = profiles.get("FV") else {
            return;
        };
        assert_eq!(ifv.weapons.len(), 1, "the IFV carries one gun of its own");
        assert_eq!(ifv.weapons[0].id, "HoverMissile");
    }

    #[test]
    fn a_harvester_is_not_a_fighter_and_a_transport_carries() {
        let Some(rules) = retail() else {
            return;
        };
        let profiles = Profiles::build(&rules);
        if let Some(miner) = profiles.get("HARV") {
            assert!(miner.roles.contains(Roles::HARVESTER));
        }
        if let Some(ifv) = profiles.get("FV") {
            assert!(ifv.roles.contains(Roles::TRANSPORT), "an IFV carries one");
            assert!(ifv.passengers > 0);
        }
    }

    /// The technology buildings, read from the retail rules rather than from a
    /// name: `[GATECH]` is the laboratory because it declares the force shield,
    /// `[GAOREP]` the ore purifier because of `OrePurifier=yes`, `[GACSPH]` a
    /// superweapon because it declares one, `[GAYARD]` the factory upgrade
    /// because of `WeaponsFactory=yes`.
    ///
    /// This is the predicate the house's build order was missing: without it the
    /// policy could not see a laboratory at all, so no house ever built one, so
    /// no house ever had the force shield, the next tech tier or anything that
    /// requires it — and the enemy's iron curtain arrived unanswered.
    #[test]
    fn the_technology_buildings_are_recognised_from_retail_keys() {
        let Some(rules) = retail() else {
            return;
        };
        let profiles = Profiles::build(&rules);

        let Some(lab) = profiles.get("GATECH") else {
            return;
        };
        assert!(
            lab.roles.contains(Roles::LABORATORY),
            "GATECH declares SuperWeapon=ForceShieldSpecial, which is the shield \
             and the tech gate at once"
        );
        assert!(
            lab.roles.contains(Roles::SUPERWEAPON),
            "a laboratory declares a superweapon like any other launcher"
        );

        // The Soviet laboratory answers the same question on the other side, so
        // the predicate is reading the key and not one faction's name.
        if let Some(soviet) = profiles.get("NATECH") {
            assert!(soviet.roles.contains(Roles::LABORATORY));
        }

        let Some(purifier) = profiles.get("GAOREP") else {
            return;
        };
        assert!(
            purifier.roles.contains(Roles::ORE_PURIFIER),
            "GAOREP carries OrePurifier=yes"
        );

        let Some(chrono) = profiles.get("GACSPH") else {
            return;
        };
        assert!(
            chrono.roles.contains(Roles::SUPERWEAPON),
            "GACSPH declares SuperWeapon=ChronoSphereSpecial"
        );

        // The iron curtain is what the enemy fires, and the predicate has to read
        // it on the other side too: `[NAIRON]` declares IronCurtainSpecial and
        // requires `NATECH`. A house that never builds the laboratory never
        // reaches it, which is the gap this predicate closes.
        if let Some(iron) = profiles.get("NAIRON") {
            assert!(
                iron.roles.contains(Roles::SUPERWEAPON),
                "NAIRON declares SuperWeapon=IronCurtainSpecial"
            );
        }

        // A shipyard carries `WeaponsFactory=yes` exactly like a war factory, and
        // neither is a technology building: this pins that the superweapon role
        // did not leak onto every factory-flagged structure.
        if let Some(shipyard) = profiles.get("GAYARD") {
            assert!(
                !shipyard.roles.contains(Roles::SUPERWEAPON),
                "GAYARD is a shipyard and fires nothing"
            );
        }
    }

    #[test]
    fn a_ranged_role_comes_from_the_reach_of_the_weapon() {
        let Some(rules) = retail() else {
            return;
        };
        let profiles = Profiles::build(&rules);
        // The verses table cannot tell a rifle from a howitzer; reach can. These
        // assertions are about the predicate, not about the units: whatever the
        // rules call them, a gun past the threshold is ranged and one under it is
        // not.
        for profile in profiles.iter() {
            if !profile.roles.contains(Roles::ARMED) || profile.roles.contains(Roles::STRUCTURE) {
                continue;
            }
            let longest = profile
                .weapons
                .iter()
                .map(|weapon| weapon.range_cells)
                .max()
                .unwrap_or(0);
            assert_eq!(
                profile.roles.contains(Roles::RANGED),
                longest >= Roles::FRONT_RANGE_CELLS,
                "{} reaches {longest} cells",
                profile.id
            );
        }
        // And the role is not free: an unarmed type never gets it.
        for profile in profiles.iter() {
            if !profile.roles.contains(Roles::ARMED) {
                assert!(
                    !profile.roles.contains(Roles::RANGED),
                    "{} is not armed and cannot be ranged",
                    profile.id
                );
            }
        }
    }

    #[test]
    fn a_refinery_is_recognized_by_either_key_retail_uses() {
        let Some(rules) = retail() else {
            return;
        };
        let profiles = Profiles::build(&rules);
        // Allied and soviet refineries carry `Refinery=yes`.
        for id in ["GAREFN", "NAREFN"] {
            if let Some(refinery) = profiles.get(id) {
                assert!(
                    refinery.roles.contains(Roles::REFINERY),
                    "{id} is a refinery"
                );
            }
        }
        // Yuri's carries `ResourceDestination=yes` and no `Refinery=` at all.
        if let Some(yuri) = profiles.get("YAREFN") {
            assert!(
                yuri.roles.contains(Roles::REFINERY),
                "a Yuri ore refinery is a refinery even without Refinery="
            );
        }
        // And the predicate is not so broad that it swallows the miners: a
        // gatherer drops ore off, it does not receive it.
        for miner in ["HARV", "SMIN", "CMIN"] {
            if let Some(truck) = profiles.get(miner) {
                assert!(
                    !truck.roles.contains(Roles::REFINERY),
                    "{miner} is a harvester, not a refinery"
                );
            }
        }
    }

    #[test]
    fn roles_come_from_the_factory_and_the_deploy_keys() {
        let Some(rules) = retail() else {
            return;
        };
        let profiles = Profiles::build(&rules);
        if let Some(barracks) = profiles.get("GAPILE") {
            assert!(
                barracks.roles.contains(Roles::TRAINS_INFANTRY),
                "a barracks trains infantry"
            );
        }
        if let Some(factory) = profiles.get("GAWEAP") {
            assert!(
                factory.roles.contains(Roles::BUILDS_VEHICLES),
                "a war factory builds vehicles"
            );
        }
        if let Some(refinery) = profiles.get("GAREFN") {
            assert!(refinery.roles.contains(Roles::REFINERY));
        }
        if let Some(maker) = profiles.get("AMCV") {
            assert!(
                maker.roles.contains(Roles::DEPLOYER),
                "a maker deploys into something"
            );
        }
    }

    #[test]
    fn a_capability_is_true_only_where_the_rules_say_so() {
        let Some(rules) = retail() else {
            return;
        };
        let profiles = Profiles::build(&rules);
        let Some(gi) = profiles.get("E1") else {
            return;
        };
        let Some(grizzly) = profiles.get("MTNK") else {
            return;
        };
        assert!(gi.roles.contains(Roles::OCCUPIER), "a GI can garrison");
        assert!(!gi.roles.contains(Roles::HARVESTER));
        assert!(grizzly.roles.contains(Roles::ARMED));
        assert!(!grizzly.roles.contains(Roles::TRANSPORT));
    }

    #[test]
    fn an_armor_class_the_warhead_cannot_touch_is_not_hurt() {
        let Some(rules) = retail() else {
            return;
        };
        let profiles = Profiles::build(&rules);
        let Some(grizzly) = profiles.get("MTNK") else {
            return;
        };
        // The verses table is the authority: every slot either has damage or
        // does not, and the predicate must agree with it.
        for armor in 0..ARMOR_SLOTS {
            let expected = grizzly
                .weapons
                .iter()
                .any(|weapon| weapon.dps_permille[armor] > 0);
            assert_eq!(grizzly.can_hurt(armor), expected);
        }
    }
}
