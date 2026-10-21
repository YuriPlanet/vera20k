"""Positive Infantry DrawIt embedded shadows, using the existing prepared owner.

GI's stock paired frames are empty. Replace one input frame with an authored
stencil, then run original instructions; never ship or rewrite retail art.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP

from tools import native_oracle as native
from tools.input_oracle.campaign_start import CampaignReader
from tools.procedural_drawing_oracle import shroud
from tools.projectile_oracle.bridge_render_inputs_palette import PaletteReader
from tools.projectile_oracle.bridge_render_shape import A, PIXELS, Z, prepare_surface
from tools.rules_oracle import weapon_speed_order


def generate():
    """Borrow the GI producer's one VM and stop after two measured body draws.

    The temporary adapters are restored even on failure. This is a standalone
    single-threaded oracle entry point, not a mutation of a shared Ghidra image.
    """
    original_catalog = weapon_speed_order.RetailRulesInputs
    original_invoke = PaletteReader.invoke
    result = {"input": {}, "rows": []}
    mask = bytes((1, 0, 1, 0, 0, 1, 1, 0))
    selected_source = None

    class FixtureCatalog(original_catalog):
        def lookup(self, name):
            nonlocal selected_source
            raw = super().lookup(name)
            if name.upper() != "GI.SHP":
                return raw
            # Keep the real body, count and sequence. Only the authored shadow
            # input changes; neither the native SHP decoder nor code is patched.
            count = struct.unpack_from("<H", raw, 6)[0]
            shadow_frame = count // 2
            altered = bytearray(raw)
            header = 8 + shadow_frame * 24
            struct.pack_into("<4HB", altered, header, 38, 34, 4, 2, 1)
            struct.pack_into("<I", altered, header + 20, len(altered))
            altered.extend(mask)
            selected_source = bytes(altered)
            result["input"] = dict(
                original_frame_count=count, shadow_frame=shadow_frame,
                authored_rect=[38, 34, 4, 2], authored_indices=list(mask),
                surface=[64, 96], background=0x39E7, old_z=2000, a_value=127,
            )
            return selected_source

    class Finished(Exception):
        pass

    def measured_invoke(owner, address, *args, **kwargs):
        if address != 0x518F90:
            return original_invoke(owner, address, *args, **kwargs)
        actor, (point, clip) = args
        typ = owner.read32(actor + 0x6C0)
        assert owner.u.mem_read(typ + 0xD98, 1) == b"\0"
        assert selected_source is not None
        source = owner.read32(typ + 0xA4)
        original_point = bytes(owner.u.mem_read(point, 8))
        original_clip = bytes(owner.u.mem_read(clip, 16))
        original_context = owner.u.context_save()
        streams = dict(main=0x886B88, scenario=owner.read32(0xA8B230) + 0x218,
                       mapgen=0xABE890)
        rng_before = {name: bytes(owner.u.mem_read(ptr, 0x3F4))
                      for name, ptr in streams.items()}
        draws, rng_calls = [], []
        body = final = None

        def observe(u, pc, _size, _data):
            nonlocal body, final
            if pc == 0x4AED70:
                sp = u.reg_read(UC_X86_REG_ESP)
                values = struct.unpack("<14i", u.mem_read(sp + 4, 56))
                draws.append(dict(frame=values[1], flags=values[4],
                                  z_adjust=values[6], gradient=values[7],
                                  intensity=values[8], colour_word=values[9] & 65535))
            elif pc == 0x70645D:
                body = bytes(u.mem_read(PIXELS, 64 * 96 * 2))
            elif pc == 0x706602:
                final = bytes(u.mem_read(PIXELS, 64 * 96 * 2))
            elif pc in (0x65C780, 0x65C7E0):
                rng_calls.append(pc)

        hook = owner.u.hook_add(UC_HOOK_CODE, observe)
        try:
            for name, no_shadow in (("ordinary", False), ("no_shadow", True)):
                # Each control starts from the same CPU/FPU state, point,
                # caller clip and Surface/A/Z contents. Native invoke resets
                # thiscall ECX and stack; no prior draw's clipped rectangle or
                # incidental return register supplies the second input.
                owner.u.context_restore(original_context)
                owner.u.mem_write(point, original_point)
                owner.u.mem_write(clip, original_clip)
                owner.u.mem_write(typ + 0xD98, bytes((no_shadow,)))
                buffers = prepare_surface(owner.u, dict(
                    old_z=2000, a_value=127, background=0x39E7))
                body = final = None
                draws.clear()
                rng_calls.clear()
                original_invoke(owner, address, *args, **kwargs)
                assert body is not None and final is not None
                changed = [dict(index=i // 2, x=(i // 2) % 64, y=(i // 2) // 64,
                                body=struct.unpack_from("<H", body, i)[0],
                                final=struct.unpack_from("<H", final, i)[0])
                           for i in range(0, len(body), 2)
                           if body[i:i + 2] != final[i:i + 2]]
                assert bytes(owner.u.mem_read(source, len(selected_source))) == selected_source
                assert bytes(owner.u.mem_read(A, len(buffers["lighting"]))) == buffers["lighting"]
                assert bytes(owner.u.mem_read(Z, len(buffers["depths"]))) == buffers["depths"]
                assert not rng_calls
                assert all(bytes(owner.u.mem_read(ptr, 0x3F4)) == rng_before[key]
                           for key, ptr in streams.items())
                CampaignReader.check_code(owner)
                result["rows"].append(dict(
                    name=name, draws=list(draws), changed_after_body=changed,
                    original_code_unchanged=True, source_unchanged=True,
                    a_unchanged=True, z_unchanged=True, rng_unchanged=True,
                ))
            ordinary, suppressed = result["rows"]
            assert len(ordinary["draws"]) == 2 and ordinary["changed_after_body"]
            assert len(suppressed["draws"]) == 1 and not suppressed["changed_after_body"]
        finally:
            owner.u.hook_del(hook)
        raise Finished

    weapon_speed_order.RetailRulesInputs = FixtureCatalog
    PaletteReader.invoke = measured_invoke
    try:
        shroud.generate_infantry_berserk_body()
    except Finished:
        pass
    finally:
        weapon_speed_order.RetailRulesInputs = original_catalog
        PaletteReader.invoke = original_invoke
    assert len(result["rows"]) == 2
    return result


def metadata():
    return native.provenance(
        scope="Prepared ground Infantry DrawIt body followed by an authored nonempty embedded shadow, with a NoShadow negative control",
        assumptions=[
            "The existing shroud.generate_infantry_berserk_body owner prepares its original Infantry/Walk prefix, selected E1/GI type reader, palettes/Convert, flat Cells/TMP geometry, local owner, Ready frame0, native facing and RGB565 Surface. Its documented prepared-state limits remain; no scenario, COM activation, Unlimbo, display admission or whole campaign is executed.",
            "Original Infantry518F90 -> Foot41C090 -> Techno705E00 -> CC_Draw_Shape4AED70 execute. The physical GI frame count and body remain; frame count/2 receives an authored4x2 raw stencil. This certifies the selected instructions on a positive input, not stock artwork, MO assets, full Infantry stances, airborne/bridge paths or rendered GPU parity.",
            "Both controls restore the same CPU/FPU context, point and caller clip and reset the Surface/A/Z contents before original invocation. Native body and shadow returns supply observations. NoShadow is the only changed retained type field; constructor initially leaves it false. No saved body/frame/colour/clip output becomes a native expected input.",
            "The body result is observed at70645D and final result at706602. Changed pixel count is an observation after native clipping, not an assertion that every authored stencil pixel must store. All source bytes, A/Z planes, three complete RNG buffers and original executable code remain unchanged; RNG call hooks observe no requests.",
        ],
        substitutions=[
            "Reuse existing RetailRulesInputs, PaletteReader, shroud Infantry prefix, native TMP registry, Windows atomic/allocator/file boundaries and bridge_render_shape Surface storage. A temporary input catalog replaces only GI shadow frame metadata/payload; no VM, decoder, palette math, projection or raster implementation is copied.",
        ],
        entry_points=dict(infantry_body=0x518F90, foot_shape=0x41C090,
                          draw_shp=0x705E00, no_shadow_read=0x706052,
                          shape=0x4AED70, body_return=0x70645D,
                          shadow_return=0x706602),
    )


if __name__ == "__main__":
    native.finish_vectors(generate, Path(__file__).with_suffix(".json"),
                          provenance=metadata)
