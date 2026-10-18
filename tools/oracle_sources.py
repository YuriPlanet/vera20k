"""Repository-source identity during a native run, and the command that ran it.

Sidecars pin no hashes of repository sources (AGENTS.md): such pins go stale on
edits that leave native output unchanged. `native_oracle.finish_vectors` still
hashes the sources a producer declares before and after generating, and refuses
to compare or publish a run whose sources changed while it ran. It records the
command that ran the producer (`replay_command`), which is not evidence.
Sidecars written before the pins were removed, on other branches, may still carry
`source_normalized_lf_sha256`; checks ignore it and `--write` drops it.
"""

from __future__ import annotations

import hashlib
import os
from pathlib import Path
import re
import shlex
import sys

REPO = Path(__file__).resolve().parents[1]
REPLAY_FIELD = 'replay_command'
LEGACY_SOURCE_FIELD = 'source_normalized_lf_sha256'
MODE_FLAGS = frozenset(('--check', '--write'))
# finish_vectors' staging target. A replay regenerates the corpus where it lives.
OUTPUT_FLAG = '--output'


def source_sha256(path: Path) -> str:
    """The source's text as a run reads it, compared before and after the run."""
    return hashlib.sha256(Path(path).read_text(encoding='utf-8').encode('utf-8')).hexdigest()


def source_key(label: str, path: Path) -> str:
    """Key sources by repository path, so a change during a run names the file."""
    resolved = Path(path).resolve()
    return resolved.relative_to(REPO).as_posix() if resolved.is_relative_to(REPO) else label


def source_identity(source_paths) -> dict:
    return {source_key(label, path): source_sha256(path)
            for label, path in (source_paths or {}).items()}


def evidence(metadata: dict) -> dict:
    """The metadata a replay must reproduce exactly: all but the replay command
    and a legacy source pin."""
    return {key: value for key, value in metadata.items()
            if key not in (REPLAY_FIELD, LEGACY_SOURCE_FIELD)}


def _repo_relative(argument: str) -> str:
    """An argument with checkout paths relative to the root a replay runs from.

    A path starts the argument or follows `=`, `:` or `,`."""
    for prefix in {f'{REPO}{os.sep}', f'{REPO.as_posix()}/'}:
        argument = re.sub(f'(?<![^=:,]){re.escape(prefix)}', '', argument)
    return argument


def _quote_replay_argument(argument: str) -> str:
    """Quote literal text while expanding recorded home paths at replay time."""
    home = Path.home()
    if home.parent == home:
        return shlex.quote(argument)
    prefixes = '|'.join(re.escape(prefix) for prefix in {str(home), home.as_posix()})
    parts = re.split(rf'(?<![^=:,])(?:{prefixes})(?=$|[/\\:,])', argument)
    if len(parts) == 1:
        return shlex.quote(argument)
    # Quote HOME independently: neither spaces in it nor shell syntax in the
    # literal suffix may split arguments or execute a substitution. A quoted
    # '~' would not expand, including in --option=path and path-list arguments.
    return '"$HOME"'.join(shlex.quote(part) if part else '' for part in parts)


def replay_command(argv=None, main=None) -> str | None:
    """The `python -m tools...` invocation running now, without its mode or output.

    Checkout paths are repository-relative; external home paths use quoted HOME.
    All other argument text remains literal."""
    main = sys.modules.get('__main__') if main is None else main
    spec = getattr(main, '__spec__', None)
    module = spec.name.removesuffix('.__main__') if spec is not None and spec.name else None
    if module is None and getattr(main, '__file__', None):
        path = Path(main.__file__).resolve()
        if path.suffix == '.py' and path.is_relative_to(REPO):
            module = '.'.join(path.relative_to(REPO).with_suffix('').parts)
    if not module or not module.startswith('tools.'):
        return None
    arguments, output = [], False
    for value in sys.argv[1:] if argv is None else argv:
        if output:
            output = False
        elif value == OUTPUT_FLAG:
            output = True
        elif value not in MODE_FLAGS and not value.startswith(OUTPUT_FLAG + '='):
            arguments.append(_repo_relative(value))
    return ' '.join(_quote_replay_argument(value) for value in ['python', '-m', module, *arguments])
