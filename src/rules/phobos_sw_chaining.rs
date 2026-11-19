//! Phobos `SW.Next`/`SW.Link`: chained superweapon launches and linked
//! charge groups.
//!
//! Reader `SWTypeExt::LoadFromINIFile` (`src/Ext/SWType/Body.cpp:170-243`):
//! `SW.Next` (`ValueableIdxVector<SuperWeaponTypeClass>`), the three
//! `SW.Next.*` flags, `SW.Next.RollChances` (`ValueableVector<float>`), the
//! numbered `SW.Next.RandomWeightsN` lists (stopping at the first empty one)
//! and their `SW.Next.RandomWeights` alias for slot 0; then `SW.Link`, the
//! three `SW.Link.*` flags, `SW.Link.RollChances` and the same weight-list
//! pair. Declarations and constructor defaults: `Ext/SWType/Body.h:72-76`,
//! `:99-100`, `:114-118`, `:177-182`, `:202-207`.
//!
//! Consumers: `SWTypeExt::ApplySWNext` (`Ext/SWType/FireSuperWeapon.cpp:305`)
//! and `SWTypeExt::ApplyLinkedSW` (`:409`) from the shared
//! `FireSuperWeaponExt` tail (`:13-40`), driven here by
//! [`crate::sim::superweapon::chaining`].
//!
//! `ValueableIdxVector<SuperWeaponType>` resolves each comma token through
//! `SuperWeaponTypeClass::FindIndex`; an unknown name is dropped at read time
//! and a reached list replaces the field. The weight vectors keep their
//! integer weights verbatim, including zero and negative values; the roll
//! chances are `Parser<float>` values stored as binary32 bits. The native
//! fields live on the retained `SWTypeExt`, so a later rules layer that omits
//! a key keeps the earlier layer's value, and the numbered weight vectors
//! grow through the whole load instead of being rebuilt per layer
//! (`Body.cpp:184-235`, `:253-265`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF32Bits;

/// One superweapon type's chaining data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct PhobosSuperWeaponChaining {
    /// `SW.Next=`: the superweapon types launched after this one.
    next: Vec<String>,
    /// `SW.Next.RealLaunch=`, constructor `true`: require the next type to be
    /// present, charged, unsuspended and affordable, and consume its charge.
    /// False launches it forcibly and restores the charge timer.
    next_real_launch: bool,
    /// `SW.Next.IgnoreInhibitors=`, constructor `false`.
    next_ignore_inhibitors: bool,
    /// `SW.Next.IgnoreDesignators=`, constructor `true`.
    next_ignore_designators: bool,
    /// `SW.Next.RollChances=`: per-roll chances as binary32 bits.
    next_roll_chances: Vec<NativeF32Bits>,
    /// `SW.Next.RandomWeightsN=` (+ the `SW.Next.RandomWeights` alias for the
    /// first list): per-roll weight lists.
    next_random_weights: Vec<Vec<i32>>,
    /// `SW.Link=`: the superweapon types this launch grants, readies or resets.
    link: Vec<String>,
    /// `SW.Link.Grant=`, constructor `false`: grant a missing linked type.
    link_grant: bool,
    /// `SW.Link.Ready=`, constructor `false`: fully charge a present one.
    link_ready: bool,
    /// `SW.Link.Reset=`, constructor `false`: restart a present one's charge.
    link_reset: bool,
    /// `SW.Link.RollChances=`.
    link_roll_chances: Vec<NativeF32Bits>,
    /// `SW.Link.RandomWeightsN=` / `SW.Link.RandomWeights`.
    link_random_weights: Vec<Vec<i32>>,
}

impl Default for PhobosSuperWeaponChaining {
    fn default() -> Self {
        Self {
            next: Vec::new(),
            next_real_launch: true,
            next_ignore_inhibitors: false,
            next_ignore_designators: true,
            next_roll_chances: Vec::new(),
            next_random_weights: Vec::new(),
            link: Vec::new(),
            link_grant: false,
            link_ready: false,
            link_reset: false,
            link_roll_chances: Vec::new(),
            link_random_weights: Vec::new(),
        }
    }
}

