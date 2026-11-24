"""Immutable tactical child orchestration, bundle validation, and repeats."""

from __future__ import annotations

import os
import stat
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Sequence

from tools.child_process import run_child

from .core import (
    INVALID,
    VALID,
    FileSnapshot,
    OutputExistsError,
    ValidationError,
    assert_snapshot_unchanged,
    canonical_json_bytes,
    contains_forbidden_verdict,
    create_directory_exclusive,
    load_json_file,
    reject_reparse_ancestors,
    require_array,
    require_bool,
    require_directory,
    require_exact_keys,
    require_int,
    require_object,
    require_regular_file,
    require_sha256,
    require_string,
    require_value,
    sha256_bytes,
    utc_now,
    write_bytes_exclusive,
    write_json_exclusive,
)
from .evidence_validation import (
    ALLOWED_SURFACE_FORMATS,
    require_identity,
    require_nonuniform_bgra,
    require_run_evidence,
    require_stable_evidence,
)
from .profile import (
    CHECKPOINT,
    CHILD_TIMEOUT_SECONDS,
    CONTRACT_SCHEMA,
    PROFILE_SCHEMA,
    ValidatedContract,
    ValidatedProfile,
    load_contract,
    load_profile,
    reject_denied_environment,
)


CAPTURE_SCHEMA = "vera20k.tactical-capture.v3"
VALIDATION_SCHEMA = "vera20k.tactical-validation.v1"
RUN_SCHEMA = "vera20k.tactical-run.v1"
REPEAT_SCHEMA = "vera20k.tactical-repeat.v1"
CHILD_DIRECTORY_NAME = "capture"
CAPTURE_MANIFEST_NAME = "capture.json"
FRAME_NAME = "frame.bgra"
PROFILE_COPY_NAME = "profile.json"
STDOUT_NAME = "stdout.txt"
STDERR_NAME = "stderr.txt"
VALIDATION_NAME = "validation.json"
RUN_NAME = "run.json"
EXPECTED_SUCCESS_ARTIFACTS = frozenset((CAPTURE_MANIFEST_NAME, FRAME_NAME))


@dataclass(frozen=True)
class EnvironmentInputs:
    working_directory: Path
    config: FileSnapshot
    executable: FileSnapshot
    archive: FileSnapshot
    font_archive: FileSnapshot
    retail_root: Path

    def snapshots(self) -> tuple[tuple[str, FileSnapshot], ...]:
        return (
            ("config.toml", self.config),
            ("VERA executable", self.executable),
            ("retail archive", self.archive),
            ("bitmap font archive", self.font_archive),
        )


@dataclass(frozen=True)
class ValidatedCapture:
    directory: Path
    manifest_snapshot: FileSnapshot
    frame_snapshot: FileSnapshot
    manifest: Mapping[str, Any]
    stable_evidence: Mapping[str, Any]


def _normal_case(path: Path) -> str:
    return os.path.normcase(os.path.normpath(str(path)))


def _canonical_directory(path: Path, label: str) -> Path:
    if not path.is_absolute():
        raise ValidationError(f"{label} must be absolute: {path}")
    if any(part in {".", ".."} for part in path.parts):
        raise ValidationError(f"{label} contains a noncanonical component: {path}")
    directory = require_directory(path, label)
    resolved = Path(os.path.realpath(directory))
    if _normal_case(directory) != _normal_case(resolved):
        raise ValidationError(f"{label} is not canonical: {directory} -> {resolved}")
    return directory


def _parse_config(snapshot: FileSnapshot) -> Mapping[str, Any]:
    try:
        text = snapshot.raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise ValidationError(f"config.toml is not UTF-8: {exc}") from exc
    try:
        parsed = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        raise ValidationError(f"config.toml is malformed: {exc}") from exc
    if not isinstance(parsed, dict):
        raise ValidationError("config.toml root must be a table")
    paths = parsed.get("paths")
    if not isinstance(paths, dict):
        raise ValidationError("config.toml [paths] table is required")
    ra2_dir = paths.get("ra2_dir")
    if not isinstance(ra2_dir, str) or not ra2_dir:
        raise ValidationError("config.toml paths.ra2_dir must be a nonempty string")
    return parsed


def _reject_loose_shadow(path: Path, label: str) -> None:
    if os.path.lexists(path):
        raise ValidationError(f"{label} must be absent: {path}")


