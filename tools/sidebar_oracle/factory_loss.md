# Factory prerequisite-loss timing

`factory_loss.py` extends the existing `ReadyBuildingFixture` and its physical
readers/VM. The consuming stock Battle regression is
`sim::production::lifecycle_tests::retail_prerequisite_loss_refunds_after_the_factory_charge_sweep`.

```sh
VERA20K_GAMEMD_EXE=/absolute/path/gamemd.exe \
VERA20K_READY_BUILDING_ASSETS=/absolute/path/ready-building-cache \
VERA20K_FACTORY_LOSS_ASSETS=/absolute/path/additional-retail-cache \
python -B -m tools.sidebar_oracle.factory_loss --check
```

The first cache is the [ready-building cache](cameo_order.md#paid-ready-building-retained-placement).
The additional cache contains the twelve physical files in `EXTRA_ASSETS`:
temperate GAWEAP/GATECH SHPs plus RTNK VXL/HVA. Obtain these through the
existing asset extractor. Both caches may be the same directory; omit
`VERA20K_FACTORY_LOSS_ASSETS` in that case. Retail bytes are not distributed.
The sidecar records the command, physical input hashes and bounds,
without repository-source hashes. `--write` deliberately regenerates the small
reference; default/`--check` compares it.

The native initializer uses healthy admitted GACNST/GAPILE/GAWEAP/GATECH,
Americans technology10 and a flat-clear map. Whole selected type constructors,
readers and post-read instructions run over RULESMD, optional LANGRULE,
Battle and Hills layers. Existing admitted map/House priors remain explicit.
CRT `0x4E6C60` constructs the Unit vector before the selected Unit constructor;
original tab/scroll gadget constructors support reached cleanup. The sidebar
is supplied as already open; ordinary full-window startup is excluded.

Whole `BeginProduction 0x4FA350` and `Factory::AI 0x4C9B20` pay MGTK's first
step, then start E1. Original zero-credit visits advance their timers to a
common due frame. The three controls supply credits0/100, call original
`BuildingLimbo 0x445880`, execute the Factory sweep `0x55B66A..0x55B68D`,
House suffix `0x4F9265..0x4F9302` and whole Strip recalculation, then use the
existing local receiver `0x6474C7..0x6474BF`. No frame rewrite occurs. Original
events execute in the same binary frame, after Factory/House; MainTick calls
Logic at `0x55DC9E`, ProcessCommandQueues at `0x55DE40`, and increments at
`0x55DE7E`. All native code and the three RNG states remain unchanged across
loss through receive. No Python cancellation, balance, timer or refund owner
supplies expected behavior.

With GATECH lost and credits0, E1 remains stage0 until the tail refunds18.
With credits100, MGTK/E1 first charge18/3; the tail refunds36 and ends at115.
The GAWEAP credits0 control refunds18 during removal, then E1 charges3 and
ends at15. Its earlier Building cleanup calls HouseAbandon before Limbo's
UpdateFactoryQueue; the control bounds the before-sweep refund. VERA retains
its existing before-sweep factory revalidation, so this does not establish
equivalence of its inline Building cleanup.

The production regression reads physical retail rules through the scenario
reader and advances the bound `SimRuntime`. It starts the same paid stage and
aligns due timers, then compares cancellation events, wallet/progress and held
object cleanup. A separate regression covers queued-copy preference, promoted
manager cleanup, retained dirty state across save/load and diagnostic replay.
These are three executed controls and connected Rust regressions, not exhaustive
factory/sidebar parity. Full House/update prefixes, queued native promotion,
other rejected-cameo order, event-ring saturation, network transport, selling,
power-loss admission and complete map/window startup remain separate bounds.
