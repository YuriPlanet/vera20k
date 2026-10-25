"""Read-only original 56DC20 ABI observation on the frozen Approach controls.

All eleven complete admission outputs must still equal the published frozen
packet before the additional stack and output observations are accepted.
"""
from pathlib import Path
import gzip
import json
import struct

from . import admission
from .publication import finish_vectors, publication_projection
from tools.native_oracle import first_difference
from unicorn.x86_const import UC_X86_REG_EAX

HERE = Path(__file__).resolve().parent


class Observed(admission.Admission):
    def execute(self, row):
        self.fnpc = []
        result = super().execute(row)
        result['fnpc'] = self.fnpc
        return result

    def observe_flow(self, pc, sp):
        if pc == 0x56DC20:
            args = list(struct.unpack('<15I', self.u.mem_read(sp + 4, 60)))
            self.fnpc.append(dict(
                pc=f'{pc:08x}', caller=f'{self.m.read32(sp):08x}', args=args,
                seed=list(struct.unpack('<hh', self.u.mem_read(args[1], 4))),
                reference_cell=list(struct.unpack(
                    '<hh', self.u.mem_read(args[12], 4),
                )),
            ))
        if pc == 0x4D69E8:
            self.fnpc[-1]['returned_cell'] = list(struct.unpack(
                '<hh', self.u.mem_read(self.u.reg_read(UC_X86_REG_EAX), 4),
            ))
        super().observe_flow(pc, sp)


def generate():
    original = admission.Admission
    admission.Admission = Observed
    try:
        result = admission.generate()
    finally:
        admission.Admission = original
    frozen = json.loads(gzip.decompress((HERE / 'admission.json.gz').read_bytes()))
    stripped = dict(result, cases=[
        {key: value for key, value in row.items() if key != 'fnpc'}
        for row in result['cases']
    ])
    stripped = json.loads(json.dumps(publication_projection(stripped)))
    difference = first_difference(frozen, stripped)
    assert difference is None, difference
    # Preserve the historical raw admission file identity in the frozen output.
    # The promoted body above is checked after the same lexical/path projection.
    promotion = json.loads((HERE / 'promotion.json').read_bytes())
    original_admission_sha = promotion['results']['admission.json']['frozen_source_sha256']
    return dict(
        schema=1,
        frozen_payload_sha256=original_admission_sha,
        cases=[dict(name=row['row']['name'], calls=row['fnpc']) for row in result['cases']],
    )


def metadata():
    result = admission.metadata()
    result['scope'] = __doc__
    result['assumptions'].append(
        'All eleven native outputs are checked exactly against the frozen '
        'admission packet after its declared lexical/path publication projection '
        'before accepting additional non-mutating 56DC20 stack/output '
        'observations. frozen_payload_sha256 retains the original pre-promotion '
        'admission JSON file identity recorded in promotion.json.'
    )
    return result


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'exhaustion_abi.json.gz', provenance=metadata)
