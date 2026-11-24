"""Strict runtime validation for matrix and evidence documents."""

from __future__ import annotations

from collections import Counter
import hashlib
from pathlib import Path, PurePosixPath, PureWindowsPath
import re

from . import GENERATOR_NAME, GENERATOR_VERSION, SCHEMA_VERSION
from .catalog import (
    RA2TS_CURRENT_POLICY,
    RESOLUTIONS,
    SCOPE_EXCLUSIONS,
    SOURCES,
    STANDARD_POLICY,
    build_blockers,
    build_rows,
    catalog_snapshot,
    resolution_token,
)
from .io import MatrixError, canonical_json_bytes, sha256_bytes


ROW_STATUSES = ("DRIFT", "DRIFT_FIXED_UNVERIFIED", "UNVERIFIED", "VERIFIED")
ROW_FAMILIES = (
    "audio",
    "control",
    "first-tactical",
    "input",
    "loading-branch",
    "loading-cadence",
    "paint",
    "pointer",
    "transition",
)
REQUIREMENTS = (
    "cursor",
    "focus",
    "frames",
    "input",
    "loading",
    "music",
    "pixels",
    "route",
    "text",
    "transition",
    "ui-sound",
)
VERIFICATION_POLICIES = (RA2TS_CURRENT_POLICY, STANDARD_POLICY)
EVIDENCE_KINDS = (
    "exhaustive-proof",
    "native-executable-differential",
    "production-regression",
    "static-research",
)
RESULT_STATUSES = ("DRIFT", "DRIFT_FIXED_UNVERIFIED", "UNVERIFIED", "VERIFIED")
COMPARISON_RESULTS = ("DRIFT", "DRIFT_FIXED_UNVERIFIED", "INCOMPARABLE", "MATCH", "NOT_RUN")
PROOF_GRADE_KINDS = ("exhaustive-proof", "native-executable-differential")
BLOCKER_STATUSES = ("RESOLVED", "UNKNOWN")
CERTIFICATION_STATES = ("IN_PROGRESS", "VERIFIED")

_SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
_ID_RE = re.compile(r"^[a-z][a-z0-9-]*:[A-Za-z0-9][A-Za-z0-9.:-]*$")


def _fail(path: str, message: str) -> None:
    raise MatrixError(f"{path}: {message}")


def _object(value: object, path: str, keys: set[str]) -> dict[str, object]:
    if not isinstance(value, dict):
        _fail(path, "expected object")
    actual = set(value)
    if actual != keys:
        _fail(path, f"keys differ; missing={sorted(keys - actual)}, unknown={sorted(actual - keys)}")
    return value


def _array(value: object, path: str) -> list[object]:
    if not isinstance(value, list):
        _fail(path, "expected array")
    return value


def _string(value: object, path: str, *, nullable: bool = False) -> str | None:
    if value is None and nullable:
        return None
    if not isinstance(value, str) or not value:
        _fail(path, "expected non-empty string")
    return value


def _integer(value: object, path: str, *, minimum: int | None = None) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        _fail(path, "expected integer")
    if minimum is not None and value < minimum:
        _fail(path, f"must be >= {minimum}")
    return value


def _enum(value: object, path: str, allowed: tuple[str, ...]) -> str:
    result = _string(value, path)
    assert result is not None
    if result not in allowed:
        _fail(path, f"expected one of {list(allowed)}, got {result!r}")
    return result


def _sha(value: object, path: str, *, nullable: bool = False) -> str | None:
    result = _string(value, path, nullable=nullable)
    if result is not None and not _SHA256_RE.fullmatch(result):
        _fail(path, "expected lowercase SHA-256")
    return result


def _portable_path(value: object, path: str) -> str:
    result = _string(value, path)
    assert result is not None
    if (
        "\\" in result
        or ":" in result
        or PurePosixPath(result).is_absolute()
        or PureWindowsPath(result).is_absolute()
        or any(part in {"", ".", ".."} for part in result.split("/"))
    ):
        _fail(path, f"expected portable relative path, got {result!r}")
    return result


