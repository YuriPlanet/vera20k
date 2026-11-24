"""Generate the exact stock-Skirmish shell certification matrix."""

from __future__ import annotations

import argparse
from collections import Counter
from pathlib import Path
import sys

from . import GENERATOR_NAME, GENERATOR_VERSION, SCHEMA_VERSION
from .catalog import (
    RESOLUTIONS,
    SCOPE_EXCLUSIONS,
    SOURCES,
    build_blockers,
    build_rows,
    catalog_snapshot,
    resolution_token,
)
from .io import (
    MatrixError,
    atomic_write,
    canonical_json_bytes,
    load_json_path,
    sha256_bytes,
)
from .validation import PROOF_GRADE_KINDS, ROW_STATUSES, validate_evidence_manifest, validate_matrix


REPO_ROOT = Path(__file__).resolve().parents[2]
OUTPUT_ROOT = (REPO_ROOT / "target" / "exact-shell-ui").resolve()
DEFAULT_OUTPUT = OUTPUT_ROOT / "matrix.v1.json"


def _proof_grade(evidence: dict[str, object], path: str) -> None:
    if evidence["kind"] not in PROOF_GRADE_KINDS:
        raise MatrixError(f"{path}: requires native differential or exhaustive proof evidence")


def build_matrix(
    evidence_manifest: object | None = None,
    *,
    artifact_root: Path | None = None,
) -> dict[str, object]:
    """Build one self-contained matrix, optionally applying validated evidence."""

    rows = build_rows()
    blockers = build_blockers()
    evidence_records: list[dict[str, object]] = []
    evidence_digest = None

    if evidence_manifest is not None:
        manifest = validate_evidence_manifest(evidence_manifest)
        evidence_records = list(manifest["evidence"])
        evidence_digest = sha256_bytes(canonical_json_bytes(manifest))
        evidence_by_id = {item["id"]: item for item in evidence_records}
        blocker_by_id = {item["id"]: item for item in blockers}
        row_by_id = {item["id"]: item for item in rows}

        for resolution in manifest["blocker_resolutions"]:
            blocker_id = resolution["blocker_id"]
            if blocker_id not in blocker_by_id:
                raise MatrixError(f"unknown blocker resolution target: {blocker_id}")
            evidence = evidence_by_id[resolution["evidence_id"]]
            _proof_grade(evidence, f"blocker {blocker_id}")
            blocker_by_id[blocker_id]["status"] = "RESOLVED"
            blocker_by_id[blocker_id]["evidence_id"] = evidence["id"]

        for result in manifest["row_results"]:
            row_id = result["row_id"]
            if row_id not in row_by_id:
                raise MatrixError(f"unknown row result target: {row_id}")
            row = row_by_id[row_id]
            status = result["status"]
            if status == "VERIFIED":
                comparison_id = result["comparison_id"]
                if result["comparison_result"] != "MATCH" or comparison_id is None:
                    raise MatrixError(
                        f"{row_id}: VERIFIED requires MATCH and comparison evidence"
                    )
                evidence = evidence_by_id[comparison_id]
                _proof_grade(evidence, f"row {row_id}")
                unresolved = [
                    blocker_id
                    for blocker_id in row["blocker_ids"]
                    if blocker_by_id[blocker_id]["status"] != "RESOLVED"
                ]
                if unresolved:
                    raise MatrixError(f"{row_id}: unresolved catalog blockers: {unresolved}")
                if evidence["kind"] == "native-executable-differential":
                    token = resolution_token(row["resolution"])
                    if token not in evidence["comparison_contract"]["resolutions"]:
                        raise MatrixError(
                            f"{row_id}: native comparison does not cover {token}"
                        )
            row["status"] = status
            row["comparison_result"] = result["comparison_result"]
            row["evidence"] = {
                "comparison_id": result["comparison_id"],
                "native_ids": list(result["native_ids"]),
                "rust_ids": list(result["rust_ids"]),
            }
            row["owner"] = result["owner"]
            row["residuals"] = list(result["residuals"])

    status_counts = {status: 0 for status in ROW_STATUSES}
    status_counts.update(Counter(row["status"] for row in rows))
    document = {
        "catalog_blockers": blockers,
        "catalog_digest": sha256_bytes(canonical_json_bytes(catalog_snapshot())),
        "certification_state": (
            "VERIFIED"
            if all(row["status"] == "VERIFIED" for row in rows)
            and all(blocker["status"] == "RESOLVED" for blocker in blockers)
            else "IN_PROGRESS"
        ),
        "coverage": {
            "by_family": dict(sorted(Counter(row["family"] for row in rows).items())),
            "by_status": dict(sorted(status_counts.items())),
            "total": len(rows),
        },
        "evidence": evidence_records,
        "evidence_digest": evidence_digest,
        "generator": {"name": GENERATOR_NAME, "version": GENERATOR_VERSION},
        "resolutions": list(RESOLUTIONS),
        "rows": rows,
        "schema_version": SCHEMA_VERSION,
        "scope_exclusions": list(SCOPE_EXCLUSIONS),
        "sources": list(SOURCES),
    }
    validate_matrix(document, artifact_root=artifact_root)
    return document


def _inside_output_root(path: Path) -> Path:
    resolved = path.resolve()
    try:
        resolved.relative_to(OUTPUT_ROOT)
    except ValueError as exc:
        raise MatrixError(f"output must stay under {OUTPUT_ROOT}, got {resolved}") from exc
    return resolved


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument(
        "--artifact-root",
        type=Path,
        default=REPO_ROOT,
        help=(
            "existing root used to resolve and hash evidence artifact paths "
            "(default: repository root)"
        ),
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail unless the existing output is byte-identical to current generation",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        output = _inside_output_root(args.output)
        evidence = load_json_path(args.evidence) if args.evidence else None
        document = build_matrix(evidence, artifact_root=args.artifact_root)
        payload = canonical_json_bytes(document)
        if args.check:
            try:
                existing = output.read_bytes()
            except OSError as exc:
                raise MatrixError(f"cannot read generated output {output}: {exc}") from exc
            if existing != payload:
                raise MatrixError(f"generated output is stale: {output}")
            action = "checked"
        else:
            atomic_write(output, payload)
            action = "wrote"
        unknown = sum(
            blocker["status"] == "UNKNOWN" for blocker in document["catalog_blockers"]
        )
        print(
            f"{action} {output} rows={document['coverage']['total']} "
            f"unknown_blockers={unknown} certification={document['certification_state']}"
        )
        return 0
    except MatrixError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
