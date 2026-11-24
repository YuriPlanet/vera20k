//! Phobos runtime banners (`New/Entity/BannerClass.cpp`): the scenario-level
//! instances the trigger actions 800-802 create, keyed by their action id.
//!
//! The scenario session owns the instances (`BannerClass::Array` is a
//! scenario global the save stream round-trips, `BannerClass.cpp:218-232`);
//! the app's banner pass resolves each against the `[BannerTypes]` catalog
//! and owns the blink phase its `Render` ticks per drawn frame.

/// One `BannerClass` (`BannerClass.h:9-21`). The position stays the action's
/// percent pair; the display scales it against the tactical view per frame,
/// as the native constructor and the replace arm both do
/// (`BannerClass.cpp:14-15`, `Ext/TAction/Body.cpp:829`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeBanner {
    /// `BannerClass::ID` — the action's Param3.
    pub id: i32,
    /// The `BannerTypeClass` the action's Text found at execution
    /// (`BannerTypeClass::Find(pTAction->Text)`,
    /// `Ext/TAction/Body.cpp:815`); the render re-resolves the name against
    /// the catalog.
    pub type_name: String,
    /// `BannerClass::Position`'s authored percent pair (Param4/Param5).
    pub position_percent: [i32; 2],
    /// `BannerClass::Variable` — the action's Param6.
    pub variable: i32,
    /// `BannerClass::IsGlobalVariable` — action 801.
    pub global_variable: bool,
}

/// The scenario's `BannerClass::Array` (`BannerClass.cpp:218-232`): the live
/// instances in creation order, created and deleted by trigger actions
/// 800-802.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct BannerList {
    banners: Vec<RuntimeBanner>,
}

impl BannerList {
    /// `CreateOrReplaceBanner` (`Ext/TAction/Body.cpp:813-843`): a found
    /// type is required; the existing id replaces type, position, variable
    /// and variable kind and keeps its blink phase, a new id appends with
    /// the type's period (`BannerClass.cpp:31-33`).
    pub(crate) fn create_or_replace(&mut self, banner: RuntimeBanner) {
        let existing = self
            .banners
            .iter_mut()
            .find(|instance| instance.id == banner.id);
        match existing {
            Some(instance) => {
                instance.type_name = banner.type_name;
                instance.position_percent = banner.position_percent;
                instance.variable = banner.variable;
                instance.global_variable = banner.global_variable;
            }
            None => self.banners.push(banner),
        }
    }

    /// `TActionExt::DeleteBanner` (`Ext/TAction/Body.cpp:858-875`): the
    /// first banner with the id goes; one action removes one banner.
    pub(crate) fn delete(&mut self, id: i32) -> bool {
        let position = self.banners.iter().position(|instance| instance.id == id);
        match position {
            Some(position) => {
                self.banners.remove(position);
                true
            }
            None => false,
        }
    }

    /// The live banners, in creation order (`BannerClass::Array`).
    pub(crate) fn entries(&self) -> &[RuntimeBanner] {
        &self.banners
    }
}
