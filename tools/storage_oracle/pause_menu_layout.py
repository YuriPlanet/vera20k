"""Original network pause templates BBA and FF, their help keys and FF's
ordinary-child placements.

`0x004F10E0` begins B5 for GameModes 0/5, FF for WOL while Session+0x300 is
clear and BBA otherwise. `ShellDialog__IsSlideTopControl` (`0x00608CD0`)
enrols BBA's 0x521/0x522 (`0x00609291`) and FF's (`0x0060954B`) in the
right-panel rows and both titles 0x694 (`0x00608E38`); FF's Resume 0x675 is
its bottom button (`0x00609A25`). `0x0060C3E6` sends FF's other children to
`0x0060B7A0`. This fixture pins both DLGTEMPLATEEX control sets, runs
`0x0060B7A0` for every FF ordinary child at three resolutions, and runs the
status-line help lookup `0x006040B0` for every control.
"""

from pathlib import Path

from tools.native_oracle import finish_vectors, provenance
from tools.storage_oracle.diplomacy_shell_layout import template_layout

# Buttons, title and status line take the dedicated placements.
DEDICATED = {
    0xBBA: (0x521, 0x522, 0x686, 0x694, 0x695),
    0xFF: (0x521, 0x522, 0x675, 0x694, 0x695),
}


def generate():
    dialogs = {}
    for dialog, dedicated in DEDICATED.items():
        controls, cases = template_layout(dialog, dedicated)
        for case, control in zip(cases, [c for c in controls
                                         if c["control_id"] not in dedicated
                                         for _ in range(3)]):
            case["dlu_rect"] = control["dlu_rect"]
        dialogs[f"{dialog:#x}"] = {"controls": controls, "cases": cases}
    return {"source": "unicorn/gamemd.exe", "dialogs": dialogs}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope="Original RT_DIALOG 0xBBA and 0xFF controls, their 0x006040B0 help labels and every FF ordinary child's complete 0x0060B7A0 placement",
        assumptions=[
            "Template bytes read from the mapped original resource directory; supplied 6x13 dialog base units, not emulated font metrics",
            "0x0060C3E6 sends FF's ordinary children to 0x0060B7A0; buttons, title and status line take dedicated placements, established by original instruction inspection",
            "Parent origin 0 and extents 640x480, 800x600, 1024x768",
            "Help: only the table fallback",
        ],
        substitutions=[
            "Shared native_child_case supplies GetParent/GetWindowRect/MoveWindow and true 0x0069BBE0 hooks",
            "Shared help_label supplies the record-table hash and GetDlgCtrlID",
        ],
        entry_points={"menu": 0x4F10E0, "dialog_proc": 0x4F11B0,
                      "dispatcher": 0x60C0C0, "ordinary_child": 0x60B7A0,
                      "help_lookup": 0x6040B0},
    ))
