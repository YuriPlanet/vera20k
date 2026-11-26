"""Campaign-map arena baseline for the AutoPlay harness.

AutoPlay plays a Battle skirmish, so a campaign map (`mapsmd03.mix`'s `*.map`,
extracted here into `yr_campaigns/`) enters this baseline as terrain and
houses, not as a mission: the load runs in `ScenarioGameMode::Skirmish`, so the
stage's triggers, objectives and team scripts do not run. A row here says how
AutoPlay does on that ground; it never says whether a campaign stage can be
completed. `autoplay/docs/campaign-baseline.md` carries the boundary.

`tools.autoplay_benchmark` owns the per-case machinery: the command line, the
log classification, the receipts and the CSV fields. Its own manifest is a
historical artifact pinned by hash and it refuses a map name that is not
`*.mmx`, which a campaign `*.map` is not. This module therefore supplies only
the campaign manifest, the run loop and a per-map report, and calls that
runner's `run_case`, `load_receipts`, `export_csv` and identity helpers as they
are, so one case runs the same way in both suites.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
import re
import subprocess
import threading
import uuid

from tools import autoplay_benchmark as b

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "autoplay/benchmarks/campaigns.json"
DEFAULT_MAPS = "yr_campaigns"
DEFAULT_JOBS = 16
# Distinct from the historical suite's 20000+, so one seed identifies one suite.
SEED_BASE = 30000
# A campaign file is authored for one side; the local house takes that side and
# the opponent the other. This is the run's fixed assignment, not a claim about
# the mission's own houses.
PREFIX_COUNTRIES = {"all": ("america", "russia"), "sov": ("russia", "america")}
MAP_NAME = re.compile(r"[A-Za-z0-9_. -]+\.(?:mmx|map)", re.I)


def configuration() -> dict:
    """The historical runner's own configuration, minus its opening check.

    `expected_opening` is the retail Battle opening `[10000, 50000, 1, 1]`. A
    campaign map is authored with its own houses and pre-placed forces, so that
    opening cannot hold there; `None` records the opening as unknown-on-purpose
    instead of tuning the constant until a row passes.
    """
    return dict(b.CONFIG, expected_opening=None)


def load_manifest(path: Path, suite: str) -> tuple[dict, list[dict]]:
    """`b.load_manifest` with a campaign map name allowed.

    Same checks, same refusals: a duplicated seed, an unknown country, a suite
    that names a seed the cases do not hold, or a configuration that is not the
    one this module writes all fail closed.
    """
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    if data["schema"] != 1 or data["config"] != configuration():
        raise ValueError("unsupported manifest configuration")
    cases = data["cases"]
    by_seed = {c["seed"]: c for c in cases}
    if len(by_seed) != len(cases):
        raise ValueError("duplicate manifest seed")
    for c in cases:
        if (type(c["seed"]) is not int or not 0 <= c["seed"] < 2**32
                or c["local"] not in b.COUNTRIES or c["opponent"] not in b.COUNTRIES
                or not MAP_NAME.fullmatch(c["map"])):
            raise ValueError(f"invalid case: {c}")
    seeds = data["suites"][suite]
    if not seeds or len(set(seeds)) != len(seeds) or not set(seeds) <= by_seed.keys():
        raise ValueError("invalid suite membership")
    return data, [by_seed[s] for s in seeds]


def build_manifest(maps: Path, output: Path, ra2: Path | None = None) -> dict:
    """Write the campaign manifest from the extracted `*.map` files.

    The name, never the path, is what a case holds: the engine resolves it
    through its own loose/MIX source, so a run works whether the file sits in
    `yr_campaigns/` or only inside `mapsmd03.mix`.
    """
    files = sorted((p for p in Path(maps).iterdir() if p.suffix.lower() == ".map"),
                   key=lambda p: p.name.lower())
    if not files:
        raise SystemExit(f"no *.map in {maps}; extract mapsmd03.mix first")
    cases, sources = [], {}
    for index, path in enumerate(files):
        local, opponent = PREFIX_COUNTRIES.get(path.name[:3].lower(), ("america", "russia"))
        cases.append(dict(seed=SEED_BASE + index, map=path.name, local=local, opponent=opponent))
        sources[path.name] = dict(sha256=b.sha(path), bytes=path.stat().st_size)
    data = dict(schema=1, config=configuration(), maps_dir=str(Path(maps).resolve()),
                ra2_dir=None if ra2 is None else str(Path(ra2).resolve()), sources=sources,
                cases=cases,
                suites=dict(campaign=[c["seed"] for c in cases], all=[c["seed"] for c in cases]))
    b.atomic_json(Path(output), data)
    return data


def identity(args, data: dict) -> dict:
    """The same identity fields as a historical run, plus this suite's own."""
    return dict(schema=1, manifest_sha256=b.sha(args.manifest), suite=args.suite,
                label=args.label, binary=str(args.binary), binary_sha256=b.sha(args.binary),
                ra2_dir=str(args.ra2_dir), retail_inputs=b.input_fingerprint(args.ra2_dir),
                jobs=args.jobs, config=data["config"], runner_sha256=b.sha(Path(__file__)),
                benchmark_runner_sha256=b.sha(Path(b.__file__)),
                **b.source_provenance(args.source_dir), python=sys.version,
                platform=sys.platform, cpu_count=os.cpu_count(),
                performance_env={k: os.environ.get(k) for k in ("RAYON_NUM_THREADS", "VERA20K_IDLE_SLEEP")})


