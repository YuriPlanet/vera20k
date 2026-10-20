# Pending first production and retained sidebar clock

`factory_loss.py --pending-production` extends the existing hold/resume owner.
It reuses `ReadyBuildingFixture`, its physical Russian type/country readers,
native allocator, command ring, local event receiver, sidebar and RNG observers.
The historical default, sale, hold/resume, system-hold and naval outputs remain
separate. The new `pending_production.json` is intended for the production
sidebar/request tests; the shared Factory remains the gameplay authority.

```sh
VERA20K_GAMEMD_EXE=/path/to/original/gamemd.exe \
VERA20K_FACTORY_SIDEBAR_ASSETS=/path/to/physical-sale-sidebar-cache \
python -B -m tools.sidebar_oracle.factory_loss --pending-production --check
```

Use `--write` deliberately to regenerate this corpus and its sidecar. The
sidecar records the replay command and physical input hashes, without
executable or repository-source hashes.
The physical cache is the existing [sale-sidebar](factory_sale_sidebar.md)
cache. It is an explicit file-service boundary, not a complete scenario,
Rules::Process or VFS winner proof. Existing setup executes the selected native
skirmish-mode store `52E10F..52E119`; whole menu startup is excluded.

The histories execute whole original Select `6AAD00`, StripAI `6A8B30`, local
receive (entry `6474C7`, stop `6474BF`), HouseBegin `4FA350`, HouseSuspend `4FA910`,
FactorySuspend1 `4C9E60` and same-head Resume0 `4C9EA0`:

- `idle_hold_resume`: first idle E2 Left creates request1 and PRODUCE14;
  prediction advances; Right creates request2 and SUSPEND15 before dispatch.
  Repeated Right adds no event. Dispatch constructs the real Factory/E2 then
  suspends that head. Later Left resumes it through ordinary PRODUCE; early
  actual FactoryAI pays on the same head. Full payment and physical exit remain
  production-integration responsibilities.
- `pending_shift_right`: Shift preserves the same pending SUSPEND behavior;
  state2 survives linkage and direct Sidebar NoInit.
- `busy_queued_only`: an actual running E2 keeps FLAKT unlinked/request0
  through busy Left and append. FLAKT Right emits ABANDON16 without EVA or
  pausing the current head. This is separate from pending Right.
- `repeat_left_and_unlinked_no_init`: repeated Left before receive preserves
  both PRODUCE events; pending Right adds SUSPEND. Unlinked state2 survives
  direct NoInit while its progress resets.

`numeric_controls` prepare native Cost, Rules BuildSpeed, MaxAhead and wallet
values explicitly, then execute actual Select and its original numerical
callees. Standalone base-duration calls execute `711EE0`, not a Python formula.
The controls distinguish signed low-word multiplication, division toward zero,
affordability equality/failure, minimum/maximum rate, the comparison against
`MaxAhead+15` rather than the full duration, resumed rate+1 including256,
stage smoothing, and the original predicted-clock stop at53. The final stop
control reaches52 with original Select/52 StripAI visits before its measured
boundary; its progress is never assigned. Resumed smoothing controls alone
supply request2/predicted stage as explicit presentation component inputs on
the existing actual paid E2+E2 fixture. No arithmetic result is supplied.

MaxAhead values, including0, are test inputs. These controls do not establish
the offline/network MaxAhead initialization or evolution. Native first-click
prediction uses type base duration before country/type/power/multiple-factory
factors. Resume prediction instead queries the actual held object's
Time_To_Build. Original admission, wallet, time and clock code execute without
return substitutions.

The initial session default15 has separate original instruction/caller evidence:
CRT initializer slot8132F0 calls4E8030, which supplies the Session receiver to
6970A0; constructor6975C0 stores15 at Session+318. Rust's existing configuration
default supplies this value to new Simulation and app-install consumers.
Explicit host overrides and mutable session updates remain authoritative.
The complete fresh-skirmish callback history, prior-network sessions and
the value at every first Select are not certified by that constructor proof.

The shared `sale_sidebar_state(request_projection=True)` reads actual record
request state at `+0x10` and predicted stage/rate/timer. Its historical default
schema preserves the old field named `request_state` that observed `+0x18`
dirty storage. Old values are not reinterpreted as request-state evidence and
old corpora are not rewritten. `ReadyBuildingFixture.click` only adds an
optional Shift argument; its default ABI remains unchanged.

The JSON schema is1. `histories[].phases[]` and each numeric `phase` retain
before/after shared sidebar/Factory views, actual event fields, native calls,
numeric register observations, meaningful prediction/Factory timer stores and
identity/lifecycle counts. `stopped_phase` is the extra post-limit visit.
`build_speed_bits` strings are little-endian binary64 bytes. `rng_states` maps
SHA256 to complete native RNG-object bytes; phase before/after maps reference
all three streams. Native request/advance observations remain attached to the
phase. Deduplication changes storage, not the compared bytes.

NoInit `6A4F20` and StripClear `6A81B0` execute at declared direct lifecycle
boundaries. NoInit retains request1/2 and links while resetting progress;
StripClear removes records without deleting the actual Factory. Whole native
save streams, historical Factory-link swizzling, new-scenario scheduling and
native SAV compatibility remain outside these controls. Timer padding is not
interpreted. Existing physical-cache/CRT/OS/COM/DirectSound transports and the
observed EVA RET4 sink are inherited. Draw commands/raster, glyphs, device RNG,
whole audio behavior, saturated local rings, failed Begin retry histories,
other class kinds and special request3 are excluded. Rust tests and production
captures must state their own bounds separately from this native comparison.

HouseSave504080 and Load503040 separately transfer the full160B8-byte House
block through410320/410380. Concrete vtable7EA8A0 supplies Size504730, and
post-load NoInit4F5190 preserves the category gate bytes53D0..53D8. Rust
retains its modeled gates in the same presentation transport as requests;
they cannot be reconstructed from per-cameo requests. This is instruction and
field-lifetime evidence, separate from the executed Sidebar NoInit controls.
Human save admission before a pending PRODUCE dispatch remains unproved.
