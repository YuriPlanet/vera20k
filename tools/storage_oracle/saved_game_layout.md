# Saved-game browser and controller boundaries

Run `python -m tools.storage_oracle.saved_game_layout --check`; use `--write`
to regenerate native observations and their metadata. Configure retail through
[`native_oracle`](../native_oracle.md). The accepted executable SHA-256 is
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.

The existing three `resources` and 21 `cases` are unchanged: physical B7/2B4/2B5
templates plus original ordinary child placement `0x60B7A0`. The added controls
reuse existing PE import, INI cache and stock MIX owners. They do not run a native
browser message loop, COM world load or production UI.

## Active callers, admission and retry

Original instruction evidence: main-menu Load control `0x689` selects state 9,
constructs controller `0x558740` and calls mode-1 wrapper `0x5587F0` at `0x52E0E1`.
Its browser `0x558DD0` reaches the controller's virtual `+4` call at `0x559532`,
resolved to `0x559D60`. Offline in-game options control `0x51E` reaches the same
wrapper at `0x4F1371`; its success returns to the running game loop. The separate
menu/in-game RNG distinction is documented by [the RNG owner](../rmg_oracle/rng.md).

Row metadata `0x559ED0` consumes SaveInfo through `0x67FD20`. The byte at row
`+0x1B5` denotes format mismatch against original identifier `0x2B898` at
`0x83D560`. Rebuild `0x5596A0` filters those rows after collecting/sorting raw
metadata. This is instruction/data evidence, not a new executed metadata fixture.
VERA's own save format is a separate compatibility boundary; these controls do
not establish native SAV interoperability.

The mode-1 arm `0x5594AE` uses the selected row's filename and CD requirement.
It hides the browser at `0x559513`, adjusts the mouse and calls the controller
at `0x559532`. False AL reaches mouse/error/re-show handling `0x559539..0x559599`,
retaining the browser for retry. Back/false from the in-game wrapper re-shows
and focuses options at `0x4F14BF/0x4F14C5`. Success tears down the browser and
controller; it does not construct a fresh scenario. These branch/cleanup claims
are instruction-level. Loader AL does **not** establish atomic rollback: the
lower `0x67E440` body does not propagate every `0x67E730` content-load failure.

## Executed controller and CD scalars

`wrapper_cases` has 16 original `0x559D60` executions: supplied loader result
0/1, progress handle absent/present, and four initial/post-load mouse combinations.
Original progress creation/show/destruction (`0x623230/0x622800/0x622720`) and
WWMouse getter/hide/show (`0x7B89F0/0x7B9930/0x7B9750`) execute. The wrapper clears
the two flags before the supplied lower loader, returns its AL, restores the
initial mouse count, and destroys a created progress window once.

Declared seams are CSF text, lower window creation, input service, dialog-record
teardown, Win32 mutex/window/ShowCursor services, and the lower `0x67E440` result
plus optional post-load mouse count. Mouse byte `+0x10=0`, empty native window
stack and inactive Session `+0x30D8=0` bound coverage. No full renderer, loaded
mouse state, active audio cleanup or nonempty dialog-stack claim follows.

`campaign_cd_retail` identifies the physical BATTLEMD.INI in
`ra2md.mix/localmd.mix` (2733 bytes, SHA-256
`7e40691a3480b1e5ffe6d1cdfccb83f67da4447157607ec3ecb5d2278acf0791`).
`campaign_cd_reader` executes original `0x46CCF8..0x46CD1D` and ReadInt `0x5276D0`
for all 14 Battles entries, missing-CD and retained-default controls. Original
constructor stores `0x46CEBA..0x46CEC6`, with caller-established EBX=-1, supply
the fresh default. All physical roster entries read CD 2; missing retains -1
or the supplied prior 7. Exact-case lexical strings feed the shared native INI
cache; full file parsing and campaign registry creation are outside this slice.

The saved campaign CD flows from Scenario `+0x34CC` to SaveInfo `+0x188` to row
`+0x1A8`; the ordinary skirmish default is -1. `cd_request_setter` executes
`0x4790B0` for requests -3/-2/-1/0/1/2/17 and flag byte `0x89E3A0=0/1/2`.
The exact-one flag forces -2; otherwise nonnegative requests normalize to 2
and negative values remain unchanged. Actual drive search, required resources,
media prompts and retry/cancel are not executed by these scalar controls.

