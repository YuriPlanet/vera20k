# In-game pause and saved-game browsers: native evidence

This is the bounded evidence and acceptance packet for ordinary offline pause,
Load, Save and Delete journeys. It establishes original behavior; it does not
report Rust validation or certify whole-shell rendered parity. Implementation
and independent criticism must record their own results.

## Identity and reachability

Source is the original retail `gamemd.exe`, SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
Instructions, the PE dialog resources and vtable bytes were read independently;
Ghidra labels were only navigation aids. Undefined functions were decoded from
the pinned PE mapping with Capstone, without changing the shared database.

`4F10E0` selects resource B5 for session modes 0 and 5 at `4F1129..4F1135`,
with procedure `4F11B0` through `622650`. The B5 callbacks construct the original
saved-game receiver (`558740`, vtable `7ED2E4`), then call Load `5587F0`, Save
`558810`, or Delete `558840`. Each enters the shared browser driver `558DD0`.
Its receiver mode is respectively 1, 2, or 3. This is the same driver used by
the pre-match saved random-map browser, with different metadata and operation
callbacks, titles, active-session geometry and background.

At `4F1354..4F138A`, successful Load closes B5 with result 51E. A cancelled or
failed Load returns to B5. Save `4F140B` and Delete `4F148F` return to B5 and
refresh its availability through `4F1720`. `558DD0` returns true only when its
stored result is 40F (`5595DA..5595EF`): Save and Delete return false even after
successful operations, and their B5 callers deliberately ignore that boolean.

For ordinary offline B5, `4F1720` enables Load 51E and Delete 520 according to
`559C20`; Save remains enabled. The scan accepts readable, compatible `*.SAV`
metadata, excludes `SAVEGAME.NET`, and rejects attributes masked by 116h.
The metadata reader `559ED0` calls `67FD20`, compares version `681090` with
`[83D560]`, and marks mismatches at record +1B5. A file merely existing does
not make Load/Delete available.

## Exact resource inventory

All rectangles below are `(x,y,width,height)` in dialog units. These standard
DLGTEMPLATE resources have style 40000040h, extended style 0, bounds
`(0,0,533,369)` and font 8-point MS Sans Serif. Child extended styles are zero.

| Resource / original address | ID | Class / style | DLU rectangle | CSF key |
|---|---|---|---|---|
| B7 / BEFCF8, 340 bytes | 40F | Button / 5001000B | 425,122,108,22 | GUI:Load |
| B7 | 525 | ListBox / 50000151 | 79,78,266,187 | empty |
| B7 | 40C | Static / 50000201 | 79,44,266,24 | GUI:SelectMission |
| B7 | 686 | Button / 5000000B | 425,346,108,23 | GUI:Back |
| B7 | 694 | Static / 50020001 | 425,1,108,10 | GUI:LoadMissionMenu |
| B7 | 695 | Static / 50000200 | 2,355,303,12 | GUI:Blank |
| 2B4 / C012F4, 364 bytes | 6C7 | Button / 5001000B | 425,122,108,22 | GUI:Save |
| 2B4 | 527 | ListBox / 50000151 | 79,80,266,157 | empty |
| 2B4 | 40C | Static / 50000201 | 79,46,266,24 | GUI:SaveMission |
| 2B4 | 686 | Button / 5000000B | 425,346,108,23 | GUI:Back |
| 2B4 | 694 | Static / 50020001 | 425,1,108,10 | GUI:SaveMissionMenu |
| 2B4 | 526 | Edit / 50000000 | 80,253,266,14 | empty |
| 2B4 | 695 | Static / 50000200 | 2,356,303,12 | GUI:Blank |
| 2B5 / C01460, 348 bytes | 6C8 | Button / 5001000B | 425,122,108,22 | GUI:Delete |
| 2B5 | 528 | ListBox / 50000151 | 78,76,266,187 | empty |
| 2B5 | 40C | Static / 50000201 | 78,42,266,24 | GUI:DeleteMission |
| 2B5 | 686 | Button / 5000000B | 425,346,108,23 | GUI:Back |
| 2B5 | 694 | Static / 50020001 | 425,1,108,10 | GUI:DeleteMissionMenu |
| 2B5 | 695 | Static / 50000200 | 2,355,303,12 | GUI:Blank |

The saved-game vtable getters `55A050/55A070/55A090` return the MissionMenu
keys above; `55A0B0` supplies `TXT_GAME_WAS_SAVED`. These were read from bytes
at `829FA0/829FB4/829FC8/829FE0`, independently of resource captions.

