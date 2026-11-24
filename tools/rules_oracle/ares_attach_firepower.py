"""Two native Ares Firepower histories through the existing attachment VM.
Original damage functions read the live aggregate on the same actor.
"""
import argparse
import os
import json
import struct
import hashlib
from pathlib import Path
from tools.rules_oracle.ares_attach_effect import AttachEffectProbe
from tools.rules_oracle.ares_emp import native, identified_bytes, ARES_SHA256, signed
from tools.spatial_oracle.damage_build import FIRE_BEGIN, FIRE_END, FIRE_STAGES
from tools.spatial_oracle.estimated_damage import ENTRY as ESTIMATE_ENTRY
from unicorn import UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.native_oracle import run_checked, finish_vectors, provenance

class FirepowerProbe(AttachEffectProbe):

    def __init__(self, images):
        self.stage = 'init'
        self.aggregate_calls = []
        self.spills = []
        self.factor_reads = []
        self.fire_stages = []
        super().__init__(images)
        self.u.hook_add(UC_HOOK_MEM_WRITE, self.observe_write)
        self.u.hook_add(UC_HOOK_MEM_READ, self.observe_read)

    def hook(self, u, pc, size, data):
        if self.world and pc == 0x100460A0:
            self.aggregate_calls.append(dict(action=self.stage, entry_count=(self.read(self.runtime + 0x24) - self.read(self.runtime + 0x20)) // 16))
        if pc in FIRE_STAGES:
            self.fire_stages.append(FIRE_STAGES[pc])
        super().hook(u, pc, size, data)

    def observe_write(self, u, access, address, size, value, data):
        pc = u.reg_read(UC_X86_REG_EIP)
        if pc in (0x10046180, 0x10046124, 0x1005F997):
            assert size == 8
            self.spills.append(dict(action=self.stage, instruction=f'{pc:08X}', destination='actor160' if address == self.actor + 0x160 else 'permanentExt80' if address == self.runtime + 0x80 else 'aggregate_binary64_intermediate', bits_le=(value & 0xFFFFFFFFFFFFFFFF).to_bytes(8, 'little').hex()))

    def observe_read(self, u, access, address, size, value, data):
        if hasattr(self, 'actor') and address == self.actor + 0x160:
            self.factor_reads.append(dict(action=self.stage, instruction=f'{u.reg_read(UC_X86_REG_EIP):08X}', bits_le=bytes(u.mem_read(address, size)).hex()))

    def setup(self, rank=False):
        self.prepare(False)
        self.write(self.actor, 0x7F5C70)
        self.write(self.typ, 0x7F6218)
        self.write(self.actor + 0x14, 5)
        self.write(self.actor + 0x6C4, self.typ)
        self.u.mem_write(self.actor + 0x150, struct.pack('<f', 1.0 if rank else 0.0))
        self.u.mem_write(self.typ + 0x29E, bytes((int(rank),)))
        self.house = self.read(self.actor + 0x21C)
        self.country = self.alloc(0x600)
        self.write(self.house + 0x34, self.country)
        self.u.mem_write(self.house + 0x188, struct.pack('<d', 1.25))
        for off in (0x100, 0x104, 0x108, 0x10C, 0x110):
            self.u.mem_write(self.country + off, struct.pack('<f', 1.0))
        self.damage_rules = self.read(0x8871E0)
        self.u.mem_write(self.damage_rules + 0x670, struct.pack('<d', 1.5))
        self.u.mem_write(self.damage_rules + 0x688, struct.pack('<d', 1.0))
        self.write(self.damage_rules + 0x16C8, 100000)
        self.weapon, self.damage_wh, self.target, self.target_type, self.target_house, self.target_country = [self.alloc(n) for n in (0x200, 0x300, 0x1000, 0x2000, 0x6000, 0x600)]
        self.write(self.weapon + 0xA4, 101)
        self.write(self.weapon + 0xAC, self.damage_wh)
        for i in range(11):
            self.u.mem_write(self.damage_wh + 0xA0 + i * 8, struct.pack('<d', 1.0))
        self.u.mem_write(self.damage_wh + 0x12C, struct.pack('<f', 1.0))
        self.write(self.target, 0x7F5C70)
        self.write(self.target_type, 0x7F6218)
        self.write(self.target + 0x6C4, self.target_type)
        self.write(self.target + 0x21C, self.target_house)
        self.write(self.target_house + 0x34, self.target_country)
        for off in (0x100, 0x104, 0x108, 0x10C, 0x110):
            self.u.mem_write(self.target_country + off, struct.pack('<f', 1.0))
        self.write(self.typ + 0x9C, 0)
        self.write(self.target_type + 0x9C, 0)
        self.descriptors = []
        self.aggregate_calls = []
        self.spills = []
        self.factor_reads = []
        return self

    def descriptor(self, name, duration, factor, discard=False, cumulative=False, existing=False):
        if existing:
            wh, ext = (self.wh, self.whext)
        else:
            wh, ext = (self.alloc(0x300), self.alloc(0x200))
        self.write(wh + 0x1CC, ext)
        self.u.mem_write(wh + 0x24, name.encode() + b'\x00')
        self.write(ext, wh)
        self.call(0x10051890, wh, ecx=ext)
        self.u.mem_write(wh + 0x179, b'\x01')
        ini = self.alloc(4)
        self.write(ini, 0x887048)
        self.reads = {(name, 'AttachEffect.Duration'): str(duration), (name, 'AttachEffect.FirepowerMultiplier'): factor, (name, 'AttachEffect.DiscardOnEntry'): 'yes' if discard else 'no', (name, 'AttachEffect.Cumulative'): 'yes' if cumulative else 'no'}
        self.call(0x10058A50, ini, ecx=ext + 0x88)
        self.call(0x100523E0, self.text('3%'), ecx=self.read(ext + 0x24))
        self.u.mem_write(wh + 0x124, struct.pack('<f', 0.7))
        self.descriptors.append(dict(name=name, duration=signed(self.read(ext + 0x8C)), raw_firepower=factor, firepower_bits_le=bytes(self.u.mem_read(ext + 0xA0, 8)).hex(), discard=bool(self.u.mem_read(ext + 0x92, 1)[0]), cumulative=bool(self.u.mem_read(ext + 0x90, 1)[0]), animation=self.read(ext + 0x94)))
        return ext

    def hit(self, ext):
        self.call(0x10052700, self.actor + 0x9C, self.source_house, ecx=ext)

    def tick(self, n=1):
        for _ in range(n):
            self.call(0x10058E00, self.runtime, ecx=self.runtime + 0x20)

    def crate(self):
        frame = self.alloc(0x80)
        stack = self.alloc(0x60)
        self.write(frame + 8, self.actor)
        self.write(frame + 0x20, self.actor)
        self.write(frame + 0x14, stack)
        self.u.mem_write(stack + 0x20, struct.pack('<d', 1.2))
        picker = self.call(0x1005F940, frame)
        recipient = self.call(0x1005F970, frame)
        return dict(picker=f'{picker:08X}', recipient=f'{recipient:08X}')

    def combat(self):
        sp = 0x2000F000
        self.write(sp + 0x40, self.weapon)
        self.u.reg_write(UC_X86_REG_ESP, sp)
        self.u.reg_write(UC_X86_REG_ESI, self.actor)
        self.fire_stages = []
        run_checked(self.u, FIRE_BEGIN, FIRE_END, count=100000, required_addresses=[FIRE_BEGIN], context={'scope': 'existing original FireAt damage region, live AE actor'})
        self.check_code()
        damage = signed(self.read(sp + 0x2C))
        assert signed(self.u.reg_read(UC_X86_REG_EDI)) == damage
        estimate = signed(self.call(ESTIMATE_ENTRY, self.target, self.weapon, ecx=self.actor))
        return dict(fire_damage=damage, estimated_damage=estimate, fire_stages=self.fire_stages)

    def observation(self, action, crate=None):
        self.stage = action
        read_start = len(self.factor_reads)
        result = dict(action=action, base_bits_le=bytes(self.u.mem_read(self.runtime + 0x80, 8)).hex(), derived_bits_le=bytes(self.u.mem_read(self.actor + 0x160, 8)).hex(), effects=self.observation_effects(), combat=self.combat())
        result['native_live_factor_reads'] = self.factor_reads[read_start:]
        if crate is not None:
            result['crate'] = crate
        return result

    def observation_effects(self):
        return super().observation()

    def perform(self, action, fn):
        self.stage = action
        before = len(self.aggregate_calls)
        spill_start = len(self.spills)
        crate = fn()
        result = self.observation(action, crate)
        result['aggregate_calls'] = self.aggregate_calls[before:]
        result['native_stores'] = self.spills[spill_start:]
        return result

    def finish(self, name, steps):
        self.check_code()
        return dict(name=name, descriptors=self.descriptors, steps=steps, three_rng_before=[hashlib.sha256(x).hexdigest() for x in self.rng_before], three_rng_after=[hashlib.sha256(bytes(self.u.mem_read(x, 0x3F4))).hexdigest() for x in self.rng], code_unchanged=True, house_firepower_bits_le=bytes(self.u.mem_read(self.house + 0x188, 8)).hex(), rank_bits_le=bytes(self.u.mem_read(self.actor + 0x150, 4)).hex(), veteran_combat_bits_le=bytes(self.u.mem_read(self.damage_rules + 0x670, 8)).hex())

def generate(ares):
    images = (ares, native.image_bytes())
    controls = []
    p = FirepowerProbe(images).setup()
    ext = p.descriptor('FACTOR_A', 4, '0.2', discard=True, existing=True)
    steps = [p.perform('prepared', lambda: None), p.perform('first_hit', lambda: p.hit(ext)), p.perform('tick_1', lambda: p.tick()), p.perform('noncumulative_refresh', lambda: p.hit(ext)), p.perform('crate_during_effect', p.crate), p.perform('duplicate_crate_rejected', p.crate)]
    steps.append(p.perform('four_visits_expire', lambda: p.tick(4)))
    p.write(p.weapon + 0xA4, -7)
    steps.append(p.perform('negative_damage_bypasses_factor', lambda: None))
    controls.append(p.finish('live_aggregate_fire_estimate_refresh_crate_expiry', steps))
    p = FirepowerProbe(images).setup(rank=True)
    a = p.descriptor('FACTOR_A', 4, '0.2', existing=True)
    b = p.descriptor('FACTOR_B', 2, '1.1', discard=True, cumulative=True)
    steps = [p.perform('crate_before_effect', p.crate), p.perform('first_factor', lambda: p.hit(a)), p.perform('ordered_second_factor', lambda: p.hit(b)), p.perform('cumulative_third_entry', lambda: p.hit(b))]

    def remove():
        removed = p.call(0x100588E0, ecx=p.runtime + 0x20) & 255
        if removed:
            p.call(0x100460A0, ecx=p.runtime)
        return dict(remove_returned_al=removed)
    steps.extend([p.perform('discard_removes_second_and_third', remove), p.perform('four_visits_expire_first', lambda: p.tick(4))])
    controls.append(p.finish('crate_baseline_ordered_spills_rank_and_discard', steps))
    return dict(schema_version=1, controls=controls)

def metadata():
    result = provenance(scope='Two native-executed Ares Firepower aggregate and live damage histories.', assumptions=['Reuses AttachEffectProbe/EmpProbe/Probe images, CRT, lexical INI, heap and declared collector/world services. Synthetic Unit/UnitType, House1.25, rawDamage101, neutral defenses and VeteranCombat1.5 are inputs, not retail tuning.', 'Actual AE reader, collector/attach, aggregate100460A0, update and Remove-vector execute. Firepower crate hooks1005F940/1005F970 use permanent Ext80; accepted recipients aggregate immediately.', 'FireAt damage region6FE302..6FE460 and complete EstimateDamage6FDB80 read the same live actor160 without copying or injecting a computed aggregate. Original getters/house/rank/ftol/warhead execute. Whole FireAt, Bullet emission, HP commit and scan scheduler are excluded.', 'Native10046180 records each ordered binary64 intermediate;10046124 writes actor160. Duration-only refresh and duplicate crates do not aggregate; Remove conditionally aggregates as native1004B197, excluding full boarding/extension lookup.', 'Actual parsed factors0.2/1.1, durations4/2, null animations and cumulative/discard flags; visits are admitted actor visits, not frames. Numeric outputs are native little-endian bytes, never host gameplay calculations.', 'Code spans and all three seeded RNG states are byte-guarded after calls; before/after full-state SHA256 recorded. No guarded RNG/detach entry reached; supplied animation services retain their existing boundary.'], substitutions=['Existing attachment services only; no numeric/aggregate result injection.'], entry_points=dict(reader=0x10058A50, attach=0x10052700, aggregate=0x100460A0, update=0x10058E00, remove=0x100588E0, picker=0x1005F940, recipient=0x1005F970, fire_begin=FIRE_BEGIN, fire_end=FIRE_END, estimate=ESTIMATE_ENTRY))
    result.update(ares_sha256=ARES_SHA256, control_count=2, actual_execution=True, execution_batches='Two histories already executed once. Generic corpus is a lossless asserted transfer without replaying these or previous neutral/speed controls.', command='python -m tools.rules_oracle.ares_attach_firepower --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --check')
    return result

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ares-dll', type=Path, default=os.environ.get('VERA20K_ARES_DLL'))
    parser.add_argument('--gamemd-exe', type=Path)
    args, remaining = parser.parse_known_args()
    if args.ares_dll is None:
        parser.error('Set VERA20K_ARES_DLL or --ares-dll')
    if args.gamemd_exe:
        os.environ['VERA20K_GAMEMD_EXE'] = str(args.gamemd_exe)
    ares = identified_bytes(args.ares_dll, ARES_SHA256)
    finish_vectors(lambda: generate(ares), Path(__file__).with_suffix('.json'), provenance=metadata, argv=remaining, source_paths={'producer': Path(__file__), 'attachment_owner': Path(__file__).with_name('ares_attach_effect.py'), 'emp_owner': Path(__file__).with_name('ares_emp.py'), 'vm_owner': Path(__file__).with_name('ares_prerequisites.py'), 'fire_interface': Path('tools/spatial_oracle/damage_build.py'), 'estimate_interface': Path('tools/spatial_oracle/estimated_damage.py'), 'runner': Path(native.__file__)})
if __name__ == '__main__':
    main()