## Transient click clocks

`postload_clock_reset` executes original `0x734210`, called from ClearScene
`0x685640` and PostLoad `0x6851D2`. It reads timeGetTime twice, subtracts `0x641`
from each, writes group slot -1 and the separate ID sentinel -100. Three supplied
clock pairs include zero and wrap. Direct corpus fields are `group_last_time`,
`group_last_slot`, `other_last_time`, `other_last_id`; they are observations of
those globals, not an identity claim for every selection latch. Saved selection,
control groups, Follow and bookmarks have separate native storage owners.
# Scenario elapsed score clock

`scenario_elapsed_clock` composes the existing `native_id_snapshot.ScenarioFixture`
with the original clock helper `0x6C8C40`, replacing only its `timeGetTime` import
with each row's ordered millisecond inputs. All timer words are raw unsigned
32-bit values. The three-word `elapsed_before`, `saved_elapsed` and
`loaded_elapsed` records correspond to Scenario `+0x614` (start bucket), `+0x618`
(opaque retained word) and `+0x61C` (accumulated buckets).

Original Save `0x689310` stops a running clock before its raw stream write and
restarts the source afterward; an already stopped source remains stopped. Load
`0x689470` replaces the outgoing record, then restarts the loaded stopped clock.
The separate `+0x620/+0x628` timer is reset by postread `0x683560`; it is not the
saved score-clock accumulator. Six executed controls retain the exact source,
stream and loaded values for running, stopped, zero, same-bucket, millisecond
rollover and accumulator-wrap inputs. The fixture covers these bodies and their
clock calls, not an entire world load, offline-pause route or final score reader.

## Loading progress dialog

`progress_resource` preserves physical RT_DIALOG `0xF0` at `0xBF8A08`,
including its 148 bytes. The resource is `300x200` DLU, with hidden Cancel
`0x5C0` followed by visible Static `0x5B0`. `0x559D60` calls `0x623230`
with `TXT_LOADING` and no cancel text, so the ordinary load route never shows
the Cancel control. It shows the dialog before calling session preparation
`0x69BB40` and the lower loader.

`progress_geometry` executes the original resize callbacks `0x775BC0`,
background classification `0x60C540/0x60C7D0` and centering
`0x777060/0x777080`. Its Windows DLU base `[6,13]`, cached C6 benchmark
`[300,163]` and borderless initial window rectangles are supplied inputs.
The five controls include ordinary, odd-sized and smaller-than-dialog
parents. The resulting client extent is `451x326`; children retain resource
order, with their screen rectangles, style and visibility recorded directly.
The separate mounted SHP canvas may have a different natural extent.

`progress_background` executes complete `0x621E90` for inactive and three
active-side inputs, with selected-art and selected-palette absence controls.
Its recorded surface operations establish the composition boundary: copy
the existing `AlternateSurface` (`0x887310`) dialog region into its cache,
draw frame 0 at local `[0,0]` with flags `0x400`, then copy that region back.
This path does not clear or regenerate the surrounding shell. Hiding B7 at
`0x559513` makes its controls inactive; it does not itself establish that
their existing pixels in AlternateSurface were erased. Win32 parent paint
message ordering and actual pixel rasterization remain outside these controls.

Original literals and executed selection choose `PUDLGBGN.SHP/DIALOGN.PAL`
when session `+0x30D8` is zero; otherwise Scenario `+0x34B8` chooses
`PUDLGBGA.SHP/DIALOG.PAL` for 0, `PUDLGBGS.SHP/DIALOG.PAL` for 1, and
`PUDLGBGY.SHP/DIALOGY.PAL` for other values. Missing selected assets suppress
the draw, without another side's fallback. `0x69BB40` clears the active-session
byte after progress creation/show, so these inputs bound each paint invocation,
not an entire asynchronous repaint history.

Static `0x6153E0` reads the control style at `0x615A8B`; ordinary F0 style
`0x50000000` selects flags `0x10` at `0x615A93`, then calls shared text drawing
`0x621040` at `0x615AE8` with its record font and primary color. This is the
plain left/top path, not the GroupBox inset or reveal-timer path. That drawing
classification rests on instructions and resource bytes; glyph rasterization
is not a new parity claim.
