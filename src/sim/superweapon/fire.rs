//! Firing a superweapon: `HouseClass::Fire_SW @ 0x004FAE50`, which the
//! SPECIAL_PLACE event (`EventClass::Execute 0x004C78D6`) calls for a
//! player's click and the computer's launchers call directly;
//! `SuperClass::ClickFire @ 0x006CB920`; the dispatch of
//! `SuperClass::Launch @ 0x006CC390`; and the computer houses' alert
//! (`HouseClass__Super_Defense_Alert @ 0x004FAF00`).
//!
//! Fire_SW clicks the Super, then hands it and the cell to every house in
//! reverse HouseClass::Array order, whether or not the click launched. The
//! Super index Fire_SW takes is the type's `[SuperWeaponTypes]` index, the
//! command's type.
//!
//! A `PostClick=` type (retail ChronoWarpSpecial) pairs with the Super at its
//! `PreDependent=` index: Fire_SW copies that Super's map cell before the
//! click (`0x004FAE6C..0x004FAE8F`) and afterwards clears its charge
//! (`SuperClass::SetReadiness @ 0x006CB820`) and releases its anim
//! (`SuperClass::StopPreclickAnim @ 0x006CB830`), which restarts its
//! recharge. Every house holds one Super per `[SuperWeaponTypes]` entry from
//! its constructor (`0x004F620E..0x004F6290`); VERA creates an entry on its
//! first grant or click ([`super_instance`]).
//!
//! RESIDUALS:
//! - A `PostClick=` type whose `PreDependent=` is unset or past the list
//!   (constructor -1) reads outside the Supers vector natively; VERA pairs
//!   nothing. Dormant in retail.
//! - Dormant in retail data: ClickFire's charge-drain arm
//!   (`0x006CBB8E..0x006CBCA0`; no retail type sets `UseChargeDrain=`) and
//!   its one-time arm (`0x006CBB3B..0x006CBB8A`; VERA grants no one-time
//!   Super).
//! - Presentation: the player's message for a refused Lightning Storm
//!   (`LightningStorm::PrintMessage @ 0x0053AE00`) or Psychic Dominator
//!   (`PsyDom::PrintMessage @ 0x0053B410`).

#[cfg(test)]
#[path = "fire_tests.rs"]
mod tests;

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::{SuperWeaponKind, SuperWeaponType};
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

impl Simulation {
    /// `HouseClass::Fire_SW @ 0x004FAE50` for `owner`'s Super of type
    /// `sw_type_id` at `cell`. Returns whether ClickFire launched (native
    /// returns 1 either way).
    pub(crate) fn fire_super_weapon(
        &mut self,
        rules: &RuleSet,
        owner: InternedId,
        sw_type_id: InternedId,
        cell: (u16, u16),
        overlay_registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        #[cfg(test)]
        super::ai_fire::observe(super::ai_fire::AiFireEvent::Fire(sw_type_id, cell));
        let Some(sw) = rules.super_weapon(self.interner.resolve(sw_type_id)) else {
            return false;
        };
        let paired = sw
            .post_click
            .then(|| pre_dependent(self, rules, sw))
            .flatten();
        if let Some((pre_type, _)) = paired {
            let cell = super_instance(self, owner, pre_type).chrono_cell();
            super_instance(self, owner, sw_type_id).chrono_cell = cell;
        }
        let launched = click_fire(self, rules, owner, sw_type_id, sw, cell, overlay_registry);
        if let Some((pre_type, pre_sw)) = paired {
            // `0x004FAEA6..0x004FAEC1`.
            super_instance(self, owner, pre_type).is_ready = false;
            self.release_super_anim(owner, pre_type);
            let frame = self.session.binary_frame;
            super_instance(self, owner, pre_type).stop_preclick(pre_sw, frame);
        }
        // `0x004FAEC6..0x004FAEF2`: every house, last to first.
        let houses: Vec<InternedId> = self.session.house_order.iter().rev().copied().collect();
        for house in houses {
            alert_super_weapon_defense(self, rules, house, sw, cell);
        }
        launched
    }
}

