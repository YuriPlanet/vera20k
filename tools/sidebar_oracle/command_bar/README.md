# Captured command bar and native setup

`neutral-bar-rgb565.bin` contains the 20,224 little-endian RGB565 words from
the original retail PCX rectangle `(0,568,632,32)`, with no translation or
resizing. `capture.json` records the PCX and payload hashes. The capture is the
same neutral 800×600 scene used for the rendering goal. Original `72FC60`
layout, stock SHP sources, and the original `87F6CC` N=1 palette independently
reconstruct every word of this rectangle.

`layout-native.json` records complete original `72FC60` execution at four
screen sizes, with a successful 16-byte allocation substituted. Source canvas
headers and the executable hash are recorded in the packet. Actual layout
entry is `72FC60`; older `72FF00` references point inside that body.

`mapping-frame-native.json` executes original `674650` mapping/gadget setup
from prepared token/header inputs and the `69DEB0` frame selector: two mapping
cases and 32 frame cases. The actual `UIMD.INI` SHA256 is
`f946c99eab17813f5e5c7b668f7b2d2e2cdc82145035106557df9b68a9854579`.
`534FA0 -> 535311 -> 674650` loads its ButtonList. Solo/skirmish commands are
Team01, Team02, TypeSelect, Deploy, Guard, PlanningMode; multiplayer adds Beacon.
Command identity, not slot, selects the numbered SHP. Draw `6D0A20` uses the
sidebar converter; `6D03A0` constructs the collapse/expand thumb. Ordinary
no-selection buttons use frame 0, rather than the disabled frame.

The ignored `retail_command_bar_atlas_and_production_append_match_native_capture`
test loads the actual archives, packs the bar using the production atlas owner,
calls the production append function, and reads actual Batch-shader GPU output
at the fixed capture rectangle. GPU presentation bytes are reduced to RGB565
words, matching native PCX extraction `7B05C0`; the already proven presentation
codebooks intentionally differ from PCX's zero-filled component expansion.
The gadget regression uses the actual retained
driver for press/release and open/closed layouts. Gameplay commands route to
existing action owners; PlanningMode/Beacon/Cheer retain their existing gameplay
limitations. The captured pixel proof covers the ordinary Allied open bar.

The user thumb branches in original `SidebarInput6D0680` are separate from
programmatic `6D1610`/`6D1660`. Original image SHA256 is
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
F1 checks Session+300 at `6D08F5` before `GUIOpenSound`, bar state or gadget
changes. F0 has no defeat guard. Accepted F0/F1 call `750920` at
`6D0860`/`6D091A` with Rules+198/+194 before testing the existing open bit.
The app uses the one retained bar state and current-House defeat projection;
the physical button still releases and consumes a refused opener.

The distinct exact-case `[AudioVisual] GUIOpenSound` and `GUICloseSound`
readers at `669423..6694B6` use ReadString128 then registered VocFind7514D0,
retaining the prior valid ID on missing/empty/unknown values. Constructor
`665892`/`665898` stores -1. Existing Rules-pass history and the fixed SOUNDMD
binder own these references; they are independent of the shell Combo keys.
Accepted cues use the existing centered sound player and Main RNG service.
Stock SOUNDMD has separate MenuACBOpen/MenuACBClose entries, both selecting
uacbopen with Control0 and zero FShift/VShift. Original equal-bound65C7E0
returns before raw draws, as covered by the consumed RMG RNG controls.
The refused F1 branch itself has no RNG request, timer write or detach call;
ordinary loss and audio lifetimes remain with their existing owners.

The [ordinary loss profile](../../map_observation.command-bar.example.json)
closes the actual loaded thumb, sells the normally deployed last yard, then
clicks the real closed thumb during the pending local result. Its
[expectations](../../map_observation.command-bar.expectations.json) require
actual F1 admission and canonical House gates before the closed outcome.
Run it through [map observation](../../map_observation.md), whose diagnostic
Thumb target records the actual mutable key and separate consumption result.
This is production behavior coverage joined to original instructions, not
native world/timing, Russian raster or device-audio equivalence. The sole-human
MPlayerDefeated path `4FC591..4FC59F` separately restores a previously open bar;
wider multiplayer bar lifecycle remains open.

Related visible fixes: tabs now derive availability from the same presented
entry collection as their cameos, following native init/add/removal/Recalc
`6A5411/6A6300/6A6820/6AA600`. The view falls back only when the active category
becomes unavailable and restores that category's parked scroll; this is a
practical projection, not a claim of full native event-lifecycle equivalence.
The developer quickplay shortcut disables both starting-force gates, preserving
authored fixtures instead of adding MCVs which reveal an extra radar area.
The actual neutral map regression preserves all 19 authored entities and shows
that the previous Bases=true path adds two MCVs. Independent source geometry
places the local AMCV's Sight=6 footprint at waypoint (30,30) on exactly the
336 extra radar pixels in the captured frame, with zero symmetric difference.
Real skirmish options and radar rendering are unchanged.
