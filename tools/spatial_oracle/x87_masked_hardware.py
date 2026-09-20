"""Hardware references for the masked-x87 primitive matrix.

Unicorn2.1.4 preserves signaling NaNs at FLD, contradicting x87 hardware. Keep its
raw observations separately; never change the production model to emulate that bug.
This standalone assembler-only Rust probe does not call VERA20k arithmetic code.
"""
from pathlib import Path
import hashlib
import json
import platform
import subprocess
import tempfile

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from tools.native_oracle import finish_vectors, load_image, provenance
from tools.spatial_oracle.x87_masked_values import cases, FRAGMENTS

SOURCE = Path(__file__).with_suffix('.rs')
CAPTURE = {}


def generate():
    if platform.machine().lower() not in ('amd64', 'x86_64'):
        raise RuntimeError('Hardware reference capture needs an x86_64 CPU; Rust fixture tests remain portable')
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    for address, encoded in (*FRAGMENTS.values(), (0x7C5F1B, 'df38')):
        assert bytes(u.mem_read(address, len(bytes.fromhex(encoded)))) == bytes.fromhex(encoded)
    rows = cases()
    inputs = ''.join(f"{r['name']} {r['operation']} {r['lhs_format']} {r['lhs_bits']} "
                     f"{r.get('rhs_format', 'f32')} {r.get('rhs_bits', '0')}\n" for r in rows)
    with tempfile.TemporaryDirectory(prefix='vera20k-x87-hardware-') as temporary:
        runner = Path(temporary) / ('probe.exe' if platform.system() == 'Windows' else 'probe')
        subprocess.run(['rustc', '--edition=2024', '--crate-name', 'x87_masked_hardware',
                        str(SOURCE), '-o', str(runner)], check=True)
        observed = subprocess.check_output([str(runner)], input=inputs, encoding='utf-8').splitlines()
    assert observed[0].startswith('cpu|')
    CAPTURE['cpu'] = observed[0][4:]
    CAPTURE['compiler'] = subprocess.check_output(['rustc', '--version'], encoding='utf-8').strip()
    CAPTURE['host'] = platform.system() + '/' + platform.machine()
    assert len(observed) == len(rows) + 1
    emulator = json.loads(Path(__file__).with_name('x87_masked_values.json').read_text(encoding='utf-8'))
    assert len(emulator) == len(rows)
    result = []
    for row, line, emulated in zip(rows, observed[1:], emulator):
        name, raw, status = line.split()
        assert name == row['name'] and row == emulated['input']
        bits, status = int(raw, 16), int(status, 16)
        assert status & 0x3800 == 0
        if row['operation'] == 'compare':
            value = dict(ordering={0: 'greater', 0x100: 'less', 0x4000: 'equal',
                                   0x4500: 'unordered'}[status & 0x4500])
        elif row['operation'] == 'ftol':
            low = bits & 0xFFFFFFFF
            value = dict(integer_low=low if low < 0x80000000 else low - 0x100000000)
        else:
            value = dict(output_bits=f'{bits & 0xFFFFFFFF:08x}')
        emulated_value = {key: emulated[key] for key in value}
        result.append(dict(input=row, **value, hardware_status=f'{status:04x}',
                           emulator_value=emulated_value,
                           agrees_with_unicorn=value == emulated_value,
                           original_opcodes_authenticated=True))
    # These are independently captured differences, not substituted expectations.
    CAPTURE['disagreement_count'] = sum(not r['agrees_with_unicorn'] for r in result)
    return result


def metadata():
    result = provenance(
        scope='2376 actual x87 hardware primitive captures using authenticated original arithmetic opcodes in an x86_64 register/operand wrapper',
        assumptions=[
            'The standalone Rust source contains inline original x87 opcode bytes, never calls the VERA20k numeric implementation, and observes memory/condition bits directly. Python supplies raw operands and formats integer/hex outputs; no software floating-point model supplies expected values',
            'Original numerical opcode bytes are authenticated against the pinned PE before compilation. They are relocated into a compiler-owned wrapper: address registers use 64-bit fixture pointers. This is hardware instruction-value evidence, not execution of original 32-bit parent control flow',
            'Each row saves/restores the full caller FPU state with FXSAVE64/FXRSTOR64, initializes the x87 stack, supplies and observes0E7F, and verifies final TOP0. The hardware status word is recorded but only compare conditions and stack balance are asserted; this does not certify a full FPU environment model',
            'FTOL rows execute the original FISTP qword opcode7C5F1B and observe low32 under0E7F, not the entire helper/control-cache function. The separate Unicorn corpus executes that original whole helper',
            'The saved Unicorn2.1.4 observations remain alongside hardware values. Its FLD conversion preserves signaling NaNs instead of quieting them; later NaN selection consequently differs. Hardware outputs are reference authority for this matrix, including the rows where Unicorn disagrees',
            'Capture requires x86_64 hardware and rustc, but production Rust arithmetic and fixture tests require neither x86 instructions nor retail files at runtime. A capture from another CPU/compiler has new provenance and must be reviewed deliberately',
            'Representative operand-pair coverage is not arbitrary operation-chain, full extended-exponent range, unmasked trap or parent gameplay equivalence',
        ],
        substitutions=['Original arithmetic opcodes are relocated; the wrapper supplies memory, control word, operand sequencing and observation. This does not patch a live game or synthesize expected arithmetic'],
        entry_points={**{name: address for name, (address, _) in FRAGMENTS.items()}, 'fistp': 0x7C5F1B},
    )
    result['hardware_capture'] = dict(CAPTURE)
    # Git's Windows CRLF checkout conversion must not change source identity.
    result['probe_source_sha256'] = hashlib.sha256(SOURCE.read_text(encoding='utf-8').encode('utf-8')).hexdigest()
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
