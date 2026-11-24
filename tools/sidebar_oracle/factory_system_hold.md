# Paid infantry system holds

Use the same physical retail cache as the existing sale/sidebar controls:

```sh
VERA20K_FACTORY_SIDEBAR_ASSETS=/absolute/path/to/retail-cache \
  python -B -m tools.sidebar_oracle.factory_loss --system-hold --check
```

`--write` explicitly replaces this separate JSON and metadata. The ordinary
Factory-loss, sale-queue, sale-sidebar and user-hold profiles retain their own
corpora. Production tests consume the new `lifecycle`, `resume` and
`forced_promotion` controls. Their recorded results are original execution,
not Python-computed expected progress, pause or affordability decisions.

## Reached behavior

An unfinished paid E2 continues when CLEG merely warps its NAHAND. Original
Temporal pause (`0x004521C0`) clears Building `+0x660` and pauses animation
slots; it does not call `HouseClass::UpdateFactoryQueue` (`0x00509140`). The
global `FactoryClass::AI` (`0x004C9B20`) steps its held object from Factory
`+0x70`, rate and timer, independently of Building `+0x660`.

The connected system-hold path requires another real queue callback. With
two NAHANDs, one warped, selling/losing the sole online one reaches its
original Building Limbo (`0x00445880`). Its detach prelude tests the remaining
producer without requiring it online and preserves E2. The Limbo tail then
calls `0x00509140`, which retains the head/FIFO through `FindFactory(1,0,1)`
but fails `FindFactory(1,1,1)`, and calls `Factory::Suspend(0)` at
`0x0050924D`. The two bytes become suspended `+0x70=1`, latch `+0x71=0`;
progress, balance and identity survive while rate and timer clear.

Temporal release sets the Building online and requests power/House rechecks.
It supplies no queue callback. The House suffix and rate-only reassessment
therefore preserve the hold, even if the rate becomes nonzero. A real stopped
portrait Left executes Select → PRODUCE → local receive → same-held-type
HouseBegin → Resume0. Alternatively a later same-category Building callback
to `0x00509140` resumes only `+0x70 && !+0x71`. A prior user Suspend1 remains
held: repeated Suspend0 refuses without overwriting its `+0x71=1`.

The local offline tail also executes original `0x006ABCD0` (Infantry16 maps
to tab2) and `0x006A60A0`. Only when that tab is active, the latter writes
Sidebar `+0x53A6` and strip `+0x3C`, then calls display redraw `0x004F42F0(0)`.
It changes no membership, selection, Factory change flag or timer and emits
no EVA. The controls execute these bodies and record their calls and flags;
they do not replace the repaint dependency with a sink.

## Shared Resume and forced promotion

The kernel controls use an actual constructed/paid E2 and original Suspend0,
then exercise both Resume0/Resume1 (`0x004C9EA0`) with affordable and exhausted
wallets. Original SpendMoney establishes exhaustion. Resume first clears
suspension, obtains actual Time_To_Build and starts a full interval. Only
afterward does it check the next charge. Affordable Resume0 sets `+0x71=1`
and runs; affordable Resume1 sets that latch and re-suspends. Unaffordable
Resume0 **or1** returns false after unpausing/rearming and retains the old
latch. No Resume debit occurs; the next FactoryAI visit retries the charge.

The forced controls advance the first paid E2 to54 with original FactoryAI,
then call original CompletedProduction (`0x004CA1A0`) as a declared successful
delivery boundary. The preceding physical Exit/Unlimbo is outside that seam.
With the sole factory warped, whole StartNextQueued (`0x004CA5A0`) executes
the actual queued-head pop, HouseBegin(force1), failed online factory query,
successful offline fallback, new E2 constructor, and Resume1. Thus the
held-start argument and constructor state come from the original caller.
This is not a hand-written new-factory state or a direct hypothetical force1
request. Affordable promotion starts held; a poor-wallet promotion remains
unpaused and cash-stalls at its next due step. Release/callback does not
auto-resume the affordable promotion's `+0x71=1`.

## Fixture and evidence bounds

All profiles share `sale_queue_inputs/setup/start`, `sale_sidebar_setup`,
`ReadyBuildingFixture`, native reader/allocator/platform boundaries and the
same held-head phase observer. Physical RULESMD, optional LANGRULE, selected
Battle mode, Hills map, ART and country/type readers execute through the
existing owners. Their admitted map/counter/power/listener priors are the
same as the earlier [sale queue](factory_sale_queue.md) and
[user hold](factory_hold_resume.md) controls. This adds no second VM, INI
parser, wallet, production or sidebar owner.

A supplied lone Temporal link precedes original notice instructions
`0x0071B0EA..0x0071B14E`: target `+0x270`, Building pause, House power/tech
dirty writes and Mark2 all execute. Whole original LetGo (`0x0071ABC0`),
including its real resume and Mark, releases it. CLEG construction, firing,
CanWarpTarget and earlier chain insertion are outside that supplied prior.
The later same-category callback is an explicit original `0x00509140` entry,
not execution of a complete second Building construction/capture. Full sale
animation, crew creation, refund and destruction precede/follow the measured
Limbo seam and are not claimed by these controls.

Each phase records `+0x70/+0x71`, stage, balance, meaningful timer start and
duration, rate, wallet/spent, held identity, FIFO, original calls, identity
cursor and lifecycle counts. All three complete `0x3F4` Main/Scenario/MapGen
RNG buffers are compared; full before/after streams are retained per control,
with phase hashes and actual requests/advances. Constructor effects during
forced promotion are measured rather than suppressed. Infantry construction
enters Foot `0x004D31E0`, then Techno `0x006F2B40`; Techno `0x006F3254`
calls raw RNG `0x0065C780` on Scenario `+0x218` and stores its low word at
`+0x3C8` (`0x006F3259`) before Resume. The recorded request entry/return
states count this method's enabled advance separately from the inherited
`0x0065C84B` hook for inline ranged advances. Disabled raw requests can
advance zero words; request count and state advancement are distinct.
Timer `+0x30` is opaque stack padding and excluded from Rust timer comparison.

The native bodies draw no RNG or detach the paid E2 on ordinary hold/resume.
The actual Building Limbo may detach Building presentation/lifecycle objects;
the held identity and Factory remain. The original constructor's draws and
identity allocation during promotion retain their own measured boundary.
Special `+0x68` products, all native save streams, GoOnline/GoOffline input
scheduling, full Rules/VFS winner selection, rendered HOLD pixels and whole
EVA/device behavior remain outside these controls. Existing EVA transport is
an observed sink; no new gameplay answer is substituted.

The sidecar pins the replay command, physical input hashes,
entry points and supplied seams. It contains no repository-source hashes.
