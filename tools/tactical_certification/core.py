"""Small fail-closed filesystem and JSON primitives for tactical evidence.

This package intentionally does not import the shell-certification package.
The two evidence formats have different schemas and lifecycle guarantees, but
shell tools take their strict JSON reader/writer, digests and error types from
here, so a capture one tool accepts is not reinterpreted by another.
"""

from __future__ import annotations

import hashlib
import json
import math
import os
import stat
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence


VALID = "VALID"
INVALID = "INVALID"
_JSON_LIMIT = 16 * 1024 * 1024
_HEX = frozenset("0123456789abcdef")
_REPARSE_ATTRIBUTE = getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400)


class ValidationError(ValueError):
    """An input cannot support an immutable tactical evidence claim."""


class OutputExistsError(FileExistsError):
    """An immutable output target already exists."""


@dataclass(frozen=True)
class FileSnapshot:
    """Stable identity and bytes for one absolute regular non-link file."""

    path: Path
    byte_length: int
    sha256: str
    device: int
    inode: int
    modified_ns: int
    mode: int
    raw: bytes

    def identity_tuple(self) -> tuple[int, int, int, int, int, str]:
        return (
            self.device,
            self.inode,
            self.byte_length,
            self.modified_ns,
            self.mode,
            self.sha256,
        )

    def public_identity(self) -> dict[str, Any]:
        return {
            "path": str(self.path),
            "byte_length": self.byte_length,
            "sha256": self.sha256,
        }


def utc_now() -> str:
    """Return an unambiguous report timestamp."""

    return datetime.now(timezone.utc).isoformat(timespec="microseconds").replace(
        "+00:00", "Z"
    )


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def absolute_path(path: str | os.PathLike[str]) -> Path:
    """Make a path absolute without resolving away its final link identity."""

    return Path(os.path.abspath(os.fspath(path)))


def _is_reparse(metadata: os.stat_result) -> bool:
    return bool(getattr(metadata, "st_file_attributes", 0) & _REPARSE_ATTRIBUTE)


def _lstat(path: Path, label: str) -> os.stat_result:
    try:
        return path.lstat()
    except OSError as exc:
        raise ValidationError(f"cannot inspect {label} {path}: {exc}") from exc


def reject_reparse_ancestors(
    path: str | os.PathLike[str],
    label: str,
    *,
    include_final: bool = True,
) -> Path:
    """Reject every existing symbolic-link or Windows reparse component."""

    supplied = Path(path)
    if not supplied.is_absolute():
        raise ValidationError(f"{label} must be absolute: {supplied}")
    target = absolute_path(supplied)
    components = list(reversed(target.parents))
    if include_final:
        components.append(target)
    for component in components:
        try:
            metadata = component.lstat()
        except FileNotFoundError:
            continue
        except OSError as exc:
            raise ValidationError(
                f"cannot inspect {label} ancestor {component}: {exc}"
            ) from exc
        if stat.S_ISLNK(metadata.st_mode) or _is_reparse(metadata):
            raise ValidationError(
                f"{label} crosses a link, junction, or reparse point: {component}"
            )
    return target


