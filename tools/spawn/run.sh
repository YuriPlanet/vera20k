#!/bin/sh
# Local multiplayer spawn.ini run (tools/spawn/README.md).
#   tools/spawn/run.sh relay|direct|browser OUT_DIR RA2_DIR [ACT_SECONDS] [TIMEOUT_SECONDS]
# browser: relay mode with headless Chrome players over the WebSocket gateway
# (browser.mjs); needs `tools/wasm/build.py --network` and the gateway's `npm ci`.
# Optional environment:
#   PLAYERS=N        humans, SourceIndex 0..N-1 (default 2)
#   SPECTATORS=S,T   sources that watch ([IsSpectator])
#   IPS=A,B,...      direct: each source's address in the other files
#   LEAVER=K         source that presses Leave at ACT_SECONDS (default 1; none)
#   RESIGN=K         source that resigns (abort box Observe) at ACT_SECONDS
#   KILL=relay|K     SIGKILL the relay or source K ACT_SECONDS after that
#                    run's match appears (relay: p0's log)
#   RELAY_RESTART=S  with KILL=relay: start the relay again S seconds later
#                    (same key and state directory)
#   BOUND=S          every other source presses Leave ACT_SECONDS+S into the
#                    match (default 60); on a score screen Leave is Continue
#   PACING=on|off|continuous  relay: sign a VO2C certificate with that pacing flag
#   SPEED=N          GameSpeed setting (default 2, 30 frames a second; 0 is 60)
#   SLOW=K           source K plays as a slow PC: SLOW_DRAW_MS (default 0) more
#                    per drawn picture and SLOW_TICK_MS (default 0) more per
#                    simulation tick (VERA20K_SPAWN_SLOW_DRAW_MS / _TICK_MS),
#                    and/or by SLOW_PLAN (VERA20K_SPAWN_SLOW_PLAN):
#                    "second:draw_ms:tick_ms[:ms],..." milliseconds more per
#                    frame pass and per simulation tick (and per tick outside
#                    its reported cost), linear between points from the
#                    match's start; tick delays of both add up
#   AI=N             N computer houses after the humans (relay: PLAYERS=1 allowed)
#   STARTS=A,B,...   each source's start (-2 none); humans on one start share its House
#   ALONE=on         relay, PLAYERS=1: a certificate the relay orders alone
#   WATCHERS=N       relay: N watchers (OUT/w0..), started WATCH_AFTER seconds
#                    (default 0) after p0's match appears (docs/spawn.md "Watching")
#   RELAY_PORT=P     relay: loopback UDP port (default 5000)
#   SERVICE_PORT=P   relay: the mock match API's port (default 8787)
#   GATEWAY_PORT=P   browser: the WebSocket gateway's port (default 8788)
#   PAGE_PORT=P      browser: the test pages' port (default 8000)
#   SIM_THREAD=V     browser, --threads package: the pages' simulation worker
#                    (1, always; default 0), or one value per source: always,0
#   BROWSERS=S,T     browser: only these sources are pages; the others are
#                    desktop clients in the same match (a mixed room)
#   MOD=FOLDER       every player plays this mod folder (copied into each
#                    player's Mods; pages import it)
# Scripted actions use the spawn-only VERA20K_SPAWN_LEAVE_AFTER_SECONDS and
# VERA20K_SPAWN_RESIGN_AFTER_SECONDS switches. Logs land in OUT_DIR.
set -eu
mode=$1
out=$(mkdir -p "$2" && cd "$2" && pwd)
ra2=$3
act=${4:-40}
limit=${5:-300}
players=${PLAYERS:-2}
leaver=${LEAVER:-1}
resign=${RESIGN:-none}
kill_target=${KILL:-none}
bound=${BOUND:-60}
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=${VERA20K_BIN:-$(ls -t "$root"/target/owned-worktrees/*/fast/vera20k 2>/dev/null | head -n 1)}
browsers=${BROWSERS:-}
# Whether any desktop client plays: not in an all-browser run.
desktop=yes
[ "$mode" = browser ] && [ -z "$browsers" ] && desktop=no
[ "$desktop" = no ] || [ -x "$bin" ] || { echo "build first: python3 -m tools.cargo_run -- build --profile fast -p vera20k --bin vera20k" >&2; exit 2; }
pids=""
cleanup() {
  for pid in $pids; do kill "$pid" 2>/dev/null || true; done
}
trap cleanup EXIT INT TERM
extra="--players $players"
[ -n "${SPECTATORS:-}" ] && extra="$extra --spectators $SPECTATORS"
[ -n "${AI:-}" ] && extra="$extra --ai $AI"
[ "${ALONE:-}" = on ] && extra="$extra --alone on"
[ -n "${SPEED:-}" ] && extra="$extra --speed $SPEED"
[ -n "${STARTS:-}" ] && extra="$extra --starts $STARTS"
watchers=${WATCHERS:-0}
[ "$watchers" -gt 0 ] && extra="$extra --watchers $watchers"
[ -n "${MOD:-}" ] && extra="$extra --mod $MOD"

relay_pid=""
relay_port=${RELAY_PORT:-5000}
service_port=${SERVICE_PORT:-8787}
gateway_port=${GATEWAY_PORT:-8788}
page_port=${PAGE_PORT:-8000}
[ "$mode" = browser ] && extra="$extra --cors http://localhost:$page_port --gateway ws://localhost:$gateway_port"
if [ "$mode" = relay ] || [ "$mode" = browser ]; then
  relay=${VERA20K_RELAY_BIN:-$(ls -t "$root"/target/owned-worktrees/*/debug/vera20k-relay 2>/dev/null | head -n 1)}
  [ -x "$relay" ] || { echo "build first: python3 -m tools.cargo_run -- build -p vera20k-relay" >&2; exit 2; }
  key=$(node -e "console.log(require('crypto').randomBytes(32).toString('hex'))")
  mkdir -p "$out/relay-state" && chmod 700 "$out/relay-state"
  VERA_RELAY_BIND=127.0.0.1:$relay_port VERA_RELAY_STATE_DIR="$out/relay-state" VERA_RELAY_TICKET_KEY=$key \
    "$relay" > "$out/relay.log" 2>&1 &
  relay_pid=$!
  pids="$pids $relay_pid"
  # shellcheck disable=SC2086
  VERA_RELAY_TICKET_KEY=$key node "$root/tools/spawn/spawn-e2e.mjs" relay --out "$out" --ra2 "$ra2" $extra \
    --port "$service_port" --relay "127.0.0.1:$relay_port" ${PACING:+--pacing "$PACING"} \
    > "$out/service.log" 2>&1 &
  service_pid=$!
  pids="$pids $service_pid"
  service_deadline=$(( $(date +%s) + limit ))
  until grep -q "match API on" "$out/service.log" 2>/dev/null; do
    if ! kill -0 "$service_pid" 2>/dev/null; then
      echo "match API exited during startup: see $out/service.log" >&2
      exit 1
    fi
    [ "$(date +%s)" -lt "$service_deadline" ] || { echo "timeout after ${limit}s: match API startup" >&2; exit 1; }
    sleep 0.2
  done