/// `SuperClass::ClickFire @ 0x006CB920` without charge drain: an admitted
/// Super launches unless a Lightning Storm finds one raging or counting
/// down (`0x006CB963`) or a Psychic Dominator finds one active
/// (`0x006CBAD6`), refusals that keep the charge; then readiness and
/// recharge as [`super::SuperWeaponInstance::finish_click_fire`].
fn click_fire(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type_id: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
    if sw.use_charge_drain {
        return false;
    }
    let instance = super_instance(sim, owner, sw_type_id);
    if !instance.click_fire_admits() && !sw.post_click {
        return false;
    }
    if sw.kind == SuperWeaponKind::LightningStorm && super::lightning_storm::has_deferment(sim) {
        return false;
    }
    if sw.kind == SuperWeaponKind::PsychicDominator && super::psychic_dominator::active(sim) {
        return false;
    }
    let launched = launch(sim, rules, owner, sw_type_id, sw, cell, overlay_registry);
    // `0x006CBBDE..0x006CBC34`: a type that is neither pairs no anim.
    if !sw.manual_control && !sw.pre_click && !sw.post_click {
        sim.release_super_anim(owner, sw_type_id);
    }
    let frame = sim.session.binary_frame;
    if let Some(instance) = sim
        .super_weapons
        .get_mut(&owner)
        .and_then(|weapons| weapons.get_mut(&sw_type_id))
    {
        instance.finish_click_fire(sw, frame);
    }
    launched
}

/// `SuperClass::Launch @ 0x006CC390`: the case of the type's `Type=`
/// (`+0xB4`, jump table `0x006CDE44`). Returns whether the case did
/// anything; native returns nothing.
fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type_id: InternedId,
    sw: &SuperWeaponType,
    (rx, ry): (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
    use super::paradrop::ParaDropKind;
    match sw.kind {
        SuperWeaponKind::MultiMissile => {
            super::nuke::launch(sim, rules, owner, sw_type_id, sw, (rx, ry))
        }
        SuperWeaponKind::LightningStorm => {
            super::lightning_storm::start(sim, rules, owner, rx, ry, sw_type_id)
        }
        SuperWeaponKind::IronCurtain => {
            super::iron_curtain::launch(sim, rules, owner, rx, ry, sw_type_id, overlay_registry)
        }
        SuperWeaponKind::ForceShield => {
            super::force_shield::launch(sim, rules, owner, rx, ry, sw_type_id)
        }
        SuperWeaponKind::GeneticConverter => super::genetic_converter::launch(
            sim,
            rules,
            owner,
            rx,
            ry,
            sw_type_id,
            overlay_registry,
        ),
        SuperWeaponKind::PsychicReveal => {
            super::psychic_reveal::launch(sim, rules, owner, rx, ry, sw_type_id)
        }
        SuperWeaponKind::ParaDrop => {
            super::paradrop::launch(sim, rules, owner, rx, ry, ParaDropKind::Generic, sw_type_id)
        }
        SuperWeaponKind::AmerParaDrop => super::paradrop::launch(
            sim,
            rules,
            owner,
            rx,
            ry,
            ParaDropKind::American,
            sw_type_id,
        ),
        SuperWeaponKind::ChronoSphere => {
            super::chronosphere::launch_source(sim, rules, owner, sw_type_id, (rx, ry))
        }
        SuperWeaponKind::ChronoWarp => super::chronosphere::launch_warp(
            sim,
            rules,
            owner,
            sw_type_id,
            (rx, ry),
            overlay_registry,
        ),
        SuperWeaponKind::PsychicDominator => {
            super::psychic_dominator::launch(sim, rules, owner, sw_type_id, (rx, ry))
        }
        SuperWeaponKind::SpyPlane => {
            super::spy_plane::launch(sim, rules, owner, sw_type_id, (rx, ry))
        }
    }
}