def validate_environment_inputs(
    executable_path: str | os.PathLike[str],
    working_directory: str | os.PathLike[str],
    profile: ValidatedProfile,
) -> EnvironmentInputs:
    """Seal every external file that can change this tactical frame."""

    cwd = _canonical_directory(Path(working_directory), "child working directory")
    config = require_regular_file(cwd / "config.toml", "config.toml")
    parsed_config = _parse_config(config)
    paths = require_object(parsed_config["paths"], "config.toml.paths")
    retail_value = require_string(
        paths.get("ra2_dir"), "config.toml.paths.ra2_dir"
    )
    retail_supplied = Path(retail_value)
    if not retail_supplied.is_absolute():
        raise ValidationError("config.toml paths.ra2_dir must be absolute")
    retail_root = _canonical_directory(retail_supplied, "canonical retail root")

    fixture = profile.fixture
    logical_map_name = require_string(
        fixture["logical_map_name"], "fixture.logical_map_name"
    )
    _reject_loose_shadow(cwd / logical_map_name, "working-directory loose map shadow")
    _reject_loose_shadow(
        retail_root / logical_map_name, "retail-root loose map shadow"
    )

    archive_name = require_string(fixture["archive_name"], "fixture.archive_name")
    archive = require_regular_file(
        retail_root / archive_name,
        "canonical retail archive",
        exact_length=require_int(
            fixture["archive_byte_length"], "fixture.archive_byte_length"
        ),
    )
    if archive.sha256 != require_sha256(
        fixture["archive_sha256"], "fixture.archive_sha256"
    ):
        raise ValidationError("canonical retail archive SHA-256 differs from profile")

    pixel_inputs = profile.pixel_inputs
    font_profile = require_object(
        pixel_inputs["bitmap_font"], "pixel_inputs.bitmap_font"
    )
    logical_font_name = require_string(
        font_profile["logical_name"], "pixel_inputs.bitmap_font.logical_name"
    )
    _reject_loose_shadow(cwd / logical_font_name, "working-directory loose font shadow")
    _reject_loose_shadow(retail_root / logical_font_name, "retail-root loose font shadow")
    font_archive = require_regular_file(
        retail_root / require_string(
            font_profile["archive_name"], "pixel_inputs.bitmap_font.archive_name"
        ),
        "bitmap font archive",
        exact_length=require_int(
            font_profile["archive_byte_length"],
            "pixel_inputs.bitmap_font.archive_byte_length",
        ),
    )
    if font_archive.sha256 != require_sha256(
        font_profile["archive_sha256"], "pixel_inputs.bitmap_font.archive_sha256"
    ):
        raise ValidationError("bitmap font archive SHA-256 differs from profile")

    executable = require_regular_file(executable_path, "VERA executable")
    return EnvironmentInputs(
        working_directory=cwd,
        config=config,
        executable=executable,
        archive=archive,
        font_archive=font_archive,
        retail_root=retail_root,
    )


def build_capture_command(
    executable: Path,
    profile: ValidatedProfile,
    contract: ValidatedContract,
    child_output: Path,
) -> list[str]:
    return [
        str(executable),
        "--tactical-capture",
        CHECKPOINT,
        "--profile",
        str(profile.path),
        "--contract",
        str(contract.path),
        "--output",
        str(child_output),
    ]


def _artifact_inventory(directory: Path) -> tuple[list[dict[str, Any]], list[str]]:
    inventory: list[dict[str, Any]] = []
    errors: list[str] = []
    try:
        entries = sorted(os.scandir(directory), key=lambda entry: entry.name)
    except OSError as exc:
        raise ValidationError(f"cannot inventory {directory}: {exc}") from exc
    for entry in entries:
        try:
            metadata = entry.stat(follow_symlinks=False)
            is_reparse = bool(
                getattr(metadata, "st_file_attributes", 0)
                & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400)
            )
            if entry.is_symlink() or is_reparse:
                kind = "link-or-reparse"
                length = None
            elif entry.is_file(follow_symlinks=False):
                kind = "file"
                length = metadata.st_size
            elif entry.is_dir(follow_symlinks=False):
                kind = "directory"
                length = None
            else:
                kind = "other"
                length = None
        except OSError as exc:
            kind = "unreadable"
            length = None
            errors.append(f"cannot inspect child artifact {entry.name!r}: {exc}")
        inventory.append(
            {"name": entry.name, "kind": kind, "byte_length": length}
        )
        if kind != "file":
            errors.append(
                f"child artifact {entry.name!r} must be a regular non-link file"
            )
    return inventory, errors


