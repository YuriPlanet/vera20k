"""Native distances of the Sqrt_Approx callers outside the movement prefix.

Run python -m tools.spatial_oracle.sqrt_approx_distances --check (or explicit
--write). Rust consumers (tests): src/sim/movement/teleport_movement.rs,
src/sim/movement/movement_step_tests.rs, src/sim/radiation.rs,
src/sim/cell_kernel.rs, src/sim/particles/spawn.rs and
src/sim/find_nearby_cell.rs.
The Drive/Ship braking distance has its own rows in track_speed_native.

Each case runs original instructions in a fresh emulator, Sqrt_Approx
(0x4CAC40) and Math::ftol (0x7C5F00) included, from just after the caller
read its coordinates to its result. Registers and scratch memory stand in
for those reads:
- teleport: TeleportLocomotionClass::Process 0x719454..0x7194AC, the chrono
  distance from the owner's GetCoords at EAX to the destination at EBP.
- walk: WalkLocomotionClass::ProcessMovement 0x75BD33..0x75BD70, the distance
  from the owner's GetCoords at EAX to the step head in ESI/EDI, which
  completes the head below 17.
- radiation: RadSiteClass::GetRadLevelAt 0x65B93E..0x65B98F, the centre
  cell's GetCoords in ESI/EDI/EBX against a cell's at EAX. SetCellRadLevels,
  DecreaseCellRadLevels and DecayCellRadLevels repeat this block.
- infantry: CellClass::PlaceInfantryInCell 0x481180..0x481212, the preferred
  spot (EDI) for the coordinate in its second argument.
- particle: ParticleClass's constructor, target less source stored as
  binary32 (0x62B6C6..0x62B707), then that vector normalized in place
  (0x62B8D7..0x62B956) and a Normalized= type's state-AI advance from it
  (0x62B956..0x62BA51). The constructor code between the first two blocks
  (type fields, RNG draws, Set_Raw_Coords) writes neither the vector nor
  the registers the later blocks read: the source pointer (EDI), the vector
  (EBP) and zero (EBX). A scratch type record holds Normalized= (+0x30F)
  and FinalDamageState= (+0x30C); the particle holds its type (+0xAC) and
  speed (+0xE4), and the target pointer is the constructor's argument at
  [ESP+0x34].
- nearest: MapClass::Find_Nearby_Passable_Cell 0x56E6F0..0x56E780, the pick
  of the candidate nearest a target cell (EBP). The function's frame holds
  the direct count at [ESP+0x24] (also ECX; the indirect count is in ESI),
  the direct candidates from [ESP+0x12C] and the indirect ones from
  [ESP+0x18C], each a packed cell; the pick lands at [ESP+0x2C].
"""
from pathlib import Path
import struct

from unicorn import UC_ARCH_X86, UC_MODE_32, Uc
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
                               UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW)

from tools.native_oracle import (NATIVE_FPCW, SCRATCH, SCRATCH_SIZE, STACK_BASE,
                                 STACK_SIZE, finish_vectors, load_image, provenance,
                                 run_checked)

SQRT_APPROX = 0x4CAC40
FTOL = 0x7C5F00
SUBTRACT_POINTS = 0x487F20
TELEPORT = (0x719454, 0x7194AC)
WALK = (0x75BD33, 0x75BD70)
RADIATION = (0x65B93E, 0x65B98F)
INFANTRY = (0x481180, 0x481212)
PARTICLE_DELTA = (0x62B6C6, 0x62B707)
PARTICLE_NORMALIZE = (0x62B8D7, 0x62B956)
PARTICLE_ADVANCE = (0x62B956, 0x62BA51)
NEAREST = (0x56E6F0, 0x56E780)
PARTICLE_TYPE, PARTICLE_SPEED, PARTICLE_VELOCITY, PARTICLE_STATE_AI_ADVANCE = (
    0xAC, 0xE4, 0x10C, 0x12C)
TYPE_FINAL_DAMAGE_STATE, TYPE_NORMALIZED = 0x30C, 0x30F
DIRECT_COUNT, CHOSEN, DIRECT, INDIRECT = 0x24, 0x2C, 0x12C, 0x18C

FIRST, SECOND, OBJECT, TYPE = SCRATCH, SCRATCH + 0x20, SCRATCH + 0x100, SCRATCH + 0x400
STACK_TOP = STACK_BASE + STACK_SIZE - 0x1000


def machine(registers, coords=(), stack=()):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    for address, values in coords:
        uc.mem_write(address, struct.pack('<3i', *values))
    uc.mem_write(STACK_TOP, struct.pack(f'<{len(stack)}I', *stack))
    uc.reg_write(UC_X86_REG_ESP, STACK_TOP)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    for register, value in registers.items():
        uc.reg_write(register, value & 0xFFFFFFFF)
    return uc


def signed(uc, register):
    return struct.unpack('<i', struct.pack('<I', uc.reg_read(register)))[0]


