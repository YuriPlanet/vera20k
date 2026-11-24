"""Strict JSON decoding, canonical encoding, hashing, and atomic output."""

from __future__ import annotations

import hashlib
import json
import math
import os
from pathlib import Path
import secrets


class MatrixError(ValueError):
    """Raised when a matrix, evidence document, or output contract is invalid."""


def _no_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise MatrixError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _reject_constant(value: str) -> object:
    raise MatrixError(f"non-finite JSON number: {value}")


def _validate_json_value(value: object, path: str = "$") -> None:
    if value is None or isinstance(value, (bool, int, str)):
        if isinstance(value, str) and any(0xD800 <= ord(char) <= 0xDFFF for char in value):
            raise MatrixError(f"unpaired surrogate at {path}")
        return
    if isinstance(value, float):
        if not math.isfinite(value):
            raise MatrixError(f"non-finite number at {path}")
        return
    if isinstance(value, list):
        for index, item in enumerate(value):
            _validate_json_value(item, f"{path}[{index}]")
        return
    if isinstance(value, dict):
        for key, item in value.items():
            if not isinstance(key, str):
                raise MatrixError(f"non-string key at {path}")
            _validate_json_value(item, f"{path}.{key}")
        return
    raise MatrixError(f"unsupported JSON value {type(value).__name__} at {path}")


def load_json_strict(data: str | bytes) -> object:
    """Load exactly one strict UTF-8 JSON value."""

    if isinstance(data, bytes):
        try:
            data = data.decode("utf-8", errors="strict")
        except UnicodeDecodeError as exc:
            raise MatrixError(f"invalid UTF-8: {exc}") from exc
    try:
        value = json.loads(
            data,
            object_pairs_hook=_no_duplicate_keys,
            parse_constant=_reject_constant,
        )
    except MatrixError:
        raise
    except (json.JSONDecodeError, RecursionError) as exc:
        raise MatrixError(f"invalid JSON: {exc}") from exc
    _validate_json_value(value)
    return value


def load_json_path(path: Path) -> object:
    try:
        return load_json_strict(path.read_bytes())
    except OSError as exc:
        raise MatrixError(f"cannot read {path}: {exc}") from exc


def canonical_json_bytes(value: object) -> bytes:
    """Encode the deterministic persisted representation."""

    _validate_json_value(value)
    try:
        payload = json.dumps(
            value,
            ensure_ascii=False,
            allow_nan=False,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    except (UnicodeEncodeError, TypeError, ValueError) as exc:
        raise MatrixError(f"cannot encode canonical JSON: {exc}") from exc
    return payload + b"\n"


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def atomic_write(path: Path, payload: bytes) -> None:
    """Durably replace one output through a unique sibling temporary file."""

    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    descriptor: int | None = None
    try:
        for _ in range(100):
            candidate = path.with_name(f".{path.name}.{secrets.token_hex(8)}.tmp")
            try:
                flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_BINARY", 0)
                descriptor = os.open(candidate, flags, 0o600)
                temporary = candidate
                break
            except FileExistsError:
                continue
        if descriptor is None or temporary is None:
            raise OSError("could not allocate an output temporary file")
        view = memoryview(payload)
        while view:
            written = os.write(descriptor, view)
            if written <= 0:
                raise OSError("short write")
            view = view[written:]
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = None
        os.replace(temporary, path)
        temporary = None
    except OSError as exc:
        raise MatrixError(f"cannot write {path}: {exc}") from exc
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if temporary is not None:
            try:
                temporary.unlink()
            except FileNotFoundError:
                pass
