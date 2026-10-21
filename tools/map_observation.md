# Chosen-map production observation

`python -m tools.map_observation` launches the normal production loader through
`--tactical-capture map-observe-v1`, advances the requested exact simulation steps,
and retains a hidden-window GPU readback. This is a production observation, not a
native comparator or a gameplay/pixel parity certification.

## Accepted random maps

A profile selecting the exact normal `RandMap.Sed` record can observe a generated
match. Place the desired `.SED` in a real isolated retail directory and point the
capture's `config.toml` there. The capture runs the existing setup generator and
acceptance before normal launch regeneration. Setup writes `RandMap.Sed` and
`RandMap.img` in that directory; retain original retail files separately. The
`.SED` seed controls MapGen, while the profile seed controls match startup.

The generated `map_source` receipt retains `seed_name` and nested `seed_source`
(`path`, `payload_len`, `source_sha256`) from the exact seed buffer consumed by
launch loading. Setup may normalize the record and stamp its localized description
before that read. Legacy name-only sources and empty seed files cannot certify a
capture. Other selected `.SED` names are not supported by this setup adapter.
This demonstrates the recorded production path, not native full-generation parity.

## Named commands and outcome inspection

Use `vera20k.map-observation-profile.v3` to name command targets without a separate
ID-discovery run. All v2 fields remain available. Numeric v1/v2 profiles and their
receipt shapes are unchanged. In a v3 command payload:

- `type_id` accepts a literal loaded production type such as `"GAPOWR"`.
- `sw_type_id` accepts the SuperWeapon section, such as `"IronCurtainSpecial"`.
- Actor ID fields accept `{"type_id":"HTNK","owner":"VERA-OBSERVER"}`,
  optionally with `"cell":[44,54]` to disambiguate. `Select.entity_ids` and
  `SetRally.producer_ids` can mix numeric IDs and selectors, and a
  `MegaMission` destination takes one as `{"Entity": <selector>}`.

Names are case sensitive, nonempty, and at most 256 UTF-8 bytes. Actor selectors
resolve **at the command's `issue_after_step` boundary**, using the current living,
active, non-limbo actors. A missing or ambiguous selection stops the capture before
that command is queued; the tool never chooses the first match. Each command
resolves its selectors afresh, so a later command can address a newly produced unit.
Use a recorded numeric ID when subsequent commands must retain one actor despite
movement or ownership changes. A cell means the actor's cell at the issue boundary.
Rule-name lookup reads the existing registry and interner; it creates no types.

`PlantC4` accepts the existing command's `attacker_id` and `target_building_id`,
including named actor selectors. It queues the ordinary Sabotage input; infantry
entry, charge planting, the building fuse and bridge-hut expiry stay with their
production owners. Acceptance of the profile alone does not establish a plant.

```json
{"issue_after_step":4510,"owner":"VERA-OBSERVER",
 "payload":{"LaunchSuperWeapon":{"sw_type_id":"IronCurtainSpecial",
                                    "target_rx":44,"target_ry":54}}}
```

The ordinary command producer still owns encoding and scheduling, and simulation
owners still decide gameplay admission. The receipt's `payload` is the numeric
command actually queued. Symbolic commands also retain `bindings`, keyed by JSON
pointer, with each resolved actor identity/location or rule name/category/handle.
There are at most 256 bindings per command and 1024 per profile; each binding costs
one observation sample. The wrapper checks every substitution and preserves all
other arguments. `VALID` continues to mean a valid capture, not a successful order.

The [computer Dreadnought bombard profile](map_observation.ai-dred-bombard.example.json)
deploys the observer's starting MCV through an actor selector and queues and places
its NAPOWR, NAREFN and NAYARD by rule name.

The [first-Unit query profile](map_observation.first-unit-query.example.json)
uses the same paid Allied opening through a war factory, then buys one MTNK.
Its ordinary Move orders occupy the observed refinery pad and clear it again.
The free CMIN naturally fills, returns with refinery contact, drives toward the
occupied pad, then returns to Teleport, docks, unloads and resumes harvesting.
The [expectations](map_observation.first-unit-query.expectations.json) assert
those retained outcomes, marked tank occupancy and 6,000 credits spent. The pad
is the radio producer's building origin plus `(3,1)`, distinct from the ART
queueing offset `(4,1)`. Actor selectors bind after normal production. Exact steps
are this Rust integration witness, not native timing goldens or direct query-PC
observations; native Unit-query/caller controls remain separate evidence.

Inspect a completed capture without restarting the game:

```sh
python -m tools.map_observation inspect --run /absolute/capture \
  --owner VERA-OBSERVER \
  --expect /absolute/checkout/tools/map_observation.first-unit-query.expectations.json \
  --output /absolute/inspection.json
```

`inspect` uses the existing checked-run reader, including retained input and frame
byte validation. Its compact report contains actor changes, House economy changes,
superweapon readiness/charge transitions and queued commands. Command rows explicitly
leave gameplay outcome unobserved; assertions assess requested effects separately.
Repeat `--owner`, `--type` or `--actor` to filter the timeline. Actor filters bind
the first observed identity and continue following that stable ID across ownership or
type changes within one load epoch. A quickload can reuse object IDs, so the timeline
starts a new `observation_epoch` and binds filters afresh. Type/actor filters suppress
House rows; command rows are filtered only by owner.
`--field health --field mission.effective` replaces the default actor
fields. `--max-rows 200` bounds the timeline and reports omitted rows, without
truncating assertion evaluation. Dotted field paths address retained object keys.

An expectation file declares one or more explicit checks:

```json
{"schema_version":"vera20k.map-observation-expectations.v1","assertions":[
  {"name":"power plant repaired",
   "subject":{"actor":{"type_id":"GAPOWR","owner":"VERA-OBSERVER","bind_step":1000}},
   "eventually":{"from_step":1500,"through_step":1650},
   "field":"health","op":"eq","value":750}
]}
```

Subjects are `actor` (a `stable_id`, or a type/optional owner/cell selector), `house`
(an owner string), `super_weapon` (`owner` and exact `type`), or `gesture` (a
nonnegative receipt ordinal). A named assertion
binds exactly one actor at `bind_step` (default 0), then follows its stable ID within
that load epoch; it never substitutes another matching actor. Actor assertions
crossing a quickload report identity as `UNOBSERVED`, even when type and ID match.
Bind after creation for produced actors, and after quickload for restored actors.
Numeric `stable_id` subjects also accept `bind_step` to select the load epoch.
Times are `at_step`, inclusive `eventually`, or inclusive `always` windows, all in
monotonic **completed steps**; simulation clocks can rewind after quickload.
Operators are `eq`, `ne`, `lt`, `le`, `gt`, `ge`. Equality distinguishes JSON types.
The synthetic actor field `present:false` requires an explicit missing-ID receipt;
it establishes absence from observation, not death or its cause.

An observed witness can pass `eventually`; an observed counterexample fails `always`.
Otherwise unavailable fields, ambiguous bindings or missing samples produce
`UNOBSERVED`, which fails the assertion suite. Reports retain the first matching,
failing and unavailable samples plus coverage counts. Assertions are independent of
timeline filters and bounded to 128 checks and 1,000,000 requested samples.
Enable the corresponding `observe_owners`, `observe_types` and optional observations
in the capture profile: an offline report cannot recover unrecorded state.

`{"gesture":0}` selects the actual `observations.gesture_input.receipts` row whose
`ordinal` is 0. Fields address that receipt, for example
`command_bar.thumb.release.dispatch.key_after` or `command_bar.thumb.after.closed`.
It is observed only at its `issue_after_step`, with that completed-step frame
retained. Missing or duplicate ordinals, an unsampled issue step, other steps and
missing fields yield `UNOBSERVED`; a gesture's state is never carried into later
samples or replaced by another gesture. An explicitly recorded `dispatch:null`
can be compared to `null`, while its unrecorded `key_after` remains `UNOBSERVED`.
This is an assertion subject, independent of timeline actor/House filters.

Exit status is 0 for `INSPECTED` or assertion `PASS`, 1 for assertion `FAIL` (including
unobserved requirements), and 2 for invalid evidence/input. Reports distinguish
capture validity from assertion results. They establish only the declared retained
samples, not command causality, between-frame behavior or native parity.

Foot observations include `pending_entry_500`, the pending-entry owner's stable
target handle or `null`. This read-only field is independent of NavCom, mission
and admitted radio contacts. Older sealed v6 receipts remain accepted without
the field; its absence supplies no evidence about pending entry.

Actor `object_alive`, when present, reads `GameEntity::is_object_alive()` from
the existing ObjectClass lifecycle owner. It is independent of Health and the
transitional `active` projection (`object_alive && !dying`); retained Die1/Die2
infantry can have Health0, `dying=true`, `active=false` and `object_alive=true`.
Observing it sends no selection or lifecycle mutation. Historical sealed
receipts may omit it, and omission supplies no observed Alive value. The fixed
boolean remains part of the existing one-sample actor row.

Actor `deactivated`, when present, is the Techno `+0x1C8` offline latch that
`sim::powered_unit` owns: a Robot Tank whose house has no powered Robot Control
Center. Historical receipts omit it; omission supplies no observed latch.

Actor `retask` reads the existing `suspended_target` and `suspended_nav`
references alongside current `target`, `archive`, `nav` and mission fields.
References retain their distinct tagged target/navigation representations;
`null` means the corresponding owner is empty. The observation sends no retask
or destination callback. Historical receipts may omit `retask`; omission supplies
no evidence about either suspended reference. These fixed fields remain part of
the existing one-sample actor row.

Actor `cell_marked` and `in_logic_vector` are independent read-only lifecycle
facts; neither is inferred from health, limbo or the other flag. Requested
terrain rows include nullable `path_grid`: `{ground_walkable, bridge_walkable}`
from the current simulation navigation projection, or `null` when no grid/cell
exists. The existing terrain `walkable` field describes resolved bridge terrain,
not dynamic structure passability. Historical receipts may omit these additions;
omission supplies no evidence. Post-Logic observations cannot prove ordering
between objects within a Logic pass.

The [Cell building-query profile](map_observation.cell-building-query.example.json)
uses the ordinary XMP03T4/Battle Allied opening: pay for power, barracks, refinery,
war factory, service depot and AMCV, then request deployment inside the factory's
ground footprint and again after moving away. Its
[expectations](map_observation.cell-building-query.expectations.json) check the
retained Move/NavCom at step7381, queued Unload at8401, the converted yard at40,96,
and9100 credits spent. Actor1737 and yard2176 belong to this exact launch/command
sequence; changing it requires discovery again. The initial MCV and production
types use v3 bindings. The interior request is deliberately after factory contact
release while the physical cell is still29,92. This is a production integration
control, not native timing or whole-function parity; marked dying and special
foundation inputs are separate component regressions.

Requested terrain cells also report `local_visibility`: the current viewer's
owner name and retained `revealed`, `visible` and `gap_covered` queries. It is
`null` when no local viewer resolves. These read-only observations let a GPU
capture demonstrate building pixels crossing a frontier while its anchor is
still unexplored, without changing sight or substituting a diagnostic reveal.
Older receipts may omit the field; their absence supplies no visibility evidence.

Requested terrain rows additionally retain nullable `render`. It stays `null`
for committed steps that were not drawn. After the matching ordinary draw,
`phase: "after_draw"` records the live TMP variant/pavement choice, exact atlas
key and nullable UV, palette-light words, camera and elevated cell center, and
the shared CPU A-buffer's nullable center sample (`a_bound` names whether that
buffer was bound). Missing cells/assets/light state or out-of-view samples stay
explicitly null. These are passive post-draw inputs, not GPU fragment or pixel
readbacks; they do not certify that a selected tile reached the final surface.
Historical receipts may omit `render` and supply no evidence for those inputs.

Actor `cloak`, when present, reads the existing raw signed state/progress,
live signed Rules `CloakingStages`, actual VXL/SHP route, and original type's
`NoShadow` flag. `null` means no cloak runtime. Reading this projection sends
no transition, timer or movement callback. Historical sealed receipts may omit
it; omission supplies no cloak-state evidence. This records VERA production
inputs and does not independently establish native timing or whole-object parity.

Optional v2/v3 `observe_cloak: true` extends this same `cloak` projection with
`runtime`: retained `in_playfield` (`Techno+3D5`), signed `step_delta_i32` and
`step_speed_i32`, and `step_timer`/`recloak_delay` rows containing signed
`start_frame`, `duration` and `remaining`. A null cloak stays null. The same flag
adds the existing actor `fire` readback, including the separate weapon rearm
timer; combining it with bomb or Temporal observation emits `fire` once. These
are public immutable owner queries, with no range test, transition, RNG draw,
membership change or inferred phase. Count each non-null extended cloak row
toward the shared 100000-sample budget. False or absent preserves the old cloak
and actor byte schema. This additive extension requires current child/wrapper
v8 when true; older sealed policies remain readable without it or with false.
V1 rejects any presence, and null/nonboolean flag values are invalid.

For an ordinary SUB retained-target control, reuse the Dreadnought water terrain
as in the AEGIS recipe below. Its existing lake includes (38,48) and (54,48).
Author full-health stock SUB/DEST with native scenario House names
`<human player>`/`Computer`, then issue ordinary Attack at completed step0.
Measure L0 playfield/cell/Logic membership, state and timers, actual command
execution and the next committed state before relying on pre-first-AI admission.
Do not infer reveal from a SUB shot: retail SubTorpedo has `DecloakToFire=no`.
An ordinary Stop can clear the retained target for cleanup. A schedule is not
proof of admission, and this fixture does not independently establish native
range, frame, pixel or sound parity.

Foot's optional `track` observation reads the installed Drive/Ship owner's
destination and head XYZ, selector, signed cursor and valid byte. It sends no
movement callback and creates no runtime. A paid head may survive Stop after
the destination becomes `null`; its cursor and physical pose show the remaining
segment. Historical sealed receipts omit `track`, which supplies no evidence
about that state. An explicit `null` means no installed track state was observed.

The explicit profile schema belongs to
`src/app/diagnostics/tactical_capture/map_observation.rs`. Start from
[`map_observation.example.json`](map_observation.example.json), whose launch is the
existing Rust `SkirmishLaunchSession` DTO. Change `launch.selected_map_file` to the
chosen retail map. Keep explicit countries, colors and distinct start slots, and
keep `pre_fill_house_roster` consistent with the opponents. Rust validates launch
admission; Python does not maintain a second launch parser or synthesize defaults.

New Foot observations include `foot.ammo_2fc`, an immutable signed count from
the existing Techno ammo owner, or null when no owner is installed. This lets
an ordinary aircraft Attack/return/rearm capture distinguish a spent round
from full ammo without inferring it from pixels. Historical receipts omit the
field; omission says nothing about their ammo state.

The [factory rally profile](map_observation.rally-drawing.example.json) adds the
ordinary `Select` command to a produced barracks. Its optional v2
`cursor_position: [720, 556]` is applied once after loading, before exact steps,
and retained in the render receipt. An explicit cursor must be inside the
outermost screen pixels; normal static-arrow and idle-camera checks still apply
to every draw. Place it over a neutral sidebar area when selecting actors would
otherwise produce an animated order cursor. Profiles without this field retain
the ordinary post-load center position.

Optional v2 `observe_action_line_inputs: true` adds each actor's existing
read-only facing, active locomotor/motion, applied speed fraction, crate/house
speed factors, current speed/weapon, veterancy, turret offset and rocking angles.
Building rows contain `null`. Fixed-point and floating-point bits are named
explicitly; they are not silently converted to native values. `false` or an
absent field adds no actor fields, preserving historical observations. Version1
rejects the option, even when false; present null/nonboolean values are invalid.
The [procedural drawing comparisons](procedural_drawing_oracle/README.md)
bind these observations to separately executed native readers/getters and retain
an observer-off replay proving unchanged frame bytes and gameplay boundaries.

Optional v2 `observe_projectiles: true` and `observe_anim_types: ["BBBLELRG"]`
add an `effects` snapshot at L0 and every committed frame. Each option is
independent. Projectile rows read every live projectile in stable-ID order,
including physical XYZ, source, visual counters and Logic membership. The type
name comes from the constructed BulletType; `weapon` is the independently
installed weapon name, or `null` for bullets constructed without one. Animation rows read the global
animation owner filtered by the requested types, including attached animations,
with attachment, stored and absolute XYZ, flags, visibility, bounds and the
existing complete runtime/timer. An unavailable absolute coordinate is `null`.
Rows disappear when their owners retire them; the observer keeps no invented
projectile-to-animation parent link or additional lifecycle state. The snapshot
also reads the Scenario RNG cursor without drawing. It counts one cursor plus
every retained object toward the shared 100000-sample budget and preserves the
128 MiB receipt limit. Animation filters contain 1..256 nonempty names, distinct
under ASCII case folding. Absent options (or only `observe_projectiles: false`)
add no frame fields; v1 rejects both options.

Optional v2 `observe_lasers: true` adds a `lasers` snapshot at L0 and each
committed frame. L0 reads initial state; later snapshots follow the app's
ordered simulation handoff and FPS sample, before that frame's draw.
`live` reads retained lasers in birth
order: copied source/target leptons, birth frame, Z adjustment, width, support
flag, House-color flag, resolved RGB, duration, age and timer. Current policy v5
also requires three-channel byte arrays `outer` and `spread`, plus the raw u32
`fade_end_bits`. `detail` reads the
process FPS counter, selected minimum/buffer, latch and sampling timer; it never
calls the mutative minimum-frame-rate query. Each observed actor also receives
`prism`: `null` for non-buildings, otherwise the support count, pending mode and
payload/countdown, and rearm timer. These are owner snapshots, not inferred
firing events or proof that a beam reached the GPU. One detail row and each live
beam count toward the shared sample budget. The current observation policy v5 also
requires `disk_shots`: copied delayed-shot owner rows in birth order, with an
independent positive `id`, nullable source ID, tagged target and weapon name,
signed damage, countdown (at most 1), ring `start` (0..15) and `step` (0..8).
Each delayed shot adds one sample. These reads do not advance a shot or query
coordinates; the validator checks structure rather than reconstructing gameplay.
Accepted older observation policies, including prior walk policy v4, retain the
previous `detail`/`live` schema and beam fields; missing disk/color/fade observations
are not reconstructed.
False/absent adds no fields; v1 rejects the option, and present null/nonboolean
values are invalid.

