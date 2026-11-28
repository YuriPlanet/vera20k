# MakesWake

A moving hull on water leaves the `[General] Wake=` animation. Phobos makes
that wake switchable per locomotor and per type.

```ini
[AudioVisual]
WalkLocomotorMakesWake=no    ; Valueable<bool>, default no
DriveLocomotorMakesWake=yes  ; default yes
HoverLocomotionClassMakesWake=yes ; default yes
ShipLocomotionClassMakesWake=yes  ; default yes

[BOAT]                       ; TechnoType section
MakesWake=no                 ; Nullable<bool>, unset -> the locomotor global
```

Readers: the four globals at `Ext/Rules/Body.cpp:543-546` (constructors
`Ext/Rules/Body.h:960-963`); the type key at `Ext/TechnoType/Body.cpp:1070`
(`Nullable<bool>`, `Body.h:301,733`).

## Native body

`TechnoExt::TypeExtData->MakesWake.Get(<locomotor>MakesWake)` gates each
locomotor's movement-wake producer:

- `0x4B079D` Drive (`Ext/Techno/Hooks.Misc.cpp:360-366`) — `NoWake` at
  `0x4B0828`;
- `0x514A32` Hover (`:368-374`) — `NoWake` at `0x514AC8`;
- `0x69FE4A` Ship (`:376-384`) — `NoWake` at `0x69FEF0`;
- `0x75AC93` Walk (`:425-441`), which otherwise requires the walker to be
  really moving, not on a bridge, on a Water cell, and on a 10-frame cadence.

## VERA

`PhobosGlobals` reads the four globals; `ObjectType::phobos_makes_wake` reads
the type key. `spawn_wakes_for_frame` resolves each movant through
`resolves_makes_wake` (type key, else the global for its active locomotor
kind) and `wake_anchor_for` returns no anchor when it is false. The batched
wake pass still spawns the `[General] Wake=` animation through the shared
constructor.

## Tests

- `src/sim/world/world_tests.rs` — `makes_wake_gates_each_locomotor_wake`:
  water-going Drive and Ship hulls wake by default, `MakesWake=no` and a
  global `no` both skip it, and an authored `yes` re-enables a
  global-disabled hull (including Ship's own global). The gate was mutated red
  (`if false &&`) and restored. `makes_wake_resolves_each_locomotor_global_and_type_override`
  covers all four kinds plus a kind with no native hook in the resolver.

## Residuals

- VERA spawns no infantry (`Walk`) movement wake at all: the resolver reads
  `WalkLocomotorMakesWake=` (and a type `MakesWake=yes` resolves true), but
  the batched pass has no Walk arm, so no walk wake is ever created in either
  build. Phobos' default (`no`) matches that; a stock walk wake or an authored
  `WalkLocomotorMakesWake=yes` stays dormant — a pre-existing residual.
- A `Nullable<bool>` `none`/`<none>` token is treated as unset (falls back to
  the global) by the shared reader, while Phobos parses it as `false` with a
  set value. This is a pre-existing helper difference for every such key.
- The `Wake_Grapple`/`Wake_Sinking` per-type overrides are separate keys with
  their own consumer sites; neither is part of this key.
