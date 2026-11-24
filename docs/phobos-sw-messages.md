# Superweapon activation messages and the sold-unit EVA

Phobos lets a superweapon announce its launch with a localized message and an
EVA line, picked by the launching House's relation to the player who hears it,
and lets a techno type replace the stock sale line.

## Keys

```ini
[SOME_SW]                       ; SuperWeaponType
Message.Activated.Owner=        ; CSFText, default empty
Message.Activated.Allies=
Message.Activated.Enemies=
EVA.Activated.Owner=            ; VoxClass, default -1 (silent)
EVA.Activated.Allies=
EVA.Activated.Enemies=
Message.LinkedSWAcquired=       ; CSFText, default empty
EVA.LinkedSWAcquired=           ; Nullable VoxClass, unset (silent)

[SOMETECHNO]                    ; TechnoType
EVA.Sold=                       ; Nullable VoxClass, unset (stock line)
```

Reader: `SWTypeExt::LoadFromINIFile` (`src/Ext/SWType/Body.cpp:241-250`),
declarations and constructor defaults `src/Ext/SWType/Body.h:119-126`,
`:208-215`; `TechnoTypeExt::LoadFromINIFile` (`src/Ext/TechnoType/Body.cpp:844`),
declaration `src/Ext/TechnoType/Body.h:110`.

- A present `Message.*` key replaces the label; an empty value clears it and
  prints nothing. The label is a `CSFText`, i.e. a `FixedString<0x20>`, so it
  keeps at most 31 bytes.
- A present `EVA.Activated.*` key resolves through `VoxClass::FindIndex`. A
  blank value stores `-1` (silence); an unknown name is rejected and the field
  keeps its constructor `-1`. VERA's text parser drops `Key=` (gamemd's parser
  shape), so a file-authored blank reads as absent; the `-1` arm serves
  retained projections that keep empty values.
- `EVA.LinkedSWAcquired` is nullable: only a set value plays. A blank value is
  still "set" with `-1`, which is silence, so both collapse to "no cue".
- `EVA.Sold` is nullable in the same way: absent keeps the stock
  `EVA_UnitSold`/`EVA_StructureSold` and a name replaces the line. A
  file-authored blank is dropped by the text parser and so also keeps the
  stock line.

## Activation announcement

Native runs these last in `SWTypeExt::FireSuperWeaponExt`
(`src/Ext/SWType/FireSuperWeapon.cpp:40-42`), after every launch effect, the
`SW.Next` chain and `SW.Link`:

- `ApplyActivatedMessage` (`:491-510`) picks Owner / Allies / Enemies with
  `pHouse->IsControlledByCurrentPlayer()`, else
  `pHouse->IsAlliedWith(HouseClass::CurrentPlayer)`, prints the label through
  `MessageListClass::PrintMessage` with `RulesClass::MessageDelay`, the
  **launching** House's `ColorSchemeIndex` and `silent = true`, and returns
  early on an empty label.
- `ApplyActivatedEva` (`:512-526`) picks the same column and calls
  `VoxClass::PlayIndex`, returning early on `-1`.

Neither has an "is the owner local" gate: every client hears the column its
own current player selects.

VERA's simulation cannot see the client's current player, so it emits
`SimSoundEvent::SuperWeaponActivatedMessage { owner, sw_type }` from
`sim::superweapon::chaining` and the app picks the column in
`app::match_runtime::sound_dispatch`:

- `Simulation::house_is_human_player(owner)` is the sim-owned half of
  `IsControlledByCurrentPlayer` (it also admits a campaign `PlayerControl=`
  House).
- The ally test uses the local client's name against the House alliance map,
  the same directional `IsAlliedWith` bitfield the rest of the app reads.
- The chosen label becomes a `HudMessage::PhobosSuperWeaponMessage` row drawn
  in the launching House's colour; the chosen cue becomes a
  `GameSoundEvent::Eva`.

## Linked superweapon announcement

`SWTypeExt::ApplyLinkedSW` (`:409-489`) tracks `isActive` across every linked
type it granted, readied or reset. Only then, and only on the granting House's
own client (`pHouse->IsCurrentPlayer()`, stricter than
`IsControlledByCurrentPlayer`) and only for a non-observer House, does it play
`EVA.LinkedSWAcquired` (when set) and print `Message.LinkedSWAcquired` in
`HouseClass::CurrentPlayer`'s colour (`:482-488`).

VERA emits `SimSoundEvent::SuperWeaponLinkedAcquired { owner, sw_type }` from
`sim::superweapon::chaining::apply_linked_sw`, gated on `isActive` and the
sim-owned observer flag; the app applies the `IsCurrentPlayer` test and
prints in the local player's colour.

## Sold-unit EVA

Phobos replaces the stock sale lines with the sold type's `EVA.Sold`:

- `FootClass_Sell` hook `0x4D9F7B` (`src/Ext/Building/Hooks.Selling.cpp:18-21`)
  for a unit or aircraft, under `IsControlledByCurrentPlayer`.
- `BuildingClass_Mi_Selling_EVASold_UndeploysInto` hook `0x449CC1`
  (`:73-79`) for a building that is not undeploying, under
  `IsOwnedByCurrentPlayer`, replacing the stock `EVA_StructureSold` that
  `0x44AB22` would have played (Phobos always jumps past it, `:113-123`).

VERA keeps the existing owners: `production::foot_on_sold` emits
`SimSoundEvent::HouseEvaNamed` when the type names a cue and `HouseEva` with
`EVA_UnitSold` when the key is absent or blank (the text parser drops the
empty value). The building path already carries its type in
`SimSoundEvent::StructureSold`, so the app picks the type's cue or
`EVA_StructureSold` there.

## Residuals

- **Listener predicate.** The activation column asks
  `Simulation::house_is_human_player`, i.e. the session's persisted current
  house, while native reads the listening client's `PlayerPtr`.
  `Simulation::is_human_player_on` exists but is not exposed to the client.
  Trigger: a client whose bound `PlayerPtr` differs from the persisted
  current house. Impact: that client can pick the wrong
  Owner/Allies/Enemies column. Frequency: multiplayer/viewer binds. Risk:
  presentation-only.
- **Sibling family readers.** `SW.Next`/`SW.Link` and the `LimboDelivery`/
  `LimboKill` blocks still rebuild from the current pass's section
  (`from_section`), so a later partial section resets keys it omits to their
  constructor defaults. Pre-existing in those chains; this chain's message
  block now retains per pass.
- **Multiplayer current player.** Which House a client owns is client-local and
  the simulation is shared, so the column pick happens per client in the app.
  A client that does not receive the launch event (a network observer whose
  view was never synced) hears nothing; native evaluates
  `HouseClass::CurrentPlayer` on the same machine it prints on, so a networked
  build must keep `PlayerPtr` on the client that renders the message.
- `IsCurrentPlayerObserver()` (`notObserver`'s second half,
  `FireSuperWeapon.cpp:412`) is not modelled: an observer House is a sim fact
  (`ScenarioSession::is_observer_house`) and already stops the event, but the
  client-local half of that predicate has no VERA owner.
- `Message.Activated.*` has no `Message.FirerColor`/`Message.Color` style keys —
  natively it always uses the launching House's scheme, which is what VERA
  does.
- `TechnoExt::TypeExtData`'s `EVA.Sold` is resolved against the fixed EVA
  catalog in `RuleSet::bind_type_eva_references`; a name that catalog does not
  define leaves the field unset, so the stock line plays.
