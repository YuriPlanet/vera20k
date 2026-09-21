#!/usr/bin/env python3
"""Print an optional module dependency report, or save it with --output PATH.

Requires cargo-modules 0.26.0 and committed source/build inputs.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import os
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[1]
TARGET = "x86_64-pc-windows-msvc"
COMMON = ["--lib", "-p", "vera20k", "--target", TARGET]
NODE = re.compile(r'^    "([^"\\]+)" \[.*// "([^"\\]+)" node$', re.MULTILINE)
EDGE = re.compile(r'^    "([^"\\]+)" -> "([^"\\]+)" .*// "uses" edge$', re.MULTILINE)


def capture(args: list[str]) -> str:
    return subprocess.check_output(
        args, cwd=ROOT, encoding="utf-8", env={**os.environ, "NO_COLOR": "1"}
    ).replace("\r\n", "\n").strip("\n") + "\n"


def dependencies(dot: str) -> dict[str, set[str]]:
    """Keep uses edges, map item paths to their nearest module, and deduplicate."""
    if not dot.startswith("digraph {") or not dot.rstrip().endswith("}"):
        raise ValueError("Incomplete cargo-modules DOT output")
    nodes = dict(NODE.findall(dot))
    modules = {name for name, kind in nodes.items() if kind in {"mod", "crate"}}
    if "vera20k" not in modules:
        raise ValueError("Expected the vera20k crate in cargo-modules output")
    if any(not (name == "vera20k" or name.startswith("vera20k::")) for name in nodes):
        raise ValueError("Generate the DOT with --no-externs --no-sysroot")

    def containing_module(item: str) -> str:
        if item not in nodes:
            raise ValueError(f"Dependency endpoint has no node: {item}")
        while item not in modules:
            item, separator, _ = item.rpartition("::")
            if not separator:
                raise ValueError("Dependency endpoint has no containing module")
        return item.removeprefix("vera20k::")

    edges = EDGE.findall(dot)
    if len(edges) != dot.count('// "uses" edge'):
        raise ValueError("Unrecognized cargo-modules dependency syntax")
    result = {module.removeprefix("vera20k::"): set() for module in modules}
    for source, target in edges:
        source, target = containing_module(source), containing_module(target)
        if source != target:
            result[source].add(target)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="Save the report to this path instead of stdout")
    args = parser.parse_args()

    # A source commit must describe the inputs actually analyzed. Documentation
    # and tooling edits are allowed; source/build-input edits must be committed.
    inputs = ["src", "Cargo.toml", "Cargo.lock", ".cargo", "rust-toolchain", "rust-toolchain.toml"]
    if capture(["git", "status", "--porcelain", "--untracked-files=all", "--", *inputs]).strip():
        raise SystemExit("Commit source/build-input changes before generating the report")
    revision = capture(["git", "rev-parse", "HEAD"]).strip()
    version = capture(["cargo", "modules", "--version"]).strip()
    if not re.search(r"\b0\.26\.0\b", version):
        raise SystemExit("This formatter targets cargo-modules 0.26.0; review its output before upgrading")

    dot = capture(["cargo", "modules", "dependencies", *COMMON, "--no-externs", "--no-sysroot"])
    graph = dependencies(dot)
    if capture(["git", "rev-parse", "HEAD"]).strip() != revision or capture(
        ["git", "status", "--porcelain", "--untracked-files=all", "--", *inputs]
    ).strip():
        raise SystemExit("Source/build inputs changed during generation; report was not generated")

    report = (
        "# Module dependencies\n\n"
        + provenance(revision, graph)
        + "\n\n`A -> B; C` lists direct module dependencies, not runtime calls.\n"
        "Verify affected callers and ownership against source.\n\n"
        + dependency_text(graph)
        + "\n"
    )
    if args.output is None:
        print(report, end="")
    else:
        args.output.write_text(report, encoding="utf-8", newline="\n")
        print(f"Wrote {args.output}: {len(graph)-1} modules, {sum(map(len, graph.values()))} dependency edges")


def dependency_text(graph: dict[str, set[str]]) -> str:
    rows = [f"{name} -> {'; '.join(sorted(graph[name])) or '-'}" for name in sorted(graph)]
    return "```text\n" + "\n".join(rows) + "\n```"


def provenance(revision: str, graph: dict[str, set[str]]) -> str:
    day = datetime.now(timezone.utc).date().isoformat()
    return (
        f"Generated snapshot: `{revision}` ({day}), cargo-modules 0.26.0.\n\n"
        f"Scope: `vera20k` library, default features, `{TARGET}`, no depth limit.\n"
        "Test-only, binary-specific and inactive conditional modules are excluded.\n"
        "External crates and the sysroot are excluded from the dependency graph.\n"
        f"Contains **{len(graph)-1} modules plus the crate root**, and **{sum(map(len, graph.values()))} distinct\n"
        "cross-module dependency edges**. Check this source commit against your checkout."
    )


if __name__ == "__main__":
    main()
