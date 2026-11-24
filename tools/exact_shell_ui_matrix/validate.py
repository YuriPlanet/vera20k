"""Validate one generated exact-shell UI matrix artifact."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

from .io import MatrixError, load_json_path
from .validation import validate_matrix


REPO_ROOT = Path(__file__).resolve().parents[2]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("matrix", type=Path)
    parser.add_argument(
        "--artifact-root",
        type=Path,
        default=REPO_ROOT,
        help=(
            "existing root used to resolve and hash evidence artifact paths "
            "(default: repository root)"
        ),
    )
    args = parser.parse_args(argv)
    try:
        document = load_json_path(args.matrix)
        validate_matrix(document, artifact_root=args.artifact_root)
        print(
            f"valid {args.matrix} rows={document['coverage']['total']} "
            f"certification={document['certification_state']}"
        )
        return 0
    except MatrixError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