else
  [ -n "${IPS:-}" ] && extra="$extra --ips $IPS"
  # shellcheck disable=SC2086
  node "$root/tools/spawn/spawn-e2e.mjs" direct --out "$out" --ra2 "$ra2" $extra > "$out/service.log"
fi

if [ "$mode" = browser ]; then
  VERA_RELAYS=127.0.0.1:$relay_port VERA_WEB_ORIGIN=http://localhost:$page_port \
    VERA_LOBBY_URL=http://127.0.0.1:$service_port VERA_ALLOW_LOCAL_HTTP=1 PORT=$gateway_port \
    node "$root/tools/wasm/gateway/server.mjs" > "$out/gateway.log" 2>&1 &
  pids="$pids $!"
  until curl -s -o /dev/null "http://127.0.0.1:$gateway_port/"; do sleep 0.2; done
  status=0
  set -- --out "$out" --dist "${VERA20K_WASM_DIST:-$root/target/wasm-dist}" \
    --ra2 "$ra2" --port "$page_port" --leave "$act" --bound "$bound" --timeout "$limit"
  [ -n "${MOD:-}" ] && set -- "$@" --mod "$MOD"
  if [ -z "$browsers" ]; then
    node "$root/tools/spawn/browser.mjs" "$@" || status=$?
    [ -f "$out/results.json" ] && cat "$out/results.json"
    exit "$status"
  fi
  # A mixed room: the pages run beside the desktop clients started below.
  node "$root/tools/spawn/browser.mjs" "$@" --sources "$browsers" > "$out/browsers.log" 2>&1 &
  pages=$!
  pids="$pids $pages"
fi

is_page() { case ",$browsers," in *",$1,"*) true ;; *) false ;; esac; }
clients=""
source=0
while [ "$source" -lt "$players" ]; do
  if is_page "$source"; then source=$((source + 1)); continue; fi
  hooks="VERA20K_SPAWN_LEAVE_AFTER_SECONDS=$((act + bound))"
  if [ "$source" = "$resign" ]; then
    hooks="$hooks VERA20K_SPAWN_RESIGN_AFTER_SECONDS=$act"
  elif [ "$source" = "$leaver" ]; then
    hooks="VERA20K_SPAWN_LEAVE_AFTER_SECONDS=$act"
  fi
  if [ "$source" = "${SLOW:-none}" ]; then
    hooks="$hooks VERA20K_SPAWN_SLOW_DRAW_MS=${SLOW_DRAW_MS:-0} VERA20K_SPAWN_SLOW_TICK_MS=${SLOW_TICK_MS:-0} VERA20K_SPAWN_SLOW_PLAN=${SLOW_PLAN:-0:0:0}"
  fi
  rm -f "$out/p$source/spawn_result.json"
  # VERA20K_SPAWN_HIDDEN_WINDOW=1: invisible window, no Dock/menu bar entry.
  # shellcheck disable=SC2086
  (cd "$out/p$source" && exec env VERA20K_SPAWN_HIDDEN_WINDOW=1 $hooks "$bin" -SPAWN -WIN > stdout.log 2>&1) &
  pids="$pids $!"
  clients="$clients $!"
  eval "client_$source=$!"
  source=$((source + 1))
