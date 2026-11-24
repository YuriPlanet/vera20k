"""Original5FB2E0 controls for declared GameOptions speed and Anim delay inputs."""
from pathlib import Path
import struct
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from tools.native_oracle import call, image_bytes, file_span, finish_vectors, provenance

HERE = Path(__file__).resolve().parent


def generate():
    image = image_bytes()
    _, code = file_span(image, 0x5FB2E0, 0x26)
    # The native short arm reads dword[base + (delay*8 + speed)*4].
    # Delay zero returns early, so only the four delay1..4 rows are read.
    _, table = file_span(image, 0x832D0C, 4 * 8 * 4)
    cs = Cs(CS_ARCH_X86, CS_MODE_32)
    rows = []
    for speed in range(8):
        for delay in (0, 1, 2, 3, 4, 5, 900):
            result = call(0x5FB2E0, ecx=0xA8EB60, stack_args=[delay],
                          writes={0xA8EB60:struct.pack('<i', speed)},
                          required_addresses=(0x5FB2E0,))
            rows.append(dict(stored_speed=speed, input_delay=delay, returned_eax=result['eax']))
    return dict(code=code.hex(), table_address='0x832D0C', table=table.hex(),
                instructions=[dict(pc=hex(i.address), bytes=i.bytes.hex(),
                                   text=f'{i.mnemonic} {i.op_str}') for i in cs.disasm(code, 0x5FB2E0)],
                rows=rows)


def metadata():
    return provenance(scope=__doc__, entry_points={'speed_normalize':0x5FB2E0},
        assumptions=['Stored speed0..7 and input delay0..5/900 are explicit scalar inputs. Original table/code execute in a fresh pinned image for every control. Delays1 at speeds1/3 distinguish the v6 production/native H2O_EXP3 rate boundary.'],
        substitutions=['Only GameOptions receiver scalar and function argument are supplied. No code, output, timer or Anim state is replaced. This controls the rate conversion, not full options loading or animation lifecycle.'])


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'speed_normalize.json', provenance=metadata)
