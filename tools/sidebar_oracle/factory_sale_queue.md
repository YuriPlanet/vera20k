# Selling barracks: House queue callbacks

`factory_loss.py --sale-queue` extends the existing `ReadyBuildingFixture`, its
physical readers and joined native VM. The default prerequisite-loss selector
and its three historical controls remain separate. This fixture is consumed by
the House production lifecycle tests; it contains no Rust-derived expectation.

```sh
VERA20K_GAMEMD_EXE=/path/to/original/gamemd.exe \
VERA20K_READY_BUILDING_ASSETS=/path/to/physical-cache \
VERA20K_FACTORY_SALE_ASSETS=/path/to/physical-cache \
python -B -m tools.sidebar_oracle.factory_loss --sale-queue --check
```

`--write` deliberately regenerates `factory_sale_queue.json` and its provenance.
The sidecar pins the command and physical inputs, without executable or
repository-source hashes.

Start with the [ready-building physical cache](cameo_order.md#paid-ready-building-retained-placement).
Add real Russian assets through `asset extract` or the existing
`tools.sidebar_oracle.stock.mix/mix_hash` selector. The sidecar names every
supplied physical file and its SHA256. Selected sources are patched RULESMD and
SOUNDMD in expandmd01.mix, fixed ART/Battle in ra2md.mix/localmd.mix, audio in
langmd.mix/audiomd.mix, and the physical Hills.map entry inside loose Hills.mmx.
The common cache is a declared file-service input, not mounted VFS/theater proof.
Do not replace a missing physical file with an empty placeholder.

The added temperate building files are NGCNST, NGCNST_A/B/C, NGPOWR,
NGPOWR_A, NGHAND, NGRADR and NGRADR_A SHPs. The native reader can additionally
request original NA-prefixed/snow files, NT-prefixed buildup SHPs, CONS/FLAKT
SHPs and their physical cameos. Keep the exact files recorded in the sidecar;
fixed ART supplies the image, sequence, cameo and buildup names. A cache does
not establish ordinary buildup admission or sprite rendering.

The existing `Lists` input allocator retains its4MiB default. This expanded
physical selection explicitly grows the same page-aligned arena to8MiB;
`Reader` and native operator-new share its cursor/capacity. `JoinedFixture`
copies that declared mapped capacity into its runtime VM. No second allocator
or guessed SHP is used. Invalid/shrinking capacities are rejected.

Whole native constructors/readers establish NACNST/NAPOWR/NAHAND/NARADR,
E2/FLAKT and Russians. Native FindIndex reuses an already constructed type;
otherwise its whole class constructor runs. Indices are local to this bounded
registry, not full-stock ordinals. Physical rule strings are read through
RULESMD, optional LANGRULE, Battle and Hills layers; ART/SOUND stay unlayered.
The already-admitted House/building list, technology10 and clear terrain remain
explicit priors. Original CanBuild/FindFactory must admit both selected infantry
types, including the real NARADR prerequisite for FLAKT.

Whole BeginProduction4FA350 constructs active E2 and queues its successor;
FactoryAI4C9B20 pays a partial stage. Original QueueMission5B35E0 and
Commence5B3570 set Selling0x13 (decimal19). Whole BuildingLimbo445880 then
executes actual Detach_All44EBF0, HouseAbandon4FAA10, RemoveFromQueue4CA620,
StartNext4CA5A0/Begin4FA350 and UpdateFactoryQueue509140 as reached.

The three controls distinguish duplicate E2 tail removal before the active
refund, a different FLAKT tail popped then rejected by forced Begin, and a
surviving second NAHAND retaining the same head/queue. The current local-human
FLAKT branch also records Sidebar's earlier FactoryAbandon followed by the
House's empty no-op call; product destruction is observed once. Shared
`NativeCallTrace` owns call/return matching. Logical bool arguments and results
use their native low bytes; unspecified upper stack/EAX bits are not booleans.
Factory scalar deletion is4CA770;4CA760 only returns object size.

Snapshots retain paid balance, cash/spending, held identity, queue, factory
registry/House slot, actual product destructor completion and callback order.
They include suspended, latch and cash-stall bytes, rate, and timer start/duration
at native+2C/+34; opaque+30 is excluded. The destructor-entry checkpoint observes
the held head after refund/reset and before PointerExpired clears it. Every
FactoryAbandon return is captured, including later empty no-op calls. Original
timer/reset writes run without supplied answers.
All three complete0x3F4 RNG buffers are compared and compact hashes retained,
along with actual request/raw-advance and constructor/identity counts. The
rejected promotion must not enter Factory creation/start or Infantry creation.

This is an executed **Selling-state removal callback** boundary. It does not
execute a complete SELL command/staged reverse buildup/crew spawn, or claim
the whole sale draws no RNG. The selling Building remains limbo after the
measured callback; its later UnInit/destruction and whole-frame scheduling are
separate production integration responsibilities. Full scenario/VFS/startup,
network/UI presentation and physical buildup cadence remain outside the native
fixture. Existing OS/COM/DirectSound/allocator transports are reused.
