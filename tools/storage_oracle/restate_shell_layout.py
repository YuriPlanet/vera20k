"""Original mission restate RT_DIALOG 0x10C, its ordinary-child placements and
help keys.

`ScenarioClass::ShowMissionRestateBriefing` (`0x0065F520`) begins dialog
0x10C (`0x0065F633`, proc `0x0065F710`) in the active-game shell.
`ResizeShellChildControl` (`0x0060C0C0`) sends title 0x694 to the right-panel
static, Return 0x423 to the lower button (`0x006097A0` groups 0x10C with
0x73) and the status line 0x695 to `0x0060B550`; 0x10C is in the list at
`0x0060C3D9` that sends every other child, the briefing text 0x6E7, to
`0x0060B7A0`. This fixture pins the DLGTEMPLATEEX controls, runs
`0x0060B7A0` for the ordinary child at three resolutions, and runs the
status-line help table lookup `0x006040B0` for every control.
"""

from pathlib import Path

from tools.native_oracle import finish_vectors, provenance
from tools.storage_oracle.diplomacy_shell_layout import template_layout

DIALOG = 0x10C
# Title, Return and status line take the dedicated placements.
DEDICATED = (0x694, 0x423, 0x695)


def generate():
    controls, cases = template_layout(DIALOG, DEDICATED)
    return {"source": "unicorn/gamemd.exe", "controls": controls, "cases": cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope="Original RT_DIALOG 0x10C controls, their 0x006040B0 help labels and the ordinary child's complete 0x0060B7A0 placement",
        assumptions=[
            "Template bytes read from the mapped original resource directory; supplied 6x13 dialog base units, not emulated font metrics",
            "0x0060C0C0 sends 0x10C's ordinary child to 0x0060B7A0 (0x0060C3D9) and its title, Return and status line to dedicated placements, established by original instruction inspection",
            "Parent origin 0 and extents 640x480, 800x600, 1024x768",
            "Help: only the table fallback; the 0x10C proc handles no 0x4E9",
        ],
        substitutions=[
            "Shared native_child_case supplies GetParent/GetWindowRect/MoveWindow and true 0x0069BBE0 hooks",
            "Shared help_label supplies the record-table hash and GetDlgCtrlID",
        ],
        entry_points={"show": 0x65F520, "dialog_proc": 0x65F710,
                      "dispatcher": 0x60C0C0, "ordinary_child": 0x60B7A0,
                      "help_lookup": 0x6040B0},
    ))
