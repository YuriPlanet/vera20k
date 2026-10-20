"""Original connection dialog RT_DIALOG 0xEA, its ordinary-child placements
and help keys.

`Wait_For_Players` (`0x00648710`) creates 0xEA (`0x00648C40`, proc
`0x0064AE50`) and registers it through `UI__RegisterWindow` (`0x00622820`).
`ResizeShellChildControl` (`0x0060C0C0`) sends Quit Game 2 to the bottom
button (`0x0060B350`, matched at `0x00609979`) and the status line 0x695 to
`0x0060B550`; every other child goes to `0x0060B7A0` (`0x0060C3EE`). This
fixture pins the DLGTEMPLATEEX controls, runs `0x0060B7A0` for each ordinary
child at three resolutions, and runs the status-line help table lookup
`0x006040B0` for every control.
"""

from pathlib import Path

from tools.native_oracle import finish_vectors, provenance
from tools.storage_oracle.diplomacy_shell_layout import template_layout

DIALOG = 0xEA
# Quit Game and the status line take the dedicated placements.
DEDICATED = (0x2, 0x695)


def generate():
    controls, cases = template_layout(DIALOG, DEDICATED)
    return {"source": "unicorn/gamemd.exe", "controls": controls, "cases": cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope="Original RT_DIALOG 0xEA controls, their 0x006040B0 help labels and every ordinary child's complete 0x0060B7A0 placement",
        assumptions=[
            "Template bytes read from the mapped original resource directory; supplied 6x13 dialog base units, not emulated font metrics",
            "0x0060C0C0 sends 0xEA's ordinary children to 0x0060B7A0 (0x0060C3EE) and Quit Game 2 and the status line to dedicated placements, established by original instruction inspection",
            "Parent origin 0 and extents 640x480, 800x600, 1024x768",
            "Help: only the table fallback",
        ],
        substitutions=[
            "Shared native_child_case supplies GetParent/GetWindowRect/MoveWindow and true 0x0069BBE0 hooks",
            "Shared help_label supplies the record-table hash and GetDlgCtrlID",
        ],
        entry_points={"wait_for_players": 0x648710, "dialog_proc": 0x64AE50,
                      "dispatcher": 0x60C0C0, "ordinary_child": 0x60B7A0,
                      "help_lookup": 0x6040B0},
    ))
