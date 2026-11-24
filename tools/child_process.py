"""Bounded exact-child lifecycle shared by repository capture tools.

Wrappers own input validation, evidence directories, artifact publication and
result schemas. This module owns only the process and temporary diagnostic files.
It never uses a shell, process-group kill or descendant traversal.
"""
from __future__ import annotations

from contextlib import ExitStack
from dataclasses import dataclass
import math
import os
from pathlib import Path
import subprocess
import tempfile
from typing import BinaryIO, Sequence


POST_KILL_WAIT_SECONDS = 5.0


@dataclass(frozen=True)
class ChildResult:
    pid: int | None
    exit_status: int | None
    timed_out: bool
    stdout: bytes
    stderr: bytes
    errors: tuple[str, ...]


def _snapshot(stream: BinaryIO) -> bytes:
    """Read only the length observed after exit/wait, never drain inherited pipes.

    A descendant may retain the regular file or continue appending. A finite
    length prevents that writer from extending our read indefinitely. On hosts
    with pread, collecting diagnostics also leaves its shared file offset alone.
    """
    stream.flush()
    os.fsync(stream.fileno())
    length = os.fstat(stream.fileno()).st_size
    if hasattr(os, 'pread'):
        data = bytearray()
        while len(data) < length:
            chunk = os.pread(stream.fileno(), min(length - len(data), 1024 * 1024), len(data))
            if not chunk:
                raise OSError('child output shrank during collection')
            data.extend(chunk)
        return bytes(data)
    stream.seek(0)
    data = stream.read(length)
    if len(data) != length:
        raise OSError('child output shrank during collection')
    return data


def _stop_child(child: subprocess.Popen[bytes], errors: list[str]) -> None:
    try:
        if child.poll() is None:
            child.kill()
    except OSError as exc:
        errors.append(f'failed to kill exact child PID {child.pid}: {exc}')
    try:
        child.wait(timeout=POST_KILL_WAIT_SECONDS)
    except subprocess.TimeoutExpired:
        errors.append(f'exact child PID {child.pid} did not exit within '
                      f'{POST_KILL_WAIT_SECONDS:g}s after kill')
    except OSError as exc:
        errors.append(f'failed to wait for exact child PID {child.pid} after kill: {exc}')


def run_child(command: Sequence[str], *, cwd: Path, temporary_directory: Path,
              timeout_seconds: float, label: str = 'capture child') -> ChildResult:
    """Run one child; return diagnostics and lifecycle errors without publishing.

    ``temporary_directory`` must already exist. Its choice belongs to the
    wrapper: some children must create their own capture directory. Nonzero exit
    statuses are returned unchanged for the wrapper to interpret. Timeout cleanup
    targets only the exact still-live Popen child and waits at most five seconds.
    """
    if isinstance(command, (str, bytes)) or not command:
        raise ValueError('command must be a nonempty argument sequence')
    if (isinstance(timeout_seconds, bool) or not isinstance(timeout_seconds, (int, float))
            or not math.isfinite(timeout_seconds) or timeout_seconds <= 0):
        raise ValueError('timeout must be a finite positive number of seconds')
    timeout = float(timeout_seconds)
    errors: list[str] = []
    child = None
    timed_out = False
    stdout, stderr = b'', b''
    try:
        with ExitStack() as stack:
            streams = [stack.enter_context(tempfile.TemporaryFile(
                mode='w+b', dir=temporary_directory, prefix=f'.child-{name}-'))
                for name in ('stdout', 'stderr')]
            try:
                child = subprocess.Popen(list(command), stdin=subprocess.DEVNULL,
                                         stdout=streams[0], stderr=streams[1],
                                         shell=False, cwd=cwd)
            except OSError as exc:
                errors.append(f'failed to start {label}: {exc}')
            if child is not None:
                try:
                    child.wait(timeout=timeout)
                except subprocess.TimeoutExpired:
                    timed_out = True
                    errors.append(f'{label} PID {child.pid} exceeded {timeout:g}s timeout')
                    _stop_child(child, errors)
                except OSError as exc:
                    errors.append(f'failed to wait for {label} PID {child.pid}: {exc}')
                    _stop_child(child, errors)
            outputs = []
            for name, stream in zip(('stdout', 'stderr'), streams):
                try:
                    outputs.append(_snapshot(stream))
                except OSError as exc:
                    outputs.append(b'')
                    errors.append(f'failed to drain child {name}: {exc}')
            stdout, stderr = outputs
    except OSError as exc:
        errors.append(f'failed to prepare or close child diagnostic files: {exc}')
    return ChildResult(pid=child.pid if child is not None else None,
                       exit_status=child.returncode if child is not None else None,
                       timed_out=timed_out, stdout=stdout, stderr=stderr,
                       errors=tuple(errors))
