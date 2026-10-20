# Original RNG and saved-load boundaries

Run `python -m tools.rmg_oracle.gen_rng_vectors --check`; use `--write` only
to regenerate the original-executable corpus and metadata. Configure retail
through [`native_oracle`](../native_oracle.md).

Historical `cases` and `ranged_cases` retain the five seeded streams and seven
retained mixed raw/ranged histories. Original Seed `0x65C6D0`, Next `0x65C780`
and RandomRanged `0x65C7E0` execute. Draws include rejection, equal/reversed
bounds and cursor continuation; these samples do not certify every signed span.

## Load and menu controls

Nine `menu_rng_cases` reuse the existing
[`ScenarioFixture`](../spatial_oracle/native_id_snapshot.md), exact stream
transport and original Save/Load/post-read. Incoming Scenario, Main and MapGen
states are produced by original Seed/Next with declared seeds and draw prefixes.
The inner load executes `0x689470 -> 0x683560`: Scenario becomes native Seed(0),
while the supplied Main, MapGen and global seed remain unchanged at this boundary.
Those observations are in `after_scenario_save_load.before/after` and
`native_seed_calls`; they do not assert retention through every full-world loader.

Each row then constructs the original RandomStraw at `0x6616B0` and executes
the real menu call `0x52E619 -> 0x52FC20`, stopping at `0x52E61E`. It supplies
the eight `SYSTEMTIME` words, GetTickCount value, game mode, seed override and
three gate globals as explicit inputs. OS imports are identified from the
original PE before their transports are installed. No RNG instruction is replaced.

With gates clear in mode 0 or 5, the observed global seed comes from the supplied
nonzero override or GetTickCount. The controls include zero and high-bit ticks.
Gate `A8B8B8`, bit 2 at `A8D5F8`, or nonzero `AA0444` selects the alternate arm,
retaining the supplied global seed. The mode-1 control retains that seed on the
regular arm. Both arms seed/copy Scenario and Main, with two observed Seed calls,
253 dword writes to each destination and no MapGen writes. The corpus retains
the full states, original seed result, event order and write offsets/sizes.
RNG comparisons exclude only padding bytes 1..3; MapGen retention is raw-byte
equality. The raw states are observations, not Python-generated expected words.

The original loaded-flag block `0x52E623..0x52E97D` is executed separately and
bypasses fresh scenario construction. Intervening `0x6370B0`, later menu work,
and complete world/type/resource loading are excluded. Thus these rows establish
the reseed boundary, not whole-load RNG draw totals or rollback.

## The in-game route is a different caller

Original instructions establish that offline options `0x4F10E0`, resource B5,
handles control `0x51E` through `0x4F1371 -> 0x5587F0`, the same browser/controller
used by the main menu. On success it writes result `0x51E` at `0x4F138A`, closes
the options dialog, and returns through StateMachine `0x48C9BA/0x48CC31` to
SessionResume `0x69BAB0` and ExitPause `0x683FB0`. MainGame calls this state machine
at `0x48CE9C` and resumes its loop at `0x48CE8A`. The menu preparer call at
`0x48CFAA` is on the later terminated-game cleanup route.

Consequently this in-game success continuation does not execute menu-only
`0x52E619 -> 0x52FC20`. That distinction rests on original caller/control-flow
reading; the full options/game loop is not emulated by these rows. The shared
Scenario reset above is executed evidence for the inner load boundary. Other
world-load consumers, process-sticky caches, audio/timer resume, and resource
initialization require their own owners and must not be inferred from this sample.