def require_regular_file(
    path: str | os.PathLike[str],
    label: str,
    *,
    maximum_length: int | None = None,
    exact_length: int | None = None,
) -> FileSnapshot:
    """Read and hash a stable absolute regular non-link file."""

    target = reject_reparse_ancestors(path, label)
    before_path = _lstat(target, label)
    if not stat.S_ISREG(before_path.st_mode):
        raise ValidationError(f"{label} is not a regular file: {target}")
    if maximum_length is not None and before_path.st_size > maximum_length:
        raise ValidationError(
            f"{label} is too large: {before_path.st_size} > {maximum_length}"
        )
    if exact_length is not None and before_path.st_size != exact_length:
        raise ValidationError(
            f"{label} byte length is {before_path.st_size}, expected {exact_length}"
        )

    digest = hashlib.sha256()
    pieces: list[bytes] = []
    try:
        with target.open("rb") as stream:
            before_handle = os.fstat(stream.fileno())
            while chunk := stream.read(1024 * 1024):
                digest.update(chunk)
                pieces.append(chunk)
            after_handle = os.fstat(stream.fileno())
        after_path = target.lstat()
    except OSError as exc:
        raise ValidationError(f"cannot read {label} {target}: {exc}") from exc

    def identity(value: os.stat_result) -> tuple[int, int, int, int]:
        return (
            value.st_dev,
            value.st_ino,
            value.st_size,
            value.st_mtime_ns,
        )

    identities = (
        identity(before_path),
        identity(before_handle),
        identity(after_handle),
        identity(after_path),
    )
    if len(set(identities)) != 1:
        raise ValidationError(f"{label} changed while being read: {target}")
    raw = b"".join(pieces)
    if len(raw) != before_path.st_size:
        raise ValidationError(
            f"{label} short read: got {len(raw)}, expected {before_path.st_size}"
        )
    if exact_length is not None and len(raw) != exact_length:
        raise ValidationError(
            f"{label} byte length is {len(raw)}, expected {exact_length}"
        )
    return FileSnapshot(
        path=target,
        byte_length=len(raw),
        sha256=digest.hexdigest(),
        device=before_path.st_dev,
        inode=before_path.st_ino,
        modified_ns=before_path.st_mtime_ns,
        mode=before_path.st_mode,
        raw=raw,
    )


def require_directory(path: str | os.PathLike[str], label: str) -> Path:
    target = reject_reparse_ancestors(path, label)
    metadata = _lstat(target, label)
    if not stat.S_ISDIR(metadata.st_mode):
        raise ValidationError(f"{label} is not a directory: {target}")
    return target


def require_new_directory_path(
    path: str | os.PathLike[str], label: str
) -> Path:
    supplied = Path(path)
    if not supplied.is_absolute():
        raise ValidationError(f"{label} must be absolute: {supplied}")
    target = absolute_path(supplied)
    reject_reparse_ancestors(target, label, include_final=False)
    if os.path.lexists(target):
        raise OutputExistsError(f"{label} already exists: {target}")
    require_directory(target.parent, f"{label} parent")
    return target


def create_directory_exclusive(path: Path, label: str) -> Path:
    target = require_new_directory_path(path, label)
    try:
        target.mkdir()
    except FileExistsError as exc:
        raise OutputExistsError(f"{label} already exists: {target}") from exc
    except OSError as exc:
        raise ValidationError(f"cannot create {label} {target}: {exc}") from exc
    return target


def _reject_json_constant(value: str) -> None:
    raise ValidationError(f"non-finite JSON number is not allowed: {value}")


def _finite_json_float(value: str) -> float:
    parsed = float(value)
    if not math.isfinite(parsed):
        raise ValidationError(f"non-finite JSON number is not allowed: {value}")
    return parsed


