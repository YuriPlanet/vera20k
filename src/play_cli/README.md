# Play a skirmish from the CLI

`vera20k-play` starts an offline Yuri’s Revenge skirmish using the headless
scenario loader. It needs the local retail game files. Its process has no
window, graphics adapter, audio device or network listener.

```sh
python3 -m tools.cargo_run -- build --profile fast -p vera20k-core --bin vera20k-play
play_bin=$(python3 -m tools.cargo_run --resolve vera20k-play --profile fast)
"$play_bin" --map Dustbowl.mmx --opponents 1 --difficulty normal --seed 0x5eed
```

Use `--ra2-dir DIR`, `RA2_DIR` or `[paths] ra2_dir` in `config.toml` to select
retail files, in that order. `--map` accepts a name in the retail game or a
loose map from the local filesystem. `--opponents` accepts 0–7, subject to the
map’s available starts. `--script FILE` streams requests from a file.
`--max-ticks N` caps committed ticks for a bounded run.

## Agent protocol

Read the `ready` event and check its `schema` (`vera20k.play.v1`). Its
`observation` includes the first roster and the local house. Send one UTF-8
JSON object per line on standard input, then read one flushed response on
standard output. Blank lines are ignored. Each request can occupy at most
64 KiB including the newline. Startup and I/O errors go to standard error.

An agent can look up its MCV in `observation.units`, use the returned `id` in
an order, then step the match and examine the resulting roster. The example
ID below is a placeholder and depends on the map and seed.

```jsonl
{"cmd":"state"}
{"cmd":"order","order":{"action":"deploy","unit":2182}}
{"cmd":"tick","ticks":60}
{"cmd":"build_options"}
{"cmd":"order","order":{"action":"build","type_name":"GAPOWR"}}
{"cmd":"tick","ticks":400}
{"cmd":"placement","type_name":"GAPOWR","x":81,"y":33}
```

`order` returns `queued` at the current tick. The simulation decides whether
the order takes effect when it dispatches it. Check the later observation to
verify the result. Several orders can be queued before advancing. Only
`tick` advances the match, in batches of 1–4096 ticks. Time stays paused while
the process waits for the next request. The CLI keeps the current match in
memory and streams responses without accumulating a transcript.

| Request | Response |
| --- | --- |
| `state` | `state` with tick, binary frame, state hash, map dimensions, local economy, own units, contacts and factory queue heads |
| `tick` with `ticks` | `state` for the committed tick or `finished` on a terminal frame or the tick limit |
| `build_options` | `build_options` with currently enabled types, their factory category and cost |
| `placement` with `type_name`, `x`, `y` | `placement` with the simulation’s placement preview |
| `cell` with `x`, `y` | `cell` with fog status and terrain level and native land type for revealed cells |
| `order` with `order` | `queued` or `error` |
| `quit`, or end of input | `finished` with a reason and the final observation |

Coordinates are map cells (`rx`, `ry`). Object IDs are scoped to the match.
`units` includes local buildings. `contacts` is a diagnostic projection of
non-owned objects in currently visible cells. It can expose the actual type
of a cloaked or disguised object in such a cell. The hash is a 16-digit hex
string. Factory heads report `Building`, `Paused` or `Done`. Placement and
cell reads leave the simulation tick unchanged.

| Order action | Fields |
| --- | --- |
| `deploy`, `stop` | `unit` |
| `move`, `attack_move`, `attack_cell`, `guard` | `unit`, `x`, `y` |
| `attack` | `unit`, `target` |
| `capture` | `unit`, `building` |
| `rally` | `producers` (array of IDs), `x`, `y` |
| `build` | `type_name` |
| `cancel_build` | `type_name`, optional `all` (defaults to `false`) |
| `place`, `super_weapon` | `type_name`, `x`, `y` |
| `place_mobile` | `category` (`Infantry`, `Vehicle`, `Ship` or `Aircraft`) |
| `sell`, `repair` | `building` |
| `unload` | `transport` |

`error` with `fatal: false` leaves the process ready for another request. A
terminal `finished` event is followed by exit status 0. A startup, I/O or
fatal frame failure exits nonzero.

An external agent owns decisions and restart policy. For continuous play, it
can keep the child’s stdin open, alternate orders and step requests, record
responses it needs, and start a new child after `finished`. The first version
covers offline skirmish. Campaign, save/resume and online rooms are outside the
current protocol.
