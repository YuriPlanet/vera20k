# Controlled original LAN shell

This is the external Windows/Wine counterpart of `lan_lobby.py`'s instruction
VM. The production shell capture driver consumes `../lan_shell_controls.json`.
Six original 455-byte output packets supply setup/GO inputs; declared service
inputs remain separately labelled. The sidecar pins the original executable and
substitutions. Screenshots, transcripts, compiled helpers and retail files stay
outside Git.

`NativeLanProbe.cs` preserves the existing LAN fixture used for these captures.
It only replaces socket open `53F540`, service `541820`, captured global send
`5410F0`/receive `541370`, and private send `5414C0`/receive `5415F0`.
Original manager construction, BB/BC/BD controls,
validation, settings, RNG, progress rendering, file receiver and handoff execute.
Socket/listen globals remain zero: this is an excluded transport difference.
This fixture does not implement or establish a working transport/backend.

## Read an existing normal game

`read-existing` samples explicit native state spans from an already running game
owned by the caller. It does not launch a game or require the LAN fixture marker,
environment or mailbox. The caller must identify its own normal game and supply
its Windows PID and absolute Windows-visible `gamemd.exe` path. Under Wine, run
the helper in the game's existing prefix and use a Windows PID from that prefix,
not the host Linux PID or a window ID.

```
NativeLanProbe.exe read-existing PID "C:\game\gamemd.exe" 00400000:2
```

Addresses are unsigned 32-bit hexadecimal, optionally prefixed with `0x`; counts
are decimal. One invocation accepts 1..64 spans, each 1..4096 bytes, totalling at
most 64 KiB. A span may not cross the end of the 32-bit address space. All arguments
are checked before opening the process. The expected image path is normalized
and must name `gamemd.exe`; the existing identity check compares the full path,
ignoring case.

The command requests only `PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION`
(`0x1010`). It checks image identity and creation time before and after the reads,
closes the handle, then writes one ASCII result containing a `pid`/`created`
header and ordered `address`/`count`/`hex` rows. Any failed or partial read or
identity check produces no result on standard output. It performs one pass;
there is no polling, process control, window input or memory write in this path.
Every LAN control command retains its isolated-fixture checks below.