## Active placement and paint

`608500` has no special child rectangles for these three resources.
`608CD0` expressly recognizes their action buttons and title 694.
The common dispatcher `60C0C0` sends action buttons to `60B000`, title to
`60B1D0`, Back through `609730` to `60B350`, footer to `60B550`, and ordinary
browser children to `60B7A0`. Resource conversion uses the established retail
base units 6 by 13 with MulDiv rounding. Ordinary positions then add signed
`(width-800)/2,(height-600)/2`, truncating toward zero and clamping final
coordinates to zero. There is no minimum-800-by-600 clamp on the delta.

| Ordinary child | 640x480 | 800x600 | 1024x768 |
|---|---|---|---|
| Load list | 39,67,399,304 | 119,127,399,304 | 231,211,399,304 |
| Load prompt | 39,12,399,39 | 119,72,399,39 | 231,156,399,39 |
| Save list | 39,70,399,255 | 119,130,399,255 | 231,214,399,255 |
| Save prompt | 39,15,399,39 | 119,75,399,39 | 231,159,399,39 |
| Save edit | 40,351,399,23 | 120,411,399,23 | 232,495,399,23 |
| Delete list | 37,64,399,304 | 117,124,399,304 | 229,208,399,304 |
| Delete prompt | 37,8,399,39 | 117,68,399,39 | 229,152,399,39 |

These table values are also preserved by the later original-body
`saved_game_layout` comparison described below. Right-side action
rectangles are `(width-147,227,125,25)`. Title is
`(width-165,2,162,16)`, Back `(width-147,SIDE3.top-25,125,25)` and footer
`(10,height-21,455,20)`. Back y is 402/502/702 for the three resolutions.

The active driver leaves static 40C visible: `558F8A..558F99` hides it only
when `69BBE0` reports no suspended in-game session. Reusing the pre-match
renderer without this distinction would omit each browser's central prompt.
Background painting delegates to `622B50` and active `72F540`; use the same
themed physical in-game SHP layers and SIDEBTTN mechanism as B5/BBB.
[The in-game controls evidence](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/2026-09-12-in-game-controls-shell.md) records
their original asset identities, geometry and text paint scope.

Static low style bits select alignment: the centered title/prompt uses native
flags 11h, with top alignment; SS_CENTERIMAGE does not add vertical centering
in `615A81..615AE8`. Button text follows `61358D..6135EE`: initial text bounds
left=x, top=y+1, right=x+w-2, bottom=y+h; pressed adds 2 to left and 4 to top;
alignment 5 and flags Ch enter `621040`. Reuse the established physical
button color/state selection rather than the pre-match browser's button art.

## List, metadata and input

`5596A0` builds all three lists. Save first contributes a `TXT_EMPTY_SLOT`
record with +1B4=false and a current timestamp from GetSystemTime followed by
SystemTimeToFileTime (`559801..559818`). Existing records contain the visible
description, separately retained filename at +100, FILETIME at +1AC/+1B0,
readable marker +1B4, incompatible marker +1B5 and session mode +1B8.
Unlike the MapSeed reader, saved-game `559ED0` does not classify an empty
description as incompatible. It copies the original filename including NUL
at `559FAA..559FCB`; do not import MapSeed's separate 32-byte filename rule.

The original qsort call at `559999..5599A5` uses comparator `559D30`:
dereference both row pointers, add 1ACh, call CompareFileTime, negate its
result. There is no secondary key. Sorting precedes filtering +1B5 records.
Save/Delete select visible row 0 and top 0. Load's `559BB5..559BF3` finds the
first compatible existing record in the unfiltered array and uses that raw
index as a visible-list index; ordinary fully compatible lists select row 0.
Preserve the already tested native equal-time qsort policy.

The primary column is the description, not the filename. `5599FF..559A37`
attempts to assign `*` at inner x=200 when metadata session mode +1B8 is
nonzero, but this attempt is a no-op in the ordinary browser. All three
callbacks register only x=2/255/315 through 4A6h; there is no x=200 column.
The 4A8h dispatch table at `61AE6E` selects `61B0D4`, which requires an
existing exact column-origin match (`61B127`) and returns -1 without adding
one at `61B134..61B141`. `559A37` ignores that result. Fresh control creation
zeroes the source record at `6100F2..610102`, constructs/copies it through
`623340/623610` with column pointer +F8 zero, then sends initialization 497h
at `610333`: there is no inherited implicit x=200 column. Independent
criticism corrected the earlier visible-star inference. Do not paint a star
or shrink the description column. Date and time are separate subitems.
The three callbacks configure columns using message 4A6h at x=2/255/315,
width=249/56/remainder. `559A37` onward converts FILETIME to local FILETIME,
then SYSTEMTIME. GetDateFormatA uses locale 400h, DATE_SHORTDATE=1, no format
override; GetTimeFormatA uses locale 400h, flags 0, no override. ANSI results
are converted to UTF-16. Use the host short-date and time APIs, not a fixed
ISO display or embedded snapshot creation time when a filesystem time exists.