/// `owner`'s Super of `sw_type`, created inactive on first use: native
/// constructs every house's Supers with the house.
fn super_instance(
    sim: &mut Simulation,
    owner: InternedId,
    sw_type: InternedId,
) -> &mut super::SuperWeaponInstance {
    sim.super_weapons
        .entry(owner)
        .or_default()
        .entry(sw_type)
        .or_insert_with(|| super::SuperWeaponInstance::new(sw_type, owner))
}

/// The Super Fire_SW pairs a `PostClick=` type with: the `[SuperWeaponTypes]`
/// entry at its `PreDependent=` index (`HouseClass+0x258` indexed by
/// `Type+0xF0`, `0x004FAE6C..0x004FAE7E`).
fn pre_dependent<'r>(
    sim: &mut Simulation,
    rules: &'r RuleSet,
    sw: &SuperWeaponType,
) -> Option<(InternedId, &'r SuperWeaponType)> {
    let name = rules
        .super_weapon_order
        .get(usize::try_from(sw.pre_dependent).ok()?)?;
    Some((sim.interner.intern(name), rules.super_weapon(name)?))
}

/// `HouseClass @ 0x004FAF00` for `house`, given the fired Super's type and
/// its cell: a computer house (not human, `+0x1EC`, nor MultiplayPassive,
/// HouseType `+0x1A6`) facing a type with `AIDefendAgainst=` (`+0xEC`)
/// measures from its base to the cell; within `AISuperDefenseDistance=`
/// (`Rules+0xEE4`) one Scenario draw `RandomRanged(0, 99)` (`0x0065C7E0`,
/// `0x004FB03D`) at most its difficulty's `AISuperDefenseProbability=`
/// (`Rules+0xEC8`, indexed by `+0x184`) alerts it: the cell it defends
/// (`+0x54F4`) becomes its first construction yard's (`+0x54`, the yard's
/// GetCoords `0x00447AC0` over 256, toward zero), else its base cell, and
/// the frame is stored (`+0x54FC`).
///
/// The base is the alternate centre (`+0x5494`) unless empty, else the
/// primary (`+0x5490`); both points are a cell's GetCoords (vt+0x48,
/// `0x00486840`) through the never-null map lookup (`0x005657A0`), an empty
/// base the origin (the zeroed static `0x00A8EFF8`), and the distance is
/// `CoordStruct::Distance3D @ 0x0041C380` of base minus cell.
fn alert_super_weapon_defense(
    sim: &mut Simulation,
    rules: &RuleSet,
    house_id: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
) {
    let Some(house) = sim.houses.get(&house_id) else {
        return;
    };
    if house.multiplay_passive || house.is_human || !sw.ai_defend_against {
        return;
    }
    let base = house.base_origin();
    let probability = house.difficulty_value(&rules.general.ai_super_defense_probability);
    let first_yard = house.build_const_order.first().copied();
    let base_coords = if base == (0, 0) {
        [0, 0, 0]
    } else {
        cell_coords(sim, base)
    };
    let distance =
        crate::util::native_x87::distance_3d_leptons(base_coords, cell_coords(sim, cell));
    if distance > rules.general.ai_super_defense_distance {
        return;
    }
    if sim.scenario_rng.next_range_i32_inclusive(0, 99) > probability {
        return;
    }
    let defended = first_yard
        .and_then(|yard| sim.substrate.entities.get(yard))
        .map(|yard| {
            let coords = crate::sim::movement::ground_pose::object_get_coords(
                yard,
                sim.resolved_terrain.as_ref(),
            );
            ((coords.x / 256) as u16, (coords.y / 256) as u16)
        })
        .unwrap_or(base);
    let frame = sim.session.binary_frame as i32;
    if let Some(house) = sim.houses.get_mut(&house_id) {
        house.alert_super_weapon_defense(defended, frame);
    }
}

/// A cell's GetCoords (vt+0x48, `0x00486840`).
pub(super) fn cell_coords(sim: &Simulation, (x, y): (u16, u16)) -> [i32; 3] {
    let coord = crate::sim::projectile::cell_ground_coord(sim.resolved_terrain.as_ref(), x, y);
    [coord.x, coord.y, coord.z]
}
