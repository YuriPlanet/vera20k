# Ares EVA catalog and Side voice selection

`ares_eva.py` executes the original EVA reader and selected Ares 3.0 bodies
using the existing native oracle fixtures. The corpus contains synthetic voice
tags and filenames, not a mod's resources or configuration.

```sh
# Select local, legally installed inputs; these variables are not committed.
export VERA20K_GAMEMD_EXE=/path/to/gamemd.exe
export VERA20K_ARES_DLL=/path/to/Ares.dll
python -B -m tools.spatial_oracle.ares_eva --check
```

The equivalent explicit inputs are `--gamemd-exe` and `--ares-dll`. Ares DLL
selection uses the existing identity check in `rules_oracle.ares_prerequisites`.
The shared `native_oracle` owns gamemd selection and mapping. `--write` explicitly
regenerates the corpus and metadata; the default checks without writing.

## Corpus and native boundaries

The JSON has `schema_version`, `read_bind` and `scalar_roundtrips` members.
`read_bind.controls` contains source-order registration, extra-column reads,
filename selection, Side defaults, retained tag reads, explicit SetSide, column
rereads and catalog reregistration. `native_reads` records actual ReadString
inputs and outputs. `rng` records original-layout state before and after the
read/bind controls and any calls to the observed RNG entries.

- Registration at `10063660` skips the three built-in tags, deduplicates
  case-insensitively and keeps the first spelling. Its EVATypes reader has a
  200-byte buffer; DialogList uses 40 bytes, including the terminator.
- `100639C0` reads each registered extra column with capacity 16 before copying
  to the event's eight-character filename storage. A missing column on reread
  clears its previous sample. `10063920` selects a stock or extra filename from
  the process's signed selected-voice index; an invalid extra index is empty.
- Actual SideExt construction (`1002D9F0`), default initialization (`1002E190`),
  container insertion and lookup execute. Defaults depend on Side.ID: GDI,
  Nod and ThirdSide select the three stock voices; an arbitrary ID starts at 0.
  Construction itself leaves the voice field at -1. The later first-phase
  initializer supplies these ID-based defaults; it is not a constructor store.
- `1002EC00` resolves `EVA.Tag`. Missing, empty and unknown values retain the
  existing field. A known tag resolves case-insensitively; unregistered
  `<none>` resolves to -1. Reading the field does not rebind the process cache.
  Explicit SetSide (`1002FB00`) performs that later binding. A negative Side
  selects -1; an in-bounds null Side pointer retains the prior cache.
- `1002EFC0` traverses the existing Side array and performs real extension
  lookup, including when the passed INI has no `[Sides]` section. Its full
  extension dispatcher is a recorded sink in that control. Separate phase
  controls execute `1002B450`: phase 0 initializes defaults, enters phase 1,
  calls the reader and becomes 2; phases 1 and 2 call the reader again;
  negative phases and phases above 2 skip it. The full Side reader is a sink
  in these phase controls; scalar `EVA.Tag` behavior is executed separately.
- Literal `none` and `<none>` clear an unregistered tag to -1. When those
  spellings are registered extra voice tags, lookup wins and resolves to
  their extra voice indices instead.

`scalar_roundtrips` stores signed values 0, 1, 4 and -1, their actual emitted
bytes, transport operations, restored fields and cache writes. The native
SideExt field slices `1002D516..1002D526` and `1002D2EE..1002D2FE` execute with
the real ByteBuffer helpers and the existing IStream transport. Scalar restore
retains the signed value without rebinding the process cache; explicit SetSide
afterward selects the restored value.

## Ownership and substitutions

The dequeue connection is established separately by original instructions and
the installed DLL hook records. Insert752596/7525AF retains the event pointer
in node+0C; dequeue7528AF restores that same pointer into EBP. The actual
7528E8 hook enters10063AB0, which takes hook-frame EBP at10063AB5 and calls
10063610. That forwarding trampoline jumps to753380 at1006361B/10063620;
the actual GetFilename hook enters10063920 and reads the then-current
B1D4C8 at10063921. The result returns through752901 to the ordinary `.WAV`
assembly and PlayFile407B60 path. Thus enqueue does not freeze a voice/sample.
The stock queue execution and the component controls here are joined by this
instruction-level connection, not by an executed installed-hook/audio replay.
Reading EVA.Tag and restoring its scalar do not bind that process cache.

The Rust owner remains the immutable fixed EVAMD `EvaRegistry` and the existing
retained Side extension. The latter stores a signed voice index during each
actual read pass. Fresh/successful saved-load adoption calls the same Vox
SetSide boundary using Scenario.Side; dequeue selects a column from the fixed
catalog. No second queue, playback route or mutable catalog is introduced.

The read/bind experiment extends `trigger_action_values.ActionFixture`. Its
explicit per-instance stack address avoids the DLL's preferred image base;
legacy fixtures keep their default stack layout. `native_oracle.map_pe32_image`
maps Ares. The existing Probe allocator/diagnostic services attach to that same
VM, and allocation uses the fixture's heap. Native string comparison,
ReadString, vectors and secure copy execute.

The scalar experiment uses Probe's VM and attaches the existing
`native_id_snapshot.ScenarioFixture.hook` IStream transport to it. Callback
dispatch uses two nonconflicting addresses; it does not replace the transport
implementation or mutate module globals. The inherited Probe constructor
string-copy and allocator services remain substitutions.

The metadata sidecar lists entry points, assumptions and substitutions. The
read/bind experiment checks that its selected original and Ares code bytes
remain unchanged. The scalar experiment checks its Ares text similarly.

## Limits

Inputs are synthetic cached INI sections. Physical ingestion, full startup,
Rules-layer scheduling and installed hook dispatch are outside this experiment.
The extra-column hook is explicitly invoked after the stock reader. Valid Side
shells are supplied; complete original Side construction and the entire Side
extension reader are not replayed together with its outer traversal. Non-ASCII tags,
out-of-array positive Side indices and allocator failures are not covered.

Scalar controls cover one framed field, not a complete Side record or savefile.
Outer extension magic, pointer/swizzle handling, adjacent fields, OLE object
creation, failed transport and cleanup are excluded. RNG observation covers the
declared entry points; queue timers, decoder/device behavior and audible output
are not demonstrated. These controls establish component behavior, not whole
game or full save/load parity.