def execute(args) -> int:
    data, cases = load_manifest(args.manifest, args.suite)
    stage = args.stage or args.suite
    _, stage_cases = load_manifest(args.manifest, stage)
    stage_seeds = {c["seed"] for c in stage_cases}
    if not stage_seeds <= {c["seed"] for c in cases}:
        raise ValueError("stage must be a subset of the target suite")
    binary, ra2, run_dir = args.binary.resolve(), args.ra2_dir.resolve(), args.run_dir.resolve()
    if not binary.is_file() or not ra2.is_dir() or args.jobs < 1:
        raise ValueError("need an existing match binary, retail directory and positive jobs")
    run_dir.mkdir(parents=True, exist_ok=True)
    lock = run_dir / "RUNNING.lock"
    # One writer per run directory; a lock left by a hard kill needs a human who
    # checked for live workers, never a guess at process ownership.
    try:
        fd = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    except FileExistsError as exc:
        raise ValueError(f"run directory locked: {lock}") from exc
    try:
        with os.fdopen(fd, "w") as f:
            f.write(str(os.getpid()) + "\n")
        wanted = identity(args, data)
        meta = run_dir / "run.json"
        if meta.exists():
            old = json.loads(meta.read_text(encoding="utf-8"))
            run_id = old.pop("run_id")
            if old != wanted:
                changed = [k for k in wanted if old.get(k) != wanted[k]]
                raise ValueError(f"resume configuration changed ({', '.join(changed)}); "
                                 "use a new run directory")
        else:
            if list((run_dir / "results").glob("*.json")):
                raise ValueError("results exist without run metadata")
            run_id = uuid.uuid4().hex
            b.atomic_json(meta, dict(wanted, run_id=run_id))
        rows = b.load_receipts(run_dir, cases, run_id)
        b.export_csv(run_dir / "results.csv", rows)
        remaining = [c for c in stage_cases if c["seed"] not in rows]
        print(f"{args.label}: {len(rows)}/{len(cases)} recorded; {len(remaining)} pending in {stage}",
              flush=True)
        stop = threading.Event()
        pool = ThreadPoolExecutor(max_workers=args.jobs)
        try:
            futures = [pool.submit(b.run_case, binary, ra2, c, data["config"], run_dir, run_id, stop)
                       for c in remaining]
            for future in as_completed(futures):
                row = future.result()
                if row is None:
                    continue
                rows[row["seed"]] = row
                b.export_csv(run_dir / "results.csv", rows)
                print(f"{len(rows)}/{len(cases)} {b.cell(row)} {row['outcome']} "
                      f"tick={row['tick']} ({row['seconds']:.1f}s)", flush=True)
        finally:
            stop.set()
            pool.shutdown(wait=True, cancel_futures=True)
            b.export_csv(run_dir / "results.csv", b.load_receipts(run_dir, cases, run_id))
        return 0 if all(s in rows and rows[s]["outcome"] in b.OUTCOMES[:4] for s in stage_seeds) else 2
    finally:
        lock.unlink(missing_ok=True)