done

# Watchers join the match once it plays; each exits when it has watched it
# to its end.
watcher_clients=""
deadline=$(( $(date +%s) + limit ))
if [ "$watchers" -gt 0 ]; then
  # A player that never reaches the match (a refused launch) must not hold
  # the run past its limit.
  until grep -q "External launch: match on screen" "$out/p0/logs/ra2.log" 2>/dev/null; do
    [ "$(date +%s)" -lt "$deadline" ] || { echo "timeout after ${limit}s: no match for the watchers" >&2; exit 1; }
    sleep 0.5
  done
  sleep "${WATCH_AFTER:-0}"
  index=0
  while [ "$index" -lt "$watchers" ]; do
    rm -f "$out/w$index/spawn_result.json"
    # The match's score screen waits for Continue, which the hook presses
    # once the players are surely gone.
    (cd "$out/w$index" && exec env VERA20K_SPAWN_HIDDEN_WINDOW=1 \
      VERA20K_SPAWN_LEAVE_AFTER_SECONDS=$((act + bound + 60)) "$bin" -SPAWN -WIN > stdout.log 2>&1) &
    pids="$pids $!"
    watcher_clients="$watcher_clients $!"
    index=$((index + 1))
  done
fi

if [ "$kill_target" != none ]; then
  if [ "$kill_target" = relay ]; then victim=$relay_pid; watch=0; else eval "victim=\$client_$kill_target"; watch=$kill_target; fi
  (
    until grep -q "External launch: match on screen" "$out/p$watch/logs/ra2.log" 2>/dev/null; do sleep 0.5; done
    sleep "$act"
    echo "$(date -u +%FT%TZ) SIGKILL $kill_target (pid $victim)" >> "$out/kill.log"
    kill -9 "$victim" 2>/dev/null || true
    if [ "$kill_target" = relay ] && [ -n "${RELAY_RESTART:-}" ]; then
      sleep "$RELAY_RESTART"
      echo "$(date -u +%FT%TZ) relay restarted" >> "$out/kill.log"
      VERA_RELAY_BIND=127.0.0.1:$relay_port VERA_RELAY_STATE_DIR="$out/relay-state" VERA_RELAY_TICKET_KEY=$key \
        exec "$relay" >> "$out/relay.log" 2>&1
    fi
  ) &
  pids="$pids $!"
fi

source=0
for pid in $clients; do
  while is_page "$source"; do source=$((source + 1)); done
  while kill -0 "$pid" 2>/dev/null; do
    [ "$(date +%s)" -lt "$deadline" ] || { echo "timeout after ${limit}s" >&2; exit 1; }
    sleep 1
  done
  status=0
  wait "$pid" || status=$?
  echo "exit status $status" >> "$out/p$source/stdout.log"
  source=$((source + 1))
done
if [ -n "$browsers" ]; then
  status=0
  wait "$pages" || status=$?
  echo "== pages ($browsers): exit status $status"; tail -n 3 "$out/browsers.log"
fi
index=0
for pid in $watcher_clients; do
  while kill -0 "$pid" 2>/dev/null; do
    [ "$(date +%s)" -lt "$deadline" ] || { echo "timeout after ${limit}s" >&2; exit 1; }
    sleep 1
  done
  status=0
  wait "$pid" || status=$?
  echo "exit status $status" >> "$out/w$index/stdout.log"
  echo "== w$index"; tail -n 1 "$out/w$index/stdout.log"
  [ -f "$out/w$index/spawn_result.json" ] && cat "$out/w$index/spawn_result.json" && echo
  index=$((index + 1))
done
source=0
while [ "$source" -lt "$players" ]; do
  if ! is_page "$source"; then
    echo "== p$source"; tail -n 1 "$out/p$source/stdout.log"
    [ -f "$out/p$source/spawn_result.json" ] && cat "$out/p$source/spawn_result.json" && echo
  fi
  source=$((source + 1))
done
[ -f "$out/kill.log" ] && cat "$out/kill.log"
[ -f "$out/results.json" ] && cat "$out/results.json"
exit 0