def _validate_manifest_envelope(
    manifest: Mapping[str, Any],
    profile: ValidatedProfile,
    contract: ValidatedContract,
    environment: EnvironmentInputs,
    *,
    require_complete: bool,
) -> tuple[Mapping[str, Any] | None, Mapping[str, Any] | None]:
    require_exact_keys(
        manifest,
        (
            "schema_version",
            "status",
            "checkpoint",
            "profile",
            "contract",
            "frame",
            "evidence",
            "failure",
            "native_comparator",
            "parity_certification",
            "evidence_limitations",
        ),
        "capture manifest",
    )
    require_value(manifest["schema_version"], CAPTURE_SCHEMA, "schema_version")
    require_value(manifest["checkpoint"], CHECKPOINT, "checkpoint")
    require_value(manifest["native_comparator"], "NONE", "native_comparator")
    require_value(
        manifest["parity_certification"], "NONE", "parity_certification"
    )
    require_identity(
        manifest["profile"],
        "profile",
        profile.snapshot,
        extra={"schema_version": PROFILE_SCHEMA, "profile_id": profile.profile_id},
    )
    contract_identity = require_identity(
        manifest["contract"],
        "contract",
        contract.snapshot,
        extra={
            "schema_version": CONTRACT_SCHEMA,
            "embedded_sha256": contract.snapshot.sha256,
            "bytes_equal": True,
        },
    )
    require_bool(contract_identity["bytes_equal"], "contract.bytes_equal")

    limitations = require_array(manifest["evidence_limitations"], "evidence_limitations")
    if not limitations or any(not isinstance(item, str) or not item for item in limitations):
        raise ValidationError("evidence_limitations must contain nonempty strings")
    require_value(
        limitations,
        list(profile.document["evidence_limitations"]),
        "evidence_limitations",
    )
    if contains_forbidden_verdict(manifest):
        raise ValidationError("capture manifest contains a native verdict without a comparator")

    status = require_string(manifest["status"], "status")
    if status == "COMPLETE":
        if manifest["failure"] is not None:
            raise ValidationError("COMPLETE manifest must not contain failure")
        frame = require_object(manifest["frame"], "frame")
        evidence = require_object(manifest["evidence"], "evidence")
        require_exact_keys(evidence, ("stable", "run"), "evidence")
        stable = require_object(evidence["stable"], "evidence.stable")
        require_stable_evidence(stable, profile, contract, environment)
        require_run_evidence(evidence["run"], profile, stable=evidence["stable"])
        return frame, stable
    if status == "FAILED":
        if manifest["frame"] is not None or manifest["evidence"] is not None:
            raise ValidationError("FAILED manifest must not contain frame or evidence")
        failure = require_object(manifest["failure"], "failure")
        require_exact_keys(failure, ("stage", "message"), "failure")
        require_string(failure["stage"], "failure.stage")
        require_string(failure["message"], "failure.message")
        if require_complete:
            raise ValidationError("capture child reported FAILED")
        return None, None
    raise ValidationError(f"unsupported capture status {status!r}")