These reads are not an atomic snapshot: state can change between spans. Wine's
`ReadProcessMemory` backend may briefly stop a target thread internally before
resuming it; see the [Wine read implementation](https://github.com/ValveSoftware/wine/blob/proton_11.0/server/ptrace.c).
Use the bytes to diagnose native state, with separately established field
addresses. This observation does not establish wall-clock timing, rendered HUD
output or gameplay parity.

`read-existing` requires a 64-bit CLR. Compile the same C# source with the existing
Framework compiler described below, using `/platform:x64` for this command; keep
the helper executable and observations in scratch. No LAN hooks are needed for
`read-existing`.

## LAN preparation

Use a separate game copy and Wine prefix. Set Windows-visible paths:

```
VERA20K_NATIVE_FIXTURE_ROOT=C:\lan-fixture
VERA20K_NATIVE_IMAGE=C:\isolated-game\gamemd.exe
```

LAN commands require a `.vera20k-native-fixture` marker file beside that copied
executable. It checks path, creation time and mailbox
identity before controlling an existing process. It never stops a Wine prefix.

Compile the C# source into the scratch fixture root with the available Framework
compiler (`csc /target:exe /platform:anycpu /out:... NativeLanProbe.cs`). In the
retail Mac wrapper this compiler needs `WINEDLLOVERRIDES=mscoree=b`; reuse that
wrapper's configured Wine environment. The helper itself needs no repository
build. Assemble the six source stubs with LLVM/Clang, for example:

```sh
clang -target i386-pc-windows-msvc -c send.s -o send.obj
llvm-objcopy -O binary --only-section=.text send.obj send.bin
```

Repeat for `open`, `service`, `send`, `receive`, `private_send`, `private_receive`;
put the raw `.bin` files in
`<fixture-root>/hooks/`. The stubs preserve flags (including DF), callee registers
and original stack cleanup. The send log holds4096 records; stop the fixture
before it overflows. Global receive admits one455-byte shell packet, or an
original51-byte loading progress packet, productAA04, from a declared12-byte
endpoint and one exact original return address. Private receive admits only a
14-byte FrameSync at the original `648710` call return `64932F`.

Expand the small corpus with Python's standard JSON/base64 libraries. For each
entry in `packets_base64` and `declared_inputs_base64`, decode the value and write
`<key>.bin` into the scratch fixture root. Keep the two namespaces distinct.
The advertised name must match the host packet identity; the guest name is
`ControlledPeer`. `lan-name PID HWND TEXT` sends original NewEdit messages and
checks its custom getter; this is controlled window input, not OS-keyboard proof.

## Shell replay

Run `NativeLanProbe.exe launch-lan` and retain the printed PID; this command holds
until that one child exits. Use real GUI input to choose MainMenu Network.

For a guest run, start this bounded event-driven service before selecting the
advertised game and pressing Join:

```
NativeLanProbe.exe serve-ad PID advertisement.bin host_endpoint.bin 600000 acceptance.bin
```

It answers actual original discovery/query requests and exits after answering
actual Join6 with captured original7. After BD has entered, supply
`host_details`, `roster`, `options` in that order:

```
NativeLanProbe.exe lan-mailbox PID publish options.bin host_endpoint.bin 5e00f9
```

Wait for each inbox consumption; an occupied inbox rejects publication. The
ordinary receive caller is5E00F9. GUI Accept must emit A1; a material host credit
change emits A0, while color alone does not invalidate acceptance. Use the
captured GO only after the intended setup/readiness controls are established.
Map-present GO emits original READY_TO_GO13h and reaches the loader.

For host admission, use real New and publish the declared compatible `peer_join`
with `peer_endpoint`. Its compatibility bytes came from an original Join6;
only the declared remote name/identity differ. Real Start with K0 emits a chat
warning. After K1, Start requests readiness; A1 does not auto-start. A second
Start publishes GO. The host's direct GO receive caller is5DD6B3, distinct from
the normal lobby consumer. Queue READY_TO_GO only for that caller.

Missing-map controls change the full-options CSV filename (field16) to an absent
`CONTROLLED.MAP`, retaining size/digest/label. Field14=1 gives K0/disabledAccept;
field14=0 gives K2/transfer-eligibleAccept. These are declared service inputs,
not captured host UI output. For successful receive, extract original
XMP29U2.MAP from `multimd.mix` with the existing `tools.sidebar_oracle.stock.mix`
and `mix_hash` owners; size83573 and SHA256
`f72fb392f0c7ffda02fd5a3db157c079598a204a66df70e125be772c112d23fd`.
Before accepting and supplying GO, run:

```
NativeLanProbe.exe serve-file PID XMP29U2.MAP host_endpoint.bin CONTROLLED.MAP
```

This responds to actual REQ_SCENARIO10h with declared FILE_INFO11h at693DE1,
then actual ACK15h triggers436 chunks through6943CF. The original receiver writes
the isolated file. Chunks are paced50ms, with a20s halfway hold for progress
capture. A fast burst once stalled in original line clipping under Wine;
pacing completed the demonstrated receiver path. No timer/renderer/completion
patch hides that observation. With no reply, the original timeout produces the
scenario-not-found modal and real OK returns to a fresh BB.

`lan-mailbox PID dump DIRECTORY` saves a bounded outgoing transcript in scratch;
`stop-owned PID` terminates only the child with the exact recorded creation
identity. Never overwrite the executable holding the child; compile control
updates to another scratch filename, using the same source owner.

## Loading, first game frames and physical abort

Start with the original shell admission above. Native Start counts nonobserver
human source nodes (`5DCEFE..5DCF32`); an AI cannot substitute for the second
human. An observer host needs two normal remote humans. Publish each peer's
map-present/readiness inputs and each READY_TO_GO through the existing mailbox;
the game fixture does not bypass or automate those setup gates.

Prepare a UTF-8 TSV with one remote peer per line, in either order:

```
ControlledPeer<TAB>C:\lan-fixture\peer_endpoint.bin
ControlledPeer2<TAB>C:\lan-fixture\peer2_endpoint.bin
```

Replace `<TAB>` with a literal tab. Names and endpoints must exactly match
native source nodes1..N, with no duplicates. The local source0 is not listed.
Before GO (or while the real loader is waiting), run:

```
NativeLanProbe.exe serve-game PID peers.tsv 600000 3000
```

This command first observes genuine `69AE90` output (type0F, length51), then
supplies each declared remote's progress100 to original global receive return
`48D552`. The other original loop receive `48DB00` is also admitted by manual
`lan-mailbox publish` for a51-byte type0F. Original `684370` still performs the
completion/timeout decision and `697B70` creates the actual connections.

On original `64A000` output at send return `64A093`, the fixture validates each
source node's native House/connection ID (`+6F`) against actual IPX connection
records (`manager+28`, record+64). It echoes that14-byte FrameSync only after
replacing byte2 with the declared remote ID and ushort+B with zero remote
commands. Frame+3, scenario CRC+7 and MaxAhead+D remain observed native bytes.
Original `648710 -> 64A0C0` validates the CRC, readiness and frame credit. Its
timer emits later FrameSync requests, so advancement may pause between those
requests; this fixture does not implement normal peer timing or gameplay.

Use physical Escape, then the actual native Abort and Leave controls. Keep the
fixture responding until the original local EXIT executes. It stops when it
observes native connection teardown, the time/frame limit, a changed identity,
or transcript overflow. Teardown alone does not prove fresh BB: capture that
return through the normal GUI. No fixture writes a House, scenario, command
queue, timer, RNG, GameActive flag or connection. Only transport mailboxes,
capture buffers and the declared six transport entry patches are writable.

The private transcript at11001000 holds4096 records of4096 bytes: caller+0,
length+4, guaranteed+8, destination connection+C, publication sequence+10,
payload+20 (maximum4064 bytes). The private inbox at11000000 is separate from
the existing global mailbox. `lan-mailbox dump` retains both transcripts.
Neither private receive nor the service command fabricates a remote EXIT or
gameplay event. These are declared empty-peer controls, not a real multiplayer
transport demonstration.

## Production checkpoints

All `lan-*` targets use `--shell-capture <target> --output <directory>` through
the existing diagnostic runner (see its CLI for the remaining flags). They
write LAN/MultiPlayer preferences; use isolated RA2_DIR and config. They enter
ordinary MainMenu Network and actual BB/BC/BD handlers. Declared service events
supply discovery, admission, host options, remote readiness/map reports and completion;
local availability follows the production file/metadata check. The absent-map
targets announce `CONTROLLED.MAP` without inserting it into discovery.
they never substitute an offline launch. Capture metadata asserts requests and
MatchSession ownership. The `lan-*-loading-first` targets stop at the genuine
network loading boundary. The target list is in
`src/app/diagnostics/shell_capture/movies/lan.rs`.

`lan-transfer-loading-first` writes the original `XMP29U2.MAP` bytes as
`CONTROLLED.MAP` at the declared file-delivery boundary, then asserts the
production parser's loose filename, length and SHA256. Its isolated retail
directory must contain a physical `.vera20k-lan-capture-root` file with exactly
`VERA20k isolated shell capture fixture\n` (a trailing newline). Existing map
files are refused. Successful completion removes the unchanged file it created;
the error/exit guard preserves any file another actor changed.

`lan-host-abort-return` and `lan-guest-abort-return` continue through InGame,
actual pause/Abort controls and queued EXIT back to BB. The host-observer
targets use the real BB preference plus two declared nonobserver peers for
loading. These are production integration controls; native runtime observer
and abort comparisons must be reported separately.

The `lan-0xbb-join-denied` and `...-join-denied-return` targets send a declared
type8 reason1 response only after a real selected-game Join. The
`...-join-timeout-return` target gives no response and waits for the normal
600×16 ms timer, checking CancelJoin, Refresh and restored lobby input. No
diagnostic changes the timer or writes modal state directly.
