"""Original own-house building upgrade admission, install and power controls.

Reuses the Building art fixture and checked original-image runner. The ordinary
Unlimbo upgrade arm executes from the candidate/host admission through PowerUp
and type-slot commit. World lookup, house refresh and temporary-object disposal
are named boundaries, not substitutes for the observed upgrade decisions.
"""
from pathlib import Path
import hashlib
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EIP
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.object_health import OBJ, TYPE, VT, RULES, SP, dwords
from tools.spatial_oracle.building_art_transition import ArtFixture

UPGRADE = SCRATCH + 0x10000
UPGRADE_TYPE = UPGRADE + 0x2000
SECOND_UPGRADE_TYPE = UPGRADE + 0x4000
HOUSE = SCRATCH + 0x20000
OTHER_HOUSE = HOUSE + 0x8000
SPANS = ((0x452670, 0x4526E9), (0x4408DF, 0x440A30),
         (0x451400, 0x451743), (0x44E7B0, 0x44E8E2),
         (0x7C8D20, 0x7C8DF1))


class UpgradeFixture(ArtFixture):
    def __init__(self):
        super().__init__()
        self.u.mem_map(UPGRADE, 0x10000)
        self.u.mem_map(HOUSE, 0x10000)
        self.calls = []
        self.cleanup = 0
        self.u.hook_add(UC_HOOK_CODE, self.observe_upgrade_boundaries)

    def observe_upgrade_boundaries(self, u, address, _size, _data):
        names = {0x50B1D0: 'house_refresh', 0x509130: 'satellite_refresh'}
        if address == 0x451400:
            self.calls.append(dict(call='power_up', count=int(u.mem_read(OBJ + 0x702, 1)[0]), hp=self.read(OBJ + 0x6C)))
        if address == 0x451E40:
            self.calls.append(dict(call='remove_animation', slot=self.read(u.reg_read(UC_X86_REG_ESP) + 4)))
        name = names.get(address)
        if address == self.cleanup and u.reg_read(UC_X86_REG_ECX) == UPGRADE:
            name = 'temporary_upgrade_uninit'
        if name is None:
            return
        self.calls.append(dict(call=name, receiver=u.reg_read(UC_X86_REG_ECX)))
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, 0)
        u.reg_write(UC_X86_REG_ESP, sp + 4)
        u.reg_write(UC_X86_REG_EIP, self.read(sp))

    def prepare_upgrade(self, *, count=0, slots=3, level=-1, same_owner=True,
                        host='POWER', requested='POWER', hp=100):
        self.prepare(hp, 100, 0, 'missing')
        u = self.u
        u.mem_write(UPGRADE, bytes(0x10000))
        u.mem_write(HOUSE, bytes(0x10000))
        u.mem_write(OBJ + 0x21C, dwords(HOUSE))
        u.mem_write(TYPE + 0x24, host.encode('ascii') + b'\0')
        u.mem_write(TYPE + 0x14E0, dwords(slots))
        u.mem_write(TYPE + 0x1F8, b'POWERIMAGE\0')
        u.mem_write(OBJ + 0x702, bytes((count & 255,)))
        u.mem_write(OBJ + 0x5EC, bytes(12))
        u.mem_write(UPGRADE, dwords(VT))
        u.mem_write(UPGRADE + 0x21C, dwords(HOUSE if same_owner else OTHER_HOUSE))
        u.mem_write(UPGRADE + 0x520, dwords(UPGRADE_TYPE))
        u.mem_write(UPGRADE_TYPE + 0xE88, requested.encode('ascii') + b'\0')
        u.mem_write(UPGRADE_TYPE + 0x16FC, dwords(level))
        u.mem_write(UPGRADE_TYPE + 0x1F8, b'POWERUPIMAGE\0')
        # Ordinary host, not the special WallTower PowerUp path; empty authored
        # animation names except the native install's dynamically copied name.
        u.mem_write(RULES + 0x87C, dwords(0))
        for slot in range(21):
            u.mem_write(TYPE + 0xF4C + slot * 0x44, bytes(16))
            u.mem_write(TYPE + 0xF5C + slot * 0x44, bytes(16))
        self.cleanup = self.read(VT + 0xF8)
        self.calls.clear()

    def admit(self):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC, UPGRADE_TYPE, self.read(UPGRADE + 0x21C)))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, OBJ)
        run_checked(u, 0x452670, RET_MAGIC)
        return bool(u.reg_read(UC_X86_REG_EAX) & 255)

    def install(self):
        u = self.u
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, UPGRADE)
        u.reg_write(UC_X86_REG_EDI, OBJ)
        stopped = run_checked(u, 0x4408DF, (0x440926, 0x440A26), count=20000)
        return dict(admitted=stopped == 0x440A26,
                    count=int(u.mem_read(OBJ + 0x702, 1)[0]),
                    hp=self.read(OBJ + 0x6C),
                    slots=[self.read(OBJ + 0x5EC + slot * 4) for slot in range(3)],
                    preceding_field=self.read(OBJ + 0x5E8),
                    house_flags={hex(offset): int(u.mem_read(HOUSE + offset, 1)[0]) for offset in (0x5778, 0x5779, 0x1FC)},
                    calls=list(self.calls),
                    replacements=list(self.replacements))

    def power(self, function):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, OBJ)
        run_checked(u, function, RET_MAGIC)
        return struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]


