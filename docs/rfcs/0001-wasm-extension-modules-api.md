# RFC 0001 Appendix: Functions and Tables

[简体中文](0001-wasm-extension-modules-api.zh-CN.md)

Reference appendix to [RFC 0001](0001-wasm-extension-modules.md), version 0.2.0, revised in the same commit as the main text, which takes precedence. Rules are stated only in the main text. Version 1 contents are in [section 9](#9-version-1-inventory) and the tier columns.

## Contents

- [Conventions](#conventions)
- [1. Simulation ABI](#1-simulation-abi)
- [2. Client ABI](#2-client-abi)
- [3. SDK mappings](#3-sdk-mappings)
- [4. Event kinds](#4-event-kinds)
- [5. Action kinds](#5-action-kinds)
- [6. AI teams](#6-ai-teams)
- [7. Properties](#7-properties)
- [8. Name kinds, search scopes and other enums](#8-name-kinds-search-scopes-and-other-enums)
- [9. Version 1 inventory](#9-version-1-inventory)

## Conventions

### References

An `i64` below 2^53. Zero means “none”. Bits 48 through 52 encode the kind. The low 48 bits encode the payload. The SDK packs and unpacks references.

| Kind | Name | Payload | Expires |
| --- | --- | --- | --- |
| 0 | Simulation object handle | Creation serial (1 to 2^48 − 1). Units, buildings, projectiles, houses, teams, triggers, tags, modifiers and superweapons share one counter. In 0.1.0 units, buildings, projectiles, triggers and modifiers share it. A house is bit 47 over its house array index, and a superweapon is bit 46 over (house index << 16) \| `[SuperWeaponTypes]` index: houses exist before the first object, so serials for them would renumber every object of every game. Teams and tags have no handles yet. | When the object is removed. Houses and superweapons never expire. |
| 1 | Type reference | `name kind << 32 \| ID` (8.1). ID `0xFFFFFFFF` is the wildcard for this name kind, valid only in the `Types` scope of `find`. | Never |
| 2 | Cell reference | `x << 16 \| y`, in cells | Never |
| 3 | Waypoint reference | Waypoint number. Functions accepting cells resolve it to its cell or return `NotFound`. | Never |
| 4 | Global rules reference | 0 for the scenario and global rules (7.8) | Never |
| 5 | Client node handle | Local node serial, separate from the creation counter | When the node is removed or the match UI closes |
| 6 | Client global reference | 1 camera, 2 local settings, 3 cursor (7.10) | Never |

TypeScript uses `i64` aliases and Rust uses newtypes. `NO_HANDLE = 0`.

### IDs

`lookup(kind, name)` returns a stable `i32` ID, or `(−1, NotFound)` for no match. Passing −1 (`NO_ID`) returns `NotFound`. Other out-of-range IDs are invalid arguments. See [8.1](#81-name-kinds-lookup) for name kinds.

**Variable IDs** ([RFC 6e](0001-wasm-extension-modules.md#6e-variables))

| ID | Variable |
| --- | --- |
| 0..99 | Red Alert 2 local variables 0..99 |
| 100..149 | Red Alert 2 global variables 0..49 |
| From 1000 | Variables in `vera.vars`, in load order (mod first, map second), then declaration order |

Names are unique within the event, action, client action and property tables. Properties with one name share an ID, and value type and units follow the reference kind. Properties, events and actions keep stock numbers. New events and actions use the numbers written in their tables (from 1000; the drafts' 100000 + n is 1000 + n), appended only. Row order is not identity. Property IDs are listed in section 7.

### Values

Property values, variables and integer record payloads use `i64`. Interpret them by value type.

| Value type | Meaning |
| --- | --- |
| `int` | Integer. The simulation API uses no floating point. |
| `bool` | 0 or 1. Writing a nonzero value sets 1. |
| `fixed` | Raw `I16F16` (`SimFixed`). `65536` represents `1.0`. Used for multipliers. |
| `lepton` | Coordinate or distance. 1 cell = 256 leptons. |
| `frames` | Logic frames, 15 per game second |
| `enum` | Member of an enum in 8.3 |
| `mask` | Bitmask. See the property for individual bits. |
| `ref` | Reference |
| `id` | ID within a name kind |
| `string` | String ID. Read with `name_of(String, id, ...)` (the SDK returns a string directly). Write with `client_set_bytes`. |
| `bytes` | Bytes in the layout specified for the property. Write with `client_set_bytes`. |

### Strings and buffers

- Pass strings and byte buffers as a pointer into module memory and a length. Strings use UTF-8. Each interface specifies its byte layout. Names use ASCII case-insensitive comparison ([RFC 4](0001-wasm-extension-modules.md#4-determinism)).
- Count-returning functions write into a module buffer (`out`, `cap`) and write the number written through their trailing out pointer.
- `len` always counts bytes. For strings and bytes, `cap` counts bytes. For reference buffers (`out` in `find` and `client_find`), it counts `i64` entries. The written count uses the same unit. The output of `get_many` and `client_get_many` must hold at least `n × m` entries of type `i64`.
- `name_of`, `rules_get` and `client_storage_read` write the full length to `len_out` and write nothing to `out` if it exceeds `cap`.
- A paged call returns its status and writes `count` and `next` to a `FindResult` (fixed layouts). Start with `after=0`. Reuse the query on subsequent pages, taking only entries with sort key ≥ `after`. [8.2](#82-search-scopes-find) specifies creation serial, house or cell index, type ID, or stable modifier insertion serial. If the per-call scan limit (main text 15.4) ends the page, `next` is the first key not examined, and `count` may be below capacity or 0. On a full page, `next=last key+1` and the following page may be empty. Otherwise `next=0`. Only `next=0` marks the last page. A valid zero-capacity query writes `count=0`, `next=0` and returns `Ok` without scanning.

### Status

```rust
enum Status {
    Ok = 0,
    Gone = 1,         // Reference is 0 or the object no longer exists
    NotFound = 2,     // No matching name or ID, or missing waypoint
    Refused = 3,      // Owner refused: occupied cell, property currently unwritable, or quota full
    Unsupported = 4,  // Disabled in this match, unknown entry, or unavailable here (initializer or decision handler)
}
```

- Host imports return at most one value, because stable Rust and AssemblyScript cannot import functions with multiple results (Rust lowers a returned pair to a hidden result pointer). An import that reads a value or creates a handle returns `status: i32` and writes the value through a trailing out pointer into module memory, in little endian: an `i32` or `i64` as its signature states, or a `FindResult` for paged calls. Pure writes return status. `frame` and `client_frame` cannot fail and return the frame number directly. Success is `Ok`. Each interface defines its empty value on failure, and the host writes that empty value through the out pointer on any non-`Ok` status. An out pointer whose target lies outside module memory is an invalid argument. In this appendix, `(value, Status)` names the value written through the out pointer and the returned status.
- Multiple return values remain allowed inside modules ([RFC 3](0001-wasm-extension-modules.md#3-loading-and-validation)). Only host imports avoid them.
- The SDK can return the value alone or both value and status. Actions without a handle return status directly. Hot paths use scalars or caller-provided buffers.
- Out-of-range IDs or indices, negative counts or capacities, invalid enums (probes return 0 for unknown entries), wrong variable scope, invalid action parameters or filter clauses, and writes during `on_decide` disable the module.

### Random numbers

The RNG column uses these terms ([RFC 4](0001-wasm-extension-modules.md#4-determinism)).

| Value | Meaning |
| --- | --- |
| `module` | Module RNG stream (`random`, a derived substream in decision handlers) |
| `native` | Native code draws from the native stream in its usual manner |
| `none` | No draw |

### Entry annotations

**Tiers:** `v1` (freeze candidate for version 1, subject to [Process](0001-wasm-extension-modules.md#process) step 5), `preview` (available in preview without a compatibility guarantee) and `later` (not in version 1). For an entry outside version 1, `has_kind` reports 0 and use returns `Unsupported`. Table names are API names. Numbers are the identity.

### Fixed layouts

Little endian, with natural field alignment. Numbers in comments are byte offsets.

**Event record**. Read in place in an `on_events` batch.

```rust
struct EventRecord {      // 120 bytes
    key: i32,             // 0   Dispatch key for the module action or decision handler
    kind: i32,            // 4   Event kind (section 4)
    trigger: i64,         // 8   Trigger that fired, always set
    holder: i64,          // 16  Tag holder: object whose tag holds the trigger, bound object, or CellTag cell reference. Otherwise 0
    subject: i64,         // 24  Object to which the trigger is attached. May be 0 or already gone
    other: i64,           // 32  Attacker, target, transport, producer, etc. May be 0
    house: i64,           // 40  House involved (retained house for events 1, 24, 25, 26, 59). Defaults to the trigger's house for this firing
    projectile: i64,      // 48  May be 0
    modifier: i64,        // 56  May be 0
    cell: i64,            // 64  Cell reference where listed under "cell", otherwise 0
    value: i64,           // 72  Damage, weapon index, stack count, frame, etc.
    value2: i64,          // 80  Second integer where listed
    id: i32,              // 88  Type ID where listed (weapon, warhead, superweapon type), otherwise -1
    frame: i32,           // 92  Frame when event occurred
    x: i32,               // 96  Position of event in leptons where listed
    y: i32,               // 100
    z: i32,               // 104
    args: i32,            // 108 String ID (name kind String): argument text of a module action, entry name of IniEntry, value of IniKey. Otherwise 0
    target: i32,          // 112 Target of a module action (ActionTarget, in 5.1 order). 0 for other records
    reserved: i32,        // 116 0
}
```

Every record sets `key`, `kind`, `trigger`, `holder` and `frame`. Reference fields omitted in section 4 are 0 (except `house`, as above). Unlisted integer payload fields are −1. `args` and `target` are nonzero only where listed. `reserved` is always 0.

**Decision record**. Read and write in place in `on_decide` ([RFC 6f](0001-wasm-extension-modules.md#6f-decidable-events)).

```rust
struct DecideRecord {     // 160 bytes
    event: EventRecord,   // 0    As above
    native: i64,          // 120  Engine's own result
    previous: i64,        // 128  Previous link's result, or native value for first link
    result: i64,          // 136  Written by handler
    handled: i32,         // 144  Handler sets to 1. Zero preserves previous link's result
    area: i32,            // 148  Result area in module memory, or 0 if unused by this kind
    area_cap: i32,        // 152  Bytes available in result area
    previous_len: i32,    // 156  Size in bytes of previous link's result, already copied into area
}
```

**Filter clause**. Used in event definitions and queries ([RFC 6a](0001-wasm-extension-modules.md#6a-triggers-events-filter-clauses-and-actions)).

```rust
struct Clause {           // 16 bytes
    subject: u8,          // 0  0 Subject, 1 Other, 2 House (trigger's house), 3 Var. In queries Subject is the candidate. Other and House are invalid
    op: u8,               // 1  Op (8.3)
    scope: u8,            // 2  Var only: VarTarget (8.3). Queries allow only 0 Global and 1 Subject
    reserved: u8,         // 3  0
    prop: i32,            // 4  Property ID, or variable ID for Var
    value: i64,           // 8  Constant to compare against
}
```

**Query**. Input to `find`.

```rust
struct Query {            // 64 bytes followed by clause_count Clause records
    scope: i32,           // 0   Search scope (8.2)
    area: i32,            // 4   0 whole map, 1 cell and radius, 2 rectangle
    cell: i64,            // 8   Area 1 center or area 2 top left (cell reference)
    radius: i32,          // 16  Area 1 radius in leptons, area 2 width in cells
    height: i32,          // 20  Area 2 height in cells
    type: i64,            // 24  Type reference. Types requires a kind wildcard or concrete type (8.2). For other scopes 0 allows any type
    owner: i64,           // 32  House reference, 0 for any
    viewpoint: i64,       // 40  House for viewpoint-dependent properties (section 7), or an object: object viewpoint properties (Engage) use it as the viewpoint, house viewpoint properties use its owner. 0 for none
    after: i64,           // 48  Pagination: next returned by previous page, 0 for first page
    clause_count: i32,    // 56  at most 16
    reserved: i32,        // 60  0
}
```

**Trigger definition**. Input to `trigger_create`.

```rust
struct TriggerDef {       // 32 bytes, then event_count EventDef records (each followed by its clauses), then action_count ActionDef records
    house: i64,           // 0   0 takes the subject's current house each time the trigger fires, or the map's Neutral house if there is no subject
    repeat: i32,          // 8   TagRepeat (8.3)
    difficulty: i32,      // 12  Bitmask: bit 0 Easy, bit 1 Normal, bit 2 Hard. 0b111 means all, 0 is invalid
    flags: i32,           // 16  Bit 0 starts disabled, bit 1 allowWin (stock action 15), bit 2 transferOnCapture (TriggerType item 8)
    decide_key: i32,      // 20  Decision handler dispatch key, -1 for none. Any other value requires exactly one event with decision support
    event_count: i32,     // 24  At least one. All must hold (AND), as in stock
    action_count: i32,    // 28  Zero or more. At least one action or decision handler is required
}
struct EventDef {         // 32 bytes followed by clause_count Clause records
    kind: i32,            // 0   Event kind
    op: i32,              // 4   Polling events: Op comparing reading to value
    interval: i32,        // 8   Polling events: frames between checks
    id: i32,              // 12  Property, type, variable or TeamType ID argument, -1 for none
    value: i64,           // 16  Numeric argument (house index, waypoint, seconds, threshold, etc.)
    clause_count: i32,    // 24  at most 16
    reserved: i32,        // 28  0
}
struct ActionDef {        // 8 bytes followed by ActionParams
    kind: i32,            // 0   Action kind (section 5)
    target: i32,          // 4   ActionTarget (5.1)
}
```

**Action parameters**. Used by `action` and actions in triggers.

```rust
struct ActionParams {     // 72 bytes
    id: i32,              // 0   Type ID (weapon, warhead, superweapon, animation, unit type, sound, TeamType, etc.)
                          //     Property ID for SetProperty and Modify, variable ID for SetVar, dispatch key for Module
    arg: i32,             // 4   Small enum where listed: Blocked, ModifierOp, Mission, Outcome, etc.
    arg2: i32,            // 8   Modifier group where listed
    reserved: i32,        // 12  0
    value: i64,           // 16  Amount, damage, score, weapon slot, rank, ammo, value to write, etc.
    value2: i64,          // 24  Second value, e.g. frame count
    cell: i64,            // 32  Cell or waypoint reference, 0 for none
    house: i64,           // 40  House target, new house for SetOwner, house of created object, or implicit house for a stock action
                          //     0 means the trigger's house inside a trigger and is invalid in a direct call
    other: i64,           // 48  Second object: source, passenger, team, trigger, tag. Taken from the event inside a trigger (5.1)
    text: i32,            // 56  String argument (CSF entry, music, EVA name, country, log text). Copied by trigger_create
    text_len: i32,        // 60
    extra: i32,           // 64  Variable-length data where listed (module code only)
    extra_len: i32,       // 68
}
```

Neither `value` nor `value2` has a missing-value sentinel. Kinds that do not use them ignore them. Zero is a regular value.

**Client event record**. Read in place in a `client_on_events` batch.

```rust
struct ClientEventRecord { // 32 bytes
    kind: i32,             // 0   Client event kind (4.6)
    source: i32,           // 4   Notify: sender module's source, 0 mod or 1 map. Otherwise 0
    ref: i64,              // 8   Node for Click, Hover, Dismiss. Cell reference for MapClick. Otherwise 0
    id: i32,               // 16  Hotkey ID for Hotkey, notification ID for Notify, otherwise -1
    value: i32,            // 20  Hover: 1 enter, 0 leave. Click: List row number or CameoGrid item index, otherwise −1
                           //     MapClick: 0 left, 1 right
    data: i32,             // 24  Notify: data. SimEvent: one EventRecord. Both in the delivery area
    data_len: i32,         // 28
}
```

**Other layouts**

```rust
struct PropValue {        // 16 bytes: initial property for client_node_create
    prop: i32,            // 0  Property ID
    reserved: i32,        // 4  0
    value: i64,           // 8
}
struct FindResult {       // 16 bytes: written by find and client_find
    count: i32,           // 0  Entries written to out
    reserved: i32,        // 4  0
    next: i64,            // 8  Continuation position, 0 on the last page
}
struct Order {            // 24 bytes followed by count unit handles (i64): input to order and client_send_order
    kind: i32,            // 0   Command (8.3)
    count: i32,           // 4   Number of units following
    target: i64,          // 8   Object or cell reference, 0 for none
    value: i64,           // 16  Type reference where listed in the command table (8.3), otherwise 0
}
struct ProjectileSpec {   // 48 bytes: variable data for CreateProjectile and one entry in a Fire shot plan
    type: i32,            // 0   Projectile type ID
    weapon: i32,          // 4   Weapon ID supplying damage and warhead. -1: in a Fire plan the shot's own weapon and payload, in CreateProjectile none
    target: i64,          // 8   Object or cell reference. In a Fire plan, 0 takes this shot's target
                          //     In CreateProjectile, 0 takes the origin object's current Target (Refused if none)
    data: [u8; 16],       // 16  Initial module data (Data0 bytes 0..7, Data1 bytes 8..15), saved and hashed
    flags: i32,           // 32  Bit 0 set: absolute origin. When clear, origin is an offset from the starting point
    origin: [i32; 3],     // 36  x, y, z in leptons. The starting point for Fire is the native firing position
                          //     For CreateProjectile it is the center of the target object or target cell
}
struct PathSpec {         // 32 bytes followed by point_count points: Launch result area
    kind: i32,            // 0   PathKind (8.3)
    point_count: i32,     // 4   Number of points following
    speed: i64,           // 8   Speed along the path, fixed point (16 fractional bits), leptons per frame. <= 0 takes the projectile's current speed
    param: [i64; 2],      // 16  Kind parameters: parabola param[0] apex height, spiral param[0] radius and param[1] frames per turn (> 0)
}                         //     Each point is three i64 values x, y, z in leptons, 24 bytes total. 1 to 16 points, exactly 1 for parabola and spiral
struct ProjectileStep {   // 56 bytes: ProjectileFrame result area
    flags: i32,           // 0   Bit 0 detonate, bit 1 set velocity, bit 2 set position, bit 3 set module data
    reserved: i32,        // 4   0
    velocity: [i32; 3],   // 8   New x, y, z velocity, fixed point (16 fractional bits), leptons per frame
    reserved2: i32,       // 20  0
    position: [i32; 3],   // 24  New x, y, z position in leptons
    reserved3: i32,       // 36  0
    data: [u8; 16],       // 40  New module data (Data0 bytes 0..7, Data1 bytes 8..15)
}
struct MissionStepResult { // 24 bytes: MissionStep result area. The first link receives the mission body's outcome
    target: i64,          // 0   Target: object or cell reference, 0 for none
    destination: i64,     // 8   Destination: object or cell reference, 0 for none
    mission: i32,         // 16  Mission to queue (Mission), -1 for none
    delay: i32,           // 20  Frames until the mission body runs again (>= 1)
}
struct MoveStep {         // 16 bytes: MoveFrame result area
    flags: i32,           // 0   Bit 0 take the frame and move by displacement, bit 1 turn to facing, bit 2 stop native movement (null destination)
    facing: i32,          // 4   Body facing 0..255 (as the Facing property)
    displacement: [i32; 2], // 8 x, y in leptons, each clamped to 2,048 either way
}
struct PathCell {         // 8 bytes: one cell of a PathFound result area (up to 24)
    x: i32,               // 0   Cell x
    y: i32,               // 4   Cell y
}
```

The `out` buffer in `get_many` and `client_get_many` holds `n × m` row-major `i64` entries. Property `j` of reference `i` is at `out[i * m + j]`. A null or gone reference writes 0 across the row with status `Gone`. Each entry uses the empty value and status of the corresponding scalar read. Write the number of slots written (including empty values) to `count_out` and return the first non-`Ok` status in row-major order, or `Ok` if all succeeded. A valid empty batch returns `(0, Ok)`.

### Custom sections

The SDK emits packed little-endian sections without alignment padding. Each begins with a format version byte. Each name is a `u16` byte length followed by ASCII bytes, with ASCII case folding ([RFC 12](0001-wasm-extension-modules.md#12-versions-and-abi)). Reject the module at load time for an unknown format version, truncated content, trailing bytes, non-ASCII name or duplicate name within a section.

```rust
// vera.actions
format: u8,               // 1
count: u32,
[{ name_len: u16, name: [u8; name_len], key: i32 }; count]

// vera.notify
format: u8,               // 1
count: u32,
[{ name_len: u16, name: [u8; name_len] }; count]

// vera.vars (simulation modules only)
format: u8,               // 1
count: u32,
[{ name_len: u16, name: [u8; name_len], scope: u8, visibility: u8, default: i64 }; count]   // Scope, Visibility (8.3)

// vera.delivery
format: u8,               // 1
offset: u32,              // Delivery area offset in module memory
bytes: u32,               // Size of delivery area in bytes
```

INI actions and `lookup(ModuleAction, name)` dispatch through `vera.actions`. Check `vera.notify` for name hash collisions at load time. For `vera.vars.scope`, 0 means global, 1 house and 2 object. See 8.3 for visibility. Events, decisions and their result areas, messages and notifications share `vera.delivery`. A single message must fit within the delivery area.

### INI keys

See [RFC 6a](0001-wasm-extension-modules.md#6a-triggers-events-filter-clauses-and-actions) for rules and [RFC 15.4](0001-wasm-extension-modules.md#154-preview-values) for values.

| File and section | Key | Value | Default |
| --- | --- | --- | --- |
| Map `[Basic]` | `VeraMinVersion` | Minimum game version. Required on maps with modules. 0.1.0 refuses a map that names a newer version but does not refuse a module map without the key | No default |
| Map `[VeraWasm]` | `Api` | `major.minor` for both `vera:sim` and `vera:client`. 0.1.0 serves `1.0` | Required with modules |
| Map `[VeraWasm]` | `RulesSize`, `UISize` | Uncompressed byte length of the respective module | Omit if module absent |
| Map `[VeraWasm]` | `StorageId` | ASCII name | Map filename |
| Map `[VeraWasmRulesPack]`, `[VeraWasmUIPack]` | `1`, `2`, … | Standard base64, 70 characters per value (readers accept up to 127 bytes). Keys are consecutive decimals from 1 without leading zeros. Join by number, not file order. A repeated, missing or non-numeric key or an overlong value refuses the match. Decompress as chunked LZO in the `IsoMapPack5` format. | — |
| Mod `rulesmd.ini` `[VeraWasm]` | `Api` | `major.minor`, matching the mod module's import version. Read but not checked in 0.1.0 | Required with modules |
| Mod `rulesmd.ini` `[VeraWasm]` | `Budget`, `MapBudget` | Per-frame budgets for mod and map modules, respectively ([RFC 15.4](0001-wasm-extension-modules.md#154-preview-values)) | 500,000 and 100,000. Ceiling 500,000. Below 20,000 refuses the match. |
| Mod `rulesmd.ini` `[VeraWasm]` | `SeatBudget` | Budget added per claimed seat | 500,000. Ceiling 500,000. |
| Mod `rulesmd.ini` `[VeraWasm]` | `Controller` | `yes` makes the lobby offer a "mod AI" option for computer seats ([RFC 6i](0001-wasm-extension-modules.md#6i-driving-a-house)). The engine seats them from the launch settings; the lobby does not offer the option in 0.1.0 | `no` |
| `VERA.INI` `[VERA]` | `WasmBudget` | Overrides the mod's `Budget`. Online, the host's value becomes a room setting. | None |
| Mod `rulesmd.ini` `[VeraWasm]` | `MapWasm` | `yes`, `no` | `yes` |
| Mod `rulesmd.ini` `[VeraWasm]` | `MapUI` | `overlay`, `full`, `no` | `overlay` |
| Mod `rulesmd.ini`, TechnoType, Weapon, Warhead or Projectile section | `Wasm.Tags` | Comma-separated tag section names | None |
| Mod `rulesmd.ini` `[VeraVars]` | Variable name | `Scope,Visibility,Default` (names from 8.3). Not implemented in 0.1.0 | None |
| Map `[VeraUI]`, `[VeraUI.<part>]` | `Parts`, part keys | As added `veraui.ini` parts (7.10) | None |
| Map `[Actions]` | Action kind 1036 | `1036,4,name,0,0,0,0,A`. The name may have `mod.` or `map.`. Not implemented in 0.1.0 | — |
| Tag section | `Trigger` | Comma-separated trigger section names | — |
| Trigger section | `Event`, `Event<n>` | `name` or `name(op,interval,id,value)` (`EventDef` fields, trailing arguments optional). `Wasm:name` is a registered module event (`ModuleEvent`). All events must hold. | At least one |
| Trigger section | `Where<n>`, `Event<k>.Where<n>` | Filter clause ([RFC 6a](0001-wasm-extension-modules.md#6a-triggers-events-filter-clauses-and-actions)). `Where<n>` belongs to the first event. | None |
| Trigger section | `Action`, `Action<n>` | `Wasm:name` or `Wasm:name(target,text)` for a module action (the text becomes the record's `args` verbatim and may contain commas). The event in `RaiseEvent` is written by name. Otherwise `name(target,params…)`, with an `ActionTarget` name and parameters in section 5 table order. | None |
| Trigger section | `Decide` | `Wasm:name`, a decision handler | None |
| Trigger section | `House` | Country name | 0 (subject's current house, or Neutral without a subject) |
| Trigger section | `Difficulty` | Comma-separated `Easy`, `Normal`, `Hard` | All |
| Trigger section | `Repeat` | `TagRepeat` name: `OneTimeOr`, `OneTimeAnd`, `Repeating` | `OneTimeOr` |
| `veraui.ini` | `[Layouts]`, layout section's `Version`, `<layout>.<part>` sections | See [interface layout](../interface-layout.md) and 7.10 for part keys | — |

## 1. Simulation ABI

Interface `vera:sim@1`. The import module name is `vera:sim@<major>.<minor>` (minimum required version, identical for all imports of one module and matching `Api=`). Import and export names are the function names below. Signatures use core Wasm types. Both `ptr` and `len` are `i32`. See the interface definition for costs ([RFC 4](0001-wasm-extension-modules.md#4-determinism)).

### 1.1 Imports

In the availability column, `init` means `vera_init`, `rules` means `on_rules`, `decide` means `on_decide` (calling other functions disables the module), and `all` means other calls. Unavailable functions in `init` or `rules` return `Unsupported`.

| # | Function | Signature | Semantics | Available in |
| --- | --- | --- | --- | --- |
| 1 | `frame` | `() -> i32` | Current frame number. Cannot fail, so it returns the value without a status. | all, decide |
| 2 | `random` | `(lo: i32, hi: i32, out: ptr) -> i32` | Write an `i32` integer in `[lo, hi]` drawn from the module stream. In `on_decide`, use a substream derived from the current request's key ([RFC 4](0001-wasm-extension-modules.md#4-determinism)) without advancing the main stream. `lo > hi` is invalid. In `vera_init` and `on_rules` 0.1.0 writes 0 and returns `Ok` | all, decide |
| 3 | `log` | `(ptr, len)` | Write one line to the in-game log | init, rules, all, decide |
| 4 | `has_kind` | `(table: i32, id: i32) -> i32` | 1 if `id` in `table` (`Table`) is available, 0 if unknown or disabled in this match | init, rules, all, decide |
| 5 | `lookup` | `(kind: i32, ptr, len, out: ptr) -> i32` | Resolve a name and write its `i32` ID (8.1) to `out`, or −1 and `NotFound` | init, rules, all, decide |
| 6 | `name_of` | `(kind: i32, id: i32, out: ptr, cap: i32, len_out: ptr) -> i32` | Write the ID's name to `out` and its length (`i32`) to `len_out` | init, rules, all, decide |
| 7 | `get` | `(ref: i64, prop: i32, out: ptr) -> i32` | Write the effective property value (`i64`), including modifiers | all, decide, rules (type and global rules references only) |
| 8 | `get_many` | `(refs: ptr, n: i32, props: ptr, m: i32, out: ptr, count_out: ptr) -> i32` | Read `m` properties of `n` references and write the number of values written (`i32`) to `count_out` | all, decide, rules (type and global rules references only) |
| 9 | `set` | `(ref: i64, prop: i32, value: i64) -> i32` | Write one property through its owner and return status | all, rules (type and global rules references only) |
| 10 | `modify` | `(ref: i64, prop: i32, op: i32, value: i64, frames: i32, source: i64, group: i32, out: ptr) -> i32` | Add a modifier (`ModifierOp`) and write its handle (`i64`). Negative `frames` means unlimited duration. A `group` of 0 means ungrouped. | all |
| 11 | `unmodify` | `(modifier: i64) -> i32` | Remove a modifier. Returns `Gone` if expired. | all |
| 12 | `rules_get` | `(sec: ptr, sec_len, key: ptr, key_len, out: ptr, cap: i32, len_out: ptr) -> i32` | Raw string for a key. Write its length (`i32`) to `len_out`, or −1 and `NotFound` for a missing key. | all, decide, rules |
| 13 | `rules_set` | `(sec: ptr, sec_len, key: ptr, key_len, val: ptr, val_len) -> i32` | Write a key in `on_rules` (except the AI document, [RFC 6d](0001-wasm-extension-modules.md#6d-ai)). During a match always `Refused` in version 1 ([RFC 8](0001-wasm-extension-modules.md#8-changing-rules)). | all, rules |
| 14 | `var_get` | `(scope: i64, var: i32, out: ptr) -> i32` | Read a variable (`i64`). `scope` is 0 for global or a reference to a house or object. | all, decide |
| 15 | `var_set` | `(scope: i64, var: i32, value: i64) -> i32` | Write a variable and reset timers of triggers waiting on it | all |
| 16 | `find` | `(query: ptr, len, out: ptr, cap: i32, res: ptr) -> i32` | Run a `Query`, write references to `out` and a `FindResult` (count, next) to `res`, and return the status. See “Strings and buffers”. | all, decide |
| 17 | `trigger_create` | `(def: ptr, len, out: ptr) -> i32` | Create a trigger (`TriggerDef`) and write its handle (`i64`). A missing waypoint returns 0 and `NotFound`. A disabled Ares event (4.2) returns (0, Unsupported). | all |
| 18 | `trigger_remove` | `(t: i64) -> i32` | Remove the trigger and all its instances (deferred) | all |
| 19 | `trigger_bind` | `(t: i64, ref: i64) -> i32` | Bind to an object, type, weapon, warhead or projectile type, or to a cell (only events marked “cell binding” in 4.1) | all |
| 20 | `trigger_unbind` | `(t: i64, ref: i64) -> i32` | Remove one binding | all |
| 21 | `action` | `(kind: i32, target: i64, params: ptr, len, out: ptr) -> i32` | Execute an action (section 5, `ActionParams`). Write the created object (`i64`), otherwise 0. | all |
| 22 | `send_notify` | `(to: i64, id: i32, ptr, len) -> i32` | Notify players of house `to`, or observers when `to` is 0. Exceeding the size or per-frame count limit returns `Refused`. | all |
| 23 | `order` | `(house: i64, ptr, len) -> i32` | Queue a standard player command (`Order`) for computer house `house`. It executes next frame, validated like that house's player input ([RFC 6g](0001-wasm-extension-modules.md#6g-house-orders)). | all |
| 24 | `ini_register` | `(kind: i32, section: i32, name: ptr, len, key: i32, out: ptr) -> i32` | Register an INI name (`IniKind`, [RFC 6h](0001-wasm-extension-modules.md#6h-ini-registration)). `section` is an `IniSection` for `Key`, 0 for an object event or 1 for a global event for `Event`, and 0 for `Registry`. `key` is the handler's dispatch key (ignored for `Event`). `out` receives an `i32`: the event ID for `Event`, otherwise 0. A name already registered or beginning with `Vera.` returns `Refused`. An invalid name, exceeding the count limit or calling outside the initialization function is an invalid argument. | init |

- In `on_rules`, `set` on type and global rules references is equivalent to `rules_set` with the value in INI text form (not implemented in 0.1.0: it returns `Unsupported`). `get` reads values processed up to that point in the current layer. Trigger, action, variable, modifier and `find` calls and reads or writes on other references return `Unsupported`.
- Write functions are `set`, `modify`, `unmodify`, `rules_set`, `var_set`, `trigger_create`, `trigger_remove`, `trigger_bind`, `trigger_unbind`, `action`, `send_notify`, `order` and `ini_register`.

### 1.2 Exports

Export functions as needed. For timing, see [RFC 5](0001-wasm-extension-modules.md#5-execution-model). A module that defines memory must export it as `memory` ([RFC 3](0001-wasm-extension-modules.md#3-loading-and-validation)).

| Export | Signature | When | Tier |
| --- | --- | --- | --- |
| `vera_init` | `()` | Generated by the SDK from top-level code (no `start` section). Runs once before the first other call. | v1 |
| `on_rules` | `()` | Once at the module's layer in rules processing | v1 |
| `on_scenario_start` | `()` | Once at scenario start | v1 |
| `on_frame` | `()` | Each frame at the end of the module phase. The SDK runs callbacks registered with `onFrame(new Every(N), fn)`. | v1 |
| `on_events` | `(ptr, count: i32)` | Each frame in the module phase after `on_message`. Delivers queued records in order across one or more calls, each with `count` entries of type `EventRecord`. | v1 |
| `on_decide` | `(ptr, count: i32)` | Called synchronously when the engine commits a result at a decision point. `count` entries of type `DecideRecord`, always 1 in version 1. | v1 |
| `on_message` | `(sender: i64, ptr, len)` | Once per incoming client message of the same origin. `sender` is the sender's house. `ptr` points into the delivery area. | v1 (phase 2) |

There is no `on_load`. Projectile paths use the decision events `Launch` and `ProjectileFrame` (4.5) without additional exports.

## 2. Client ABI

Interface `vera:client@1`, import module name `vera:client@<major>.<minor>`, with the version rules from section 1. All names begin with `client_`. Floating point is available. For import restrictions and budgets, see [RFC 1](0001-wasm-extension-modules.md#1-two-worlds) and [RFC 3](0001-wasm-extension-modules.md#3-loading-and-validation).

### 2.1 Imports

Reads expose only what the current player's UI can show (Client column in section 7).

| # | Function | Signature | Semantics |
| --- | --- | --- | --- |
| 1 | `client_frame` | `() -> i32` | Most recently completed simulation frame. Cannot fail, so it returns the value without a status. |
| 2 | `client_random` | `(lo: i32, hi: i32, out: ptr) -> i32` | Write a local random `i32` for presentation only |
| 3 | `client_log` | `(ptr, len)` | Write to the in-game console |
| 4 | `client_has_kind` | `(table: i32, id: i32) -> i32` | As `has_kind` |
| 5 | `client_lookup` | `(kind: i32, ptr, len, out: ptr) -> i32` | As `lookup`. Variable and notification names come from simulation module declarations for this match. |
| 6 | `client_name_of` | `(kind: i32, id: i32, out: ptr, cap: i32, len_out: ptr) -> i32` | As `name_of` |
| 7 | `client_get` | `(ref: i64, prop: i32, out: ptr) -> i32` | Read a property (`i64`). An invisible object yields `Gone`. An invisible property yields its empty value and `Refused`. |
| 8 | `client_get_many` | `(refs: ptr, n: i32, props: ptr, m: i32, out: ptr, count_out: ptr) -> i32` | Layout and batch status as for `get_many`, with `client_get` visibility checks for each entry |
| 9 | `client_find` | `(query: ptr, len, out: ptr, cap: i32, res: ptr) -> i32` | As `find`, restricted to entries visible to the current player |
| 10 | `client_var_get` | `(scope: i64, var: i32, out: ptr) -> i32` | Read a variable (`i64`), subject to visibility ([RFC 6e](0001-wasm-extension-modules.md#6e-variables)) |
| 11 | `client_set` | `(ref: i64, prop: i32, value: i64) -> i32` | Write a property of a client-owned reference (node, camera, cursor, local settings). Writing a node without permission returns `Refused` ([RFC 10](0001-wasm-extension-modules.md#10-client-modules)). |
| 12 | `client_set_bytes` | `(ref: i64, prop: i32, ptr, len) -> i32` | Write a string or byte property (`Label`, `Art`, `Items`, `ItemProgress`, etc.). Writing a node without permission returns `Refused`. |
| 13 | `client_node_create` | `(kind: i32, parent: i64, props: ptr, len, out: ptr) -> i32` | Create a node (`NodeKind`) and write its handle (`i64`), with initial properties from a `PropValue` array. A `parent` of 0 means top level. |
| 14 | `client_node_remove` | `(node: i64) -> i32` | Remove a node and its descendants. Removing a node without permission returns `Refused`. |
| 15 | `client_node_find` | `(ptr, len, out: ptr) -> i32` | Find a node by `Name` and write its handle (`i64`) (7.10, part name in the active layout for INI nodes). Search the module's own source first. Within a group of equal names return the first created. No match returns (0, NotFound). |
| 16 | `client_action` | `(kind: i32, target: i64, params: ptr, len) -> i32` | Execute a local action (5.4, `ActionParams`) |
| 17 | `client_send_order` | `(ptr, len) -> i32` | Issue a standard player command (`Order`), validated like player input |
| 18 | `client_send_message` | `(ptr, len) -> i32` | Place a message in the command stream for the same-origin simulation module's `on_message`. Observers receive `Refused`. Size and rate limits apply ([RFC 11](0001-wasm-extension-modules.md#11-multiplayer)). |
| 19 | `client_storage_read` | `(key: ptr, key_len, out: ptr, cap: i32, len_out: ptr) -> i32` | Read storage for this module's origin. Write the length (`i32`) to `len_out`, or −1 and `NotFound` for no entry ([RFC 10](0001-wasm-extension-modules.md#10-client-modules)). |
| 20 | `client_storage_write` | `(key: ptr, key_len, data: ptr, data_len) -> i32` | Write storage. `data_len` of 0 deletes the key. ASCII keys of 1..64 bytes, values ≤ 16 KiB, ≤ 256 keys and ≤ 256 KiB total keys plus values per origin. Exceeding a limit returns `Refused` and preserves storage. |
| 21 | `client_status` | `(ptr, len) -> i32` | Register, replace or remove one of this source's status templates over the battlefield, given as the keys of its INI section with `Name=` first ([status templates](../wasm-modules/status-templates.md)). A template of a name the source already has replaces it; one without `ElementN=` removes it. A key that does not parse, a name that does not resolve or a limit passed returns `Refused` and logs why; text that is not UTF-8 is an invalid argument. Not implemented before 0.2.0 |

Only `client_lookup`, `client_name_of`, `client_has_kind` and `client_log` are available in `client_init`.

### 2.2 Exports

All are v1 (phase 2).

| Export | Signature | When |
| --- | --- | --- |
| `client_init` | `()` | SDK-generated from top-level code. Once before the first other call. |
| `client_on_start` | `()` | Once after the match UI is built and the INI layout loaded. For a rejoining player, once after catch-up. |
| `client_on_render` | `(dt: f32, alpha: f32)` | Once per render frame. `dt` and `alpha` serve presentation only. |
| `client_on_events` | `(ptr, count: i32)` | Once per render frame before `client_on_render`. Uses `ClientEventRecord` (4.6). |

## 3. SDK mappings

The table illustrates how SDK operations map to host calls. Use the [TypeScript SDK reference](../wasm-modules/ts-sdk.md) for the implemented names and signatures. Rust uses newtypes and snake case (`where_prop`). The 0.1.0 Rust SDK makes the same calls under Rust names: queued callbacks take a dispatch key from `named_actions!` (`.enqueue(KEY)`), exports come from macros (`init!`, `on_rules!`, `on_scenario_start!`, `on_frame!`, `on_events!`, `on_decide!`, `on_message!`), variables from `vars!`, and its `where_var` tests global variables only.

| Purpose | TypeScript | Host call |
| --- | --- | --- |
| Read a property | `Techno.health(t)`, `House.money(h)`, `Cell.landType(c)` | `get` |
| Write a property | `Techno.setOwner(t, h)`, `Techno.setMission(t, Mission.Hunt)`, `Trigger.setEnabled(t, false)` | `set` |
| Batch read | `Techno.read(list, [Prop.Health, Prop.Owner])` | `get_many` |
| String property | `House.name(h)` returns `string` | `get` plus `name_of` |
| Modifiers | `Modifier.multiply(t, Prop.SpeedMultiplier, fixed(0.5), 150)`. Add with `Modifier.add`, override with `Modifier.override`, remove early with `Modifier.remove(m)`. | `modify`, `unmodify` |
| Actions | One function per kind, e.g. `Action.nukeStrike(cell, house)`, `Action.launchSuperWeapon(house, type, cell)`, `Action.createObject(cell, type, house)`, `Action.addMoney(house, amount)` | `action` |
| House orders | `House.order(h, Order.queueProduction(Rules.technoType("HTNK")))`, `House.order(h, Order.attackMove(units, cell))`. `Client.order` takes the same builders | `order` |
| Trigger actions | `ActionDef.damage(Target.Other, warhead, 50)`, `ActionDef.setVar(Target.Subject, v, 1)` only construct an `ActionDef` | — |
| Triggers | `Trigger.on(event).bind(ref).immediate(def).enqueue(fn).done()`, `.decideSync(fn)` | `.decideSync` and `.done` call `trigger_create`, then `trigger_bind` for each `.bind` |
| Property polling | `Trigger.onProperty(prop, op, value, interval)` | Constructs an `EventDef` for `PropertyPoll` (4.4) |
| Bindings | `Trigger.bind(t, ref)`, `Trigger.unbind(t, ref)`, `Trigger.remove(t)`, `Trigger.bindAllTypes(t, NameKind.TechnoType)` | `trigger_bind`, `trigger_unbind`, `trigger_remove`. `bindAllTypes` calls `find` with scope `Types` and a kind wildcard, then `trigger_bind` once per type. |
| Variables | Declarations generate `Wave.get()`, `Panicked.get(unit)`, `ReinforcementsBought.set(house, 1)`. Stock variables use `Var.local(n).get()`, `Var.global(n).set(value)`. | Cache `lookup` at initialization (stock IDs are fixed). One `var_get` or `var_set` per access. Clients have only `client_var_get`. |
| Names | `Rules.technoType("E1")`, `Rules.weapon("RedEye2")`, `Rules.superWeaponType("NukeSpecial")` return type references, as does `Rules.country(name)` for a country name | Encode the type reference on successful `lookup`. On failure the SDK returns `NO_HANDLE` and retains `NotFound`. |
| Rules | `Rules.get(section, key)`, `Rules.set(section, key, value)` | `rules_get`, `rules_set` |
| Searches | `.first()` or `.page(buffer, after)`. `.country(americans)` filters by country. | One `find` per `.page` call. first uses capacity 1, calls again from `next` until it has a match or `next=0`, and then returns 0 for no match. country takes a resolved country type reference and generates a Country equality clause. |
| Other | `Cell.at(x, y)`, `Waypoint.cell(n)` only encode references. `Notify.id(name)`, `Notify.send(to, id, data)` | No call for references. Notifications use `lookup`, `send_notify`. |
| INI registration | In initialization, `Ini.key(name, IniSection.TechnoType, key)`, `Ini.registry(name, key)`, `Ini.event(name, EventScope.Object)`. Handlers receive `IniKeyRecord` (`typeRef`, `value`) and `IniEntryRecord` (`position`, `entryName`). | One `ini_register` each. One `name_of` (`String`) to read a value. |
| Module events | `Trigger.onModuleEvent(ev)`, `Action.raiseEvent(ev, target, other, value, value2)`, `ActionDef.module(key, target, text)` | `trigger_create`. `action` (`RaiseEvent`). |
| Registration | Top-level `onScenarioStart(fn)`, `onFrame(new Every(N), fn)`, `onMessage(fn)`, `namedAction(name, fn)`. Client `onStart(fn)`, `onRender(fn)`, `onClientEvent(ClientEvent.Click, fn)` | SDK-generated exports |
| Client | `Ui.find(name)`, `Ui.create(NodeKind.Text, parent)`, `Node.setLabel(n, text)`, `Node.setVisible(n, v)`, `Client.viewer()`, `Client.sendMessage(data)`, `Client.order(...)`, `Storage.read(key)`, `Node.setItems(n, items)` | One `client_*` call each. `Storage.read` calls twice if the value exceeds its buffer. `Node.setItems` joins entries with `\n` and calls `client_set_bytes`. |

- Event views are valid only during their callback. Named fields read the ABI fields below in place, with the same units and empty values. Common fields retain their names. Decision views read the embedded event. `setResult(v)` writes `result` and sets `handled=1`.
- See 5.1, 8.3 and 8.1 for `Target`, `VarTarget` and `NameKind`, respectively. `.once()` selects one-shot behavior. `.house(h)` sets the house. `.where`, `.whereOther` and `.whereHouse` check properties of the subject, other object and house, respectively.
- `.immediate(def)` and `.enqueue(fn)` may repeat. The former executes at the trigger point, the latter delivers during the module phase. `.done()` finalizes the definition. For events supporting decisions, `.decideSync(fn)` sets a synchronous decision handler and finalizes the definition.
- `namedAction(name, fn)` requires a string literal name, written to `vera.actions` at build time.

| SDK view | Named field → record field |
| --- | --- |
| `TakeDamageEvent` | `victim → subject`, `attacker → other`, `damage → value`, `warhead → id` |
| `FireEvent` | `shooter → subject`, `target → other`, `targetCell → cell`, `weaponSlot → value`, `weapon → id` |
| `TargetChangedEvent` | `newTarget → other` |
| `ClickEvent` (client) | `node → ref`, `itemIndex → value` |

## 4. Event kinds

See [RFC 5](0001-wasm-extension-modules.md#5-execution-model), [RFC 6a](0001-wasm-extension-modules.md#6a-triggers-events-filter-clauses-and-actions), [RFC 6f](0001-wasm-extension-modules.md#6f-decidable-events) and [RFC 7](0001-wasm-extension-modules.md#7-runtime-triggers-and-tags).

**Fields** lists populated `EventRecord` fields. **Binding** restricts which subjects raise an event. Object means an object with a trigger bound directly, by type or by tag. Type means a bound type. Cell means a cell with a CellTag or cell binding. Global requires no binding. **Result slot** describes `result` (also the meaning of `native`). Each event kind has a fixed result area size ([RFC 15.4](0001-wasm-extension-modules.md#154-preview-values)). INI trigger sections name events 0..88 and 1000..1055; in 0.1.0 1060..1065 have no INI names. `trigger_create` returns `Unsupported` for an event kind the engine does not raise (the `later` rows), and `has_kind` returns 0 for it. House tag events poll every 8 frames. Logic tag events poll every frame.

### 4.1 Stock events 0..61

Class `R` is delivered, `H` polls a House tag, and `L` polls a Logic tag. Fields `S`, `O` and `Hs` denote subject, other and house. “Persistent” means the event writes persistent state. H and L are global. R defaults to object binding, with exceptions in the table. All are `v1` except `later` events 0, 24, 31, 33, 51, 53, 54 and 59 and have no decision form.

| # | API name | Arguments | Class | Fields | Binding exceptions and notes |
| --- | --- | --- | --- | --- | --- |
| 0 | — | — | — | — | Private, never true |
| 1 | `EnteredBy` | `value` house index (−1 for any) | R | S, O, retained Hs | Persistent |
| 2 | `SpiedBy` | Ignored | R | S, O | Raised on the infiltrated building, `other` the spy, for module triggers |
| 3 | `ThievedBy` | `value` house index | R | S, O, Hs | Raised on the stolen vehicle, `other` the thief, `Hs` the thief's house, for module triggers |
| 4 | `DiscoveredByPlayer` | Ignored | R | S, O |  |
| 5 | `HouseDiscovered` | `value` house | H | Hs |  |
| 6 | `AttackedByAnyHouse` | Ignored | R | S, O |  |
| 7 | `DestroyedByAnyHouse` | Ignored | R | S, O | Requires a kill source |
| 8 | `AnyEvent` | — | H | S |  |
| 9 | `AllUnitsDestroyed` | `value` house | H | Hs |  |
| 10 | `AllBuildingsDestroyed` | `value` house | H | Hs |  |
| 11 | `AllDestroyed` | `value` house | H | Hs |  |
| 12 | `CreditsExceed` | `value` credits | H | Hs |  |
| 13 | `ElapsedTime` | `value` seconds (×15 frames) | L | — |  |
| 14 | `MissionTimerExpired` | — | L | — |  |
| 15 | `BuildingsDestroyedCount` | `value` count | H | Hs |  |
| 16 | `UnitsDestroyedCount` | `value` count | H | Hs |  |
| 17 | `NoFactoriesLeft` | — | H | Hs |  |
| 18 | `CiviliansEvacuated` | — | H | Hs |  |
| 19 | `BuildBuildingType` | `value` BuildingType index | H | Hs | Persistent, also accepts `id` |
| 20 | `BuildUnitType` | `value` VehicleType index | H | Hs | As 19 |
| 21 | `BuildInfantryType` | `value` InfantryType index | H | Hs | As 19 |
| 22 | `BuildAircraftType` | `value` AircraftType index | H | Hs | As 19 |
| 23 | `TeamLeavesMap` | `id` TeamType | R | — | Global binding |
| 24 | `ZoneEntryBy` | `value` house (−1 for any) | R | S, O, retained Hs |  |
| 25 | `CrossesHorizontalLine` | As 24 | R | O, retained Hs | Cell binding |
| 26 | `CrossesVerticalLine` | As 24 | R | O, retained Hs | Cell binding |
| 27 | `GlobalIsSet` | `value` 0..49 | L | — |  |
| 28 | `GlobalIsClear` | `value` 0..49 | L | — |  |
| 29 | `DestroyedByAnythingNotInfiltrate` | Ignored | R | S, O |  |
| 30 | `LowPower` | `value` house | H | Hs |  |
| 31 | `BridgeDestroyed` | Ignored | R | `cell` | Cell binding |
| 32 | `BuildingExists` | `value` BuildingType index | H | Hs | Persistent |
| 33 | `SelectedByPlayer` | Ignored | R | S | Disabled online. Client form: `SelectionChanged` (4.6) |
| 34 | `ComesNearWaypoint` | `value` waypoint | R | S | Within 0x500 leptons |
| 35 | `EnemyInSpotlight` | Ignored | R | S |  |
| 36 | `LocalIsSet` | `value` 0..99 | L | — |  |
| 37 | `LocalIsClear` | `value` 0..99 | L | — |  |
| 38 | `FirstDamagedCombat` | Ignored | R | S, O | Requires a damage source |
| 39 | `HalfHealthCombat` | Ignored | R | S, O |  |
| 40 | `QuarterHealthCombat` | Ignored | R | S, O |  |
| 41 | `FirstDamagedAny` | Ignored | R | S, O (may be 0) |  |
| 42 | `HalfHealthAny` | Ignored | R | S |  |
| 43 | `QuarterHealthAny` | Ignored | R | S |  |
| 44 | `AttackedByHouse` | `value` house array index | R | S, O | Ares translates Player@X |
| 45 | `AmbientLightBelow` | `value` | L | — | Checked when ambient light changes |
| 46 | `AmbientLightAbove` | `value` | L | — | As 45 |
| 47 | `ElapsedScenarioTime` | `value` seconds | L | — |  |
| 48 | `DestroyedByAnything` | Ignored | R | S, O (may be 0) |  |
| 49 | `PickupCrate` | Ignored | R | S |  |
| 50 | `PickupCrateAny` | Ignored | L | — | Checked when a crate is picked up |
| 51 | `RandomDelay` | `value` seconds | L | — | RNG native |
| 52 | `CreditsBelow` | `value` credits | H | Hs |  |
| 53 | `SpyAsHouse` | `value` house | R | S, Hs | Persistent |
| 54 | `SpyAsInfantry` | `value` InfantryType index | R | S | Persistent |
| 55 | `NavalUnitsDestroyed` | `value` house | H | Hs |  |
| 56 | `LandUnitsDestroyed` | `value` house | H | Hs |  |
| 57 | `BuildingDoesNotExist` | `value` BuildingType index | H | Hs |  |
| 58 | `PowerFull` | `value` house | H | Hs |  |
| 59 | `EnteredOrOverflownBy` | As 1 | R | S, O, retained Hs |  |
| 60 | `TechTypeExists` | `id` type, `value` count | L | — |  |
| 61 | `TechTypeDoesNotExist` | `id` type | L | — |  |

### 4.2 Ares events 62..88

Available only with `rules.ares_form()`. Otherwise `trigger_create` returns 0 and `Unsupported`, and `has_kind` returns 0. Events 62..86 are `v1`, 87 and 88 `later`, all with no decision form. R events populate `subject` and bind to objects. H and L are global.

| # | API name | Arguments | Class |
| --- | --- | --- | --- |
| 62 | `UnderEmp` | — | R |
| 63 | `UnderEmpByHouse` | `value` house | R |
| 64 | `RemovedFromEmp` | — | R |
| 65 | `RemovedFromEmpByHouse` | `value` house | R |
| 66 | `InSpotlight` | — | R |
| 67 | `DriverKilled` | — | R |
| 68 | `DriverKilledByHouse` | `value` house | R |
| 69 | `VehicleTaken` | — | R |
| 70 | `VehicleTakenByHouse` | `value` house | R |
| 71 | `Abducted` | — | R |
| 72 | `AbductedByHouse` | `value` house | R |
| 73 | `AbductsSomething` | — | R |
| 74 | `AbductsSomethingOfHouse` | `value` house | R |
| 75 | `SuperWeaponActivated` | `value` superweapon type index | H |
| 76 | `SuperWeaponDeactivated` | `value` superweapon type index | H |
| 77 | `SuperWeaponNearWaypoint` | `id` superweapon type, `value` waypoint (5 cells) | H |
| 78 | `HouseReverseEngineeredType` | `id` type | H |
| 79 | `SomethingReverseEngineered` | — | R |
| 80 | `TypeReverseEngineered` | `id` type | R |
| 81 | `HouseOwnsAtLeast` | `id`, `value` | H |
| 82 | `HouseOwnsAtMost` | `id`, `value` | H |
| 83 | `AttackedOrDestroyedByAnybody` | — | R |
| 84 | `AttackedOrDestroyedByHouse` | `value` house | R |
| 85 | `DestroyedByHouse` | `value` house | R |
| 86 | `TypeCountAtMost` | `id`, `value` | L |
| 87 | `HouseNoKeepAliveObjects` | `value` house | H |
| 88 | `HouseNoKeepAliveBuildings` | `value` house | H |

### 4.3 New delivered events

Modifier events bind by target. For an object, bind that object. For a house, the event is global and sets `house`.

These events fire at safe points. **Object + weapon** also permits binding to weapon, warhead or projectile types ([RFC 6a](0001-wasm-extension-modules.md#6a-triggers-events-filter-clauses-and-actions)). The tier is for the delivered form. Of the decision forms only `TakeDamage` is `v1`. Those of `Fire` and `FatalDamage` are `preview`, that of `TargetChanged` is `later`. 1037 and 1038 are reserved (`OnBehavior`, `OnBehaviorExpire`). Events 1044 and later share one numbering with 4.5.

| # | Event | Fields | When raised | Binding | Decision point → result (— means no decision) | Tier |
| --- | --- | --- | --- | --- | --- | --- |
| 1000 | `TakeDamage` | `other` source (may be 0), `value` positive settled damage, after armor multipliers, immunities, falloff, the subject armor's Verses, the cap, the building minimum and Ares culling. Delivered records clamp it to health before the hit, decisions see it unclamped. `id` warhead, `x`, `y`, `z` subject position in leptons | After damage commits | Object | Before damage commits, only when this value is above 0 → final damage before the health clamp (clamped to 0..2^31−1). Zero damage and healing are not asked. | v1 |
| 1001 | `HitByWarhead` | `id` warhead, `value` final damage, `other` source, `x`, `y`, `z` subject position in leptons | After damage is accepted, excluding immune hits | Object | — | later |
| 1002 | `AttackedBy` | `other` attacker, `value` weapon slot (−1 for none), `projectile` (may be 0) | On target after firing step | Object | — | v1 |
| 1003 | `FatalDamage` | `other` attacker (may be 0), `value` damage, `id` warhead, `x`, `y`, `z` subject position in leptons | After lethal damage commits, before death processing, which rechecks. A death without a damage commit (crush, chrono erasure, crash, passengers and slaves dying inside, drops) raises it before the kill is recorded, `value` the health left and `id` −1. A sale is no death | Object | At the entry of the damage path's death arm, before any of its effects → death plan (bits in the table below). Deaths outside the damage path are not asked | v1 |
| 1004 | `Killed` | `other` killer (may be 0), `id` lethal warhead (−1 for none), `x`, `y`, `z` death position in leptons | After recording kill | Object | — | v1 |
| 1005 | `Fire` | `other` target (may be 0), `cell` target cell (always set), `value` weapon slot, `id` weapon, `projectile` (may be 0) | After shot completes | Object + weapon | Before firing, once the launch has a solution and the bullet payload is built → number of entries in the shot plan (0..16). Result area contains a `ProjectileSpec` array; the first link receives the native shot as one entry ([RFC 9](0001-wasm-extension-modules.md#9-example-projectile-trajectories)). | v1 |
| 1006 | `SpawnFire` | `other` spawned aircraft, `value` weapon slot, `projectile`. Target in `cell` or the spawned aircraft's `Target` property | After spawned aircraft fires | Object | — | later |
| 1007 | `HitTarget` | `other` target, `value` damage, `id` weapon, `projectile` | On shooter after damage commits | Object + weapon | — | later |
| 1008 | `TargetChanged` | `other` new target object (0 when cleared or a cell), `cell` its cell (0 when cleared) | After the target setter changed the target, or a direct target write by the engine (not the initial target of a new object) | Object | Before setting the target during the object's turn. Parallel speculative targeting raises no event → chosen target reference (0 for none). An unattackable target falls back to the native result. Use `find` for candidates. | v1 |
| 1009 | `ProjectileIncoming` | `projectile`, `other` firer (may be 0) | After projectile admission | Object | — | later |
| 1010 | `Detonated` | On the source (`subject` and `other`, may be 0). `projectile` (already gone at delivery), `cell`, `id` warhead, `x`, `y`, `z` detonation position in leptons | After a fired projectile's whole detonation (cluster included). Death weapons, animation weapons and other detonations without a projectile raise none | Object + weapon | — | v1 |
| 1011 | `Birth` | `other` producer or origin (may be 0), `value` production time (−1 for none) | After object placement (production, reinforcements, spawning, a deploy or undeploy, survivors, crew and pilots; `other` the building or vehicle they came from) | Object | — | v1 |
| 1012 | `TransportEntered` | On passenger (`subject`). `other` transport or building, `value` occupied space, `value2` 1 garrison / 0 transport passenger | After entry completes | Object | — | v1 |
| 1013 | `TransportExited` | As `TransportEntered` (garrison: `value` occupants left inside) | After exit completes; a garrison occupant also when removed inside (before it is removed) | Object | — | v1 |
| 1014 | `PassengerAdded` | On transport or building (`subject`). `other` passenger, `value` occupied space, `value2` garrison flag | As `TransportEntered` | Object | — | v1 |
| 1015 | `PassengerRemoved` | As `PassengerAdded` | As `TransportExited` | Object | — | v1 |
| 1016 | `Commanded` | On unit (`subject`). `value` command (`Command`), `other` target object (may be 0), `cell` target cell (may be 0), `value2` command serial shared by units receiving the same order from one click | After simulation executes player or AI command for bound object | Object | — | v1 |
| 1017 | `Crushed` | `other` crusher, `x`, `y`, `z` position in leptons | After the crush is decided, before the victim's kill is recorded | Object | — | v1 |
| 1018 | `HouseTransfer` | `house` new house, `value` 1 for mind control | Before transfer, which rechecks | Object | — | later |
| 1019 | `HouseTransferred` | `house` old house, `value` 1 for mind control | After transfer | Object | — | v1 |
| 1020 | `ModifierStart` | `modifier`, `value` property ID, `other` source (may be 0) | After adding modifier | Modifier target | — | v1 |
| 1021 | `ModifierExpire` | `modifier` (reads as gone after delivery), `value` property ID | After expiry or removal | Modifier target | — | v1 |
| 1022 | `ModifierStacks` | `modifier`, `value` number of the subject's group 0 modifiers with the same origin and property | After adding a group 0 modifier when that number exceeds 1 | Modifier target | — | v1 |
| 1023 | `Timer` | `value` frame of last firing | Every `interval` frames | Global | — | v1 |
| 1024 | `SuperWeaponLaunched` | `id` superweapon type, `house`, `cell` target | After superweapon launch | Global | — | v1 |
| 1025 | `ProductionCancelled` | `house`, `id` type, `value` credits refunded | After player or AI cancels production queue entry | Type | — | later |
| 1040 | `ModuleEvent` | `id` event ID (same as `EventDef.id`), `subject` (0 for a global event), `other`, `value`, `value2`, `house` from the `RaiseEvent` parameters ([RFC 6h](0001-wasm-extension-modules.md#6h-ini-registration)) | When `RaiseEvent` executes | Object when registered as an object event, otherwise global | — | v1 |
| 1048 | `MissionChanged` | `value` new mission (`Mission`) | After the current mission changes. Deferred: the engine writes mission changes from many places without a frame's effects at hand, so there is no single raise point yet | Object | — | later |
| 1051 | `Deployed` | `value` 1 deployed, 0 undeployed, `other` the object it became (0 when it stays itself) | After a deploy or undeploy completes (a unit into a building, a simple deployer, an infantryman) | Object | — | v1 |
| 1052 | `Promoted` | `value` new rank (1 veteran, 2 elite) | After the rank sample stores a promotion | Object | — | v1 |
| 1053 | `ExperienceGained` | `other` victim, `value` gain in hundredths of a rank (the `Experience` unit) | On each recipient after a kill's experience is awarded | Object | — | v1 |
| 1054 | `Cloaked` | — | After cloaking completes | Object | — | v1 |
| 1055 | `Decloaked` | — | After uncloaking completes | Object | — | v1 |
| 1043 | `OrderRefused` | `subject` refused unit (0 for a house-level order), `house` house that issued the order, `value` command (`Command`), `value2` reason (`RefuseReason`), `other` and `cell` the order's target, `id` type for a house-level order (otherwise −1) ([RFC 6i](0001-wasm-extension-modules.md#6i-driving-a-house)) | After an order is refused in the execution step, once per unit, or once for a house-level order | Global | — | v1 |

`FatalDamage`'s death plan (the decision form's result; `native` is the engine's own plan):

| Bits | Meaning | Native value |
| --- | --- | --- |
| 0 | Explode: passengers die with the object and the death weapon may fire | `Explodes=`, the veteran ability or a suicide weapon; 0 for a low dropped bomb over water |
| 1 | Fire the death weapon (needs bit 0) | 1; 0 under Ares `SuppressDeathWeapon` |
| 2 | Throw debris | 1; 0 for a low dropped bomb over water |
| 3 | Survivors: a building's survivors appear, a vehicle's passengers escape | 0 when the killing hit ignored defenses |
| 4 | A vehicle's crew escapes | 0 when the killing hit forbids it |
| 5 | Unit-lost announcement | 1; 0 under Ares `UnitLost.Suppress` |
| 6 | Tiberium remains: up to 2 Riparius on the cell and each orthogonal neighbour. The engine has no wreck object; this is the death's one leftover | Ares `TiberiumRemains` (by default `TiberiumHeal=` with `TiberiumHealEnabled`); 0 without Ares |
| 8–15 | An infantryman's `InfDeath` (0..255) | The killing warhead's |
| 32–62 | Death weapon ID plus 1, fired instead of the type's own (at `DeathWeaponDamageModifier`); 0 for the type's own | 0 |

Other bits must be 0. An answer with other bits, a negative answer or an unknown weapon ID keeps the previous link's result. Sinking, crashing, `DeathFrames=`, explosion animations and releasing slaves and mind-controlled units stay with the engine. A plan that drops debris, the passengers' deaths or the death weapon drops their random draws with them: a different outcome, still deterministic.

### 4.4 New polling events

All are `v1` with no decision form. See [RFC 6a](0001-wasm-extension-modules.md#6a-triggers-events-filter-clauses-and-actions) for sampling intervals, staggering and trigger edges. By default `op` compares the reading against `EventDef.value`. The record's `value` is the actual reading. House events are global and set `house`. Object events set `subject`.

| # | Event | Reading |
| --- | --- | --- |
| 1026 | `PropertyPoll` | `EventDef.id` is a property ID. `op`, `value` and `interval` specify comparison, threshold and interval. For a bound object, accepts only `Health`, `HealthPercent`, `Ammo`, `PassengerCount`, `TransportSpaceLeft`, `Storage`, `Mission`, `Moving` and `LandType`. `Money` reads the trigger's house (global event). Reject other properties at creation. The SDK field `reading` reads `EventRecord.value`. |
| 1027 | `HousePowerPoll` | Power output minus drain |
| 1028 | `ModifierCountPoll` | Number of modifiers on the subject in group `id` from this module's origin |
| 1029 | `DistancePoll` | Distance to a cell in leptons. `EventDef.value` is a cell or waypoint reference. `op` compares distance to `id` (leptons). |
| 1030 | `VariablePoll` | Variable `id`, read on the subject, trigger's house or global scope according to its declaration. Writing the variable resets the timer. |

### 4.5 Decision-only events

Require a trigger with a decision handler. These events have no delivered form.

| # | Event | Fields | Decision point | Binding | Result slot | Tier |
| --- | --- | --- | --- | --- | --- | --- |
| 1031 | `ProductionAllowed` | `house`, `id` type | Once per frame for each house and bound type at a fixed step. Sidebar and AI read the cached result. | Type | 1 allow, 0 disallow | preview |
| 1032 | `ProductionQueued` | `house`, `id` type, `subject` 0 | When player or AI adds the type to the production queue, excluding continued or forced requeues. On refusal for a computer house the engine makes it choose again | Type | 1 allow, 0 refuse. Refusal leaves the item out of the queue and spends no credits. | v1 |
| 1033 | `Defeat` | `house` | When the engine's defeat check is about to defeat this house. `EndFor` and stock win/loss actions bypass the check. | Global | 1 defeat, 0 veto this check and ask again at the next check | v1 |
| 1034 | `CommandAllowed` | `subject`, `value` command (`Command`), `other` target object, `cell` target cell | When simulation executes each player or AI command on a bound object. Cursor previews do not raise it. | Object | 1 allow, 0 refuse | preview |
| 1035 | `Launch` | `projectile`, `subject` firer (0 when gone), `other` target object, `cell` cell of the target coordinate | Once for each flying projectile when it is launched, after it enters the game: firing, shot plans, shrapnel, airburst, superweapons and trigger strikes. `Inviso=` projectiles are not asked | Object + weapon | Size of `PathSpec` in bytes (32 + 24 × point count). Result area contains one `PathSpec`. Unhandled, the projectile flies natively | preview |
| 1036 | `ProjectileFrame` | `subject` and `projectile` the projectile, `value` elapsed flight frames | Each frame as the engine advances projectiles in engine order, after the animation wait, target resolution and the frame fuse, before computing the candidate position, once per bound projectile that moves this frame | Projectile type | Unused. Result area contains one `ProjectileStep`. `handled` of 0 follows the `Launch` path or native advancement | preview |
| 1039 | `DamageIncoming` | `subject`, `other` source (may be 0), `id` warhead, `value` signed incoming damage, `value2` `previous` put through falloff, the Verses of the armor in the trigger's `EventDef.id` (−1 for the subject's) and the cap, with the sign of `previous`. With bit 0 of `EventDef.value` set, a positive value is first divided by the product of the subject's armor multipliers, truncating toward zero. `x`, `y`, `z` subject position in leptons | Techno receive-damage entry, after infantry prone, the parasite prefix and the engine's own shield, before death prevention, subject armor and the minimum of 1. Not asked for zero incoming damage, ignore-defenses hits, Psychedelic or Temporal warheads, or hits that immunity, Iron Curtain or allegiance would reject. Zero Verses against the subject's armor does not prevent the request. | Object | Incoming damage continuing into the native flow. It must be 0 or have the sign of `previous`, otherwise the previous result stands. Recompute `value2` from each link's own `previous`. A result of 0 suppresses the minimum of 1. | preview |
| 1044 | `FireAllowed` | `subject` shooter, `other` target object (0 for a cell), `cell` target cell, `value` weapon slot, `id` weapon | When a fire routine's fire check allowed a shot, before it fires | Object | 1 allow, 0 veto (the shooter waits as if rearming) | preview |
| 1045 | `Retaliate` | `subject` damaged object, `other` source, `id` warhead | After the engine decided whether to retaliate against a hit with a source | Object | 1 retaliate, 0 not; `native` the engine's answer. Retaliation still needs the source in reach | preview |
| 1046 | `DestinationSet` | `subject`, `other` native destination object (0 for a cell), `cell` its cell, `value` native destination reference | When the destination setter receives a non-null destination | Object | Destination reference (object or cell), 0 for none. A gone object, an off-map cell or one the setter refuses keeps the native destination | preview |
| 1047 | `CrushAllowed` | `subject` victim, `other` crusher | When a crusher entering a cell is about to crush an occupant. Cell entry tests (path queries) are not asked | Object | 1 allow, 0 spare (the victim stays in the cell) | preview |
| 1049 | `HarvestCell` | `subject` harvester or slave manager, `cell` scanned cell, `value` its reference (0 for none) | When the ore scan answers | Object | Cell reference to harvest, 0 for none; an off-map cell keeps the native answer | preview |
| 1050 | `DeployAllowed` | `subject` | Before a unit deploys into a building, a simple deployer starts deploying or an infantryman deploys | Object | 1 allow, 0 refuse (as a blocked deploy) | preview |
| 1060 | `IdleMission` | `subject` | When an infantryman or vehicle goes idle, after the engine picks the mission to queue and before queuing it. Not asked when nothing is queued, nor for aircraft and buildings | Object | Mission to queue (`Mission`), 255 for none. `native` is the engine's pick | preview |
| 1062 | `MissionStep` | `subject`, `value` the mission whose body ran (`Mission`), `other` target object (0 for a cell or none), `cell` target cell | Each time an infantryman's or vehicle's Guard, Sticky, Area Guard, Hunt or Attack mission body runs, after the body and its random draws, before its mission timer and queued mission are written. Aircraft and buildings are not asked | Object | Unused. Result area contains one `MissionStepResult`, arriving with the body's outcome: the target and destination it left, the mission it queues (−1 for none) and its delay. Each changed field is applied through the engine's own setters (the destination through `DestinationSet`); a gone object, an off-map cell, an unknown mission or a delay below 1 keeps the body's. What else the body did (firing, deploying, a mission queued by going idle) stands | preview |
| 1063 | `MoveFrame` | `subject`, `x`, `y`, `z` its position, `value` 1 while its locomotor is moving, `other` and `cell` its navigation target (object or cell) | Each frame a vehicle or infantryman on a Drive, Ship, Walk, Mech or Hover locomotor reaches its locomotor's processing with no step in flight (a track or sub-cell step runs to its end first), after an active `Push`'s frame. Not asked for transported, warping or train objects, nor for other locomotors | Object | Unused. Result area contains one `MoveStep`, zero for the first link. Bit 0 takes the frame: the native processing does not run and the object moves by the displacement; a step into another cell checks that cell only (on the map, not from a bridge deck, the object's own cell entry test answering free), otherwise it stays. The engine keeps its height, moves its occupancy, ends its path queue and runs the cell arrival on a new cell. `handled` of 0, or no bit 0, leaves the frame to the locomotor | preview |
| 1064 | `PathFound` | `subject`, `x`, `y`, `z` its position, `cell` the search goal's cell, `value` the native route's cell count (0 for none) | When a path search's route (or its failure) is adopted, after the search and before the route is copied, wherever the search ran (the path batch only computes it early). Not asked for a train extending its queued route or a route with a tube step | Object | Number of cells (0..24); result area contains that many `PathCell`s, the first link the native route's. 0 is no route (the native failure follows); otherwise the cells, each next to the one before and the first next to the object's cell, all on the map, replace the route, else the native route stands | preview |
| 1065 | `ApproachPoint` | `subject`, `x`, `y`, `z` its position, `other` target object (0 for a cell target), `value2` the target's cell reference, `cell` the chosen cell, `value` its reference | When an object approaching a target out of range (Attack, Hunt, Area Guard, Rescue, Patrol and the engine's reach check) has chosen the cell to move to, after the search and its random draws, before it is set as the destination. Not asked when the approach ends without a chosen cell (target in range, a queued destination, dropping the target, no reachable cell) | Object | Cell reference to move to; a cell on the map replaces the chosen one, anything else (0 included) keeps the previous link's (the native cell for the first). The destination is then set as for the native cell | preview |

### 4.6 Client event kinds

Delivered in render-frame batches to `client_on_events` (`ClientEventRecord`). All are `v1` (phase 2).

| # | Kind | Fields | When | Condition |
| --- | --- | --- | --- | --- |
| 0 | `Click` | `ref` node, `value` zero-based `List` row or `CameoGrid` item index, −1 for other nodes | Click on node. The click does not reach the map. | Node `Clickable` is 1 |
| 1 | `Hover` | `ref` node, `value` 1 enter / 0 leave | Cursor enters or leaves node | Node `Hoverable` is 1 |
| 2 | `Hotkey` | `id` hotkey ID | Registered hotkey pressed | Registered with local action `RegisterHotkey` (5.4). A game hotkey bound to the same key takes precedence and executes alone. |
| 3 | `Dismiss` | `ref` node | Mouse pressed outside node or Esc pressed | Node visible and `Dismissable` is 1 |
| 4 | `MapClick` | `ref` cell, `value` 0 left / 1 right | Map click, excluding clicks that generated `Click` or `Dismiss` | Always. When cursor `CaptureMapClick` is 1 the click does not reach the game. |
| 5 | `SelectionChanged` | — | Local selection changes. Read with `client_find` and a `Selected=1` clause. | Always |
| 6 | `Notify` | `source`, `id` notification ID, `data`, `data_len` | Notification addressed to this machine arrives ([RFC 10](0001-wasm-extension-modules.md#10-client-modules)) | Always |
| 7 | `SimEvent` | `data`, `data_len`: one `EventRecord` in the delivery area | `Commanded`, `OrderRefused`, `TakeDamage`, `Killed`, `Birth` and `AttackedBy` whose subject is the player's own object, and house events addressed to the player's house. `other` the local player cannot see is 0. Not delivered while catching up frames ([RFC 10](0001-wasm-extension-modules.md#10-client-modules)) | Always |

### 4.7 Scenario-start delivery records

Delivered only at scenario start, to the module that registered them, before that module's `on_scenario_start` ([RFC 6h](0001-wasm-extension-modules.md#6h-ini-registration)). They cannot be used in triggers. Using them in `EventDef` is an invalid argument. `key` is the dispatch key from registration, `trigger`, `holder` and `target` are 0, and `id` is −1. All are `v1`.

| # | Record | Fields |
| --- | --- | --- |
| 1041 | `IniEntry` | `args` entry section name, `value` position in the merged list (from 0; a repeated entry name is delivered only the first time) |
| 1042 | `IniKey` | `subject` type reference of the section (global rules reference for `General`), `args` merged value |

## 5. Action kinds

### 5.1 Using actions

See [RFC 5](0001-wasm-extension-modules.md#5-execution-model), [RFC 6b](0001-wasm-extension-modules.md#6b-actions) and [RFC 2](0001-wasm-extension-modules.md#2-module-sources-mods-and-maps).

```rust
enum ActionTarget { Subject = 0, Other = 1, TagHolder = 2, Projectile = 3, House = 4, Cell = 5 }   // Trigger targets default to Subject
```

- In triggers, the target is an `ActionTarget`. The event supplies `other` and the engine ignores `ActionParams.other`. Direct calls take a target reference and use `ActionParams.other` for the second object (source, passenger, team, trigger or tag).
- In the Target column, *object* accepts `Subject`, `Other` or an object-valued `TagHolder` (an object handle in direct calls). *Cell* accepts `Cell` or the cell of an object target, including `Projectile` (a cell, waypoint or object reference in direct calls). `House` selects a house. A dash means no target. Unlisted target kinds are invalid arguments.
- *live* marks stock actions that act on the tag holder. Their default trigger target is `TagHolder`.
- With Ares enabled, stock actions enter the Ares branch first. Unless noted otherwise, these actions are `v1` and use `native` RNG when running native logic.

### 5.2 Stock actions 0..145

Parameters are fields of `ActionParams`. Where stock actions take a waypoint, the API accepts a cell or waypoint reference in `cell`.

See [section 9](#9-version-1-inventory) for version 1 tiers.

| # | API name | Target | Parameters | Notes |
| --- | --- | --- | --- | --- |
| 0 | — | — | — | No operation |
| 1 | `DeclareWinner` | — | `text` country | Affects the player independently of the trigger house |
| 2 | `DeclareLoser` | — | `text` country | |
| 3 | `BeginProduction` | `House` | — | |
| 4 | `CreateTeam` | `House` | `id` TeamType | Subject to the team limit. House 0 uses the TeamType's house. Returns a team handle or 0. Only trigger calls add the stock object placement range |
| 5 | `DestroyTeams` | — | `id` TeamType | |
| 6 | `AllToHunt` | `House` | — | |
| 7 | `Reinforce` | — | `id` TeamType | Uses the TeamType's own waypoint |
| 8 | `DropZoneFlare` | Cell | — | |
| 9 | `FireSale` | `House` | — | |
| 10 | `PlayMovie` | — | `value` movie number | For every player |
| 11 | `ShowText` | — | `text` CSF entry name | |
| 12 | `DestroyTrigger` | — | `other` trigger or `text` trigger ID | Defers removal of the running instance, as in `trigger_remove` |
| 13 | `BeginAutocreate` | `House` | — | |
| 14 | `ChangeHouse` | Object (*live*) | `house` new owner | Same as `SetOwner` |
| 15 | `AllowWin` | — | — | Flag only (bit 1 of `TriggerDef.flags`), with no execution branch |
| 16 | `RevealAll` | — | — | |
| 17 | `RevealAround` | Cell | — | |
| 18 | `RevealZone` | Cell | — | |
| 19 | `PlaySound` | — | `id` sound | |
| 20 | `PlayTheme` | — | `text` theme | |
| 21 | `PlaySpeech` | — | `text` EVA name | |
| 22 | `ForceTrigger` | — | `other` trigger or `text` trigger ID | Fires with no actor and a zero cell |
| 23 | `TimerStart` | — | — | |
| 24 | `TimerStop` | — | — | |
| 25 | `TimerExtend` | — | `value` seconds | |
| 26 | `TimerShorten` | — | `value` seconds | |
| 27 | `TimerSet` | — | `value` seconds | |
| 28 | `SetGlobal` | — | `value` 0..49 | Out of range is an invalid argument, as in `var_set` |
| 29 | `ClearGlobal` | — | `value` 0..49 | |
| 30 | `AutoBaseBuilding` | `House` | `value` on/off | |
| 31 | `GrowShroud` | — | — | |
| 32 | `DestroyObject` | Object (*live*) | — | Ares survivor rules apply, as in `Kill` |
| 33 | `OneTimeSuperWeapon` | `House` | `id` superweapon type | |
| 34 | `RepeatingSuperWeapon` | `House` | `id` superweapon type | |
| 35 | `PreferredTarget` | `House` | `value` | |
| 36 | `AllChangeHouse` | `House` | `house` new owner | |
| 37 | `MakeAlly` | `House` | `house` other house | Bidirectional |
| 38 | `MakeEnemy` | `House` | `house` other house | Bidirectional |
| 39 | — | — | — | Unavailable |
| 40 | `SetVisibleMapArea` | Cell (top left) | `value` width, `value2` height | |
| 41 | `PlayAnimAt` | Cell | `id` animation | Ares `MakeInfantry=` ownership rules |
| 42 | `DetonateWeaponAt` | Cell | `id` weapon | Same as `DetonateWeapon` with no source |
| 43 | `PlayVoxelAnimAt` | Cell | `id` voxel animation | |
| 44 | `IonStormStart` | — | `value` duration | `Refused` if a storm is in progress |
| 45 | `IonStormStop` | — | — | `Refused` if no storm is in progress |
| 46 | `LockInput` | — | — | |
| 47 | `UnlockInput` | — | — | |
| 48 | `CenterCamera` | Cell | `value` speed | Each player's camera. See 5.4 for the client form |
| 49 | — | — | — | Zoom in, unavailable |
| 50 | — | — | — | Zoom out, unavailable |
| 51 | `Reshroud` | `House` | — | |
| 52 | `SpotlightBehavior` | Object (*live*, building) | `value` | |
| 53 | `EnableTrigger` | — | `other` trigger or `text` trigger ID | Checks difficulty and resets the timer, as when writing `Enabled` |
| 54 | `DisableTrigger` | — | `other` trigger or `text` trigger ID | |
| 55 | `RadarEvent` | Cell | `value` `RadarEventType` | |
| 56 | `SetLocal` | — | `value` 0..99 | |
| 57 | `ClearLocal` | — | `value` 0..99 | |
| 58 | `MeteorShower` | Cell | `value` intensity | |
| 59 | `ReduceOre` | Cell | — | |
| 60 | `SellBuilding` | Object (*live*, building) | — | |
| 61 | `TurnOffBuilding` | Object (*live*, building) | — | Same as writing 0 to `Online` |
| 62 | `TurnOnBuilding` | Object (*live*, building) | — | |
| 63 | `DamageBridgeAt` | Cell | — | |
| 64 | `LightFlashSmall` | Cell | — | Flash for a 50-damage hit |
| 65 | `LightFlashMedium` | Cell | — | 100 damage |
| 66 | `LightFlashLarge` | Cell | — | 300 damage |
| 67 | `AnnounceWin` | — | — | |
| 68 | `AnnounceLose` | — | — | |
| 69 | `EndScenario` | — | — | |
| 70 | `DestroyTag` | — | `other` tag or `text` tag ID | Stops delivery already in progress |
| 71 | `SetAmbientStep` | — | `value` | |
| 72 | `SetAmbientRate` | — | `value` | |
| 73 | `SetAmbientLight` | — | `value` | |
| 74 | `AiTriggersOn` | `House` | — | Same as writing `AiTriggersActive` |
| 75 | `AiTriggersOff` | `House` | — | |
| 76 | `RatioAiTriggers` | `House` | `value` | |
| 77 | `RatioAircraft` | `House` | `value` | |
| 78 | `RatioInfantry` | `House` | `value` | |
| 79 | `RatioUnits` | `House` | `value` | |
| 80 | `ReinforceAt` | Cell | `id` TeamType | |
| 81 | `WakeUp` | Object (*live*) | — | Switches sleeping or harmless objects to Guard |
| 82 | `WakeSleepers` | `House` | — | Wakes sleeping objects outside this house |
| 83 | `WakeHarmless` | `House` | — | Wakes harmless objects belonging to this house |
| 84 | `WakeGroup` | — | `value` group | |
| 85 | `VeinGrowth` | — | `value` | |
| 86 | `OreGrowth` | — | `value` | |
| 87 | `IceGrowth` | — | `value` | |
| 88 | `CreateParticleSystem` | Cell | `id` particle system | |
| 89 | `RemoveParticleSystems` | Cell | — | |
| 90 | `LightningStrike` | Cell | — | |
| 91 | `GoBerserk` | Object (*live*, infantry) | — | |
| 92 | `FirestormOn` | `House` | — | `Unsupported` when Ares is disabled |
| 93 | `FirestormOff` | `House` | — | As above |
| 94 | `IonCannonStrike` | Cell | — | |
| 95 | `NukeStrike` | Cell | — | |
| 96 | `ChemStrike` | Cell | — | |
| 97 | `ToggleTrainCargo` | — | — | |
| 98 | `PlaySoundAtRandomWaypoint` | — | `id` sound | Ares variant, native RNG |
| 99 | `PlaySoundAt` | Cell | `id` sound | Plays at the cell center |
| 100 | `PlayRadarMovie` | — | `value` movie | |
| 101 | `ReshroudAt` | Cell | — | |
| 102 | `LightningStormStrike` | Cell | — | Uses the trigger house |
| 103 | `TimerText` | — | `text` CSF entry name | |
| 104 | `FlashTeam` | — | `id` TeamType or `other` team, `value` frames | |
| 105 | `TalkBubble` | — | `other` team (may be 0), `value` frames | |
| 106 | `SetTechLevel` | — | `id` type, `value` level | See also 7.7 |
| 107 | `ReinforceByChrono` | Cell | `id` TeamType | |
| 108 | `CreateCrate` | Cell | `value` kind (20 for random) | Random selection uses native RNG |
| 109 | `IronCurtainAt` | Cell | — | |
| 110 | `PauseGame` | — | `value` duration | |
| 111 | `EvictOccupants` | Object (*live*, building) | — | |
| 112 | `JumpCamera` | Cell | — | |
| 113 | `Cheer` | `House` | — | |
| 114 | `SetSidebarTab` | — | `value` 0..3 | |
| 115 | `FlashCameo` | — | `id` type, `value` duration | |
| 116 | `StopSoundsAt` | Cell | — | |
| 117 | `PlayRadarMovieAndPause` | — | `value` movie | |
| 118 | `ClearSmudges` | — | — | |
| 119 | `DestroyAll` | `House` | — | |
| 120 | `DestroyAllBuildings` | `House` | — | |
| 121 | `DestroyAllLandUnits` | `House` | — | |
| 122 | `DestroyAllNavalUnits` | `House` | — | |
| 123 | `MindControlBase` | `House` | `house` controlled house | |
| 124 | `RestoreMindControlledBase` | `House` | `house` controlled house | |
| 125 | `CreateBuildingAt` | Cell | `id` building type | Returns the building or 0 |
| 126 | `RestoreStartingTechnos` | `House` | — | |
| 127 | `ChronoScreenEffect` | — | `value` duration | |
| 128 | `TeleportAll` | `House` | `cell` destination | |
| 129 | `SuperWeaponSetCharge` | `House` | `id` superweapon type, `value` percentage | Same as writing `ChargePercent` |
| 130 | `RestoreStartingBuildings` | `House` | — | |
| 131 | `FlashBuildings` | `House` | `id` type, `value` duration | |
| 132 | `SuperWeaponSetRecharge` | `House` | `id` superweapon type, `value` frames | |
| 133 | `SuperWeaponResetRecharge` | `House` | `id` superweapon type | |
| 134 | `SuperWeaponReset` | `House` | `id` superweapon type | |
| 135 | `SetPreferredTargetCell` | `House` | `cell` | |
| 136 | `ClearPreferredTargetCell` | `House` | — | |
| 137 | `SetAlternateBase` | `House` | `cell` | |
| 138 | `ClearAlternateBase` | `House` | — | |
| 139 | `RadarBlackout` | `House` | `value` frames | |
| 140 | `SetDefensiveTargetCell` | `House` | `cell` | |
| 141 | `ClearDefensiveTargetCell` | `House` | — | |
| 142 | `RetintRed` | — | `value` | |
| 143 | `RetintGreen` | — | `value` | |
| 144 | `RetintBlue` | — | `value` | |
| 145 | `HomeCamera` | — | — | |

Ares actions return `Unsupported` when Ares is disabled.

| # | API name | Target | Parameters |
| --- | --- | --- | --- |
| 146 | `AddAuxPower` | `House` | `value` power |
| 147 | `KillDrivers` | `House` | — |
| 148 | `SetEvaVoice` | — | `value` |
| 149 | `SetGroup` | Object | `value` group |

### 5.3 Atomic actions

Numbers are the identity and are appended only. 1041 is reserved (`ApplyEffect`). 1043 and 1044 are reserved (`AttachRoutine`, `DetachRoutine`, engine-side micro routines, later). Code-only actions pass variable-length data through `action`. Actions not documented as returning a handle give a value of 0, and the status is returned by the same call. In version 1 `Modify` and `RemoveModifiers` apply only to objects.

| # | Name | Target | Parameters | Effect and nonzero result | RNG | Tier |
| --- | --- | --- | --- | --- | --- | --- |
| 1000 | `FireWeapon` | Object (shooter) | `id` weapon, or −1 to use slot `value` (default 0, primary). Aim at `cell`, then `other`, then the current target in that order. No target gives `Gone` | Fires, or returns `Refused` if unable to fire now | native | v1 |
| 1001 | `LaunchSuperWeapon` | `House` | `id` superweapon type, `cell` (required, omission is an invalid argument) | Launches this house's superweapon | native | v1 |
| 1002 | `SpawnAnim` | Cell | `id` animation | Creates an animation | native | v1 |
| 1003 | `AttachAnim` | Object | `id` animation, `value2` frames (0 one pass; positive keeps it looping that many frames; negative until `DetachAnim`) | Attaches the animation to the object (as team script action 51). A kept animation loops as a Phobos AttachEffect's does, follows the object, is taken off while the object is in a transport or fully cloaked and put back after, and goes when the object leaves the game. At 65,536 held animations in the match, further holds return `Refused` | native | preview |
| 1004 | `Remove` | Object | — | Removes without death, survivors or kill credit | none | v1 |
| 1005 | `Kill` | Object | `other` optional source | Kills through the damage path with damage equal to current health, ignoring defenses. Runs death animations and survivor logic, crediting the source | native | v1 |
| 1006 | `Damage` | Object | `id` warhead, `value` damage, `other` optional source | Applies warhead and armor rules with kill and experience credit | native | v1 |
| 1007 | `Heal` | Object | `value` health points | Restores health without kill credit | none | v1 |
| 1008 | `CreateObject` | Cell or factory building | `id` type, `house` owner, `arg` initial mission (`Mission`, 255 for default). Copy the tag from `other` if nonzero | Returns the new object on success. At a factory, the door opens and the unit exits toward the rally point, waiting if blocked. An incompatible factory or occupied target cell gives (0, Refused) | native | v1 |
| 1009 | `SetMission` | Object | `arg` mission, `cell` or `other` mission target | Queues a mission like a player order. `Move` with a cell moves there | none | v1 |
| 1010 | `Teleport` | Object | `cell` destination, `arg` occupied-cell policy (`Blocked`) | Chronoshift movement. `Nearby` chooses a nearby cell, `Fail` refuses | native | v1 |
| 1011 | `ClearTarget` | Object | — | Drops the current target | none | v1 |
| 1012 | `ClearDisguise` | Object | — | Removes a spy or Mirage Tank disguise | none | v1 |
| 1013 | `ClearMindControl` | Object | — | Releases mind control and restores the original owner | none | v1 |
| 1014 | `AddExperience` | Object | `value` experience points | Adds experience | none | v1 |
| 1015 | `AddMoney` | `House` | `value` amount (may be negative) | Adjusts money | none | v1 |
| 1016 | `AllowType` | `House` | `id` type, `value` 1 to allow or 0 to disallow | Updates the buildable list | none | v1 |
| 1017 | `SetOwner` | Object | `house` new owner | Transfers ownership, as when writing `Owner` | none | v1 |
| 1018 | `AddAmmo` | Object | `value` ammo | Adds ammo | none | v1 |
| 1019 | `AddPassenger` | Object (transport) | `other` passenger | Boards the passenger | native | v1 |
| 1020 | `RemovePassenger` | Object (transport) | `other` passenger | Unloads the passenger | native | later |
| 1021 | `SetProperty` | Object, `House` or cell | `id` property, `value` new value | Writes a writable property, as in `set` | none | v1 |
| 1022 | `Modify` | Object or `House` | `id` property, `arg` operation (`ModifierOp`), `arg2` group, `value` operand, `value2` frames, `other` optional source | Adds and returns a modifier, as in `modify` | none | v1 |
| 1023 | `RemoveModifiers` | Object or `House` | `arg2` group, restricted to property `id` unless −1 | Removes this module origin's modifiers in the group | none | v1 |
| 1024 | `SetVar` | Object, `House` or — | `id` variable, `value` new value | Writes as in `var_set`. Object scope uses the target object. House scope uses the `House` target (0 resolves as for `ActionParams.house`). Global scope ignores the target | none | v1 |
| 1025 | `DetonateWeapon` | Cell | `id` weapon, `other` optional source | Detonates the weapon at cell center without spawning a projectile | native | v1 |
| 1026 | `DetonateWarhead` | Cell | `id` warhead, `value` damage, `other` optional source | Applies warhead area damage | native | v1 |
| 1027 | `CreateProjectile` | Object or cell (origin) | `extra` one `ProjectileSpec`, `other` source for kill and experience credit (may be 0) | Creates and returns a projectile at the position given by `origin`, or 0 on failure. Code only | native | later |
| 1028 | `DetonateProjectile` | `Projectile` | — | Detonates immediately | native | later |
| 1029 | `RemoveProjectile` | `Projectile` | — | Removes without detonating | none | later |
| 1030 | `EndFor` | `House` | `arg` result (`Outcome`) | Makes this house win or lose | none | v1 |
| 1031 | `SetScore` | `House` | `value` score | Sets the score | none | v1 |
| 1032 | `AddScore` | `House` | `value` score | Adds to the score | none | v1 |
| 1033 | `Log` | — | `text` | Writes one line to the in-game log | none | v1 |
| 1034 | `Convert` | Object | `id` type, `value2` frames (0 for permanent, positive to revert on expiry) | Changes type using Ares conversion rules. Mutually exclusive with teleportation and forced motion | native | v1 |
| 1035 | `Push` | Object | `cell` directional target, `value` distance in leptons (negative pushes away; at most 2,048; 0 ends the push), `value2` frames (1..255; 0 or less for one per 64 leptons) | Pushes a vehicle or infantryman on a Drive, Ship, Walk, Mech or Hover locomotor along the ground in addition to its movement: each frame, at its locomotor's processing with no step in flight, a share of the distance moves it as a `MoveFrame` step does; a refused step ends the push. A new push replaces the old one; teleporting, converting or leaving ends it. Saved and hashed while it runs. `Refused` for other objects and while warping. Not launched into the air | native | preview |
| 1036 | `Module` | Any | `id` dispatch key, `text` argument text (optional) | Queues a module action for the module phase. The record's `target` is the trigger's target selection and `args` the string ID of the argument text. INI spelling is `Action=Wasm:name` or `Action=Wasm:name(target,text)` in named sections, action 1036 in maps (no arguments) | none | v1 |
| 1037 | `CreateTeamUnlimited` | `House` | `id` TeamType | Starts and returns a team without checking Max, like script action 18 | native | later |
| 1038 | `DisbandTeam` | — | `other` team | Destroys the team, records the AI trigger outcome and releases its members | none | later |
| 1039 | `AddTeamMember` | Object | `other` team | Joins a team subject to admission checks | none | later |
| 1040 | `RemoveTeamMember` | Object | — | Leaves the current team | none | later |
| 1042 | `RaiseEvent` | Object (object event) or any (global event) | `id` event ID. `other`, `value`, `value2`, `house` are written to the record | Raises module event `ModuleEvent` ([RFC 6h](0001-wasm-extension-modules.md#6h-ini-registration)). `Gone` if the target of an object event is not an existing object. An unregistered event ID is an invalid argument. INI spelling is `RaiseEvent(target,event name,value,value2)` | none | v1 |
| 1046 | `DetachAnim` | Object | `id` animation, or −1 for all (code only: INI names one animation) | Removes the animations this module origin's `AttachAnim` keeps on the object | none | preview |
| 1045 | `SetFacing` | Object | `value` body facing 0..255 | Turns the body through the active locomotor (rotating at its rate where the locomotor does, snapping where it snaps). `Refused` without a locomotor | none | preview |

### 5.4 Client action kinds

Run with `client_action(kind, target, params, len)`. Affects only the local machine and uses `ActionParams`. All are `v1` (phase 2) except `HomeCamera`, which is `later`.

| # | Name | Target | Parameters | Effect |
| --- | --- | --- | --- | --- |
| 0 | `PlaySound` | Cell or 0 | `id` sound | Plays locally. Target 0 disables positional sound |
| 1 | `PlaySpeech` | — | `text` EVA name | Plays local EVA speech |
| 2 | `CenterCamera` | Cell | `value` speed | Moves the local camera as in action 48 |
| 3 | `JumpCamera` | Cell | — | `Camera.jumpTo(cell)` jumps the local camera. Map action 112 keeps its original delivery scope |
| 4 | `HomeCamera` | — | — | Same as action 145 |
| 5 | `Select` | — | `extra` array of object handles, `value` 1 to append | Selects locally, accepting only objects this player can select |
| 6 | `FlashObject` | Object | `value` frames | Flashes the object locally |
| 7 | `ShowMessage` | — | `text` CSF entry name or literal, `value` color | Shows one line of text |
| 8 | `RegisterHotkey` | — | `text` hotkey name, `extra` default key chord as text, e.g. `Ctrl+Alt+K`, with modifiers and key name separated by `+` | Registers the hotkey. Get its ID with `client_lookup(Hotkey, name)`. If a game hotkey already uses the default chord, registration succeeds without binding the chord and returns `Refused`. Game hotkeys take priority |

## 6. AI teams

See [RFC 6d](0001-wasm-extension-modules.md#6d-ai). Team actions include 4, 5, 7, 80 and 107 in 5.2 and the team actions in 5.3. Team state is exposed through properties (7.9).

### 6.1 Definitions

See [RFC 6d](0001-wasm-extension-modules.md#6d-ai) for the read and write layers of the AI document. At runtime use the IDs returned by `lookup(TeamType, name)`, `lookup(ScriptType, name)` and `lookup(AiTrigger, name)` and the corresponding type references.

| Section | Contents | Notes |
| --- | --- | --- |
| `[TaskForces]` and each task force section | `Name`, `Group` and entries such as `0=count,type` | Up to 6 entries |
| `[ScriptTypes]` and each script section | `Name` and entries such as `0=action,argument` | Up to 50 entries. Action numbers are in 6.2 |
| `[TeamTypes]` and each team type section | `Name`, `Script`, `TaskForce`, `Priority`, `Max`, `Group`, `House`, `VeteranLevel`, `Waypoint`, `TransportWaypoint`, `IsBaseDefense`, `Suicide`, `Aggressive`, `UseTransportOrigin`, `Full`, `Whiner`, `Autocreate`, `AreTeamMembersRecruitable`, `Reinforce`, `AvoidThreats`, `OnTransOnly`, `Recruiter`, `Annoyance`, `GuardSlower`, `TransportsReturnOnUnload`, `MindControlDecision`, `Droppod`, `OnlyTargetHouseEnemy`, `Loadable`, `Tag` | Defaults come from the reader |
| `[AITriggerTypes]` | 18 fields per row, including name, primary team, house, condition, object type, count, comparison, starting/minimum/maximum weight, multiplayer, side, secondary team and easy/normal/hard flags | Writing resets current weight to the initial weight |

- The reader accepts `Whiner` but it has no effect. It ignores `TechLevel`, `Prebuild`, `LooseRecruit` and `IonImmune`. AI trigger fields 3, 11 and 13 are unused.
- AI trigger conditions: -1 always true, 0 enemy owns N of this type, 1 this house owns them, 2 enemy power surplus below 100, 3 below 0, 4 compare enemy money, 5 own Iron Curtain nearly ready, 6 own Chronosphere nearly ready, 7 civilian-side house owns the type. Comparators 0..5 follow `Compare` (8.3). Other values never match.
- Runtime changes to AI trigger weights are `later`.

### 6.2 Team script actions

Action numbers for lines in `[ScriptTypes]`. All are `v1` as data. The equivalent column gives the operation for a single object.

| # | `ScriptAction` | Parameters | Equivalent |
| --- | --- | --- | --- |
| 0 | `AttackQuarry` | Target category (2..11) | `SetMission` (`Hunt`) |
| 1 | `AttackWaypoint` | Waypoint | `SetMission` (`Attack`, cell) |
| 2 | `GoBerserk` (conventional name) | No operation, never completes | — |
| 3 | `MoveToWaypoint` | Waypoint | `SetMission` (`Move`, cell) |
| 4 | `MoveToCell` | Packed cell (x = arg%128, y = arg/128) | `SetMission` (`Move`, cell) |
| 5 | `Guard` | `arg*15` frames | `SetMission` (`Guard`) |
| 6 | `JumpToLine` | One-based line number | Team's `Cursor` property |
| 7 | `PlayerWins` | — | `EndFor` |
| 8 | `Unload` | 1, 2, 3 | `RemovePassenger` |
| 9 | `Deploy` | — | — |
| 10 | `FollowFriendlies` | — | — |
| 11 | `DoMission` | Mission ID | `SetMission` |
| 12 | `SetGlobal` | Index | `SetVar` (global variable) |
| 13 | `IdleAnimation` | Animation | — |
| 14 | `LoadOntoTransport` | — | `AddPassenger` |
| 15 | `SpyOnBuildingAtWaypoint` | Waypoint | — |
| 16 | `PatrolToWaypoint` | Waypoint | `SetMission` (`Patrol`) |
| 17 | `ChangeScript` | ScriptType index | Team's `Script` property |
| 18 | `ChangeTeam` | TeamType index | — |
| 19 | `Panic` | — | — |
| 20 | `ChangeHouse` | Country index | `SetOwner` |
| 21 | `Scatter` | — | — |
| 22 | `GoToNearbyShroud` | Refused outside single-player mode | — |
| 23 | `PlayerLoses` | — | `EndFor` |
| 24 | `PlaySpeech` (conventional name) | Only advances to the next line | — |
| 25 | `PlaySound` | SOUNDMD index | Stock action 19 |
| 26 | `PlayMovie` | Movie | Stock action 10 |
| 27 | `PlayMusic` | Music theme | Stock action 20 |
| 28 | `ReduceOre` | — | Stock action 59 |
| 29 | `BeginProduction` | — | Stock action 3 |
| 30 | `FireSale` | — | Stock action 9 |
| 31 | `SelfDestruct` | — | `Kill` |
| 32 | `IonStormStart` | Duration | Stock action 44 |
| 33 | `IonStormEnd` | — | Stock action 45 |
| 34 | `CenterCamera` | Speed | Stock action 48 |
| 35 | `ReshroudMap` | — | Stock action 51 |
| 36 | `RevealMap` | — | Stock action 16 |
| 37 | `DeleteMembers` | — | `Remove` |
| 38 | `ClearGlobal` | Index | `SetVar` (global variable) |
| 39 | `SetLocal` | Index | `SetVar` (local variable) |
| 40 | `ClearLocal` | Index | `SetVar` (local variable) |
| 41 | `Unpanic` | — | — |
| 42 | `ForceFacing` | Facing | — |
| 43 | `WaitUntilFullyLoaded` | — | — |
| 44 | `TruckUnload` | — | `Convert` |
| 45 | `TruckLoad` | — | `Convert` |
| 46 | `AttackEnemyBuilding` | Type `arg&0xFFFF`, mode `arg>>16` | — |
| 47 | `MoveToEnemyBuilding` | As above | — |
| 48 | `ScoutHouses` | — | — |
| 49 | `RegisterSuccess` | Advances and marks the team successful | Team's `Succeeded` property (read-only) |
| 50 | `Flash` | Frames | Stock action 104 |
| 51 | `PlayAnimation` | Low 16 bits are the animation, high 16 bits the loop count | `SpawnAnim` |
| 52 | `TalkBubble` | Frames | Stock action 105 |
| 53 | `GatherAtEnemyBase` | Ares: + `AISafeDistance` | — |
| 54 | `RegroupAtBase` | Ares: + `AIFriendlyDistance` | — |
| 55 | `IronCurtain` | Ares: `SW.Group` | `LaunchSuperWeapon` |
| 56 | `ChronoshiftToBuilding` | Same as 46 | `Teleport` |
| 57 | `ChronoshiftToQuarry` | Target category | `Teleport` |
| 58 | `MoveToOwnBuilding` | Type, mode | — |
| 59 | `AttackBuildingAtWaypoint` | Waypoint | — |
| 60 | `EnterGrinder` | — | — |
| 61 | `OccupyTankBunker` | — | — |
| 62 | `EnterBioReactor` | — | — |
| 63 | `OccupyBattleBunker` | — | — |
| 64 | `GarrisonNearest` | — | — |
| 65, 66 | — | Unused | — |
| 67 | `AresSeekVehicle` | Ares. Shares a branch with 64 but the step check makes it unreachable | — |
| 68 | `AresConvert` | Ares only | `Convert` |

## 7. Properties

Properties are grouped by reference kind. A property's ID is its position in the engine's property table, which lists each name once, at its first appearance. Names shared by several reference kinds (`Type`, `Name`, `X`, `Visible`, `Enabled` and others) share one ID. `lookup(Property, name)` returns the ID, and the Rust SDK has it as `Prop::<Name>`. Later additions are appended, so an ID does not follow table order. The 0.1.0 IDs:

| IDs | Section | Properties |
| --- | --- | --- |
| 0..37 | 7.1 Objects | 0 `Type`, 1 `Owner`, 2 `Category`, 3 `Health`, 4 `Strength`, 5 `HealthPercent`, 6 `X`, 7 `Y`, 8 `Z`, 9 `Cell`, 10 `Layer`, 11 `LandType`, 12 `Mission`, 13 `Target`, 14 `Rank`, 15 `Experience`, 16 `Ammo`, 17 `Storage`, 18 `Transport`, 19 `PassengerCount`, 20 `TransportSpaceLeft`, 21 `Team`, 22 `Tag`, 23 `DisguiseType`, 24 `DisguiseOwner`, 25 `Cloaked`, 26 `Moving`, 27 `Facing`, 28 `TurretFacing`, 29 `Armor`, 30 `Online`, 31 `Selected`, 32 `Relation`, 33 `NearbyEnemyCount`, 34 `TargetInRange`, 35 `CanFireAtTarget`, 36 `DoorOpen`, 37 `Animating` |
| 38..65 | 7.2 Modifiable properties | 38 `ArmorMultiplier`, 39 `FirepowerMultiplier`, 40 `SpeedMultiplier`, 41 `ROFMultiplier`, 42 `WeaponRangeMultiplier`, 43 `WeaponRangeExtra`, 44 `TargetPriorityMultiplier`, 45 `CritMultiplier`, 46 `CritExtraChance`, 47 `DamageReceivedExtra`, 48 `ReflectDamagePercent`, 49 `RevengeWeapon`, 50 `PrimaryWeapon`, 51 `SecondaryWeapon`, 52 `Unkillable`, 53 `Cloakable`, 54 `ForceDecloak`, 55 `DisableWeapons`, 56 `DisableTargeting`, 57 `Selectable`, 58 `IgnoreOrders`, 59 `Targetable`, 60 `PenetratesForceShield`, 61 `ImmunityFlags`, 62 `TintColor`, 63 `TintIntensity`, 64 `LaserTrailType`, 65 `PowerOutage` |
| 66..72 | 7.3 Modifiers | 66 `Property`, 67 `Op`, 68 `Value`, 69 `Remaining`, 70 `Source`, 71 `Group`, 72 `Origin` |
| 73..77 | 7.4 Projectiles | 73 `VX`, 74 `VY`, 75 `VZ`, 76 `Data0`, 77 `Data1` |
| 78..92 | 7.5 Houses | 78 `Name`, 79 `Country`, 80 `Side`, 81 `Index`, 82 `Money`, 83 `PowerOutput`, 84 `PowerDrain`, 85 `LowPower`, 86 `TechLevel`, 87 `IsDefeated`, 88 `IsHuman`, 89 `IsCivilian`, 90 `Kills`, 91 `Losses`, 92 `AiTriggersActive` |
| 93..99 | 7.6 Cells | 93 `Height`, 94 `Overlay`, 95 `Ore`, 96 `Revealed`, 97 `Placeable`, 98 `CellTag`, 99 `Waypoint` |
| 100..131 | 7.7 Types and rules | 100 `Buildable`, 101 `Enabled`, 102 `Damage`, 103 `ROF`, 104 `Burst`, 105 `MinimumRange`, 106 `Range`, 107 `CellSpread`, 108 `PercentAtMax`, 109 `Wall`, 110 `Wood`, 111 `Sparky`, 112 `Fire`, 113 `Conventional`, 114 `Rocker`, 115 `InfDeath`, 116 `Cost`, 117 `Speed`, 118 `ROT`, 119 `Points`, 120 `Soylent`, 121 `Crushable`, 122 `Selfhealing`, 123 `Explodes`, 124 `Power`, 125 `Inaccurate`, 126 `Arcing`, 127 `Shadow`, 128 `Sight`, 129 `InitialAmmo`, 130 `Passengers`, 131 `SizeLimit` |
| 132..140 | 7.8 Global references | 132 `Frame`, 133 `Difficulty`, 134 `GameMode`, 135 `PlayerCount`, 136 `MissionTimer`, 137 `MissionTimerRunning`, 138 `AmbientLight`, 139 `IonStormActive`, 140 `GameSpeed` |
| 141..150 | 7.9 Triggers, tags, teams and superweapons | 141 `House`, 142 `Repeat`, 143 `MapId`, 144 `Script`, 145 `Cursor`, 146 `Succeeded`, 147 `MemberCount`, 148 `Ready`, 149 `ChargePercent`, 150 `RechargeTime` |
| 151..172 | 7.10 Client nodes and client globals | 151 `Label`, 152 `Art`, 153 `Width`, 154 `Hold`, 155 `Gap`, 156 `Visible`, 157 `Dismissable`, 158 `Clickable`, 159 `Hoverable`, 160 `Items`, 161 `ItemProgress`, 162 `ItemTooltips`, 163 `Fill`, 164 `Anchor`, 165 `Palette`, 166 `Tint`, 167 `Anim`, 168 `HideAnchor`, 169 `Number`, 170 `Viewer`, 171 `IsSpectator`, 172 `CaptureMapClick` |
| 173..180 | 7.6 Cells: `Passable.<SpeedType>` in `SpeedType` order | 173 `Passable.Foot`, 174 `Passable.Track`, 175 `Passable.Wheel`, 176 `Passable.Hover`, 177 `Passable.Winged`, 178 `Passable.Float`, 179 `Passable.Amphibious`, 180 `Passable.FloatBeach` |
| 181..187 | 7.1 Objects (RFC 6i) | 181 `Destination`, 182 `WeaponReady`, 183 `Deployed`, 184 `UnderConstruction`, 185 `Parasite`, 186 `ControlGroup`, 187 `Engage` |
| 188..212 | 7.5 Houses (RFC 6i) | 188 `ControlDomains`, 189 `Factory.Building.Type`, 190 `Factory.Building.Progress`, 191 `Factory.Building.Ready`, 192 `Factory.Building.Queued`, 193 `Factory.Defense.Type`, 194 `Factory.Defense.Progress`, 195 `Factory.Defense.Ready`, 196 `Factory.Defense.Queued`, 197 `Factory.Infantry.Type`, 198 `Factory.Infantry.Progress`, 199 `Factory.Infantry.Ready`, 200 `Factory.Infantry.Queued`, 201 `Factory.Vehicle.Type`, 202 `Factory.Vehicle.Progress`, 203 `Factory.Vehicle.Ready`, 204 `Factory.Vehicle.Queued`, 205 `Factory.Naval.Type`, 206 `Factory.Naval.Progress`, 207 `Factory.Naval.Ready`, 208 `Factory.Naval.Queued`, 209 `Factory.Aircraft.Type`, 210 `Factory.Aircraft.Progress`, 211 `Factory.Aircraft.Ready`, 212 `Factory.Aircraft.Queued` |
| 213..226 | 7.6 Cells (RFC 6i): `Zone.<MovementZone>` in `MovementZone` order, then `Obstructed` | 213 `Zone.Normal`, 214 `Zone.Crusher`, 215 `Zone.Destroyer`, 216 `Zone.AmphibiousDestroyer`, 217 `Zone.AmphibiousCrusher`, 218 `Zone.Amphibious`, 219 `Zone.Subterrannean`, 220 `Zone.Infantry`, 221 `Zone.InfantryDestroyer`, 222 `Zone.Fly`, 223 `Zone.Water`, 224 `Zone.WaterBeach`, 225 `Zone.CrusherAll`, 226 `Obstructed` |
| 227 | 7.8 Global references (RFC 6i) | 227 `AutomationAllowed` |
| 228 | 7.2 Modifiable properties (0.0.23) | 228 `ROTMultiplier` |
| 229 | 7.10 Client nodes (0.0.25) | 229 `Backdrop` |
| 230 | 7.1 Objects (0.2.0) | 230 `ImageOverride` |

`TargetArmor` (7.2, preview) has no ID: it is not implemented in 0.1.0.

The columns use the following conventions.

- **Value** uses the [value types](#values).
- **Access:** `R` is read-only. `W` writes through the owner. `W(rules)` writes only in `on_rules` (`set` on type or global rules references equals `rules_set`). In version 1, types and global rules are not writable during a match ([RFC 8](0001-wasm-extension-modules.md#8-changing-rules)). An unwritable property returns `Refused`.
- **Client** ([RFC 10](0001-wasm-extension-modules.md#10-client-modules)): `all` uses simulation semantics. `own` covers the local house and its objects, returning an empty value and `Refused` for others. `seen` includes all owned objects and currently visible foreign objects subject to RFC 10. Other objects appear absent. `sim` is simulation-only and returns an empty value and `Unsupported` on the client.
- **Viewpoint** properties depend on a house: the trigger house for filter clauses, `viewpoint` for queries and the local player on the client. Direct `get` returns `Unsupported` ([RFC 6c](0001-wasm-extension-modules.md#6c-objects-properties-and-modifiers)). **Derived** properties are computed and read-only. Those marked “derived, high” require a search or range check, charged to the owning module's budget when evaluated in a filter clause ([RFC 15](0001-wasm-extension-modules.md#15-performance)). Other derived properties cost the same as a field read.

### 7.1 Objects

Properties of a `Techno`. Only the final row supports modifiers.

| Property | Value | Access | Client | Tier | Notes |
| --- | --- | --- | --- | --- | --- |
| `Type` | ref (type) | R | seen (disguised enemies expose the disguise type) | v1 | Change type with `Convert` |
| `Owner` | ref (house) | W | seen (disguised enemies expose the disguise house) | v1 | Writing is equivalent to `SetOwner` |
| `Category` | enum `Category` | R | seen | v1 |  |
| `Health` | int | W | seen | v1 | Writing sets health without kill credit |
| `Strength` | int | R | seen | v1 | Maximum health |
| `HealthPercent` | int | R (derived) | seen | v1 |  |
| `X`, `Y`, `Z` | lepton | R | seen | v1 |  |
| `Cell` | ref (cell) | R | seen | v1 |  |
| `Layer` | enum `Layer` | R | own | v1 |  |
| `LandType` | enum `LandType` | R (derived) | own | v1 | Terrain type of the current cell |
| `Mission` | enum `Mission` | W | own | v1 | Writing queues an untargeted mission like a player order |
| `Target` | ref | R | own | v1 | Object or cell. Clear with `ClearTarget` |
| `Rank` | enum `Rank` | W | own | v1 | Writing sets the rank |
| `Experience` | int | R | own | v1 | Increase with `AddExperience` |
| `Ammo` | int | W | own | v1 |  |
| `Storage` | int | R | own | v1 | Ore carried |
| `Transport` | ref | R | own | v1 | Containing transport or building, or 0. Find passengers with a `Transport = t` clause |
| `PassengerCount` | int | R | own | v1 |  |
| `TransportSpaceLeft` | int | R (derived) | own | v1 |  |
| `Team` | ref (team) | R | sim | later | Join and leave with team actions |
| `Tag` | ref (tag) | W | sim | later | Writing an existing tag replaces the old tag ([RFC 7](0001-wasm-extension-modules.md#7-runtime-triggers-and-tags)) |
| `DisguiseType`, `DisguiseOwner` | ref | R | own | v1 | Simulation reads the actual state. 0 if undisguised |
| `Cloaked` | bool | R | own | v1 | Fully cloaked (cloaking and uncloaking read 0) |
| `Moving` | bool | R | own | v1 |  |
| `Facing` | int 0..255 | R | own | v1 | Body facing, in the same units as map unit facing and the `ForceFacing` script action |
| `TurretFacing` | int 0..255 | R | own | v1 | Turret facing in `Facing` units. `Unsupported` without a turret |
| `Armor` | id (`Armor`) | R | own | v1 |  |
| `Online` | bool | W | own | v1 | Buildings, equivalent to stock actions 61 and 62 |
| `Destination` | ref | R | own | v1 | Move destination (cell or object), or 0 |
| `WeaponReady` | bool | R | own | v1 | Primary weapon cooldown finished |
| `Deployed` | bool | R | own | v1 | Deployed |
| `UnderConstruction` | bool | R | seen | v1 | Building is in its construction animation |
| `Parasite` | ref | R | own | v1 | Parasite unit attached to it, or 0 |
| `ControlGroup` | int | R | Client only | v1 (phase 2) | Local control group number, or −1. `Unsupported` in the simulation |
| `Visible` | bool | R (viewpoint) | own | v1 | The viewpoint house can see it at this moment: fog, cloaking and detection consistent with that player's client ([RFC 6i](0001-wasm-extension-modules.md#6i-driving-a-house)) |
| `Engage` | enum `EngageResult` | R (viewpoint object, derived, high) | own | v1 | Whether the viewpoint object as shooter can strike it. The viewpoint must be an object, otherwise `Unsupported`. Reuses the engine's fire permission and damage checks |
| `Selected` | bool | R | Client only | v1 (phase 2) | Local selection. `Unsupported` in the simulation |
| `Relation` | enum `Relation` | R (viewpoint) | own | v1 | Relationship to the viewpoint house |
| `NearbyEnemyCount` | int | R (derived, high) | own | v1 | Number of enemy objects within sight (`Sight`) |
| `TargetInRange` | bool | R (derived, high) | own | v1 | Current target is within range of the weapon the object selects against it (the primary unless the target calls for the secondary) |
| `CanFireAtTarget` | bool | R (derived, high) | own | v1 | Can fire at the current target now |
| `DoorOpen` | bool | W | seen | v1 (writing preview) | Whether the factory exit door is open. `Unsupported` for buildings without an exit door and for other objects. Writing 1 or 0 holds a war factory's or gate's door drawn open or shut, −1 lets it show its own state again; the door, production, the exit and a gate's passage run on, and a read still answers the door's own state. A vehicle's door has no picture: `Unsupported` |
| `Animating` | bool | W | seen | v1 (writing later) | Whether the building's working animation is active. `Unsupported` for other objects. Writing changes the displayed state without starting production or dispatching a unit |
| `ImageOverride` | ref (type) | W | seen | preview | The TechnoType the object is drawn as, 0 for its own: its model or infantry image, palette and house colour as that type's; what the object is and does stays its own. Only a type of the object's category, and no building; another returns `Refused`. Kept through a conversion, dropped when the object leaves the game. Saved and hashed. An infantry image draws with the object's own art `Sequence=` frames, so one of another sequence may show a wrong frame or none in some poses |
| Modifiable properties | See 7.2 | | | | Modifiable |

### 7.2 Modifiable properties

Base values come from the type or a default. Modifiers determine the effective value ([RFC 6c](0001-wasm-extension-modules.md#6c-objects-properties-and-modifiers)). Properties with a Phobos key use the Phobos composition rules. Write with `modify` only (`set` returns `Refused`). Client access is `own` unless noted.

| Property | Value | Base value | Allowed operations | Meaning | Phobos key | Tier |
| --- | --- | --- | --- | --- | --- | --- |
| `ArmorMultiplier` | fixed | 1.0 | Add, multiply, override | Incoming damage multiplier | `ArmorMultiplier` (warhead filters such as `.AllowWarheads` and `.DisallowWarheads` apply only to Phobos effect types) | v1 |
| `TargetArmor` | id (`Armor`) | Type's armor | Override | Armor used for target selection, threat scoring, weapon selection and fire permission. Does not affect damage settlement. Not implemented in 0.1.0 (no property ID) | — | preview |
| `FirepowerMultiplier` | fixed | 1.0 | Add, multiply, override | Firepower multiplier | `FirepowerMultiplier` | v1 |
| `SpeedMultiplier` | fixed | 1.0 | Add, multiply, override | Speed multiplier. Flying, jumpjet and rocket movement, which read their type speed, scale it by the module modifiers alone | `SpeedMultiplier` | v1 |
| `ROTMultiplier` | fixed | 1.0 | Add, multiply, override | Body turn rate (`ROT=` of the PrimaryFacing) of vehicles, ships and buildings. Applied when the object's modifiers change, so a turn in progress keeps its end frame. Not turrets, aircraft or jumpjets | — | preview |
| `ROFMultiplier` | fixed | 1.0 | Add, multiply, override | Rate-of-fire multiplier | `ROFMultiplier`, `ROFMultiplier.ApplyOnCurrentTimer` | v1 |
| `WeaponRangeMultiplier` | fixed | 1.0 | Add, multiply, override | Range | `WeaponRange.Multiplier` | v1 |
| `WeaponRangeExtra` | lepton | 0 | Add, multiply, override | Range | `WeaponRange.ExtraRange` | v1 |
| `TargetPriorityMultiplier` | fixed | 1.0 | Add, multiply, override | Target priority multiplier (used with `TargetChanged`) | — | later |
| `CritMultiplier` | fixed | 1.0 | Add, multiply, override | Critical hit | `Crit.Multiplier` | later (0.1.0: available in Phobos builds only) |
| `CritExtraChance` | fixed | 0 | Add, multiply, override | Critical hit | `Crit.ExtraChance` | later (0.1.0: available in Phobos builds only) |
| `DamageReceivedExtra` | int | 0 | Add, multiply, override | Flat incoming damage adjustment | — | later |
| `ReflectDamagePercent` | fixed | 0 | Add | Reflected damage | `ReflectDamage`, `ReflectDamage.*` | later |
| `RevengeWeapon` | id (weapon) | −1 | Override | Revenge weapon | `RevengeWeapon`, `RevengeWeapon.*` | later |
| `PrimaryWeapon`, `SecondaryWeapon` | id (weapon) | Type's weapon | Override | Replacement weapon | — | later |
| `Unkillable` | bool | 0 | Override | Prevents death | `Unkillable` | later (0.1.0: available in Phobos builds only) |
| `Cloakable` | bool | Type's `Cloakable` | Override | Cloaking | `Cloakable` | v1 |
| `ForceDecloak` | bool | 0 | Override | Cloaking | `ForceDecloak` | v1 |
| `DisableWeapons` | bool | 0 | Override | Disables firing | `DisableWeapons` | v1 |
| `DisableTargeting` | bool | 0 | Override | Stops target acquisition | — | later |
| `Selectable` | bool | 1 | Override | 0 prevents selection | — | later |
| `IgnoreOrders` | bool | 0 | Override | Ignores player orders | — | later |
| `Targetable` | bool | 1 | Override | 0 prevents targeting | — | later |
| `PenetratesForceShield` | bool | 0 | Override | Penetrates force shields | `PenetratesForceShield` | later |
| `ImmunityFlags` | mask | Type's immunities (`ImmuneToPsionics=`, `ImmuneToRadiation=`, `ImmuneToPoison=`, `ImmuneToPsionicWeapons=`, `Warpable=no`; with Ares the rank abilities, `ImmuneToEMP=` and its default, and `ImmuneToBerserk=`; without Ares berserk follows `ImmuneToPsionics=`) | Override only, bits 0..6; other operations or bits return `Refused` | Immunity bits: 0 mind control, 1 radiation, 2 poison, 3 psychic, 4 chrono, 5 EMP, 6 berserk. The override replaces, bit by bit, the immunity each check computes, so it can also remove one: 0 capture by mind control, the Psychic Dominator, Ares permanent mind control and abduction ownership; 1, 2 and 3 the receive-damage gates of radiation, poison and psychic warheads, and 1 also radiation-field damage; 4 the Chrono Legionnaire's warp and the Chronosphere's `Warpable=` test; 5 Ares EMP; 6 the Psychedelic (berserk) gate and the fire check for gas | — (a zero multiplier with `ArmorMultiplier.AllowWarheads` covers some cases) | preview |
| `TintColor` | int (`0xRRGGBB`) | 0 | Override only; other operations or bits return `Refused` | Tint, the same for every viewer; client access `seen`. Its RGB565 word is ORed into the Phobos shield and AttachEffect tints | `Tint.Color` | preview |
| `TintIntensity` | fixed | 0 | Add only; other operations return `Refused` | Tint intensity (1.0 full), added to the Phobos tints' intensity; client access `seen` | `Tint.Intensity`, `Tint.Cumulative` | preview |
| `LaserTrailType` | id | −1 | Override | Laser trail, client access `seen` | `LaserTrail.Type` | later |
| `PowerOutage` | bool | 0 | Override | House power outage (house property, 7.5) | — | later |

For attached animations, timed conversions, pushes and teleportation, see `AttachAnim`, `Convert`, `Push` and `Teleport` in [5.3](#53-atomic-actions).

**Phobos AttachEffect keys** are read only when Phobos is enabled, follow the implementation at Phobos commit `31ae9f85` and are handled per instance by the AttachEffect owner. They are not mapped to modifiers.

| Phobos key | Meaning |
| --- | --- |
| `Duration`, `AttachEffect.DurationOverrides` | Instance duration, negative for indefinite. A refresh without an override keeps the instance's own. Counts down on the object's turn and pauses off-map, frozen or in a transport. |
| `Cumulative=no`, `AttachEffect.ReplaceLongerDuration` | One instance per type. Refresh when the new duration is not shorter or replacement is allowed. |
| `Cumulative`, `Cumulative.MaxCount` | Stacks per type, unlimited by default. When full, refresh the eligible instance with the least time left. |
| `AttachEffect.CumulativeSourceMaxCount` | Stacks per (invoker, source warhead or object) |
| `AttachEffect.CumulativeRefreshAll`, `.OnAttach`, `AttachEffect.CumulativeRefreshSameSourceOnly` | Refresh all eligible instances. With `.OnAttach` refresh and add even below the cap. By default refresh only the same source. |
| `AttachEffect.Delays`, `AttachEffect.InitialDelays`, `AttachEffect.RecreationDelays` | Recreation delay after expiry, initial delay and recreation delay after removal. None is a cooldown. |
| `Groups`, `RemoveTypes`, `RemoveGroups` | Labels and removal, not modifier exclusion groups. Warheads attach before removing. |
| `RequiredGroups`, `DisallowedGroups`, `IgnoreFromSameSource`, `SameSourceOnly` | Weapon target filters that do not affect application |
| `AllowTransfer`, `AllowTransfer.Convert`, `AttachEffects.AttachOnOwnerChange` | Affect only Phobos effects |
| `PenetratesIronCurtain`, `Powered`, `DiscardOn`, `DiscardOn.*` | Instance admission, activity and removal |
| `ExpireWeapon*`, `Animation*`, `CumulativeAnimations*` | Instance expiry weapons and animations |

Properties with a dash in the Phobos column become available when the module feature is enabled ([RFC 14](0001-wasm-extension-modules.md#14-project-contract)).

### 7.3 Modifiers

The handle returned by `modify` expires on removal or expiry. All properties are `R`, client `sim`, tier `v1`. List modifiers in insertion order with `find` (scope `Modifiers`, clause `Target = ref`).

| Property | Value | Notes |
| --- | --- | --- |
| `Target` | ref | Modified object or house |
| `Property` | int | Property ID |
| `Op` | enum `ModifierOp` | |
| `Value` | int | Value as supplied |
| `Remaining` | frames | Frames remaining, −1 for unlimited |
| `Source` | ref | Source object, possibly 0 |
| `Group` | int | 0 means ungrouped |
| `Origin` | enum `Source` | Its origin: mod, map or INI behavior. Phobos effects are not modifiers. |

### 7.4 Projectiles

All are `preview`. A projectile reference is an object handle (kind 0) sharing the creation serial with units; `find` lists no projectiles.

| Property | Value | Access | Client |
| --- | --- | --- | --- |
| `Type` | ref (projectile type) | R | seen |
| `Category` | enum `Category`, always `Projectile` | R | seen |
| `X`, `Y`, `Z` | lepton | R | seen |
| `VX`, `VY`, `VZ` | fixed velocity with 16 fractional bits, in leptons per frame | R | own |
| `Source` | ref | R | own |
| `Target` | ref | R | own |
| `Data0`, `Data1` | int, 16 bytes of module data. `Data0` contains bytes 0..7 and `Data1` bytes 8..15, little-endian. Saved and included in the hash | W | sim |

### 7.5 Houses

| Property | Value | Access | Client | Tier | Notes |
| --- | --- | --- | --- | --- | --- |
| `Name` | string | R | all | later |  |
| `Country` | ref (country type) | R | all | v1 | Multiple houses may share a country. Page by house index |
| `Side` | int | R | all | later |  |
| `Index` | int | R | all | v1 | House array index |
| `Money` | int | R | own | v1 | Change with `AddMoney` |
| `PowerOutput`, `PowerDrain` | int | R | own | v1 |  |
| `LowPower` | bool | R | own | v1 |  |
| `TechLevel` | int | R | own | v1 |  |
| `IsDefeated`, `IsHuman`, `IsCivilian` | bool | R | all | v1 | `IsCivilian` is not implemented in 0.1.0 (returns `Unsupported`) |
| `Kills`, `Losses` | int | R | own | later | Statistic |
| `AiTriggersActive` | bool | W | sim | later | Same as stock actions 74 and 75 |
| `ControlDomains` | mask (`Domain`) | W | sim | v1 | Claimed domains ([RFC 6i](0001-wasm-extension-modules.md#6i-driving-a-house)). `Refused` for a human house, a defeated house or a bit claimed by another module. Saved and hashed |
| `Factory.<Kind>.Type` | ref | R | own | v1 | `<Kind>` is `Building`, `Defense`, `Infantry`, `Vehicle`, `Naval` or `Aircraft`. Type in production, or 0 |
| `Factory.<Kind>.Progress` | int 0..100 | R | own | v1 |  |
| `Factory.<Kind>.Ready` | bool | R | own | v1 | Finished, awaiting placement or exit |
| `Factory.<Kind>.Queued` | int | R | own | v1 | Number of queued entries |
| `Relation` | enum `Relation` | R (viewpoint) | all | v1 | List allies and enemies with a `Relation` clause in `find` |
| `PowerOutage` | bool | R | own | later | Modifiable (7.2) |

### 7.6 Cells

| Property | Value | Access | Client | Tier | Notes |
| --- | --- | --- | --- | --- | --- |
| `LandType` | enum `LandType` | R | all | v1 |  |
| `Height` | int | R | all | v1 |  |
| `Overlay` | ref (overlay type) | R | seen (currently visible cells) | v1 | 0 if absent |
| `Ore` | int | R | seen | v1 |  |
| `Passable.<SpeedType>` | bool | R | seen | v1 | Terrain only, ignoring objects and overlays. One per `SpeedType`, e.g. `Passable.Foot`, `Passable.Track` |
| `Revealed` | bool | R (viewpoint) | own | v1 | Whether the viewpoint house has revealed the cell |
| `Placeable` | bool | R (viewpoint) | own | v1 | Whether the viewpoint house can place the query's `type` here. Valid in `find` and `client_find`. Elsewhere returns `Unsupported` |
| `CellTag` | ref (tag) | R | sim | v1 | 0 if absent |
| `Waypoint` | int | R | all | v1 | Waypoint number at this cell, or −1 if absent |
| `Zone.<MovementZone>` | int | R | all | v1 | Connected-area number for this movement zone: two cells with equal numbers are mutually reachable. One per `MovementZone` |
| `Obstructed` | bool | R | seen | v1 | Occupied by a building, wall or impassable overlay |

### 7.7 Types and rules

Typed forms of common INI keys (parsed values). Use `rules_get` for others ([RFC 8](0001-wasm-extension-modules.md#8-changing-rules)). Client access is `all`. All are read-only during a match in version 1. See [section 9](#9-version-1-inventory) for tiers.

| Property | Value | Type | Access |
| --- | --- | --- | --- |
| `Name` | string | Any type | R |
| `Category` | enum `Category` | TechnoType | R |
| `Buildable` | bool | TechnoType | R (viewpoint), the viewpoint house meets the prerequisites |
| `Enabled` | bool | AI trigger | W, enable or disable |

In the next two tables, keys with a value type have a property of the same name, `R` during a match. List and string keys use only `rules_get` and `rules_set`.

**Later allowlist candidates** (read-only in version 1, each key must first meet the conditions in [RFC 8](0001-wasm-extension-modules.md#8-changing-rules)):

| Type | Property (key) | Existing objects | Notes |
| --- | --- | --- | --- |
| Weapon | `Damage` int, `ROF` frames, `Burst` int, `MinimumRange` lepton | Use the new value on next use | |
| Weapon | `Range` lepton | As above | Only for projectiles with `ROT>0`. With `ROT=0`, writes are limited to `on_rules` |
| Warhead | `Verses` | As above | List, write only through `rules_set` |
| Warhead | `CellSpread` lepton, `PercentAtMax` fixed, `Wall`, `Wood`, `Sparky`, `Fire`, `Conventional`, `Rocker` bool, `InfDeath` int | As above | `AnimList` uses only `rules_set` |
| TechnoType | `BuildTime`-class multipliers fixed | Computed on demand | |
| TechnoType | `Speed` int (INI value) | Convert on demand | Check the state of each locomotion kind |
| TechnoType | `Soylent` int, `Selfhealing`, `Explodes` bool, `VeteranAbilities`, `EliteAbilities` | Same | Ability lists use only `rules_set` |
| TechnoType | `Power` int | Recalculate power for every house that owns the type | |
| Global | Numeric fields in `[General]`, `[CombatDamage]` and `[AudioVisual]` | As above | Global rules reference properties or `rules_set`. Integers use int, decimals fixed, cell distances lepton and time frames. Repeat any frame conversion |

**Copied to each object at construction** (read-only until an existing-object policy is specified for allowlisting)

| Key | Required policy |
| --- | --- |
| `Cost` int | Paid production in progress and refunds: otherwise cancelling after a price change creates money |
| `ROT` int | Update each object's facing controller |
| `Crushable` bool | Update each object's copy |
| `Strength` int | Preserve health points or health fraction |
| `Sight` int (cells) | Update each object and its revealed area |
| `Ammo`, `InitialAmmo` int | Change each object's capacity |
| `Storage` int | Update capacity |
| `Passengers`, `SizeLimit` int | Update capacity and define overflow handling |
| Phobos `AttachEffect.*` on a type | Affects new objects only |

**Only in `on_rules`** (`W(rules)`)

| Key | In-match alternative |
| --- | --- |
| Weapon `Range`, `[General] Gravity`, projectile `ROT`, `Floater`, Phobos projectile `Gravity` | |
| `Primary`, `Secondary`, `Weapon{n}`, `EliteWeapon{n}`, `OccupyWeapon`, weapon `Warhead`/`Projectile` | Use `PrimaryWeapon` and `SecondaryWeapon` modifiers on individual objects |
| `Prerequisite`, `PrerequisiteOverride`, `[General] Prerequisite*`, Ares `Prerequisite.List{n}` | |
| `Owner`, `RequiredHouses`, `ForbiddenHouses` | |
| Weapon `Report`, `Anim`, warhead `AnimList`, projectile `Inaccurate`, `Arcing`, `Shadow` | |
| `Factory` | |
| `Foundation`, `[AI] AIBaseSpacing`, `BuildConst`, `IsBaseDefense`, `UndeploysInto`, `Spawned`/`MissileSpawn` | |
| `Spawns`, `SpawnsNumber`, mind-control and temporal weapon settings | |
| `Armor` (name) | |
| `Image`, art keys, `Sequence` | |
| Ares `PoweredBy`, `PowersUnit`, `PoweredUnit` | |
| List sections and any key referring to a new type | |
| `[Colors]`, country `Color`, `Side`, `ParentCountry` | |
| Sound and EVA keys | |
| `TechLevel` | `SetTechLevel` (action 106) |
| `[TaskForces]`, `[ScriptTypes]`, `[TeamTypes]`, `[AITriggerTypes]` | Also `Refused` in `on_rules` in version 1 ([RFC 6d](0001-wasm-extension-modules.md#6d-ai)) |

Ares and Phobos keys have yet to be classified.

### 7.8 Global references

Global rules reference (kind 4) for scenario and global state.

| Property | Value | Access | Client | Tier |
| --- | --- | --- | --- | --- |
| `Frame` | int | R | all | v1 |
| `Difficulty` | enum `Difficulty` | R | all | v1 |
| `GameMode` | string | R | all | v1 |
| `PlayerCount` | int | R | all | v1 |
| `MissionTimer` | frames | R | all | v1 (change with stock actions 23 through 27) |
| `MissionTimerRunning` | bool | R | all | v1 |
| `AmbientLight` | int | R | all | v1 |
| `IonStormActive` | bool | R | all | v1 |
| `GameSpeed` | int | R | all | v1 |
| `AutomationAllowed` | bool | R | all | v1 |
| Numeric global keys | See 7.7 | R | all | later |

`GameSpeed` uses the game speed levels of the game options. Read-only in version 1.

### 7.9 Triggers, tags, teams and superweapons

Client access is `sim` for triggers, tags and teams, and `own` for superweapons.

| Reference | Property | Value | Access | Tier | Notes |
| --- | --- | --- | --- | --- | --- |
| Trigger | `Enabled` | bool | W | v1 | Writing 1 is equivalent to stock action 53, writing 0 to action 54. Affects every instance |
| Trigger | `House` | ref | R | v1 | 0 selects the subject's house at firing time, or the neutral house if there is no subject |
| Trigger | `Repeat` | enum `TagRepeat` | R | v1 |  |
| Trigger | `MapId` | string | R | later | Map trigger ID. Look up with a `MapId` clause in `find` |
| Tag | `MapId` | string | R | later |  |
| Tag | `Repeat` | enum `TagRepeat` | R | later |  |
| Team | `Type` | ref (TeamType) | R | later |  |
| Team | `House` | ref | R | later |  |
| Team | `Script` | ref (ScriptType) | W | later | Changes the running team's script |
| Team | `Cursor` | int | W | later | Script cursor. Writing jumps to that line |
| Team | `Succeeded` | bool | R | later |  |
| Team | `MemberCount` | int | R | later | List members using `find` with a `Team = t` clause |
| Superweapon | `Type` | ref | R | v1 |  |
| Superweapon | `Owner` | ref | R | v1 |  |
| Superweapon | `Ready` | bool | R | v1 |  |
| Superweapon | `ChargePercent` | int | W | v1 | Same as stock action 129 |
| Superweapon | `RechargeTime` | frames | W | later | Same as stock action 132 |

### 7.10 Client nodes

Client-only references, writable with `client_set` and `client_set_bytes`. They have no simulation representation.

**Node kinds** (`NodeKind`). Version 1 can create `Text`, `Button`, `Row`, `Column`, `Image`, `Bar` and `List`; 0.2.0 adds `Decoration` and `MapMarker`, which are top level only and drawn over the battlefield rather than laid out (`X`, `Y` are pixels from their anchor; the battlefield layer draws after the objects, not at their depth). Only INI creates a `CameoGrid`: modules can find and read it and write its common properties and `ItemTooltips`, and `client_node_create(CameoGrid)` returns `Unsupported`. Coordinates are sidebar pixels in the sidebar and screen pixels in the battlefield and overlay.

| # | Kind | `Type` in `veraui.ini` | Tier |
| --- | --- | --- | --- |
| 0 | `Text` | `Text` | v1 |
| 1 | `Button` | `Button` | v1 |
| 2, 3 | `Row`, `Column` | `Row`, `Column` | v1 |
| 4 | `CameoGrid` | `CameoGrid` | v1 |
| 5 | `List` | `List` (added by this RFC) | v1 |
| 6 | `Image` | `Image` (added by this RFC) | v1 |
| 7 | `Bar` (progress bar) | `Bar` (added by this RFC) | v1 |
| 8 | `Decoration` | — Display properties attached to an object: its `Art`, then its `Number` and `Label` on one line under it, in its `Tint`. Not drawn while this player does not see the object | preview |
| 9 | `MapMarker` | — A dot in its `Tint` at its anchor cell (or a seen object), a line through its `Items` cells, a ground circle of `Width` leptons, and its `Art`, `Number` and `Label`, on the battlefield and as dots on the radar | preview |

**Node properties** are writable unless stated otherwise. Use `client_set_bytes` for `string` and `bytes`.

| Property | Value | Applies to | Notes |
| --- | --- | --- | --- |
| `Name` | string | All | Name for `client_node_find`. For a `veraui.ini` node, the part name after the layout prefix in the section name, read-only. A module names a node it created once with `client_set_bytes`, unique among its source's nodes. Naming again, a taken name or another's node returns `Refused` |
| `Label` | string | `Text`, `Button`, `Decoration` | CSF entry name or literal. Display the CSF entry if it exists, otherwise the literal text |
| `Art` | string | `Button`, `Image`, `Decoration` | Image name, resolved by name; the engine decides the format. Drawn unscaled from the node's top left. A module node's or map part's button draws its `Label` over the picture (one pixel down and right while held); the picture dims while disabled (the label greys), brightens under the pointer and darkens while held. A `veraui.ini` layout's own picture button draws the picture alone |
| `X`, `Y`, `Width`, `Height` | int | All | Pixels |
| `Hold` | enum `Hold` | All | |
| `Gap` | int | `Row`, `Column` | Gap between parts in pixels |
| `Visible` | bool | All | |
| `Enabled` | bool | `Button`, `List` | When 0, drawn disabled and produces no `Click` |
| `Dismissable` | bool | All | If 1 and visible, a mouse press outside the node or Esc emits `Dismiss` and consumes the press before it reaches the map |
| `Clickable` | bool | All | 1 emits `Click`. INI `Button` defaults to 1, other kinds to 0 |
| `Hoverable` | bool | All | 1 emits `Hover` |
| `Items` | string | `List`, `CameoGrid` | One entry per line separated by `\n`. `List` takes CSF entry names or literals. `CameoGrid` takes type names for module-created grids only. Native grid items are read-only |
| `ItemProgress` | bytes | `CameoGrid` | `later`. One `i32` per entry in `Items` order: 0..100, −1 for none |
| `ItemTooltips` | string | `CameoGrid` | One `typeName=extra text` per line, appended to the type's tooltip. Also writable on native grids |
| `Fill` | int | `Bar` | 0..100 |
| `Backdrop` | bool | `Text` | Whether a text over the battlefield stands on black, 1 by default; 0 for a text on the module's own picture |
| `Anchor` | ref | `Decoration`, `MapMarker` | A decoration's object; a marker's cell, or object. A house or other reference returns `Refused`. Hide the decoration when the object is invisible |
| `Palette` | string | `Decoration` | `later`: answers `Unsupported` |
| `Tint`, `TintIntensity` | int | `Decoration`, `MapMarker` | `0xRRGGBB` (−1 none) and `fixed`. A decoration tints its object on this screen alone, folded into the custom tint after the Phobos tints; a marker draws in it |
| `Anim` | string | `Decoration` | Attached animation played on this screen alone. `later`: stored, not drawn |
| `HideAnchor` | bool | `Decoration` | Hides the object's body on this screen; it is still listed, selected and clicked, and its status strip still draws. A map module's `HideAnchor` and `Tint` restyle the object only under `MapUI=full` |
| `Number` | int | `Decoration`, `MapMarker` | Number displayed at the anchor |
| `Items` | string | `MapMarker` | One `x,y` cell a line: the marker's line runs from its anchor through them, at most 256. Lines and circles together may draw 65,536 samples per frame. A line that is not two numbers returns `Refused` |
| `Width` | int | `MapMarker` | Radius in leptons of a ground circle round the anchor cell, 0 for none |
| `Hold` | int | `MapMarker` | 0 battlefield and radar, 1 battlefield, 2 radar |

**INI layout keys.** `Type` takes the INI name in the node kind table. `X`, `Y`, `Width`, `Height`, `Gap` and `Art` match their node properties. `Hold` takes one of the nine positions in [8.3](#83-enums). INI `Label` applies to buttons and text.

| Layout-only key | Values |
| --- | --- |
| `Parts` | Children of a row or column |
| `Command` | Keyboard command name, e.g. `StopObject` or `GuardObject` |
| `Value` | `Credits`, `Power`, `Drain`, `Name`, `Time`, `Kills`, `Losses` |
| `Tab`, `Columns`, `Rows` | Cameo grid |

**Client global references** (kind 6)

| Reference | Property | Value | Access | Tier |
| --- | --- | --- | --- | --- |
| Camera | `Cell` | ref (cell) | R. `Camera.cell()` reads the current cell | v1 |
| Camera | `X`, `Y` | lepton | W: camera center, jumps on write, clamped to the map | later |
| Local settings | `Viewer` | ref (house) | R. Local house, or 0 for spectators | v1 |
| Local settings | `IsSpectator` | bool | R | v1 |
| Local settings | Other local settings | — | Not exposed ([RFC 10](0001-wasm-extension-modules.md#10-client-modules)) | later |
| Cursor | `X`, `Y` | int | R. Screen pixels in the same coordinate system as node `X` and `Y` | v1 (phase 2) |
| Cursor | `Cell` | ref (cell) | R. Cell under the cursor, or 0 when off-map | v1 (phase 2) |
| Cursor | `CaptureMapClick` | bool | W. At 1, map clicks reach only the module as `MapClick`. At 0 (default), the game handles the click normally and the module still receives `MapClick` | v1 (phase 2) |

## 8. Name kinds, search scopes and other enums

### 8.1 Name kinds (`lookup`)

| # | Kind | Lookup target or special return value |
| --- | --- | --- |
| 0 | `TechnoType` | Infantry, vehicle, aircraft and building types |
| 1 | `Weapon` | Weapon |
| 2 | `Warhead` | Warhead |
| 3 | `Projectile` | Projectile (BulletType) |
| 4 | `Anim` | Animation |
| 5 | `VoxelAnim` | Voxel animation |
| 6 | `ParticleSystem` | Particle system |
| 7 | `SuperWeapon` | Superweapon type |
| 8 | `Armor` | Armor name |
| 9 | `Overlay` | Overlay |
| 10 | `Sound` | Sound |
| 11 | `Country` | Country (HouseType) |
| 12 | `TeamType` | AI team type |
| 13 | `ScriptType` | Team script |
| 14 | `AiTrigger` | AI trigger |
| 15 | `Variable` | Variable (`vera.vars`). The `mod.` and `map.` prefixes address the other origin. Returns the variable ID |
| 16 | `Notify` | Notification (`vera.notify`). Returns the FNV-1a hash of the lowercase-folded name |
| 17 | `ModuleAction` | Module action or decision handler in this module's `vera.actions`. No prefixes. Returns the dispatch key |
| 18 | `String` | Strings the engine already holds identically on every machine: strings returned by `string` properties, music, EVA names, map trigger and tag IDs, and argument texts in the string table and the names and values of `IniEntry` and `IniKey` (`args`). `NotFound` if absent. Creates no strings. The simulation never looks up CSF or translation tables: CSF entry names pass unchanged as text arguments and `lookup` does not test whether they exist. |
| 19 | `Property` | Property name. Returns the property ID |
| 20 | `EventKind` | Event kind name |
| 21 | `ActionKind` | Action kind name |
| 22 | `Hotkey` | Hotkey registered by this module (client) |
| 23 | `VeraEffect` | Reserved |
| 24 | `ModuleEvent` | Registered module event (any source, [RFC 6h](0001-wasm-extension-modules.md#6h-ini-registration)). Returns the event ID |

### 8.2 Search scopes (`find`)

| # | Scope | Result | Order | Client |
| --- | --- | --- | --- | --- |
| 0 | `Objects` | Unit and building handles (projectiles are not listed in 0.1.0) | Creation sequence | seen |
| 1 | `Houses` | House handles | House array order | all |
| 2 | `Cells` | Cell references | Cell index (y first, then x) | Cells visible to the local player |
| 3 | `Types` | A wildcard `type` returns every type reference of that name kind. A specific type returns only that type. Zero is an invalid argument. Use a `Buildable=1` clause to list buildable types or a `Category` clause to select infantry, vehicles, aircraft or buildings | Type ID | all |
| 4 | `Teams` | Team handles | Creation sequence | sim |
| 5 | `Triggers` | Trigger handles | Creation sequence | sim |
| 6 | `Tags` | Tag handles | Creation sequence | sim |
| 7 | `Modifiers` | Modifier handles | Insertion order | sim |
| 8 | `SuperWeapons` | Superweapon handles | Creation sequence | own |

### 8.3 Enums

Where numbers are omitted, the interface definition assigns values in declaration order.

| # | Enum | Values |
| --- | --- | --- |
| 0 | `Op` (filter clause) | 0 `Eq` (`=`), 1 `Ne` (`!=`), 2 `Lt` (`<`), 3 `Le` (`<=`), 4 `Gt` (`>`), 5 `Ge` (`>=`) |
| 1 | `Compare` (AI trigger) | 0 `Less`, 1 `LessEqual`, 2 `Equal`, 3 `GreaterEqual`, 4 `Greater`, 5 `NotEqual` (AI trigger comparator order) |
| 2 | `ModifierOp` | 0 `Add`, 1 `Multiply`, 2 `Override` |
| 3 | `Scope` (variable) | 0 `Global`, 1 `House`, 2 `Object` |
| 4 | `VarTarget` (filter clause variable) | 0 `Global`, 1 `Subject`, 2 `Other`, 3 `House`. Queries accept only 0 and 1 |
| 5 | `Visibility` (variable) | 0 `Private`, 1 `Allies`, 2 `InSight` (object scope only), 3 `Public` |
| 6 | `Source` | 0 `Mod`, 1 `Map`, 2 `PhobosEffect` (reserved, not produced in version 1), 3 `Behavior` (INI behavior): a modifier's `Origin` |
| 7 | `Table` (`has_kind`) | 0 event kind, 1 action kind, 2 property, 3 name kind, 4 search scope, 5 node kind, 6 client event kind, 7 client action kind, 8 other enum (`id` is the enum number in 8.3 times 256 plus the value) |
| 8 | `Mission` | 0 `Sleep`, 1 `Attack`, 2 `Move`, 3 `QMove`, 4 `Retreat`, 5 `Guard`, 6 `Sticky`, 7 `Enter`, 8 `Capture`, 9 `Eaten`, 10 `Harvest`, 11 `AreaGuard`, 12 `Return`, 13 `Stop`, 14 `Ambush`, 15 `Hunt`, 16 `Unload`, 17 `Sabotage`, 18 `Construction`, 19 `Selling`, 20 `Repair`, 21 `Rescue`, 22 `Missile`, 23 `Harmless`, 24 `Open`, 25 `Patrol`, 26 `ParadropApproach`, 27 `ParadropOverfly`, 28 `Deliberate` (INI name `Wait`), 29 `AttackMove`, 30 `SpyplaneApproach`, 31 `SpyplaneOverfly`. `None` = 255. Numbers match the map format |
| 9 | `Relation` | 0 `Self`, 1 `Ally`, 2 `Enemy`, 3 `Neutral` (civilian or passive house) |
| 10 | `Category` | 0 `Infantry`, 1 `Vehicle`, 2 `Aircraft`, 3 `Building`, 4 `Projectile` |
| 11 | `Layer` | `Underground`, `Surface`, `Ground`, `Air`, `Top` (water surface is a terrain type) |
| 12 | `LandType` | `Clear`, `Road`, `Water`, `Rock`, `Wall`, `Tiberium`, `Beach`, `Rough`, `Ice`, `Railroad`, `Tunnel`, `Weeds` |
| 13 | `SpeedType` | Values of the INI key `SpeedType`. Each has a corresponding `Passable.<SpeedType>` cell property |
| 14 | `Difficulty` | `Easy`, `Normal`, `Hard` (global `Difficulty` property). Trigger difficulty uses the `TriggerDef.difficulty` mask |
| 15 | `TagRepeat` | 0 `OneTimeOr`, 1 `OneTimeAnd`, 2 `Repeating` (stock tag repeat types) |
| 16 | `Rank` | 0 `Rookie`, 1 `Veteran`, 2 `Elite` |
| 17 | `Outcome` | 0 `Win`, 1 `Lose` |
| 18 | `Blocked` | 0 `Nearby`, 1 `Fail` |
| 19 | `SuperWeaponKind` | 0 `MultiMissile` … 11 `PsychicReveal`, plus Ares kinds |
| 20 | `Command` | See the command table below |
| 21 | `Hold` | `TopLeft`, `Top`, `TopRight`, `Left`, `Middle`, `Right`, `BottomLeft`, `Bottom`, `BottomRight` (`Hold` values in `veraui.ini`) |
| 22 | `PathKind` | 0 `Bezier`, 1 `Parabola`, 2 `Spiral`, 3 `Waypoints` |
| 23 | `IniKind` | 0 `Key`, 1 `Registry`, 2 `Event` |
| 24 | `IniSection` | 0 `TechnoType`, 1 `Weapon`, 2 `Warhead`, 3 `Projectile`, 4 `SuperWeapon`, 5 `Country`, 6 `General` (only the `[General]` section) |
| 25 | `Domain` | Bits: 0 `Production`, 1 `Forces`, 2 `Abilities`, 3 `SuperWeapons` ([RFC 6i](0001-wasm-extension-modules.md#6i-driving-a-house)) |
| 26 | `RefuseReason` | 0 `Other`, 1 `Gone`, 2 `IgnoresOrders`, 3 `IllegalTarget`, 4 `Prerequisite`, 5 `CannotAfford`, 6 `QueueFull`, 7 `CannotPlace`, 8 `NotReady`, 9 `Vetoed` (vetoed by `CommandAllowed` or `ProductionQueued`), 10 `RateLimited` |
| 27 | `EngageResult` | 0 `Ok`, 1 `Illegal`, 2 `OutOfRange`, 3 `NoDamage` |

**Commands** (`Command` and the fields of `Order`, for `order` and `client_send_order`). Fields marked — are zero. An order that violates this table returns `Refused`. See section 9 for tiers.

| # | Command | `target` | `value` | Unit list |
| --- | --- | --- | --- | --- |
| 0 | `Move` | Cell | — | Units |
| 1 | `MoveToObject` | Object | — | Units |
| 2 | `Stop` | — | — | Units |
| 3 | `Attack` | Object | — | Units |
| 4 | `AttackMove` | Cell | — | Units |
| 5 | `Guard` | Object, cell (guard the area) or 0 (in place) | — | Units |
| 6 | `DeployMcv` | — | — | MCV |
| 7 | `UndeployBuilding` | — | — | Building |
| 8 | `SetRally` | Cell | — | Factory building |
| 9 | `QueueProduction` | — | Type reference | None |
| 10 | `SuspendProduction` | — | Type reference | None |
| 11 | `CancelProductionByType` | — | Type reference | None |
| 12 | `PlaceReadyBuilding` | Cell | Building type reference | None |
| 13 | `SellBuilding` | — | — | Building |
| 14 | `ToggleRepair` | — | — | Building |
| 15 | `EjectBunker` | — | — | Building |
| 16 | `UnloadPassengers` | — | — | Transport or building |
| 17 | `CaptureBuilding` | Object (building) | — | Engineer |
| 18 | `PlantC4` | Object (building) | — | Units |
| 19 | `LaunchSuperWeapon` | Cell | Superweapon type reference | None |
| 20 | `ToggleInfantryDeploy` | — | — | Infantry |
| 21 | `ForceAttackCell` | Cell | — | Units |
| 22 | `SellWallAtCell` | Cell | — | None |
| 23 | `Scatter` | — | — | Units |
| 24 | `TogglePrimaryFactory` | — | — | Factory building |
| 25 | `Enter` | Object | — | Units |
| 26 | `Harvest` | Cell | — | Harvester |
| 27 | `TakeProduct` | — | Factory kind (index of `Factory.<Kind>`: 0 `Building` … 5 `Aircraft`) | None |

`Scatter` moves units a short distance aside, as the scatter key does. `TogglePrimaryFactory` toggles a factory's primary flag, as clicking a factory that is not primary does. `Enter` boards a transport or enters a bunker, repair depot, refinery or Grinder, as clicking it does.

## 9. Version 1 inventory

Tables with a tier column follow that column. This section covers the rest. Each entry must pass [Process](0001-wasm-extension-modules.md#process) step 5 before it freezes. The `v1` column lists what 0.1.0 implements, except where a row or the main text says otherwise.

| Table | `v1` | `later` or with phase 2 |
| --- | --- | --- |
| Simulation imports | All 24 (`send_notify` came with phase 2) | — |
| Simulation exports | All 7 (`on_message` came with phase 2) | — |
| Stock and Ares actions | 1, 2, 4, 5, 7, 8, 10..12, 14, 16..18, 20..35, 41..48, 51, 53..73, 76..80, 82..90, 92..97, 100..110, 112, 114..118, 125, 127..129, 131..145, 148, 149 | The rest |
| TechnoType properties | `Name`, `Category`, `Buildable`, `Strength`, `Cost`, `Points`, `Soylent`, `Speed`, `ROT`, `Crushable`, `Selfhealing`, `Explodes`, `Power`, `Sight`, `Ammo`, `InitialAmmo`, `Storage`, `Passengers`, `SizeLimit`, `TechLevel` | The rest |
| Weapon properties | `Name`, `Damage`, `ROF`, `Burst`, `MinimumRange`, `Range` | The rest |
| Warhead properties | `Name`, `CellSpread`, `PercentAtMax`, `Wall`, `Wood`, `Sparky`, `Fire`, `Conventional`, `Rocker`, `InfDeath` | The rest |
| Name kinds | `TechnoType`, `Weapon`, `Warhead`, `Projectile`, `Anim`, `VoxelAnim`, `ParticleSystem`, `SuperWeapon`, `Country`, `TeamType`, `Variable`, `ModuleAction`, `Property`, `ActionKind`, `String`, `ModuleEvent`. Phase 2: `Notify`, `Hotkey` | The rest. `Armor` previews with `TargetArmor` |
| Search scopes | `Objects` (no projectiles), `Houses` (no area, type or owner filter), `Cells`, `Types` (TechnoType, Weapon and Warhead, no area or owner filter), `Modifiers`, `SuperWeapons` | The rest |
| Commands | All in 8.3 except the right column | `EjectBunker` |
| Client node properties | All in 7.10 except the right column | Properties only of `Decoration`, `ItemProgress`, writing `Items` of a `CameoGrid` |
