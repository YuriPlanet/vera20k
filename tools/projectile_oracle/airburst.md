# Original Airburst child fan

`python -m tools.projectile_oracle.airburst --check` reproduces three bounded
controls from original executable bytes. Set `VERA20K_GAMEMD_EXE` to the locally
identified supported executable. The existing `finish_vectors`/provenance owner
records replay, payload identity and declared boundaries; binary identity is
resolved by the existing native oracle infrastructure. No game assets or mod
files are inputs.

Native behavior established: BulletType ctor46BC7C initializes the
`AirburstWeapon` pointer to null; reader46C2B9 uses ReadString128 and stores the
Weapon factory result, retaining the prior pointer on empty input. The factory
does not recursively read a Weapon body. The live Weapon sweep679B10 precedes
the Bullet sweep679B33. The Rust reader regression preserves this order for a
map-only child allocation.

The parent reaches its existing shared tail after ordinary/special receivers,
animation and debris. Alive/nullify early returns still suppress that tail.
The Airburst fan469EBA emits eight neighbors in the direction initializer49F2F0
order, then its retained center Cell. `Cluster` does not set the child count.
The fan uses the parent's live Location for the cell lookup and every child
Fire origin, even when its detonation impact argument differs.

The positive control supplies Cluster2, child Weapon Damage37/Speed73/Brightyes,
and parent multiplier768. Nine actual native child constructors and Fires retain
raw Damage37, independent multiplier256, source pointer and its FirersPalette
scheme7, null installed Weapon, literal max speed50 and Brightfalse. Each
successful child takes one Scenario RandomRanged(0,32); rejection sampling uses
19 raw words for the pinned seed. First COM allocation failure instead yields
eight children, eight ranged requests and17 raw words, without spending an ID
or a draw for the failed attempt. Full Scenario/Main/MapGen state bytes and
every request/raw word are saved. Main and MapGen remain unchanged.

The launch owner divides the child Weapon speed by10, uses the original sine
table and pitch literal4012D9891049EE22, and retains the tiny horizontal
components. Actual native Fire initializes each Arm timer to(100,0) and its
unused first proximity timer to(100,INT_MAX). Runtime `FiredBullet` admission
owns classification, placement, Logic append, palette, target expiry and timers;
the fan does not duplicate the shrapnel implementation or store.

The third history is an **off-map cleanup control**, not a target-damage
comparison. Its supplied cell table has no usable-map diamond (Map+F4/F8=0).
Native AI467A35 calls568350, rejects the candidate, writes mode2 at467A44 and
reaches UnInit through467B88/467FA9. The parent is explicitly UnInit-ed at its
AI caller's continuation; children then retire in their Logic order, and actual
deferred drain removes all ten Bullets. No child reaches AreaDamage. Source
Unit lifecycle/type, ordinary on-map flight, target damage, whole map loading
and rendering are not native-executed by this corpus. Do not call this complete
combat parity. The separate Rust production regression enters real Bullet AI,
shared AreaDamage, target health and expiry on the existing valid arena.

The existing COM transport delegates successful activations to registered native
factory6C5090. Existing world-admission guards and display submission are supplied;
coordinate fixup, Construct, Fire, Logic registration, RNG and retirement remain
original code. Heap allocation, OS Interlocked/IsBadReadPtr and deferred-drain SEH
use their existing shared oracle transports. The initial two histories and the
later off-map continuation were executed in separate batches; the old two saved
histories were retained rather than replayed during incremental extension.

Runtime boundaries: native missing AirburstWeapon/Projectile/Warhead pointers can
fault; VERA retains its normal absent-definition recovery. A child has no installed
WeaponType. `Inviso+FlakScatter` therefore reaches native's null-Weapon crash;
that pre-existing recovery boundary is not new supported gameplay. The stock
V3AirburstP -> V3Cluster -> ClusterBits data was checked through the production
exact-case layered reader: ClusterBits leaves Inviso and FlakScatter unset
(false defaults), with ROT60. It therefore does not take this fault arm. This
data reference alone does not establish an active stock unit firing that
projectile. Allocation
failure is tested natively; Rust's infallible store has no injected allocation
failure mechanism.

The Rust implementation requires a represented parent and reads its live payload,
Location and post-damage source. Receiver-only virtual packets do not stand in
for a Bullet; a future Airburst death-weapon chain must represent its temporary
native Bullet through the existing lifecycle owner. Runtime reserves the native
ID before the launch draw, while its existing admission constructs/publishes the
store entry after that draw. No object callback or other Logic visit interleaves
those operations, so the optimization changes no observed RNG/object result.

Ares installs1001A090 over the entire fan, including Splitsfalse: it changes
geometry, construction inputs and launch RNG. This original implementation is
explicitly disabled in an Ares build. The later Ares consumer must extend this
same shared-tail boundary after its required WeaponTypes pre-registration,
without changing stock factory/sweep order or adding recursive body reads.

Focused Rust filters: `airburst_reference_retains_rebinds`,
`stock_airburst_tail_matches_native_children_rng_and_cleanup`,
`stock_airburst_children_reach_shared_damage_and_expire`,
`stock_airburst_reads_live_source_and_requires_a_represented_parent`, and
`stock_airburst_does_not_run_in_ares_build`.
