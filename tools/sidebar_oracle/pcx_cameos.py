"""Bounded Ares 3.0 PCX cameo readers, selectors and native packed-word blit.

Only synthetic names, fields, palette colors and pixels are fixture inputs.
Set VERA20K_ARES_DLL and VERA20K_GAMEMD_EXE to the enrolled binaries, then run
python -m tools.sidebar_oracle.pcx_cameos --check. No game assets are copied.
"""

import argparse
import hashlib
import os
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP,
)

from tools.native_oracle import (
    IMAGE_BASE, IMAGE_SIZE, OracleError, finish_vectors, map_pe32_image,
    provenance, run_checked,
)


ARES_SHA256 = "93ce890f387a6c1b3240a2dd7b674f25fec5a0e96f0b0d0306796b2edf7b5b83"
SCRATCH, STACK, RETURN = 0x20000000, 0x20010000, 0x20020000
DRAWN_SURFACE = 0x100C20CC
FIXTURE = Path(__file__).with_suffix(".json")


class Probe:
    def __init__(self, images):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        ares, gamemd = images
        map_pe32_image(self.u, ares, image_base=0x10000000,
                       image_size=0xE0000, header_size=0x400)
        map_pe32_image(self.u, gamemd, image_base=IMAGE_BASE,
                       image_size=IMAGE_SIZE, header_size=0x1000)
        self.u.mem_map(SCRATCH, 0x30000)

    def put(self, at, *values):
        self.u.mem_write(at, struct.pack("<" + "I" * len(values),
                                        *(value & 0xFFFFFFFF for value in values)))

    def read(self, at):
        return struct.unpack("<I", self.u.mem_read(at, 4))[0]

    def string(self, at):
        result = bytearray()
        for index in range(0x800):
            byte = self.u.mem_read(at + index, 1)[0]
            if byte == 0:
                return result.decode("ascii")
            result.append(byte)
        raise OracleError("Synthetic string is not terminated")

    def service_return(self, cleanup, value):
        sp = self.u.reg_read(UC_X86_REG_ESP)
        self.u.reg_write(UC_X86_REG_EIP, self.read(sp))
        self.u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
        self.u.reg_write(UC_X86_REG_EAX, value)

    def call(self, entry, args=(), ecx=0):
        self.put(STACK, RETURN, *args)
        self.u.reg_write(UC_X86_REG_ESP, STACK)
        self.u.reg_write(UC_X86_REG_ECX, ecx)
        run_checked(self.u, entry, RETURN, count=200_000,
                    context={"entry": hex(entry)})
        return self.u.reg_read(UC_X86_REG_EAX)


def reader_case(images, name, value, cached_surface=0, load_succeeds=False):
    p = Probe(images)
    obj, ini, section, key, locale = (SCRATCH + offset for offset in
                                     (0, 0x1000, 0x2000, 0x3000, 0x8000))
    p.u.mem_write(obj, b"prior.pcx\0")
    p.put(obj + 32, 0x11111111)
    p.u.mem_write(section, b"SYNTHETIC\0")
    p.u.mem_write(key, b"CameoPCX\0")
    p.put(ini, SCRATCH + 0x4000)
    # Configure the CRT's existing C-locale branch; its copy/lowercase/security
    # cookie instructions execute. No locale/CRT function is replaced.
    p.put(0x100C0360, locale)
    p.put(0x100C0364, 0)
    p.put(0x100C28BC, 0)
    p.put(0x100C2244, 0)
    p.put(locale + 0xA8, 0)
    calls = []
    loaded = False

    def service(uc, address, _size, _data):
        nonlocal loaded
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == 0x528A10:
            args = [p.read(sp + 4 + index * 4) for index in range(5)]
            calls.append(dict(service="ReadString", section=p.string(args[0]),
                              key=p.string(args[1]), default=p.string(args[2]),
                              capacity=args[4]))
            raw = (value or "").encode("ascii")
            if len(raw) >= args[4]:
                raise OracleError("ReadString fixture exceeds supplied capacity")
            uc.mem_write(args[3], raw + b"\0")
            p.service_return(20, len(raw))
        elif address == 0x1002E110:
            calls.append(dict(service="GetSurface", filename=p.string(p.read(sp + 4))))
            p.service_return(8, cached_surface or (SCRATCH + 0x7000 if loaded else 0))
        elif address == 0x1002E430:
            calls.append(dict(service="LoadFile", filename=p.string(p.read(sp + 4)),
                              mode=p.read(sp + 8)))
            loaded = load_succeeds
            p.service_return(12, int(loaded))
        elif address == 0x10061960:
            calls.append(dict(service="INIParseFailed", filename=p.string(p.read(sp + 12))))
            p.service_return(16, 0)

    p.u.hook_add(UC_HOOK_CODE, service)
    returned = p.call(0x100762C0, (ini, section, key, 0x10093FFE), obj)
    return dict(name=name, value=value, cached_surface=cached_surface,
                load_succeeds=load_succeeds, filename=p.string(obj),
                surface=p.read(obj + 32), returned_al=returned & 255,
                service_calls=calls)


