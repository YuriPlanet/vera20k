//! Shared mutable BuildingType normal animation names written by4409BF.
use super::Simulation;
use crate::rules::ruleset::RuleSet;
use crate::sim::building_upgrades::BuildingUpgradeArtNames;
use crate::sim::intern::InternedId;

impl Simulation {
    /// The retired match stops owning live Type names before the process
    /// Rules owner's pre-reset read. Full_Init then deletes the old Types.
    pub fn retire_building_upgrade_art_names(&mut self) -> Vec<(String, u8, String)> {
        std::mem::take(&mut self.building_upgrade_art_names)
            .into_entries()
            .map(|((type_id, slot), image)| {
                (self.interner.resolve(type_id).to_owned(), slot, image)
            })
            .collect()
    }

    pub(crate) fn building_upgrade_art_names(&self) -> &BuildingUpgradeArtNames {
        &self.building_upgrade_art_names
    }

    pub(crate) fn set_building_upgrade_art_name(
        &mut self,
        type_id: InternedId,
        slot: u8,
        image: &str,
        rules: &RuleSet,
    ) {
        let old = self
            .building_upgrade_art_names
            .name(type_id, slot)
            .or_else(|| {
                let name = self.interner.resolve(type_id);
                let object = rules.object(name)?;
                object
                    .building_upgrade()
                    .power_up_art(slot)
                    .map(|record| record.normal_name())
            })
            .unwrap_or("");
        if !old.eq_ignore_ascii_case(image) {
            self.building_upgrade_art_names
                .set_name(type_id, slot, image);
        }
    }
}