def validate_capture_bundle(
    capture_directory: str | os.PathLike[str],
    profile: ValidatedProfile,
    contract: ValidatedContract,
    environment: EnvironmentInputs,
) -> ValidatedCapture:
    directory = require_directory(capture_directory, "child capture directory")
    inventory, inventory_errors = _artifact_inventory(directory)
    if inventory_errors:
        raise ValidationError("; ".join(inventory_errors))
    names = frozenset(item["name"] for item in inventory)
    if names != EXPECTED_SUCCESS_ARTIFACTS:
        raise ValidationError(
            f"successful child artifact set is {sorted(names)}, "
            f"expected {sorted(EXPECTED_SUCCESS_ARTIFACTS)}"
        )

    manifest_snapshot, manifest = load_json_file(
        directory / CAPTURE_MANIFEST_NAME, "tactical capture manifest"
    )
    frame_metadata, stable = _validate_manifest_envelope(
        manifest,
        profile,
        contract,
        environment,
        require_complete=True,
    )
    assert frame_metadata is not None and stable is not None
    frame_snapshot = require_regular_file(directory / FRAME_NAME, "tactical frame")
    require_exact_keys(
        frame_metadata,
        (
            "file_name",
            "width",
            "height",
            "row_stride",
            "byte_length",
            "sha256",
            "surface_format",
            "pixel_layout",
        ),
        "frame",
    )
    require_value(frame_metadata["file_name"], FRAME_NAME, "frame.file_name")
    width = require_int(frame_metadata["width"], "frame.width")
    height = require_int(frame_metadata["height"], "frame.height")
    require_value(width, profile.capture["output_width"], "frame.width")
    require_value(height, profile.capture["output_height"], "frame.height")
    require_value(frame_metadata["row_stride"], width * 4, "frame.row_stride")
    require_value(
        frame_metadata["byte_length"], width * height * 4, "frame.byte_length"
    )
    require_value(
        frame_metadata["byte_length"],
        frame_snapshot.byte_length,
        "frame.byte_length",
    )
    require_value(frame_metadata["sha256"], frame_snapshot.sha256, "frame.sha256")
    surface = require_string(frame_metadata["surface_format"], "frame.surface_format")
    if surface not in ALLOWED_SURFACE_FORMATS:
        raise ValidationError(f"frame.surface_format is unsupported: {surface}")
    stable_graphics = require_object(
        stable["graphics"], "evidence.stable.graphics"
    )
    require_value(
        surface,
        stable_graphics["surface_format"],
        "frame.surface_format",
    )
    require_value(frame_metadata["pixel_layout"], "BGRA8", "frame.pixel_layout")
    require_nonuniform_bgra(frame_snapshot.raw)

    assert_snapshot_unchanged(manifest_snapshot, "tactical capture manifest")
    assert_snapshot_unchanged(frame_snapshot, "tactical frame")
    final_inventory, final_inventory_errors = _artifact_inventory(directory)
    if final_inventory_errors:
        raise ValidationError("; ".join(final_inventory_errors))
    if final_inventory != inventory:
        raise ValidationError("child capture artifact inventory changed during validation")
    return ValidatedCapture(
        directory=directory,
        manifest_snapshot=manifest_snapshot,
        frame_snapshot=frame_snapshot,
        manifest=manifest,
        stable_evidence=stable,
    )


def build_validation_report(
    capture_directory: str | os.PathLike[str],
    profile: ValidatedProfile,
    contract: ValidatedContract,
    environment: EnvironmentInputs,
    *,
    additional_errors: Sequence[str] = (),
) -> tuple[dict[str, Any], ValidatedCapture | None]:
    errors = list(additional_errors)
    capture: ValidatedCapture | None = None
    try:
        capture = validate_capture_bundle(
            capture_directory, profile, contract, environment
        )
    except (ValidationError, OSError) as exc:
        errors.append(str(exc))
    report: dict[str, Any] = {
        "schema_version": VALIDATION_SCHEMA,
        "status": VALID if not errors else INVALID,
        "errors": errors,
        "checkpoint": CHECKPOINT,
        "profile_id": profile.profile_id,
        "native_comparator": "NONE",
        "parity_certification": "NONE",
        "capture": None
        if capture is None
        else {
            "directory": str(capture.directory),
            "manifest_sha256": capture.manifest_snapshot.sha256,
            "frame_sha256": capture.frame_snapshot.sha256,
            "frame_byte_length": capture.frame_snapshot.byte_length,
        },
        "evidence_limitations": list(profile.document["evidence_limitations"]),
    }
    return report, capture


def _outer_inventory_errors(run_directory: Path) -> list[str]:
    allowed_before_reports = {PROFILE_COPY_NAME, CHILD_DIRECTORY_NAME}
    errors: list[str] = []
    try:
        entries = list(os.scandir(run_directory))
    except OSError as exc:
        return [f"cannot inspect wrapper run directory: {exc}"]
    for entry in entries:
        if entry.name not in allowed_before_reports:
            errors.append(
                f"unexpected wrapper-run entry before report publication: {entry.name!r}"
            )
    return errors


