# Phobos house keep-alive globals (`KeepAlive.*`)

Ares tracks per-house keep-alive counters so a house is not defeated while it
still owns qualifying technos. Phobos replaces Ares' update callback
(`AresHouseExt_UpdateKeepAlive`, `Misc/Hooks.Ares.cpp:190-243`) and adds five
`[General]` keys that supply the **category default** used when a type does
not author its own `KeepAlive=`:

| Key | Phobos read | Constructor (`Body.h:1046-1050`) |
|---|---|---|
| `KeepAlive.Infantry` | `Ext/Rules/Body.cpp:652` | `no` |
| `KeepAlive.Units` | `Body.cpp:653` | `no` |
| `KeepAlive.Aircraft` | `Body.cpp:654` | `no` |
| `KeepAlive.Buildings` | `Body.cpp:655` | `yes` (non-Combat buildings) |
| `KeepAlive.Defenses` | `Body.cpp:656` | `yes` (`BuildCat=Combat`) |

The type key itself (`KeepAlive=`, `Ext/TechnoType/Body.cpp:1234`,
`Nullable<bool>`) was already ported as `ObjectType::keep_alive()`. VERA's
owners are `src/rules/phobos_globals.rs` and the keep-alive fact in
`src/sim/house_tracking.rs::TrackingFacts::of`.

## Behaviour

- The hook gates the category switch on `!Insignificant && !DontScore`; those
  types contribute no default (`result=false`). An explicit type `KeepAlive=`
  still moves the Phobos counters.
- Infantry, Units and Aircraft take their own key. A Building takes
  `KeepAlive.Defenses` when `BuildCat=Combat`, otherwise
  `KeepAlive.Buildings`.
- A type's `KeepAlive=` wins over the category default, exactly as
  `TechnoTypeExt::KeepAlive.Get(keepAlive)` reads.
- `KeepAliveTechnos` counts every qualifying techno; `KeepAliveBuildings`
  additionally counts buildings. VERA's `TrackingFacts.keep_alive` feeds the
  same counters (`HouseTracking`) and the trigger/defeat consumers.

The Phobos defaults equal Ares' own default only for buildings (`yes`) and
everything else `no`; the split between `Buildings` and `Defenses` and the
unit/infantry/aircraft defaults only exist with these keys.

## Feature gating

The mechanism belongs to Ares' house survive/defeat logic, so the consumer
compiles under `all(feature = "ares", feature = "phobos")`. A `phobos`-only
build reads the globals but has no Ares keep-alive tracking to feed; an
`ares`-only build keeps the pre-Phobos default (structures only).

## Tests

- `phobos_keep_alive_globals_pick_the_category_default` in
  `src/sim/ares_house_survival_tests.rs` (`--features "phobos ares"`): all
  five categories, the Combat split, the type override, and the constructor
  defaults.
- `the_keep_alive_globals_default_and_read_from_general` in
  `src/rules/phobos_globals_tests.rs` (constructors and authored reads).
- The existing Ares keep-alive tracking, reader-history and survival tests
  run unchanged under the same feature set.

## Residuals

- VERA models the Ares callback's counters through `TrackingFacts`; the
  hook's boolean return (`result`, false for insignificant/dont-score types)
  has no separate VERA channel because the tracking owner predates this chain
  and still counts an explicit `KeepAlive=yes` on such a type, matching the
  Phobos counter increments. Trigger: custom rules authoring
  `Insignificant`/`DontScore` with `KeepAlive=yes`.