def selector_case(images, elite, normal, promoted):
    p = Probe(images)
    shim, techno, extension = SCRATCH, SCRATCH + 0x1000, SCRATCH + 0x2000
    p.put(shim + 0x18, techno)
    p.put(techno + 0x2FC, extension)
    p.put(extension + 0x228, normal)
    p.put(extension + 0x24C, promoted)
    p.put(0xA83D4C, SCRATCH + 0x9000)

    def service(_uc, address, _size, _data):
        if address == 0x1003D800:
            p.service_return(4, int(elite))

    p.u.hook_add(UC_HOOK_CODE, service)
    p.call(0x1005E430, (shim,))
    selected = p.read(DRAWN_SURFACE)
    bypass = p.call(0x1005E3E0)
    return dict(elite=elite, normal_surface=normal, promoted_surface=promoted,
                selected_surface=selected, skip_shp_jump=bypass)


def superweapon_case(images, surface):
    p = Probe(images)
    shim, extension = SCRATCH, SCRATCH + 0x2000
    p.put(shim + 0x24, SCRATCH + 0x1000)
    p.put(extension + 0x4D0, surface)

    def service(_uc, address, _size, _data):
        if address == 0x10057C70:
            p.service_return(4, extension)

    p.u.hook_add(UC_HOOK_CODE, service)
    p.call(0x1005E410, (shim,))
    return dict(surface=surface, selected_surface=p.read(DRAWN_SURFACE))


def blit_case(images, name, source_size, destination_size, rectangle, words):
    p = Probe(images)
    destination, source, vtable, stubs, dst_pixels, src_pixels, bounds = (
        SCRATCH + offset for offset in (0x1000, 0x2000, 0x3000, 0x4000,
                                       0x5000, 0x6000, 0x7000))
    p.put(destination, vtable)
    p.put(source, vtable)
    for slot in (0x5C, 0x60, 0x74, 0x7C, 0x80):
        p.put(vtable + slot, stubs + slot)
    count = destination_size[0] * destination_size[1]
    before = [0x1111] * count
    p.u.mem_write(dst_pixels, struct.pack("<" + "H" * count, *before))
    p.u.mem_write(src_pixels, struct.pack("<" + "H" * len(words), *words))
    p.put(bounds, *rectangle)
    calls = []

    def service(uc, address, _size, _data):
        if not stubs <= address < stubs + 0x100:
            return
        slot = address - stubs
        instance = uc.reg_read(UC_X86_REG_ECX)
        if instance not in (destination, source):
            raise OracleError("Unexpected synthetic BSurface receiver")
        values = {0x5C: dst_pixels if instance == destination else src_pixels,
                  0x60: 1, 0x74: destination_size[0] * 2,
                  0x7C: source_size[0], 0x80: source_size[1]}
        if slot not in values:
            raise OracleError("Unexpected BSurface vtable slot")
        calls.append(dict(surface="destination" if instance == destination else "source",
                          vtable_slot=hex(slot)))
        p.service_return(8 if slot == 0x5C else 0, values[slot])

    p.u.hook_add(UC_HOOK_CODE, service)
    returned = p.call(0x6BA580, (bounds, destination, source, 0xF81F), 0xAC4848)
    after = list(struct.unpack("<" + "H" * count, p.u.mem_read(dst_pixels, count * 2)))
    return dict(name=name, source_size=source_size, destination_size=destination_size,
                rectangle=rectangle, source_words=words, color_key=0xF81F,
                before_words=before, after_words=after, returned_al=returned & 255,
                service_calls=calls)


def palette_case(images):
    p = Probe(images)
    colors = [[255, 0, 255], [0, 0, 0], [248, 3, 248], [255, 4, 255],
              [17, 41, 83], [255, 255, 255]]
    raw = bytes(value for color in colors + [[0, 0, 0]] * (256 - len(colors))
                for value in color)
    p.u.mem_write(STACK + 0x3B8, raw)
    for address, value in ((0x8A0DD0, 11), (0x8A0DD4, 3), (0x8A0DD8, 0),
                           (0x8A0DDC, 3), (0x8A0DE0, 5), (0x8A0DE4, 2)):
        p.put(address, value)
    p.u.reg_write(UC_X86_REG_ESP, STACK)
    run_checked(p.u, 0x6B9DAC, 0x6B9E15, count=20_000)
    words = list(struct.unpack("<256H", p.u.mem_read(STACK + 0xB4, 512)))
    return dict(rgb=colors, words=words[:len(colors)],
                unused_black_words=sorted(set(words[len(colors):])))


