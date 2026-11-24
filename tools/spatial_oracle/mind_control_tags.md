# MindControl object Tag callbacks

```sh
python -m tools.spatial_oracle.mind_control_tags --write
python -m tools.spatial_oracle.mind_control_tags --check
```

Select the original executable through `VERA20K_GAMEMD_EXE` or `RA2_DIR` as
described in [native setup](../native_oracle.md). No extracted retail assets
are needed for these synthetic component controls.

Each row enters original `BulletClass::DetonateAtCoord` at `4690B0`, takes its
MindControl branch, and executes the original live Tag/Trigger/TEvent/action
chain. The ordinary rows stop **before** `CaptureUnit471D40`; this is an
incoming call boundary, not successful capture. Null-target and same-owner
controls execute genuine capture refusal, then stop **before** `469AA4`, not
at Detonate's return. Other controls cover missing firer/manager/Tag, dead
target, pre-callback human-target state, event44's source House-index mismatch,
and original action54/action12 disabling/removing their own Trigger.

The existing `tag_lifecycle` owner constructs actual Tags, Triggers and a base
Object, and attaches its Tag through original `ReplaceTag`. Bullet/Techno/type
and House state is supplied. Original Unit vtables are used without changing
their entries. CaptureManager's constructor runs up to its final global
registration, and original `466270` initializes the callbacks' empty Cell.
No game function result is substituted. Inherited Windows pointer probing and
bounded allocation are the only platform services. The host's final dispatch
request is adapted into the Detonate entry; all nested native calls run normally.

The compact receipt retains actual event/action order, callback returns,
victim/source/cell/force arguments, live bullet pointer reloads, pending/enabled
Tag state, Scenario globals and the capture boundary. Pointer labels identify
observed objects; they are not an alternative owner lookup. Full Scenario RNG
state is compared in memory and retained as equality plus draw count. Executable
sections and relevant original vtables are verified unchanged.

The sparse fixture does not establish firing/flight, successful capture,
sound, Unit/House/Bullet construction, or a physical campaign graph. In
particular, single-use Tag disposal broadcasts `PointerExpired` into
constructor-owned vectors and world listeners absent here. Complete
owner-changing callbacks and destroyed-source behavior are also excluded;
action14 ChangeHouse uses `TechnoTargetSweep`, not Bullet `PointerExpired`.
Those paths are excluded rather than replacing their callbacks.

The Rust consumer compares the event6/event44 prefix and this graph's action
and live Trigger state. Rust continues through its existing capture owner;
its later ownership events48/29 are outside the native prefix. Those incoming
kinds cannot activate this graph's non-latching event6/event44 predicates.
Only the two actual native refusal rows compare the capture outcome.

Run the associated Rust checks with:

```sh
cargo test --locked -p vera20k-core --lib mind_control_
```

For the stock campaign regression, set `RA2_DIR` to the retail installation
and `VERA20K_REQUIRE_RETAIL_ASSETS=1`. It loads ALL02 through the production
archive resolver and preserves the map's own Tag/Trigger/actions. The expansion
archive can override the original map's TeamType and Voice cue, so the test
resolves those references from the selected physical action record. The flat
arena and already-acquired stock YURI are supplied; the base-building opening
and a complete original campaign are not replayed.
