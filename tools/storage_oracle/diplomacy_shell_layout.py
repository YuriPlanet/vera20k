"""Original Diplomacy RT_DIALOG 0x73, its ordinary-child placements and help keys.

`Show_Diplomacy_Menu` (`0x006586D0`) begins dialog 0x73 in the active-game
shell. `ResizeShellChildControl` (`0x0060C0C0`) sends title 0x694 to the
right-panel static (`0x00608D4B`), Return 0x423 to the lower button
(`0x006097AB`) and the status line 0x695 to `0x0060B550`; dialog 0x73 is in
the list at `0x0060C3E1` that sends every other child to `0x0060B7A0`. This
fixture pins the DLGTEMPLATEEX controls, runs `0x0060B7A0` for each ordinary
child at three resolutions, and runs the status-line help table lookup
`0x006040B0` for every control.
"""

from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32

from tools.native_oracle import finish_vectors, load_image, provenance
from tools.storage_oracle.in_game_shell_geometry import native_child_case
from tools.storage_oracle.shell_help_keys import help_label
from tools.storage_oracle.shell_relayout import Image, parse_template

DIALOG = 0x73
# Title, Return and status line take the dedicated placements, not 0x60B7A0.
DEDICATED = (0x694, 0x423, 0x695)


def template_layout(dialog, dedicated):
    """One RT_DIALOG's controls with their help labels, and the complete
    `0x0060B7A0` placement of every child outside `dedicated` at three
    resolutions. Callers establish which children are ordinary."""
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    controls = []
    cases = []
    for control in parse_template(Image(uc).dialog_template(dialog)):
        control_id = control["id"] & 0xFFFF
        x, y, w, h = control["template"]
        controls.append({
            "control_id": control_id, "class": control["class"],
            "style": control["style"], "dlu_rect": [x, y, w, h],
            "caption": control["caption"],
            "help": None if control_id == 0xFFFF else help_label(dialog, control_id),
        })
        if control_id in dedicated:
            continue
        pixels = [(x * 6 + 2) // 4, (y * 13 + 4) // 8,
                  (w * 6 + 2) // 4, (h * 13 + 4) // 8]
        for width, height in ((640, 480), (800, 600), (1024, 768)):
            case = native_child_case(control_id, pixels, width, height)
            case["dialog_id"] = dialog
            cases.append(case)
    return controls, cases


def generate():
    controls, cases = template_layout(DIALOG, DEDICATED)
    return {"source": "unicorn/gamemd.exe", "controls": controls, "cases": cases}

if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope="Original RT_DIALOG 0x73 controls, their 0x006040B0 help labels and every ordinary child's complete 0x0060B7A0 placement",
        assumptions=[
            "Template bytes read from the mapped original resource directory; supplied 6x13 dialog base units, not emulated font metrics",
            "0x0060C0C0 sends 0x73's ordinary children to 0x0060B7A0 (0x0060C3E1) and its title, Return and status line to dedicated placements, established by original instruction inspection",
            "Parent origin 0 and extents 640x480, 800x600, 1024x768",
            "Help: only the table fallback; the 0x73 proc handles no 0x4E9 and the children's own 0x4E8 answers are outside this fixture",
        ],
        substitutions=[
            "Shared native_child_case supplies GetParent/GetWindowRect/MoveWindow and true 0x0069BBE0 hooks",
            "Shared help_label supplies the record-table hash and GetDlgCtrlID",
        ],
        entry_points={"show": 0x6586D0, "dialog_proc": 0x657DA0,
                      "dispatcher": 0x60C0C0, "ordinary_child": 0x60B7A0,
                      "help_lookup": 0x6040B0},
    ))
