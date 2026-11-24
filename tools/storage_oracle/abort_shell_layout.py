"""Original B6 resource and complete 60B7A0 placement of its ordinary question.

Active 60C0C0 selects 60B7A0 for B6 at60C3D1..60C3F8 after the registered
button/footer exceptions. This fixture does not emulate Win32 focus or painting.
"""

from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.storage_oracle.saved_game_layout import resource
from tools.storage_oracle.in_game_shell_geometry import native_child_case


def generate():
    template = resource(0xB6, 0xBEFBC0, 312, 5)
    question = next(c for c in template["controls"] if c["control_id"] == 0xFFFF)
    x, y, w, h = question["dlu_rect"]
    pixels = [(x * 6 + 2) // 4, (y * 13 + 4) // 8,
              (w * 6 + 2) // 4, (h * 13 + 4) // 8]
    cases = []
    for width, height in ((640, 480), (800, 600), (1024, 768)):
        case = native_child_case(0xFFFF, pixels, width, height)
        case["dialog_id"] = 0xB6
        cases.append(case)
    return {"source": "unicorn/gamemd.exe", "resource": template, "cases": cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope="Original B6 resource and three complete 60B7A0 question placements",
        assumptions=[
            "Pinned B6 resource bytes; supplied 6x13 dialog base units, not emulated font metrics",
            "60C0C0 active B6 dispatcher and button/footer exclusions established by original instruction inspection",
            "Parent origin0 and extents640x480,800x600,1024x768; original base800x600",
            "Button/footer geometry, painting, keyboard focus and exit transactions are outside this fixture",
        ],
        substitutions=[
            "Shared native_child_case supplies GetParent/GetWindowRect/MoveWindow and true69BBE0 hooks",
        ],
        entry_points={"modal": 0x4F1840, "callback": 0x4F18B0,
                      "dispatcher": 0x60C0C0, "ordinary_child": 0x60B7A0},
    ))
