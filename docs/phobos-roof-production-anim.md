# Roof production anims

Phobos is in every build; turn it on from a game's data to give the factory unload animation its own
art and placement when the produced unit leaves through the roof (a JumpJet or
other `ExitThroughRoof` unit).

```ini
[SOMEBUILDING]                          ; ART section
RoofProductionAnim=                     ; AnimationType
RoofProductionAnimDamaged=              ; AnimationType
RoofProductionAnimGarrisoned=           ; AnimationType
RoofProductionAnimX=                    ; integer, defaults to ProductionAnimX
RoofProductionAnimY=                    ; integer, defaults to ProductionAnimY
RoofProductionAnimZAdjust=              ; integer, defaults to ProductionAnimZAdjust
RoofProductionAnimYSort=                ; integer, defaults to ProductionAnimYSort
RoofProductionAnimPowered=              ; boolean, defaults to ProductionAnimPowered
RoofProductionAnimPoweredLight=         ; boolean, defaults to ProductionAnimPoweredLight
RoofProductionAnimPoweredEffect=        ; boolean, defaults to ProductionAnimPoweredEffect
RoofProductionAnimPoweredSpecial=       ; boolean, defaults to ProductionAnimPoweredSpecial
```

## Behavior

`PickRoofProductionAnim` (`Ext/Building/Hooks.cpp:1080-1089`) picks
`Garrisoned` → `Damaged` → the base key; a null pick falls back to the plain
`ProductionAnim` name, never to the plain damaged/garrisoned variants.

`PlayRoofProductionAnim` (`:1091-1165`) plays slot 8 with the roof name and a
temporary override of the slot's X/Y/ZAdjust/YSort and powered flags; the
override only applies to this play, the type record keeps its stock values.
`TryPlayRoofProductionAnim` (`:1167-1188`) first blocks the play through
`IsPoweredAnimBlocked` over the same effective flags, and returns handled in
either case so the stock PlayAnim is skipped for a roof-exit unit. VERA models
the temporary record mutation as a per-call `BuildingAnimConfig` override.

The three call sites:
- Infantry factory exit (`0x444D11`, `Ext/Building/Hooks.cpp:1210-1240`):
  destroy the Idle slot, then the roof anim or the stock damaged/base
  ProductionAnim.
- Vehicle factory unload (`0x44DDF0`/`0x44DDDE`, `:1188-1205`): the docked
  unit's `IsRoofExitTechno` selects the roof path, otherwise the stock play
  stays.
- Door draw: `DrawDoorAnim` (`0x43D0CF`) selects the body pair by
  `ExitThroughRoof.Get(JumpJet)`, while `DrawUnderDoorAnim`/MarkRoof
  (`0x43D363`) selects the under-door pair by `IsRoofExitTechno`
  (`JumpJet || BalloonHover`). The two predicates differ when only
  `BalloonHover=yes` is set.

`AllowBuildingProductionAnim` (`0x43CC73`, `0x44B7AE`) admits the production
anim for a ConstructionYard or a `Factory=BuildingType` with a valid
ProductionAnim name. VERA's play sites already require a resolved slot name, so
the gate is behaviorally transparent here; the only non-ConYard BuildingType
factory arm is dormant on retail data.

Retail leaves every roof key unset, so the stock unload art is unchanged.

## Where VERA20K owns it

- `rules/phobos_roof_production_anim.rs` - the eleven ART reads on `ArtEntry`
  and the pick order.
- `rules/object_type.rs` - `exits_through_roof` / `draws_through_roof` over
  the existing `ExitThroughRoof` rules owner.
- `sim/building_art.rs` - `play_roof_production_anim`, the per-call slot-8
  config override.
- `sim/building_art_power.rs` - `powered_anim_blocked`
  (`Ext/BuildingType/Body.cpp:101-115`).
- `sim/production/production_queue.rs` - the infantry factory exit arm.
- `sim/world/techno_ai/building_missions.rs` - the vehicle factory unload arm.
- `app/presentation/instances/shp.rs` - the two door-draw predicates.

## Tests
`phobos_roof_production_anim_tests` reads all eleven keys, the absent/`<none>`
controls and the pick order; `object_type_exposes_the_two_roof_exit_predicates`
pins both predicates; `roof_production_anim_plays_the_override_and_falls_back`
plays the roof name, the stock control and the unresolved-name fallback. The
existing GAWEAP ground/NightHawk door test keeps its expected pair.

## Residuals

- `IsPoweredAnimBlocked`'s Ares `HasPowerFromMapFile` exception is not modeled;
  VERA uses the building's online latch.
- The 0x44B7AE Mission_Repair production-anim play itself is not modeled in
  VERA (stock baseline gap); the roof gate there is therefore not reachable.
- `AllowBuildingProductionAnim`'s non-ConYard `Factory=BuildingType` arm is
  dormant on retail data.