def _sorted_unique(values: list[object], path: str, key) -> None:
    keys = [key(value) for value in values]
    if keys != sorted(keys) or len(keys) != len(set(keys)):
        _fail(path, "must be sorted and unique")


def _string_array(
    value: object,
    path: str,
    *,
    allowed: tuple[str, ...] | None = None,
) -> list[str]:
    result = []
    for index, item in enumerate(_array(value, path)):
        string = _string(item, f"{path}[{index}]")
        assert string is not None
        if allowed is not None and string not in allowed:
            _fail(f"{path}[{index}]", f"expected one of {list(allowed)}")
        result.append(string)
    _sorted_unique(result, path, lambda item: item)
    return result


def _resolution(value: object, path: str) -> dict[str, int]:
    obj = _object(value, path, {"height", "width"})
    return {
        "height": _integer(obj["height"], f"{path}.height", minimum=1),
        "width": _integer(obj["width"], f"{path}.width", minimum=1),
    }


def _comparison_contract(value: object, path: str) -> dict[str, object]:
    obj = _object(
        value,
        path,
        {
            "color_space",
            "crop",
            "cursor_policy",
            "frame_timing",
            "resolutions",
            "scaling",
            "surface_region",
        },
    )
    resolutions = _string_array(obj["resolutions"], f"{path}.resolutions")
    if not resolutions:
        _fail(f"{path}.resolutions", "must not be empty")
    for token in resolutions:
        if not re.fullmatch(r"[1-9][0-9]*x[1-9][0-9]*", token):
            _fail(f"{path}.resolutions", f"invalid resolution token {token!r}")
    return {
        "color_space": _string(obj["color_space"], f"{path}.color_space"),
        "crop": _string(obj["crop"], f"{path}.crop"),
        "cursor_policy": _string(obj["cursor_policy"], f"{path}.cursor_policy"),
        "frame_timing": _string(obj["frame_timing"], f"{path}.frame_timing"),
        "resolutions": resolutions,
        "scaling": _string(obj["scaling"], f"{path}.scaling"),
        "surface_region": _string(obj["surface_region"], f"{path}.surface_region"),
    }


def _artifact(value: object, path: str) -> dict[str, str]:
    obj = _object(value, path, {"id", "path", "sha256"})
    artifact_id = _string(obj["id"], f"{path}.id")
    assert artifact_id is not None
    if not _ID_RE.fullmatch(artifact_id):
        _fail(f"{path}.id", "must be namespaced")
    digest = _sha(obj["sha256"], f"{path}.sha256")
    assert digest is not None
    return {
        "id": artifact_id,
        "path": _portable_path(obj["path"], f"{path}.path"),
        "sha256": digest,
    }


def _evidence_record(value: object, path: str) -> dict[str, object]:
    obj = _object(
        value,
        path,
        {
            "artifacts",
            "comparison_contract",
            "id",
            "kind",
            "proof_domain",
        },
    )
    evidence_id = _string(obj["id"], f"{path}.id")
    assert evidence_id is not None
    if not evidence_id.startswith("evidence:") or not _ID_RE.fullmatch(evidence_id):
        _fail(f"{path}.id", "must use the evidence: namespace")
    kind = _enum(obj["kind"], f"{path}.kind", EVIDENCE_KINDS)
    artifacts = [
        _artifact(item, f"{path}.artifacts[{index}]")
        for index, item in enumerate(_array(obj["artifacts"], f"{path}.artifacts"))
    ]
    if not artifacts:
        _fail(f"{path}.artifacts", "at least one hashed artifact is required")
    _sorted_unique(artifacts, f"{path}.artifacts", lambda item: (item["id"], item["path"]))
    proof_domain = _string(obj["proof_domain"], f"{path}.proof_domain", nullable=True)
    comparison = None
    if obj["comparison_contract"] is not None:
        comparison = _comparison_contract(obj["comparison_contract"], f"{path}.comparison_contract")

    if kind == "native-executable-differential":
        if comparison is None or proof_domain is not None:
            _fail(
                path,
                "native evidence requires a comparison contract and forbids proof_domain",
            )
    elif kind == "exhaustive-proof":
        if proof_domain is None or comparison is not None:
            _fail(
                path,
                "exhaustive proof requires proof_domain and forbids a comparison contract",
            )
    elif any(item is not None for item in (proof_domain, comparison)):
        _fail(path, "non-proof evidence forbids proof/comparison fields")

    return {
        "artifacts": artifacts,
        "comparison_contract": comparison,
        "id": evidence_id,
        "kind": kind,
        "proof_domain": proof_domain,
    }


