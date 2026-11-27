//! Superweapon references (appendix 7.9) and `find` over them (scope 8,
//! `SuperWeapons`).
//!
//! A superweapon reference names one House's Super of one type
//! ([`super::super_ref`]): every House constructs one Super per
//! `[SuperWeaponTypes]` entry once (`construct_house_supers`, the HouseClass
//! constructor's Supers vector) and nothing adds or removes one afterwards,
//! so the pair (house index, type index) is a fixed handle that never
//! expires. Superweapons have no creation serial in this engine; like
//! houses they sit outside the objects' counter on their own bit (see
//! `SUPER_REF_BIT`). Their sort key, house index then type index, is the
//! order the houses construct them in.
//!
//! | Property | Owner |
//! | --- | --- |
//! | `Type` | the Super's type: a `SuperWeapon` type reference (`[SuperWeaponTypes]` index) |
//! | `Owner` | the House holding it |
//! | `Ready` | granted (`SuperClass+0x6D`) and charged (`+0x6F`) |
//! | `ChargePercent` | 100 when ready, else the charge timer (`+0x30`/`+0x38`) against GetRechargeTime (`0x006CC260`), truncated; 0 when not granted. Writing runs stock action 129 (SetPercentage `0x006CC1E0` through TAction 129) |
//!
//! `find` lists the granted Supers only (an ungranted one is not the
//! house's to fire), filterable by `owner`, a `SuperWeapon` `type` and
//! clauses. Its area must be the whole map.

use vera20k_wasm_ext::{ActionParams, Area, FindPage, Invalid, NAME_SUPER_WEAPON, Query, Status};

use super::{Host, Reader, Ref, decode_ref, house_ref, super_ref, type_ref};
use crate::sim::superweapon::SuperWeaponInstance;

/// Stock action `SuperWeaponSetCharge` (TAction 129), which writing
/// `ChargePercent` runs.
const ACTION_SET_CHARGE: i32 = 129;

impl Reader<'_> {
    /// House `house`'s Super of type `index`, with the type's name.
    fn super_weapon(&self, house: usize, index: usize) -> Option<(&SuperWeaponInstance, &str)> {
        let owner = *self.sim.houses.order().get(house)?;
        let name = self.rules.super_weapon_order.get(index)?;
        let type_id = self.sim.interner.get(name)?;
        let instance = self.sim.super_weapons.get(&owner)?.get(&type_id)?;
        Some((instance, name.as_str()))
    }

    /// A superweapon's property (appendix 7.9); `Gone` for a reference that
    /// names no Super.
    pub(super) fn super_weapon_property(
        &self,
        house: usize,
        index: usize,
        name: &str,
    ) -> (i64, Status) {
        let Some((instance, type_name)) = self.super_weapon(house, index) else {
            return (0, Status::Gone);
        };
        let ready = instance.is_active() && instance.is_ready();
        let value = match name {
            "Type" => type_ref(NAME_SUPER_WEAPON, index as u32),
            "Owner" => house_ref(house),
            "Ready" => i64::from(ready),
            "ChargePercent" if ready => 100,
            "ChargePercent" if !instance.is_active() => 0,
            "ChargePercent" => {
                let Some(sw) = self.rules.super_weapon(type_name) else {
                    return (0, Status::NotFound);
                };
                let recharge = i64::from(instance.recharge_time(sw));
                if recharge <= 0 {
                    100
                } else {
                    let frame = self.sim.session.binary_frame() as i32;
                    let left = i64::from(instance.charge_remaining(frame)).clamp(0, recharge);
                    (recharge - left) * 100 / recharge
                }
            }
            _ => return (0, Status::Unsupported),
        };
        (value, Status::Ok)
    }
}

impl Host<'_, '_> {
    /// `set` on a superweapon: `ChargePercent` 0..=100 runs stock action
    /// 129 for its house and type (refused for an ungranted Super, as the
    /// action refuses); the other properties are read-only.
    pub(super) fn write_super_weapon(
        &mut self,
        house: usize,
        index: usize,
        name: &str,
        value: i64,
    ) -> Result<Status, Invalid> {
        if self.reader().super_weapon(house, index).is_none() {
            return Ok(Status::Gone);
        }
        Ok(match name {
            "ChargePercent" if (0..=100).contains(&value) => {
                let params = ActionParams {
                    id: index as i32,
                    value,
                    ..ActionParams::default()
                };
                let (_, status) = vera20k_wasm_ext::SimHost::action(
                    self,
                    ACTION_SET_CHARGE,
                    house_ref(house),
                    &params,
                    "",
                )?;
                status
            }
            "ChargePercent" | "Type" | "Owner" | "Ready" => Status::Refused,
            _ => Status::Unsupported,
        })
    }
}

impl Reader<'_> {
    /// `find` over granted Supers, by (house index, type index): one house
    /// with an `owner`, one type with a `SuperWeapon` `type`.
    pub(super) fn find_super_weapons(
        &self,
        query: &Query,
        cap: usize,
    ) -> Result<(FindPage, Status), Invalid> {
        if query.area != Area::Map {
            return Ok((FindPage::default(), Status::Unsupported));
        }
        let only_type = match decode_ref(query.type_ref)? {
            None => None,
            Some(Ref::Type {
                kind: NAME_SUPER_WEAPON,
                id,
            }) if (id as usize) < self.rules.super_weapon_order.len() => Some(id as usize),
            Some(_) => {
                return Err(Invalid(format!(
                    "{:#x} is not a SuperWeapon type for a SuperWeapons query",
                    query.type_ref
                )));
            }
        };
        let houses = match decode_ref(query.owner)? {
            None => 0..self.sim.houses.order().len(),
            Some(Ref::House(index)) if index < self.sim.houses.order().len() => index..index + 1,
            Some(Ref::House(_)) => return Ok((FindPage::default(), Status::Gone)),
            Some(_) => {
                return Err(Invalid(format!(
                    "{:#x} is not a house reference",
                    query.owner
                )));
            }
        };
        let types = match only_type {
            Some(index) => index..index + 1,
            None => 0..self.rules.super_weapon_order.len(),
        };
        let mut page = FindPage::default();
        'houses: for house in houses {
            for index in types.clone() {
                let key = super_key(house, index);
                if key < query.after {
                    continue;
                }
                if page.found.len() == cap {
                    break 'houses;
                }
                let granted = self
                    .super_weapon(house, index)
                    .is_some_and(|(instance, _)| instance.is_active());
                // An ungranted Super, or (for a client) another house's.
                if !granted
                    || self
                        .gate
                        .is_some_and(|gate| !gate.lists(super_ref(house, index)))
                {
                    if !page.skip(key) {
                        break 'houses;
                    }
                    continue;
                }
                if !page.examine(key, query) {
                    break 'houses;
                }
                let reference = super_ref(house, index);
                match self.passes(reference, query)? {
                    Some(true) => page.found.push((key, reference)),
                    Some(false) => {}
                    None => return Ok((FindPage::default(), Status::Unsupported)),
                }
            }
        }
        Ok((page, Status::Ok))
    }
}

/// A Super's sort key: house index, then type index.
const fn super_key(house: usize, index: usize) -> i64 {
    ((house as i64) << 16) | index as i64
}
