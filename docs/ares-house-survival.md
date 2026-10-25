# Optional Ares House survival

Build with `--features ares` to enable this multiplayer defeat-rule extension.
Default builds retain the retail three-entry `BaseUnit` sum and original normal
game counter test. INIs cannot enable omitted extensions. No Ares DLL is loaded.

```ini
[General]
BaseUnit=MCVA,MCVB,MCVC,MCVD

[IMPORTANTUNIT]
KeepAlive=yes

[DECORATION]
KeepAlive=no
```

Register the example types normally. In an Ares build, each tracked object
contributes to its House's retained KeepAlive count. An unset key defaults to
true for a Building that is neither Insignificant nor DontScore, including
unit-like deployed buildings; other classes default to false. An explicit
boolean overrides that default even on excluded types. The shared 2048-byte
reader retains missing, empty and invalid inputs across reached Rules passes.
`none` is explicit false; `<none>` does not clear a previous value.

Construction/destruction and ownership transfer update the count through the
existing HouseTracking owner. Limbo alone does not remove the contribution.
The signed counter wraps and survives Rust save/load without being rebuilt
from the surviving objects. Constructor-bound type facts retain the matching
removal input; production types are immutable during this selected match path.
Live type mutation or INI rereading while objects remain alive is outside this
coverage. Snapshot schema473 rejects older positional records; the existing
capability header rejects cross-mode loads before Simulation decoding.

The multiplayer gate keeps its retail campaign, frame, defeated and passive
House exclusions. Then either game mode remains alive when KeepAlive is
nonzero. Otherwise:

- Short game scans the entire ordered `BaseUnit` list; any nonzero tracked
  UnitType count suffices. Counts are not added. A limbo MCV still counts.
- Normal game checks the active Unit, Infantry and Aircraft totals separately.
  A class with a nonzero total must contain an owned object outside limbo, or
  one with an allocated ParasiteClass. Health and BuildRefinery are not gates.

`KeepAlive=no` changes that object's counter contribution; it does not exclude
an MCV from `BaseUnit` or a mobile object from the normal-game scan. An unmet
gate uses the existing Blowup_All then MPlayer_Defeated chain.

[Native controls](../tools/spatial_oracle/ares_house_survival.md) document the
executed Ares3.0 reader, tracking, survival and retained-field stream boundaries.
The Rust checks consume those outputs and separately exercise ordinary
construction, capture, limbo, deletion, rules restoration and snapshot hashing.
These are bounded component and integration checks, not complete Ares/MO,
native SAV-format or downstream death/scheduler parity.
