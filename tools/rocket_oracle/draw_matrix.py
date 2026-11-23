"""Original RocketLocomotionClass::Draw_Matrix (0x00663470) and the draw it feeds.

AircraftClass::Draw_It draws a missile's body through its locomotor's
Draw_Matrix (ILocomotion slot +0x24, called at 0x00414969) with the draw key
(the HVA frame & 0x1F, 0 for the one-frame missile HVAs), then takes the
camera copy 0x00754BE0 times that matrix (MatrixMultiply 0x005AF980,
0x0041499F..0x004149AA). This oracle executes the same three calls.

Flight samples: every flight row of flight.py runs again through the same
harness (constructor, Move_To, one Process a frame through Detonate). After
the constructor and after each Process, Draw_Matrix runs on the live
locomotor and owner; a row keeps the first frame of each distinct (facing
step, CurrentPitch) and its matrix and key. The sampled flights must
reproduce flight.json frame for frame, so sampling cannot have disturbed
them. The sweep rows record the camera product as well.

Sweep samples: an owner snapped to a facing, with CurrentPitch written
directly. A V3 owner at every five-bit step and ten pitches (zero, -0, both
signs, the stored PitchFinal products, vertical, past vertical), and at both
sides of each step's rounding boundary; CMisl and DMisl owners at three
facings and the ten pitches, with incoming keys 0 and 7. The stored
PitchInitial products the key also compares against occur in the flights.

Native and executed: Draw_Matrix with Matrix3x4_SetIdentity 0x005AE860,
RotateZ 0x005AF1A0, Matrix_rotate_y_axis 0x005AF080, the sine and cosine
tables, FacingClass::Current 0x004C93D0 and the Rules type compare; the camera
copy and MatrixMultiply; the startup camera from tools/voxel_oracle/lighting.py.
"""
from pathlib import Path
import json
import struct

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.native_oracle import require
from tools.rocket_oracle.flight import (
    ARGUMENT, BASE, INTERFACE, LOCO, OWNER, ROWS, SP, WORK, Rocket, blocks, dwords, f32_bits,
)
from tools.voxel_oracle.lighting import native_camera

CAMERA = 0xB44318
DRAW_MATRIX = WORK + 0xC000
DRAW_KEY = WORK + 0xC100
CAMERA_COPY = WORK + 0xC200
DRAW = WORK + 0xC300
HALF_PI_F32 = 0x3FC90FDB


def step(raw):
    return (((raw >> 10) + 1) >> 1) & 0x1F


def words(matrix):
    """A row-major 3x4 matrix as twelve hex binary32 words."""
    return ' '.join(f'{word:08x}' for word in struct.unpack('<12I', bytes(matrix)))


class Drawn(Rocket):
    """A flight whose every observed state also records its draw."""

    def __init__(self, row, camera):
        self.samples = []
        self.seen = set()
        super().__init__(row)
        self.uc.mem_write(CAMERA, camera)

    def draw(self, key_in):
        u = self.uc
        u.mem_write(DRAW_KEY, dwords(key_in))
        require(self.call(0x663470, 0, [INTERFACE, DRAW_MATRIX, DRAW_KEY]) == DRAW_MATRIX, 'native 0x663470 returned an unexpected result')
        self.call(0x754BE0, CAMERA_COPY, [])
        u.mem_write(SP, dwords(RET_MAGIC, DRAW_MATRIX))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, DRAW)
        u.reg_write(UC_X86_REG_EDX, CAMERA_COPY)
        run_checked(u, 0x5AF980, RET_MAGIC, count=100_000, required_addresses=[0x5AF980])
        return dict(
            key_in=key_in,
            key_out=struct.unpack('<i', u.mem_read(DRAW_KEY, 4))[0],
            matrix=words(u.mem_read(DRAW_MATRIX, 48)),
            draw=words(u.mem_read(DRAW, 48)),
        )

    def state(self):
        observed = super().state()
        identity = (step(observed['facing']), observed['current_pitch'])
        if identity not in self.seen:
            self.seen.add(identity)
            drawn = self.draw(0)
            del drawn['draw']
            self.samples.append(dict(frame=self.frame, facing=observed['facing'],
                                     current_pitch=observed['current_pitch'], **drawn))
        return observed

    def sweep(self, facing, pitch_bits, key_in):
        self.uc.mem_write(ARGUMENT, dwords(facing))
        self.call(0x4C9300, OWNER + 0x388, [ARGUMENT])
        self.uc.mem_write(LOCO + 0x54, dwords(pitch_bits))
        return dict(facing=facing, current_pitch=pitch_bits, **self.draw(key_in))


