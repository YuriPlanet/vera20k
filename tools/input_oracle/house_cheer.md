# AllToCheer admission and event controls

The opt-in `--house-cheer` mode of the existing selection/keyboard owner runs
the original registered `AllToCheer` command, `536C00 -> 730F30`, against the
sealed active-retail executable identified in the sidecar. The producer is
the complete 172-byte body at `730F30..730FDB`. It executes the real
`EventClass` constructor `4C66C0` and its own inlined OutList append; this
particular producer does not call the separate enqueue body `6521C0`.

```sh
python -m tools.input_oracle.selection_navigation --house-cheer --check
# Explicitly regenerate original-executable observations for review:
python -m tools.input_oracle.selection_navigation --house-cheer --write
```

The original selection, keyboard-registration and Infantry-cheer payloads are
separate references. This option does not regenerate them. In particular,
`spatial_oracle/infantry_cheer.json` supplies no keyboard, event, House-scan or
sound evidence.

The 15 retained histories contain 34 visits. Their inputs supply the current
House index, Rules rate, signed last/current simulation frames, the Session+300
local-defeat guard, and a capacity-128 OutList's occupied count/read/write indexes. Every press
executes the registered command with argument zero. Refused presses leave
the native last-frame scalar and queue unchanged. Accepted presses construct
opcode `2D`; only an available slot receives the actual 111-byte copy and OS
timestamp. The full-buffer control observes that construction and the
last-frame write still occur, while insertion and the timestamp call do not.

The corpus includes equality and rearm, zero and negative authored rates,
`INT_MAX + 1`, `INT_MIN - 1`, a signed current-frame control, nonzero locks,
the last free slot and a wrapping write index. A full-buffer history supplies
a later drained count as an explicit scheduler boundary, retaining both
indexes. The original producer then supplies the subsequent refusal/rearm
results; the fixture does not emulate draining or calculate those outcomes.

The retained JSON field `input_locked` supplies dword `A8B538`, the native
Session+300 local-defeat guard. Its historical name does not refer to the
separate Scenario+35A2/A8ED9C cinematic lock. Rust's component adapter uses
the current House defeat projection; the production keyboard and command-bar
paths retain their independent Scenario lock gates. Real loss/input integration
is tested separately without assigning either flag.

Whole original reset `734210` supplies the last-frame value `-100`, including
its two OS-clock calls. The default-rate control starts from a sentinel,
executes original Rules store `665D92..665D9B` with `ESI=Rules`, and reads its
result `1`. This is a constructor-store control, not full Rules initialization
or reading. The separate stock-rate input is `300`; the reset history includes
its exact frame-200 refusal and frame-201 issue boundary. Stock provenance and
layered reader behavior belong to the production Rules tests.

`cases[].input` contains the initial state. Each `phases[].input` supplies the
operation and current frame, plus an explicit lock/count change when present.
`phases[].expected` observes the native last frame/rate, queue count/indexes,
newly written event headers, constructor calls, last-frame writes and OS clock
calls. Event headers contain only opcode, signed House index, simulation frame
and the parallel queue timestamp. The constructor does not initialize byte
`+1` or arbitrary payload; those bytes are deliberately absent from the
reference. Existing occupied records are opaque, and new slots are located
from actual native memory writes rather than an alternate queue algorithm.

The existing `SelectionFixture` and `KeyboardFixture` own registration,
allocation, invocation and checked original-image execution. The only reached
platform substitutions are bounded registration allocation, the existing
diagnostic log boundary and `timeGetTime=100000`. No input admission,
arithmetic, event header, queue result, reset or last-frame write is supplied.
All three initialized native RNG buffers remain unchanged through each
history, and no observed RNG entry or detach entry is visited.

These comparisons cover producer arithmetic and state transitions. They do
not execute Windows key delivery, full keyboard configuration, event dispatch,
House iteration, Infantry action, presentation/audio, network scheduling or
native saved-game loading. Downstream animation/sound/RNG and the whole class
audit require their own evidence. The sidecar records the native binary,
replay command and fixture substitutions without repository source hashes.