def teleport(name, owner, destination):
    uc = machine({UC_X86_REG_EAX: FIRST, UC_X86_REG_EBP: SECOND},
                 [(FIRST, owner), (SECOND, destination)])
    run_checked(uc, *TELEPORT, required_addresses=(SQRT_APPROX, FTOL),
                context=dict(case=name, owner=owner, destination=destination))
    return dict(name=name, owner=owner, destination=destination,
                expected=signed(uc, UC_X86_REG_EAX))


def walk(name, owner, head):
    # The block's last instruction reloads the owner from [EBP+0xC].
    uc = machine({UC_X86_REG_EAX: FIRST, UC_X86_REG_ESI: head[0], UC_X86_REG_EDI: head[1],
                  UC_X86_REG_EBP: OBJECT}, [(FIRST, owner)])
    run_checked(uc, *WALK, required_addresses=(SQRT_APPROX, FTOL),
                context=dict(case=name, owner=owner, head=head))
    return dict(name=name, owner=owner, head=head, expected=signed(uc, UC_X86_REG_EAX))


def radiation(name, centre, cell):
    uc = machine({UC_X86_REG_ESI: centre[0], UC_X86_REG_EDI: centre[1],
                  UC_X86_REG_EBX: centre[2], UC_X86_REG_EAX: FIRST}, [(FIRST, cell)])
    run_checked(uc, *RADIATION, required_addresses=(SQRT_APPROX, FTOL),
                context=dict(case=name, centre=centre, cell=cell))
    return dict(name=name, centre=centre, cell=cell, expected=signed(uc, UC_X86_REG_EAX))


def infantry(name, coord):
    # PlaceInfantryInCell(result, coord, ...): the stack holds a return slot,
    # the result pointer and the coordinate pointer.
    uc = machine({UC_X86_REG_ECX: OBJECT}, [(FIRST, coord)], stack=(0, SECOND, FIRST))
    run_checked(uc, *INFANTRY, required_addresses=(SUBTRACT_POINTS, SQRT_APPROX, FTOL),
                context=dict(case=name, coord=coord))
    return dict(name=name, coord=coord, expected=signed(uc, UC_X86_REG_EDI))


def particle(name, source, target, speed, final_damage_state):
    uc = machine({UC_X86_REG_EAX: SECOND, UC_X86_REG_EDI: FIRST, UC_X86_REG_ESI: OBJECT},
                 [(FIRST, source), (SECOND, target)], stack=(0,) * 13 + (SECOND,))
    uc.mem_write(OBJECT + PARTICLE_TYPE, struct.pack('<I', TYPE))
    uc.mem_write(OBJECT + PARTICLE_SPEED, struct.pack('<f', speed))
    uc.mem_write(TYPE + TYPE_FINAL_DAMAGE_STATE, bytes([final_damage_state & 0xFF]))
    uc.mem_write(TYPE + TYPE_NORMALIZED, b'\x01')
    context = dict(case=name, source=source, target=target, speed=speed,
                   final_damage_state=final_damage_state)
    run_checked(uc, *PARTICLE_DELTA, context=context)
    run_checked(uc, *PARTICLE_NORMALIZE, required_addresses=(SQRT_APPROX,), context=context)
    run_checked(uc, *PARTICLE_ADVANCE, required_addresses=(FTOL,), context=context)
    velocity = struct.unpack('<3I', uc.mem_read(OBJECT + PARTICLE_VELOCITY, 12))
    advance = uc.mem_read(OBJECT + PARTICLE_STATE_AI_ADVANCE, 1)[0]
    return dict(name=name, source=source, target=target, speed=speed,
                final_damage_state=final_damage_state, velocity=list(velocity),
                advance=advance)


