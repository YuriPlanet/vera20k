# TiberiumEater

A Foot type that eats tiberium under its cells for credits.

```ini
[SOMETECHNO]                          ; TechnoType
TiberiumEater.TransDelay=             ; int, >= 0 enables the eater and sets its delay
TiberiumEater.CashMultiplier=1.0      ; float
TiberiumEater.AmountPerCell=0         ; int, 0 takes the whole cell
TiberiumEater.Cell0=0,0               ; cell offsets (x,y); Cell1=.. continues
TiberiumEater.Display=true            ; bool
TiberiumEater.DisplayToHouse=all      ; AffectedHouse
TiberiumEater.DisplayOffset=0,0       ; point
TiberiumEater.Anims=                  ; fallback anim list
TiberiumEater.Anims.Tiberium0=        ; per-tiberium-index anim list
TiberiumEater.AnimMove=true           ; bool
TiberiumEater.UnderEMP=false          ; bool
```

Reader `Ext/TechnoType/Body.cpp:1402-1414`, settings
`New/Type/Affiliated/TiberiumEaterTypeClass.cpp:5-55`; constructor defaults
`TiberiumEaterTypeClass.h:8-21`. `TransDelay >= 0` creates the settings, a
negative value disables them, an absent key keeps the current state.

## Behavior

`FootExt::UpdateTiberiumEater` (`Ext/Foot/Body.cpp:15-121`), the
`FootClass_AI` tail step (`Ext/Techno/Hooks.cpp:44`):

- a limbo'd object, or a deactivated/EMP'd one unless `UnderEMP`, holds a
  running timer with `StartTime++` and returns;
- a running `TransDelay` timer defers the scan; otherwise every
  `TiberiumEater.Cell{i}` offset becomes a world coordinate
  (`GetFLHAbsoluteCoords`, body matrix only), and each tiberium cell there is
  reduced by `min(AmountPerCell, value / TiberiumClass.Value)` (the whole
  amount when `AmountPerCell <= 0`);
- the owner is paid
  `round(amount * Value * CashMultiplier * (1 + OrePurifiers * PurifierBonus))`
  credits, a money label is emitted when `Display` is on and the local
  player sees the object, and an anim is constructed at the cell from the
  tiberium index's list (the per-index list when set, else `Anims`; one
  entry plays directly, eight pick by the body facing index, any other
  count draws `RandomRanged(0, count-1)`), owned by the object's house and
  attached to it when `AnimMove`;
- any eaten cell starts the `TransDelay` timer.

## Native anchors

`FootClass_AI` (`0x4DA54E`, `Ext/Techno/Hooks.cpp:40`) is the visit whose body runs the eater; the
cell offsets come from the type's `TiberiumEater.Cell0..N`, not from a second site.

## Where VERA20K owns it

`rules::phobos_tiberium_eater` reads the keys onto `ObjectType`
(`phobos_tiberium_eater: Option<..>`; the type has no eater without a
non-negative `TransDelay`). `sim::tiberium_eater` runs from the `FootClass_AI`
tail before the health-band conversion, uses `combat::fire_coord::
custom_flh_coordinate` for the offsets, the shared
`reduce_tiberium_at_with_native_context` for `CellClass::ReduceTiberium`,
`credit_income::transact_money` and the `BountyText` money label,
`spawn_owned_anim` for the anim, and `FootExt::TiberiumEater_Timer` on the
entity for the delay.

The money label keeps `FlyingStrings::AddMoneyString`'s gates in the client
consumer (`flying_credits::admit`): the impact point's shroud for the local
viewer, the `DisplayToHouse`/owner filter, then the source's
visual-character (`Hidden`) test; no residual.

RESIDUAL: `GetFLHAbsoluteCoords(..., false)` reuses the shared body-matrix
FLH port; a building/voxel-weapon pivot chain is not re-projected here (the
eater runs on Foot types only).

RESIDUAL: `Cell{i}` values are cell offsets in the INI; native stores them
multiplied by `LeptonsPerCell` (256), which VERA reproduces. A `<default>`
`Anims.Tiberium{i}` clears the per-index list so the general `Anims` applies;
unknown anim names are dropped from both before the 1/8/random arms count
them, as the native type vectors only hold resolvable entries.

RESIDUAL: VERA reads the merged section once. Native keeps the eater object
across INI layers, so a later pass that repeats
`TiberiumEater.TransDelay=<default>` (presence cleared) keeps the earlier
positive delay, and a negative pass destroys the settings while a later
positive one recreates constructor defaults before reading only that pass's
keys. VERA sees only the merged value: a cleared delay disables the eater and
a remove/re-add sequence keeps earlier key values. Trigger: layered rules
that clear or toggle `TiberiumEater.TransDelay`; effect: the delay/settings
native retains can differ.
