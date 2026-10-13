"""Original BounceClass tumble: its quaternion kernels and a spinning flight.

python -m tools.spatial_oracle.bounce_tumble --check (or explicit --write)

Kernel rows call the original functions on supplied bits:
- `from_axis_angle`: Quaternion_FromAxisAngle 0x00646480 (ECX out, EDX axis,
  one float angle on the stack; Sqrt_Approx 0x004CAC40, SinFromTable 0x004CACB0,
  CosFromTable 0x004CAD00).
- `multiply`: 0x00645ED0 (ECX out, EDX a, stack b), the product the integrator
  uses, divided by the squared norm.
- `to_matrix`: Quaternion_ToMatrix 0x00646980 (ECX out, EDX q), 12 floats.

`flight` rows seed the Scenario RNG through 0x0065C6D0, call BounceClass::Init
0x004397E0 on a body (three RandomRanged draws, the axis and both quaternions),
then call BounceClass::Update 0x00439B00 once per tick until it returns 2 or 400
ticks pass, reading GetDrawingMatrix 0x004399E0 after every tick. Terrain is
anim_bouncer_flight.py's flat level-0 machine (original map queries).

Row schema: `flight[i].ticks[t]` = [Update result, orientation bits x4 (+0x30),
rotation bits x4 (+0x40), drawing matrix bits x12] after tick t+1;
`init_body_hex` is the 0x50-byte body after Init, `rng_after` the 0x3F4-byte
generator state after Init.

Rust consumer: src/sim/bounce.rs (tests).
"""
import math
import struct
from pathlib import Path

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP

from tools.native_oracle import SCRATCH, call, finish_vectors, provenance, run_checked
from tools.spatial_oracle.anim_bouncer_flight import (BODY, CENTER, MAX_TICKS, SP, STOP,
                                                      call as body_call, machine, words)

FROM_AXIS_ANGLE, MULTIPLY, TO_MATRIX = 0x646480, 0x645ED0, 0x646980
SEED, INIT, UPDATE, DRAWING_MATRIX = 0x65C6D0, 0x4397E0, 0x439B00, 0x4399E0
SCENARIO_PTR = 0xA8B230
SCENARIO = 0x22000000
ARGS = BODY + 0x200        # Init's coordinate and velocity
MATRIX_OUT = BODY + 0x300


def f32(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


def f64(value):
    return struct.unpack("<Q", struct.pack("<d", value))[0]


def from_axis_angle(axis, angle):
    out, src = SCRATCH, SCRATCH + 0x40
    result = call(FROM_AXIS_ANGLE, ecx=out, edx=src, stack_args=[angle],
                  writes={src: struct.pack("<3I", *axis)}, dumps={"q": (out, 16)})
    return list(struct.unpack("<4I", bytes.fromhex(result["dumps"]["q"])))


def multiply(a, b):
    out, left, right = SCRATCH, SCRATCH + 0x40, SCRATCH + 0x80
    result = call(MULTIPLY, ecx=out, edx=left, stack_args=[right],
                  writes={left: struct.pack("<4I", *a), right: struct.pack("<4I", *b)},
                  dumps={"q": (out, 16)})
    return list(struct.unpack("<4I", bytes.fromhex(result["dumps"]["q"])))


def to_matrix(q):
    out, src = SCRATCH, SCRATCH + 0x40
    result = call(TO_MATRIX, ecx=out, edx=src, writes={src: struct.pack("<4I", *q)},
                  dumps={"m": (out, 48)})
    return list(struct.unpack("<12I", bytes.fromhex(result["dumps"]["m"])))


def unit(x, y, z):
    length = math.sqrt(x * x + y * y + z * z)
    return [f32(x / length), f32(y / length), f32(z / length)]


NEGATIVE_ZERO = 0x80000000
AXES = [
    unit(1, 0, 0), unit(0, 1, 0), unit(0, 0, 1), unit(1, 1, 1), unit(-3, 2, -7),
    unit(0.25, -0.5, 0.125), [f32(0.6), f32(0.8), f32(0.0)],
    [f32(2.0), f32(-3.0), f32(6.0)],                     # unnormalized: length 7
    [f32(1e-3), f32(2e-3), f32(-2e-3)],                  # short
    [0, 0, 0], [NEGATIVE_ZERO, 0, NEGATIVE_ZERO],         # zero length keeps the raw axis
    [f32(-0.0001), NEGATIVE_ZERO, f32(0.9999)],
]
ANGLES = [0.0, 0.20943951, 0.41887903, -0.20943951, 1.0, 3.14159265, -2.5, 6.5, 40.0,
          1e-6, -1e-6]


def kernel_rows():
    angle_rows = [dict(axis_bits=axis, angle_bits=f32(angle),
                       out_bits=from_axis_angle(axis, f32(angle)))
                  for axis in AXES for angle in ANGLES]
    angle_rows.append(dict(axis_bits=AXES[3], angle_bits=NEGATIVE_ZERO,
                           out_bits=from_axis_angle(AXES[3], NEGATIVE_ZERO)))
    quaternions = [row["out_bits"] for row in angle_rows[::7]]
    quaternions += [[0, 0, 0, f32(1.0)], [0, 0, 0, 0], [NEGATIVE_ZERO] * 4,
                    [f32(0.5), f32(-0.5), f32(0.5), f32(-0.5)],
                    [f32(1e-3), f32(-2e-3), f32(3e-3), f32(0.999)],
                    [f32(2.0), f32(0.0), f32(-1.0), f32(0.25)]]
    multiply_rows = []
    for i, a in enumerate(quaternions):
        for b in quaternions[i % 3::3]:
            multiply_rows.append(dict(a_bits=a, b_bits=b, out_bits=multiply(a, b)))
    matrix_rows = [dict(q_bits=q, out_bits=to_matrix(q)) for q in quaternions]
    return angle_rows, multiply_rows, matrix_rows


def seed_rng(uc, seed):
    uc.mem_map(SCENARIO, 0x1000)
    uc.mem_write(SCENARIO_PTR, words(SCENARIO))
    uc.mem_write(SP, words(STOP, seed))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, SCENARIO + 0x218)
    run_checked(uc, SEED, STOP, count=100_000)