def nearest(name, target, direct, indirect=()):
    def packed(cell):
        return struct.unpack('<I', struct.pack('<2h', *cell))[0]
    frame = [0] * (INDIRECT // 4 + 24)
    frame[DIRECT_COUNT // 4] = len(direct)
    for base, cells in ((DIRECT, direct), (INDIRECT, indirect)):
        for index, cell in enumerate(cells):
            frame[base // 4 + index] = packed(cell)
    uc = machine({UC_X86_REG_ECX: len(direct), UC_X86_REG_ESI: len(indirect),
                  UC_X86_REG_EBP: FIRST}, stack=frame)
    uc.mem_write(FIRST, struct.pack('<2h', *target))
    run_checked(uc, *NEAREST, required_addresses=(SQRT_APPROX,),
                context=dict(case=name, target=target, direct=direct, indirect=indirect))
    chosen = struct.unpack('<2h', uc.mem_read(STACK_TOP + CHOSEN, 4))
    return dict(name=name, target=list(target), candidates=[list(c) for c in direct or indirect],
                expected=list(chosen))


def generate():
    return dict(
        teleport=[teleport(*case) for case in (
            ('seven cells east and one level up', [2688, 2688, 0], [4480, 2688, 104]),
            ('from an infantry spot off its cell centre', [2748, 2628, 0], [4480, 4480, 0]),
            ('a three-four-five run on one level', [2688, 2688, 0], [3456, 3712, 0]),
            ('the destination itself', [2688, 2688, 0], [2688, 2688, 0]),
        )],
        walk=[walk(*case) for case in (
            ('the head itself', [2688, 2688, 0], [2688, 2688, 0]),
            ('17 east of the head', [2705, 2688, 0], [2688, 2688, 0]),
            ('16 east and 5 south', [2704, 2693, 0], [2688, 2688, 0]),
            ('8 west and 15 north', [2680, 2673, 0], [2688, 2688, 0]),
            ('12 east and 12 south', [2700, 2700, 0], [2688, 2688, 0]),
        )],
        radiation=[radiation(*case) for case in (
            ('the centre cell', [128, 128, 0], [128, 128, 0]),
            ('seven cells south and one level up', [128, 128, 0], [128, 1920, 104]),
            ('a ramp four and five cells off', [128, 128, 0], [1152, 1408, 52]),
            ('a neighbour two levels down', [1408, 1408, 208], [1152, 1408, 0]),
        )],
        infantry=[infantry(*case) for case in (
            ('the cell centre', [2688, 2688, 0]),
            ('59 east, 11 south', [2747, 2699, 0]),
            ('59 east, 10 south', [2747, 2698, 0]),
            ('60 east', [2748, 2688, 0]),
            ('60 south', [2688, 2748, 0]),
            ('36 east, 48 south', [2724, 2736, 0]),
            ('60 west', [2628, 2688, 0]),
            ('far north-west', [2620, 2620, 0]),
        )],
        # Retail's one Normalized= type, FireStream, flies at 28.0 with
        # FinalDamageState=14.
        particle=[particle(*case) for case in (
            ('a three-four-five vector', [0, 0, 0], [3, 4, 0], 5.0, 0),
            ('a three-four-five run', [0, 0, 0], [300, 400, 0], 5.0, 14),
            ('the same run at a fractional speed', [0, 0, 0], [300, 400, 0], 2.5, 14),
            ('the unit diagonal', [100, 100, 100], [101, 101, 101], 28.0, 14),
            ('a long mixed vector', [2000, 1000, 50], [1300, 1250, 14], 28.0, 14),
            ('along one axis', [0, 0, 0], [1000, 0, 0], 28.0, 14),
            ('420 east', [0, 0, 0], [420, 0, 0], 28.0, 14),
            ('a steep climb', [0, 0, 0], [300, 400, 1200], 28.0, 14),
            ('300 east at speed one, past a byte', [0, 0, 0], [300, 0, 0], 1.0, 0),
            ('a short mixed vector', [7, 7, 7], [12, -5, 14], 28.0, 14),
            ('no offset', [5, 5, 5], [5, 5, 5], 28.0, 14),
            ('no offset, final damage state past 127', [5, 5, 5], [5, 5, 5], 28.0, 200),
            ('final damage state 255 divides by zero', [0, 0, 0], [300, 400, 0], 5.0, 255),
        )],
        nearest=[nearest(*case) for case in (
            ('the nearest of three', (10, 10), [(13, 14), (12, 12), (14, 13)]),
            ('equal distances keep the first', (10, 10), [(13, 14), (14, 13)]),
            ('128 cells and a step aside share a root', (10, 10), [(138, 11), (138, 10)]),
            ('150 cells off, squares 22501 and 22500 share a root', (10, 100),
             [(160, 101), (160, 100)]),
            ('distinct roots past 128 cells', (5, 5), [(160, 5), (159, 6)]),
            ('only indirect candidates', (10, 10), [], [(20, 20), (11, 9), (9, 11)]),
        )],
    )


def metadata():
    return provenance(
        scope=('The Sqrt_Approx distance blocks of Teleport Process, Walk ProcessMovement, '
               'RadSiteClass::GetRadLevelAt and PlaceInfantryInCell, and the particle '
               'constructor velocity normalization and Normalized= state-AI advance, and '
               'Find_Nearby_Passable_Cell\'s pick nearest a target, on synthetic coordinates.'),
        assumptions=['Registers and scratch memory hold the coordinates the callers read just '
                     'before each block (GetCoords results, the destination or step head, the '
                     'particle source and target); no objects, cells or map are built',
                     'A scratch particle type record holds only Normalized= and '
                     'FinalDamageState=; the particle only its type pointer and speed',
                     'The nearest pick starts from its frame as the candidate collection '
                     'leaves it: counts, packed candidate arrays and the target pointer',
                     'Startup x87 chop53 control 0E7F'],
        substitutions=[],
        entry_points={'teleport': TELEPORT[0], 'walk': WALK[0], 'radiation': RADIATION[0],
                      'infantry': INFANTRY[0], 'particle_delta': PARTICLE_DELTA[0],
                      'particle_normalize': PARTICLE_NORMALIZE[0],
                      'particle_advance': PARTICLE_ADVANCE[0], 'nearest': NEAREST[0],
                      'sqrt_approx': SQRT_APPROX, 'ftol': FTOL})


def source_paths():
    return {'producer': Path(__file__), 'native_oracle': Path('tools/native_oracle.py')}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
                   source_paths=source_paths())
