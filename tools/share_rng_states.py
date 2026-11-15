"""Store each corpus's repeated RNG states once: `python -m tools.share_rng_states FILE...`.

Rewrites each JSON file in place, compact, through
`native_oracle.shared_rng_states` (tools/README.md, Capture files).
"""
from __future__ import annotations

import json
from pathlib import Path
import sys

from tools.native_oracle import shared_rng_states


def main(argv: list[str] | None = None) -> None:
    for name in sys.argv[1:] if argv is None else argv:
        path = Path(name)
        before = path.stat().st_size
        shared = shared_rng_states(json.loads(path.read_text(encoding="utf-8")))
        path.write_text(json.dumps(shared, separators=(",", ":"), allow_nan=False) + "\n",
                        encoding="utf-8")
        print(f"{path}: {len(shared['shared_rng_states'])} distinct RNG states, "
              f"{before:,} to {path.stat().st_size:,} bytes")


if __name__ == "__main__":
    main()
