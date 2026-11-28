# Warhead reverse engineering

`[WarheadType] ReverseEngineer=yes` records each hit target's type for the
firing house through Ares' reverse-engineering list, as if the target had been
ground in a `ReverseEngineersVictims=` grinder. Plain `Valueable<bool>`, default
`no`, read by `Ext/WarheadType/Body.cpp:418` (constructor `Ext/WarheadType/Body.h:548`).

```ini
[WARHEAD]
ReverseEngineer=yes
```

## Native evidence

`Ext/WarheadType/Detonate.cpp:227-228` calls `ApplyReverseEngineer` after the
Ammo and RemoveDisguise one-time arms and before ReturnWarhead:

```cpp
void WarheadTypeExt::ApplyReverseEngineer(HouseClass* pHouse, TechnoClass* pTarget)
{
    if (pHouse && !pHouse->Type->MultiplayPassive && AresFunctions::ReverseEngineer)
        AresFunctions::ReverseEngineer(
            reinterpret_cast<void*>(pHouse->unknown_16084), pTarget->GetTechnoType());
}
```

The target is the victim's `TechnoTypeClass`; `unknown_16084` is the firing
house's HouseExt, so the call is the same HouseExt::ExtData::ReverseEngineer
(`10022360`) the Ares grinder arm uses. `AresFunctions::ReverseEngineer` is
non-null only with Ares installed.

## Where VERA20K owns it

- `rules::warhead_type::WarheadType::reverse_engineer`.
- `sim::reverse_engineering::Simulation::ares_reverse_engineer_house`
  (`sim/reverse_engineering.rs`): the house-level half split out of the
  grinder path — `CanBeReversed`/`ReversedAs`, the HouseExt `+0x34` record,
  the buildability recheck and House `+1FC`. The grinder arm
  (`ares_reverse_engineer`) now gates on `ReverseEngineersVictims=` and calls
  it.
- `sim::combat::crit`'s per-target pass calls it after RemoveDisguise and
  before ReturnWarhead when the firing house is not `MultiplayPassive`; the
  arm compiles only with both `phobos` and `ares`.

## Native anchors

`ReverseEngineer` runs inside the warhead body Phobos hooks at `BulletClass_Detonate` (`0x46920B`, `Ext/WarheadType/Hooks.cpp:8`) (`Ext/WarheadType/Detonate.cpp`), on the one-time effects of
`DetonateOnOneUnit`.

## Residuals

- The arm needs the Ares feature; a Phobos-only build reads the key but has no
  HouseExt list to write.
