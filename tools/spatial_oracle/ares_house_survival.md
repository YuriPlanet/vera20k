# Ares House survival controls

`house_defeat_gate.py --ares-survival` extends the existing gate Fixture. It
executes the Ares3.0 replacement at `4F8EBD -> 10024990 -> 4F8F87`, its shared
KeepAlive writers at `4FF71B -> 10024580` and `4FF563 -> 100245D0`, and their
original game continuations. The default21 retail controls and `--outcomes`
remain separate.

Set `VERA20K_GAMEMD_EXE` to the original game executable and
`VERA20K_ARES_DLL` to Ares3.0, then run:

```text
python -m tools.spatial_oracle.house_defeat_gate --ares-survival --check
```

`--write` deliberately regenerates `ares_house_survival.json` and its provenance
sidecar. The existing Ares prerequisite owner checks the DLL identity. Game
executable variants follow the shared oracle's identity policy.

The corpus contains these groups, intended for direct Rust-test consumption:

- `reader_histories[0]`: full TypeExt constructor defaults and12 ordered
  synthetic physical INI inputs. Each step carries `physical_ini`, `before`,
  `after`, section admission, native ReadString arguments and diagnostic tokens.
  Nullable pairs use `value` and `is_set` bytes. The existing physical Rules VM
  owns file loading and exact section/key/ReadString behavior; original Ares
  nullable and bool helpers execute. Missing/empty/invalid retain; `none` parses
  false, `<none>` is invalid, and physical trimming precedes first-character
  bool parsing. These are explicit file inputs, not a full Rules Process,
  archive search, type-reset or complete extension-reader execution.
- `tracking_histories`:8 cases with two gain/loss steps each. Inputs name the
  class, nullable override, exclusion flags,1x1 undeployer and optional signed
  counter prior. `steps[*].after` records the retained KeepAlive count, companion
  building KeepAlive count, original building/vehicle scalars and original
  per-type counts. Default buildings, unit-like buildings, excluded types,
  explicit overrides and signed wrapping are distinguished. Reader-produced
  nullable pairs supply the tracking controls.
- `capture`: the original ChangeOwner tracking slice `7015D8..7015EB` transfers
  a counted excluded unit from old to new House. The old owner pointer remains
  installed while loss and gain use their respective House receivers. This is
  not the complete ChangeOwner, capture or mind-control mechanism.
- `survival_cases`:19 complete replacement-gate cases entered through the
  original multiplayer prefix. `input` explicitly supplies `short_game`,
  `base_indices`, index-keyed `counts`, `capacity`, `keep_alive_count`, all three
  active totals, and class-grouped `objects`. Every object names `category`,
  `owner` (`ours`/`other`), `limbo` and allocated `parasite` presence. `events`
  records the native defeat sinks; an empty list means neither was entered.
  `base_slots_visited`, `classes_visited`, `objects_visited` and `hooks` expose
  actual traversal/continuations. Duplicate BaseUnit indices share one pointer.
  Valid nonnull type pointers and nonnegative ArrayIndex values are fixture
  priors; arbitrary corrupt pointers/negative indices are excluded.
- `stream_cases`:3 native nullable serializer/deserializer controls, composed
  with the exact HouseExt+18 save/load field slices. `stored_bytes`,
  `restored` and `consumed_bytes` preserve actual native output, including the
  wrapped counter produced by tracking. Preallocated native byte-vector streams
  and original copy leaves execute. Full SAV framing, swizzling and other
  extension fields are excluded.

The Ares gate first tests retained HouseExt+18 for **nonzero**. ShortGame then
scans the full Rules BaseUnit vector in order and accepts any tracked count
**nonzero**, without addition. The capacity check skips index>=capacity. This
differs from the default retail path's three-element wrapping sum and signed
positivity checks. KeepAlive=no affects the retained contribution; it does not
exclude that unit from BaseUnit or normal-game eligibility.

Normal game tests active Unit, Infantry, Aircraft totals in that order; a
nonzero total admits that class's ordered array scan. An object qualifies when
its current owner matches and it is outside limbo or owns a ParasiteClass
(`FootParasite::class`, not its victim's eater). KeepAlive count admission
precedes those scans. The native body does not read health or BuildRefinery.

The register bridge follows the existing NoManualFire context/continuation
pattern. It restores the original context and saved EAX before continuation;
both original executable text ranges remain unchanged. The existing gate VM
retains its historical stack top while Ares occupies the unused lower mapping.
Original Unit/Building object virtuals and CounterClass changes execute; the
synthetic Type's original shared ArrayIndex getter reads+DF8. Owned-type-set
registration `749020` returns zero at the existing tracking harness boundary;
that set is not consumed by survival.

Blowup_All and MPlayer_Defeated are recorded and returned at entry, in native
order. No death receiver, teardown, result scheduling or whole-game parity is
claimed. Controls reject entry to the shared Main/RandomRanged, action-timer and
detach functions; downstream death RNG/timers remain outside these sinks.
KeepAlive updates remain with the existing HouseTracking constructor/tracking,
ownership-transfer and destructor lifecycle, including limbo objects.