def _unique_object(pairs: Sequence[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValidationError(f"duplicate JSON object key: {key}")
        result[key] = value
    return result


def parse_json_bytes(raw: bytes, label: str, *, maximum_length: int = _JSON_LIMIT) -> Mapping[str, Any]:
    if type(maximum_length) is not int or maximum_length < 1:
        raise ValidationError("JSON maximum_length must be a positive integer")
    if len(raw) > maximum_length:
        raise ValidationError(f"{label} exceeds {maximum_length} bytes")
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise ValidationError(f"{label} is not UTF-8: {exc}") from exc
    try:
        parsed = json.loads(
            text,
            object_pairs_hook=_unique_object,
            parse_constant=_reject_json_constant,
            parse_float=_finite_json_float,
        )
    except ValidationError:
        raise
    except (json.JSONDecodeError, RecursionError) as exc:
        raise ValidationError(f"{label} is not strict JSON: {exc}") from exc
    if not isinstance(parsed, dict):
        raise ValidationError(f"{label} root must be an object")
    return parsed


def load_json_file(
    path: str | os.PathLike[str], label: str, *, maximum_length: int = _JSON_LIMIT
) -> tuple[FileSnapshot, Mapping[str, Any]]:
    snapshot = require_regular_file(path, label, maximum_length=maximum_length)
    return snapshot, parse_json_bytes(snapshot.raw, label, maximum_length=maximum_length)


def require_object(value: Any, field: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise ValidationError(f"{field} must be an object")
    return value


def require_array(value: Any, field: str) -> Sequence[Any]:
    if not isinstance(value, list):
        raise ValidationError(f"{field} must be an array")
    return value


def require_string(value: Any, field: str) -> str:
    if not isinstance(value, str):
        raise ValidationError(f"{field} must be a string")
    return value


def require_int(value: Any, field: str) -> int:
    if type(value) is not int:
        raise ValidationError(f"{field} must be an integer")
    return value


def require_number(value: Any, field: str) -> float:
    if type(value) not in (int, float):
        raise ValidationError(f"{field} must be a number")
    parsed = float(value)
    if not math.isfinite(parsed):
        raise ValidationError(f"{field} must be finite")
    return parsed


def require_bool(value: Any, field: str) -> bool:
    if type(value) is not bool:
        raise ValidationError(f"{field} must be a boolean")
    return value


def require_exact_keys(
    value: Mapping[str, Any], expected: Iterable[str], field: str
) -> None:
    expected_keys = set(expected)
    actual_keys = set(value)
    missing = sorted(expected_keys - actual_keys)
    extra = sorted(actual_keys - expected_keys)
    if missing or extra:
        details: list[str] = []
        if missing:
            details.append(f"missing={missing}")
        if extra:
            details.append(f"unexpected={extra}")
        raise ValidationError(f"{field} keys are invalid ({', '.join(details)})")


def require_value(value: Any, expected: Any, field: str) -> None:
    if type(value) is not type(expected) or value != expected:
        raise ValidationError(f"{field} is {value!r}, expected {expected!r}")


def require_sha256(value: Any, field: str) -> str:
    digest = require_string(value, field)
    if len(digest) != 64 or any(character not in _HEX for character in digest):
        raise ValidationError(f"{field} must be a lowercase SHA-256 digest")
    return digest


def assert_snapshot_unchanged(before: FileSnapshot, label: str) -> FileSnapshot:
    after = require_regular_file(before.path, label)
    if after.identity_tuple() != before.identity_tuple():
        raise ValidationError(f"{label} changed during the operation: {before.path}")
    return after


def write_bytes_exclusive(path: Path, data: bytes) -> Path:
    """Create, flush, and fsync one immutable regular file."""

    supplied = Path(path)
    if not supplied.is_absolute():
        raise ValidationError(f"output file must be absolute: {supplied}")
    target = absolute_path(supplied)
    reject_reparse_ancestors(target, "output file", include_final=False)
    try:
        with target.open("xb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
    except FileExistsError as exc:
        raise OutputExistsError(f"refusing to overwrite output: {target}") from exc
    except OSError as exc:
        raise ValidationError(f"cannot write immutable output {target}: {exc}") from exc
    return target


def canonical_json_bytes(value: Mapping[str, Any], *, compact: bool = False) -> bytes:
    return (
        json.dumps(
            value,
            indent=None if compact else 2,
            separators=(",", ":") if compact else None,
            sort_keys=True,
            ensure_ascii=False,
            allow_nan=False,
        )
        + "\n"
    ).encode("utf-8")


def write_json_exclusive(
    path: Path, value: Mapping[str, Any], *, maximum_length: int | None = None,
    compact: bool = False,
) -> Path:
    raw = canonical_json_bytes(value, compact=compact)
    if maximum_length is not None:
        if type(maximum_length) is not int or maximum_length < 1:
            raise ValidationError("JSON maximum_length must be a positive integer")
        if len(raw) > maximum_length:
            raise ValidationError(f"JSON output exceeds {maximum_length} bytes")
    return write_bytes_exclusive(path, raw)


def contains_forbidden_verdict(value: Any) -> bool:
    """Reports never classify native parity without a native comparator."""

    if isinstance(value, str):
        return value in {"MATCH", "DRIFT"}
    if isinstance(value, dict):
        return any(contains_forbidden_verdict(item) for item in value.values())
    if isinstance(value, list):
        return any(contains_forbidden_verdict(item) for item in value)
    return False