def report(args) -> int:
    data, cases = load_manifest(args.manifest, args.suite)

    def read_run(path: Path) -> tuple[dict, dict]:
        meta = json.loads((path / "run.json").read_text(encoding="utf-8"))
        if meta["manifest_sha256"] != b.sha(args.manifest):
            raise ValueError("run manifest mismatch")
        return meta, b.load_receipts(path, cases, meta["run_id"])

    meta, rows = read_run(args.run_dir)
    missing = [c["seed"] for c in cases if c["seed"] not in rows]
    if missing:
        print(f"missing receipts: {missing}")
    print(f"{meta['label']}  binary={meta['binary_sha256'][:12]}  jobs={meta['jobs']}  "
          f"head={str(meta.get('source_head'))[:8]}")
    print(f"{'map':<16}{'local':<12}{'opponent':<12}{'outcome':<10}{'tick':>8}{'sec':>8}"
          f"{'rate':>8}{'stall':>7}{'safety':>9}{'veto':>7}")
    for c in cases:
        row = rows.get(c["seed"])
        if row is None:
            print(f"{c['map']:<16}{c['local']:<12}{c['opponent']:<12}{'MISSING':<10}")
            continue
        rate = b.observed_tick_rate(row)
        print(f"{c['map']:<16}{c['local']:<12}{c['opponent']:<12}{row['outcome']:<10}"
              f"{str(row['tick']):>8}{row['seconds']:>8.1f}{str(rate):>8}"
              f"{str(row.get('factory_handoff_stalled')):>7}"
              f"{str(row.get('safety_veto_reports')):>9}"
              f"{str(row.get('front_guard_veto_reports')):>7}")
    valid = [rows[c["seed"]] for c in cases if c["seed"] in rows
             and rows[c["seed"]]["outcome"] in b.OUTCOMES[:4]]
    counts = Counter(r["outcome"] for r in valid)
    decided = counts["won"] + counts["lost"]
    print(f"\ncases={len(cases)} valid={len(valid)} "
          + " ".join(f"{k}={counts[k]}" for k in b.OUTCOMES[:4])
          + f" decided_win_rate={round(counts['won'] / decided, 4) if decided else None}")
    if args.before:
        old_meta, before = read_run(args.before.resolve())
        for key in ("config", "retail_inputs", "jobs", "platform", "cpu_count",
                    "benchmark_runner_sha256", "performance_env"):
            if old_meta.get(key) != meta.get(key):
                raise ValueError(f"comparison configuration mismatch: {key}")
        print(f"before={old_meta['label']}  binary={old_meta['binary_sha256'][:12]}")
        moved = [c for c in cases if c["seed"] in before and c["seed"] in rows
                 and before[c["seed"]]["outcome"] != rows[c["seed"]]["outcome"]]
        print(f"changed={len(moved)}")
        for c in moved:
            a, z = before[c["seed"]], rows[c["seed"]]
            print(f"{c['map']:<16}{a['outcome']:<10}-> {z['outcome']:<10}"
                  f"tick {a['tick']} -> {z['tick']}")
    return 0 if not missing else 2


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description="Campaign-map arena baseline for AutoPlay. A campaign map is "
                    "played as a Battle skirmish: its terrain and houses, not its triggers.")
    sub = parser.add_subparsers(dest="command", required=True)

    m = sub.add_parser("manifest", help="write autoplay/benchmarks/campaigns.json")
    m.add_argument("--maps-dir", type=Path, default=None,
                   help=f"extracted campaign *.map directory (default <ra2-dir>/{DEFAULT_MAPS})")
    m.add_argument("--ra2-dir", type=Path, default=None)
    m.add_argument("--output", type=Path, default=MANIFEST)

    run = sub.add_parser("run", help="play every campaign case")
    run.add_argument("--manifest", type=Path, default=MANIFEST)
    run.add_argument("--suite", choices=("campaign", "all"), default="campaign")
    run.add_argument("--stage", choices=("campaign", "all"), default=None)
    run.add_argument("--run-dir", type=Path, required=True)
    run.add_argument("--binary", type=Path, required=True)
    run.add_argument("--ra2-dir", type=Path, required=True)
    run.add_argument("--label", required=True)
    run.add_argument("--source-dir", type=Path, default=ROOT)
    run.add_argument("--jobs", type=int, default=DEFAULT_JOBS)

    rep = sub.add_parser("report", help="per-map table for one campaign run")
    rep.add_argument("--manifest", type=Path, default=MANIFEST)
    rep.add_argument("--suite", choices=("campaign", "all"), default="campaign")
    rep.add_argument("--run-dir", type=Path, required=True)
    rep.add_argument("--before", type=Path, default=None)

    args = parser.parse_args(argv)
    if args.command == "manifest":
        ra2 = args.ra2_dir
        maps = args.maps_dir or (ra2 / DEFAULT_MAPS if ra2 else Path(DEFAULT_MAPS))
        data = build_manifest(maps, args.output, ra2)
        print(f"{args.output}: {len(data['cases'])} campaign maps, seeds "
              f"{SEED_BASE}..{SEED_BASE + len(data['cases']) - 1}")
        return 0
    if args.command == "run":
        return execute(args)
    return report(args)


if __name__ == "__main__":
    raise SystemExit(main())