def _evidence_records(value: object, path: str) -> list[dict[str, object]]:
    records = [
        _evidence_record(item, f"{path}[{index}]")
        for index, item in enumerate(_array(value, path))
    ]
    _sorted_unique(records, path, lambda item: item["id"])
    return records


def _verify_evidence_artifacts(
    evidence: list[dict[str, object]],
    artifact_root: Path | None,
) -> None:
    if not evidence:
        return
    if artifact_root is None:
        _fail("$.evidence", "artifact_root is required when evidence is present")
    try:
        root = artifact_root.resolve(strict=True)
    except OSError as exc:
        _fail("$.evidence", f"cannot resolve artifact_root {artifact_root}: {exc}")
    if not root.is_dir():
        _fail("$.evidence", f"artifact_root is not a directory: {root}")

    for evidence_index, record in enumerate(evidence):
        for artifact_index, artifact in enumerate(record["artifacts"]):
            path = f"$.evidence[{evidence_index}].artifacts[{artifact_index}]"
            candidate = root.joinpath(*artifact["path"].split("/"))
            try:
                resolved = candidate.resolve(strict=True)
            except OSError as exc:
                _fail(f"{path}.path", f"artifact is missing or unreadable: {exc}")
            try:
                resolved.relative_to(root)
            except ValueError:
                _fail(f"{path}.path", "artifact resolves outside artifact_root")
            if not resolved.is_file():
                _fail(f"{path}.path", f"artifact is not a regular file: {resolved}")
            digest = hashlib.sha256()
            try:
                with resolved.open("rb") as stream:
                    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                        digest.update(chunk)
            except OSError as exc:
                _fail(f"{path}.path", f"artifact cannot be read: {exc}")
            actual = digest.hexdigest()
            if actual != artifact["sha256"]:
                _fail(
                    f"{path}.sha256",
                    f"artifact digest mismatch: expected {artifact['sha256']}, got {actual}",
                )


