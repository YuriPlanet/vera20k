//! What an object can do, derived once from the retail keys.
//!
//! Every pass used to answer "can this type do X" for itself: `census::Traits`
//! carried the ones the build list needed, `micro` re-read `Crusher=`, `kite`
//! scanned warheads for `Parasite=`, and the capabilities nobody asked about
//! (`Gunner=`, `Agent=`, `Cloakable=`, `Spawns=`) were implemented in the
//! simulation and unreachable from the policy. One table, derived from the
//! same keys the simulation reads, is the difference between adding a
//! capability in one place and adding it in five.

use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;

/// One capability, as a name a decision can ask about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tag {
    /// `Gunner=yes`: a transport whose passenger mans a turret (the IFV).
    Gunner,
    /// `Passengers=` above zero: it carries infantry.
    Transport,
    /// `Occupier=yes`: infantry that can hold a civilian building.
    Occupier,
    /// `Deployer=yes`: the D key turns it into a position.
    Deployer,
    /// `Engineer=yes`: it takes a building by walking into it.
    Engineer,
    /// `Infiltrate=yes`: it enters a `Capturable=` building.
    Infiltrator,
    /// `Agent=yes`: it enters a `Spyable=` building.
    Spy,
    /// `Cloakable=yes`.
    Cloakable,
    /// `Sensors=yes` or `SensorsSight=`: it sees the invisible.
    Detector,
    /// `Spawns=`: it carries sub-units (V3, Dreadnought, carrier).
    Spawns,
    /// A weapon whose warhead is `Parasite=yes`.
    Parasite,
    /// A weapon whose warhead is `MindControl=yes`.
    MindControl,
    /// `UnitRepair=yes`: the shed a damaged vehicle enters.
    Repair,
    /// `Crusher=` or `OmniCrusher=`: it flattens what it drives over.
    Crusher,
    /// `ResourceGatherer=`: a harvester.
    Harvester,
    /// `ResourceDestination=`: an unloading bay.
    Refinery,
    /// At least one weapon in some slot, not `NeverUse=`.
    Armed,
    /// `DeployFire=`: the deployed pose fires a different slot.
    DeployFire,
}

impl Tag {
    /// Every tag, in the order a report prints them.
    pub const ALL: &'static [Tag] = &[
        Tag::Armed,
        Tag::Crusher,
        Tag::Gunner,
        Tag::Transport,
        Tag::Occupier,
        Tag::Deployer,
        Tag::DeployFire,
        Tag::Engineer,
        Tag::Infiltrator,
        Tag::Spy,
        Tag::Cloakable,
        Tag::Detector,
        Tag::Spawns,
        Tag::Parasite,
        Tag::MindControl,
        Tag::Repair,
        Tag::Harvester,
        Tag::Refinery,
    ];
}

/// The capabilities of one type, as a bit per tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities(u64);

impl Capabilities {
    /// Derive the table from the type's own keys and the warheads its weapons
    /// name. Every field read here is one the simulation already reads.
    pub fn of(rules: &RuleSet, ty: &ObjectType) -> Self {
        let mut set = Self::default();
        let mut armed = false;
        let mut parasite = false;
        let mut mind_control = false;
        for slot in 0..8 {
            let Some(weapon) = ty.weapon_at(slot).and_then(|name| rules.weapon(name)) else {
                continue;
            };
            if weapon.never_use {
                continue;
            }
            armed = true;
            let Some(warhead) = weapon
                .warhead
                .as_deref()
                .and_then(|name| rules.warhead(name))
            else {
                continue;
            };
            parasite |= warhead.parasite;
            mind_control |= warhead.mind_control;
        }
        set.set(Tag::Armed, armed);
        set.set(Tag::Crusher, ty.crusher || ty.omni_crusher);
        set.set(Tag::Gunner, ty.gunner);
        set.set(Tag::Transport, ty.passengers > 0);
        set.set(Tag::Occupier, ty.occupier);
        set.set(Tag::Deployer, ty.deployer);
        set.set(Tag::DeployFire, ty.deploy_fire);
        set.set(Tag::Engineer, ty.engineer);
        set.set(Tag::Infiltrator, ty.infiltrate);
        set.set(Tag::Spy, ty.agent);
        set.set(Tag::Cloakable, ty.cloakable);
        set.set(Tag::Detector, ty.sensors || ty.sensors_sight > 0);
        set.set(Tag::Spawns, ty.spawns.is_some());
        set.set(Tag::Parasite, parasite);
        set.set(Tag::MindControl, mind_control);
        set.set(Tag::Repair, ty.unit_repair);
        set.set(Tag::Harvester, ty.resource_gatherer);
        set.set(Tag::Refinery, ty.resource_destination);
        set
    }

    fn set(&mut self, tag: Tag, on: bool) {
        if on {
            self.0 |= 1 << tag as u32;
        }
    }

    pub fn has(self, tag: Tag) -> bool {
        self.0 & (1 << tag as u32) != 0
    }

    /// The tags this type carries, in [`Tag::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = Tag> {
        Tag::ALL.iter().copied().filter(move |tag| self.has(*tag))
    }
}

#[cfg(all(test, tests_rules_a_n))]
mod tests {
    use super::*;

    fn rules() -> Option<RuleSet> {
        crate::rules::retail_ini_fixture::retail_unlayered_rules()
    }

    fn caps(rules: &RuleSet, name: &str) -> Capabilities {
        let ty = rules.object(name).unwrap_or_else(|| panic!("{name}"));
        Capabilities::of(rules, ty)
    }

    /// The table reads the same keys the simulation does: a Grizzly crushes,
    /// an IFV does not; the IFV mans a turret; the Guardian GI digs in.
    #[test]
    fn the_table_derives_from_the_retail_keys() {
        let Some(rules) = rules() else {
            return;
        };
        assert!(caps(&rules, "MTNK").has(Tag::Crusher));
        assert!(!caps(&rules, "FV").has(Tag::Crusher));
        assert!(caps(&rules, "FV").has(Tag::Gunner));
        assert!(caps(&rules, "GGI").has(Tag::Deployer));
        assert!(caps(&rules, "GGI").has(Tag::DeployFire));
        assert!(caps(&rules, "CMIN").has(Tag::Harvester));
        assert!(caps(&rules, "GAREFN").has(Tag::Refinery));
        assert!(caps(&rules, "GADEPT").has(Tag::Repair));
        assert!(caps(&rules, "E1").has(Tag::Armed));
    }

    /// The capabilities the policy never asked about are named now, so the
    /// pilots that will use them can be written against a name.
    #[test]
    fn the_unused_capabilities_are_named() {
        let Some(rules) = rules() else {
            return;
        };
        assert!(caps(&rules, "SPY").has(Tag::Spy));
        assert!(caps(&rules, "SPY").has(Tag::Infiltrator));
        assert!(caps(&rules, "DRON").has(Tag::Parasite));
        assert!(!caps(&rules, "DRON").has(Tag::MindControl));
    }
}