STEPS = [s << 11 for s in range(32)]
# Both sides of each step's rounding boundary: +0x3FF stays, +0x400 rounds up.
BOUNDARIES = [(raw + offset) & 0xFFFF for raw in STEPS for offset in (0x3FF, 0x400)]
# Zero and -0 skip RotateY; 0x3F490FDA and 0x3FC90FDA are the stored PitchFinal
# products of 0.5 and 1.0 the flights reach (key bit 0x40).
PITCHES = [0x00000000, 0x80000000, f32_bits(1e-6), f32_bits(0.33), 0x3F490FDA, 0x3FC90FDA,
           HALF_PI_F32, f32_bits(-0.7853982), HALF_PI_F32 | 0x80000000, f32_bits(3.0)]
KEY_FACINGS = (0x0000, 0x4000, 0xA400)
SWEEP_ROWS = [
    ('v3_owner_every_step', dict(blocks=blocks('V3')),
     [(raw, pitch, 0) for raw in STEPS for pitch in PITCHES]),
    ('v3_owner_step_rounding', dict(blocks=blocks('V3')),
     [(raw, f32_bits(0.7853982), 0) for raw in BOUNDARIES]),
    ('cmisl_owner_key_bits', dict(blocks=blocks('CMisl')),
     [(raw, pitch, key) for raw in KEY_FACINGS for pitch in PITCHES for key in (0, 7)]),
    ('dmisl_owner_key_bits', dict(blocks=blocks('DMisl')),
     [(raw, pitch, key) for raw in KEY_FACINGS for pitch in PITCHES for key in (0, 7)]),
]


def generate():
    camera = native_camera()
    flights = {row['name']: row['output'] for row in json.loads(
        Path(__file__).with_name('flight.json').read_text(encoding='utf-8'))}
    rows = []
    for name, changes in ROWS:
        rocket = Drawn(dict(BASE, **changes), camera)
        output = rocket.run()
        if output != flights[name]:
            raise AssertionError(f'{name}: sampling changed the flight')
        rows.append(dict(name=name, kind='flight', samples=rocket.samples))
    for name, changes, cases in SWEEP_ROWS:
        rocket = Drawn(dict(BASE, **changes), camera)
        rows.append(dict(name=name, kind='sweep', owner_blocks=changes['blocks'],
                         samples=[rocket.sweep(*case) for case in cases]))
    return dict(source='unicorn/gamemd.exe', camera=words(camera), rows=rows)


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='RocketLocomotionClass::Draw_Matrix and the camera product AircraftClass::Draw_It applies to '
              'it: the identity, RotateZ by the owner facing step, RotateY by -CurrentPitch when it is not '
              'zero, and the draw key (0x20/0x40 pitch compares, -1, the facing step), sampled along the '
              '16 flight.py flights, and a sweep of every facing step at ten pitches, each step\'s rounding '
              'boundary, and V3, CMisl and DMisl owners with incoming keys 0 and 7. Not the voxel caches, '
              'Draw_Point, Shadow_Matrix or rasterization.',
        entry_points={'draw_matrix': 0x663470, 'identity': 0x5AE860, 'rotate_z': 0x5AF1A0,
                      'rotate_y': 0x5AF080, 'facing_current': 0x4C93D0, 'camera_copy': 0x754BE0,
                      'matrix_product': 0x5AF980, 'aircraft_draw_call': 0x414969},
        assumptions=['The flight.py harness and its assumptions (FPCW 0E7F after the rocket module '
                     'initializers; RulesClass blocks per row; the owner facing at +0x388 built by its '
                     'constructor, Set_ROT and Snap); the startup camera at 0x00B44318 from '
                     'tools/voxel_oracle/lighting.py native_camera; the draw key starts at the HVA frame & '
                     '0x1F, 0 for the one-frame V3ROCKET/DMISL/BSUBMISL HVAs, and 7 in the key-bit rows.'],
        substitutions=['The flight seams of flight.py; sweep rows write CurrentPitch (+0x54) directly and snap '
                       'the facing with 0x004C9300.'],
    ))
