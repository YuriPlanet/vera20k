//! Phobos TiberiumEater (`Ext/Foot/Body.cpp:15-121`,
//! `New/Type/Affiliated/TiberiumEaterTypeClass.cpp`): a Foot type that eats
//! tiberium under FLH cell offsets for credits.
//!
//! Enable state (`Ext/TechnoType/Body.cpp:1402-1414`): the per-type
//! `TiberiumEater.TransDelay` is a `Nullable<int>`; a value `>= 0` creates
//! the eater settings (constructor defaults), a negative value disables it,
//! and an absent key keeps the current state. VERA parses the merged section
//! once, so the presence rules reduce to "set and non-negative".
//!
//! Settings (`TiberiumEaterTypeClass.h:8-21`): `TransDelay` -1,
//! `CashMultiplier` 1.0f, `AmountPerCell` 0, `Cells` one `(0,0)`,
//! `Display` true, `DisplayToHouse` All, `DisplayOffset` empty, `Anims`
//! empty, `Anims.Tiberium0..3` unset, `AnimMove` true, `UnderEMP` false.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::ini_value::crt_atoi;
use crate::rules::phobos_shields::AffectedHouse;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhobosTiberiumEater {
    pub trans_delay: i32,
    pub cash_multiplier: f32,
    pub amount_per_cell: i32,
    /// `Cell{i}=x,y` offsets, in leptons (cells × 256).
    pub cells: Vec<(i32, i32)>,
    pub display: bool,
    pub display_to_house: AffectedHouse,
    pub display_offset: [i32; 2],
    pub anims: Vec<String>,
    /// `Anims.Tiberium{i}=`: `None` when the key is absent, so the general
    /// `Anims` list applies (`NullableVector::GetElements`).
    pub anims_tiberiums: [Option<Vec<String>>; 4],
    pub anim_move: bool,
    pub under_emp: bool,
}

/// `TechnoTypeExt::LoadFromINI`'s eater block: `None` when the type has no
/// eater (`TransDelay` unset or negative after the merged passes).
pub fn parse(section: &IniSection) -> Option<PhobosTiberiumEater> {
    let trans_delay = section.read_phobos_nullable_int("TiberiumEater.TransDelay", None)?;
    if trans_delay < 0 {
        return None;
    }
    Some(from_section(section, trans_delay))
}

fn from_section(section: &IniSection, trans_delay: i32) -> PhobosTiberiumEater {
    let mut cells = vec![(0_i32, 0_i32)];
    for index in 0.. {
        let pair = cell_pair(section, &format!("TiberiumEater.Cell{index}"));
        if index >= cells.len() {
            let Some(pair) = pair else {
                break;
            };
            cells.push(pair);
        } else if let Some(pair) = pair {
            cells[index] = pair;
        }
    }
    // `TiberiumEaterTypeClass::LoadFromINI` stores `x * LeptonsPerCell`.
    for cell in &mut cells {
        cell.0 = cell.0.wrapping_mul(256);
        cell.1 = cell.1.wrapping_mul(256);
    }
    let list = |key: &str| -> Option<Vec<String>> {
        // `NullableVector<T>::Read`: `<default>` clears the presence so
        // `GetElements` falls back to the general list.
        let raw = section.read_name(key, 0x800)?;
        if raw.trim().eq_ignore_ascii_case("<default>") {
            return None;
        }
        section
            .read_list(key, 0x800)
            .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
    };
    let mut anims_tiberiums: [Option<Vec<String>>; 4] = Default::default();
    for (index, slot) in anims_tiberiums.iter_mut().enumerate() {
        *slot = list(&format!("TiberiumEater.Anims.Tiberium{index}"));
    }
    PhobosTiberiumEater {
        trans_delay,
        cash_multiplier: section.read_phobos_double("TiberiumEater.CashMultiplier", 1.0) as f32,
        amount_per_cell: section.read_int("TiberiumEater.AmountPerCell", 0),
        cells,
        display: section.read_ares_bool("TiberiumEater.Display", true),
        display_to_house: crate::rules::phobos_shields::read_affected_house(
            section,
            "TiberiumEater.DisplayToHouse",
            AffectedHouse::ALL,
        ),
        display_offset: cell_pair(section, "TiberiumEater.DisplayOffset")
            .map_or([0, 0], |(x, y)| [x, y]),
        anims: list("TiberiumEater.Anims").unwrap_or_default(),
        anims_tiberiums,
        anim_move: section.read_ares_bool("TiberiumEater.AnimMove", true),
        under_emp: section.read_ares_bool("TiberiumEater.UnderEMP", false),
    }
}

/// `Nullable<Vector2D<int>>` / `Valueable<Point2D>`: two comma-separated
/// integers; anything else leaves the value unset.
fn cell_pair(section: &IniSection, key: &str) -> Option<(i32, i32)> {
    let raw = section.read_name(key, 0x800)?;
    let mut parts = raw.split(',');
    let x = parts.next()?;
    let y = parts.next()?;
    if x.trim().is_empty() || y.trim().is_empty() {
        return None;
    }
    Some((crt_atoi(x), crt_atoi(y)))
}
