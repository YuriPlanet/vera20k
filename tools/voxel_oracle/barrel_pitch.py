"""Execute original YR turret offset and barrel pitch instructions.

python -m tools.voxel_oracle.barrel_pitch --check (or explicit --write)

UnitClass::DrawVoxelBody's turret arm 73BA4C..73BC26 on a cache miss:
FILD Type+0x720, FMUL [B1D008], FSTP float into 5AE980 (translate along X);
RotateZ 5AF1A0 by the turret-minus-body facing angle; copy to the barrel
matrix, translate it by minus the turret translation (5AE8F0), RotateY
5AF080 by -(step * [7E4408]) of the barrel elevation (+0x370), translate
back; then camera (754BE0) times each (5AF980).
[B1D008] comes from the static initializers 735180 -> 7351B0 -> 7351D0.
This fixture executes those initializers, the offset block, the RotateY
block alone, and the whole arm. FacingClass::Current (4C93D0) runs with
finished timers, so each facing reads back its Desired dword. It does not
emulate the locomotor, the voxel caches, dispatch or rasterization.
"""

from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE, NATIVE_FPCW, call, load_image,
    run_checked, finish_vectors, provenance,
)
from tools.voxel_oracle.lighting import IDENTITY, native_camera, native_facing, native_multiply

# Windows starts a process with this control word; static initializers run
# before WinMain selects NATIVE_FPCW.
STARTUP_FPCW = 0x027F
TYPE = SCRATCH
TECHNO = SCRATCH + 0x1000
FACING_READ = SCRATCH + 0x2000
GARBAGE = 0xDEAD0000


def global_after(func, address, *, writes=None, fpcw):
    result = call(func, dumps={"value": (address, 8)}, writes=writes, fpcw=fpcw)
    return bytes.fromhex(result["dumps"]["value"])


def native_offset_scale(fpcw):
    diagonal = global_after(0x735180, 0xB1CFD0, fpcw=fpcw)
    cell = global_after(0x7351B0, 0xB1D0B0, writes={0xB1CFD0: diagonal}, fpcw=fpcw)
    return global_after(0x7351D0, 0xB1D008, writes={0xB1D0B0: cell}, fpcw=fpcw)


def draw_frame(scale, camera):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    uc.mem_write(0xB1D008, scale)
    uc.mem_write(0xB44318, camera)
    return uc, sp


def facing_class(raw):
    # Desired dword (high half is whatever follows the DirStruct), start
    # facing, timer start -1, duration 0, ROT word of the barrel's 3.
    return struct.pack("<IIiiih", GARBAGE | raw, raw, -1, 0, 0, 0x300) + bytes(2)


def native_offset(scale, camera, leptons):
    uc, sp = draw_frame(scale, camera)
    uc.mem_write(TYPE + 0x720, struct.pack("<i", leptons))
    uc.reg_write(UC_X86_REG_EBX, TYPE)
    uc.mem_write(sp + 0x94, IDENTITY)
    run_checked(uc, 0x73BA4C, 0x73BA68, required_addresses=[0x5AE980])
    return struct.unpack("<I", bytes(uc.mem_read(sp + 0xA0, 4)))[0]


def native_pitch(scale, camera, raw, matrix):
    uc, sp = draw_frame(scale, camera)
    uc.mem_write(FACING_READ, struct.pack("<I", GARBAGE | raw))
    uc.reg_write(UC_X86_REG_EAX, FACING_READ)
    uc.mem_write(sp + 0x104, matrix)
    run_checked(uc, 0x73BB83, 0x73BBB1, required_addresses=[0x5AF080])
    return bytes(uc.mem_read(sp + 0x104, 48))


def native_arm(scale, camera, leptons, body, turret, barrel, *, locomotor_matrix=None):
    uc, sp = draw_frame(scale, camera)
    uc.mem_write(TYPE + 0x720, struct.pack("<i", leptons))
    for offset, raw in ((0x370, barrel), (0x388, body << 11), (0x3A0, turret << 11)):
        uc.mem_write(TECHNO + offset, facing_class(raw))
    uc.reg_write(UC_X86_REG_EBX, TYPE)
    uc.reg_write(UC_X86_REG_EBP, TECHNO)
    # Default preserves the original flat Drive controls. Other callers may
    # transport an actual original locomotor-produced matrix (e.g. Ship69F670).
    matrix = native_facing(body) if locomotor_matrix is None else locomotor_matrix
    assert len(matrix) == 48
    uc.mem_write(sp + 0x94, matrix)
    run_checked(uc, 0x73BA4C, 0x73BC26,
                required_addresses=[0x5AE980, 0x4C93D0, 0x5AF1A0, 0x5AE8F0,
                                    0x5AF080, 0x754BE0, 0x5AF980])
    return bytes(uc.mem_read(sp + 0xD4, 48)), bytes(uc.mem_read(sp + 0x134, 48))


