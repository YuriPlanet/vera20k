# UnlimboDetonate

A warhead can bring back a unit that fired a `LimboLaunch=` weapon, at the
point where that bullet detonates.

```ini
[SOMEWARHEAD]                          ; WarheadType
UnlimboDetonate=false                  ; boolean
UnlimboDetonate.ForceLocation=false    ; boolean
UnlimboDetonate.KeepTarget=true        ; boolean
UnlimboDetonate.KeepSelected=true      ; boolean
```

Reader `WarheadTypeExt::LoadFromINI` (`Ext/WarheadType/Body.cpp:420-423`);
the constructor defaults are no / no / yes / yes
(`Ext/WarheadType/Body.h:550-553`). Phobos documents that the warhead cannot
be combined with `Parasite=`; every arm below tests that first.

## Behavior

- **Fire admission** (`TechnoClass_FireAt_UpdateWeaponType`, hook `0x6FDD7D`,
  `Ext/Techno/Hooks.Firing.cpp:588-607`): a `LimboLaunch=` weapon whose
  warhead has `UnlimboDetonate` and is not `Parasite=` cannot fire while the
  firing Foot's locomotor `Is_Really_Moving_Now`; a Building never reaches
  the Foot cast.
- **Fire tail** (`TechnoClass_Fire_SetContext`, hook `0x6FE562`, and
  `TechnoClass_Fire_UnlimboDetonate`, hook `0x6FF7FF`,
  `Ext/WarheadType/Hooks.cpp:609-657`): the context records the firer's
  `IsSelected` and `InLimbo` at `TechnoClass::Fire` entry. After the
  LimboLaunch Limbo (`0x006FF7F3`), when the firer is alive, has health, the
  bullet exists, it was not already in limbo and the warhead qualifies, the
  hook writes the bullet's `Owner` (`+0xB0`) back to the firer, which Limbo's
  `Detach_All` cleared; with `KeepSelected` it also remembers the pre-fire
  selection.
- **Detonation** (`BulletClass_Logics_Extras`, hook `0x469AA4`,
  `Ext/Bullet/Hooks.DetonateLogics.cpp:502-592`): after the weapon's own
  warhead, when the bullet's owner is in limbo and the warhead qualifies:
  - without `ForceLocation`, `MapClass::NearByLocation` searches the impact
    cell for a cell passable to the type's `SpeedType`/`MovementZone`;
    `OnBridge` comes from that cell and the location is its
    `GetCoordsWithBridge`; an airborne target keeps the impact Z;
  - with `ForceLocation`, the location is the impact coordinate itself, its
    `OnBridge` comes from `TryGetCellAt`, and the `Unlimbo` call is bracketed
    by `++/-- Unsorted::ScenarioInit`;
  - a successful `Unlimbo`: an airborne target also sets `IsFallingDown` and
    `OnParachuted`; `KeepTarget` (default yes) sets the bullet's Object target
    back, otherwise clears it; a remembered selection is consumed and the
    firer is `Select`ed;
  - a refused `Unlimbo`: the remembered selection is consumed without a
    `Select`, the firer is `SetLocation`d to the requested location and killed
    with `[CombatDamage] C4Warhead` (ignore-defenses, no attacker).

`MapClass::UnselectAll_ClearLimboLaunchers` (hook `0x48DC90`,
`Ext/WarheadType/Hooks.cpp:659-674`) clears the remembered selections on
`Unselect_All`.

## Where VERA20K owns it

- `rules::warhead_type::WarheadType` holds the four keys, read in the native
  order with the constructor defaults.
- `sim/combat/phobos_unlimbo_detonate.rs`:
  - `fire_context` / `fire_hook` are called by
    `Simulation::parasite_limbo_launch`, the existing `TechnoClass::Fire`
    LimboLaunch owner (`0x006FF749..0x006FF872`), before and after its Limbo;
  - `refuses_moving_limbo_launch` runs in `emit_admitted_fire` before the
    stock FireAt InLimbo refusal;
  - `emit` runs from `commit_projectile_detonations_inline` at the Extras-hook
    site after `phobos_extra_warheads` and before the airburst tail, and reuses
    the existing owners: `reveal_constructed_object_at_coord_with_overlay_context`
    (Unlimbo/placement), `assign_target_represented` (SetTarget),
    `techno_select` (Select) and `commit_entities` (the refused arm's C4
    damage).
- `GameEntity::unlimbo_detonate` is the remembered selection, client-local
  like `limbo_reselect`; `apply_selection_snapshot` clears it for an empty
  selection snapshot, VERA's representation of `Unselect_All`.

## Draws and timers

No RNG draws and no timer writes of its own. The reappear goes through the
existing Unlimbo/placement owner (which draws none); the refused arm commits
one `EntityDamageEvent` through `commit_entities`. No detach calls are added:
the firer's Limbo detaches the bullet owner as stock, and the fire hook writes
it back.

## Residuals

- `TechnoExt::OnParachuted` **is ported** (`Ext/Techno/Body.h:80`):
  `ObjectFallState::on_parachuted` (`sim/object_fall.rs`) carries the latch;
  the airborne reappear sets it next to `raise_falling_down`
  (`Ext/Bullet/Hooks.DetonateLogics.cpp:607-610`), and the paradrop spawn sets
  it right after `SpawnParachuted` returns true
  (`Ext/Techno/Hooks.cpp:1907`, `sim/aircraft/drop_payload.rs`). The
  falling-down landing arm reads and clears it on every Techno, before the
  `IsABomb` gate (`Ext/Techno/Hooks.cpp:1030-1031`), and a set flag leaves the
  damage packet at 0 so neither the EMP arm nor the `FallingDownDamage`
  ratio runs (`:1067-1084`). Tests:
  `sim::movement::parachute_descent::falling_damage_tests::
  a_parachuted_landing_skips_the_falling_down_ratio` and
  `sim::superweapon::paradrop_tests::
  a_paradropped_passenger_is_marked_on_parachuted`.
  The other two Phobos set points — `Ext/Foot/Body.cpp:784` and
  `Ext/TechnoType/Body.cpp:705` — live in the in-air branches Phobos rewrites
  there, which VERA does not own; a fall raised through them stays unflagged.
- `MapClass::Unselect_All` clearing: VERA clears remembered selections only
  when `apply_selection_snapshot` receives an empty set, while native clears
  on every `Unselect_All`, including the clear before a new non-empty
  selection. Trigger: the player replaces the selection while a limbo launcher
  is in flight; effect: VERA can still re-select the firer where native would
  not. Downstream risk is selection-only.
- `LimboLaunchers` is a vector of TechnoExts in native; VERA keeps one
  remembered bit per entity. A `KeepSelected` shot by an unselected firer
  leaves a stale false entry in the native vector and none in VERA; both
  observe no re-selection.
- Extra warheads: Phobos tests the detonating bullet's own warhead, while a
  VERA `ExtraWarhead` packet runs the arm again with the extra warhead.
  Trigger: `ExtraWarheads=` lists an `UnlimboDetonate` warhead while the
  main one lacks it; effect: VERA brings the firer back where native would
  not.
- Synthetic detonations: the arm does not require a represented Bullet, so a
  direct warhead packet whose `source_id` names a limboed `LimboLaunch` firer
  also runs it. Trigger: a non-bullet warhead effect reusing that id; effect:
  VERA unlimbos the firer, native's `BulletClass_Logics` hook never runs.