def validate_evidence_manifest(value: object) -> dict[str, object]:
    """Validate and normalize an optional evidence/status overlay."""

    obj = _object(
        value,
        "$",
        {"blocker_resolutions", "evidence", "row_results", "schema_version"},
    )
    if _integer(obj["schema_version"], "$.schema_version") != SCHEMA_VERSION:
        _fail("$.schema_version", f"unsupported version; expected {SCHEMA_VERSION}")
    evidence = _evidence_records(obj["evidence"], "$.evidence")
    evidence_ids = {item["id"] for item in evidence}

    row_results = []
    for index, item in enumerate(_array(obj["row_results"], "$.row_results")):
        path = f"$.row_results[{index}]"
        result = _object(
            item,
            path,
            {
                "comparison_id",
                "comparison_result",
                "native_ids",
                "owner",
                "residuals",
                "row_id",
                "rust_ids",
                "status",
            },
        )
        comparison_id = _string(
            result["comparison_id"], f"{path}.comparison_id", nullable=True
        )
        native_ids = _string_array(result["native_ids"], f"{path}.native_ids")
        rust_ids = _string_array(result["rust_ids"], f"{path}.rust_ids")
        referenced = set(native_ids) | set(rust_ids)
        if comparison_id is not None:
            referenced.add(comparison_id)
        unknown = sorted(referenced - evidence_ids)
        if unknown:
            _fail(path, f"references unknown evidence: {unknown}")
        row_results.append(
            {
                "comparison_id": comparison_id,
                "comparison_result": _enum(
                    result["comparison_result"],
                    f"{path}.comparison_result",
                    COMPARISON_RESULTS,
                ),
                "native_ids": native_ids,
                "owner": _string(result["owner"], f"{path}.owner", nullable=True),
                "residuals": _string_array(result["residuals"], f"{path}.residuals"),
                "row_id": _string(result["row_id"], f"{path}.row_id"),
                "rust_ids": rust_ids,
                "status": _enum(result["status"], f"{path}.status", RESULT_STATUSES),
            }
        )
    _sorted_unique(row_results, "$.row_results", lambda item: item["row_id"])

    resolutions = []
    for index, item in enumerate(_array(obj["blocker_resolutions"], "$.blocker_resolutions")):
        path = f"$.blocker_resolutions[{index}]"
        resolution = _object(item, path, {"blocker_id", "evidence_id"})
        evidence_id = _string(resolution["evidence_id"], f"{path}.evidence_id")
        assert evidence_id is not None
        if evidence_id not in evidence_ids:
            _fail(f"{path}.evidence_id", "references unknown evidence")
        resolutions.append(
            {
                "blocker_id": _string(resolution["blocker_id"], f"{path}.blocker_id"),
                "evidence_id": evidence_id,
            }
        )
    _sorted_unique(resolutions, "$.blocker_resolutions", lambda item: item["blocker_id"])

    used = set()
    for item in row_results:
        used.update(item["native_ids"])
        used.update(item["rust_ids"])
        if item["comparison_id"] is not None:
            used.add(item["comparison_id"])
    used.update(item["evidence_id"] for item in resolutions)
    unused = sorted(evidence_ids - used)
    if unused:
        _fail("$.evidence", f"unreferenced evidence records: {unused}")
    if not evidence and not row_results and not resolutions:
        _fail("$", "empty evidence manifest has no effect; omit it")
    return {
        "blocker_resolutions": resolutions,
        "evidence": evidence,
        "row_results": row_results,
        "schema_version": SCHEMA_VERSION,
    }


_IMMUTABLE_BLOCKER_FIELDS = (
    "description",
    "evidence_needed",
    "id",
    "source_refs",
)

_IMMUTABLE_ROW_FIELDS = (
    "blocker_ids",
    "checkpoint",
    "family",
    "id",
    "requirements",
    "resolution",
    "source_refs",
    "state",
    "variant",
    "verification_policy",
)


