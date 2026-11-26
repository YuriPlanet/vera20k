"""Manifest-based AutoPlay regression runner (stdlib only, Python 3.10+).

See autoplay/docs/benchmark-v42.1.md. The historical CSV is evidence, not a
live run receipt. This runner never reads enemy information into the brain.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor, as_completed
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import sys
import threading
import time
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / "autoplay/benchmarks/v41.json"
CONFIG = dict(ticks=200000, per_frame=512, progress=1800, difficulty="hard",
              opponents=1, credits=10000, units=0, wall_seconds=900,
              expected_opening=[10000, 50000, 1, 1])
COUNTRIES = set("america russia yuri korea iraq france germany cuba libya greatbritain".split())
HEADER = re.compile(r"^autoplay-match: .+? on (.+?), seed (0x[0-9a-fA-F]+|\d+), (\d+) ticks, (\d+) per frame\s*$", re.M)
END = re.compile(r"^autoplay-match: (won|lost|timeout) at tick (\d+)\s*$", re.M)
CENSUS = re.compile(r"^\s*(\d+)\s+(\d+)\s+(\d+)\s+(-?\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s*$", re.M)
WALL = re.compile(r"no site for \w*WALL; retaining finished structure")
STATUS = re.compile(r"auto play: building=.*?yard=(\d+).*?fighters=(\d+).*?pts=(\d+)/\d+")
MOBILIZATION_FIELDS = ("wave_ready_tick", "opening_deadline_tick", "opening_purchase_requests", "local_counter_requests")
SAFETY_FIELDS = ("safety_veto_reports", "casualty_entries", "casualty_recovered")
READINESS_FIELDS = ("readiness_samples", "peak_available_hp", "max_unavailable_hp", "median_available_hp_pct", "siege_post_requests")
CONVERGENCE_FIELDS = ("economy_rest_entries", "economy_rest_samples", "front_deferred_events", "local_contact_samples")
FACTORY_FIELDS = ("factory_handoff_samples", "factory_pending_peak", "factory_handoff_ends", "factory_handoff_live_ends", "factory_handoff_stalled", "factory_handoff_veto_reports", "expansion_risk_veto_reports")
ASSAULT_FIELDS = ("assault_commits", "assault_ends", "assault_loss_aborts", "assault_progress_aborts", "front_guard_veto_reports", "front_guard_infantry_reports", "empty_fist_samples")
ECONOMY_FIELDS = ("budget_samples", "budget_low_cash_samples", "native_spent_delta", "peak_committed_credits", "peak_investment_bank", "rest_proposal_samples")
FEEDBACK_FIELDS = ("assault_feedback_reports", "assault_missing_owned", "assault_removed_value", "assault_damage_value", "assault_target_damage", "assault_max_retry_cooldown")
BUDGET = re.compile(r"auto play: economy_budget tick=(\d+) cash=(-?\d+) spent=(-?\d+) committed=(\d+) bank=(\d+) proposed=(\d+) rest=(true|false)")
FEEDBACK = re.compile(r"auto play: assault_feedback tick=\d+ target=\d+ missing_owned=(\d+) removed_value=(\d+) damage_value=(\d+) target_damage=(\d+) failures=(\d+) cooldown=(\d+) visible=(?:true|false)")
FACTORY = re.compile(r"auto play: factory_handoff tick=(\d+) pending=(\d+)")
READINESS = re.compile(r"auto play: readiness tick=(\d+) roster=(\d+) available=(\d+) raw_hp=(\d+) available_hp=(\d+) attack=(\d+)")
OUTCOMES = ("won", "lost", "timeout", "wallclock", "crash", "nonzero_exit", "protocol_error", "launch_error")


def sha(path: Path) -> str:
    with path.open("rb") as f:
        digest = hashlib.sha256()
        for block in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def atomic_json(path: Path, data) -> None:
    atomic_text(path, json.dumps(data, indent=2, sort_keys=True, ensure_ascii=False) + "\n")


def atomic_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + "." + uuid.uuid4().hex + ".tmp")
    try:
        with tmp.open("w", encoding="utf-8", newline="") as f:
            f.write(text)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, path)
    finally:
        tmp.unlink(missing_ok=True)


def cell(case: dict) -> str:
    return f"{case['seed']}:{case['map']}:{case['local']}:{case['opponent']}"


def diagnostics(text: str) -> dict:
    rows = [tuple(map(int, row)) for row in CENSUS.findall(text)]
    samples = [tuple(map(int, row)) for row in STATUS.findall(text)]
    # Census totals are snapshots, not a direct count of kills or real DPS.
    last = rows[-1] if rows else None
    first = rows[0] if rows and rows[0][0] == 0 else None
    wave = re.search(r"auto play: mobilization_exit tick=(\d+) reason=wave-ready", text)
    deadline = re.search(r"auto play: mobilization_exit tick=(\d+) reason=deadline", text)
    observed = bool(re.search(r"auto play: mobilization(?:_exit|_purchase)? tick=", text))
    safety_observed = "auto play: safety_continuity tick=" in text
    readiness = [tuple(map(int, row)) for row in READINESS.findall(text)]
    fractions = [100.0 * row[4] / row[3] for row in readiness if row[3] > 0]
    convergence_observed = "auto play: convergence tick=" in text
    delivery = [tuple(map(int, row)) for row in FACTORY.findall(text)]
    delivery_observed = bool(delivery) or "auto play: factory_handoff_end tick=" in text or "auto play: factory_handoff_stalled tick=" in text
    doctrine_observed = "auto play: combat_doctrine tick=" in text or "auto play: assault_commit tick=" in text
    budgets = [(*map(int,row[:6]), row[6] == "true") for row in BUDGET.findall(text)]
    feedback = [tuple(map(int,row)) for row in FEEDBACK.findall(text)]
    feedback_observed = bool(feedback) or "auto play: combat_feedback tick=" in text
    return dict(budget_samples=len(budgets) if budgets else None,
                budget_low_cash_samples=sum(r[1]<1000 for r in budgets) if budgets else None,
                native_spent_delta=budgets[-1][2]-budgets[0][2] if len(budgets)>1 and budgets[-1][2]>=budgets[0][2] else None,
                peak_committed_credits=max((r[3] for r in budgets),default=None),
                peak_investment_bank=max((r[4] for r in budgets),default=None),
                rest_proposal_samples=sum(r[6] and r[5]>0 for r in budgets) if budgets else None,
                assault_feedback_reports=len(feedback) if feedback_observed else None,
                assault_missing_owned=sum(r[0] for r in feedback) if feedback_observed else None,
                assault_removed_value=sum(r[1] for r in feedback) if feedback_observed else None,
                assault_damage_value=sum(r[2] for r in feedback) if feedback_observed else None,
                assault_target_damage=sum(r[3] for r in feedback) if feedback_observed else None,
                assault_max_retry_cooldown=max((r[5] for r in feedback),default=None),
                assault_commits=len(re.findall(r"auto play: assault_commit tick=", text)) if doctrine_observed else None,
                assault_ends=len(re.findall(r"auto play: assault_end tick=", text)) if doctrine_observed else None,
                assault_loss_aborts=len(re.findall(r"auto play: assault_end [^\n]* reason=(?:loss-limit|forecast-changed|cohort-detached)\b", text)) if doctrine_observed else None,
                assault_progress_aborts=len(re.findall(r"auto play: assault_end [^\n]* reason=(?:no-damage-progress|deadline)\b", text)) if doctrine_observed else None,
                front_guard_veto_reports=len(re.findall(r"auto play: front_guard_veto tick=", text)) if doctrine_observed else None,
                front_guard_infantry_reports=len(re.findall(r"auto play: front_guard_veto [^\n]* infantry=true", text)) if doctrine_observed else None,
                empty_fist_samples=sum(r[2]>=6 and r[5]==0 for r in readiness) if readiness else None,
                factory_handoff_samples=len(delivery) if delivery_observed else None,
                factory_pending_peak=max((r[1] for r in delivery), default=None),
                factory_handoff_ends=len(re.findall(r"auto play: factory_handoff_end tick=", text)) if delivery_observed else None,
                factory_handoff_live_ends=len(re.findall(r"auto play: factory_handoff_end [^\n]* orderable=true", text)) if delivery_observed else None,
                factory_handoff_stalled=len(re.findall(r"auto play: factory_handoff_stalled tick=", text)) if delivery_observed else None,
                factory_handoff_veto_reports=len(re.findall(r"auto play: factory_handoff_veto tick=", text)) if delivery_observed else None,
                expansion_risk_veto_reports=len(re.findall(r"auto play: expansion_risk_veto tick=", text)) if delivery_observed else None,
                economy_rest_entries=len(re.findall(r"auto play: economy_rest tick=\d+ active=true", text)) if convergence_observed else None,
                economy_rest_samples=len(re.findall(r"auto play: convergence tick=\d+ economy_rest=true", text)) if convergence_observed else None,
                front_deferred_events=len(re.findall(r"auto play: front_deferred tick=", text)) if convergence_observed else None,
                local_contact_samples=len(re.findall(r"auto play: local_contact tick=", text)) if convergence_observed else None,
                readiness_samples=len(readiness) if readiness else None,
                peak_available_hp=max((r[4] for r in readiness), default=None),
                max_unavailable_hp=max((max(0, r[3]-r[4]) for r in readiness), default=None),
                median_available_hp_pct=round(statistics.median(fractions), 2) if fractions else None,
                siege_post_requests=len(re.findall(r"auto play: siege_post_request tick=", text)) if readiness else None,
                safety_veto_reports=len(re.findall(r"auto play: safety_veto tick=", text)) if safety_observed else None,
                casualty_entries=len(re.findall(r"auto play: casualty_enter tick=", text)) if safety_observed else None,
                casualty_recovered=len(re.findall(r"auto play: casualty_recovered tick=", text)) if safety_observed else None,
                wave_ready_tick=int(wave[1]) if wave else None,
                opening_deadline_tick=int(deadline[1]) if deadline else None,
                opening_purchase_requests=len(re.findall(r"auto play: mobilization_purchase tick=", text)) if observed else None,
                local_counter_requests=len(re.findall(r"auto play: local_counter_purchase tick=", text)) if observed else None,
                last_sample_tick=last[0] if last else None,
                opening=[first[i] for i in (1, 2, 4, 5)] if first else None,
                own=last[4] if last else None, enemy=last[5] if last else None,
                credits=last[1] if last else None, power=last[3] if last else None,
                peak_pts=max((s[2] for s in samples), default=None),
                last_yards=samples[-1][0] if samples else None,
                negative_power_samples=sum(r[3] < 0 for r in rows),
                wall_warnings=len(WALL.findall(text)),
                site_release_requests=len(re.findall(r"auto play: site_release tick=", text)),
                recovery_entries=len(re.findall(r"auto play: force_recovery tick=", text)),
                recovery_ready_exits=len(re.findall(r"auto play: force_recovery_exit .*?reason=threshold", text)),
                recovery_timeout_exits=len(re.findall(r"auto play: force_recovery_exit .*?reason=timeout", text)),
                wall_product_departures=len(re.findall(r"auto play: site_release_observed .*?product_departed=true", text)),
                launch_config=next(iter(re.findall(r"^autoplay-match: launch (.+)$", text, re.M)), None))


def group(case: dict) -> str:
    b = case["baseline"]
    if b["outcome"] == "lost":
        return "early" if b["tick"] < 30000 else "late"
    return "win" if b["outcome"] == "won" else "unresolved"


def select_suites(cases: list[dict]) -> dict:
    # Fixed diagnostic anchors from the handoff; fill each bucket by diversity,
    # not by selecting the most easily reversible losses.
    anchors = {20000, 20004, 20024, 20035, 20069, 20074, 20145, 20222, 20444,
               20345, 20898, 20930}
    by_seed = {c["seed"]: c for c in cases}
    selected = [by_seed[s] for s in sorted(anchors)]
    def extend(target: int):
        for bucket in ("early", "late", "win", "unresolved"):
            while sum(group(c) == bucket for c in selected) < target:
                taken = {c["seed"] for c in selected}
                counts = {k: Counter(c[k] for c in selected) for k in ("local", "opponent", "map")}
                candidates = [c for c in cases if group(c) == bucket and c["seed"] not in taken]
                # Equal coverage is preferred; seed hash gives a stable tie-break.
                selected.append(min(candidates, key=lambda c: (
                    counts["local"][c["local"]] + counts["opponent"][c["opponent"]] + counts["map"][c["map"]],
                    hashlib.sha256(str(c["seed"]).encode()).hexdigest())))
        return sorted(c["seed"] for c in selected)
    smoke = extend(6)
    dev = extend(30)
    return dict(smoke=smoke, dev=dev, holdout=sorted(set(by_seed) - set(dev)), all=sorted(by_seed))


def import_v41(archive: Path) -> dict:
    """Recover the exact indexed log, refusing a guess if evidence disagrees."""
    with zipfile.ZipFile(archive) as z:
        rows = list(csv.DictReader(io.TextIOWrapper(z.open("v41_matrix_results.csv"), encoding="utf-8-sig")))
        if len(rows) != 1102:
            raise ValueError(f"expected 1102 V41 records, got {len(rows)}")
        names = Counter(z.namelist())
        cases = []
        for i, row in enumerate(rows):
            name = f"v41_matrix/{i:04d}_{row['local']}__{row['opponent']}__{row['map']}.log"
            if names[name] != 1:
                raise ValueError(f"missing or duplicate indexed log: {name}")
            raw = z.read(name)
            text = raw.decode("utf-8", errors="replace")
            header = HEADER.findall(text)
            seed = 20000 + i
            if len(header) != 1 or (header[0][0], int(header[0][1], 0), int(header[0][2]), int(header[0][3])) != (row['map'], seed, CONFIG['ticks'], CONFIG['per_frame']):
                raise ValueError(f"header mismatch: {name}")
            d = diagnostics(text)
            if d["opening"] != CONFIG["expected_opening"]:
                raise ValueError(f"opening mismatch: {name}")
            samples = CENSUS.findall(text)
            if len(samples) < 2 or int(samples[1][0]) != CONFIG["progress"]:
                raise ValueError(f"progress cadence mismatch: {name}")
            ends = END.findall(text)
            outcome = row["outcome"]
            tick = int(row["tick"]) if row["tick"] else None
            if outcome == "wallclock":
                if ends or (tick is not None and tick != d["last_sample_tick"]):
                    raise ValueError(f"wallclock mismatch: {name}")
                tick = d["last_sample_tick"]
            elif outcome not in ("won", "lost", "timeout") or ends != [(outcome, str(tick))]:
                raise ValueError(f"result mismatch: {name}")
            # Keep the V41 manifest byte-identical to the original importer;
            # the new fields are not historical measurements.
            for k in (*MOBILIZATION_FIELDS, *SAFETY_FIELDS, *READINESS_FIELDS, *CONVERGENCE_FIELDS, *FACTORY_FIELDS, *ASSAULT_FIELDS, *ECONOMY_FIELDS, *FEEDBACK_FIELDS):
                d.pop(k, None)
            # V41 never emitted the new recovery event schema. Missing is not zero.
            for k in ("site_release_requests", "recovery_entries", "recovery_ready_exits", "recovery_timeout_exits", "wall_product_departures"):
                d[k] = None
            cases.append(dict(seed=seed, map=row["map"], local=row["local"], opponent=row["opponent"],
                              baseline=dict(outcome=outcome, tick=tick, tick_source="last_sample" if outcome == "wallclock" else "terminal",
                                            seconds=float(row["seconds"]), log=name,
                                            log_sha256=hashlib.sha256(raw).hexdigest(), **d)))
    if len({(c['map'], c['local'], c['opponent']) for c in cases}) != len(cases):
        raise ValueError("historical cell repeated")
    return dict(schema=1, baseline_commit="ff55d1105", archive_sha256=sha(archive),
                config=CONFIG, suites=select_suites(cases), cases=cases,
                provenance="V41 historical CSV + indexed log headers; 16 jobs reported; actual executable, retail asset hashes and effective defaults unavailable")


def load_manifest(path: Path, suite: str) -> tuple[dict, list[dict]]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if data["schema"] != 1 or data["config"] != CONFIG:
        raise ValueError("unsupported manifest configuration")
    cases = data["cases"]
    by_seed = {c["seed"]: c for c in cases}
    if len(by_seed) != len(cases):
        raise ValueError("duplicate manifest seed")
    for c in cases:
        if (type(c["seed"]) is not int or not 0 <= c["seed"] < 2**32
                or c["local"] not in COUNTRIES or c["opponent"] not in COUNTRIES
                or not re.fullmatch(r"[A-Za-z0-9_. -]+\.mmx", c["map"], re.I)):
            raise ValueError(f"invalid case: {c}")
    seeds = data["suites"][suite]
    if not seeds or len(set(seeds)) != len(seeds) or not set(seeds) <= by_seed.keys():
        raise ValueError("invalid suite membership")
    return data, [by_seed[s] for s in seeds]


def command(binary: Path, ra2: Path, case: dict, config: dict) -> list[str]:
    return [str(binary), "--ra2-dir", str(ra2), "--map", case["map"],
            "--country", case["local"], "--opponent", case["opponent"], "--seed", str(case["seed"]),
            *[x for key in ("ticks", "per_frame", "progress", "difficulty", "opponents", "credits", "units")
              for x in ("--" + key.replace("_", "-"), str(config[key]))]]


def classify(text: str, case: dict, config: dict, exit_code: int | None, timed_out: bool) -> dict:
    d = diagnostics(text)
    headers = HEADER.findall(text)
    valid_header = len(headers) == 1 and (headers[0][0], int(headers[0][1], 0), int(headers[0][2]), int(headers[0][3])) == (case["map"], case["seed"], config["ticks"], config["per_frame"])
    ends = END.findall(text)
    outcome, tick, source = "protocol_error", d["last_sample_tick"], "last_sample"
    if timed_out:
        outcome = "wallclock" if valid_header and d["opening"] is not None else "protocol_error"
    elif exit_code is not None and exit_code != 0:
        # POSIX signals and Windows NTSTATUS exceptions are distinct from a
        # normal non-zero process exit (e.g. bad CLI arguments).
        outcome = "crash" if exit_code < 0 or exit_code >= 0x80000000 else "nonzero_exit"
    elif valid_header and d["opening"] is not None and len(ends) == 1:
        result, terminal = ends[0][0], int(ends[0][1])
        if 0 <= terminal <= config["ticks"] and (result != "timeout" or terminal == config["ticks"]):
            outcome, tick, source = result, terminal, "terminal"
    return dict(outcome=outcome, tick=tick, tick_source=source if tick is not None else None,
                exit_code=exit_code, wall_cap_hit=timed_out, header_valid=valid_header,
                reported_terminal=ends[0] if len(ends) == 1 else None,
                opening_matches=d["opening"] == config["expected_opening"], **d)


def run_case(binary: Path, ra2: Path, case: dict, config: dict, run_dir: Path,
             run_id: str, stop: threading.Event) -> dict | None:
    if stop.is_set():
        return None
    seed = case["seed"]
    log = run_dir / "logs" / f"{seed}.log"
    log.parent.mkdir(parents=True, exist_ok=True)
    # Never overwrite evidence of an interrupted attempt.
    if log.exists():
        log = log.with_name(f"{seed}-{uuid.uuid4().hex[:8]}.log")
    cmd = command(binary, ra2, case, config)
    started = time.monotonic()
    code, capped, error = None, False, None
    with log.open("wb") as f:
        try:
            env = {**os.environ, "RUST_LOG": "info"}
            env.pop("VERA20K_TRACE", None)
            process_group = ({"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt"
                             else {"start_new_session": True})
            with subprocess.Popen(cmd, cwd=ROOT, stdout=f, stderr=subprocess.STDOUT, env=env, **process_group) as proc:
                while True:
                    code = proc.poll()
                    if code is not None:
                        break
                    if stop.is_set():
                        proc.kill()
                        proc.wait()
                        return None
                    remaining = config["wall_seconds"] - (time.monotonic() - started)
                    if remaining <= 0:
                        capped = True
                        proc.kill()
                        code = proc.wait()
                        break
                    try:
                        code = proc.wait(timeout=min(.5, remaining))
                        break
                    except subprocess.TimeoutExpired:
                        pass
        except OSError as exc:
            error = str(exc)
    seconds = time.monotonic() - started
    row = classify(log.read_text(encoding="utf-8", errors="replace"), case, config, code, capped)
    if error:
        row.update(outcome="launch_error", error=error)
    row.update({k: case[k] for k in ("seed", "map", "local", "opponent")},
               run_id=run_id, seconds=round(seconds, 3),
               log=log.relative_to(run_dir).as_posix(), log_sha256=sha(log))
    atomic_json(run_dir / "results" / f"{seed}.json", row)
    return row


def source_provenance(source_dir: Path) -> dict:
    # The supplied source tree is descriptive, not proof of how an arbitrary
    # binary was built. Binary SHA-256 remains the authoritative identity.
    def git(*args):
        return subprocess.check_output(["git", *args], cwd=source_dir, stderr=subprocess.DEVNULL)
    try:
        return dict(source_dir=str(source_dir), source_head=git("rev-parse", "HEAD").decode().strip(),
                    tracked_diff_sha256=hashlib.sha256(git("diff", "--binary", "--no-ext-diff", "HEAD")).hexdigest())
    except (OSError, subprocess.CalledProcessError):
        return dict(source_dir=str(source_dir), source_head=None, tracked_diff_sha256=None)


def input_fingerprint(ra2: Path) -> dict:
    # Top-level retail MIX/loose rules/maps etc. The old run has no such hashes.
    suffixes = {".mix", ".ini", ".mmx", ".map", ".mpr", ".pkt", ".csf", ".yrm"}
    files = sorted((p for p in ra2.iterdir() if p.is_file() and p.suffix.lower() in suffixes), key=lambda p: p.name.lower())
    if not files:
        raise ValueError("no retail inputs found in RA2_DIR")
    return {p.name: sha(p) for p in files}


def load_receipts(run_dir: Path, cases: list[dict], run_id: str) -> dict:
    expected = {c["seed"]: c for c in cases}
    rows = {}
    for p in sorted((run_dir / "results").glob("*.json")):
        row = json.loads(p.read_text(encoding="utf-8"))
        seed = row["seed"]
        if (seed not in expected or p.stem != str(seed) or seed in rows or row["run_id"] != run_id
                or cell(row) != cell(expected[seed]) or row["outcome"] not in OUTCOMES):
            raise ValueError(f"foreign or invalid receipt: {p}")
        log = (run_dir / row["log"]).resolve()
        if not log.is_relative_to(run_dir.resolve()) or not log.is_file() or sha(log) != row["log_sha256"]:
            raise ValueError(f"missing or changed log for receipt: {p}")
        rows[seed] = row
    return rows


CSV_FIELDS = ("seed", "map", "local", "opponent", "outcome", "tick", "tick_source", "seconds", "exit_code", "opening_matches", "last_sample_tick", "own", "enemy", "credits", "power", "peak_pts", "last_yards", "negative_power_samples", "wall_warnings", "site_release_requests", "recovery_entries", "recovery_ready_exits", "recovery_timeout_exits", "wall_product_departures", *MOBILIZATION_FIELDS, *SAFETY_FIELDS, *READINESS_FIELDS, *CONVERGENCE_FIELDS, *FACTORY_FIELDS, *ASSAULT_FIELDS, *ECONOMY_FIELDS, *FEEDBACK_FIELDS, "run_id", "log", "log_sha256")


def export_csv(path: Path, rows: dict) -> None:
    buf = io.StringIO(newline="")
    writer = csv.DictWriter(buf, fieldnames=CSV_FIELDS, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(rows[s] for s in sorted(rows))
    atomic_text(path, buf.getvalue())


def execute(args) -> int:
    data, cases = load_manifest(args.manifest, args.suite)
    stage = args.stage or args.suite
    _, stage_cases = load_manifest(args.manifest, stage)
    stage_seeds = {c["seed"] for c in stage_cases}
    if not stage_seeds <= {c["seed"] for c in cases}:
        raise ValueError("stage must be a subset of the target suite")
    binary = args.binary.resolve()
    ra2 = args.ra2_dir.resolve()
    run_dir = args.run_dir.resolve()
    if not binary.is_file() or not ra2.is_dir() or args.jobs < 1:
        raise ValueError("need an existing match binary, retail directory and positive jobs")
    run_dir.mkdir(parents=True, exist_ok=True)
    lock = run_dir / "RUNNING.lock"
    # One writer per run directory. After a hard kill, check for live workers
    # before removing this lock manually; never guess at process ownership.
    try:
        fd = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    except FileExistsError as exc:
        raise ValueError(f"run directory locked: {lock}") from exc
    try:
        with os.fdopen(fd, "w") as f:
            f.write(str(os.getpid()) + "\n")
        identity = dict(schema=1, manifest_sha256=sha(args.manifest), suite=args.suite,
                        label=args.label, binary=str(binary), binary_sha256=sha(binary),
                        ra2_dir=str(ra2), retail_inputs=input_fingerprint(ra2), jobs=args.jobs,
                        config=data["config"], runner_sha256=sha(Path(__file__)),
                        **source_provenance(args.source_dir.resolve()),
                        python=sys.version, platform=sys.platform, cpu_count=os.cpu_count(),
                        performance_env={k: os.environ.get(k) for k in ("RAYON_NUM_THREADS", "VERA20K_IDLE_SLEEP")})
        meta = run_dir / "run.json"
        if meta.exists():
            old = json.loads(meta.read_text(encoding="utf-8"))
            run_id = old.pop("run_id")
            if old != identity:
                different = [k for k in identity if old.get(k) != identity[k]]
                raise ValueError(f"resume configuration changed ({', '.join(different)}); use a new run directory")
        else:
            if list((run_dir / "results").glob("*.json")):
                raise ValueError("results exist without run metadata")
            run_id = uuid.uuid4().hex
            atomic_json(meta, dict(identity, run_id=run_id))
        rows = load_receipts(run_dir, cases, run_id)
        export_csv(run_dir / "results.csv", rows)
        remaining = [c for c in stage_cases if c["seed"] not in rows]
        print(f"{args.label}: {len(rows)}/{len(cases)} recorded; {len(remaining)} pending in {stage}", flush=True)
        stop = threading.Event()
        pool = ThreadPoolExecutor(max_workers=args.jobs)
        try:
            futures = [pool.submit(run_case, binary, ra2, c, data["config"], run_dir, run_id, stop) for c in remaining]
            for future in as_completed(futures):
                row = future.result()
                if row is None:
                    continue
                rows[row["seed"]] = row
                export_csv(run_dir / "results.csv", rows)
                print(f"{len(rows)}/{len(cases)} {cell(row)} {row['outcome']} tick={row['tick']} ({row['seconds']:.1f}s)"
                      + (" OPENING MISMATCH" if not row["opening_matches"] else ""), flush=True)
        except BaseException:
            stop.set()
            pool.shutdown(wait=True, cancel_futures=True)
            export_csv(run_dir / "results.csv", load_receipts(run_dir, cases, run_id))
            raise
        finally:
            pool.shutdown(wait=True, cancel_futures=True)
        return 0 if all(s in rows and rows[s]["opening_matches"] and rows[s]["outcome"] in OUTCOMES[:4]
                        for s in stage_seeds) else 2
    finally:
        lock.unlink(missing_ok=True)


def observed_tick_rate(row: dict):
    """Whole-match wall-clock throughput, not strategy CPU time. A capped
    match uses its last recorded tick, so the rate is a lower estimate."""
    tick, seconds = row.get("tick"), row.get("seconds")
    if tick is None or seconds is None or seconds <= 0 or tick < 0:
        return None
    return round(tick / seconds, 2)


def compare(args) -> int:
    _, cases = load_manifest(args.manifest, args.suite)
    def read_run(path):
        meta = json.loads((path / "run.json").read_text(encoding="utf-8"))
        if meta["manifest_sha256"] != sha(args.manifest):
            raise ValueError("run manifest mismatch")
        _, target_cases = load_manifest(args.manifest, meta["suite"])
        if not {c["seed"] for c in cases} <= {c["seed"] for c in target_cases}:
            raise ValueError("requested comparison suite is not contained in the run")
        return meta, load_receipts(path, target_cases, meta["run_id"])
    meta, after = read_run(args.run_dir)
    if args.before:
        old, before = read_run(args.before)
        for k in ("config", "retail_inputs", "jobs", "platform", "cpu_count", "runner_sha256", "performance_env"):
            if old[k] != meta[k]:
                raise ValueError(f"comparison configuration mismatch: {k}")
        print("Same recorded configuration; confirm both runs used the same machine and engine controls.")
        if old["source_head"] != meta["source_head"]:
            print("WARNING: different Git HEADs; changes cannot be attributed to policy alone.")
    else:
        before = {c["seed"]: dict(c["baseline"], opening_matches=True) for c in cases}
        print("Historical V41 comparison, NOT a controlled same-engine A/B: old binary/data hashes unavailable.")
        if meta["jobs"] != 16:
            print("WARNING: historical run reported 16 jobs; wallclock results are not directly comparable.")
    valid = []
    missing, errors, drift = [], [], []
    for c in cases:
        s = c["seed"]
        a, b = before.get(s), after.get(s)
        if a is None or b is None:
            missing.append(s)
        elif a["outcome"] not in OUTCOMES[:4] or b["outcome"] not in OUTCOMES[:4]:
            errors.append(s)
        elif (not a["opening_matches"] or not b["opening_matches"]
              or (args.before and a.get("launch_config") != b.get("launch_config"))):
            drift.append(s)
        else:
            # Enrich comparison copies rather than change historical manifest
            # or any authenticated result receipt.
            a = dict(a, observed_tick_rate=observed_tick_rate(a))
            b = dict(b, observed_tick_rate=observed_tick_rate(b))
            valid.append((c, a, b))
    print(f"paired={len(valid)}/{len(cases)} missing={len(missing)} errors={len(errors)} opening_drift={len(drift)}")
    if missing or errors or drift:
        print(f"missing seeds={missing}; error seeds={errors}; opening drift seeds={drift}")
    transitions = Counter((a["outcome"], b["outcome"]) for _, a, b in valid)
    print("before -> after", *OUTCOMES[:4], sep="\t")
    for a in OUTCOMES[:4]:
        print(a, *(transitions[a, b] for b in OUTCOMES[:4]), sep="\t")
    def summary(rows):
        n = len(rows)
        count = Counter(r["outcome"] for r in rows)
        decided = count["won"] + count["lost"]
        return dict(n=n, **{k: count[k] for k in OUTCOMES[:4]},
                    decided_win_rate=round(count["won"] / decided, 4) if decided else None,
                    all_win_rate=round(count["won"] / n, 4) if n else None,
                    early_losses=sum(r["outcome"] == "lost" and r["tick"] < 30000 for r in rows))
    print("paired before:", summary([a for _, a, _ in valid]))
    print("paired after: ", summary([b for _, _, b in valid]))
    print("lost->won seeds:", [c["seed"] for c, a, b in valid if a["outcome"] == "lost" and b["outcome"] == "won"])
    print("won->lost seeds:", [c["seed"] for c, a, b in valid if a["outcome"] == "won" and b["outcome"] == "lost"])
    def diagnostic_summary(rows):
        def total(key):
            values = [r[key] for r in rows if r.get(key) is not None]
            return sum(values) if values else None
        def median(key):
            values = [r[key] for r in rows if r.get(key) is not None]
            return statistics.median(values) if values else None
        return dict(wall_warnings=total("wall_warnings"),
                    site_release_requests=total("site_release_requests"),
                    wall_product_departures=total("wall_product_departures"),
                    recovery_entries=total("recovery_entries"),
                    recovery_ready_exits=total("recovery_ready_exits"),
                    recovery_timeout_exits=total("recovery_timeout_exits"),
                    negative_power_samples=total("negative_power_samples"),
                    median_peak_pts=median("peak_pts"),
                    opening_observed_cases=sum(r.get("opening_purchase_requests") is not None for r in rows),
                    wave_ready_cases=(sum(r.get("wave_ready_tick") is not None for r in rows)
                                      if any(r.get("opening_purchase_requests") is not None for r in rows) else None),
                    median_wave_ready_tick=median("wave_ready_tick"),
                    opening_deadline_cases=(sum(r.get("opening_deadline_tick") is not None for r in rows)
                                            if any(r.get("opening_purchase_requests") is not None for r in rows) else None),
                    opening_purchase_requests=total("opening_purchase_requests"),
                    local_counter_requests=total("local_counter_requests"),
                    safety_veto_reports=total("safety_veto_reports"),
                    casualty_entries=total("casualty_entries"),
                    casualty_recovered=total("casualty_recovered"),
                    readiness_observed_cases=sum(r.get("readiness_samples") is not None for r in rows),
                    median_available_hp_pct=median("median_available_hp_pct"),
                    median_peak_available_hp=median("peak_available_hp"),
                    median_max_unavailable_hp=median("max_unavailable_hp"),
                    siege_post_requests=total("siege_post_requests"),
                    economy_rest_entries=total("economy_rest_entries"),
                    economy_rest_samples=total("economy_rest_samples"),
                    front_deferred_events=total("front_deferred_events"),
                    local_contact_samples=total("local_contact_samples"),
                    median_factory_pending_peak=median("factory_pending_peak"),
                    factory_handoff_ends=total("factory_handoff_ends"),
                    factory_handoff_live_ends=total("factory_handoff_live_ends"),
                    factory_handoff_stalled=total("factory_handoff_stalled"),
                    factory_handoff_veto_reports=total("factory_handoff_veto_reports"),
                    expansion_risk_veto_reports=total("expansion_risk_veto_reports"),
                    **{key: total(key) for key in (*ASSAULT_FIELDS,*FEEDBACK_FIELDS[:5],"budget_samples","budget_low_cash_samples","rest_proposal_samples")},
                    **{key: median(key) for key in ("native_spent_delta","peak_committed_credits","peak_investment_bank","assault_max_retry_cooldown")},
                    median_observed_tick_rate=median("observed_tick_rate"),
                    losses_without_yard_sample=sum(r["outcome"] == "lost" and r.get("last_yards") == 0 for r in rows))
    print("paired diagnostics before:", diagnostic_summary([a for _, a, _ in valid]))
    print("paired diagnostics after: ", diagnostic_summary([b for _, _, b in valid]))
    if args.output:
        fields = ("outcome", "tick", "tick_source", "seconds", "peak_pts", "last_yards",
                  "negative_power_samples", "wall_warnings", "site_release_requests",
                  "wall_product_departures", "recovery_entries", "recovery_ready_exits", "recovery_timeout_exits", *MOBILIZATION_FIELDS, *SAFETY_FIELDS, *READINESS_FIELDS, *CONVERGENCE_FIELDS, *FACTORY_FIELDS, *ASSAULT_FIELDS, *ECONOMY_FIELDS, *FEEDBACK_FIELDS, "observed_tick_rate")
        buf = io.StringIO(newline="")
        writer = csv.writer(buf)
        writer.writerow(["seed", "map", "local", "opponent", *[f"{side}_{key}" for side in ("before", "after") for key in fields]])
        for c, a, b in valid:
            writer.writerow([c["seed"], c["map"], c["local"], c["opponent"], *[row.get(key) for row in (a, b) for key in fields]])
        atomic_text(args.output, buf.getvalue())
    for field in ("local", "opponent", "map"):
        buckets = defaultdict(list)
        for c, a, b in valid:
            buckets[c[field]].append((a, b))
        print(f"\n{field}: n / wins-before -> wins-after / losses-before -> losses-after")
        for k, pairs in sorted(buckets.items()):
            print(k, len(pairs), f"{sum(a['outcome']=='won' for a,b in pairs)}->{sum(b['outcome']=='won' for a,b in pairs)}",
                  f"{sum(a['outcome']=='lost' for a,b in pairs)}->{sum(b['outcome']=='lost' for a,b in pairs)}")
    return 2 if missing or errors or drift else 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    imp = sub.add_parser("import-v41", help="rebuild historical manifest from original archive")
    imp.add_argument("archive", type=Path)
    imp.add_argument("--output", type=Path, default=DEFAULT_MANIFEST)
    for name in ("run", "compare"):
        p = sub.add_parser(name)
        p.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
        p.add_argument("--suite", choices=("smoke", "dev", "holdout", "all"), default="smoke")
        p.add_argument("--run-dir", type=Path, required=True)
        if name == "run":
            p.add_argument("--stage", choices=("smoke", "dev", "holdout", "all"),
                           help="run only this subset of the fixed target suite; later stages reuse receipts")
            p.add_argument("--binary", type=Path, required=True, help="explicit freshly built autoplay-match executable")
            p.add_argument("--ra2-dir", type=Path, default=Path(os.environ["RA2_DIR"]) if os.environ.get("RA2_DIR") else None, required=not os.environ.get("RA2_DIR"))
            p.add_argument("--label", required=True, help="source/version description; no implicit build")
            p.add_argument("--source-dir", type=Path, default=ROOT, help="checkout from which the explicit binary was built")
            p.add_argument("--jobs", type=int, default=16)
        else:
            p.add_argument("--before", type=Path, help="compare against another run directory rather than historical V41")
            p.add_argument("--output", type=Path, help="write a detailed CSV of valid paired results")
    args = parser.parse_args(argv)
    try:
        if args.action == "import-v41":
            data = import_v41(args.archive)
            atomic_json(args.output, data)
            print(f"imported {len(data['cases'])} cases; { {k: len(v) for k,v in data['suites'].items()} }")
            return 0
        return execute(args) if args.action == "run" else compare(args)
    except KeyboardInterrupt:
        print("interrupted; completed receipts remain on disk", file=sys.stderr)
        return 130
    except (OSError, ValueError, KeyError) as exc:
        print(f"benchmark: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
