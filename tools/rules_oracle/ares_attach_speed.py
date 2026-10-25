"""Four Ares attached-speed histories, using the existing AttachEffectProbe.

Native constructors/readers, aggregate, crate hooks and Foot getter execute;
this producer contains no alternate gameplay arithmetic or VM.
"""
import argparse
import os
from pathlib import Path
import struct

from tools import native_oracle as native
from tools.native_oracle import finish_vectors, provenance
from tools.rules_oracle.ares_attach_effect import AttachEffectProbe
from tools.rules_oracle.ares_emp import ARES_SHA256, identified_bytes, signed


class SpeedProbe(AttachEffectProbe):
    def descriptor(self, extension, warhead, duration, speed=None, discard=False):
        self.call(0x10051890, warhead, ecx=extension)
        ini = self.alloc(4)
        self.write(ini, 0x887048)
        name = self.string(warhead + 0x24)
        self.reads = {(name, 'AttachEffect.Duration'): str(duration)}
        if speed is not None:
            self.reads[name, 'AttachEffect.SpeedMultiplier'] = str(speed)
        if discard:
            self.reads[name, 'AttachEffect.DiscardOnEntry'] = 'yes'
        self.call(0x10058A50, ini, ecx=extension + 0x88)
        self.call(0x100523E0, self.text('3%'), ecx=self.read(extension + 0x24))
        self.u.mem_write(warhead + 0x124, struct.pack('<f', .7))
        return dict(duration=signed(self.read(extension + 0x8C)),
                    speed_bits=bytes(self.u.mem_read(extension + 0xB8, 8)).hex(),
                    discard=bool(self.u.mem_read(extension + 0x92, 1)[0]))

    def prepare_speed(self):
        self.prepare(False)
        # Original Unit/UnitType vtables and GetCurrentSpeed backing. Drive's
        # constructor/Process is outside this numeric control.
        self.write(self.actor, 0x7F5C70)
        self.write(self.typ, 0x7F6218)
        self.write(self.actor + 0x14, 5)
        self.write(self.actor + 0x6C4, self.typ)
        self.write(self.typ + 0x678, 10)
        self.write(self.actor + 0x6CC, -1)
        self.u.mem_write(self.actor + 0x578, struct.pack('<d', 1.0))
        house_type = self.alloc(0x600)
        self.write(self.read(self.actor + 0x21C) + 0x34, house_type)
        for offset in (0x128, 0x12C, 0x130):
            self.u.mem_write(house_type + offset, struct.pack('<f', 1.0))
        self.selected = self.descriptor(self.whext, self.wh, 60, .2, True)
        self.call(0x100460A0, ecx=self.runtime)
        self.constructed = self.uninitialized = 0
        return self

    def observe_speed(self, action):
        self.check_code()
        return dict(action=action,
                    crate_base_bits=bytes(self.u.mem_read(self.runtime + 0x90, 8)).hex(),
                    derived_speed_bits=bytes(self.u.mem_read(self.actor + 0x580, 8)).hex(),
                    foot_current_speed=signed(self.call(0x4DB1A0, ecx=self.actor)),
                    remaining=self.observation()['remaining'])

    def hit_speed(self, extension=None):
        self.call(0x10052700, self.actor + 0x9C, self.source_house,
                  ecx=self.whext if extension is None else extension)

    def speed_crate(self):
        frame, native_sp = self.alloc(0x80), self.alloc(0x60)
        self.write(frame + 8, self.actor)
        self.write(frame + 0x14, native_sp)
        self.u.mem_write(native_sp + 0x20, struct.pack('<d', 1.2))
        picker = self.call(0x1005F9D0, frame)
        recipient = self.call(0x1005FA00, frame)
        return dict(picker_continuation=f'{picker:08X}',
                    recipient_continuation=f'{recipient:08X}')

    def visits(self, count):
        for _ in range(count):
            self.call(0x10058E00, self.runtime, ecx=self.runtime + 0x20)