def bits(matrix):
    return list(struct.unpack("<12I", matrix))


def generate():
    scale = native_offset_scale(STARTUP_FPCW)
    if native_offset_scale(NATIVE_FPCW) != scale:
        raise ValueError("offset scale depends on the static initializers' control word")
    camera = native_camera()
    offsets = [-2**31, -100, -80, -1, 0, 1, 50, 70, 100, 255, 2**31 - 1]
    placed = native_facing(20)[:12] + struct.pack("<f", 3.5) + native_facing(20)[16:28] \
        + struct.pack("<f", -2.25) + native_facing(20)[32:44] + struct.pack("<f", 7.0)
    matrices = [IDENTITY, native_facing(5), native_multiply(camera, native_facing(13)), placed]
    raws = [0x4000, 0x3F00, 0x3C00, 0x3A00, 0x3800, 0x3000, 0x2000, 0x0000,
            0x4100, 0x4800, 0xC000, 0xBFFF, 0xFFFF]
    pitch_cases = [{"raw": raw, "matrix_bits": bits(matrix),
                    "result_bits": bits(native_pitch(scale, camera, raw, matrix))}
                   for matrix in matrices for raw in raws]
    arm_cases = []
    for body in (0, 5, 13, 24):
        body_draw = native_multiply(camera, native_facing(body))
        for delta in (0, 3, 16):
            turret = (body + delta) % 32
            for barrel in (0x4000, 0x3800, 0x2000, 0xC000, 0x4800):
                for leptons in (0, 50, -100):
                    turret_draw, barrel_draw = native_arm(scale, camera, leptons, body, turret, barrel)
                    arm_cases.append({
                        "body_step": body, "turret_step": turret, "barrel": barrel,
                        "turret_offset": leptons, "body_draw_bits": bits(body_draw),
                        "turret_draw_bits": bits(turret_draw),
                        "barrel_draw_bits": bits(barrel_draw)})
    return {"source": "unicorn/gamemd.exe",
            "offset_scale_bits": struct.unpack("<Q", scale)[0],
            "offset_cases": [{"turret_offset": leptons,
                              "units_bits": native_offset(scale, camera, leptons)}
                             for leptons in offsets],
            "pitch_cases": pitch_cases, "arm_cases": arm_cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
                       scope=("TurretOffset scale for 11 offsets; RotateY barrel block for 13 elevations "
                              "on 4 matrices; whole turret/barrel arm for 4 flat body steps x 3 turret "
                              "offsets x 5 elevations x 3 TurretOffsets"),
                       assumptions=[
                           "static initializers run under the process-start control word 0x027F; "
                           "generation fails unless 0x0E7F yields the same scale",
                           "x87 0x0E7F during the draw: WinMain 6BBFB7..6BBFC9",
                           "startup camera native blocks execute without substitutions (lighting.py)",
                           "locomotor matrix is the flat facing block 55A760 (Drive, no slope, no translation)",
                           "FacingClass timers finished (start -1, duration 0): Current returns Desired; "
                           "Desired high half is 0xDEAD to show the step ignores it",
                           "voxel caches, dispatch, rasterization, palette and GPU excluded",
                       ], substitutions=[], entry_points={
                           "offset_scale_init": 0x735180, "cell_width_init": 0x7351B0,
                           "units_per_lepton_init": 0x7351D0, "offset_block": 0x73BA4C,
                           "pitch_block": 0x73BB83, "turret_arm": 0x73BA4C,
                           "facing_current": 0x4C93D0, "rotate_y": 0x5AF080,
                           "rotate_z": 0x5AF1A0, "translate": 0x5AE8F0,
                           "camera_copy": 0x754BE0, "matrix_product": 0x5AF980}))