The common custom list uses GAME.FNT cell height 17 plus 2 pixels: 19-pixel
rows, one-pixel border, columns measured from its inner left. Overflow removes
UTF-16 units and appends literal `...`. The established common scrollbar uses
20-pixel outer width, 22-pixel arrows and a logarithmic thumb with minimum 14;
track clicks jump, dragging centers on the pointer, arrows repeat after
500 ms and each 25 ms. Existing native comparisons are linked below.

Load procedure `558A30` accepts action 40F on click and list 525 notification
2 (double-click) when LB_GETCOUNT is nonzero. Save `558B90` and Delete
`558CB0` have no double-click activation. Save selection notification 1 in
`5588E0` assigns the existing description or the receiver's working default
for New, focuses edit 526 and sends EM_SETSEL; the custom NewEdit does not
implement a selection range.

Save init `558C40..558C56` sends EM_LIMITTEXT=79. `614B30` keeps a UTF-16
caret, including programmatic assignment limits; accepted typed units exceed
1Fh. `614DC0..614DE3` explicitly sends parent message 4DAh on WM_KEYDOWN
Return; Save accepts it at `558C27..558C32`. Submission reads capacity 80
including NUL and trims units at or below 20h through `727D60`.
`558FC8..558FE5` enables the action from visible row count, even with blank
Save text. An accepted action with no selection returns to the modal loop.

## Operations, prompts and exit results

All confirmations below use original `5D3490`/`5D36A0`, not a new generic
platform dialog. Yes returns 0; No returns 1. Preserve the shared prompt
layout and prevent input or painting from passing through to its hidden owner.

| Trigger | Native result and next state |
|---|---|
| Save entry with insufficient free space | `558DD0` compares `48DD50` to receiver +10=800h; `TXT_DISKFULL` with `TXT_OK`, then returns without opening browser. |
| Empty trimmed Save description | `5591E1` onward shows `TXT_MUSTENTER_DESCRIPTION` / OK, then focuses edit and stays. |
| Save selected existing file, even after changing description | Retains that record's filename; if physically present, `TXT_CONFIRM_SAVE` / `TXT_YES` / `TXT_NO`. No keeps browser and edit. |
| Save New | Generates `SAVE%04lX.SAV` from the persistent CRT rand stream until unused, then saves with the trimmed description. |
| Actual Save failure | `5593DA..55942F`: `TXT_ERROR_SAVING_GAME` / OK, re-show browser and stay. This differs from MapSeed's success-on-write-failure quirk. |
| Actual Save success | `55943A..55946D`: `TXT_GAME_WAS_SAVED` / OK, close child and return to B5. This path does not update the caller's default description. |
| Load failure | `559529` onward: `TXT_ERROR_LOADING_GAME` / OK, re-show and stay without closing B5. |
| Load success | Browser returns true; B5 closes with 51Eh and loaded game resumes through its owner. |
| Delete action | `559095..5590BA`: body is `TXT_DELETE_FILE_QUERY`, two newlines, selected description; Yes/No. No stays unchanged. |
| Confirmed Delete | `559140..55914B` calls delete callback, ignores its result, removes row and selects row0. Remaining rows keep browser open; deleting last row closes to B5. |
| Back 686 | Child result 2, no operation, return to B5. |

The saved-game operation vtable is independently confirmed at `7ED2E4`:
load +4=`559D60` calls `67E440`; save +8=`559E40` calls `67CEF0` and returns
its success byte; delete +C=`559EB0` calls DeleteFileA; metadata +10=`559ED0`.
Native temporary progress panels surround actual save/load work. Media/theater
availability checks precede load; ordinary installed-media journeys are the
acceptance priority, not uncommon media failure branches.

