# Nominal building crew and active infantry counts

Replay with Unicorn2.1.4 and a locally supplied executable:

```powershell
$env:VERA20K_GAMEMD_EXE = 'C:/your-install/gamemd.exe'
python -m tools.spatial_oracle.house_tracking --nominal-survivor --check
```

`--write` regenerates only `nominal_building_survivor.json` and its sidecar.
The historical default tracking generator/corpus and the existing whole
`building_sale` controls remain unchanged. This mode extends the existing
`house_tracking.Fixture`; physical INI loading reuses
`rules_oracle.weapon_speed_order.retail_history`. It contains no second VM,
parser, CounterClass implementation or private game data.

The three sections are intended for focused production-reader/runtime tests:

- `reader_history` executes constructor's `Nominal` byte store7113F0, then
  actual physical INI loading and current-default ReadBool713F2C..713F44.
  Its eight supplied sources cover setting, missing/empty/invalid retention,
  exact-case key lookup, missing section and physical whitespace trimming.
  Constructor register EBX=0 is an established prior; a whole TechnoType
  constructor, registry allocation and startup/language-digest admission do
  not run. The physical strings and their sequence are explicit in each row.
- `producer_rows` execute the original death mark443111..44312E or selling
  continuation44A72D..44A74E. Both start with the original Infantry constructor
  byte store517A98. Death tests Building+6E9 and crew Type+C9E before Unlimbo;
  selling tests Type+C9E after a successful return. At the original selling
  virtual call, the harness pauses at51DFF0, executes the active-addition
  component in the same VM on a separate stack frame, restores registers
  only, then resumes the native success/failure branch and mark. Memory
  decisions and counter outputs are never replaced. `nominal_at_unlimbo`
  distinguishes the two callers' timing.
- `count_histories` execute Added502C3C..502C7F and Removed50297D..5029B2,
  including real49FA00/49FA70 from entry through return. Valid supplied
  ArrayIndex7 is below capacity16, so resizing is outside coverage. Every
  step records actual per-type slot and Counter+10 total, native slot/total
  instruction visits and independent receiving Houses. Histories contrast
  ordinary/marked infantry, DontScore, repeated reveal/limbo, signed wrapping,
  and on-map/limbo capture. Capture executes original70158A/701757 limbo guards
  and701735 owner store, composing reached callbacks in the same VM. The
  intervening tracking/kill/sight work and whole ChangeOwner do not run.

Unlimbo admission0/1 is an explicit external input at the original function
entry. Its placement, visibility and lifecycle internals are excluded. Failed
producer rows stop before object deletion; their retained diagnostic object
is not a claim that failed crew survive in the game. These are producer/count
components, not whole death or selling parity. The existing `building_sale`
fixture remains the owner for whole selling, Scatter and first Walk controls.

The count slices start **after** force-value accounting. The three sentinel
values are preserved to demonstrate that a skip applies only to the active
counter slice; they do not execute or certify Cost_Of/force-value arithmetic.
Constructor Add/Remove_Tracking, infantry438/439 latches and KeepAlive remain
separate mechanisms. In particular, Removed has no survivor-flag test or
balance latch. Its native slot/total decrement can retain negative values.

Three independent RandomClass states are initialized by original65C6D0 with
seed31. Rows retain complete-state hashes and observed65C780/65C7E0 entries;
these bound unchanged RNG for the selected component calls. The selected
mark/counter bodies contain no timer writes or detach calls. Earlier crew
selection/constructor/placement draws and downstream death/cleanup are
outside this component. No native stream or renderer executes here; snapshot,
hash and shared UI-name integration need their own Rust composition checks.