def capture_once(
    executable_path: str | os.PathLike[str],
    profile_path: str | os.PathLike[str],
    contract_path: str | os.PathLike[str],
    run_directory: str | os.PathLike[str],
    *,
    working_directory: str | os.PathLike[str],
    _timeout_seconds: float | None = None,
) -> tuple[dict[str, Any], dict[str, Any]]:
    """Run one exact child and retain immutable wrapper-owned diagnostics."""

    profile = load_profile(profile_path)
    contract = load_contract(contract_path)
    reject_denied_environment(contract)
    environment = validate_environment_inputs(
        executable_path, working_directory, profile
    )
    timeout = (
        float(profile.budgets["child_timeout_seconds"])
        if _timeout_seconds is None
        else float(_timeout_seconds)
    )
    if not (0.0 < timeout <= float(profile.budgets["absolute_timeout_max_seconds"])):
        raise ValidationError("child timeout exceeds the tactical schema maximum")
    if _timeout_seconds is None and timeout != CHILD_TIMEOUT_SECONDS:
        raise ValidationError("v2 child timeout must be exactly 720 seconds")

    run_dir = create_directory_exclusive(Path(run_directory), "wrapper run directory")
    write_bytes_exclusive(run_dir / PROFILE_COPY_NAME, profile.snapshot.raw)
    child_output = run_dir / CHILD_DIRECTORY_NAME
    reject_reparse_ancestors(child_output, "child output", include_final=False)
    if os.path.lexists(child_output):
        raise OutputExistsError(f"child output already exists: {child_output}")

    command = build_capture_command(
        environment.executable.path, profile, contract, child_output
    )
    started_at = utc_now()
    child = run_child(command, cwd=environment.working_directory, temporary_directory=run_dir,
                      timeout_seconds=timeout)
    child_pid, exit_status, timed_out = child.pid, child.exit_status, child.timed_out
    stdout, stderr = child.stdout, child.stderr
    errors = list(child.errors)

    finished_at = utc_now()
    if not timed_out:
        if exit_status is None:
            errors.append("capture child has no exit status")
        elif exit_status != 0:
            errors.append(f"capture child exited with nonzero status {exit_status}")

    before_snapshots = {
        label: snapshot for label, snapshot in environment.snapshots()
    }
    before_snapshots["tactical profile"] = profile.snapshot
    before_snapshots["tactical contract"] = contract.snapshot
    after_identities: dict[str, Any] = {}
    for label, snapshot in before_snapshots.items():
        try:
            after = assert_snapshot_unchanged(snapshot, label)
            after_identities[label] = {
                **after.public_identity(),
                "unchanged": True,
            }
        except ValidationError as exc:
            errors.append(str(exc))
            after_identities[label] = {
                "path": str(snapshot.path),
                "byte_length": None,
                "sha256": None,
                "unchanged": False,
            }

    logical_map_name = require_string(
        profile.fixture["logical_map_name"], "fixture.logical_map_name"
    )
    for shadow, label in (
        (
            environment.working_directory / logical_map_name,
            "working-directory loose map shadow",
        ),
        (
            environment.retail_root / logical_map_name,
            "retail-root loose map shadow",
        ),
    ):
        try:
            _reject_loose_shadow(shadow, label)
        except ValidationError as exc:
            errors.append(str(exc))

    errors.extend(_outer_inventory_errors(run_dir))
    validation, capture = build_validation_report(
        child_output,
        profile,
        contract,
        environment,
        additional_errors=errors,
    )
    write_bytes_exclusive(run_dir / STDOUT_NAME, stdout)
    write_bytes_exclusive(run_dir / STDERR_NAME, stderr)
    validation_path = write_json_exclusive(run_dir / VALIDATION_NAME, validation)
    validation_snapshot = require_regular_file(validation_path, "validation report")

    run_report: dict[str, Any] = {
        "schema_version": RUN_SCHEMA,
        "status": validation["status"],
        "errors": validation["errors"],
        "checkpoint": CHECKPOINT,
        "profile_id": profile.profile_id,
        "started_at_utc": started_at,
        "finished_at_utc": finished_at,
        "command": command,
        "working_directory": str(environment.working_directory),
        "retail_root": str(environment.retail_root),
        "child": {
            "pid": child_pid,
            "exit_status": exit_status,
            "timed_out": timed_out,
            "timeout_seconds": timeout,
            "cleanup_scope": "exact-child-pid-only",
        },
        "inputs_before": {
            label: snapshot.public_identity()
            for label, snapshot in before_snapshots.items()
        },
        "inputs_after": after_identities,
        "artifacts": {
            "profile_copy": {
                "path": PROFILE_COPY_NAME,
                "sha256": profile.snapshot.sha256,
                "byte_length": profile.snapshot.byte_length,
            },
            "stdout": {
                "path": STDOUT_NAME,
                "sha256": sha256_bytes(stdout),
                "byte_length": len(stdout),
            },
            "stderr": {
                "path": STDERR_NAME,
                "sha256": sha256_bytes(stderr),
                "byte_length": len(stderr),
            },
            "validation": {
                "path": VALIDATION_NAME,
                "sha256": validation_snapshot.sha256,
                "byte_length": validation_snapshot.byte_length,
            },
            "capture": validation["capture"],
        },
        "native_comparator": "NONE",
        "parity_certification": "NONE",
        "evidence_limitations": list(profile.document["evidence_limitations"]),
    }
    if contains_forbidden_verdict(run_report):
        raise ValidationError("run report contains a native verdict without a comparator")
    write_json_exclusive(run_dir / RUN_NAME, run_report)
    return run_report, validation


