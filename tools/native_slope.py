"""Checked retail startup slope matrices shared by native fixtures.

The pinned image supplies the original CRT/control-word and matrix initializer
bodies. A reset-style FPCW, scratch stack and return sentinel are supplied; this
does not execute complete Windows startup. No behavioral hooks or code patches
are installed. Each call completes before the next initializer uses its state.
"""

import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_FPCW

from tools.native_oracle import NATIVE_FPCW, NATIVE_SHA256, finish_vectors, load_image, provenance, run_checked


_BASE, _SP, _STOP = 0x20000000, 0x2000E000, 0x30000000


def initialize_slope_tables(uc, sp, stop):
    """Execute the shared original startup on an existing loaded native VM.

    The caller owns mapped stack/sentinel storage. Preserve its control word;
    original CRT startup still initializes its own cached rounding state.
    """
    previous_fpcw = uc.reg_read(UC_X86_REG_FPCW)
    uc.reg_write(UC_X86_REG_FPCW, 0x037F)

    def run(address, arguments=()):
        uc.mem_write(
            sp,
            struct.pack('<' + 'I' * (len(arguments) + 1), stop, *arguments),
        )
        uc.reg_write(UC_X86_REG_ESP, sp)
        run_checked(uc, address, stop, count=10_000_000,
                    required_addresses=(address,))

    run(0x007CEAAF)
    run(0x007CBF49, (0x300, 0x300))
    run(0x007C5EE4)
    for address in (
        0x754910, 0x754980, 0x7549A0, 0x7549C0, 0x7549E0,
        0x754A20, 0x754A50, 0x754CB0,
    ):
        run(address)
    uc.reg_write(UC_X86_REG_FPCW, previous_fpcw)


def _slope_machine(*, fpcw=None):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(_BASE, 0x10000)
    uc.mem_map(_STOP, 0x1000)
    if fpcw is not None:
        uc.reg_write(UC_X86_REG_FPCW, fpcw)
    initialize_slope_tables(uc, _SP, _STOP)
    return uc


def slope_matrices():
    """Return the 21 native matrices as fresh rows of twelve raw f32 words.

    Preserve one machine across the original initializer chronology, including
    the control-word cache written by 7C5EE4. In particular, 755852..755875 writes
    identity into rows 17..20; do not reconstruct those rows from other slopes.
    """
    uc = _slope_machine()
    return [
        list(struct.unpack('<12I', uc.mem_read(0xB45188 + 48 * slope, 48)))
        for slope in range(21)
    ]


def slope_blend_vectors():
    """Observe full755A40 against the actual startup matrices/quaternions."""
    uc = _slope_machine(fpcw=NATIVE_FPCW)
    fpcw_before = uc.reg_read(UC_X86_REG_FPCW)
    output = _BASE + 0x100
    cases = []
    for source, target, phase in ((1, 2, 0.0), (1, 2, 0.5), (1, 2, 1.0),
                                  (2, 1, 0.5), (0, 4, 0.25), (2, 2, 0.5), (5, 9, 0.5)):
        uc.mem_write(_SP, struct.pack('<IId', _STOP, target, phase))
        uc.reg_write(UC_X86_REG_ESP, _SP)
        uc.reg_write(UC_X86_REG_ECX, output)
        uc.reg_write(UC_X86_REG_EDX, source)
        run_checked(uc, 0x755A40, _STOP, count=100000,
            required_addresses=(0x755A40,) if source == target else (0x755A40, 0x646590, 0x646980))
        cases.append(dict(input=dict(from_slope=source, to_slope=target,
            phase_bits=struct.unpack('<Q', struct.pack('<d', phase))[0]),
            matrix_bits=list(struct.unpack('<12I', uc.mem_read(output, 48)))))
    return dict(native_sha256=NATIVE_SHA256, fpcw_before=fpcw_before,
        fpcw=uc.reg_read(UC_X86_REG_FPCW),
        cached_control_822d80=struct.unpack('<H', uc.mem_read(0x822D80, 2))[0],
        matrices=[list(struct.unpack('<12I', uc.mem_read(0xB45188 + 48 * slope, 48))) for slope in range(21)],
        quaternions=[list(struct.unpack('<4I', uc.mem_read(0xB43188 + 16 * slope, 16))) for slope in range(21)],
        cases=cases)


def blend_metadata():
    return provenance(scope='Original startup21 slope matrices/quaternions and seven full755A40 blend results',
        assumptions=[
            'Existing native_slope VM and original initializer chronology produce B45188 matrices and B43188 quaternions. Runtime FPCW0E7F is restored after startup; actual cached CRT controls are observed.',
            'Supplied valid slope indices and finite f64 phase values cover equal indices, endpoints, reversal, flat-to-ramp, the selected Ship1-to2 half transition and distinct-index5-to9 identical-quaternion aliases. Original755A40 stores phase as f32, calls full646590 and646980; no host quaternion, trig, normalization or interpolation computes expected outputs.',
            'Original755A40 accepts phase endpoints without caller clamping; equal indices copy the original matrix directly. No whole locomotor timer/cell history or rendering result is claimed.',
        ], substitutions=[], entry_points=dict(startup=0x754CB0, matrix=0x7559B0,
            blend=0x755A40, slerp=0x646590, quaternion_matrix=0x646980))


if __name__ == '__main__':
    import sys
    if '--blend' not in sys.argv[1:]:
        raise SystemExit('Select --blend for the additive slope-blend comparison')
    finish_vectors(slope_blend_vectors, Path(__file__).with_name('native_slope_blend.json'),
        provenance=blend_metadata, argv=[arg for arg in sys.argv[1:] if arg != '--blend'])
