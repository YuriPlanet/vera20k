"""Original TriggerType ReadINI flag region, including ReadString/strtok/atoi.

The cached INI entry and prior fields are supplied. Original CRT calls consume
the first three tokens before the original flag region runs; House/name/link
resolution, event/action loading and live Tag/Trigger lifecycle are excluded.
"""
import hashlib
from pathlib import Path

from unicorn.x86_const import *
from tools.native_oracle import RET_MAGIC, SCRATCH, finish_vectors, provenance, run_checked
from tools.spatial_oracle.building_body_rules import INI, SP, TYPE, dwords
from tools.spatial_oracle.infantry_deploy_rules import VectorFixture, PTD

BUFFER = SCRATCH + 0x8000
KEY = TYPE + 0x24
START, END = 0x7273B9, 0x72749B


class FlagsFixture(VectorFixture):
    def call(self, address, args, *, ecx=0, cdecl=False):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, ecx)
        run_checked(u, address, RET_MAGIC)
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4 + (0 if cdecl else 4 * len(args))
        return u.reg_read(UC_X86_REG_EAX)

    def execute(self, raw, prior):
        u = self.u
        u.mem_write(KEY, b'TEST_TRIGGER\0')
        self.ini(KEY, raw)
        u.mem_write(TYPE + 0x9C, bytes(prior))
        u.mem_write(PTD, bytes(0x74))
        code = bytes(u.mem_read(START, END - START))
        # Same capacity/default as ReadINI727274. Cached section avoids file IO.
        length = self.call(0x528A10, [TYPE + 0x1F8, KEY, 0x889F64, BUFFER, 512], ecx=INI)
        read = bytes(u.mem_read(BUFFER, 512)).split(b'\0')[0].decode('ascii')
        if length:
            # These are the original tokenizer calls, not Python tokenization.
            for pointer in (BUFFER, 0, 0):
                self.call(0x7C9CC2, [pointer, 0x817F70], cdecl=True)
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_EBP, TYPE)
            u.reg_write(UC_X86_REG_EBX, 0)
            run_checked(u, START, END)
            assert u.reg_read(UC_X86_REG_ESP) == SP
        assert bytes(u.mem_read(START, END - START)) == code
        return dict(raw=raw, prior=prior, read=read, applied=bool(length),
                    output=list(u.mem_read(TYPE + 0x9C, 5)))


def generate():
    f = FlagsFixture()
    rows = [None, '', ' ', 'Neutral,<none>,Name', 'Neutral,<none>,Name,',
            'Neutral,<none>,Name,0,1,1,1,0', 'Neutral,<none>,Name,1,1,1,1,0',
            'Neutral,<none>,Name,0,0,0,0,0', 'Neutral,<none>,Name,0,1',
            'Neutral,<none>,Name,,0,1,0,1,0', 'Neutral,<none>,Name, ,0,1,0,1',
            ',,Neutral,,<none>,,Name,,0,,1,,1,,1,,0,',
            'Neutral,<none>,Name,0,1,,0', 'Neutral,<none>,Name,0,1, ,0',
            'Neutral,<none>,Name,0,' + '1,' * 280,
            'Neutral,<none>,Name,' + ' ' * 490 + '0,1,1,1,1']
    for value in ('-1', '2', '01', 'junk', '0x10', '1tail', '+', '-0',
                  '2147483648', '4294967296', '9' * 40, '-9223372036854775808',
                  '\t-2', '\v1', '\f1', '\x1f1'):
        rows.append(f'Neutral,<none>,Name,{value},{value},{value},{value},{value}')
    return [f.execute(raw, prior) for prior in ([1, 1, 1, 1, 0], [0, 0, 0, 0, 1]) for raw in rows]


def metadata():
    f = FlagsFixture()
    code = bytes(f.u.mem_read(START, END - START))
    out = provenance(
        scope='Original TriggerType ReadINI7273B9..72749B flag assignments with real ReadString512, strtok and atoi; no live trigger execution.',
        assumptions=['Supplied cached INI entry and explicit prior9C..A0 fields; first prior is the fresh constructor726C80 state.',
                     'Three original strtok calls skip the House/link/name tokens; their resolution/copy and Events/Actions loading are excluded.',
                     'Missing/empty ReadString follows the reader72727B early exit and retains fields.',
                     'Values are supplied INI entries, not physical-file input; physical line-length limits are separate.'],
        substitutions=['Inherited fixture supplies external GetLastError/SetLastError/TlsGetValue imports and unused heap/lock imports. Original CRT/tokenization/numeric/flag instructions are unchanged.'],
        entry_points={'read_string': 0x528A10, 'strtok': 0x7C9CC2, 'atoi': 0x7C9BFD, 'flags': START})
    out['original_slice'] = dict(start=f'{START:08X}', end_exclusive=f'{END:08X}',
                                 hex=code.hex(), sha256=hashlib.sha256(code).hexdigest())
    return out


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
