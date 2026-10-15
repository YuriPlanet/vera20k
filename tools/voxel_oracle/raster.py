"""Execute native VXL/HVA loading, preparation, and opaque raster writes.

The renderer's inputs are immutable file bytes and a caller matrix. Only file
vtable IO and allocation are substituted; original render instructions run.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW

from tools import native_oracle as oracle

BASE, SIZE = 0x20000000, 0x01000000
FILE, VTABLE, VXL, HVA_OBJECT = BASE + 0x100, BASE + 0x200, BASE + 0x300, BASE + 0x400
INPUT, INTERMEDIATE, SECTION, RESULT = BASE + 0x500, BASE + 0x600, BASE + 0x700, BASE + 0x800
RET, CALLBACK = 0x30000000, 0x30000100


def pack(*values):
    return struct.pack('<' + 'I' * len(values), *values)


def render_native(vxl_bytes, hva_bytes, vpl_bytes, draw_matrix=None, diffuse=False,
                  *, bullet_velocity_bits=None):
    """Unmirrored native Render core, preserving all submitted sections.

    `diffuse` lights the draw as VoxelAnimClass::DrawIt does, through 753C80
    with section zero and the world light (0x00749D97..0x00749DA3), instead of
    TechnoClass's 753D00.
    Bullet mode executes468090 through46B0C0's completed packed raster and
    stops before490B90. The surface blit/Convert are outside that boundary.
    """
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    oracle.load_image(u)
    u.mem_map(oracle.STACK_BASE, oracle.STACK_SIZE)
    u.mem_map(BASE, SIZE)
    u.mem_map(RET, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, oracle.NATIVE_FPCW)
    u.reg_write(UC_X86_REG_ESP, oracle.STACK_BASE + oracle.STACK_SIZE - 0x1000)
    u.mem_write(FILE, pack(VTABLE))
    for offset in (0x1c, 0x24, 0x28, 0x34):
        u.mem_write(VTABLE + offset, pack(CALLBACK + offset))
    heap = BASE + 0x10000
    data, pos = b'', 0
    substitutions, params = [], []
    bullet_observation, section_matrices = {}, []
    observing_bullet = False
    writes, zero_writes, zero_erases = 0, 0, 0
    visited, uninitialized = set(), set()
    initialized = bytearray(oracle.IMAGE_SIZE)
    for rva, raw, size, _vsz, _flags in oracle._sections(oracle.image_bytes()):
        initialized[rva:rva + size] = b'\1' * size

    def write(address, blob):
        u.mem_write(address, blob)
        if oracle.IMAGE_BASE <= address < oracle.IMAGE_BASE + oracle.IMAGE_SIZE:
            start = address - oracle.IMAGE_BASE
            initialized[start:start + len(blob)] = b'\1' * len(blob)

    def memory(_u, access, address, size, _value, _userdata):
        if oracle.IMAGE_BASE <= address < oracle.IMAGE_BASE + oracle.IMAGE_SIZE:
            start = address - oracle.IMAGE_BASE
            if access == 17:
                initialized[start:start + size] = b'\1' * size
            elif not all(initialized[start:start + size]):
                for byte in range(address, address + size):
                    if not initialized[byte - oracle.IMAGE_BASE]:
                        uninitialized.add((u.reg_read(UC_X86_REG_EIP), byte))

    u.hook_add(UC_HOOK_MEM_READ | UC_HOOK_MEM_WRITE, memory)

    def read32(address):
        return struct.unpack('<I', bytes(u.mem_read(address, 4)))[0]

    def finish(value, argument_bytes):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_ESP, sp + 4 + argument_bytes)
        u.reg_write(UC_X86_REG_EIP, read32(sp))

    def code(_u, address, _size, _userdata):
        nonlocal heap, pos, writes, zero_writes, zero_erases
        visited.add(address)
        sp = u.reg_read(UC_X86_REG_ESP)
        if observing_bullet:
            if address in (0x65c780, 0x65c7e0):
                raise oracle.OracleError('Bullet draw unexpectedly consumed RNG')
            if address in (0x468152, 0x4681db):
                name, offset = ('heading_word', 0x20) if address == 0x468152 else ('pitch_word', 0x28)
                bullet_observation[name] = struct.unpack('<H', u.mem_read(sp + offset, 2))[0]
            elif address == 0x468166:
                bullet_observation['heading_step'] = u.reg_read(UC_X86_REG_EDX)
            elif address == 0x4681eb:
                bullet_observation['pitch_step'] = u.reg_read(UC_X86_REG_ECX)
            elif address == 0x46b0c0:
                args = [read32(sp + 4 + i * 4) for i in range(7)]
                write(INPUT, bytes(u.mem_read(args[1], 48)))
                bullet_observation['draw'] = dict(frame=args[4], flags=args[5], brightness=args[6],
                    point=list(struct.unpack('<2i', u.mem_read(args[2], 8))),
                    clip=list(struct.unpack('<4i', u.mem_read(args[3], 16))))
            elif address == 0x7540f0:
                section_matrices.append(list(struct.unpack('<12I', u.mem_read(read32(sp + 8), 48))))
            elif address == 0x46b18c:
                write(RESULT, bytes(u.mem_read(u.reg_read(UC_X86_REG_EAX), 24)))
        if address in (0x7dfab6, 0x7dfbc8):
            writes += 1
            if u.reg_read(UC_X86_REG_EDX) & 255 == 0:
                zero_writes += 1
                destination = 0xb2ff78 + u.reg_read(UC_X86_REG_EAX)
                zero_erases += int(bytes(u.mem_read(destination, 1))[0] != 0)
        if address in (0x7df9c0, 0x7dfae0):
            raw = bytes(u.mem_read(read32(sp + 4), 0x34))
            params.append({
                'entry': address,
                'column_steps': list(struct.unpack_from('<3i', raw, 12)),
                'origin_xy': list(struct.unpack_from('<2H', raw, 24)),
                'axis_xy': [list(struct.unpack_from('<2h', raw, offset)) for offset in (30, 36, 42)],
                'sizes': list(raw[48:51]),
            })
        if address == 0x7c8e17:
            count = read32(sp + 4)
            allocation = heap
            heap += (count + 15) & ~15
            assert heap < BASE + SIZE
            substitutions.append(['allocate', count])
            finish(allocation, 0)
        elif CALLBACK <= address < CALLBACK + 0x100:
            op = address - CALLBACK
            assert u.reg_read(UC_X86_REG_ECX) == FILE
            if op == 0x1c:
                assert read32(sp + 4) == 1
                pos = 0
                finish(1, 4)
            elif op == 0x24:
                destination, count = read32(sp + 4), read32(sp + 8)
                assert 0 <= pos <= pos + count <= len(data)
                write(destination, data[pos:pos + count])
                substitutions.append(['read', pos, count])
                pos += count
                finish(count, 8)
            elif op == 0x28:
                distance, origin = read32(sp + 4), read32(sp + 8)
                assert origin == 1
                pos += distance
                finish(pos, 8)
            elif op == 0x34:
                finish(0, 0)
            else:
                raise AssertionError(hex(address))

    u.hook_add(UC_HOOK_CODE, code)

    def invoke(address, ecx=0, edx=0, args=(), required=()):
        sp = oracle.STACK_BASE + oracle.STACK_SIZE - 0x1000 - 4 * (1 + len(args))
        u.mem_write(sp, pack(RET, *args))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ECX, ecx)
        u.reg_write(UC_X86_REG_EDX, edx)
        oracle.run_checked(u, address, RET, required_addresses=required)
        return u.reg_read(UC_X86_REG_EAX)

    data = vpl_bytes
    assert invoke(0x753b70, ecx=FILE, required=(0x758a30,)) == 0
    data = vxl_bytes
    assert invoke(0x755db0, ecx=VXL, args=(FILE, 0), required=(0x756035, 0x756144)) == 1
    section_count = read32(VXL + 4)
    hva_matrices = []
    if hva_bytes is not None:
        data = hva_bytes
        assert invoke(0x5bd5c0, ecx=HVA_OBJECT, args=(FILE,), required=(0x5ae5e0,)) == 1
        assert read32(HVA_OBJECT + 4) == section_count
        scale_bits = read32(read32(VXL + 0x14) + 0x0c)
        invoke(0x5bd730, ecx=HVA_OBJECT, args=(scale_bits,))
        for section in range(section_count):
            hva_matrices.append(bytes(u.mem_read(read32(HVA_OBJECT + 0x0c) + section * 48, 48)))

    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_ESP, oracle.STACK_BASE + oracle.STACK_SIZE - 0x1000)
    oracle.run_checked(u, 0x52bddd, 0x52be0a, required_addresses=(0x5ae860, 0x754c00))
    if bullet_velocity_bits is not None:
        if hva_bytes is None:
            raise ValueError('Bullet controls require its loaded HVA')
        from tools.voxel_oracle.lighting import native_camera
        bullet, ptype, scene, point, clip, colors, scheme = [BASE + n for n in
            (0x1000, 0x2000, 0x3000, 0x4000, 0x4010, 0x4100, 0x4200)]
        write(0xb44318, native_camera())
        write(0xa8b230, pack(scene))
        write(bullet + 0xac, pack(ptype))
        write(bullet + 0xe8, struct.pack('<3Q', *[int(value, 16) for value in bullet_velocity_bits]))
        write(ptype + 0xb0, pack(VXL, HVA_OBJECT))
        write(ptype + 0x236, b'\1')
        # Only the pointer selection is observed; no supplied Convert executes.
        # Index1 is the independently covered absent-Color LightGold result.
        write(ptype + 0x2d4, pack(1))
        write(0xb054d4, pack(colors))
        write(colors, pack(scheme, scheme))
        write(point, struct.pack('<2i', 32, 48))
        write(clip, struct.pack('<4i', 0, 0, 64, 96))
        sp = oracle.STACK_BASE + oracle.STACK_SIZE - 0x1000
        write(sp, pack(RET, point, clip))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ECX, bullet)
        observing_bullet = True
        oracle.run_checked(u, 0x468090, 0x46b21a, required_addresses=(
            0x468112, 0x4cae30, 0x7c5f00, 0x4cac40, 0x5af1a0, 0x5af080,
            0x754be0, 0x5af980, 0x46b0c0, 0x753c80, 0x753e00, 0x7540f0, 0x754510))
        observing_bullet = False
        draw_matrix = bytes(u.mem_read(INPUT, 48))
        sp = u.reg_read(UC_X86_REG_ESP)
        bullet_observation['palette_call'] = dict(
            color_index=read32(ptype + 0x2d4),
            flags=read32(sp), remap=read32(sp + 4), gradient=read32(sp + 8),
            brightness=read32(sp + 12))
    else:
        write(INPUT, draw_matrix)
    if bullet_velocity_bits is not None:
        pass
    elif diffuse:
        invoke(0x753c80, ecx=VXL, args=(0, INPUT, 0x887470), required=(0x758670,))
    else:
        invoke(0x753d00, ecx=VXL, args=(0, INPUT, 0x887430, 0x887470, 0x40400000),
               required=(0x7586f0,))
    if bullet_velocity_bits is None:
        invoke(0x753e00)
    for section in range(section_count) if bullet_velocity_bits is None else ():
        if hva_bytes is not None:
            invoke(0x5af980, ecx=INTERMEDIATE, edx=INPUT,
                   args=(read32(HVA_OBJECT + 0x0c) + section * 48,))
        else:
            write(INTERMEDIATE, draw_matrix)
        invoke(0x5af980, ecx=SECTION, edx=0x887430, args=(INTERMEDIATE,))
        section_matrices.append(list(struct.unpack('<12I', bytes(u.mem_read(SECTION, 48)))))
        invoke(0x7540f0, ecx=VXL, edx=section, args=(0, SECTION), required=(0x7564b0, 0x5afb80))
    boxes = []
    for section in range(section_count):
        raw = bytes(u.mem_read(0xb2d958 + section * 0x88, 0x88))
        boxes.append({
            'corner_index': struct.unpack_from('<I', raw, 12)[0],
            'extents_bits': list(struct.unpack_from('<6I', raw, 16)),
            'corner_bits': list(struct.unpack_from('<24I', raw, 40)),
        })
    if bullet_velocity_bits is None:
        invoke(0x754510, ecx=RESULT, required=(0x756590,))
    assert params and all(p['entry'] in (0x7df9c0, 0x7dfae0) for p in params)
    assert not uninitialized, sorted(uninitialized)[:10]
    pixels = bytes(u.mem_read(0xb2ff78, 65536))
    result = {
        'input_matrix_bits': list(struct.unpack('<12I', draw_matrix)),
        'hva_matrix_bits': [list(struct.unpack('<12I', matrix)) for matrix in hva_matrices],
        'section_matrix_bits': section_matrices, 'boxes': boxes,
        'raster_params': params,
        'rect': list(struct.unpack('<6i', bytes(u.mem_read(RESULT, 24)))),
        'paint_order': [(read32(0xb2d824 + i * 4) - 0xb2d958) // 0x88 for i in range(section_count)],
        'pixels': [[i, value] for i, value in enumerate(pixels) if value],
        'write_counts': {'all': writes, 'zero': zero_writes, 'zero_erases_nonzero': zero_erases},
        'normal_pages': bytes(u.mem_read(0xb45990, 256)).hex(),
        'read_before_initialized': [], 'substitutions': substitutions,
    }
    if bullet_velocity_bits is not None:
        result['bullet'] = bullet_observation
    return result
