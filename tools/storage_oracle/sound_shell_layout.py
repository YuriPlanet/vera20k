"""B8 raw resource and complete native60B7A0 ordinary child placement.

Button art selection/title/Back/footer exceptions are instruction evidence;
this fixture retains resource extents before type3 MNBTTN paint.
"""
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.storage_oracle.saved_game_layout import resource
from tools.storage_oracle.in_game_shell_geometry import native_child_case


def generate():
    template = resource(0xB8, 0xBEFE4C, 800, 14)
    cases = []
    for index, child in enumerate(template["controls"]):
        if child["control_id"] in (0x686, 0x694, 0x695):
            continue
        x, y, w, h = child["dlu_rect"]
        pixels = [(x*6+2)//4, (y*13+4)//8, (w*6+2)//4, (h*13+4)//8]
        for width, height in ((640,480), (800,600), (1024,768)):
            case = native_child_case(child["control_id"], pixels, width, height)
            case["dialog_id"] = 0xB8
            case["resource_index"] = index
            cases.append(case)
    return {"source":"unicorn/gamemd.exe", "resource":template, "cases":cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="Original B8 resource and33 complete ordinary child placements",
            assumptions=["Supplied6x13 DLU base units; not font-selection emulation",
                         "60C0C0 active B8 enrollment established from original instructions",
                         "Special title/Back/footer excluded; type3 button paint not emulated"],
            substitutions=["Shared native_child_case Win32 geometry hooks and true69BBE0"],
            entry_points={"modal":0x6B6230,"callback":0x6B6300,"ordinary_child":0x60B7A0}))