def generate(ares):
    images = ares, native.image_bytes()
    controls = []
    p = SpeedProbe(images).prepare_speed()
    steps = [p.observe_speed('prepared')]
    p.hit_speed()
    steps.append(p.observe_speed('first_hit'))
    p.visits(1)
    steps.append(p.observe_speed('tick_1'))
    p.hit_speed()
    steps.append(p.observe_speed('noncumulative_reapply'))
    p.visits(60)
    steps.append(p.observe_speed('sixty_visits_expire'))
    controls.append(dict(name='base_one_reapply_and_expire', descriptor=p.selected, steps=steps))

    p = SpeedProbe(images).prepare_speed()
    steps = [p.observe_speed('prepared')]
    crate = p.speed_crate()
    steps.append(dict(**p.observe_speed('speed_crate_before_hit'), crate=crate))
    p.hit_speed()
    steps.append(p.observe_speed('hit_after_crate'))
    p.visits(60)
    steps.append(p.observe_speed('sixty_visits_expire'))
    controls.append(dict(name='existing_speed_crate_base', descriptor=p.selected, steps=steps))

    p = SpeedProbe(images).prepare_speed()
    steps = [p.observe_speed('prepared')]
    p.hit_speed()
    steps.append(p.observe_speed('first_hit'))
    for action in ('accept_crate_while_slowed', 'reject_duplicate_crate'):
        crate = p.speed_crate()
        steps.append(dict(**p.observe_speed(action), crate=crate))
    removed = p.call(0x100588E0, ecx=p.runtime + 0x20) & 255
    if removed:
        p.call(0x100460A0, ecx=p.runtime)
    steps.append(p.observe_speed('discard_on_entry_and_aggregate'))
    controls.append(dict(name='crate_during_slow_and_discard', descriptor=p.selected, steps=steps))

    p = SpeedProbe(images).prepare_speed()
    steps = [p.observe_speed('prepared')]
    other_wh, other_ext = p.alloc(0x300), p.alloc(0x200)
    p.write(other_wh + 0x1CC, other_ext)
    p.u.mem_write(other_wh + 0x24, b'OTHER_NEUTRAL\0')
    p.write(other_ext, other_wh)
    p.u.mem_write(other_wh + 0x179, b'\1')
    p.descriptor(other_ext, other_wh, 5)
    p.hit_speed(other_ext)
    steps.append(p.observe_speed('neutral_hit'))
    p.hit_speed()
    steps.append(p.observe_speed('slow_hit_with_neutral'))
    p.visits(5)
    steps.append(p.observe_speed('five_visits_neutral_expired'))
    p.visits(55)
    steps.append(p.observe_speed('sixty_total_visits_all_expired'))
    controls.append(dict(name='neutral_effect_coexists', descriptor=p.selected, steps=steps))
    return dict(schema_version=1, controls=controls)


def metadata():
    result = provenance(
        scope='Four native-executed Ares 3.0 attached-speed histories with actual Foot getter and crate hooks.',
        assumptions=[
            'Reuses AttachEffectProbe/EmpProbe/Probe, native PE mapping, CRT scanner, heap and declared collector/world services. No second VM or gameplay algorithm.',
            'Actual WH constructor/reader takes synthetic Duration60, SpeedMultiplier0.2, DiscardOnEntryyes, verse3%, CellSpread0.7. Animation is null. Other neutral type uses Duration5.',
            'Unit/UnitType vtables, raw type speed10, rookie, House speed1, Foot fraction1 and CTF flag-1 are explicit inputs. Original FootGetCurrentSpeed4DB1A0, House bonus and ftol execute. No retail type-speed claim.',
            'Actual aggregate100460A0, attach/update/Remove-vector and Ares Speed1/Speed2 hooks1005F9D0/1005FA00 execute. Remove conditionally aggregates as native1004B197. Full boarding, Drive constructor/Process, Bullet producer, physical INI scenario and rendering are excluded.',
            'Visits are admitted object visits, not global frames. Stored bits are little-endian binary64; values come from native execution, not hand arithmetic or prior Rust.',
            'Original code and three seeded RNG states are guarded by the existing probe; no RNG/Detach guard reached. Supplied world/animation services retain their existing coverage boundaries.',
        ],
        substitutions=['Existing AttachEffectProbe services only; no numeric or result injection.'],
        entry_points=dict(reader=0x10058A50, attach=0x10052700, aggregate=0x100460A0,
                          update=0x10058E00, remove=0x100588E0, picker=0x1005F9D0,
                          recipient=0x1005FA00, getter=0x4DB1A0))
    result.update(ares_sha256=ARES_SHA256, control_count=4, actual_execution=True,
                  execution_batches='The four histories were already executed together; this generic producer reproduces that batch without rerunning the six neutral histories.',
                  command='python -m tools.rules_oracle.ares_attach_speed --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --check')
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
    finish_vectors(lambda: generate(ares), Path(__file__).with_suffix('.json'),
                   provenance=metadata, argv=remaining,
                   source_paths={'producer': Path(__file__),
                                 'attachment_owner': Path(__file__).with_name('ares_attach_effect.py'),
                                 'emp_owner': Path(__file__).with_name('ares_emp.py'),
                                 'vm_owner': Path(__file__).with_name('ares_prerequisites.py'),
                                 'runner': Path(native.__file__)})


if __name__ == '__main__':
    main()
