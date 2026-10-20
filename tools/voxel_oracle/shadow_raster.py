"""Execute native selected-section VXL shadow preparation and stencil writes.

The renderer's inputs are immutable file bytes and a caller matrix. Only file
vtable IO and allocation are substituted; original render instructions run.
"""
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_ESI

from tools import native_oracle as oracle

BASE, SIZE = 0x20000000, 0x01000000
FILE, VTABLE, VXL, HVA_OBJECT = BASE + 0x100, BASE + 0x200, BASE + 0x300, BASE + 0x400
INPUT, INTERMEDIATE, SECTION, RESULT = BASE + 0x500, BASE + 0x600, BASE + 0x700, BASE + 0x800
RET, CALLBACK = 0x30000000, 0x30000100


def pack(*values):
    return struct.pack('<' + 'I' * len(values), *values)


def render_shadow_native(vxl_bytes, hva_bytes, vpl_bytes, draw_matrix, body_pixels=None,
                         *, considered_aircraft=False, shadow_index=0, body_offset=(0, 0)):
    """Original selected-section core, optionally reading a supplied BSurface.

    Without an HVA it makes VoxelAnimClass::DrawIt's call (0x00749CBE..
    0x00749CE8): the caller matrix itself and the viewer identity at 0x887430.
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
    substitutions = []
    uninitialized = set()
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
        nonlocal heap, pos
        sp = u.reg_read(UC_X86_REG_ESP)
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
    write(INPUT, draw_matrix)
    assert 0 <= shadow_index < section_count
    invoke(0x753e00)
    mask_surface = 0
    mask_reader = None
    if body_pixels is not None:
        mask_surface = BASE + 0xa00
        u.reg_write(UC_X86_REG_ESI, mask_surface)
        u.reg_write(UC_X86_REG_ESP, oracle.STACK_BASE + oracle.STACK_SIZE - 0x1000)
        oracle.run_checked(u, 0x7473fa, 0x74742e, required_addresses=(0x43ad00,))
        assert len(body_pixels) == 65536
        write(read32(mask_surface + 0x14), body_pixels)
        mask_reader = read32(read32(mask_surface) + 0x28)
    if hva_bytes is None:
        write(INTERMEDIATE, draw_matrix)
    else:
        #707280 takes frame-zero ShadowIndex HVA, independently of body frame.
        invoke(0x5af980, ecx=INTERMEDIATE, edx=INPUT,
               args=(read32(HVA_OBJECT + 0x0c) + shadow_index * 48,))
    view_product = 0x887430
    if hva_bytes is not None or considered_aircraft:
        invoke(0x5ae860, ecx=SECTION)
        #707319..70732C: ConsideredAircraft scales the viewer, independently
        #of the body/HVA matrix. Keep the original uniform-scale operation.
        if considered_aircraft:
            invoke(0x5aea10, ecx=SECTION, args=(0x3F000000,))
        view_product = BASE + 0x900
        invoke(0x5af980, ecx=view_product, edx=0x887430, args=(SECTION,))
    invoke(0x753f90, ecx=VXL, edx=shadow_index,
           args=(0, view_product, INTERMEDIATE, 0x887420, mask_surface, *body_offset),
           required=(0x7564b0, 0x5afb80))
    quad_before = bytes(u.mem_read(0xb3ff78, 0x48))
    assert read32(0xb2fb70) == 1
    invoke(0x754510, ecx=RESULT, required=(0x756860,) + ((mask_reader,) if mask_reader else ()))
    quad_after = bytes(u.mem_read(0xb3ff78, 0x48))
    assert not uninitialized, sorted(uninitialized)[:10]
    pixels = bytes(u.mem_read(0xb2ff78, 65536))
    return {
        'input_matrix_bits': list(struct.unpack('<12I', draw_matrix)),
        'hva_matrix_bits': [list(struct.unpack('<12I', matrix)) for matrix in hva_matrices],
        'combined_matrix_bits': list(struct.unpack('<12I', bytes(u.mem_read(INTERMEDIATE, 48)))),
        'viewer_matrix_bits': list(struct.unpack('<12I', bytes(u.mem_read(view_product, 48)))),
        'shadow_translation_bits': list(struct.unpack('<3I', bytes(u.mem_read(0x887420, 12)))),
        'quad_corners_before_bits': list(struct.unpack('<12I', quad_before[12:60])),
        'quad_corners_after_bits': list(struct.unpack('<12I', quad_after[12:60])),
        'quad_origin_after': list(struct.unpack('<2i', quad_after[64:72])),
        'rect': list(struct.unpack('<6i', bytes(u.mem_read(RESULT, 24)))),
        'pixels': [[i, value] for i, value in enumerate(pixels) if value],
        'read_before_initialized': [], 'substitutions': substitutions,
    }