Optional v2 `observe_electric_bolts: true` adds immutable bolt, Spotlight and
particle snapshots at L0, each committed frame, and after every tactical
composite, including repeated draws during an audio completion tail. It retains
native particle IDs, the Scenario identity cursor and all three complete logical
RNG states. Each observed actor also receives the `prism` firing-state fields
described above. Counts include one detail row, three RNG streams, every bolt,
light, particle system and particle toward the existing sample budget.
These observations consume no RNG and create no effects. Compare observer-off
captures to establish that diagnostics leave production output unchanged. Draw
rows describe the post-composite registry: the last submitted geometry of an
expiring bolt is absent from its retained live vector. False/absent adds no
fields; v1 and present null/nonboolean values are rejected.

Optional v2/v3 `observe_radiation_beams: true` adds `radiation_beams` at L0
and each committed Logic boundary, plus `radiation_beam_draws` with policy
`radiation-beam-after-each-tactical-composite-v1`. Every actual map render is
sampled after the production composite, including repeated audio-tail renders
with no Logic advance. This does not depend on the electric-bolt flag or its
step whitelist. Each draw row retains ordinal, completed steps, simulation tick,
binary frame and `beams`. The ordinary map presentation clock remains unchanged.

Both snapshots contain `live`, `composites` and `segments`. `live` copies birth
frame, world-lepton endpoints, Z adjustment, RGB color, duration, amplitude bits,
current progress/spacing and nullable cached length bits from the existing beam
owner. `composites` copies the last actual per-beam progress-before/after, expiry
and segment-count records. `segments` copies the exact submitted line endpoints
in logical screen pixels, their two signed Z adjustments, and packed replacement
`rgb565` words from that composite. The observer performs no projection, color
conversion, progress advancement, birth, RNG draw or extra composite. Logic
snapshots may still hold work from the previous draw; the separate draw rows
identify the actual consumption boundary. An expired beam can be absent from
`live` while its final composite record and visible segments remain recorded.

The selected mode0/kind1 birth lasts 15 Tactical composites, not 15 Logic ticks.
The validator checks receipt shape, clock coverage and the submitted segment
tally; it does not reconstruct native lifetime or geometry. Count one snapshot
plus every live, composite and segment row against the shared 100000-sample
budget. Absent/false emits no new fields and preserves historical payloads.
True requires current child/wrapper v8; v1 rejects any presence and present
null/nonboolean flags are invalid. This telemetry does not certify native
frame/color parity or the unselected radiation-beam family.

Optional v2 `observe_bombs: true` adds `bombs` at L0 and each committed frame.
The bomb owner independently reads its sorted carrier index and the carried
records: carrier and planter stable IDs, planter House, and signed start/end
frames. A null planter preserves pointer expiry. The observer neither refreshes
BombVisible nor calls Attach, expiry, clock, Defuse or any other gameplay
callback. Every observed actor also receives `fire`: its existing last-fire
frame, Foot body counter, shared Stage value and signed rearm timer. Current
actor `target`, mission and Foot Doing/firing latch retain their ordinary fields.
Together these boundary values can associate a real attach or defuse with a
same-turn target clear; they do not create firing events or certify native
arithmetic or whole-weapon parity. The owner snapshot plus each carrier index
entry and each carried record count toward the shared 100000-sample budget;
the 128 MiB receipt limit is unchanged. False/absent adds no frame or actor
fields, v1 rejects any presence, and present null/nonboolean values are invalid.

Optional v2/v3 `observe_temporal: true` adds `temporal` and the same `fire`
readback to every observed actor at L0 and each committed frame. `temporal`
reads public owner queries: `has_link`, nullable `warp_target` and `chain_head`,
nullable `inside_transport` (`transport`, `open_topped`), and nullable `cargo`
(`count`, ordered `passengers`). The count, list, carrier relationship and link
queries remain independent; the observer never repairs membership, calls a
gunner callback, advances a timer or draws RNG.
The existing Unit `current_weapon_138` and `current_turret_124` report IFV mode;
private burst/charge state is not exposed. Count one snapshot and every cargo
entry toward the shared sample budget. Fire state is emitted once when both
bomb and Temporal observations are enabled. A preceding Attack does not prove a
live Temporal victim at gunner removal: ordinary UnloadPassengers clears its
attack target before the callback, so inspect the reached phase boundaries.

This opt-in is additive within child/wrapper v8: false or absent leaves the
historical actor and fire byte schema unchanged. True requires the current v8
policy; older sealed policies without the flag remain readable. V1 rejects any
presence, and present null/nonboolean values are invalid. Use the existing
`render_depth_fixture --temporal-gunner` scene for stock full-health CLEG/FV,
enemy HTNK and distant enemy GAPOWR. Its authored native House names are
`<human player>`/`Computer`; ordinary launch diagnostics still use
`Americans`/`Computer1`. Boarding, approach, live rearm, unload and later CLEG
firing must be established by runtime history, without actor or rule injection.

Optional v2/v3 `observe_spawns: true` adds `spawns` to every observed actor at L0
and each committed frame. It is `null` when the actor has no spawn manager.
Otherwise it reads current rules `type_spawns_number`, retained `spawn_type`,
`count_docked`, `count_alive`, `mode`, `regen_rate`, `reload_rate`,
`current_target`, `queued_target`, `update_timer`, `spawn_timer` and ordered
`slots`. Slot rows contain nullable `child`, `state`, `timer` and `is_missile`.
Mode and state use the existing enum names; targets use the ordinary tagged
target shape. Timers retain signed `{start_frame,duration}` values without
advancing or inferring a countdown. Current type capacity, the two count queries
and retained slot sequence are independent reads, so analysis can detect a
disagreement. The observer never launches, recalls, docks, refills or retires a
child. It includes managers and limbo children already admitted by the ordinary
actor filter; removed actors use the existing `missing_actor_ids` history.
One present manager and every retained slot count toward the shared sample
budget, in addition to the actor. False/absent adds no actor fields; v1 rejects
any presence, and present null/nonboolean profile values are invalid. These
snapshots support a real Carrier sortie/reload/removal observation, not native
flight scheduling or rendered parity by themselves.