def fly(case):
    uc = machine("flat0")
    seed_rng(uc, case["seed"])
    coord = [CENTER * 256 + 128, CENTER * 256 + 128, case["z"]]
    uc.mem_write(BODY, bytes(0x50))
    uc.mem_write(ARGS, struct.pack("<3i", *coord))
    uc.mem_write(ARGS + 0x10, struct.pack("<3I", *case["velocity_bits"]))
    elasticity, gravity, angle = (f64(case["elasticity"]), f64(1.4), case["angle_bits"])
    body_call(uc, INIT, ARGS, elasticity & 0xFFFFFFFF, elasticity >> 32,
              gravity & 0xFFFFFFFF, gravity >> 32, 0, 0, ARGS + 0x10,
              angle & 0xFFFFFFFF, angle >> 32)
    init_body = bytes(uc.mem_read(BODY, 0x50))
    rng_after = bytes(uc.mem_read(SCENARIO + 0x218, 0x3F4)).hex()
    ticks = []
    for _ in range(MAX_TICKS):
        result = body_call(uc, UPDATE)
        body_call(uc, DRAWING_MATRIX, MATRIX_OUT)
        ticks.append([result, *struct.unpack("<8I", uc.mem_read(BODY + 0x30, 32)),
                      *struct.unpack("<12I", uc.mem_read(MATRIX_OUT, 48))])
        if result == 2:
            break
    return dict(input=dict(case, coord=coord), init_body_hex=init_body.hex(),
                rng_after=rng_after, ticks=ticks,
                final_body_hex=bytes(uc.mem_read(BODY, 0x50)).hex())


def flight_cases():
    # [TIRE]: Elasticity=0.8, MinZVel=28..MaxZVel=32, MaxXYVel=10, spin 12..24 degrees
    # (ReadINI stores radians). Launch Z = ground + 10 (VoxelAnimClass 0x0074950C).
    # The double angles are 12 and 24 degrees times the pi/180 at 0x007F65E8.
    pi_180 = struct.unpack("<d", struct.pack("<Q", 0x3F91DF46A2452B7C))[0]
    angles = [f64(12.0 * pi_180), f64(24.0 * pi_180), f64(0.0), f64(-1.0 * pi_180), f64(2.75)]
    velocities = [(3.0, -4.0, 30.0), (-10.0, 9.0, 28.0), (0.0, 0.0, 32.0), (7.0, 1.0, 29.0)]
    seeds = [1, 7, 31, 0x5CA1AB1E, 0xFFFFFFFF, 42]
    for index, seed in enumerate(seeds):
        for angle_index, angle in enumerate(angles):
            velocity = velocities[(index + angle_index) % len(velocities)]
            yield dict(seed=seed, z=10, elasticity=0.8, angle_bits=angle,
                       velocity_bits=[f32(v) for v in velocity])
    # Elasticity 0 stops on first contact; an odd launch height exercises the clamp.
    yield dict(seed=99, z=10, elasticity=0.0, angle_bits=angles[0],
               velocity_bits=[f32(2.0), f32(-1.0), f32(28.0)])
    yield dict(seed=1000, z=57, elasticity=0.8, angle_bits=angles[1],
               velocity_bits=[f32(-5.0), f32(5.0), f32(31.0)])


def generate():
    angle_rows, multiply_rows, matrix_rows = kernel_rows()
    return dict(from_axis_angle=angle_rows, multiply=multiply_rows, to_matrix=matrix_rows,
                flight=[fly(case) for case in flight_cases()])


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("Quaternion_FromAxisAngle 0x00646480, the product 0x00645ED0 and "
               "Quaternion_ToMatrix 0x00646980 on supplied bits (unit, unnormalized, short, "
               "zero and signed-zero axes; zero, small, large and negative angles; zero and "
               "non-unit quaternions); and 32 [TIRE]-like flights: BounceClass::Init "
               "0x004397E0 after a native Scenario seed, then Update 0x00439B00 and "
               "GetDrawingMatrix 0x004399E0 per tick on flat level-0 terrain until result 2 "
               "or 400 ticks."),
        assumptions=[
            "x87 control word 0x0E7F (PC53, chop); 0x00822D80 holds 0x0E7F for ftol 0x007C5F00.",
            "Flights use anim_bouncer_flight.py's machine('flat0') and carry its terrain "
            "assumptions (supplied cell table, original deck init and slope matrices).",
            "Init receives gravity 1.4 and a zero clamp, as VoxelAnimClass's constructor "
            "passes them (0x00749648..0x00749663); the angle double and velocity floats are "
            "supplied, not drawn by the constructor.",
            "The sine table and Sqrt_Approx table are the image's own data.",
        ],
        substitutions=[],
        entry_points={"from_axis_angle": FROM_AXIS_ANGLE, "multiply": MULTIPLY,
                      "to_matrix": TO_MATRIX, "seed": SEED, "init": INIT, "update": UPDATE,
                      "drawing_matrix": DRAWING_MATRIX}))
