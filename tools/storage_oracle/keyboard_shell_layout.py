"""Original A3 extended resource and full60B7A0 ordinary child placement.

The local DLGTEMPLATEEX decoder retains the original 32-bit child IDs. Supplied
6x13 font base units and the shared geometry hooks bound this comparison; title,
Back, footer, dropdown viewport and owner-paint art dimensions remain separate.
"""

from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from tools.native_oracle import finish_vectors, load_image, provenance
from tools.storage_oracle.in_game_shell_geometry import native_child_case


def resource():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    address, size = 0xBEF560, 1134
    data = bytes(uc.mem_read(address, size))
    version, signature, help_id, exstyle, style, count, *bounds = struct.unpack_from(
        "<HHIIIH4h", data)
    if (version, signature, help_id, exstyle, style, count, bounds) != (
            1, 0xFFFF, 0, 0, 0x40000040, 19, [0, 0, 533, 369]):
        raise RuntimeError("Unexpected original A3 extended dialog header")
    offset = 26

    def variable():
        nonlocal offset
        start = offset
        word = struct.unpack_from("<H", data, offset)[0]
        if word == 0xFFFF:
            offset += 4
            return struct.unpack_from("<H", data, start + 2)[0]
        while struct.unpack_from("<H", data, offset)[0]:
            offset += 2
        offset += 2
        return data[start:offset - 2].decode("utf-16-le")

    menu, window_class, title = variable(), variable(), variable()
    point_size, weight, italic, charset = struct.unpack_from("<HHBB", data, offset)
    offset += 6
    font = variable()
    if (point_size, weight, italic, charset, font) != (8, 0, 0, 1, "MS Sans Serif"):
        raise RuntimeError("Unexpected original A3 dialog font")
    controls = []
    for _ in range(count):
        offset = (offset + 3) & ~3
        child_help, child_exstyle, child_style, x, y, w, h, control = struct.unpack_from(
            "<III4hI", data, offset)
        offset += 24
        class_id, caption = variable(), variable()
        extra = struct.unpack_from("<H", data, offset)[0]
        offset += 2 + extra
        controls.append(dict(control_id=control, help_id=child_help,
                             style=child_style, exstyle=child_exstyle,
                             **{"class": class_id}, caption=caption,
                             dlu_rect=[x, y, w, h]))
    if offset > size or any(data[offset:]):
        raise RuntimeError("Unparsed original A3 extended resource bytes")
    return dict(dialog_id=0xA3, address=address, size=size,
                sha256=hashlib.sha256(data).hexdigest(), hex=data.hex(),
                template="DLGTEMPLATEEX", menu=menu, window_class=window_class,
                title=title, controls=controls)


def generate():
    template = resource()
    cases = []
    for index, child in enumerate(template["controls"]):
        if child["control_id"] in (0x686, 0x694, 0x695):
            continue
        x, y, w, h = child["dlu_rect"]
        pixels = [(x * 6 + 2) // 4, (y * 13 + 4) // 8,
                  (w * 6 + 2) // 4, (h * 13 + 4) // 8]
        for width, height in ((640, 480), (800, 600), (1024, 768)):
            case = native_child_case(child["control_id"], pixels, width, height)
            case.update(dialog_id=0xA3, resource_index=index)
            cases.append(case)
    if len(cases) != 48:
        raise RuntimeError("Missing original A3 ordinary placement cases")
    return dict(source="unicorn/gamemd.exe", resource=template, cases=cases)


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="Original A3 DLGTEMPLATEEX and48 complete ordinary child placements",
            assumptions=[
                "Supplied6x13 DLU base units; not font-selection emulation",
                "60C0C0 active A3 enrollment established from original instructions",
                "Special title/Back/footer excluded; type3 paint and combo collapsed height not emulated",
                "Original32-bit static child IDs retained; resource_index distinguishes anonymous captions",
            ],
            substitutions=["Shared native_child_case Win32 geometry hooks and true69BBE0"],
            entry_points={"modal": 0x5FBEF0, "callback": 0x5FB320,
                          "dispatcher": 0x60C0C0, "ordinary_child": 0x60B7A0}))