The [Carrier profile](map_observation.carrier-spawn-pips.example.json) selects a
map-placed stock CARRIER, clicks an unarmed enemy LCRF, then orders Stop after
the first damage. Materialize its scratch map with the existing
[Aegis adaptation recipe](#human-guard-aegis-distributed-fire-observation) and
the same Dreadnought water terrain. In that recipe use this profile, retain the
empty Structures/Infantry/AITriggerTypes sections, set Waypoints to
`0=48040` and `1=62062`, and replace the Units body with:

```ini
0=<human player>,CARRIER,256,40,48,64,Guard,None,0,-1,0,-1,1,1
1=Computer,LCRF,256,52,48,64,Sleep,None,0,-1,0,-1,1,1
2=Computer,LCRF,256,55,52,64,Sleep,None,0,-1,0,-1,1,1
```

Replace `launch.selected_map_file` with that map's absolute path. The second
craft preserves an opponent after target loss; neither type nor weapon rules
are changed. At 800×600, camera cell(43,46), Normal sidebar and zoom1, the
selection/attack clicks are(166,269)/(526,449). Check actual camera and picker
receipts before reusing them with another layout. Read `spawns` alongside the
Hornets' ordinary limbo, Ammo, target/nav and missing-ID observations; docking
fills a slot while it is still Reloading, and ReadyDocked does not add another
filled pip. The example's ending step bounds a production witness, not a native
timing golden. A parent-removal witness needs a separate ordinary damage case.

Optional v2 `observe_audio` records device-mixer queue pulls from requested ordinary
SFX Players. For example:

```json
"observe_audio": {
  "sound_ids": ["SquidMove"],
  "max_events": 4,
  "max_samples_per_event": 262144,
  "completion_tail_ms": 4000
}
```

`max_events` accepts1..32, with at most262144 samples per event.32 events
can retain the stock low-power line's typing requests and insertion sound;
the maximum PCM hex occupies64MiB within the128MiB receipt limit. End a
typed-message profile after its reveal settles: the audio completion tail
freezes the step clock while rendering continues, so a still-revealing
TextLabel can request its typing cue on every repeated paint.

Current coordinate-sound rows also retain signed XYZ `coordinate_leptons`
and the u32 caller `flags`. The reader accepts them as a bounded pair and
does not derive them from projected pixels. Historical rows may omit both;
their omission supplies no raw coordinate or flags evidence.

Optional `observe_audio.coordinate_request_scope` selects `all` (the
historical default) or `watched`. The latter keeps only coordinate nodes whose
resolved sound ID matches `sound_ids` in each passive frame snapshot. This
bounds a long focused run without retaining every unrelated map ambience
node. Unbound nodes are excluded in that scope. Playback and final whole-pool,
channel and coordinate-node counts remain global; an empty filtered list does
not establish an empty global registry. Old executables require the historical
four-field audio profile.

`map_observation.radar-availability-audio.example.json` follows an ordinary
paid Soviet opening on XMP03T4, sells the completed NARADR, then builds and
places its replacement. Its RadarOn/RadarOff filter observes the registered
centered one-shots from House508DF0 through Radar656DF0/656BE0; the housing
owner suppresses equal availability updates and source hydration is silent.
Inspect actual payment, placement and sale results alongside sample identity,
PCM and event/channel retirement. The observation does not certify native
whole-world scheduling, device mixing or speaker output.

The production SfxPlayer enables the recorder at accepted L0. Its existing
decoder, arbiter, Player, gain/pause/stop processing and device mixer still own
playback. A forwarding Source observes the Player output that the production
device mixer pulls, **before mixer resampling/summing**, including any synchronous
initialization prefetch. It does not establish that each sample reached the DAC
or physical speakers, and is neither OS loopback nor native audio equivalence.
Both paths preserve the ordinary new-Player, mixer-add, gain-setup, append order;
unobserved output retains `Player::connect_new`. The diagnostic
callback uses preallocated atomic slots, with no per-sample diagnostic locks or
allocations; Source format queries occur at span boundaries.

Each observed submission retains the reusable arbiter slot, owner, resolved
sample identities, decoded source sample count, action/context rows, format
spans, exact little-endian f32 PCM bytes, finite/nonzero counts and SHA256.
`submission` is the capture ordinal; `event` is a reusable production pool slot.
A stopped old source can finish draining while a new submission reuses its slot.
New outputs also retain `event_serial`, the allocation serial read from the
existing arbiter owner at submission. Delayed binding, sample metadata and
terminal actions match both slot and serial; they cannot complete a newer
output that reused the slot.
The PCM includes any Player queue filler silence. Completion requires both the
existing SFX owner's terminal action and the device mixer pulling the queue to its end;
recording is not cut off merely because a stop/release was requested.

Immediately before finish, the SFX owner supplies its existing pending/live
payload tokens and the observer reads the arbiter's token/channel queries.
`event_live_at_finish`, `channel_live_at_finish`,
`token_slot_occupied_at_finish`, `pending_payload_at_finish`
and `live_payload_at_finish` preserve those independent ownership results.
They are not inferred from `settled`, the terminal action or source end.
Payload membership remains observable after token invalidation, so an orphan
payload cannot disappear merely because its slot was recycled. A live finished
event may still await the next ordinary service's reap after its Player ends.
Token liveness describes that allocation; slot occupancy describes the physical
pool slot, including a newer allocation or a dead record awaiting reap. An
explicit Stop invalidates its serial before the reaping pass. Report-level
`live_event_count_at_finish` and `busy_channel_count_at_finish` retain the
arbiter's global live-list/channel counts, including unfiltered cues. Their
capacities are the owner's 300 pool records and 13 ordinary channels.
`coordinate_request_count_at_finish`, when present, reads the SFX owner's
independent 200-node Voc750E20 request pool. `null` means that pool was not
observed. Scoped PCM completion does not imply that this count or the global
event/channel counts are zero. Retained-coordinate submissions use the typed
`owner_role:"coordinate_request"`; their request IDs are independent of
positional actor and unit-voice IDs, even when the numbers coincide.
Without a final owner snapshot all five output values and both aggregate counts
are `null`, explicitly unobserved. Historical receipts may omit the entire
six-field output group and the two-field aggregate group; partial
groups, malformed types and duplicate slot/serial submissions are rejected.
The fixed metadata uses the existing bounded submission rows and receipt
limit; no callback lock, allocation, RNG draw or audio decision is added.
Exported audio metadata preserves both groups when present. A selected quiet
capture can assert zero global counts and no token, occupied slot or payload;
terminal PCM alone does not establish those cleanup boundaries.

When the existing continuous playback owner supplies its passive observation
handle, `resolved_samples` instead records actual clip starts copied into the
ring, and `source_sample_count` records its nonpadding interleaved sample
copies. Lookahead can copy a clip that Stop later discards. The optional
`playback_clips` table therefore separately retains each loaded clip's `name`,
`filled_samples` and actual iterator-delivered `pulled_samples`, before Player
gain. Ring padding contributes to observed PCM but never to those clip counts.
The table has the sound reader's maximum 32 loaded entries; starts retain the
existing 128-name bound, and exhaustion sets truncation. Counter totals must
equal `source_sample_count`, and pulls cannot exceed fills. Export preserves
these values without decoding the assets again.
`source_state_alive_at_finish` reads the playback owner's weak reference to
its shared loaded clips, cursor and draw capability, independently of the
observer's passive counters. The two-field playback group is omitted for
historical paths without that handle; partial or malformed groups are rejected.
The completion wait additionally requires this source state to retire, exact
recorded tokens to be reaped, and matching pending/live SFX payloads to disappear.
It adds no callback diagnostic lock, allocation or sample-selection decision.

An action's `context` is the **last completed exact-step capture boundary**.
It remains fixed during a step, including sound submission, and refreshes after
the step receipt commits. For example, a sound emitted during native frame 0
can be submitted with context step 0/frame 0, then started by the following
audio service with context step 1/frame 1. It is not an exact action or PCM
sample timestamp. `service_ms` is SfxPlayer's last serviced wall-clock value;
actions between service passes retain that value rather than sampling a new
clock. Frame observations are post-commit state: the step 1/frame 1 row contains
the result of executing native frame 0. A literal load also refreshes the
context's loaded frame/tick while `completed_steps` remains monotonic.

Limits are 1..16 distinct ASCII sound IDs (128 bytes each), 1..16 submissions,
1..262144 samples per submission, 64 action/format rows and 128 resolved sample
names per submission. After the requested exact steps, the existing frame/audio
service continues without simulation ticks for at most the requested 1..10000 ms
tail. It stops when observed outputs end; exhaustion/timeouts fail the capture
instead of claiming complete output. Repeated tail draws are counted separately
from the exact-step draw transcript. PCM stays inside the existing sealed
`capture.json`; the 128 MiB receipt limit is unchanged. Frame `audio_state` and a
load segment's `restored_audio_state` read the existing Main/Scenario cursors and
filtered actors' body counter, MoveSound latch and countdown, and the Gattling
owner's paired `gattling_stage`/`gattling_value` getters (`TechnoClass+0x140/+0x144`)
without advancing any owner. These signed 32-bit values are independent of the
unit animation's `stage_f8`. Historical audio actor rows may omit both Gattling
fields; a partial pair, null, boolean or out-of-range value is rejected. A
stage-transition witness must require their presence explicitly rather than
infer progression from animation, firing counts or nominal ticks. Each row
charges its actors and both cursors to the shared
100000-sample budget, including the immediate restored row. Optional
`audio_state.coordinate_requests` reads the SFX owner's current ordered request
list without validating handles or driving playback. Each row retains
`request_id`, the handle's nullable `sound_id`, immutable `source_world_pixels`
and `source_cell`, and nested `suspend_depth`. The pixels use VERA's absolute
world frame, including its 15-pixel Y translation; they are not client pixels.
Each retained request costs one observation sample. An absent field or `null`
leaves the pool unobserved; an empty array observes an empty pool. Row changes
can bound request survival and retirement to capture boundaries, but do not
supply an exact within-step timestamp. Sound RNG and device
timing are presentation inputs; repeated PCM captures need not be byte-identical.

Export retained PCM after the wrapper validates every byte and summary:

```sh
python -m tools.map_observation export-audio --run /absolute/retained-run \
  --output /absolute/new-audio-directory
```

Each format span becomes an IEEE-float WAV; no retail decode or mix runs again.
The export records a final incomplete channel frame, if present, as
`unframed_tail_samples`; those samples remain intact in the original receipt.
Neither export nor validation writes into a retained run.

The [GI damage-feedback profile](map_observation.e1-feedback.example.json)
uses a full-health local GI, one initial tank and four supporting stock tanks.
Generate its clear-ground placement map before capturing:

```sh
python tools/render_depth_fixture.py target/stock-e1-feedback-removal.map --damage-feedback removal
```

Copy the profile and set `launch.selected_map_file` to the generated map's
absolute path; relative map names are searched in the retail install.

The first Attack admits ordinary damage; the later volley can remove the
GI while its unbound `GIFear` event is playing. Inspect the actual health crossing,
submission/start/completion contexts, null owner, physical sample and retirement.
The `single` fixture variant supplies only the initial tank for a natural-end
control. Both variants retain full map health and retail weapons/rules. A launch
seed does not specify the later percent draw after loading and audio service;
absence of a submission alone cannot establish the exact rejected value.

The [movement-voice profile](map_observation.move-voice.example.json) selects
one full-health stock MTNK and issues an ordinary command click on clear ground.

Generate its map with:

```sh
python tools/render_depth_fixture.py target/move-voice.map --move-order-voice multiple
```

Copy the profile and use the map's absolute path. The `stock` variant retains
all retail type values. `multiple`, `unknown-first` and `unknown-only` override
only `VoiceMove` with inputs executed by the native
[movement-voice comparison](spatial_oracle/move_order_voice.json); geometry,
health and weapons stay identical. Inspect the committed acknowledgement,
queued/head voice actions, physical sample/finite PCM and event/channel
retirement. Loading and service can advance Main before the order, so a launch
seed alone does not determine which entry a multi-entry list chooses.

The [Dolphin Sonic profile](map_observation.sonic-wave.example.json) uses a
stock DLPH, enemy CARRIER and a distant enemy survivor. Extract the physical
`multimd.mix/XMP03T4.MAP` with the existing asset tools, then generate the scene:

```sh
python tools/render_depth_fixture.py /absolute/ordinary-dolphin-sonic.map \
  --sonic-wave /absolute/XMP03T4.MAP
```

This copies the supplied map's terrain/scenery/lighting/waypoints and authors
only the scene actors/Houses. It changes no unit, weapon or Wave fields. The
fixed naval cells `(106,118)` and `(108,118)` must be admitted by the production
loader; copied pack bytes alone are not proof of water or a successful shot.
The optional `observe_waves` reads the existing Wave owner and its derived Sonic
polygon, without advancing lifetime or RNG. `observe_projectiles` and the existing
`observe_bombs` fire-state view retain the real shot and rearm timer.
Set the profile's selected map to the result. The example waits before ordinary
Attack so the stock DLPH completes its initial cloaking transition, then issues
Stop while a shot can still be live. That initial transition's existing
`Detach_All(false)` can otherwise interrupt a wave by clearing its owner. No
cloaking or weapon field is overridden. Inspect the actual firing, persistent
body, interrupted fade and cleanup, and retain endpoints at the observed steps. The real GPU readbacks exercise the
shared retained Display destination pass, while the original pixel controls
are the separate bounded comparison in `magnetic_wave.py --sonic`. Neither
establishes original full-match/pixel equivalence for the retail scene.

The [Terror Drone firing profile](map_observation.drone-fire.example.json)
orders a retail DRON to jump a sleeping enemy tank, then a local tank to fire
SonicZap at the host, then issues Stop to the released DRON. Generate its scene with
`python tools/render_depth_fixture.py /absolute/drone-fire.map --drone-fire`.
Optional `--terrain-template /absolute/extracted/Hills.map` retains that physical
map's terrain packs, scenery, lighting and waypoints; the template must admit
the fixed scene around `(48,48)`. Set the profile's selected map to the result.
DRON, DroneJump and its ART keep their retail fields. The fixture explicitly
overrides MTNK's primary to SonicZap and SonicZap's damage to 4 so a normal shot
can exercise the production parasite-release receiver while both actors survive.
`observe_disguise_inputs` retains the existing read-only SHP-frame projection. Exact
release/idle steps must be inspected from the run. The example Stop at step 49 is based
on the retained Hills scene: change it if a different terrain template shifts the
hit, since a type selector cannot bind a still-limboed DRON. The profile does not
prescribe a successful hit or native timing and is not an original scene/pixel
comparison.

The [building power-status profile](map_observation.building-power-status.example.json)
selects an ordinary full-health owned GAPOWR in the existing clear-ground scene:

```sh
python tools/render_depth_fixture.py target/building-power-status.map --building-brackets
```

Copy the profile and set the generated map's absolute path. Its three owned
plants keep their retail Power, health and rules. Inspect the selected building's
localized House power/drain text, then the same scene after an ordinary right-click
deselect. The text reads the House's retained totals; it does not sum buildings or
assess power during drawing. The existing Building-array text pass draws after
Ground2, with the same physical GAME.FNT and House color as PRIMARY labels.

The [Ivan bomb-clock profile](map_observation.bomb-clock.example.json) uses an
ordinary Crazy Ivan planting order on a friendly stock tank:

```sh
python tools/render_depth_fixture.py target/bomb-clock.map --bomb-clock
```

Set the generated map's absolute path in a copy of the profile. It selects Ivan,
clicks the nearby tank, then deselects Ivan and selects the carrier. Health,
weapons, visibility and the fuse keep their retail values. `observe_bombs` reads
the existing bomb owner's carrier index and records at each committed step.
Use 120 steps to inspect the active selected clock; the supplied 480-step run
continues through detonation and removal. Each clock belongs to its admitted
Display parent's Extras prefix, before rank, brackets and health, with the
global tactical clip and the existing MOUSEPAL material.

Literal `key` gestures may optionally include a unique `modifiers` list containing
`Ctrl`, `Shift`, `Alt`, and/or `Super`. These use the shared modifier-key owner and
ordinary key edges, and release every modifier before rendering. Bare historical
gestures keep their existing receipts. The release game's Ctrl+Shift+M/N shortcuts
exercise the production quicksave/quickload owner. A quickload gesture requires
`allow_load_segments: true` and must actually restore an earlier clock. Up to 16
`load_segments` retain the triggering gesture ordinal and before/after tick,
binary frame and simulation milliseconds. Capture-step numbering remains
monotonic; draw/receipt validation follows the restored clock, including its
rewind. This is VERA same-content snapshot loading, not original SAV compatibility.
Run these profiles with a fresh scratch working directory/config: the production
save repository uses that working directory's `saves/` folder. There is no
diagnostic save backend or bypass of preparation/commit.

The [planning mode profile](map_observation.planning-mode.example.json) selects
the opening AMCV through the ordinary production command, holds literal Z across
two tactical clicks, sends a repeated press, taps S and X while held, then
releases it. The S/X gesture receipts expose the refusal HUD rows and preserve
the queued-command list so an ordinary Stop/Scatter fallback can be detected. A second sequence
uses the loaded `PlanningMode` command-bar gadget. The capture runs 1200 steps
and retains the editing state, queued opaque planning records and subsequent
movement. It requires the stock Z binding and a loaded, open command bar.

Optional `edge` on a key gesture is `pressed`, `repeat` or `released`; omission
keeps the historical full tap. The production keyboard dispatcher permits
separate edges only for the actual planning binding or its owned release.
Profiles must balance every press and release. The camera seal admits only that
planning-owned held key; other held keys remain invalid. Keyboard receipts add
`planning_before` and `planning_after`; input observations add `planning_active`
and `planning_paths` (actor ID, ordered cells, optional loop start and editing
flag). These read the simulation owner without changing it. The Python validator
checks types, bounds, key-edge outcomes and matching releases; historical
receipts may omit the added planning fields.

A v2 `command_bar` gesture names an exact loaded bar command, for example
`{"kind":"command_bar","command":"Guard"}`. Rust resolves the name through
the existing command registry, current UIMD slot layout and retained gadget,
then sends ordinary left press/release edges at its current center. A missing,
closed, disabled or inconsistent control fails capture. This does not call a
semantic command dispatcher directly. Its receipt records `command`, actual
`slot`, retained `gadget_id`, `rect` and `resolved_position`; the shared mouse
receipt checks the expected gadget captured the press and release restored
neutral input. The command-bar receipt adds one sample to the existing budget.
Profiles containing it use `map-local-gesture-v4`; historical tactical/sidebar/
key-only profiles keep their existing policy and receipt shape unless they also
request a right button as described below. It accepts no
click modifiers. Python checks receipt shape/geometry without reproducing the
runtime command registry, layout or action admission.

The diagnostic-only `{"kind":"command_bar","command":"Thumb"}` targets the
retained open/close thumb through the same mouse driver. It does not add a gameplay
command or a command slot: its receipt omits `slot` and adds `thumb` with `before`,
`press`, `release` and `after`. `before`/`after` are state objects; each edge has
exactly `state` and `dispatch`. Each state records boolean `closed`, `pressed`,
`sticky`, `disabled`, `local_player_defeated`, `input_locked` and `left_held`, u16
`gadget_id`, four i32 `rect` components, nullable string `current_house`, nullable
u8 `side_index`, and nullable boolean `has_lost`/`exit_ready`. These are passive
observations of the existing owners, not flags inferred from remaining actors.
Each nonnull dispatch records u16 `masked_flags`, u8 `modifier`, u16 `key_after`
and u32 `action_result`. `key_after` is the actual mutable gadget key after
dispatch; `action_result` records consumption and is not a fired gadget ID.
Validation preserves the observed bar result, including a pre-fix reopening.

A Thumb may record `left_press_captured:false` only when `before`, press and
release all record `input_locked:true`, both edge dispatches are `null`, and
`closed` is unchanged through all four states. The driver does not read a stale
dispatch when the Scenario guard prevents entry. Physical input still records
`left_held:true` on press and `false` before, on release and after neutral refresh;
`neutral_input_restored` remains required. Other gestures keep their existing
capture requirements. A Thumb adds four state samples and one sample for each
nonnull dispatch to the existing one-sample command-bar receipt (normally seven
total, or five for a locked refusal). Legacy command receipts retain their exact
schema and require no Thumb observations.

The Area Guard [standing discovery](input_oracle/profiles/area-guard-standing-discovery.json)
and [combat discovery](input_oracle/profiles/area-guard-combat-discovery.json)
profiles load authored object placements on unchanged terrain with retail MTNK
and E1 types. Materialize `selected_map_file` as an absolute path before capture;
relative names resolve under the retail directory. Inspect L0 actor IDs before
reusing the [literal G](input_oracle/profiles/area-guard-key.json),
[command-bar Guard](input_oracle/profiles/area-guard-bar.json), or
[mixed tank/GI selection](input_oracle/profiles/area-guard-mixed.json) profiles.
They use ordinary Select followed by the actual input route, observe tagged
queued orders and actor retask state, and retain CommandBar SFX PCM. The mixed
key profile also exposes the existing Main cursor; unit order voices use the
separate voice queue and are outside this PCM observer. Native ordered voice
requests and list draws require the separate executed input comparison.
The [in-range retask profile](input_oracle/profiles/area-guard-retask.json) places
the enemy four cells away, requests G, later presses S, then clicks empty ground
at cell `(38,55)`. The literal pixel `(46,359)` comes from the current cell-center
projection, flat authored terrain and the discovered camera `(-556,1066)` at
zoom 1. Check the actual queued Move destination and camera in each run before
using the retask observation. Its 110-step window observes attack and explicit
retask; it does not certify target destruction, pursuit or return-to-post timing.

The [idle SQD profiles](spatial_oracle/fv_cell_attack/profiles/README.md) use an
authored water map with unchanged retail types. A separate ignored real-device
SfxPlayer test exercises release/detach/hard-stop consumers; ordinary Stop on a
stationary SQD does not manufacture an idle-lapse trigger.

The [projectile trailer discovery profile](projectile_oracle/profiles/projectile-trailer-discovery.json)
loads an authored water scene with unchanged retail SUB and LCRF rules. Use it
to confirm stable actor IDs before the ordinary `Attack` in the
[trajectory profile](projectile_oracle/profiles/projectile-trailer.json).
Copy these profiles to scratch and replace `launch.selected_map_file` with the
absolute path of the tracked
[`projectile-trailer.mpr`](projectile_oracle/profiles/projectile-trailer.mpr)
in the checkout before running the capture command. Relative map names resolve
under the retail directory, not the capture checkout.
The [flight profile](projectile_oracle/profiles/projectile-trailer-flight.json)
records the first 40 steps, with a GPU readback of surviving bubbles after
impact. The longer trajectory profile follows their final cleanup.
Its [observer-off control](projectile_oracle/profiles/projectile-trailer-flight-no-effects.json)
omits only the effect fields, allowing final state and GPU bytes to be compared.
The trajectory and flight profiles follow the production `SubTorpedo` path and
filter `BBBLELRG`; neither injects a projectile or animation. Their endpoints bound a first-shot experiment;
inspect actual emission, retirement and cleanup frames before drawing a timing
conclusion. Per-frame owner snapshots and a rendered endpoint establish production
integration within that window, not original-engine pixel or whole-weapon parity.

Optional v2 `observe_super_weapons: true` adds a `super_weapons` list to each
observed House row: every Super the House holds, ordered by the interned type id
an ordinary `LaunchSuperWeapon` command names, with its grant, readiness, hold,
charge start, duration and remaining frames, and the Force Shield's fade countdown
(`SuperClass+0x50`, -1 when idle) and its coordinate. The rows count toward the sample
budget. `false` or an absent field adds nothing; version 1 rejects the option,
and present null/nonboolean values are invalid.

Optional v2 `observe_diplomacy: true` adds a `diplomacy` object to each observed
House row: `allies`, the other Houses its own ally mask names in House array
order, and `enemy`, the House its anger chose (null for none). It follows the
same presence rules as `observe_super_weapons`.

`render.frame_wall_mean_ms`, when present, reads the existing frame timer's last
up-to-60 intervals. It includes simulation, diagnostic observation, rendering and
presentation pacing. It is cadence metadata, not GPU duration or ordinary play
FPS, and does not participate in deterministic comparisons.

Build through the shared Cargo owner, then run from the checkout with an existing
`config.toml` pointing at retail assets; the capture always renders at native
resolution. All supplied paths must be absolute without symlink
ancestors; use canonical temporary paths on macOS. The output parent must exist,
and the output directory itself must not exist.

```sh
python -m tools.cargo_run -- build -p vera20k --profile fast --bin vera20k
env -u RA2_DIR python -m tools.map_observation \
  --profile /absolute/checkout/tools/map_observation.example.json \
  --contract /absolute/checkout/src/app/diagnostics/tactical_capture/contract.v2.json \
  --cwd /absolute/checkout \
  --output /absolute/evidence/new-map-observation
```

Captures use the `fast` profile: it runs the same simulation as `--release` and
rebuilds faster after an edit. Keep timing on `--release`. The default executable is
the unchanged `fast` or release `vera20k` recorded for this checkout by
`tools.cargo_run`; it never guesses a target directory. When both are recorded, each
may come from a different revision, so the capture refuses until `--build-profile
fast` or `--build-profile release` chooses one. Recorded byte identity does not
establish source freshness: build the intended revision first.
`--executable /absolute/path/to/vera20k` selects an explicit binary instead.
For preserved builds, `--build-label LABEL` asks the shared Cargo owner for that
label's verified host `vera20k`, in the one profile the label was built with.
`--executable` cannot be combined with `--build-label` or `--build-profile`.
It checks the artifact path, target/profile classification, ambiguity and actual
executable SHA, with no fallback to the latest build. For example:

```sh
python -m tools.cargo_run --label before-map-change -- build -p vera20k --profile fast --bin vera20k
python -m tools.cargo_run --resolve vera20k --profile fast --from-label before-map-change
env -u RA2_DIR python -m tools.map_observation \
  --build-label before-map-change \
  --profile /absolute/checkout/tools/map_observation.example.json \
  --contract /absolute/checkout/src/app/diagnostics/tactical_capture/contract.v2.json \
  --cwd /absolute/checkout \
  --output /absolute/evidence/before-map-change
```

The supplied contract must match the repository contract bytes. Every environment
variable in its denylist must be absent, even if set to an empty string. The
wrapper reports denied variables and does not silently change the environment.
After sourcing the native development environment, explicitly remove `RA2_DIR`
for this command as above; asset loading uses the working directory's config.

On Linux, winit's text-free `Ime::Disabled` startup notification is recognized
as lifecycle state while the capture window is observably hidden and unfocused.
Keyboard, pointer and other IME events still reach the input guards.

The [factory tank exit profile](map_observation.factory-tank-exit.example.json)
exercises the ordinary human GAWEAP → MTNK production path on retail AnyTown
(`XMP03T4.MAP`). It deploys the starting MCV, builds power, barracks and refinery,
places the 5×3 factory at `(25,91)`, then queues one tank without a rally order.
Its numeric type/entity handles belong to the recorded retail roster and seed;
inspect a zero-step observation before reusing it with different rules or assets.
Use `RUST_LOG=info` with the command above to retain placement foundations in
`logs/ra2.log`. The profile requires 5600 steps to observe the complete exit.

The [Jumpjet instance discovery profile](map_observation.jumpjet-instance.example.json)
extends the AnyTown CMIN profile with that same GAWEAP production prefix and
placement, then queues stock SHAD type210 at step4600. It preserves seed305419896,
input delay2 and the original6000-step budget; the900-second timeout comes from
the factory profile. Its type filter records SHAD, GAWEAP and their prerequisites
alongside CMIN. Factory placement/timing came from the production capture below;
SHAD admission, spawn time, stable ID and current-candidate placement still require
an actual run. This profile is a discovery pass, with no guessed Move/Stop handle.
After inspecting its actual SHAD row, a second profile can append ordinary Move
and Stop commands using that literal stable ID and suitable later frame boundaries,
while preserving the discovery pass's seed, launch and production prefix. The
protocol accepts ordinary command IDs; `observe_types` only filters observations.

Current actor rows include `jumpjet`, null unless an active installed Jumpjet
runtime exists. Its retained destination XYZ, moving byte, phase, landing latch,
linked parameters and flight/facing values come from read-only value getters.
`params` preserves binary32 bits and `flight` preserves binary64 bits rather than
converting them to JSON floats. Foot rows also include `air`: independent raw
`tracker_cell_560` and `slot_cell_564`, spatial bucket/enter order, and raw slot
holder IDs at the actor's current cell, tracker cache and slot cache. NativeNull
is explicitly `[0,0]`; neither cache is reconstructed from location or membership.
These projections send no gameplay callbacks or slot/tracker writes. They are
additive v6 extensions, so sealed older receipts remain readable unchanged.

The 2026-10-02 release observation loaded MIX entry `-854728974` with map payload
SHA-256 `7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`.
Factory1456 placed at step4501, became operational4551, and delivered tank1528
at5249. Unload states1/2/3/4 occurred at5250/5265/5293/5325. The tank reached the
forced track head at5321, cleared the factory footprint at5333, and returned to
Guard at5340 in Cell `(30,93)` / XYZ `[7808,23936,416]`; the factory returned to
Guard at5370. The inspected 800×600 BGRA readback has SHA-256
`adceea6c241b3e4eb16eb7039d6c1159dfdb376f4d9191bfcc4b3634127c80cb`.
The integrated release binary SHA-256 is
`ec8dfce838c10c8eb86e5107d5643c2e907584c435303d0e5e6f0995cfccd823`
(31,504,832 bytes). Its retained `capture.json` and `run.json` SHA-256 values are
`f51df731f516a72f28ab2a1bb3ec2687a9f2e95a54edc5455e1cd0e1ea270394`
and `aab418567732c72350abebc572bfd898a0bab4b27c9725dbeb2a1b5986a05bf1`.
The factory and tank pose, lifecycle, mission and NavCom trajectories match
the pre-integration release
`079f26fb33e3f97ab5571c9350bcdc619384c03116a98ebfee1be8aab07a15d2`
at every recorded step; both releases produce the final readback above.
After integrating main `03750cc7` (shared refinery/depot docking), the final
release binary SHA-256 is
`30511790b949abba1b62dd596d9cf505d8e90b0e6be5d8888f34056ea114a6b1`
(31,541,264 bytes). Its v6 bundle is valid at step5600; `capture.json` and
`run.json` have SHA-256 values
`cf0c3c24a1fc485160d3906f0a4f4b67fbca55eb2ce15891f9356560f6cf3262`
and `8fbe3b95517d3a4beedab78f65add32d0867e270f61df33a82ce2a2b18dcd405`.
All 5,601 frame boundaries match the earlier integrated release for the
factory/tank's recorded pose, lifecycle, health, mission, NavCom, archive,
target, Foot, Building and original Unit fields (11,202 actor comparisons),
and the final BGRA bytes are identical. The new v6 radio/miner observations
and incoming IFV weapon/turret fields were absent from that earlier schema;
they are excluded from this comparison. The final deterministic state hash is
`4786212690033122157`. The external
`unit-unlimbo-main1010-runtime-summary.json` retains the declared field lists,
input/artifact identities, comparison counts and endpoints.
The final zero/default ExitCoord candidate was observed on 2026-10-03 with
release binary SHA-256
`e88f7537f4b0aef9dcf4ebddb3cf8c94c56f460127116938bcb6cd641f365b43`
(31,512,752 bytes). The stock R6 bundle passes the ordinary v6 validator and
matches R5's retained initial/final state, map identity and complete observation
transcript; the final BGRA bytes remain identical. R5's executable path was
superseded by the shared release build, so the strict cross-run wrapper cannot
revalidate that historical binary. This comparison uses the retained capture
bytes and does not weaken that wrapper.

R7 uses the same release and production loader with a loose copy of the original
map bytes plus only `[GAWEAP] ExitCoord=0,0,0`. The existing asset reader confirms
that the scenario layer overrides stock `[512,256,0]` with `[0,0,0]`; the loader
reports the derived map SHA-256
`0dfbeaf28550ed9213e54ad4cca70b698488065e26e021770b263091cb7f4199`.
R7 also passes the ordinary v6 validator. Every factory1456 and product1528
actor field, including radio, miner and IFV fields, matches R6 at all 5,601
boundaries (11,202 presence/actor comparisons; 3,700 present actors), and the
final BGRA bytes are identical. The different map/rule inputs produce different
whole-state hashes; those hashes are not an equivalence assertion. The external
`unit-unlimbo-zero-final-runtime-summary-20261003.json` records these identities,
declared fields and comparison limits (SHA-256
`9d9e235d7f56d9948ffa1292fb11dc9390827115eca9a7faa007981a0feb649f`).
After incoming Grand Cannon/recoil integration and snapshot284 composition,
final R8 stock and R9 zero-map bundles both pass the ordinary v6 validator.
Release binary SHA-256 is
`d894cf0b771716e60d394c692c419b62e125920c1d0a3c12ab1c13e67c2a2f0f`
(31,640,672 bytes). Their complete factory/tank actor dictionaries match at
all 5,601 boundaries (11,202 presence/actor checks, 3,700 present), and their
final BGRA bytes retain the same `adceea6c…` identity above. The factory/tank
fields also match retained R6 after removing only incoming optional
`building.voxel_gun` from that historical schema comparison. R8/R9 retain
the stock/zero map identities and complete exit/Guard endpoints above; their
whole-state hashes remain distinct with different rule inputs. Final summary
SHA-256 is
`e503a82b7e63b1cb4c654f644a6359e41d606ed0c4708870efecea5039b1b43d`.
These are Rust production observations. Bounded original Door, Unload and Unit
comparisons are documented by the existing
[Unit Unlimbo evidence owner](spatial_oracle/anytown_damage/unit_unlimbo.md);
this profile does not establish native whole-object clock or pixel equivalence.

The [Spy Satellite audio profile](map_observation.spy-sat-audio.example.json)
uses stock Allied prerequisites and two power plants on shrouded XMP03T4.
Ordinary paid GASPYSAT placement, sale and replacement drive the existing House
aggregate and map owners; the later scene also allows combat loss. It watches
`SpyUplinkOn`/`SpyUplinkOff` through registered centered playback and natural
retirement. The sealed cursor stays over a sidebar gadget so whole-map reveal
does not change its feedback. Native component boundaries and excluded map
callbacks are documented by the [SpySat evidence owner](input_oracle/spy_sat_audio.md).

The [CLEG Cell destination profile](map_observation.cleg-cell-destination.example.json)
uses the same retail start, builds America's AMRADR and a Battle Lab, then produces
CLEG 1635. Two ordinary Cell orders share step 9500, with the second replacing the
first before Process. The optional type filter keeps its 10000-step capture within
the fixed sample budget. The [destination evidence owner](spatial_oracle/infantry_teleport_destination.md#production-integration-and-validation)
records the observed arrival, NavCom cleanup, Guard return and GPU/input identities.

A new wrapper v8 bundle contains sealed `profile.json`, `config.toml` and
`contract.json` copies, plus `stdout.log`, `stderr.log`, `run.json` and the child's
atomically published `child-output/{capture.json,frame.bgra}`. Runtime still reads
the supplied original paths; retaining copies does not redirect the game loader.
Original files and retained copies must remain unchanged during capture.
`run.json` records exact input hashes, command, child PID/status, timeout, receipt
validation and capture artifact identities. A valid observation requires unchanged
profile/config/executable/contract files, matching profile and contract receipts,
a v8 child manifest with resident UnitAtlas statistics, a checked presentation-clock
transcript and neutral-input evidence, zero initial tick/frame/time,
the requested final tick/frame and endpoint step
receipts, a loaded loose/MIX map digest, hidden unfocused rendering without input
violations, and correctly sized/hashed BGRA bytes. Simulation time must advance
for nonzero steps; its scheduling formula remains owned by Rust. Zero steps must
retain the initial fingerprint. Map hashes attest the bytes reported consumed by
the loader; this wrapper does not independently extract MIX entries or reimplement
the loader. Compare deterministic fingerprints separately from presentation pixels.

The v4 `render.presentation_clock` records actual consumed presentation times:

```json
{
  "policy": "map-exact-step-presentation-v1",
  "origin_ms": 0,
  "interval_ms": 22,
  "draws": [
    {"completed_steps": 1, "radar_ms": 22, "tooltip_ms": 22, "message_ms": 22}
  ]
}
```

For a positive budget N, exactly N game draws occur at completed steps 1 through
N. A zero-step observation has one draw at step 0, with all three times zero.
Every row must contain the exact radar tick, tooltip poll and wall-clock message
management times consumed by that draw; each must equal its completed step × 22.
The budget is bounded at 100000 steps. Unknown fields/policies, missing or extra
rows, reordered/repeated steps, wrong integer types and any time discrepancy fail
validation. The wrapper retains the validated transcript as
`capture.presentation_clock`; it does not reconstruct a missing transcript.

Version2/3 profiles may set `observe_tooltips: true` to add an exact `tooltip`
projection to every presentation-clock draw. Omitted or false preserves the
historical row shape; unsolicited or missing projections fail validation.
This passively reads the existing cursor/camera, input gates, plane coordinate,
Cell ground bit and active tooltip (ID, text, captured cursor and shown time).
It sends no input and does not arm, reveal or force a tooltip. Active text is
bounded to256 Unicode characters; integer coordinates retain their signed
native-width bounds. Ordinary monotone draws reject a shown time later than
their consumed tooltip clock. Load-segment captures retain the u64 field bound
without assuming presentation state resets with the simulation clock. The
projection shares the100000-draw budget and does not add simulation observation
samples.

Comparison requires matching clock policies and consumed draw times. Tooltip
projections are observed output: changes at any draw are reported as `MISMATCH`
under `presentation_clock.draws[index].tooltip`, even when the final frame is
unchanged. Malformed transcripts or different clock schedules remain `INVALID`.

The required v4 `render.neutral_input` has exactly
`{"static_default_cursor":true,"camera_input_idle":true}`. Rust verifies these
prerequisites and render readiness on every draw before publishing the final
receipt; the wrapper checks their declared types/values and retains them as
`capture.neutral_input`. This bounds the capture to neutral input and a static
default cursor. It does not establish arbitrary animated-cursor reproducibility.

This supplied clock is a diagnostic reproducibility policy, not native wall-time
cadence. Ordinary play and other capture profiles retain their existing wall
clocks. The clock does not change simulation scheduling or provide evidence for
native pixels, audio playback, menus or outcome timing. Full-frame comparison
remains exact: no radar masks, channel tolerances or skipped pixels are applied.

## Sidebar input and retained-order observation

The [retail opening profile](map_observation.sidebar-opening.example.json)
uses stock AnyTown and ordinary MCV deployment/production before clicking the GI.
Numeric command handles belong to that seeded launch; rediscover them with a
zero-step observation after population changes. Pass absolute profile/contract
paths to the wrapper.

A v2 profile may opt in to `observe_sidebar_steps: [0, 1, 100]`. Each listed
capture completed step records the local retained sidebar view: tabs, ordered
visible type/name/cost/queue rows, scroll position and hit rectangles. These
steps remain monotonic across quickload; a restored simulation tick does not
repeat an earlier sidebar sample or select a different requested row. At
rendered steps, the harness checks that the actual `GameRenderOutput.sidebar_view` matches the
retained projection. Step 0 in a positive-step run is explicitly retained-only.

Sidebar gestures resolve their coordinates from that current view and dispatch
ordinary mouse press/release edges through the production gadget router. An
omitted button preserves the historical left route:

```json
{
  "issue_after_step": 100,
  "gesture": {"kind": "sidebar", "target": {"kind": "tab", "tab": "vehicle"}}
}
```

Other targets are `{"kind":"cameo","type_id":"MTNK"}`,
`{"kind":"scroll_down"}`, `{"kind":"scroll_up"}`, `{"kind":"repair"}` and
`{"kind":"sell"}`. Repair/sell targets click their actual retained toggle
buttons, not a guessed keyboard shortcut. Their gesture receipts add `toggle`
before/after rectangles, disabled bits and active latches; historical strip and
frame receipt shapes remain unchanged. Cameos must already
be visible; scroll controls must be enabled. Each receipt records the target's
resolved coordinates and before/after views. Commands created by a cameo click
still use the ordinary synchronized command scheduler. This observes Rust
production integration; native ordering comparisons are owned by the
[sidebar oracle](sidebar_oracle/cameo_order.md), with their separate bounds.

## Literal mouse buttons

The v2/v3 `click` and cameo `sidebar` gestures accept an optional literal
`button`: `"left"` or `"right"`. Omission defaults to Left and keeps the
historical JSON and receipt shape. An explicit Left is also admitted. For example:

```json
{"issue_after_step": 100,
 "gesture": {"kind":"click","position":[236,225],"button":"right"}}
```

```json
{"issue_after_step": 101,
 "gesture": {"kind":"sidebar","target":{"kind":"cameo","type_id":"GAPOWR"},"button":"right"}}
```

Both buttons use the same cursor move and ordinary gadget/mouse edge driver.
The driver checks the corresponding held button, tactical capture or resolved
cameo gadget after press, then releases it and restores the sealed neutral
cursor before another step. It never writes a synthetic held state or sends a
semantic sidebar action. A Right receipt has `left_press_captured:false` and
required `right_press_captured:true`; other receipts omit the latter field.
Right cameos still resolve the current visible type and retained hit rectangle.

Profiles containing any Right use `map-local-right-gesture-v5`, including mixed
key, command-bar and Left inputs. They opt in to the existing `local_input`
projection and startup `keyboard_bindings` table described below, so pending
placement and immediate Main cursors can be read without a dummy key. This
does not add a state owner or change the capture schema version. Bare and
explicit Left-only profiles retain their existing policy/projections. Python
checks exact requested buttons, typed press receipts, policy and existing target
geometry; it does not infer cancellation or command admission from the button.

Right is admitted only on tactical clicks and cameo targets. Right tabs,
chrome controls, command-bar buttons and drags are rejected. Right dragging
requires the ordinary camera's per-frame polling; the complete between-step
gesture driver does not advance that poll. Mouse modifiers and held gestures
across steps are also outside this profile contract. Null, unknown and wrong-case
button values are rejected before launch.

## Minimap presses

A v2/v3 gesture `{"kind":"minimap","position":[740,120]}` sends an ordinary
left press/release at render-target pixels, then restores the sealed neutral
cursor. `{"kind":"minimap_object","entity_id":935}` instead resolves an
installed, clickable tracker pixel for an already observed identity. It checks
the existing reverse hit before pressing, rather than projecting a new dot.
Both forms require the production online radar surface to accept the press;
it neither installs radar state nor issues a camera command. Coordinates use
the render target bounds. Ordinary tactical clicks keep their stricter bounds.

Minimap profiles opt in to the existing local-input projection and binding table,
so each gesture retains camera position before and after, with any queued commands.
Its `minimap` receipt records resolved position, actual tracker identity (or null
on an empty hit), clamped Cell and world center from the same production query
used immediately by the mouse press.
The existing local-key gesture policy carries that projection even without a key.
A moving tracker hit follows its object's virtual+4C navigation coordinate before
the radar's signed clamp, Cell ground center and immediate camera update.

[`map_observation.radar-orders.example.json`](map_observation.radar-orders.example.json)
builds a Soviet radar and conscript on stock XMP03T4 through ordinary production.
It clicks the minimap before selection, selects the conscript, clicks a different
minimap position, then holds Z for two more clicks and releases it. Inspect the
gesture receipts for the camera-only first click, the selected unit's Move command
with an unchanged camera, and two PlanningRecord commands followed by the release
commit. The final planning path should retain two nodes with editing disabled.
These are Rust production-input observations; they do not establish whole-run
native equivalence.

## Literal keyboard gestures

A v2 profile can send a literal key press and release through the same in-game
keyboard edge owner used by `WindowEvent::KeyboardInput`:

```json
{
  "issue_after_step": 10,
  "gesture": {"kind": "key", "key": "N"}
}
```

Keys are one ASCII letter or digit, or `Escape`. They are not command names.
An optional unique `modifiers` list contains literal `Ctrl`, `Shift`, `Alt`,
and/or `Super` edges, for example `{"kind":"key","key":"P","modifiers":["Shift"]}`.
The shared modifier owner presses them before the key and releases them in
reverse order before rendering. The loaded `KEYBOARDMD.INI` bindings and registered
startup defaults resolve them in production. Each keyboard receipt reports the
production `encoded_key`, resolved registered `binding_command` (or `null`),
and observed held-key press/release state. The capture never dispatches a
semantic hotkey command directly. An input, including `Escape`, that opens a
menu leaves the capture's admitted unpaused in-game state and fails. This
profile does not observe paused Options interactions.
The gesture transcript also records the startup `keyboard_bindings` table in
registered command order: each INI command name and its loaded `first_key`, or
`null` when unbound. This uses the production binding owner rather than parsing
the retail file again. Individual key receipts resolve their actual binding,
including any additional binding that is not the command's first key.

Multiple key gestures with the same `issue_after_step` execute in profile order
before another simulation step. For example, two `N` rows then an `M` row can
observe immediate pending selection changes without a command-drain tick between
them. Every gesture releases its key before the next one; OS autorepeat and held
keys across frames are outside this profile's coverage. The sealed neutral cursor
and all ordinary idle-input checks still apply. A neutral sidebar location can
keep placement or repair cursors out of the tactical viewport during observations.

Profiles containing a key gesture use `map-local-gesture-v3` unless a command-bar
or Right gesture selects the newer policy described above. Their existing
per-frame and before/after input receipts add `local_input`: current camera
top-left and zoom, follow target, repair/sell modes, targeting kind and type,
`main_rng_cursor: [index_a, index_b]`, the selection-voice gate, and ordered
pending `UnitSelected` voice requests. The required cursor pair is read from
`Simulation::rng_views().main` without advancing or copying the RNG state. Its
before/after values can witness immediate Main stream draws during an input
gesture; they do not establish equality of the whole RNG state. These values
come from the existing input/sidebar/simulation/audio owners. Voice requests
are observed without draining the queue and do not prove audible playback.
The optional `building_placement` projection borrows
`MatchInputState::building_placement_view()`. Its `current` and `saved_retry`
are independent targets: each is `null` or `{type_id, product_id, issuer}`.
`product_id` is the actual stable object ID or `null` after pointer expiry;
`issuer` resolves the retained House identity through that observation's live
simulation interner. `current_preview_present` and `saved_preview_present`
report retained previews without recomputing them. An observed empty owner
has both targets `null` and both flags `false`. Historical omission means
unobserved state, not an empty transaction. A projection and each non-null
target count toward the retained-sample budget. Before/after literal quickload
receipts can observe a submitted backup and another current target immediately
across the production prepared-load commit, without waiting for a Place receiver.
The projection adds no placement writer, queue drain or save schema.
When the active owner retains a preview, `building_placement` can also contain
`current_preview`: `{origin, foundation, valid, in_build_area,
foundation_visible, reason, cells, wall_autofill_cells}`. Origin and foundation
are unsigned16 coordinate pairs; each cell is `{coord, valid, level, slope}`
with an unsigned16 coordinate pair, boolean admission, signed8 level and
unsigned8 slope. The three preview admission/gate fields are booleans. Reason
is `null` or the production label `Not ready for placement`, `Not a building`,
`Blocked terrain`, `Overlaps structure`, or `Outside build radius`.
`wall_autofill_cells` contains unsigned16 coordinate pairs. These are copied
owner fields: diagnostics neither recompute placement nor derive cell geometry.
Empty cell lists remain valid when no terrain is installed.
An included preview must be an object and requires
`current_preview_present=true`; `null` is not a serialized preview. Historical
omission remains valid even with that flag true. Each included preview charges
one sample plus every cell and wall coordinate toward the same retained-sample
budget, including copies in immediate gesture receipts and per-frame input.
The optional `selection_scope` projection reads the shared TypeSelect scope
owner: `mode` (`ordinary`, `combatant`, `type`, `health`, or `veterancy`),
`across_map`, and `last_outcome_key` (the retained HUD outcome's CSF key or
`null`). It also reads the category-navigation owner's ordered
`retained_navigation` IDs and independent `health_category` and
`veterancy_category` cursors. Each cursor starts at `-1`; `0..2` means
critical/heavily damaged/healthy for Health and elite/veteran/little experience
for Veterancy. Pointer expiry removes a retained entry; Deselect and Alive
changes alone retain it. These projections read the owners without advancing
navigation. The four navigation modes share the same map-scope byte.

New actor rows report `veterancy_raw_bits` (the unsigned 32-bit experience
accumulator representation), `veterancy_rank_cache` (`-1..2`) and
`elite_flash_frames` (`0..65535`) from the existing entity owner. These fields
distinguish live experience from the AI's sampled cache and promotion timer.
Historical omission supplies no inferred rank or timer value.
`hud_messages` reads actual
localized message text rows in insertion order through `MessageList.messages()`;
it neither posts nor expires a message and does not invent source keys for rows.
New keyboard captures include these projections. Older sealed receipts may omit
them; omission supplies no evidence about that state. One scope projection,
every retained navigation ID (including duplicates), each HUD row and each
voice request count toward the retained-sample budget.
Ordered `selected_ids` and
`selection_pending` continue to report the existing optimistic input selection,
separately from commands queued for the next simulation step.

Left-only mouse profiles retain their previous policies and receipt shapes. Keyboard
and sidebar/tactical gestures may share one profile; all its input observations
then include the extended local state. No save/load state replacement is added
by these observations. The admitted literal quicksave/quickload route above
can replace the world through its existing production owner.

## War Miner Attack return observation

[`map_observation.war-miner-attack.example.json`](map_observation.war-miner-attack.example.json)
uses stock Russia/Battle/AnyTown, ordinary MCV deployment and building production,
and the refinery's free War Miner. At step2500 it ForceAttacks a stationary
friendly conscript. Actor and terrain samples show Attack, combat target removal,
the idle return and later ore consumption. The conscript belongs to the local
house and remains stationary. The command bypasses UI click resolution,
using the existing synchronized command scheduler.

Numeric handles are tied to that launch and should be rechecked after population
changes. This observes production behavior; it does not certify native combat,
scheduler, RNG or rendering parity. See the [mission ownership notes](../src/sim/miner/README.md)
and [original executable packet](spatial_oracle/harvest_attack_return.md).

## Unloading miner and parked aircraft bodies

[`map_observation.war-miner-unload.example.json`](map_observation.war-miner-unload.example.json)
is the War Miner launch above without the attack, stopped at step3775 with the
camera on the refinery pad. In this launch the free War Miner's Unload mission
dumps from step3762 until Harvest resumes at step3794, so step3775 draws its
body from `UnloadingClass=HORV`.
[`map_observation.docked-aircraft.example.json`](map_observation.docked-aircraft.example.json)
uses stock France, builds GAPOWR, GAREFN, GAAIRC and one ORCA by ordinary
production and stops at step5400 with the Harrier parked on the Air Force Command.

Both frames show whether a body is drawn from the sprites its model is seeded with
(`draws_turret_parts` in [`voxel_frame_catalog.rs`](../src/rules/voxel_frame_catalog.rs)).
Release captures of `6dab3753` drew neither body, only its health pips. With the
turret split following the drawn model both are drawn: the simulation fingerprints
are equal and the frames differ only inside x287..349/y246..298 (miner) and
x295..335/y264..293 (Harrier). The same holds in flight (x296..339/y65..101): add
`{"Move": {"entity_id": 1489, "target_rx": 44, "target_ry": 98}}`
at step5200 and stop at step5300 with the camera on (41,95). These observe
production output; no gamemd frame was compared. Numeric handles are tied to
each launch.

The validation receipt (`tools/map_observation.unit-body-draw.validation.json`, in git at 1b1a8b43e) records
the source and executable identities, frame hashes, changed-pixel bounds and
test results. The step3745 frame immediately before dumping is byte-identical.
Original runs, raw frames, profiles, native disassembly and logs are retained
in the receipt's local evidence archive; executables remain with the shared
build owner. Siege Chopper deployment is tracked separately below; aircraft
shadows remain open.

## IFV passenger turret observation

[`map_observation.ifv-turret.example.json`](map_observation.ifv-turret.example.json)
uses ordinary America/Battle startup, retail rules and an authored map with four
IFVs, a Prism Tank, and GI, Engineer and Tesla Trooper passengers. The command
schedule boards at step 10, unloads at 160 and boards again at 300; it ends at 400.
The empty IFV is a control. Commands pass through the existing synchronized input
queue and gameplay admission; the observation adds no passenger or turret state.

Generate the exact map and a profile copy using the existing fixture owner:

```sh
python - /absolute/evidence/ifv-turret-switching-owned.map /absolute/evidence/ifv-profile.json <<'PYGEN'
from pathlib import Path
import json, re, sys
from tools.render_depth_fixture import build_fixture

map_path, profile_path = map(Path, sys.argv[1:])
assert all(path.is_absolute() and not path.exists() for path in (map_path, profile_path))
mission = build_fixture(walls_only=True).replace(
    'Name=Depth Continuation - Walls and Cliff', 'Name=IFV Turret Switching')
units = [('FV',46,46), ('FV',51,46), ('FV',46,51), ('FV',51,51), ('SREF',56,48)]
infantry = [('E1',50,46), ('ENGINEER',45,51), ('SHK',50,51)]
sections = {
    'Structures': '',
    'Units': '\n'.join(f'{i}=<human player>,{t},256,{x},{y},64,Guard,None,0,-1,0,-1,1,1'
                      for i,(t,x,y) in enumerate(units)),
    'Infantry': '\n'.join(f'{i}=<human player>,{t},256,{x},{y},2,Guard,64,None,0,-1,0,1,1'
                         for i,(t,x,y) in enumerate(infantry)),
}
for name, body in sections.items():
    mission, count = re.subn(
        r'(?ms)^\[' + re.escape(name) + r'\]\n.*?(?=^\[|\Z)',
        lambda match: f'[{name}]\n{body}\n\n', mission)
    assert count == 1, (name, count)
map_path.write_text(mission, encoding='ascii')
profile = json.loads(Path('tools/map_observation.ifv-turret.example.json').read_text())
profile['launch']['selected_map_file'] = str(map_path)
profile_path.write_text(json.dumps(profile, indent=2) + '\n')
PYGEN
```

The map SHA-256 is
`cd6b249de915ef5802368ce51cb2fd4958782f1b93c0b1e829fce0459817b4d6`.
It retains the fixture's terrain and walls and supplies no type/rules overrides.
Recorded unit IDs are IFVs 13–16 and SREF 17; passengers 18–20. Recheck L0 IDs
after changing a launch or fixture input. Shorter captures set `ticks` to the
chosen completed step and retain only commands with `issue_after_step < ticks`.

Unit observations expose the existing owner words `current_weapon_138` and
`current_turret_124` as paired signed integers. Historical rows without both
remain readable. They establish selected state; retained GPU frames separately
establish rendered output. Neither establishes native raster parity.

The validation receipt (`tools/map_observation.ifv-turret.validation.json`, in git at 1b1a8b43e) records the
native corpus, build/source identities, capture hashes and inspected images.
All three passengers board at step 12: GI selects weapon/turret `(2,1)`, Engineer
`(1,2)`, and Tesla Trooper `(6,3)`. Departure restores `(0,0)` at step 164;
Engineer and Tesla board again at 309 and GI at 322. The empty control remains
`(0,0)`. These timings are production observations, not native cadence goldens.

Retained GPU frames show the initial empty models at step 10, all four models
at 100, the reset models and departed passengers at 280, and restored passenger
models at 400. The empty frame is byte-identical to the preceding build. A second
full cycle matches all 401 observation rows, fingerprints and final BGRA bytes;
each shorter phase matches the corresponding full-cycle trajectory prefix.

The shared charge-model prerequisite is exercised by replacing the commands
with `ForceAttackCell { attacker_id: 17, target_rx: 60, target_ry: 48 }` at step
10 and `Stop { entity_id: 17 }` at 30, ending at 150. SREF selects index 0 at step
1, 3 at 13, 2 at 38, 1 at 63 and 0 at 88. Separate frames at steps 25 and 50 show
the selected models; their trajectories match the full charge run's prefixes.
This establishes model selection through the existing fire/rearm path, not
whole Prism weapon or combat parity.

At step 25, the preceding build still draws SREFTUR. Selecting SREFTUR3 changes
156 pixels, all inside the Prism Tank's rectangle; all previously exposed
command/actor/terrain observations match across the 26 retained boundaries.

The fixture's resident atlas grows from 5,568 to 8,832 sprites and from
23,394,168 to 34,689,699 R8Uint texel bytes to retain the six additional gun
models. Bodies and shadows remain shared. These are measured atlas resources,
not whole-process GPU memory or a 20,000-unit performance result.

The [native component comparison](spatial_oracle/ifv_turret_switching.md) covers
rules, selection, cargo-head reset, draw/load decisions and shared charge-index
arithmetic. Rust passenger regressions additionally cover Guardian GI, failed
exit, save/load and selected-weapon firing. Existing Temporal gunner reload
handover, malformed custom content and whole Prism combat remain outside this
bounded ordinary IFV cycle.


## Grand Cannon barrel observation

[`map_observation.grand-cannon.example.json`](map_observation.grand-cannon.example.json)
starts France/Battle with stock rules and assets on an
[authored clear-ground map](map_observation/examples/grand_cannon_barrel.map).
The fixture's Grand Cannon, Patriot control and power plants belong to the
computer opponent (`Computer`), because a skirmish never creates the local
House's authored Structures (`0x0044F8CB`); they carry no type overrides. The
cannon is actor 1. An ordinary `ForceAttackCell` command, issued as `Computer1`
at step 25, targets `(48,59)`; the profile ends at step 120. The launch swaps the
two colour indices, so the cannon keeps the colour it had when the observer
owned it. Against that observer-owned fixture on the build before native owner
names, this run takes the same target at step 26 and Attack at step 27, and its
final tactical pixels are identical; only the sidebar differs.

Run the profile through the capture command above, from the repository root.
For an external profile copy, set `launch.selected_map_file` to the absolute path
of the tracked map. The validation receipt (`tools/map_observation.grand-cannon.validation.json`, in git at 1b1a8b43e)
records exact inputs, binaries, native corpora, capture hashes and test results.
The same release build also loaded the unchanged retail-directory `Hills.mmx`
and completed 20 steps with a valid GPU readback. That load uses the idle profile,
selects `Hills.mmx`, and omits the fixture camera and terrain-cell requests.

The idle case removes commands and ends at step 20. The opposite-facing case
changes the target to `(48,37)` and ends at step 160. Shorter copies ending at
98 and 141 capture peak recoil and the return to rest. Their entire observation
trajectories match the corresponding prefixes of the 160-step run. That run's
second execution matches every observation, final simulation hash and BGRA byte.

The inspected Metal readbacks show the restored barrel at rest, aimed in both
directions and during recoil. The opposite-facing shot starts at step 95, reaches
travel 8 at step 98 and returns to exactly 0 at step 141: 46 admitted AI updates,
matching the stock native recoil history. `building.voxel_gun` exposes the
existing primary facing, elevation, HVA counter and immutable recoil state; it
does not supply gameplay state to the simulation.

Before/after captures at steps 20, 120 and 160 have equal final simulation hashes
and identical pre-existing command/actor/terrain observations at every recorded
boundary. This excludes only the newly added `voxel_gun` diagnostic, which the
old binary did not expose. Their changed pixels all lie within the cannon:
372, 446 and 368 pixels respectively. These are endpoint full-state hashes and
trajectory observations, not per-tick full-world hash comparisons.

The fixture's resident atlas grows from 2,304 to 2,400 sprites and from
14,089,600 to 14,319,788 R8Uint texel bytes. Recoil uses the existing rasterized
parts; the resident and last-build sprite counts stay at 2,400 throughout these
captures. These measurements exclude other caches and driver allocations.

The [building comparison](voxel_oracle/building_barrel.md) executes original
loader and draw instructions and the stock guns' rasters; the
[recoil comparison](voxel_oracle/recoil.md) executes readers, firing gates and
complete update histories. These bounded component comparisons and production
captures do not establish native pixel parity. Native cache-pool lifetime, custom
barrel overrides, upgrade-provided turrets and EMP cannon missions remain separate
mechanisms. Measured subpixel float rounding of a translated gun remains a visual
residual.

## V3 rocket pitch observation

[`map_observation.v3-rocket-pitch.example.json`](map_observation.v3-rocket-pitch.example.json)
starts Russia/Battle with stock rules and assets on an
[authored clear-ground map](map_observation/examples/v3_rocket_pitch.map): the
Siege Chopper fixture's terrain with a pre-placed V3 (actor 1) at (42,48)
facing east, and no type overrides. An ordinary `ForceAttackCell` at step 10
targets (54,48). Set `launch.selected_map_file` to the tracked map's absolute
path in an external profile copy; the loader resolves a relative path against
the retail root.

The observation rows show the V3ROCKET (actor 2) launched at step 31, tilting
on the rail through 95, climbing, at its apex (Z 1393) near 204, diving, and
gone after 253. Copies ending at 60, 150, 204 and 235 (`camera_cell` (48,47))
were captured with release binaries built from `e8502be0e` (SHA-256
`91dc808361fe4255136bd5c993660e4eb05d15b2577c0a4130d69edb90937a65`) and with
the Rocket draw arm (SHA-256
`08ae22351e32a102878c6dc82dbcc9ecb8f5a4526a8706726d45df559db3657d`), map
SHA-256 `8a1a33d9a2705b2f99252b2a94f1b16cffd804b1bafab6b58fa7e966229aa3c5`
(before its V3's owner became the native `<human player>`; the renamed map gives
the example profile the same actor rows and final frame).
Each pair ends with the same simulation hash. The changed pixels, 823, 854, 292
and 1035, all lie in one box around the missile at most 51 by 66 pixels; the
old binary draws it level, the new one along its pitch. A rerun of the 235-step
copy from the tracked map gives the same BGRA bytes
(`760c19390bf64dcda8d82e9392c06c76cb9e445faa30c04a3834b211eb407ecf`) and the
same actor rows at every step; its state hash differs from frame 0 on, because
the hash covers the map path.

[`rocket_oracle/draw_matrix.py`](rocket_oracle/draw_matrix.py) executes the
original Draw_Matrix and camera product; rasterization is the existing native
port. These captures are production observations, not native pixel parity.

## Computer nuclear missile observation

[`map_observation.ai-nuclear-missile.example.json`](map_observation.ai-nuclear-missile.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_nuclear_missile.map):
the nuclear-missile fixture's terrain with the computer's pre-placed NAMISL and two
NAPOWR. A skirmish never creates the local House's authored Structures (`0x0044F8CB`),
so the observer's base is its starting SMCV at waypoint 0, (41,63): the profile's one
command, `DeployMcv` at step 0 through an actor selector, deploys it into the NACNST at
(40,62), present from step 14. NACNST is the only observed type (the computer's own yard,
deployed from its MCV at (61,61), is observed too). The profile observes no Super rows:
every observed House's twelve rows per frame would spend the 100,000-sample budget long
before the 9000-frame charge. The loader looks a relative map name up in the retail
root: run a profile copy whose `launch.selected_map_file` is the tracked map's
absolute path.

The computer's Strategy tick fires its NukeSpecial at the observer's construction yard,
the enemy object AI_TryFireSW values most, which drops from 1000 to 358 health at step
9499, after the warhead's wait on NUKEBALL, while the computer's own yard keeps 1000.
These are Rust production observations: the chain's native comparisons are the
`tools.superweapon_oracle` `ai_*` rows, and no whole-run timing or pixel equivalence
with gamemd is claimed.

## Computer Psychic Dominator observation

[`map_observation.ai-psychic-dominator.example.json`](map_observation.ai-psychic-dominator.example.json)
starts America/Battle against a Yuri computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_psychic_dominator.map):
the Psychic Dominator fixture's terrain with the computer's pre-placed YAPPET at (46,42)
and two GAPOWR, observer MTNK at (43,53), (44,54) and (45,55) and a fourth at (52,62),
and no commands. The observer's starting MCV stays undeployed at its start (59,33). A
deploy on its own would empty the observer's base cell, as gamemd's does (see the
[Spy Plane observation](#computer-spy-plane-observation)), but AI_Fire_PsyDom
(`0x0050A150`) aims at Foot units and reads no base cell. MTNK is the only observed
type, and the profile observes no Super rows, for the sample budget. The loader looks a
relative map name up in the retail root: run a profile copy whose
`launch.selected_map_file` is the tracked map's absolute path.

Each grouped tank counts all three in its 38 cells, and the last-to-first scan keeps the
last of them, (45,55): at step 9074 the tanks at (44,54) and (45,55) in the 3x3 block
(`DominatorCaptureRange=1`) become Computer1's and hunt the tank at (43,53), which stays
the observer's, outside the block, as does the one at (52,62). These are Rust production
observations: the chain's native comparisons are the `tools.superweapon_oracle`
`ai_psydom` rows and the `tools.ai_strategy_oracle` `all_to_hunt` rows. The run never
reaches All_To_Hunt, and no whole-run timing or pixel equivalence with gamemd is claimed.

## Computer Spy Plane observation

[`map_observation.ai-spy-plane.example.json`](map_observation.ai-spy-plane.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_spy_plane.map): the
computer nuclear-missile fixture with the computer's NARADR in place of its NAMISL,
beside its two NAPOWR. The observer builds its base the native way: `DeployMcv` at step 0
deploys its starting SMCV at (59,33) into the NACNST at (58,32), present from step 14, and
`QueueProduction` at step 70 and `PlaceReadyBuilding` at step 500 place a NAPOWR at
(62,32) at step 501.

The power plant gives the observer its base cell. Building Unlimbo recalculates its
House's base centre (`0x00440D07`, `HouseClass::Recalc_Base_Center @ 0x004FD150`, which
stores the empty cell before averaging) before the building joins the House's building
list (`0x00441553`), and UnitClass::Deploy skips Set_Starting_Cell for a House a human
controls (`0x0073985B..0x00739862`). A human's deploy on its own therefore empties the
start cell the launch assigned until the House's next building counts the yard. This
order is native behavior established by reading the instructions, without a native run;
`sim::deploy_tests` holds VERA's regression test. A copy of this profile that only
deploys sends its spy plane to (31,30), and a copy of the
[Iron Curtain profile](#computer-iron-curtain-observation) that only deploys forms no
team by step 8000. Observing only `Computer1`, with SPYP and NACNST as the types, keeps
the Super rows within the sample budget. The loader looks a relative map name up in the
retail root: run a profile copy whose `launch.selected_map_file` is the tracked map's
absolute path.

The computer's SpyPlaneSpecial is granted on the first step (charge start 0, 3600 frames)
and is ready from step 3601. Its Strategy tick fires it at frame 3707 (the charge restarts
there), and one SPYP appears at step 3708 at (70,65). It flies toward (63,38),
AI_GroundRallyPoint's cell beside the observer's yard, turns to Overfly at step 3885 at
(64,40), flies on and is last seen at step 4104 at (56,9).
These are Rust production observations: the chain's native comparisons are the
`tools.superweapon_oracle` `spy_plane_launch`, `send_spy_planes`, `spyplane_missions`,
`aircraft_leave_map` and `ai_*` rows, and no whole-run timing or pixel equivalence with
gamemd is claimed.

## Computer paradrop observation

[`map_observation.ai-paradrop.example.json`](map_observation.ai-paradrop.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_paradrop.map): the
computer Spy Plane fixture with the computer's CAAIRP at (42,38) and its two NAPOWR at
(40,44) and (46,42). The observer deploys and builds as in the Spy Plane observation, its
NAPOWR at (62,32) giving it a base cell at its NACNST at (58,32). Observing only
`Computer1`, with PDPLANE and E2 as the types, keeps the Super rows within the sample
budget. Run a profile copy whose `launch.selected_map_file` is the tracked map's absolute
path.

The computer's ParaDropSpecial is granted on the first step (charge start 0, 3600 frames)
and is ready from step 3601. Its Strategy tick fires it at frame 3707 (the charge restarts
there) at (63,38), AI_GroundRallyPoint's cell beside the observer's yard
(`superweapon/ai_fire.rs`), and one PDPLANE carrying nine E2 (`SovParaDropNum=`) appears
at step 3708 at (70,65). It turns to Overfly at step 3877 at (64,41), drops the
conscripts between (63,41) and (62,36), each with Hunt queued, its house being a
computer's, queues Retreat at step 3923 at (62,35) and is last seen at step 4153 at
(84,50). The conscripts set off hunting from step 4381. These are Rust production
observations: the chain's native comparisons are the `tools.superweapon_oracle`
`paradrop_launch`, `send_paradrop_planes`, `paradrop_missions`, `drop_payload`,
`spawn_parachuted` and `ai_*` rows, and no whole-run timing or pixel equivalence with
gamemd is claimed.

## Computer Iron Curtain observation

[`map_observation.ai-iron-curtain.example.json`](map_observation.ai-iron-curtain.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_iron_curtain.map): the
computer's pre-placed NAIRON at (55,62), NAPOWR at (66,56) and (66,52), NAWEAP at (56,56)
and six HTNK at (50..52,58..59) near its start (62,62). The HTNK lines end `1,1`, the two
recruit flags (`Techno+0x421`/`+0x422`): with `0,0` no Autocreate team may take them. The
observer starts at (37,37), far from the computer's tanks, and builds as in the
[Spy Plane observation](#computer-spy-plane-observation): `DeployMcv` at step 0 gives it
the NACNST at (36,36), and `QueueProduction` at step 70 and `PlaceReadyBuilding` at step
500 the NAPOWR at (40,36) at step 501. The AI triggers' zone test reads the enemy's base
cell, which the deploy alone would leave empty, so no team forms without the power
plant. HTNK is the only observed type, and the profile observes no Super rows, for the
sample budget. The loader looks a relative map name up in the retail root: run a profile
copy whose `launch.selected_map_file` is the tracked map's absolute path.

The computer's IronCurtainSpecial is granted on the first step (charge start 0, 4500
frames) and is ready from step 4501, as a copy ending at step 7530 shows when it observes
only `Computer1`'s Super rows and an absent type. Of the two retail Easy Iron Curtain
triggers, which share weight 5000, this run takes Soviet Iron Curtain Easy (`0D2769BC-G`):
two HTNK (`2 Rhinos`) with the `Soviet Iron Curtain Factories` script, while the other four
stay home. The two leave at step 7180 for (53,54) (script action 53), and at frame 7418 the
house fires its Iron Curtain at (54,54), the team's centre (action 55; the charge restarts
there). From step 7420 the team attacks the observer's NACNST (actor 20), and it is still
attacking at the last step, 8000. Ten frames into the curtain each HTNK draws its tint
number from the Scenario stream (`TechnoClass::UpdateIronTint`). These are Rust
production observations: the chain's
native comparisons are the `tools.superweapon_oracle` `team_super_actions`,
`iron_curtain_launch`, `curtain_overrides` and `iron_tint` rows, and no whole-run timing
or pixel equivalence with gamemd is claimed.

## Computer Chronosphere observation

[`map_observation.ai-chronosphere.example.json`](map_observation.ai-chronosphere.example.json)
starts America/Battle against an America computer opponent (`Computer1`, Easy) on an
[authored map](map_observation/examples/ai_chronosphere.map) laid out as the Iron Curtain
one: the computer's GACSPH at (55,62), GAPOWR at (66,56) and (66,52), GAWEAP at (56,56)
and three recruitable MTNK at (50..52,58). The observer builds the warp's target the
native way: `DeployMcv` at step 0 deploys its starting AMCV at waypoint 0, (34,33), into
the GACNST at (32,33), present from step 14; `QueueProduction` at step 70 and
`PlaceReadyBuilding` at step 620 place the GAPOWR at (38,31) at step 621. A GI at (66,30)
keeps the observer in the game. MTNK is the only observed type, and the profile observes
no Super rows, for the sample budget.

The computer's three MTNK leave as a team at step 7181 and regroup at (47,47)..(48,49) by
step 7312 (action 54), but the house never fires its Chronosphere and Chrono Warp at them,
on this map or on the original authored-base one. That original fired on `ad3d20b1c`
(2026-10-13), the two MTNK in the 3x3 block appearing at (39,33), under the observer's
GAPOWR, at steps 7402 and 7403, but not on `d12e3a848` (2026-10-15) or later: a
regression not yet bisected. These are Rust production observations: the chains' native
comparisons are the `tools.superweapon_oracle` `team_super_actions` and `chrono_*` rows,
and no whole-run timing or pixel equivalence with gamemd is claimed.

## Computer Lightning Storm observation

[`map_observation.ai-lightning-storm.example.json`](map_observation.ai-lightning-storm.example.json)
starts Russia/Battle against an America computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_lightning_storm.map):
the computer-nuke fixture with the computer's pre-placed GAWEAT and two GAPOWR in place
of its NAMISL and NAPOWR, and `FreeRadar=yes`. As in the
[computer nuclear missile observation](#computer-nuclear-missile-observation), the
profile deploys the observer's starting SMCV at (41,63) at step 0 into the NACNST at
(40,62) and observes only NACNST, with no Super rows. Run a profile copy whose
`launch.selected_map_file` is the tracked map's absolute path.

The computer's Strategy tick fires its LightningStormSpecial over the observer's yard,
and strikes wear the yard down from 1000 health at step 9374 to 346 at step 9543. These
are Rust production observations: the chain's native comparisons are the
`tools.superweapon_oracle` `storm_*`, `radar_outage` and `ai_*` rows, and no whole-run
timing or pixel equivalence with gamemd is claimed.

## Computer V3 bombard observation

[`map_observation.ai-v3-bombard.example.json`](map_observation.ai-v3-bombard.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Normal) with stock
rules and assets on an [authored map](map_observation/examples/ai_v3_bombard.map) laid
out as the Iron Curtain one, with these buildings and units:

- the computer's NARADR at (55,62), NAPOWR at (66,56) and (66,52) and NAWEAP at (56,56);
- two V3 at (50,58) and (51,58) and two HTK at (52,58) and (50,59), all recruitable
  (`1,1`).

The observer builds the targets the native way: `DeployMcv` at step 0 deploys its
starting SMCV at waypoint 0, (34,33), into the NACNST at (32,33); `QueueProduction` and
`PlaceReadyBuilding` pairs at steps 70/500, 510/880 and 890/1260 add the NAPOWR at
(38,31), the NAHAND at (29,33) and the NALASR at (34,38), each the next one's
prerequisite. The map's `[AITriggerTypes]` raises the retail Soviet Bombard Medium
trigger (`0CA24D8C-G`, Normal only, the computer owning a NARADR) to the priority weight
5000 (`0x006F0C72`), so the first team pass with an enemy picks it. Its TeamType
(`0ACDDB6C-G`), TaskForce (2 V3, 2 HTK) and script (`0607B8FC-G`) are retail. V3,
V3ROCKET, NALASR and NACNST are the observed types. Run a profile copy whose
`launch.selected_map_file` is the tracked map's absolute path.

The NALASR stands at (34,38) from step 1261, but on current main the team never forms,
with this map or the original authored-base one, and the four units keep Guard to the
end. The original formed it on `592b804b` (2026-10-10) and not on `ad3d20b1c`
(2026-10-13): a regression not yet bisected. Before that regression the team pass at
frame 5175 created the team; the two V3 attacked the observer's NALASR from step 5807,
their rockets destroying it at 6259, and then its NACNST from 6262, the HTK shelling it
in between.

These are Rust production observations. The V3's native comparisons are
`tools/rocket_oracle`'s (flight, spawn manager, kamikaze tracker); the team's are
`tools/ai_team_oracle.py`'s. No whole-run timing or pixel equivalence with gamemd is
claimed.

## Computer Dreadnought bombard observation

[`map_observation.ai-dred-bombard.example.json`](map_observation.ai-dred-bombard.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Normal) with stock
rules and assets on an [authored map](map_observation/examples/ai_dred_bombard.map)
laid out as the Iron Curtain one. A lake of 234 `water09` cells (TEMPERATE tile 322, a
1x1 template) covers x 30..55, y 44..52. The map holds:

- the computer's NATECH at (55,62), NAPOWR at (66,56) and (66,52), NAWEAP at (56,56) and
  NAYARD at (52,48) in the lake;
- its DRED at (48,46), HYD at (46,45) and (46,50) and SQD at (44,48), recruitable.

The observer builds the target the native way: `DeployMcv` at step 0 deploys its
starting SMCV at waypoint 0, (34,33), into the NACNST at (32,33); `QueueProduction` and
`PlaceReadyBuilding` pairs at steps 70/500, 510/1850 and 1860/2550 add the NAPOWR at
(38,31), the NAREFN at (36,35) and the NAYARD at (31,45), at the lake's west end, which
needs the other two. The map's `[AITriggerTypes]` raises the retail Soviet Navy Bombard
trigger (`0C32D84C-G`, Normal and Hard, the computer owning a NATECH) to weight 5000. Its
TeamType (`0EC2038C-G`), TaskForce (DRED, 2 HYD, SQD) and script (`0A869C8C-G`: attack
factories, 49, attack anything) are retail.

Two native rules decide the layout:

- **The computer needs a NAYARD.** AI trigger eligibility needs a factory of the house
  for every TaskForce type (`0x00509610`).
- **The observer's factory must be the NAYARD on the same lake.** A team's attack quarry
  is its leader's `Greatest_Threat` flat walk (`Quarry_To_Threat @ 0x00645BB0` sets
  neither bit 0 nor bit 1). That walk passes the leader's zone (`0x006F8E44..0x006F8EC4`,
  `0x006F9D69`) to `Evaluate_Candidate`'s movement-zone gate
  (`0x006F7E32..0x006F7E9C`), which admits only targets in the leader's Water zone. A
  yard on land is out of reach.

