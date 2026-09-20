"""Original Infantry ART tail with supplied successful base-reader/Image boundary.

The actual tail5246A1 reads Crawls/firing frames and calls whole523D00. Successive
reads operate on the same constructed sequence bank; fixed ART entries and the
effective Type+1F8 Image are supplied, not inferred from a flattened projection.
"""
from pathlib import Path
import hashlib
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EBX
from tools.native_oracle import run_checked, finish_vectors, provenance
from tools.spatial_oracle.infantry_sequence_rules import Fixture as SequenceFixture, TYPE, SP, SPANS, INI
from tools.spatial_oracle.map_queries import dwords

TAIL = (0x5246A1, 0x52473F)
IMAGE = (0x5F92F8, 0x5F9340)


class Fixture(SequenceFixture):
    def __init__(self):
        super().__init__()
        for address in (0x5295F0, 0x5276D0):
            self.u.hook_add(UC_HOOK_CODE, self.read_boundary, begin=address, end=address)

    def execute_passes(self, passes, art):
        self.constructor()
        u = self.u
        # Constructor5237xx establishes Crawls=true. FireUp/Down frames are
        # supplied zero; this fixture's focus is retained sequence/Crawls data.
        u.mem_write(TYPE+0xEBD, b'\1')
        u.mem_write(TYPE+0xE40, bytes(16))
        self.reads = []
        original = bytes(u.mem_read(TAIL[0], TAIL[1]-TAIL[0]))
        states = []
        for image in passes:
            if image is not None:
                u.mem_write(TYPE+0x1F8, image.encode('ascii')+b'\0')
                self.values = {(section, key): value for section, entries in art.items()
                               for key, value in entries.items()}
                u.reg_write(UC_X86_REG_ESI, TYPE)
                u.reg_write(UC_X86_REG_ESP, SP)
                run_checked(u, TAIL[0], TAIL[1], count=200000,
                            required_addresses=[TAIL[0], 0x523D00])
            states.append(dict(records=self.records(), crawls=u.mem_read(TYPE+0xEBD, 1)[0],
                               fire_frames=list(struct.unpack('<4i', u.mem_read(TYPE+0xE40, 16)))))
        assert original == bytes(u.mem_read(TAIL[0], TAIL[1]-TAIL[0]))
        assert self.original == [bytes(u.mem_read(a,b-a)) for a,b in SPANS]
        return dict(passes=passes, art=art, states=states, reads=self.reads,
                    original_code_unchanged=True)

    def image_read(self, prior, raw):
        u = self.u
        u.mem_write(TYPE+0x24, b'TEST\0')
        u.mem_write(TYPE+0x1F8, prior.encode('ascii')+b'\0')
        self.values = {('TEST', 'Image'): raw}
        self.reads = []
        u.mem_write(SP+0x1B0, dwords(INI))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBX, TYPE)
        run_checked(u, IMAGE[0], IMAGE[1], count=10000,
                    required_addresses=[IMAGE[0], 0x528A10])
        return dict(prior=prior, raw=raw, output=self.string(TYPE+0x1F8), reads=self.reads)


def generate():
    f = Fixture()
    art = {'A': {'Sequence': 'ActionsA', 'Crawls': 'no'},
           'B': {'Sequence': 'ActionsB'},
           'C': {'Crawls': 'yes'},
           'D': {'Sequence': 'actionsA', 'Crawls': 'junk'},
           'E': {'Sequence': 'ActionsA', 'Crawls': ''},
           'ActionsA': {'Deploy': '1,2,3,S', 'Guard': '91,4,5'},
           'ActionsB': {'Deploy': '9', 'Ready': '7,1,1'},
           'actionsA': {'Deploy': '-2,-3,-4,W'}}
    cases = [f.execute_passes(passes, art) for passes in
            (['A'], ['B'], ['C'], ['Missing'], ['A','B'], ['B','A'],
             ['A','C'], ['A','Missing'], ['A',None,'B'], ['A','B','A'],
             ['A','D'], ['A','E'], ['D','B'], ['C','B'])]
    images = [f.image_read(prior, raw) for prior in ('TEST', 'OtherImage')
              for raw in (None, '', ' ', 'A', ' actionsA ', 'a', 'X'*24+'Tail',
                          ' '*23+'A B', ' '*24+'X', 'ABC,DEF')]
    return dict(schema_version=2, cases=cases, image_reads=images)


def metadata():
    f = Fixture()
    out = provenance(
        scope='Original Infantry ART reader tail and retained sequence/Crawls fields across supplied successful Rules reads and effective Image changes.',
        assumptions=[
            'Original5246A1..52473F executes Crawls/firing-frame reads and calls whole523D00. Base TechnoType ReadINI success and effective Image+1F8 are explicitly supplied per pass; null pass means its absence/failure does not enter this tail.',
            'Fixed ART section/key payloads are supplied through the same cached-index boundary as infantry_sequence_rules; original ReadString/ReadBool/ReadInt/sscanf are unchanged.',
            'Original sequence constructor loop executes. Crawls is supplied constructortrue from523788; fire-frame fields are suppliedzero. Type construction/Rules registry allocation/Image parser and Sounds lookup are not executed.',
            'Image rows execute original ObjectTypeReadINI5F92F8..5F9340 including the prior-field default copy and25-byte ReadString. Supplied prior Image/type ID and admitted base-reader branch; missing/empty/24-byte truncation cases do not execute other ObjectType fields.',
            'This proves retained caller data and exact ART section case, not whole Type loading or draw behavior.'
        ], substitutions=[], entry_points={'art_tail':TAIL[0], 'read_sequence':0x523D00})
    out['original_slices'] = [dict(start=f'{a:08X}', end_exclusive=f'{b:08X}',
        hex=(code := bytes(f.u.mem_read(a,b-a))).hex(), sha256=hashlib.sha256(code).hexdigest())
        for a,b in [*SPANS,TAIL,IMAGE]]
    return out


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
