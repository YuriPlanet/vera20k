"""Mirror canonical project skills into Claude's discovery directory.

Run --check for a read-only drift check, or --write after editing .agents/skills.
Codex UI metadata, Python caches, and the machine-local ghidra-up skill are excluded.
Unexpected destination files are reported, never deleted. Review removals explicitly.
"""

from __future__ import annotations

import argparse
from pathlib import Path


def excluded(relative: Path) -> bool:
    return (
        relative.parts[0] == "ghidra-up"
        or "__pycache__" in relative.parts
        or (len(relative.parts) > 1 and relative.parts[1] == "agents")
        or relative.suffix in {".pyc", ".pyo"}
    )


def redirected(path: Path) -> bool:
    return path.is_symlink() or path.is_junction()


def inventory(root: Path) -> dict[Path, bytes]:
    """Reject redirected paths rather than following them outside the skill tree."""
    if any(redirected(part) for part in (root, *root.parents)):
        raise ValueError(f"Refusing redirected skill root: {root}")
    if not root.exists():
        return {}
    files = {}
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root)
        if excluded(relative):
            continue
        if any(redirected(part) for part in (path, *path.parents) if part != root):
            raise ValueError(f"Refusing redirected skill path: {path}")
        if path.is_file():
            files[relative] = path.read_bytes()
    return files


def synchronize(source: Path, destination: Path, *, write: bool) -> tuple[int, list[str]]:
    """Return number of copied files and unresolved differences; never prune."""
    if not source.is_dir():
        raise ValueError(f"Canonical skill directory missing: {source}")
    expected = inventory(source)
    existing = inventory(destination)
    if not any(path.name == "SKILL.md" for path in expected):
        raise ValueError(f"No canonical skills found: {source}")
    issues = [f"Destination-only file (review manually): {path.as_posix()}"
              for path in sorted(existing.keys() - expected.keys())]
    # Refuse all writes while an unexpected destination file needs a decision.
    if issues:
        return 0, issues
    changed = 0
    for relative, content in expected.items():
        if existing.get(relative) == content:
            continue
        if write:
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
            changed += 1
        else:
            kind = "Missing" if relative not in existing else "Different"
            issues.append(f"{kind}: {relative.as_posix()}")
    return changed, issues


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    try:
        count, issues = synchronize(root / ".agents/skills", root / ".claude/skills",
                                    write=args.write)
    except (OSError, ValueError) as error:
        print(f"Skill sync failed: {error}")
        return 1
    if issues:
        print("\n".join(issues))
        return 1
    print(f"Skill mirrors match; {count} files updated.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