DRED, DMISL and NAYARD are the observed types. Run a profile copy whose
`launch.selected_map_file` is the tracked map's absolute path.

The observer's NAYARD (actor 40) is placed at step 2551, and the team pass at frame 5175
creates the Navy Bombard team. The run then goes:

- **The attack order.** At step 5181 the Dreadnought (actor 1) attacks the observer's
  NAYARD from where it lies.
- **The volleys.** They launch at 5281/5301, 5481/5501, 5681/5701 and 5881/5901, each
  pair 20 frames apart. The pool refills 160 frames after each volley's second launch,
  80 for the KamikazeWait and 80 for `SpawnRegenRate=`; new DMISL appear at 5461, 5661,
  5861 and 6061.
- **The hits.** DMISL take the yard from 1458 to 1198 (step 5492), 933 (5512), 635
  (5691), 370 (5710) and 60 (5891), and destroy it at 5909. The HYD shell it in between.
- **The end.** The Dreadnought's target clears at 5909 and it returns to Guard at 5910.

The native comparisons are as for the V3 observation, with
`spawn_manager::oracle_tests` covering the volley; no whole-run equivalence is claimed.

## Human Guard Aegis distributed-fire observation

[`map_observation.aegis-distributed-fire.example.json`](map_observation.aegis-distributed-fire.example.json)
loads a human AEGIS on legal water at (40,48), initially on ordinary Guard with
no target, and two hostile stock ZEP at (44,44) and (45,44). It queues no commands.
The computer's ordinary mission/Jumpjet updates make the aircraft airborne; the
profile does not assign altitude, AirTracker membership, candidates or history.
It retains projectiles and both aircraft for 360 completed steps, with an
explicit short-game-off, no-bases, no-shroud session. No type, weapon, timer or
health rules are changed.

