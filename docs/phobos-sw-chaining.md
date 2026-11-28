# Superweapon chaining and link groups

A superweapon can launch a chain of other superweapons at the same target, and
it can grant, ready or reset a group of linked superweapons when it fires.
Phobos reads the keys in `SWTypeExt::LoadFromINIFile`
(`src/Ext/SWType/Body.cpp:170-265`) and applies them from
`SWTypeExt::FireSuperWeaponExt` (`Ext/SWType/FireSuperWeapon.cpp:13-40`), the
hook on the common `SuperClass::Launch` tail (`0x6CDE40`).

```ini
[SW]                                ; SuperWeaponType
SW.Designators=                     ; techno list (shared with Ares)
SW.Inhibitors=                      ; techno list (shared with Ares)
SW.AuxBuildings=                    ; building list (shared with Ares)
SW.NegBuildings=                    ; building list (shared with Ares)
SW.AuxTechnos=                      ; techno list
SW.NegTechnos=                      ; techno list
SW.Next=                            ; SuperWeaponType list
SW.Next.RandomWeights=              ; int list alias for slot 0
SW.Next.RandomWeights0..N=          ; int lists, until the first empty one
SW.Next.RollChances=                ; float list, one die per entry
SW.Next.RealLaunch=true             ; boolean
SW.Next.IgnoreInhibitors=false      ; boolean
SW.Next.IgnoreDesignators=true      ; boolean
SW.Link=                            ; SuperWeaponType list
SW.Link.Grant=false                 ; boolean
SW.Link.Ready=false                 ; boolean
SW.Link.Reset=false                 ; boolean
SW.Link.RandomWeights=              ; int list alias for slot 0
SW.Link.RandomWeights0..N=          ; int lists
SW.Link.RollChances=                ; float list
```

`SW.Next` and `SW.Link` are `ValueableIdxVector<SuperWeaponTypeClass>`: each
comma token goes through `SuperWeaponTypeClass::FindIndex` and an unknown name
is dropped at read time. The numbered weight lists stop at the first empty key;
the unnumbered `RandomWeights` is a valid alias that replaces or creates slot
0 (`Body.cpp:198-222`, `:255-265`). The four list keys `SW.Designators`,
`SW.Inhibitors`, `SW.AuxBuildings` and `SW.NegBuildings` are Ares keys; the
Ares owner reads them and Phobos reuses that one reader. `SW.AuxTechnos`,
`SW.NegTechnos` and `SW.TechLevel` are the Phobos availability owner
(`docs/phobos-superweapon-technos.md`).

## Behavior

After a Super's own launch effect, `FireSuperWeaponExt` runs `SW.Next`, then
the type conversion, then `SW.Link`, in that order:

- **`SW.Next`** launches each selected next Super at the same cell.
  Randomness starts at the weight lists, not at the chances: with no weights,
  one sure roll launches every listed type; with weights, each
  `SW.Next.RollChances` entry (default: one sure roll) draws one Scenario
  `RandomDouble`, skipped when `dice > chance`, then `ChooseOneWeighted` picks
  one index from that roll's list (the last list repeats). A picked weight past
  the list is ignored.
- **Real launch** (`SW.Next.RealLaunch=yes`, the default) requires the next
  type's Super to be present, charged, unsuspended and affordable
  (`HouseClass::CanTransactMoney` over the next type's `Money.Amount`), then
  runs `SetReadiness(true)`, `Launch`, `Reset`, consuming the charge.
  `RealLaunch=no` skips that gate, launches forcibly, and writes the saved
  recharge timer back after the Reset (the charge is not consumed).
- Each next type's `SW.Inhibitors` (unless `SW.Next.IgnoreInhibitors=yes`) and
  `SW.Designators` (unless `SW.Next.IgnoreDesignators=yes`, the default) are
  checked around the launch cell with the same eligibility and range tests as
  Ares' targeting preflight.
- **`SW.Link`** acts once per selected linked Super (same weight/roll
  selection). `SW.Link.Grant=yes` gives a missing Super a one-time grant
  (`Grant(true, false, false)`, instantly charged). A present Super is
  `Reset` with `SW.Link.Reset=yes`; otherwise `SW.Link.Ready=yes` (or a grant
  whose type has Ares' `SW.InitialReady`) writes `RechargeTimer.TimeLeft = 0`
  and `SetReadiness(true)`; a grant that none of those cover is reset to a
  normal recharge. A defeated or observer house is skipped.

`ChooseOneWeighted` sums the integer weights in binary32 and returns the first
prefix whose `sum2 / sum` exceeds the same dice draw; a zero-sum list answers
-1 (no index). The two mechanisms share this one helper.

## Where VERA20K owns it

- `rules::phobos_sw_chaining::PhobosSuperWeaponChaining` reads the keys in the
  ordered Process superweapon body and is projected into
  `RuleSet::phobos_super_weapon_chaining`, hashed under
  `phobos-super-chaining-v1`.
- `sim/superweapon/chaining.rs` is the whole tail: `apply_phobos_launch_tail`
  runs `SW.Next`, `fire::apply_sw_type_conversion`, then `SW.Link`. Both
  dispatch tails call it: the end of `fire::launch` for stock kinds and
  `super::finish_ares_launch` for Ares handlers.
- A chained launch reuses the one launch owner: `fire::launch_chained_super`
  for `RealLaunch=yes` and the existing
  `fire::launch_super_preserving_charge` for `RealLaunch=no`.
- `sim/superweapon/targeting.rs::scan_influences` is the shared
  designator/inhibitor scan; Ares' `TargetingData` and the chain gate both use
  it, and `has_inhibitor`/`has_designator` are that owner's predicates.

The tail runs for every Phobos build. With every key unset the lists are
empty, so nothing is chained or linked and the previous single launch is
unchanged. In a build without the Ares capability the four Ares list keys are
unread, so a chained Super's inhibitor gate is open and no designator is
required — the same as a build without Ares.

## Draws and timers

- One Scenario `RandomDouble` per selected roll of `SW.Next` and `SW.Link`
  (no draw when there is no weight list).
- Each chained launch runs its own effect's draws; a `RealLaunch=no` launch
  also writes the saved `StartTime`/`TimeLeft` back.
- `SW.Link.Ready` writes `TimeLeft = 0`; `SW.Link.Reset` and a bare grant run
  `SuperClass::Reset`.
- No detach calls of its own.

## Residuals

- A cyclic `SW.Next`/`SW.Link` chain recurses without bound natively; VERA
  stops after 16 nested levels and logs. Trigger: a chain that reaches itself.
- `SW.Link.Grant` also inserts the linked Super's cameo into the current
  player's sidebar and repaints it natively; VERA's sidebar has no owner for
  that insertion here and leaves the grant to the ordinary availability pass.
  `Message.LinkedSWAcquired`/`EVA.LinkedSWAcquired` are presentation and not
  ported.
- `SW.Next.RollChances` without a weight list is ignored in both: native only
  enters random mode when `SW.Next_RandomWeightsData` is non-empty.
- An Ares-handler launch that refuses its own activation returns before the
  shared tail in VERA, so its Convert/Next/Link arms do not run; native's
  refused handler falls through to the stock case and still reaches
  `0x6CDE40`.
- The tail's `ShotCount++` (feeding `SW.Shots`) is not modelled beyond the
  Ares-handler launches VERA already counts.
- The chained `Launch` receives `pHouse->IsCurrentPlayer()` natively; the
  shared launch owner takes no such flag (no stock arm reads one).