The ordinary Skirmish New-slot default is localized `GUI:SkirmishGame`,
**not the selected map's Basic.Name**. The original reset writer `683610`
checks session mode 5 at `683A6D..683A79`, loads key `GUI:SkirmishGame`
(ASCII bytes at `83D854`) through `734E60`, and copies it to Scenario+1360
at `683A91..683A9F`. Full initialization `686B20` takes the noncampaign
clear branch at `686B5D..686B65`; Clear_Scene calls this reset writer at
`685237`. The later MISSIONMD Name/UIName override block is campaign-only:
`686C8B..686C91` skips it for every nonzero session mode. In offline B5,
`4F143B..4F1455` formats Scenario+1360 using the literal wide `%s` at
`8240B4` and supplies that stack string to `558810`. Original English
`langmd.mix/ra2md.csf` resolves the key to `Skirmish Game`, unchanged by
string-table normalization. Campaign instead supplies Scenario+13DA;
its separate mission-specific default is outside this ordinary skirmish claim.

Independent criticism corrected an earlier ambiguous success-copy statement.
At `55947E..55948E`, receiver +C is pushed as the **source**, followed by the
local scratch buffer as the destination. `7CA489` independently reads source
from `[esp+8]`, destination from `[esp+4]`, and writes source UTF-16 units to
destination. Thus success copies the supplied default back into local scratch
and closes; it does not replace the caller's default with the submitted edit.
Reopening Save or selecting New therefore must not remember the last submitted
description through a new persistent default field.

## Escape and default-dialog proof

Opening Options with Escape is established separately from closing it.
`532620` constructs the object whose vtable is `7EBC5C`; `533C8E..533CCC`
binds key 1Bh. Its name getter `537260` returns `Options`, and execute
`5372D0` calls `647040`. The latter queues event 0Ch; its execution at
`4C7929..4C794A` enters game state 1 outside replay suppression. Sidebar
event 80F3h at `653952..653959` also enters state 1.

Ordinary B5 Escape does **not** mean Resume. `622650` creates the dialog with
CreateDialogIndirectParamA and registers it with `5D4E70`. The modal owner
pumps `623120` -> `5D4D50`; `5D4DDB..5D4DEE` calls IsDialogMessageA before
dispatch. Microsoft documents Escape as WM_COMMAND/IDCANCEL (2), and requires
the application to close a modeless dialog explicitly. B5 rejects ID2 at
`4F1320..4F134E` and returns zero at `4F16F1` without changing its result.
Common `622B50`, parent subclass `612A60` and generic subclass `610CA0` do
not translate Escape to Resume. Browser procedures likewise do not consume
WM_COMMAND ID2, so Escape alone does not operate Back. These are code plus
documented Win32 conclusions, not a live retail keyboard capture.

