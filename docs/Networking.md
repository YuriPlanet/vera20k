# Multiplayer behavior and ownership

VERA20k is reproducing active-retail `gamemd.exe` multiplayer behavior, function
by function, through the existing gameplay owners. This includes synchronized
command queues, session setup, UDP transport, reliable delivery, frame pacing,
checksums, disconnects and cleanup. The audit is unfinished. A working packet
exchange is not a complete multiplayer game.

Legacy IPX sockets and the IPX protocol are excluded. Shared behavior in
IPX-named classes remains relevant. Retail's Network menu constructs the IPX
interface (`52E3D2`); VERA uses IPv4 UDP for this LAN entry point. That replacement,
including limited broadcast on port 1234, is an explicit transport adaptation.
The Internet path, router connections and remaining dependencies still need their
own active-native evidence and implementation.

## Current production path

The Network menu enters the LAN lobby. The physical service handles discovery,
join, options, roster, readiness and GO, then transfers the same socket pair into
the running match. The app keeps servicing global messages during loading and
frame waits. Gameplay uses the same OutList/DoList owners as offline play; the
transport does not own a second simulation command queue.

| Responsibility | Authoritative owner |
|---|---|
| Pending and scheduled simulation commands, native event dispatch | `sim::command_queue`, `sim::world::network_commands` |
| Event compression/decompression and persistent caches | `net::event_packet` |
| Packet construction, cumulative counts and peer frame progress | `net::command_exchange` |
| Shared reliable send queue, retransmission and response-time history | `net::reliable` |
| Global application message admission, acknowledgements and retirement | `net::global` |
| UDP sockets, datagram checksum and retained raw send attempts | `net::udp` |
| Native frame-zero barrier, send cadence and command wait | `net::live_commands` |
| LAN packet layouts and options parsing | `net::lan_messages` |
| Participant membership, admitted endpoints, launch state and checksum history | `app::match_runtime::session::MatchSession` |
| Discovery projection and physical lobby/setup service | `app::lan_service` |
| Setup UI, map admission and pending handoff | `app::shell_network_setup` |
| Production frame boundary and post-Logic continuation | `app::match_runtime::sim_tick` and `sim::world` |
| Connection dialogs, votes and participant cleanup | `app::match_runtime::network_match` |

Simulation never reads a socket. The app lends the transport-backed command
boundary to the simulation at the established multiplayer queue phase. A pending
network wait saves the post-Logic continuation; resuming it does not repeat Logic
or input admission. Presentation and local input state do not select the shared
simulation update order.

The service retains admitted setup traffic in the global receive queue during
the asynchronous lobby-to-setup transition. Loading retains the frozen setup,
so that transition guard does not block loading progress. Row bindings and
participant membership remain distinct: replacing a human row sends the native
kick, while its departure removes the admitted participant and endpoint.

## Native behavior and deliberate bounds

- Event packets use the native layouts and persistent compression caches from
  `64B4D0`, `64B660` and `64BDD0`. The packet boundary rejects a semantic event
  without a supported raw command producer rather than inventing a wire form.
- `649CA0` mirrors the local OutList before broadcasting it. Send failures do not
  undo that mirror; the reached native wait and recovery paths decide what
  happens next. Transport queues retain bytes, not gameplay decisions.
- Private and global connections share the `48C3E0` send/retry owner. Global
  admission and FIFO retirement preserve `53FCB0`, `53FF10` and `5400D0` behavior.
- Ordinary acknowledgements use the primary socket even after an alternate-socket
  receive. The native exception is the separate CDEF router connection, which is
  not implemented here.
- Raw UDP send attempts follow `7B23B0`/`7B3AA0`: hard errors retain the exact FIFO
  head; WouldBlock retires that raw attempt. The reliable owner still controls its
  retry. Portable polling supplies write opportunities in place of Win32 events.
- LAN pre/post-GO timing preserves the distinct host and guest paths, including
  conditional response-time reads and signed versus unsigned arithmetic. New
  private connections inherit those settings.
- Setup Back retains transport history within a lobby visit. A fresh post-match
  visit resets queues, sequence history, observations and lobby timing while
  retaining the portable socket bindings.

Mixed VERA/gamemd matches are not an established capability. Matching packet
layouts is insufficient: every participating simulation would also need to agree
on all relevant state and arithmetic.

## Required remaining work

The active audit follows player-visible chains and their dependencies. Completing
a ledger row or recording a gap does not complete an unimplemented behavior.

- Complete the normal Battle opening and subsequent command producers, including
  distinct keyboard/self-click deploy paths, production, placement, building
  actions, targeting, special weapons and simulation-generated publication.
- Complete map/preview transfer, random-map and cooperative handoffs, plus the
  Internet session and router paths. Preserve shared session and transport owners.
- Establish source filtering, interface selection, extra-socket/router handling
  and remaining protocol configuration. Service intermediate loading progress
  while the production loader is working.
- Exercise loss, duplication, reordering, retry exhaustion, late participants,
  desync, quit, timeout, kicks and return-to-lobby lifetimes through production
  clients. More than two participants need independent coverage.
- Complete the function-level audit with original bodies, active callers and
  retail data. Justify each exclusion with native evidence.
- Demonstrate full LAN and Internet matches between independent VERA20k clients
  on separate machines, matching per-frame checksums and correct failure
  handling. Cross-platform determinism remains required.

The local MoveFlash renderer and native DoList overflow behavior also remain
explicit residuals. Safe rejection of unsupported commands and capacity edges
is not evidence that those gameplay mechanisms are reproduced.

## Reproducible validation tools

The `tools/storage_oracle/lan_*` harnesses execute original instruction slices
for the packet, checksum, queue, timing and message controls their Rust tests
consume. Metadata states the input boundaries and substitutions. These small
corpora establish only their covered cases.

The `lan-live-host` and `lan-live-guest` shell-capture targets drive independent
App processes through the ordinary LAN service and gameplay frame boundary.
They require isolated mutable copies of the retail game directory, each with a
`.vera20k-lan-capture-root` containing `VERA20k isolated shell capture fixture`
followed by a newline. Set each process's `RA2_DIR` to its own copy and keep its
own `config.toml`.

After building the normal app with `--profile fast`, run the host with
`--shell-capture lan-live-host --width 800 --height 600 --cursor-x 400
--cursor-y 300 --output <new-host-directory> --lan-bind 127.0.0.1:32100
--lan-peer 127.0.0.1:32110`. Run the guest with `lan-live-guest`, a separate output
directory, and the bind/peer endpoints exchanged. The output manifest's
`lan.native_checksums` records per-frame checksums. This diagnostic covers a
bounded Move/Stop scenario; it does not establish a full match or separate-machine
networking. Validation results belong in the PR and checkpoint, not the tree.

The `lan-battle-host` and `lan-battle-guest` targets use the same isolated setup
and endpoints. Both select Americans through setup, deploy their AMCV (host D
key, guest self-click), and produce a GAPOWR. The input driver holds, resumes,
cancels with a refund check, then builds and places a fresh product while
retaining its paid identity. Both clients must observe both completed power
plants. The manifest retains 1,800 frame checksums and the observed production
actions. This is a bounded opening check with the same 180-second diagnostic
budget; simulation timing and network timeouts still belong to production.