def generate(images):
    return dict(
        schema_version=1,
        readers=[reader_case(images, *args) for args in (
            ("missing", None), ("empty", ""),
            ("mixed_case", "MiXeD.PCX", SCRATCH + 0x7000),
            ("none_is_filename", "none"), ("angled_none_is_filename", "<none>"),
            ("truncate_31_bytes", "A" * 35 + ".PCX", SCRATCH + 0x7000),
            ("no_extension_added", "ICON", SCRATCH + 0x7000),
            ("load_success", "Loaded.PCX", 0, True),
            ("load_fails", "Missing.PCX"))],
        selectors=[selector_case(images, elite, normal, promoted)
                   for elite in (False, True)
                   for normal, promoted in ((0, 0), (SCRATCH + 0x7000, 0),
                                            (0, SCRATCH + 0x8000),
                                            (SCRATCH + 0x7000, SCRATCH + 0x8000))],
        superweapons=[superweapon_case(images, surface)
                      for surface in (0, SCRATCH + 0x7000)],
        blits=[blit_case(images, "key_black_and_neighbor", [3, 1], [3, 1],
                         [0, 0, 3, 1], [0xF81F, 0, 0xF81E]),
               blit_case(images, "fully_keyed_still_succeeds", [3, 1], [3, 1],
                         [0, 0, 3, 1], [0xF81F] * 3),
               blit_case(images, "source_extent_and_destination_pitch", [2, 2], [8, 4],
                         [2, 1, 60, 48], [0xF81F, 0x1234, 0, 0xF81E])],
        palette=palette_case(images))


def metadata(images):
    result = provenance(
        scope="23 bounded PCX cameo controls: nine filename-reader, eight normal/promoted selector, two SW selector, three packed-word blit and one six-color palette conversion.",
        assumptions=[
            "Synthetic TechnoType/SW extensions and Syringe register shells; the elite predicate result and SW extension lookup are supplied, not computed by a native House lifecycle.",
            "Original Ares PCXFile100762C0, shared INI_EX10006010, bounded-copy CRT100039A0/1007B2E0 and C-locale lowercase CRT1007F1D7 execute. The CRT locale globals select the ordinary ASCII C locale; non-ASCII locale behavior is excluded.",
            "Original palette loop6B9DAC..6B9E15 uses supplied RGB565 losses3/2/3 and shifts11/5/0, six synthetic colors and250black entries. No file decoder runs.",
            "Whole original gamemd6BA580 blit runs against supplied BSurface interfaces and sufficient destination allocation. The source-extent control proves this routine uses source dimensions, not60x48 rectangle clipping.",
            "No physical INI/MIX loading, PCX RLE decoding, DLL startup, whole-sidebar/device drawing, House elite qualification, non-indexed PCX behavior or full-mod parity is established. No RNG draw, simulation timer or detach call occurs in these bounded bodies.",
        ],
        substitutions=[
            "ReadString528A10 supplies an already parsed ASCII payload or empty/absent result. Its callers, default argument, key, capacity and subsequent original reader instructions execute.",
            "PCX GetSurface1002E110 and LoadFile1002E430 supply explicit cache/file-success results; INIParseFailed10061960 records the failed filename without logging externally.",
            "CameoIsElite1003D800 supplies the declared Boolean; SW ExtMap lookup10057C70 returns the supplied type extension.",
            "BSurface Lock/Unlock/Pitch/Width/Height virtual services supply synthetic memory and geometry. Their original caller and every packed-word comparison/store execute.",
        ],
        entry_points=dict(reader=0x100762C0, techno_selector=0x1005E430,
                          sw_selector=0x1005E410, shp_bypass=0x1005E3E0,
                          palette_loop=0x6B9DAC, pcx_blit=0x6BA580))
    result["binary_identities"] = {
        "Ares.dll": dict(bytes=len(images[0]), sha256=ARES_SHA256)
    }
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__, add_help=False)
    parser.add_argument("--ares-dll", type=Path, default=os.environ.get("VERA20K_ARES_DLL"))
    parser.add_argument("--gamemd-exe", type=Path, default=os.environ.get("VERA20K_GAMEMD_EXE"))
    args, remaining = parser.parse_known_args()
    if args.ares_dll is None or args.gamemd_exe is None:
        raise OracleError("Set VERA20K_ARES_DLL and VERA20K_GAMEMD_EXE, or pass both binary arguments")
    images = args.ares_dll.read_bytes(), args.gamemd_exe.read_bytes()
    if hashlib.sha256(images[0]).hexdigest() != ARES_SHA256:
        raise OracleError("This address fixture requires the recorded Ares3.0 DLL identity")
    finish_vectors(lambda: generate(images), FIXTURE,
                   provenance=lambda: metadata(images), argv=remaining,
                   source_paths={"producer": Path(__file__),
                                 "native_oracle": Path(__file__).parents[1] / "native_oracle.py"})


if __name__ == "__main__":
    main()