The primary references are Microsoft's
[Dialog Box Programming Considerations](https://learn.microsoft.com/en-us/windows/win32/dlgbox/dlgbox-programming-considerations)
(WM_COMMAND, default processing and keyboard interface) and
[IsDialogMessageA](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-isdialogmessagea).
The shared message-box callback does handle IDOK/IDCANCEL as result 1;
therefore Escape dismisses an acknowledgment and cancels a confirmation.
Initial-focus Enter has that same result; it must not activate the browser's
Save edit through a prompt. Save edit Enter is the explicit 4DAh path above.

This native Escape conclusion does not imply a current saved-seed regression:
the existing app saved-seed keyboard handler consumes its modal keys before
generic Escape routing. Its interception must survive the shared-browser
refactor; no saved-seed Escape bug is claimed by this packet.

## Cohesive implementation boundary and acceptance

Use one saved-file presentation/interaction mechanism with an opaque identity
parameter. Keep native ANSI MapSeed identity separate from the save repository's
PathBuf identity; never round-trip operational paths through display strings.
Share row selection, native ordering policy, scrolling, caret editing and
prompt ownership. Supply game metadata compatibility, titles,
active layout and real operations through the game adapter. Do not share
MapSeed's empty-description invalidation or its write-failure semantics.

The existing consumers are `ui/skirmish_shell/state/saved_seed_browser.rs`,
`ui/skirmish_shell/seed_list.rs`, `app/shell_saved_seeds.rs` and the saved-seed
sprite/text renderer. `app/persistence` owns repository reads/writes/deletion
and PreparedLoad; app orchestration must consume explicit operation results.
At evidence capture, the parent was adding `save_game`, explicit overwrite,
and `try_load_save_file`; that work is not independently validated here.
PreparedLoad still validates the active map/rules and builds a candidate before
commit. Cross-map loading needs the match-start/map-resource pipeline and
cannot be certified by a same-map browser test. VERA snapshot files are not
retail SAV format; UI parity does not imply binary compatibility.

Acceptance for this increment:

1. At 640x480, 800x600 and 1024x768, all three children show the correct
   themed active background, prompt, title, action, Back, list and Save edit;
   hit targets follow the same physical rectangles as painting.
2. Enter from B5 with no saves, then Save New, acknowledge success, reopen
   Load/Delete: availability refreshes, description/date/time appear without
   a spurious skirmish marker, and a real compatible same-map load restores
   the game. New initially uses localized `GUI:SkirmishGame`.
3. Select an existing save, edit its description, cancel overwrite, then
   confirm overwrite: cancellation preserves disk and edit; confirmation
   updates the selected file, not an accidental new filename.
4. Blank Save is actionable and shows its native warning; ordinary editor
   movement, 79-unit limit, Enter and list-selection-to-edit behavior work.
5. Load double-click activates; Save/Delete double-click only selects. Test
   enough rows to scroll with arrows/track/drag and preserve selection. Check
   the ordinary wheel no-op separately; do not impose standard list scrolling
   on this custom list without native evidence.
6. Delete cancel changes nothing; confirmed delete selects row0, stays while
   rows remain and returns to B5 when empty. Re-entry refreshes actual disk.
7. Induce an ordinary save/load failure and verify a visible error plus an
   unchanged live game; Back returns to B5; successful Load closes both.
8. Escape does not dismiss B5 or browser; prompt Escape/initial Enter cancels
   confirmation or dismisses acknowledgment without input leakage.
9. Pause ownership spans each parent/child/prompt transition. The developer
   generic save panel must not appear underneath; hidden B5/world controls
   cannot receive browser input, and clock/cursor restoration happens once.
10. Regression-check pre-match saved seeds after the shared refactor. Finish
    with independent criticism of evidence, design, diff and validation and
    a named omission/regression audit; preserve unhandled cross-map loading
    and other required journeys explicitly rather than claiming all shells.

## Existing executable comparisons and limits

Mouse wheel is not assigned a custom scroll branch. All three preserved browser
resource lists have style `0x50000151`, without `WS_VSCROLL`. In `618D40`, message
`0x20A` follows `61A8E9` → `61AB69` → `61BBF8` and reaches the saved original
WndProc through `61C45D..61C48A`. The custom visible top index is separate:
`LB_GETTOPINDEX` (`18E`) returns state `+F0` at `61A40A..61A41E`;
`LB_SETTOPINDEX` (`197`) clamps and writes it at `61A367..61A407`.
Ordinary scrolling uses the custom sibling scrollbar/`115` route
`619BAB..619BF6`. Lack of an explicit wheel case alone therefore does not prove
swallowing; the delegated Win32 behavior also matters.

The independent critic's current-Windows control probe used a hidden parent,
a 400x300 ListBox with the exact resource style, 60 rows and 18-pixel items.
Sending `WM_MOUSEWHEEL` with delta −120 retained top index 0; adding only
`WS_VSCROLL` (`0x50200151`) advanced it to 6. A forwarding subclass logged only
`0x20A` in both cases, with no nested `WM_VSCROLL` or `LB_SETTOPINDEX`.
This is host-control evidence supporting the observed live VERA no-op, not a
retail-live wheel comparison. The existing wheel no-op is preserved; no new
scrolling behavior was inferred from generic Win32 list-box documentation.

Following the integration request, `tools/storage_oracle/saved_game_layout.py`
preserves all three original dialog resources (bytes, hashes and parsed
controls) and executes complete original `60B7A0` for all seven ordinary
list/prompt/edit controls at three stock resolutions: 21 cases. It reuses
the existing bounded Win32 hooks, with supplied 6x13 DLU metrics, parent
origin zero and active-session predicate true. The default read-only replay
checks the saved golden. The new `ui/shell/saved_games.rs` test consumes these
native resource and placement results; its Rust execution remains the
integration owner's responsibility.

Additional preserved executable comparisons cover original shell geometry in
`tools/storage_oracle/in_game_shell_geometry.py`, original FILETIME comparator
and CRT qsort in `seed_order.py`, CRT rand in `crt_random.py`, and list thumb
arithmetic in `saved_scrollbar.py`. Their existing goldens establish only their
declared supplied-input scope. The [saved-seed evidence](2026-09-12-saved-seed-browser.md)
links their integration and validation; the active child resources above must
still receive Rust geometry checks and physical output validation.

Only this document, the new saved-games layout leaf and new native fixture
files were written by this evidence owner. No Cargo, Git or Ghidra mutation
was performed. Rare filename overflow/collision exhaustion, media failures
and disk allocation edge cases were not expanded into new harnesses.