def generate():
    f = UpgradeFixture()
    original = [bytes(f.u.mem_read(a, b-a)) for a,b in SPANS]
    cases = [
        ('first', dict()),
        ('next', dict(count=1)),
        ('full', dict(count=3)),
        ('zero_slots_first', dict(slots=0)),
        ('zero_slots_after_first', dict(count=1, slots=0)),
        ('explicit_one_first', dict(level=1)),
        ('explicit_three_first', dict(level=3)),
        ('explicit_three_small_host', dict(level=3, slots=1, hp=25)),
        ('explicit_three_after_first', dict(level=3, count=1)),
        ('level_zero', dict(level=0)),
        ('level_four', dict(level=4)),
        ('level_negative_two', dict(level=-2)),
        ('other_owner', dict(same_owner=False)),
        ('name_mismatch', dict(requested='OTHER')),
        ('name_case_fold', dict(requested='pOwEr', hp=25)),
    ]
    admissions = []
    for name, inputs in cases:
        f.prepare_upgrade(**inputs)
        admitted = f.admit()
        result = f.install()
        assert admitted == result['admitted']
        admissions.append(dict(name=name, inputs=inputs, admitted=admitted, install=result))
    power_rows = []
    for name, count, hp, strength, host_power, host_drain, upgrade_power, drain, other_slot in (
        ('full', 1,100,100,300,0,200,0,False),
        ('damaged',1,50,100,300,0,200,0,False),
        ('sparse_all_three_slots',1,100,100,300,0,200,0,True),
        ('count_zero_ignores_slots',0,100,100,300,0,200,0,True),
        ('fractional_truncation',1,1,3,3,0,2,0,False),
        ('drain_only',1,100,100,0,50,0,30,False),
    ):
        f.prepare_upgrade(count=count,hp=hp)
        u=f.u
        u.mem_write(OBJ+0x660,b'\1')
        u.mem_write(TYPE+0xA0,dwords(strength))
        u.mem_write(TYPE+0xEE0,dwords(host_power,host_drain))
        u.mem_write(UPGRADE_TYPE+0xEE0,dwords(upgrade_power,drain))
        u.mem_write(OBJ+0x5EC,dwords(UPGRADE_TYPE,0,SECOND_UPGRADE_TYPE if other_slot else 0))
        u.mem_write(SECOND_UPGRADE_TYPE+0xEE0,dwords(70,0))
        power_rows.append(dict(name=name,inputs=dict(count=count,hp=hp,strength=strength,
            host_power=host_power,host_drain=host_drain,upgrade_power=upgrade_power,
            upgrade_drain=drain,other_slot=other_slot),
            output=f.power(0x44E7B0),drain=f.power(0x44E880)))
    removal_rows = []
    for name,count,slot in (('first',1,0),('explicit_three_sparse',3,2),
                            ('null_last_slot',2,0),('empty',0,None)):
        f.prepare_upgrade(count=count)
        u=f.u
        u.mem_write(UPGRADE_TYPE+0x16F0,dwords(-1,-1))
        if slot is not None:
            u.mem_write(OBJ+0x5EC+slot*4,dwords(UPGRADE_TYPE))
        # No supplied live animations: the original release helper executes its
        # empty-slot arm. Full Anim destruction remains the existing owner.
        u.mem_write(OBJ+0x55C,bytes(21*4))
        u.mem_write(SP,dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_ECX,OBJ)
        run_checked(u,0x451690,RET_MAGIC)
        removal_rows.append(dict(name=name,initial_count=count,initial_type_slot=slot,
            count=int(u.mem_read(OBJ+0x702,1)[0]),
            slots=[f.read(OBJ+0x5EC+n*4) for n in range(3)],calls=list(f.calls)))
    assert original == [bytes(f.u.mem_read(a,b-a)) for a,b in SPANS]
    return dict(schema_version=1, admission_and_install=admissions, power=power_rows,
                removal=removal_rows)


def metadata():
    f=UpgradeFixture()
    result=provenance(
        scope='Original452670 admission,4408DF..440A26 own-building Unlimbo upgrade arm with451400 PowerUp,451690 ordinary PowerDown,44E7B0/44E880 power and drain.',
        assumptions=['Candidate lookup and Building RTTI already accepted; fixtures supply an ordinary own-house host, original Building vtable/type getter and original image bytes.',
                     'No WallTower, InfantryAbsorb, UnitAbsorb, overpower or satellite upgrade flags; health ratio and integer conversion execute originally with PC53/chop0E7F.',
                     'No full scenario startup, visual rendering, factory economy, house refresh implementation or temporary object destruction is certified.'],
        substitutions=['Existing ArtFixture451890 records native animation allocation arguments and supplies an arena pointer.',
                       '50B1D0 house refresh,509130 optional satellite refresh and upgrade temporary-object virtual UnInit are recorded callouts with no world effects; none decides admission, count, HP or type-slot writes.'],
        entry_points={'admission':0x452670,'install_arm':0x4408DF,'power_up':0x451400,'power_down':0x451690,'power':0x44E7B0,'drain':0x44E880})
    result['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
    return result


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