def _validate_current_catalog_binding(
    *,
    catalog_digest: str,
    sources: list[str],
    resolutions: list[dict[str, int]],
    exclusions: list[dict[str, object]],
    blockers: list[dict[str, object]],
    rows: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Bind every immutable matrix field to the current generated catalog."""

    expected_digest = sha256_bytes(canonical_json_bytes(catalog_snapshot()))
    if catalog_digest != expected_digest:
        _fail(
            "$.catalog_digest",
            f"does not match current catalog; expected {expected_digest}",
        )
    if sources != list(SOURCES):
        _fail("$.sources", "does not match the current catalog source set")
    if resolutions != list(RESOLUTIONS):
        _fail("$.resolutions", "does not match the current catalog resolution set")
    if exclusions != list(SCOPE_EXCLUSIONS):
        _fail("$.scope_exclusions", "does not match the current catalog exclusions")

    expected_blockers = build_blockers()
    actual_blocker_ids = [item["id"] for item in blockers]
    expected_blocker_ids = [item["id"] for item in expected_blockers]
    if actual_blocker_ids != expected_blocker_ids:
        _fail(
            "$.catalog_blockers",
            "blocker IDs do not match the current catalog",
        )
    for actual, expected in zip(blockers, expected_blockers, strict=True):
        for field in _IMMUTABLE_BLOCKER_FIELDS:
            if actual[field] != expected[field]:
                _fail(
                    f"$.catalog_blockers[{actual['id']}].{field}",
                    "does not match the current catalog",
                )

    expected_rows = build_rows()
    actual_row_ids = [item["id"] for item in rows]
    expected_row_ids = [item["id"] for item in expected_rows]
    if actual_row_ids != expected_row_ids:
        _fail("$.rows", "row IDs do not match the current catalog")
    for actual, expected in zip(rows, expected_rows, strict=True):
        for field in _IMMUTABLE_ROW_FIELDS:
            if actual[field] != expected[field]:
                _fail(
                    f"$.rows[{actual['id']}].{field}",
                    "does not match the current catalog",
                )
    return expected_rows


def _matrix_overlay_digest(
    *,
    evidence: list[dict[str, object]],
    blockers: list[dict[str, object]],
    rows: list[dict[str, object]],
    baseline_rows: list[dict[str, object]],
) -> str | None:
    """Reconstruct the normalized overlay embedded in a generated matrix."""

    blocker_resolutions = [
        {
            "blocker_id": blocker["id"],
            "evidence_id": blocker["evidence_id"],
        }
        for blocker in blockers
        if blocker["status"] == "RESOLVED"
    ]
    baseline_by_id = {row["id"]: row for row in baseline_rows}
    row_results = []
    for row in rows:
        baseline = baseline_by_id[row["id"]]
        if (
            row["comparison_result"] == baseline["comparison_result"]
            and row["evidence"] == baseline["evidence"]
            and row["owner"] == baseline["owner"]
            and row["residuals"] == baseline["residuals"]
            and row["status"] == baseline["status"]
        ):
            continue
        row_results.append(
            {
                "comparison_id": row["evidence"]["comparison_id"],
                "comparison_result": row["comparison_result"],
                "native_ids": row["evidence"]["native_ids"],
                "owner": row["owner"],
                "residuals": row["residuals"],
                "row_id": row["id"],
                "rust_ids": row["evidence"]["rust_ids"],
                "status": row["status"],
            }
        )

    if not evidence and not blocker_resolutions and not row_results:
        return None
    manifest = {
        "blocker_resolutions": blocker_resolutions,
        "evidence": evidence,
        "row_results": row_results,
        "schema_version": SCHEMA_VERSION,
    }
    return sha256_bytes(canonical_json_bytes(manifest))


def _validate_matrix_semantics(
    *,
    blockers: list[dict[str, object]],
    evidence: list[dict[str, object]],
    rows: list[dict[str, object]],
) -> None:
    evidence_by_id = {item["id"]: item for item in evidence}
    blocker_by_id = {item["id"]: item for item in blockers}
    used_evidence: set[str] = set()

    for blocker in blockers:
        evidence_id = blocker["evidence_id"]
        if blocker["status"] == "UNKNOWN":
            if evidence_id is not None:
                _fail(f"$.catalog_blockers[{blocker['id']}].evidence_id", "UNKNOWN forbids evidence")
            continue
        if evidence_id not in evidence_by_id:
            _fail(f"$.catalog_blockers[{blocker['id']}].evidence_id", "unknown evidence")
        record = evidence_by_id[evidence_id]
        if record["kind"] not in PROOF_GRADE_KINDS:
            _fail(f"$.catalog_blockers[{blocker['id']}]", "resolution requires proof-grade evidence")
        used_evidence.add(evidence_id)

    for row in rows:
        status = row["status"]
        row_evidence = row["evidence"]
        role_ids = set(row_evidence["native_ids"]) | set(row_evidence["rust_ids"])
        comparison_id = row_evidence["comparison_id"]
        if comparison_id is not None:
            role_ids.add(comparison_id)
        unknown = sorted(role_ids - set(evidence_by_id))
        if unknown:
            _fail(f"$.rows[{row['id']}].evidence", f"unknown evidence: {unknown}")
        used_evidence.update(role_ids)
        comparison_result = row["comparison_result"]
        if comparison_result != "NOT_RUN" and comparison_id is None:
            _fail(
                f"$.rows[{row['id']}].comparison_result",
                "a comparison result requires comparison evidence",
            )
        if status == "DRIFT" and comparison_result != "DRIFT":
            _fail(f"$.rows[{row['id']}].status", "DRIFT requires comparison_result DRIFT")
        if comparison_result == "DRIFT" and status != "DRIFT":
            _fail(
                f"$.rows[{row['id']}].comparison_result",
                "comparison_result DRIFT requires status DRIFT",
            )
        if (
            status == "DRIFT_FIXED_UNVERIFIED"
            and comparison_result != "DRIFT_FIXED_UNVERIFIED"
        ):
            _fail(
                f"$.rows[{row['id']}].status",
                "DRIFT_FIXED_UNVERIFIED requires the same comparison result",
            )
        if (
            comparison_result == "DRIFT_FIXED_UNVERIFIED"
            and status != "DRIFT_FIXED_UNVERIFIED"
        ):
            _fail(
                f"$.rows[{row['id']}].comparison_result",
                "comparison_result DRIFT_FIXED_UNVERIFIED requires the same status",
            )
        if status != "VERIFIED":
            continue
        if comparison_result != "MATCH" or comparison_id is None:
            _fail(
                f"$.rows[{row['id']}].status",
                "VERIFIED requires MATCH and comparison evidence",
            )
        record = evidence_by_id[comparison_id]
        if record["kind"] not in PROOF_GRADE_KINDS:
            _fail(f"$.rows[{row['id']}].status", "VERIFIED requires proof-grade evidence")
        unresolved = [
            blocker_id
            for blocker_id in row["blocker_ids"]
            if blocker_by_id[blocker_id]["status"] != "RESOLVED"
        ]
        if unresolved:
            _fail(f"$.rows[{row['id']}].status", f"unresolved catalog blockers: {unresolved}")
        if record["kind"] == "native-executable-differential":
            token = resolution_token(row["resolution"])
            if token not in record["comparison_contract"]["resolutions"]:
                _fail(
                    f"$.rows[{row['id']}].status",
                    f"native comparison contract does not cover {token}",
                )

    unused = sorted(set(evidence_by_id) - used_evidence)
    if unused:
        _fail("$.evidence", f"unreferenced evidence records: {unused}")


def validate_matrix(
    value: object,
    *,
    artifact_root: Path | None = None,
) -> dict[str, object]:
    """Validate and normalize a generated matrix artifact."""

    obj = _object(
        value,
        "$",
        {
            "catalog_blockers",
            "catalog_digest",
            "certification_state",
            "coverage",
            "evidence",
            "evidence_digest",
            "generator",
            "resolutions",
            "rows",
            "schema_version",
            "scope_exclusions",
            "sources",
        },
    )
    if _integer(obj["schema_version"], "$.schema_version") != SCHEMA_VERSION:
        _fail("$.schema_version", f"unsupported version; expected {SCHEMA_VERSION}")
    generator = _object(obj["generator"], "$.generator", {"name", "version"})
    if _string(generator["name"], "$.generator.name") != GENERATOR_NAME:
        _fail("$.generator.name", f"expected {GENERATOR_NAME!r}")
    if _integer(generator["version"], "$.generator.version") != GENERATOR_VERSION:
        _fail("$.generator.version", f"expected {GENERATOR_VERSION}")
    catalog_digest = _sha(obj["catalog_digest"], "$.catalog_digest")
    assert catalog_digest is not None
    evidence_digest = _sha(obj["evidence_digest"], "$.evidence_digest", nullable=True)
    sources = _string_array(obj["sources"], "$.sources")
    for index, source in enumerate(sources):
        _portable_path(source, f"$.sources[{index}]")

    resolutions = [
        _resolution(item, f"$.resolutions[{index}]")
        for index, item in enumerate(_array(obj["resolutions"], "$.resolutions"))
    ]
    _sorted_unique(resolutions, "$.resolutions", lambda item: (item["width"], item["height"]))
    resolution_keys = {(item["width"], item["height"]) for item in resolutions}

    evidence = _evidence_records(obj["evidence"], "$.evidence")
    _verify_evidence_artifacts(evidence, artifact_root)

    exclusions = []
    for index, item in enumerate(_array(obj["scope_exclusions"], "$.scope_exclusions")):
        path = f"$.scope_exclusions[{index}]"
        exclusion = _object(item, path, {"description", "id", "source_refs"})
        exclusion_id = _string(exclusion["id"], f"{path}.id")
        assert exclusion_id is not None
        if not exclusion_id.startswith("scope:") or not _ID_RE.fullmatch(exclusion_id):
            _fail(f"{path}.id", "must use scope: namespace")
        exclusions.append(
            {
                "description": _string(exclusion["description"], f"{path}.description"),
                "id": exclusion_id,
                "source_refs": _string_array(
                    exclusion["source_refs"], f"{path}.source_refs"
                ),
            }
        )
    _sorted_unique(exclusions, "$.scope_exclusions", lambda item: item["id"])

    blockers = []
    for index, item in enumerate(_array(obj["catalog_blockers"], "$.catalog_blockers")):
        path = f"$.catalog_blockers[{index}]"
        blocker = _object(
            item,
            path,
            {"description", "evidence_id", "evidence_needed", "id", "source_refs", "status"},
        )
        blocker_id = _string(blocker["id"], f"{path}.id")
        assert blocker_id is not None
        if not blocker_id.startswith("catalog:") or not _ID_RE.fullmatch(blocker_id):
            _fail(f"{path}.id", "must use catalog: namespace")
        blockers.append(
            {
                "description": _string(blocker["description"], f"{path}.description"),
                "evidence_id": _string(
                    blocker["evidence_id"], f"{path}.evidence_id", nullable=True
                ),
                "evidence_needed": _string(
                    blocker["evidence_needed"], f"{path}.evidence_needed"
                ),
                "id": blocker_id,
                "source_refs": _string_array(blocker["source_refs"], f"{path}.source_refs"),
                "status": _enum(blocker["status"], f"{path}.status", BLOCKER_STATUSES),
            }
        )
    _sorted_unique(blockers, "$.catalog_blockers", lambda item: item["id"])
    blocker_ids = {item["id"] for item in blockers}

    rows = []
    for index, item in enumerate(_array(obj["rows"], "$.rows")):
        path = f"$.rows[{index}]"
        row = _object(
            item,
            path,
            {
                "blocker_ids",
                "checkpoint",
                "comparison_result",
                "evidence",
                "family",
                "id",
                "owner",
                "requirements",
                "residuals",
                "resolution",
                "source_refs",
                "state",
                "status",
                "variant",
                "verification_policy",
            },
        )
        resolution = _resolution(row["resolution"], f"{path}.resolution")
        if (resolution["width"], resolution["height"]) not in resolution_keys:
            _fail(f"{path}.resolution", "not declared in top-level resolutions")
        row_blockers = _string_array(row["blocker_ids"], f"{path}.blocker_ids")
        unknown_blockers = sorted(set(row_blockers) - blocker_ids)
        if unknown_blockers:
            _fail(f"{path}.blocker_ids", f"unknown blockers: {unknown_blockers}")
        variant = row["variant"]
        if not isinstance(variant, dict):
            _fail(f"{path}.variant", "expected object")
        for key, value in variant.items():
            if not isinstance(key, str) or not key:
                _fail(f"{path}.variant", "variant keys must be non-empty strings")
            if isinstance(value, bool) or isinstance(value, (int, str)):
                continue
            _fail(f"{path}.variant.{key}", "variant values must be string, integer, or boolean")
        row_evidence = _object(
            row["evidence"],
            f"{path}.evidence",
            {"comparison_id", "native_ids", "rust_ids"},
        )
        rows.append(
            {
                "blocker_ids": row_blockers,
                "checkpoint": _string(row["checkpoint"], f"{path}.checkpoint"),
                "comparison_result": _enum(
                    row["comparison_result"],
                    f"{path}.comparison_result",
                    COMPARISON_RESULTS,
                ),
                "evidence": {
                    "comparison_id": _string(
                        row_evidence["comparison_id"],
                        f"{path}.evidence.comparison_id",
                        nullable=True,
                    ),
                    "native_ids": _string_array(
                        row_evidence["native_ids"], f"{path}.evidence.native_ids"
                    ),
                    "rust_ids": _string_array(
                        row_evidence["rust_ids"], f"{path}.evidence.rust_ids"
                    ),
                },
                "family": _enum(row["family"], f"{path}.family", ROW_FAMILIES),
                "id": _string(row["id"], f"{path}.id"),
                "owner": _string(row["owner"], f"{path}.owner", nullable=True),
                "requirements": _string_array(
                    row["requirements"], f"{path}.requirements", allowed=REQUIREMENTS
                ),
                "residuals": _string_array(row["residuals"], f"{path}.residuals"),
                "resolution": resolution,
                "source_refs": _string_array(row["source_refs"], f"{path}.source_refs"),
                "state": _string(row["state"], f"{path}.state"),
                "status": _enum(row["status"], f"{path}.status", ROW_STATUSES),
                "variant": dict(sorted(variant.items())),
                "verification_policy": _enum(
                    row["verification_policy"],
                    f"{path}.verification_policy",
                    VERIFICATION_POLICIES,
                ),
            }
        )
    if not rows:
        _fail("$.rows", "matrix must contain rows")
    _sorted_unique(rows, "$.rows", lambda item: item["id"])
    baseline_rows = _validate_current_catalog_binding(
        catalog_digest=catalog_digest,
        sources=sources,
        resolutions=resolutions,
        exclusions=exclusions,
        blockers=blockers,
        rows=rows,
    )
    expected_evidence_digest = _matrix_overlay_digest(
        evidence=evidence,
        blockers=blockers,
        rows=rows,
        baseline_rows=baseline_rows,
    )
    if evidence_digest != expected_evidence_digest:
        _fail(
            "$.evidence_digest",
            f"does not match embedded overlay; expected {expected_evidence_digest!r}",
        )

    coverage = _object(obj["coverage"], "$.coverage", {"by_family", "by_status", "total"})
    expected_family = dict(sorted(Counter(row["family"] for row in rows).items()))
    expected_status = {status: 0 for status in ROW_STATUSES}
    expected_status.update(Counter(row["status"] for row in rows))
    expected_status = dict(sorted(expected_status.items()))
    if coverage["by_family"] != expected_family:
        _fail("$.coverage.by_family", f"does not match rows; expected {expected_family}")
    if coverage["by_status"] != expected_status:
        _fail("$.coverage.by_status", f"does not match rows; expected {expected_status}")
    if _integer(coverage["total"], "$.coverage.total", minimum=0) != len(rows):
        _fail("$.coverage.total", f"expected {len(rows)}")

    _validate_matrix_semantics(blockers=blockers, evidence=evidence, rows=rows)
    expected_certification = (
        "VERIFIED"
        if all(row["status"] == "VERIFIED" for row in rows)
        and all(blocker["status"] == "RESOLVED" for blocker in blockers)
        else "IN_PROGRESS"
    )
    if _enum(
        obj["certification_state"], "$.certification_state", CERTIFICATION_STATES
    ) != expected_certification:
        _fail("$.certification_state", f"expected {expected_certification}")
    return value