Materialize a scratch map and profile from the checkout. This reuses the
[Dreadnought fixture's water terrain](map_observation/examples/ai_dred_bombard.map)
without adding another terrain copy or map generator:

```sh
python - /absolute/evidence/aegis-distributed-fire.map /absolute/evidence/aegis-profile.json <<'PYGEN'
from pathlib import Path
import json, re, sys

map_path, profile_path = map(Path, sys.argv[1:])
assert all(path.is_absolute() and not path.exists() for path in (map_path, profile_path))
mission = Path('tools/map_observation/examples/ai_dred_bombard.map').read_text()
mission = mission.replace('Name=Computer Dreadnought Bombard Observation',
                          'Name=Human Guard Aegis Distributed Fire Observation')
mission = mission.replace('IgnoreGlobalAITriggers=no', 'IgnoreGlobalAITriggers=yes')
mission = mission.replace('HomeCell=49043', 'HomeCell=46043')
mission = mission.replace('AltHomeCell=49043', 'AltHomeCell=46043')
sections = {
    'Structures': '',
    'Infantry': '',
    'AITriggerTypes': '',
    'Waypoints': '0=48038\n1=62062',
    'Units': ('0=<human player>,AEGIS,256,40,48,64,Guard,None,0,-1,0,-1,1,1\n'
              '1=Computer,ZEP,256,44,44,64,Sleep,None,0,-1,0,-1,1,1\n'
              '2=Computer,ZEP,256,45,44,64,Sleep,None,0,-1,0,-1,1,1'),
}
for name, body in sections.items():
    mission, count = re.subn(
        r'(?ms)^\[' + re.escape(name) + r'\]\n.*?(?=^\[|\Z)',
        lambda match: f'[{name}]\n{body}\n\n', mission)
    assert count == 1, (name, count)
map_path.write_text(mission, encoding='ascii')
profile = json.loads(Path('tools/map_observation.aegis-distributed-fire.example.json').read_text())
profile['launch']['selected_map_file'] = str(map_path)
profile_path.write_text(json.dumps(profile, indent=2) + '\n')
PYGEN
```

Run that profile with the existing production capture command, then inspect the
sealed run:

```sh
python -m tools.map_observation inspect --run /absolute/evidence/aegis-run \
  --type AEGIS --type ZEP --max-rows 1000 \
  --field health --field physical_leptons --field target --field mission.effective \
  --field foot.air.spatial_bucket \
  --expect /absolute/checkout/tools/map_observation.aegis-distributed-fire.expectations.json \
  --output /absolute/evidence/aegis-inspection.json
```

The [expectations](map_observation.aegis-distributed-fire.expectations.json) bind
the two aircraft by their L0 cells, follow their stable identities, require
initial Guard/no target, airborne-index entry, damage to each stock 2000-health
ZEP and both remaining alive. The 2000 threshold is the stock RULESMD Strength,
not a native damage or timing golden; recheck it through the production reader
before adapting the fixture to other rules. Establish engagement of both aircraft
by inspecting their actual health changes alongside first appearances of
`effects.projectiles` with the AEGIS `source_id` and `weapon: "Medusa"`.
Post-Logic actor samples can retain a null target throughout: acquisition, firing
and target clear can occur inside one pass. A nonnull post-Logic target is
therefore not required by these expectations. Projectile rows retain physical
coordinates, but not their target identity; exact firing order/history and the
Bullet's retained target after the shooter's target clears remain focused
production Rust-test evidence. The capture alone does not establish target order.

The final GPU frame is the rendered output for step 360. To see missiles during
an earlier volley, copy the materialized profile with `ticks` set to an observed
missile-live step and capture again; check that its trajectory matches the full
run's prefix. Keep images and validation reports outside the repository and the
sealed child-output directory. An elite variant may change only the map AEGIS
veterancy field from 0 to 200 and use `MedusaE`; an explicit-Attack control must
use a separate profile. Neither control substitutes for the command-free Guard
witness. These are Rust production integration observations, not a native
encounter, native RGB comparison or whole-mechanism parity certification.

## Siege Chopper deployment observation

[`map_observation.siege-chopper.example.json`](map_observation.siege-chopper.example.json)
uses a normal Russia/Battle launch and an authored map containing one local SCHP.
It keeps stock rules and assets. The ordinary command schedule moves actor 1 to
(50,50) at step 10, issues `DeployMcv` at steps 200 and 500, then moves it back to
(48,48) at step 650. The legacy command name also carries simple deployment and
undeployment. The run ends at step 750. The cursor rests over the sidebar: from
step 240 the SCHP flies under the default cursor, and the capture rejects a
frame whose cursor shows Select feedback. An enqueue receipt establishes neither
admission nor completion; this route also bypasses self-click and deploy-key input.

Create the map and a profile copy from the checkout, using new absolute paths.
The existing fixture owner supplies the map encoding and clear Temperate terrain;
the edits below author map objects and Houses without changing rules or runtime
state. This is the portable recipe retained in the local evidence archive's
`captures/fixture-reproduction.md`.

```sh
python - /absolute/evidence/siege-chopper.map /absolute/evidence/siege-profile.json <<'PYGEN'
from pathlib import Path
import json, re, sys
from tools.render_depth_fixture import build_fixture, pack_lcw

map_path, profile_path = map(Path, sys.argv[1:])
assert all(path.is_absolute() and not path.exists() for path in (map_path, profile_path))
mission = build_fixture(walls_only=True)
mission = mission.replace('Name=Depth Continuation - Walls and Cliff',
                          'Name=Siege Chopper Deployment Observation')
mission = mission.replace('Americans', 'VERA-OBSERVER')
mission = mission.replace('Country=VERA-OBSERVER', 'Country=Russians')
sections = {
    'Structures': '',
    'Infantry': '',
    'Units': '0=<human player>,SCHP,256,48,48,64,Guard,None,0,-1,0,-1,1,1',
    'OverlayPack': pack_lcw(bytes([255]) * 262144),
    'OverlayDataPack': pack_lcw(bytes(262144)),
}
for name, body in sections.items():
    mission, count = re.subn(
        r'(?ms)^\[' + re.escape(name) + r'\]\n.*?(?=^\[|\Z)',
        lambda match: f'[{name}]\n{body}\n\n', mission)
    assert count == 1, (name, count)
map_path.write_text(mission, encoding='ascii')
profile = json.loads(Path('tools/map_observation.siege-chopper.example.json').read_text())
profile['launch']['selected_map_file'] = str(map_path)
profile_path.write_text(json.dumps(profile, indent=2) + '\n')
PYGEN
```

The recorded map SHA-256 is
`9ee2c2acd28a6845f8c352bdc320d2c62e0c98a5622e75f48f42d2cbbab2854d`.
Recheck L0 actor IDs after changing any launch or fixture input. The recorded
fixture has SCHP 1 at (48,48) and an opposing MCV far away; it does not induce
combat. Run the copied profile with the build, environment and path requirements
at the start of this document. Retain a full cycle first, then choose phase
captures from its observed trajectory. For a shorter capture, set `ticks` to the
chosen completed step and keep only commands whose `issue_after_step < ticks`.
Each run retains the final GPU frame; earlier trajectory rows alone are not images.

New actor rows include optional `unit` observations: raw deployment bytes
`deployed_6e0`, `deploying_6e1`, `undeploying_6e2`, the landing request
`landing_for_deploy_134`, owner `stage_f8`, persistent `body_counter_538`, and
`deploy_anim_130`. The last is null or contains the retained stable ID and a
nullable `live` body with type, frame and owner identity. A retained ID with no
live Anim stays explicit. Non-Unit rows use null. The wrapper checks types and
ranges, accepts older rows without this extension and preserves them unchanged;
it does not turn these values into gameplay or parity assertions.

The baseline ignores the two deploy inputs and still draws the flying SCHP at
step 400. The candidate release loads the fixture through the production app
and draws each phase below. All eight wrapper receipts are valid; each phase
trajectory matches the corresponding prefix of the full cycle.

| Phase to inspect | Actual completed step and retained GPU frame |
| --- | --- |
| Airborne SCHP before deployment | 190; SCHP visible at Z=500 |
| Landing requested by deployment | 230; SCHP visible at Z=230, landing request set |
| Forward SCHPDEPL with the unit body hidden | 300; attached animation frame 5 visible |
| Deployed SCHD body | 400; deployed flag set, animation reference cleared |
| Reverse SCHPDEPL with the unit body hidden | 550; attached reverse animation frame 5 visible |
| SCHP returned to flight | 640; SCHP visible at Z=500 before the later Move order |

The validation receipt (`tools/map_observation.siege-chopper.validation.json`, in git at 1b1a8b43e) records
source/build/map/profile identities, every retained frame hash and inspected
image, and the exact transition steps. Landing starts at 202, touchdown clears
the request at 253, forward animation starts at 254, and deployed state begins
at 345. Reverse animation starts at 502, undeployment assigns its nearby
destination at 584, and automatic flight restores Z=500 at 635. The final Move
reaches (48,48) by 750. A repeated full cycle matches all 751 observation rows,
initial/final fingerprints and final BGRA bytes exactly.

The first candidate exposed a missing atlas refresh: its simulation advanced
the transition but the selected palette frames were absent, making SCHPDEPL
invisible. The final build uses the existing atlas owner to service live Anim
palette demands on committed ticks. Against that failed rendering candidate,
all eight runs retain identical simulation observations and fingerprints; only
the forward/reverse frames change, each by 1,561 pixels inside the chopper's
bounds. Both candidates and the baseline remain in the local evidence archive.
These are production observations; no gamemd raster comparison is claimed.

The [native evidence](spatial_oracle/unit_simple_deploy.md) covers the original
transition and Stage bodies, Jumpjet handler branches, deployment admission,
body selection and selected-HVA frame arithmetic. Its
[fixture limits](spatial_oracle/unit_simple_deploy.meta.json) describe supplied
callbacks and the component timeline; they do not establish a full native flight
or world scheduler. Aircraft shadows, type-specific custom palettes,
viewer-dependent disguise palette selection, the forced Magnetron source and
EMP admission remain outside this stock observation. Full native animation
lifetime, audio playback and raster parity are also unclaimed.

## War Miner refinery-cycle observation

[`map_observation.refinery-docking.example.json`](map_observation.refinery-docking.example.json)
uses the same stock Russia/Battle/AnyTown launch and first five ordinary deployment
and production commands as the Attack profile. It issues no combat order and
advances 7200 steps so the free War Miner can fill, dock, deposit and harvest again.
The v3 profile names its command types (SMCV, NAPOWR, NAREFN), observes only
HARV and NAREFN actors and requests no terrain rows, to stay within the
retained-sample budget. It seals a static cursor at (720, 556), off every object.

Child/run v6 adds actor `miner` and `radio` projections and frame `houses` rows.
It preserves the existing optional `unit` deployment projection alongside them.
A miner row contains `cargo_bales`, `capacity_bales`, `unload_active` and
`harvesting`; other actors have null `miner`. Radio rows preserve the complete
contact-slot array, including null holes, and the optional `dock_entered_with`
stable ID on both the miner and refinery. House rows follow requested-owner order
and expose the existing economy's `credits`, `spent_credits` and `score`. A
missing House has null economy; the observer never creates a wallet. The score is
House+0x54E8, fed by refinery deposits (x5 per bale), kills and captures, rather
than spendable cash. These immutable reads never send
radio queries, advance timers or change cargo.

Capture manifests and sealed run receipts use compact JSON with the same fields
and a trailing newline.
The map observer and transactional publisher use the same encoder for the
128 MiB receipt limit; indentation previously made the complete 7,200-step
refinery observation exceed that limit. Readers also accept historical indented
receipts.

Use the existing runner with that profile:

```sh
env -u RA2_DIR python -m tools.map_observation \
  --build-label refinery-docking-reviewed-20261002 \
  --profile /absolute/checkout/tools/map_observation.refinery-docking.example.json \
  --contract /absolute/checkout/src/app/diagnostics/tactical_capture/contract.v2.json \
  --cwd /absolute/checkout \
  --output /absolute/evidence/refinery-docking
```

The release observation can show cargo draining alongside the actual House payout,
contact release and resumed harvesting. It is production integration evidence;
native scheduler, movement, audio and pixels require separate comparisons. Native
coverage is recorded in [`spatial_oracle/refinery_dock.md`](spatial_oracle/refinery_dock.md).

The refinery validation receipt (`tools/spatial_oracle/refinery_dock.validation.json`, in git at 1b1a8b43e)
records two observed deposits at steps 3778 and 5670, release and Harvest at
3794 and 5686, then new cargo at 4229 and 6154. Each deposit paid 1,000 credits
while spent credits stayed at 2,600. Both complete compact receipts are about
59 MB and retain all 7,201 observed boundaries within the 128 MiB limit.

## Repair-depot service observation

[The depot profile](map_observation.depot-repair.example.json) uses stock
Russia/Battle/AnyTown and ordinary Deploy, Build, Attack, Stop,
MegaMission Enter and SellBuilding commands. It damages one starting HTNK, spends
the initial 5900 credits on prerequisites, stops the produced HARV before it
harvests, orders repair, then sells the barracks to fund the retained occupant.
No administrative health, wallet or contact edits are used.

The final release observation on 2026-10-03 reconfirmed HTNK 1375,
NADEPT 1811, barracks 1536 and HARV 1754 before sealing the 6900-step profile.
The tank arrives at the pad at 5484 with 40 HP and zero credits. It retains
the contact through 607 unfunded frames until the sale refund at 6091.
There are 45 paid steps of 8 HP/2 credits: first 6126, second 6211, then 15-frame
intervals. Health reaches 400 and both contacts clear at 6856. Physical movement
starts 6869, the tank leaves the pad cell at 6876 and reaches (35,94) on Guard
by 6900. Final cash is 160, spent credits 5990 and harvested credits 0.

The capture and offline validator both returned `VALID`, retaining 99,286
samples and 63,016,494 run-receipt bytes.
[The native comparisons and coverage report](spatial_oracle/building_repair.depot_service.validation.md)
separately establish the service arithmetic, timers, radio/arrival and animation
dependencies. These measured runtime IDs/timings are Rust integration evidence;
they are not native whole-match or pixel goldens. Reconfirm IDs before adapting
the profile to different inputs.

## Allied depot waiters

[The Allied waiter profile](map_observation.depot-waiters.example.json) constructs
a stock GADEPT through ordinary Queue/Place and damages three starting MTNKs
through Attack/Stop before repair orders. It starts with 20,000 credits;
construction spends 6,100. One ordinary Move stages the third tank south of
the pad before repair. All three repair from40 to300 HP in33 paid steps/66 credits
each, release at6620/7250/7878 and leave the foundation by7903. The last tank is
outside on Guard with no NavCom, contact or pending entry by7908. Allied A/B/C/D
animations are observed and the final render is inspected. The incidental CMIN
income is retained; repair cost uses the spent-credit delta, not cash alone.

[The crowded profile](map_observation.depot-waiters-crowded.example.json) omits
that staging Move. Its second and third tanks initially select the same outside
parking Cell `(37,94)`. The third stops at `(37,93)` inside the marked foundation,
retains pending1985 through7242 and clears it7243 after the second tank releases.
It stays40 HP without a persistent contact. The capture does not expose the
intra-frame HELLO/CAN_LOAD/BREAK sequence. Both clean waiters also initially
share one goal `(34,92)`, then rest outside the footprint and both receive service;
shared goals alone therefore do not establish a queue bug.

Both profiles advance8,500 ordinary frames through the production retail loader.
Numeric handles belong to this saved roster/seed; rediscover them for changed
inputs. [Original executable comparisons](spatial_oracle/building_repair.depot_waiters.md)
separately reproduce marked-cell admission, parking, nearby ally stop and
foundation refusal. They do not certify the full native crowded travel path.

## Natural ore-spread observation

[`map_observation.ore-spread.example.json`](map_observation.ore-spread.example.json)
loads stock AnyTown in Battle mode and advances one ordinary simulation frame.
Its 213 terrain observations are initially empty cells next to raw-map ore,
selected from the same `XMP03T4.MAP` payload recorded by the production loader
(SHA256 `7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`).
Coordinates were read with the existing shrapnel-repair map-facts decoder; the
profile does not create ore, alter queues or provide gameplay results. The final
integrated release run witnessed 16 new ore cells at density 3. This is a
production integration observation, not a gamemd gameplay/pixel comparison.
Original executable queue/RNG/Mark comparisons and the final run identities are
in [`spatial_oracle/ore_queue.md`](spatial_oracle/ore_queue.md) and its
`validation receipt` (`tools/spatial_oracle/ore_queue.validation.json`, in git at 1b1a8b43e).

## Ordinary scheduled commands and actor trajectories

The [paid concrete-wall profile](map_observation.concrete-wall.example.json)
uses stock Battle/AnyTown, America and ordinary starting forces. It deploys
the MCV, pays for GAPOWR and GAPILE, then places two paid GAWALL endpoints
at `(33,87)` and `(33,90)`. The production placement owner supplies the two
intervening wall cells through its regular autofill. It moves the starting
MTNK to `(37,88)` and orders ForceAttackCell at `(33,88)`; the profile supplies
no ready product, overlay state, damage stage or random result. The defense
tab samples retain the normal queue view. Numeric command handles belong to
this launch; rediscover bindings with v3 named commands after changing it.

The [wall-sale profile](map_observation.wall-sale.example.json) reuses that
paid opening, attacks `(33,89)`, then enters Sell, clicks the actual wall and
exits with RMB. These three gestures share one step because observation draws
require a static cursor. Receipts retain sticky Sell mode after the left click
and the queued signed Cell-sale command; subsequent frames observe removal,
path reopening, attacker release and no refund. The audio recorder observes
the ordinary SellBuilding player. This unsuspended tank route does not prove
the distinct native Drive obstruction/Override prerequisite or native audio.

The original-executable [wall component](spatial_oracle/anytown_damage/navigation.md)
separately establishes three-stage AP damage, inline cardinal damage200,
connectivity and removal. Its decoded Overlay construction inputs and
independent RNG entry seeds are explicit component boundaries. The paid
profile observes the normal producer and its consumers; `VALID` alone does
not establish successful orders, native factory timing or whole-frame parity.

[`map_observation.engineer-repair.example.json`](map_observation.engineer-repair.example.json)
uses ordinary Battle/AnyTown orders to deploy the MCV, construct GAPOWR/GAPILE,
deliver an ENGINEER, damage the power plant by MTNK Attack, stop the tank,
enable paid repair and send the Engineer through CaptureBuilding. The
production receipt (`tools/spatial_oracle/engineer_repair.production.json`, in git at 1b1a8b43e) records the
1650-step release capture and exact repeat: all fingerprints, complete observed
trajectories and full Metal BGRA bytes MATCH. At frame1602, actual health changes
304→750, damaged slot1581 is replaced by healthy slot1742 and Engineer1557 is
absent after deferred cleanup. Separate frame1601 and1650 GPU images were inspected.
These are observed production timings/IDs, not native goldens. The profile's
numeric handles/IDs belong to this exact stock launch; discover them again when
changing launch inputs. Native fixtures and the physical retail input test cover
the health sample, House consumers and sound request that the map observer does
not expose. Native whole-clock, pixel and audible output remain outside this run.

For TIBTRE observation, use
[`map_observation.tibtre.example.json`](map_observation.tibtre.example.json).
It loads retail AnyTown (`XMP03T4.MAP`), advances 800 simulation steps and looks
at the two TIBTRE02 cells `(74,32)` and `(76,27)` plus their eight neighbors.
Requested terrain rows also retain `overlay: {id, density}` and
`terrain_object: {name, frame, active}` when those owners exist. A nonanimated
object has null frame/active. These are immutable reads of the live owners;
they do not inject ore or advance an animation. The validator accepts old rows
without these fields and requires both fields together on new rows. Native
cadence and admission comparisons are recorded in
[`spatial_oracle/tibtre.md`](spatial_oracle/tibtre.md).

Profile v1 remains accepted and retains its original JSON projection. It cannot
declare the following extension fields, even as empty arrays. Profile v2 adds
optional `commands`, `observe_owners`, `observe_types`, `camera_cell` and
`terrain_cells`; omitted
fields stay omitted in the sealed request, and explicit null is rejected.
The existing v1 example is unchanged. Start retail bridge discovery with
[`map_observation.bridge-response.example.json`](map_observation.bridge-response.example.json):
an accepted Battle launch on stock AnyTown (`XMP03T4.MAP`), with ordinary starting
forces, an allied Computer1 and hostile Computer2. It requests zero steps, observes
both AI Houses and centers the camera on the concrete low bridge near (87,53).
The physical span occupies x86..88/y51..57 at level4 with LOBRDB overlays;
it has no structural elevated deck stamp. Verify those facts in the L0 terrain
receipt before choosing orders. A high-deck response needs a separate map site.
Its disabled shroud is an explicit launch option for this diagnostic observation.
No actor IDs or gameplay state are granted by the profile.

For a long production run, `observe_types` can narrow actor discovery to 1..256
unique, nonempty names from the loaded rule registry, for example `["CLEG"]`.
Names are literal and case sensitive. Discovery requires both a requested owner
and a requested type; omitting the field keeps the existing owner-only behavior.
Once discovered, a stable ID remains observed through owner/type changes, and
its disappearance produces a missing-ID row. The transcript includes
`type_filter` only when requested. House and terrain observations are unchanged,
and filtering never changes the simulation or increases the sample budget.

For numeric profiles, first retain L0 to discover the actual generated AMCV, MTNK and E1 stable IDs and
positions. A short ordinary deployment probe then discovers the actual GACNST
created by the AI MCV. Seal the final profile with those IDs and observed timings:

```json
{
  "commands": [
    {"issue_after_step": 0, "owner": "Computer1",
     "payload": {"DeployMcv": {"entity_id": 120}}},
    {"issue_after_step": 40, "owner": "Computer1",
     "payload": {"Move": {"entity_id": 121, "target_rx": 87, "target_ry": 53}}},
    {"issue_after_step": 200, "owner": "Computer2",
     "payload": {"Attack": {"attacker_id": 130, "target_id": 140}}}
  ],
  "observe_owners": ["Computer1", "Computer2"],
  "camera_cell": [87, 53],
  "terrain_cells": [[87, 53]]
}
```

These IDs and timings illustrate syntax; obtain actual values from the production
probes. Command payloads are the existing Rust serde `Command`, with no separate
order translator. Supported orders are Move, Stop, Attack, Guard,
DeployMcv, ForceAttackCell, QueueProduction, PlaceReadyBuilding and
CaptureBuilding (the resolved Engineer repair/capture mission), ToggleRepair,
MegaMission Enter, UnloadPassengers and SellBuilding. MegaMission Enter is the
event a click into a depot, refinery, bunker or transport sends:
`{"MegaMission": {"entity_id": 1375, "mission": "Enter", "target": null,
"destination": {"Entity": 1811}}}`. SellBuilding uses `entity_id` and the production sale lifecycle; it can
restore funds for a repair experiment without writing a House wallet.
Rust rejects
ignored payload fields or argument
types. Python checks diagnostic structure and the exact typed request/receipt;
it does not duplicate the gameplay command parser or admissions.

Child/run v5 and later record `observations.rule_types` at L0. These are the actual
rules-owned Infantry, Unit, Aircraft and Structure list names, their categories,
and their existing `interned_id` numeric handles from the loaded simulation.
The observer never interns a name. Obtain the intended type's handle from that
same launch before sealing a production profile; handles are local to that
world's interner and must not be copied between different launches or fixtures.
The numeric `type_id` below illustrates serde syntax, not a stock handle:

```json
{
  "commands": [
    {"issue_after_step": 100, "owner": "Human1",
     "payload": {"QueueProduction": {"type_id": 41}}},
    {"issue_after_step": 500, "owner": "Human1",
     "payload": {"PlaceReadyBuilding": {"type_id": 41, "rx": 87, "ry": 53}}}
  ]
}
```

Use the actual loaded House name, discovered type handle, placement site and
observed ready timing. These orders use the ordinary production queue and
placement admission. Recording an enqueued command does not imply the House
could build it, had sufficient credits, had a ready object, or accepted the cell.

Rows must be nondecreasing by `issue_after_step`, retaining input order for ties,
and occur before the final step. An issue at step N calls the ordinary
`try_schedule_command` producer at simulation tick N, before advancing frame N+1.
The producer owns encoding and queuing; the diagnostic adds no input-delay offset.
Move carries an already resolved semantic destination, as synchronized/replay
orders do; it does not claim to reproduce the preceding cell-click resolver.
The receipt records enqueue success, not gameplay admission or completion.
Ordinary actor/House checks remain in the command drain; verify their actual effects
in the trajectory. Nonlocal-House envelopes are explicit diagnostic commands,
not authority for the local player's UI to control an AI opponent.

`observations.frames` contains L0 and every committed simulation frame. Each
requested House contributes all its represented entities, including inactive
objects. Rows retain stable IDs through capture and record disappeared IDs instead
of silently rebinding them. Actor rows contain physical lepton XYZ, cell, bridge
layer, health/lifecycle, raw Mission current/queued/suspended/effective/handler and
timer state, exact tagged target/ArchiveTarget/NavCom, Foot +688/+68D, Infantry
Doing and the existing virtual +4C coordinate owner's result. An unavailable
coordinate is explicit; observation does not invent one or initialize gameplay.
Foot fields are null for structures. Terrain rows read only allocated real cells,
report current tile/subtile, presentation tile, level/slope and bridge fields, and
report unallocated cells explicitly without stamping the shared Dummy.

V5 actor rows add `building`, null for Foot objects. Structures read their sole
body state, queued body request, construction control, complete serialized
StageClass, ready latch, ActuallyPlaced and the last sampled operational byte.
No diagnostic field advances a timer or calculates a replacement gameplay state.
`building.animation_slots` contains occupied slots in native slot order, with
their actual animation stable ID and the live animation's canonical type name,
interned type ID, native object ID, world coordinates, attachment, Logic membership
and complete retained runtime/timer. A stale occupied slot retains its ID with
`animation: null`; it is never silently omitted or replaced. These rows allow
construction completion and the following operational visit's allocation,
replacement and cleanup to be inspected through the normal match path.

Current animation runtime receipts include `temporal_paused` (Anim +11A),
separate from `paused` (Anim +19E), for both building slots and effect animations.
This additive v8 projection is optional when reading older sealed receipts:
omission stays unobserved and is never converted to `false`. When present, the
field must be a boolean, is retained unchanged and participates in comparison.
Other unknown runtime fields remain invalid.

The camera calls the existing presentation centering/clamp owner once at L0.
`render.camera` records its requested cell and actual final top-left/zoom. The
existing neutral-input and exact draw requirements remain in force. A terrain
receipt establishes CPU state; the separate retained BGRA frame establishes the
production rendered output. Neither is a native comparison.

Profiles are bounded to 1024 commands, 30 observed House names and 256 terrain
cells. Captures retain at most 100000 combined actor, missing-ID and terrain
samples, including occupied animation-slot samples since v5 and House rows since
v6. Child/run/report JSON
is limited to 128 MiB on read and publication;
the shared JSON owner keeps its 16 MiB default for other tools. Observe only the
Houses needed by the experiment and choose a bounded step budget. Comparison checks
the complete command/actor/terrain trajectory and camera as well as fingerprints
and pixels; large transcripts remain referenced in their sealed source bundles
rather than being copied twice into the comparison report.

For the response chain, attack Computer1's **deployed GACNST** with Computer2's
ordinary MTNK. The yard exercises Building ReceiveDamage's sourced positive-hit
prelude (`44227E..4422BC` -> `708080`); retail GACNST and AMCV both lack
`ToProtect=yes`. The separate generic Techno damage response uses that flag,
with retail protected miners as controls. Computer1's own E1/MTNK defenders are
the response candidates; an allied local House does not substitute for that
ownership gate. For this map site, verify an actual low-bridge defender and an
ordinary ground control, yard damage, Rescue/AreaGuard dispatch, archived victim,
movement and cleanup. Validate an elevated deck separately. Do not presume
deployment clearance, bridge arrival or random mission selection from this example.
The runtime profile establishes production integration, not native whole-world
equivalence. Visible reproduction uses the normal release shell and the same
Battle choices; this capture route remains hidden. `RA2_QUICKPLAY` has no ordinary
AI starting forces and is unsuitable for this response fixture. Campaign-map
sandboxes remain separate from campaign startup validation.

The shared `tools.child_process` owner bounds spawn/wait/cleanup and collects
finite output snapshots. On timeout it kills only its exact child. Existing
outputs are never overwritten. A failed child, failed manifest, missing artifact,
changed input or invalid receipt produces an invalid report when publication is
possible. Exit codes: `0` valid observation, `1` retained invalid observation,
`2` invalid inputs or publication failure. This tool never claims native parity.

Portable validation:

```sh
python -m unittest tools.tests.test_map_observation tools.tactical_certification.tests.test_core tools.tests.test_child_process
```

The saved validation receipt (`tools/map_observation.validation.json`, in git at 1b1a8b43e) records the checked
release binary and source snapshot, repeated MIX-map state, loose-map and zero-step
captures, missing-map diagnostics, and the explicit coverage limits. It is a run
summary, not a native oracle golden.

## Validate and compare saved observations

Use the same owner to recheck an existing run or compare before/after captures.
These commands read evidence and write one new JSON report outside the run
directories; they do not launch a game or change the captures.

```sh
python -m tools.map_observation validate \
  --run /absolute/evidence/before-map-change \
  --output /absolute/evidence/before-validation.json
python -m tools.map_observation compare \
  --before /absolute/evidence/before-map-change \
  --after /absolute/evidence/after-map-change \
  --output /absolute/evidence/comparison.json
```

Validation reads the fixed artifact paths under the supplied run, checks actual
profile/config/contract/frame/log bytes and child receipt semantics, then
cross-checks the wrapper's copied evidence and original input identities. It does
not trust a stored `VALID` verdict or matching digest strings. The executable must
still exist at its recorded original path and match its recorded length and SHA;
a labeled preserved build makes that requirement durable. The report marks this
as `EXTERNALLY_REVALIDATED`. It cannot validate a deleted or replaced executable
from its old receipt alone.

A new wrapper v8 run validates its `SEALED_COPY` inputs without requiring the
original profile, config or contract files to remain available. It validates the
retained contract's v2 rules without requiring today's checkout to have identical
contract bytes. Offline checking does not apply the current process environment
denylist because it does not launch a child.

Both runs must validate before comparison. Profile, config and contract **bytes**
must match, including formatting; differing inputs make the comparison `INVALID`.
Executable bytes may intentionally differ, and both verified identities appear in
the report. Comparison checks complete initial/final fingerprints, map source,
exact steps, presentation-clock policy and consumed schedule, resident atlas
statistics, surface format and actual BGRA bytes. Clock policies must match; a
legacy wall-clock observation and a diagnostic-clock observation are `INVALID`
together even when their pixels match.
Differences name precise field paths and before/after values. There are no pixel
tolerances or omitted atlas fields.
Current v8 pairs also compare the complete command, rule-handle inventory,
actor/building/animation/unit/miner/radio/House/terrain transcript and camera,
including the delayed-shot and color/fade observations when requested. Historical
v5 pairs retain their original building/animation and rule-handle transcript,
including optional `unit` deployment observations when recorded. V4 pairs
compare their original command and actor/terrain transcript and camera, while
v3 pairs retain their original comparison fields.

`MATCH` means these checked observations are exactly equal for the compared
fields; it neither establishes independent execution nor certifies native parity.
`MISMATCH` means valid, comparable observations differ. `INVALID` means an input,
artifact, identity or receipt check failed. `native_comparator` and
`parity_certification` remain `NONE`. Compare exits `0` for `MATCH`, `1` for
`MISMATCH`, and `2` for `INVALID` or publication failure; validate exits `0` for
`VALID` and `2` otherwise. Reports are never overwritten.

Run bundles currently must remain at their original absolute location: command
output and wrapper artifact identity paths are cross-checked against the supplied
run. A copied or relocated bundle is rejected, and validation never follows its
stored artifact paths to read some other bundle. Same-directory and symlink-alias
comparisons are rejected. Keep reports outside both evidence directories.

### Historical captures

Offline `validate` and `compare` reject old wall-clock observations by default.
`--allow-legacy-clock` permits child v2 with sealed wrapper v2, preserving its
original `run.capture` projection. Validation identifies its clock separately as
`legacy-wall-clock`; it does not invent a diagnostic transcript or neutral-input
guarantee. Live capture accepts only child v8 and never offers this override.
Wrapper and child generations must correspond: v8/v8, v7/v7, v6/v6, v5/v5,
v4/v4, v3/v3, v2/v2 or v1/v2. Current v8 uses ordinary observation policy v5.
Sealed v7 uses prior walk policy v4: all existing walk actor fields remain required,
while laser snapshots retain their original beam-only schema. V7 and v8 captures
remain individually readable but cannot be compared as equal observation policies;
missing delayed shots and beam color/fade fields are never invented.
Historical v5 remains readable with its original policy, production
commands, rule handles and building fields, including the optional `unit` extension
when present, without miner/radio/House projections. Older rows without `unit`
remain unchanged.
V5 and v6 comparisons are invalid because their observation policies differ.
Historical v4 remains readable with its original observation policy and profile
v2 trajectory, without production orders, rule-handle inventory or building
fields. V4 and v5/v6 comparisons are invalid because their observation policies
differ. No historical receipt is upgraded or supplied with missing state.
Sealed wrapper/child v3 remains readable without a clock override, with profile
v1 only and its original projection. It cannot declare v4 actor/command/camera
receipts. Comparisons between v3 and v4/v5/v6 are invalid because their observation
policies differ; no missing trajectory is reconstructed.

Wrapper v1 also retained only `profile.json`. It additionally requires
`--allow-legacy-inputs` to reread the original config and contract paths and check
their actual lengths/hashes against the old receipt. These inputs are reported as
`EXTERNALLY_REVALIDATED_UNSEALED`, never as retained copies. Missing or changed
originals fail validation; the profile copy and original executable are checked
as above. Neither legacy flag grants the other permission.

For a comparison between two old wall-clock captures, where one uses wrapper v1:

```sh
python -m tools.map_observation compare \
  --before /absolute/evidence/old-wrapper-capture \
  --after /absolute/evidence/sealed-wall-clock-capture \
  --allow-legacy-inputs --allow-legacy-clock \
  --output /absolute/evidence/historical-comparison.json
```

Same-policy legacy runs may compare when their actual input bytes match. A
legacy/v3, legacy/v4, legacy/v5 or legacy/v6 comparison remains `INVALID` with both flags,
including when frame
bytes are equal. Child v1 remains unsupported historical evidence: it lacks atlas
statistics. No command rewrites historical receipts or adds evidence they did not
record.

Focused portable checks, including malformed/tampered packages, input provenance,
exact mismatches, legacy policy and binary selection:

```sh
python -m unittest tools.tests.test_map_observation tools.tests.test_cargo_run
python -O -m unittest tools.tests.test_map_observation tools.tests.test_cargo_run
```

The checked comparison validation (`tools/map_comparison.validation.json`, in git at 1b1a8b43e) records
whole-suite checks, preserved-label selection, revalidated historical pairs and
new sealed production captures for this workflow. The earlier capture validation
receipt above remains historical evidence for its recorded source revision.

### Checked diagnostic-clock captures

The presentation-clock validation (`tools/map_presentation_clock.validation.json`, in git at 1b1a8b43e)
records fifteen independent release captures and eight exact full-frame
comparisons on Apple M4/Metal: three Allied 30-step runs, and two each at Allied
zero/200 steps, Soviet 30 steps, and message steps 1/182/183. All same-profile
pairs matched their complete BGRA bytes, simulation fingerprints, atlas evidence
and consumed clock transcript. The announcement was visible at steps 1 and 182
and absent at 183, preserving its current 4000 ms lifetime from its committed
step-1 timestamp. This is a production regression, not native trigger or message
timeout parity.

The receipt retains full profiles, fixture construction, binary/source identities
and commands. Reproduce the base map with the existing
`tools.render_depth_fixture.build_fixture(cliff_back=True)` owner; append the
receipt's literal trigger for the message fixture. Point each retained profile's
`launch.selected_map_file` at the corresponding new absolute map path, then run
and compare fresh output directories with the commands above. The trigger uses
the current announcement action without ending the scenario.

The original historical same-binary pair still reports `MISMATCH` with
`--allow-legacy-clock`: 11032 radar pixels differed. No pixels are excluded to
obtain the new matches, and historical receipts are preserved. New and legacy
policies intentionally cannot compare. The current 600-case native radar timer
check, retained-surface native check, four explicit retail GPU tests and one
retail radar-history test passed with their existing goldens unchanged.

Ordinary `radar-online-v2` live capture remains untested on this macOS host: its
sealed Windows Verdana path fails the existing POSIX absolute-path checks, and
the installed macOS font does not match its sealed bytes. The diagnostic captures
and native/GPU checks do not replace that production coverage.

## Resident unit-atlas measurement

The final rendered frame records `render.unit_atlas` in the v6 child manifest;
the validated wrapper retains it as `capture.unit_atlas` in `run.json`. The
`UnitAtlas` owner reads actual wgpu texture descriptors and resident entries.
It reports resident sprite count, the last actual build's rasterized sprite count,
and each page's extent, format, dimensions, mip/sample counts and texel payload
bytes, plus their total. Page count is the length of `pages`.

Pages are currently single-layer, single-mip, single-sample D2 `R8Uint`, so the
payload is width × height bytes, including unused page space. Unsupported
descriptors fail observation rather than retaining stale byte arithmetic. The
last-build count includes rasterized shadow sprites, can differ from admitted
resident entries, and stays unchanged on a no-op refresh.

This is resident UnitAtlas texture payload for the captured scenario. It excludes
CPU caches, `VxlPoseFrameCache`, other atlases, palettes,
driver overhead and peak allocation; it does not establish 30-player saturation.
Statistics are captured with the final render evidence before readback and kept
outside deterministic simulation fingerprints. GPU allocation may differ across
adapters even for identical simulation state.

This replaces the retired `measure-atlas` binary, which estimated a hardcoded
roster and tile sizes without constructing an atlas. The unused
`bridge-oracle-compare` binary was also retired: its trace schema had no repository
producer, and its five bin-only tests covered that abandoned comparison schema,
not active bridge behavior. Current native bridge comparison owners remain in
[`anytown_damage`](spatial_oracle/anytown_damage/README.md) and
[`shrapnel_damage`](spatial_oracle/shrapnel_damage/navigation.md); their native
outputs and Rust regression witnesses are preserved.

For a headless retail growth/no-op/fresh-pack comparison on a real GPU:

```sh
VERA20K_REQUIRE_RETAIL_INI=1 python -m tools.cargo_run -- test -p vera20k --lib --release \
  render::atlas_refresh_retail_tests::retail_atlas_refresh_costs -- --ignored --nocapture
```

Set `RA2_DIR` or provide the local config for that test. It emits the same owner's
statistics at each allocation stage; fresh and grown totals need not match.

The [atlas observation validation](atlas_observation.validation.json) records
before/after production captures and the explicit retail GPU atlas refresh test.
The older `map_observation.validation.json` (in git at 1b1a8b43e) remains historical v1 evidence; it
has not been retroactively given statistics or new source identities.

Final PR1018 integration includes main2e05101f and snapshot285. Stock R10 and
scenario-zero R11 ordinary v6 wrappers both finish5,600 ticks/exit0 using the
same release binary `9a3cbe2f2e1df8ea8763088bbfffbf694a30efc45193b1b0a4c33ce599e6104c`
(31,667,312 bytes). All11,202 complete factory/tank actor and presence comparisons
match across5,601 boundaries, with3,700 present samples and both objects ending
in Guard at the retained cleared-footprint poses. Final800x600 BGRA bytes remain
`adceea6c241b3e4eb16eb7039d6c1159dfdb376f4d9191bfcc4b3634127c80cb`;
the lossless preview was visually inspected outside the sealed bundles.
`unit-unlimbo-pr1018-final-runtime-summary-20261003.json` retains current wrapper
validation and complete comparisons, SHA-256
`92a0266bebb2298fd35d3821c06e23718c49cdd13b083608162c9c0e3222e8d0`.
The earlier R8 actor data also matches; its overwritten executable basename is
historical evidence and is not newly strict-validated. Stock/zero rule inputs
differ, so this actor/pixel comparison is not whole-run equality or native parity.

After main2909aba80 failure-diagnostic integration, candidate08005785's release
build retains exactly the recorded R10/R11 executable SHA-256
`9a3cbe2f2e1df8ea8763088bbfffbf694a30efc45193b1b0a4c33ce599e6104c`.
Both existing runs revalidate with ordinary v6 against those same current
bytes. This is same-executable evidence continuity, not a new execution.
External `pr1018-diagnostics-release-continuity-20261003.json` records the
identity and validations (SHA-256
`d6413cf7fc50e5e763aeacec6f34ef53e9994ad1dd03a4050b8d88854af07cfd`).
The previous bounded actor/GPU comparisons and native-parity limits remain.

### Occupied crate slots

Set `observe_crates: true` to read the existing MapClass crate-slot authority at each committed boundary. Each frame adds `crates` in ascending slot order, containing only occupied slots: `slot`, signed packed `cell`, raw timer `start_frame`/`aux`/`duration`, and current real-grid `overlay` (`id`, `data`, or null for a missing grid). Empty coordinates disappear; accepted ghosts remain visible to the observation even without an overlay. This projection never performs a gameplay map lookup or stamps the shared Dummy, places crates, or changes timers. It supports locating a naturally generated startup crate and observing ordinary movement pickup, replacement and cleanup. Each occupied slot counts toward the existing sample budget. Omit the flag or set it false to preserve existing frame fields.


### Starting tank and ordinary low concrete bridge

[map_observation.low-concrete-bridge.example.json](map_observation.low-concrete-bridge.example.json)
uses the stock Battle opening on XMP03T4 with the local player at start1. Its
starting MTNK1374 moves from (88,35) to the bank (87,50), then receives an ordinary
ForceAttackCell order for (87,54). No unit is spawned or relocated by the observer.
The camera covers the bridge and its three width cells at x86..88/y54.

The bridge's live identity changes from LOBRDB12 through LOBRDB16 to LOBRDB28.
Width data0/1/2 remains distinct. LOBRDB28 is a retained overlay with empty art,
not an erased cell. Bridge mutation mirrors live Terrain and OverlayGrid
synchronously; the retained render-index entries remain anchors, and their live
identity resolves at draw time. Generic placement upserts are not required for
every bridge transition. The profile records ordinary gameplay and rendering;
its shot count and timing are not native mission/RNG goldens.

### Paid Desolator deploy, undeploy and radiation light

[map_observation.radiation-light.example.json](map_observation.radiation-light.example.json)
starts Iraq on XMP03T4 and builds the ordinary power, barracks, refinery and
radar prerequisites before paying for DESO1575. The sidebar observation and
shared production test establish the enabled option before queueing; the
production event receiver alone does not repeat that prerequisite decision.
The unit exits the barracks and moves to
(33,88); selection and two command-bar Deploy gestures start and stop its
normal self-fire. The resulting field fades after undeployment.

[map_observation.radiation-snow.example.json](map_observation.radiation-snow.example.json)
uses the archived two-player XXmas.MAP in Battle mode. Its natural SMCV1207
builds the same paid prerequisites before DESO1374 exits the barracks and moves
to (37,86). The two ordinary command-bar gestures deploy and undeploy it.
The selected flat Snow cell uses the complete eight-file Clear01.sno family;
the source's Snow color branch is compared through the same lifecycle, lighting
and TMP owners as the Temperate case.

The cursor rests on the sidebar at (700,500), so selection does not leave an
animated world-order cursor in the final frame. The profile records ordinary
production and visible field cleanup. Its elapsed frames, object IDs and RNG
are captured Rust inputs, rather than native mission goldens. Native source,
Cell lighting and TMP comparisons live with the radiation oracle and tests.