def validate_existing(
    capture_directory: str | os.PathLike[str],
    profile_path: str | os.PathLike[str],
    contract_path: str | os.PathLike[str],
    *,
    executable_path: str | os.PathLike[str],
    working_directory: str | os.PathLike[str],
) -> tuple[dict[str, Any], ValidatedCapture | None]:
    profile = load_profile(profile_path)
    contract = load_contract(contract_path)
    reject_denied_environment(contract)
    environment = validate_environment_inputs(
        executable_path, working_directory, profile
    )
    return build_validation_report(
        capture_directory, profile, contract, environment
    )


def validate_repeat(
    first_directory: str | os.PathLike[str],
    second_directory: str | os.PathLike[str],
    profile_path: str | os.PathLike[str],
    contract_path: str | os.PathLike[str],
    *,
    executable_path: str | os.PathLike[str],
    working_directory: str | os.PathLike[str],
) -> dict[str, Any]:
    profile = load_profile(profile_path)
    contract = load_contract(contract_path)
    reject_denied_environment(contract)
    environment = validate_environment_inputs(
        executable_path, working_directory, profile
    )
    errors: list[str] = []
    captures: list[ValidatedCapture | None] = []
    reports: list[dict[str, Any]] = []
    for directory in (first_directory, second_directory):
        report, capture = build_validation_report(
            directory, profile, contract, environment
        )
        reports.append(report)
        captures.append(capture)
        errors.extend(report["errors"])
    first, second = captures
    if first is not None and second is not None:
        same_directory = _normal_case(first.directory) == _normal_case(second.directory)
        try:
            same_directory = same_directory or os.path.samefile(
                first.directory,
                second.directory,
            )
        except OSError as exc:
            errors.append(f"cannot prove repeat capture directories are distinct: {exc}")
        if same_directory:
            errors.append("repeat capture directories must be distinct")
        if first.frame_snapshot.raw != second.frame_snapshot.raw:
            errors.append("same-profile BGRA bytes differ")
        if canonical_json_bytes(first.stable_evidence) != canonical_json_bytes(
            second.stable_evidence
        ):
            errors.append("same-profile stable evidence differs")
        for field in (
            "profile",
            "contract",
            "frame",
            "checkpoint",
            "native_comparator",
            "parity_certification",
            "evidence_limitations",
        ):
            if first.manifest[field] != second.manifest[field]:
                errors.append(f"same-profile manifest field differs: {field}")
    report = {
        "schema_version": REPEAT_SCHEMA,
        "status": VALID if not errors else INVALID,
        "errors": errors,
        "checkpoint": CHECKPOINT,
        "profile_id": profile.profile_id,
        "captures": [item["capture"] for item in reports],
        "compared": {
            "exact_bgra": True,
            "entire_stable_evidence": True,
            "typed_envelope_identities": True,
            "excluded": [
                "evidence.run",
                "host timestamps",
                "run paths",
                "process IDs",
                "durations",
            ],
        },
        "native_comparator": "NONE",
        "parity_certification": "NONE",
        "evidence_limitations": list(profile.document["evidence_limitations"]),
    }
    if contains_forbidden_verdict(report):
        raise ValidationError("repeat report contains a forbidden native verdict")
    return report