impl PhobosSuperWeaponChaining {
    /// `SWTypeExt::LoadFromINIFile`'s `SW.Next*`/`SW.Link*` reads into the
    /// retained extension. Native calls this once per reached rules layer on
    /// the same `SWTypeExt`, so a key this layer does not reach keeps the
    /// earlier layer's value, and the weight vectors keep the native
    /// replacement/append rules ([`read_numbered_weights`],
    /// [`read_appended_weights`]).
    /// `resolve` answers the canonical registered superweapon name for a
    /// token (`SuperWeaponTypeClass::FindIndex`), or `None` for an unknown
    /// one.
    pub(crate) fn from_section(
        previous: Self,
        section: &IniSection,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Self {
        let mut out = previous;
        if let Some(next) = read_idx_vector(section, "SW.Next", &resolve) {
            out.next = next;
        }
        if let Some(value) = section.read_bool_value("SW.Next.RealLaunch") {
            out.next_real_launch = value;
        }
        if let Some(value) = section.read_bool_value("SW.Next.IgnoreInhibitors") {
            out.next_ignore_inhibitors = value;
        }
        if let Some(value) = section.read_bool_value("SW.Next.IgnoreDesignators") {
            out.next_ignore_designators = value;
        }
        if let Some(chances) = read_roll_chances(section, "SW.Next.RollChances") {
            out.next_roll_chances = chances;
        }
        read_numbered_weights(
            section,
            "SW.Next",
            "SW.Next.RandomWeights",
            &mut out.next_random_weights,
        );
        if let Some(link) = read_idx_vector(section, "SW.Link", &resolve) {
            out.link = link;
        }
        if let Some(value) = section.read_bool_value("SW.Link.Grant") {
            out.link_grant = value;
        }
        if let Some(value) = section.read_bool_value("SW.Link.Ready") {
            out.link_ready = value;
        }
        if let Some(value) = section.read_bool_value("SW.Link.Reset") {
            out.link_reset = value;
        }
        if let Some(chances) = read_roll_chances(section, "SW.Link.RollChances") {
            out.link_roll_chances = chances;
        }
        read_appended_weights(
            section,
            "SW.Link",
            "SW.Link.RandomWeights",
            &mut out.link_random_weights,
        );
        out
    }

    pub(crate) fn next(&self) -> &[String] {
        &self.next
    }

    pub(crate) fn next_real_launch(&self) -> bool {
        self.next_real_launch
    }

    pub(crate) fn next_ignore_inhibitors(&self) -> bool {
        self.next_ignore_inhibitors
    }

    pub(crate) fn next_ignore_designators(&self) -> bool {
        self.next_ignore_designators
    }

    pub(crate) fn next_roll_chances(&self) -> Vec<f64> {
        roll_chances(&self.next_roll_chances)
    }

    pub(crate) fn next_random_weights(&self) -> &[Vec<i32>] {
        &self.next_random_weights
    }

    pub(crate) fn link(&self) -> &[String] {
        &self.link
    }

    pub(crate) fn link_grant(&self) -> bool {
        self.link_grant
    }

    pub(crate) fn link_ready(&self) -> bool {
        self.link_ready
    }

    pub(crate) fn link_reset(&self) -> bool {
        self.link_reset
    }

    pub(crate) fn link_roll_chances(&self) -> Vec<f64> {
        roll_chances(&self.link_roll_chances)
    }

    pub(crate) fn link_random_weights(&self) -> &[Vec<i32>] {
        &self.link_random_weights
    }
}

fn roll_chances(chances: &[NativeF32Bits]) -> Vec<f64> {
    chances
        .iter()
        .map(|bits| f64::from(f32::from_bits(bits.bits())))
        .collect()
}

/// `ValueableIdxVector<SuperWeaponType>`: absent keeps the field, a reached
/// list replaces it, and each unknown token is dropped.
fn read_idx_vector(
    section: &IniSection,
    key: &str,
    resolve: &impl Fn(&str) -> Option<String>,
) -> Option<Vec<String>> {
    section.read_list(key, 2048).map(|tokens| {
        tokens
            .into_iter()
            .filter_map(|token| resolve(token.trim()))
            .collect()
    })
}

/// `ValueableVector<float>`: a present list replaces the field; each token is
/// a Phobos float (binary32) and a token that does not convert is skipped.
fn read_roll_chances(section: &IniSection, key: &str) -> Option<Vec<NativeF32Bits>> {
    section.read_phobos_double_list(key).map(|chances| {
        chances
            .into_iter()
            .map(|chance| NativeF32Bits::from_bits((chance as f32).to_bits()))
            .collect()
    })
}

/// The numbered weight lists then the unnumbered alias, as native updates the
/// retained `SW.Next_RandomWeightsData`/`LimboDelivery_RandomWeightsData`
/// (`Body.cpp:184-235`): a numbered slot below the retained length is
/// replaced, a longer one is appended, and the loop stops at the first
/// empty/missing list without touching the retained tail. The alias then
/// replaces or creates slot 0. Both native loops share this body; it lives
/// here because the `SW.Link` sibling below belongs to the same reader.
pub(crate) fn read_numbered_weights(
    section: &IniSection,
    prefix: &str,
    alias: &str,
    retained: &mut Vec<Vec<i32>>,
) {
    for index in 0.. {
        let key = format!("{prefix}.RandomWeights{index}");
        let Some(weights) = section.read_int_list(&key) else {
            break;
        };
        if weights.is_empty() {
            break;
        }
        if retained.len() > index {
            retained[index] = weights;
        } else {
            retained.push(weights);
        }
    }
    if let Some(weights) = section.read_int_list(alias)
        && !weights.is_empty()
    {
        if retained.is_empty() {
            retained.push(weights);
        } else {
            retained[0] = weights;
        }
    }
}

/// `SW.Link`'s numbered loop (`Body.cpp:253-263`) has no replace arm: every
/// reached list is appended to the retained vector, so a later rules pass
/// duplicates the whole list run. The alias below still patches slot 0. This
/// is Phobos' own behavior (a plain `emplace_back`), reproduced as written.
fn read_appended_weights(
    section: &IniSection,
    prefix: &str,
    alias: &str,
    retained: &mut Vec<Vec<i32>>,
) {
    for index in 0.. {
        let key = format!("{prefix}.RandomWeights{index}");
        let Some(weights) = section.read_int_list(&key) else {
            break;
        };
        if weights.is_empty() {
            break;
        }
        retained.push(weights);
    }
    if let Some(weights) = section.read_int_list(alias)
        && !weights.is_empty()
    {
        if retained.is_empty() {
            retained.push(weights);
        } else {
            retained[0] = weights;
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_sw_chaining_tests.rs"]
mod tests;
