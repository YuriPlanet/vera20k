"""Original Type reset through its first complete Process call.

The reset prefix executes all populated native Type scalar/base destructors.
Stop at668A2C, before the later INI/file reload tail. Filtered physical General,
CombatDamage and referenced Warhead sections supply the ordinary reread case;
this is not full retail discovery or the complete Full_Init lifecycle.
"""
import hashlib
from pathlib import Path
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.projectile_oracle.bridge_render_inputs import BulletReader, assets_root, lexical
from tools.rules_oracle.select_anim_inputs import controls, read_general_blocks, state, pointer_name
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords


class ResetReader(BulletReader):
    def __init__(self):
        self.retiring = {}
        self.reset_events = []
        self.record_reset = False
        super().__init__({})

    def hook(self, u, address, size, data):
        if self.record_reset:
            if address == 0x7C8B3D:
                ptr = self.read32(u.reg_read(UC_X86_REG_ESP) + 4)
                if ptr in self.retiring:
                    self.reset_events.append(dict(at=f'{address:08x}', event='delete_type',
                                                  **self.retiring[ptr]))
            elif address in (0x6686C0, 0x6688C7, 0x6688EB, 0x66890F, 0x6689E7,
                             0x668BF0, 0x668EF0, 0x668EF5, 0x668F56, 0x668F5B):
                self.reset_events.append(dict(at=f'{address:08x}', event='original_phase'))
        super().hook(u, address, size, data)


def fresh_reset():
    # Same initial constructed Weapon/Rules prefix as weapon_speed_order.fresh.
    m = ResetReader()
    u = m.u
    u.mem_write(0x887568, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    m.invoke(0x772FA0, m.cstring('OrderProbe'))
    rules = m.alloc(0x2000)
    u.mem_write(0x8871E0, dwords(rules))
    m.invoke(0x665650, rules)
    # Original RTTI in detach/destruction uses an initially empty SEH chain.
    u.mem_map(0, 0x1000)
    u.mem_write(0, dwords(0xFFFFFFFF))
    # Execute native vector initializers, stopping before CRT atexit registration.
    run_checked(u, 0x5F6FF0, 0x5F7026)
    run_checked(u, 0x4E7B60, 0x4E7B96)
    # Unrelated supplied Color fallback is absent in this reset fixture.
    u.mem_write(0xB054E0, dwords(0))
    u.mem_write(0x8874C0, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    return m, rules


def raw_references(m, rules):
    splash = m.read32(rules + 0xBC4)
    count = m.read32(rules + 0xBD0)
    return dict(lightning=m.read32(rules + 0x17B4),
                weather=m.read32(rules + 0x2F4), nullify=m.read32(rules + 0x350),
                splash_storage=splash, splash_count=count,
                splash=[m.read32(splash + 4 * i) for i in range(count)],
                warhead_count=m.read32(0x8874D0), anim_count=m.read32(0x8B4160))


def run_reset(m, rules, reread, *, include_process):
    before = raw_references(m, rules)
    for family, base in (('Animation', 0x8B4150), ('Warhead', 0x8874C0), ('Weapon', 0x887568)):
        data, count = m.read32(base + 4), m.read32(base + 16)
        for index in range(count):
            ptr = m.read32(data + 4 * index)
            m.retiring[ptr] = dict(family=family, name=pointer_name(m, ptr), pointer=ptr)
    m.rules_cache(reread)
    m.u.reg_write(UC_X86_REG_ESP, SP)
    m.u.reg_write(UC_X86_REG_ECX, rules)
    m.u.mem_write(SP + 4, dwords(RULES))
    m.record_reset = True
    end = 0x668A2C if include_process else 0x6689E7
    run_checked(m.u, 0x6686C0, end, count=2000000)
    m.record_reset = False
    deleted = [e['pointer'] for e in m.reset_events if e['event'] == 'delete_type']
    assert set(deleted) == set(m.retiring), (deleted, m.retiring)
    after = raw_references(m, rules)
    if not include_process:
        assert after['warhead_count'] == after['anim_count'] == after['lightning'] == 0
        assert all(after[k] == before[k] for k in ('weather', 'nullify', 'splash', 'splash_count', 'splash_storage'))
        assert all(p in deleted for p in [before['weather'], before['nullify'], *before['splash']])
    return dict(before_raw=before, after_raw=after, events=m.reset_events,
                stop_before=f'{end:08x}', cached_reread=reread)


def generate():
    m, rules = fresh_reset()
    wh = m.invoke(0x75E3B0, m.cstring('WH'))
    base = controls()[1][1]
    m.rules_cache(base)
    read_general_blocks(m, rules)
    m.invoke(0x75D3A0, wh, (RULES,))
    # Full CombatDamage read is safe in this bounded cache and owns SplashList.
    m.invoke(0x66BBB0, rules, (RULES,))
    before = state(m, rules, wh)
    stale = run_reset(m, rules, {}, include_process=False)
    stale['before_named'] = before

    raw = (assets_root() / 'RULESMD.INI').read_bytes()
    physical, _ = lexical(raw, {'General', 'CombatDamage', 'IonWH'})
    selected = {'General': {k: physical['General'][k] for k in
                           ('LightningWarhead', 'WeatherConBoltExplosion', 'WeaponNullifyAnim')},
                'CombatDamage': {'SplashList': physical['CombatDamage']['SplashList']},
                'IonWH': physical['IonWH']}
    m, rules = fresh_reset()
    m.rules_cache(selected)
    m.invoke(0x668BF0, rules, (RULES,))
    before = state(m, rules, m.read32(rules + 0x17B4))
    reread = run_reset(m, rules, selected, include_process=True)
    after = state(m, rules, m.read32(rules + 0x17B4))
    assert before == after
    reread.update(before_named=before, after_named=after)
    return dict(retired_references=stale,
                physical_reread=reread,
                physical_rules_sha256=hashlib.sha256(raw).hexdigest())


def metadata():
    return provenance(scope=__doc__, entry_points={'rules_ctor':0x665650,
        'reset':0x6686C0, 'first_reset_process_call':0x668A27,
        'process':0x668BF0, 'stop_after_first_process':0x668A2C}, assumptions=[
        'Original populated AnimType, WeaponType and WarheadType scalar/base destructors execute. Read-only observations record delete calls; no detach/dtor instruction or result is replaced.',
        'The first case stops before post-reset Process. Freed Anim references remain stale native addresses; they are not dereferenced to claim live identity. LightningWarhead detaches to null.',
        'The second case executes complete Process before and inside reset over physical selected fields and the full referenced IonWH section. It excludes unrelated retail type discovery and later reset INI/file reload passes.',
        'Original global vector initialization runs before atexit registration; SEH begins empty. Inherited unrelated synthetic Color registry fallback is removed from fixture initial state.',
    ], substitutions=[
        'BulletReader supplies deterministic bump allocation, no-op free, TLS, archive and cached lexical INI boundaries. Free calls are observed before that boundary; no storage reuse is modeled.',
    ])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
