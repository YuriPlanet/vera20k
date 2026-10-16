# Selling barracks: retained sidebar and draw commands

`factory_loss.py --sale-sidebar` reuses the [sale queue inputs and native
fixture](factory_sale_queue.md), including original constructors, layered type
readers, House admission, BeginProduction and FactoryAI. It does not change the
default loss controls or `--sale-queue` payload. The small corpus is consumed by
sidebar view and production-rendering regressions.

```sh
VERA20K_GAMEMD_EXE=/path/to/original/gamemd.exe \
VERA20K_FACTORY_SIDEBAR_ASSETS=/path/to/physical-cache \
python -B -m tools.sidebar_oracle.factory_loss --sale-sidebar --check
```

`--write` regenerates `factory_sale_sidebar.json` and its sidecar. Provenance pins
the binary, command and physical input hashes, without repository-source hashes.
The supported original image is SHA256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
Add physical DARKEN.SHP from language.mix/cameo.mix and Soviet GCLOCK2.SHP from
ra2.mix/sidec02.mix to the sale-queue cache using the existing MIX selector. The
Soviet clock header contains 55 frames. Native LoadFileFromMIX5B40B0 and
4A38D0 read the supplied bytes; no shape or frame count is fabricated.

The executed layout initialization is shared with `geometry.py`; native tab
selection opens Infantry. Startup House+1FC is consumed while the base is
online. Whole SellBack447110(-1) then sets Selling0x13 without requesting a new
pass. Whole SidebarAction6A7780, all four StripAI calls and StripDraw6A9540 run
with retained paid E2 and queued FLAKT. The controls also execute an explicit
external House+1FC request, and a second native NAHAND survivor. The original
House suffix4F9265..4F9302 clears that request and owns rechecking/pruning.

Original FactoryAI reaches stages53 and54; stage and completion are never
supplied. Stage53 runs ordinary Sidebar/StripAI before Draw. Stage54 is drawn
before StripAI can issue a later PLACE event for completed mobile products.
The additional `selling_complete_auto_place_tail` control executes ordinary
SidebarAction, the existing local receive6474C7..6474BF, next-frame FactoryAI
and another SidebarAction/receive. It records the first event and receiver
result, both visits' held identity/wallet/timers, native changed byte and cleanup
counts. The same ReadyBuildingFixture WinMM transport handles StripAI's
CALL EBX through IAT7E1530 as well as direct calls, supplying wall-ms0 without
changing the caller's stack. Gameplay dispatch and placement execute unchanged.
`selling_complete_manual_click_tail` reuses that reached completed state and
the existing SelectAction6AAD00 owner, with declared Infantry kind16/tab2.
It records the actual indexed event, EVA request and local receiver's effect on
the held product, wallet, queue and lifecycle. This manual cameo type kind16
is distinct from automatic PLACE's held-instance kind15. Neither input route
adds a dispatcher or supplies its result. Both tail controls exclude later Draw.
Draw records linked/unavailable/completed inputs at6A99C3 and actual shape/text
commands: held E2 clears BL and uses clock frame=stage+1 (flags0x404), or
TXT_READY/text flags0x142 at completion. Unlinked unavailable FLAKT uses the
physical DARKEN shape, frame0 and flags0x401. These are original command
decisions, not raster or full-frame comparisons.

The existing `power_draw.py` command decoder and RET56 transport observe
CC_Draw_Shape4AED70. The inherited joined surface executes original width and
height getters. Original StringTable/text code executes with its cold diagnostic
fallback: TXT_READY and the requested text command are established; localized
glyphs, fonts and pixels are excluded. The physical cache is a declared input
service, not complete scenario, Rules::Process or VFS/theater-winner proof.
Entries, decisions and commands project E2/FLAKT; total counts remain visible
because the bounded registry can admit other entries. The native event outlist
is received by the explicit event-tail controls; recheck controls leave
their pruning events for the later receiver.

Complete Main/Scenario/MapGen buffers and shared request/raw-advance hooks are
compared around House/Sidebar/Draw after SELL/completion preparation. Constructor,
identity and detach entries are observed. Setup, the SELL click/audio path, complete
sale/crew effects and unrepresented later event receive/cleanup are outside that zero-draw
boundary. The manual control measures its own Select/receive boundary, including
the reached GUI sound call. The inherited observed EVA752700 RET4 sink remains
a boundary; these controls do not establish EVA playback or device RNG.
