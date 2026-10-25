"""Six bounded Ares 3.0 warhead AttachEffect lifecycle histories.

Synthetic registered names only. Reuses the existing EMP/Probe VM, native
readers and collector; animation/world services remain declared boundaries.
"""
import argparse
import os
from pathlib import Path
import struct

from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP,
)

from tools import native_oracle as native
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.rules_oracle.ares_emp import EmpProbe, signed
from tools.rules_oracle.ares_prerequisites import ARES_SHA256, Probe, identified_bytes
from tools.input_oracle.fast_scroll import return_from_sink


class AttachEffectProbe(EmpProbe):
    """Add animation service observations, not another execution/world owner."""

    def __init__(self, images):
        self.constructed = 0
        self.uninitialized = 0
        super().__init__(images)

    def hook(self, u, pc, size, data):
        if self.world:
            sp = u.reg_read(UC_X86_REG_ESP)
            arg = lambda i: self.read(sp + 4 + i * 4)
            owner = u.reg_read(UC_X86_REG_ECX)
            if pc == 0x10001B60:
                # Preserve the original allocator wrapper. Only its pure heap
                # leaf is supplied, using Probe's existing arena.
                return Probe.hook(self, u, pc, size, data)
            if pc == 0x10001B50:
                return_from_sink(u, 0, self.alloc(arg(0)))
                return
            if pc == 0x1000AAE0:
                self.constructed += 1
                self.animations.append(owner)
                self.write(owner, self.anim_vtable)
                self.write(owner + 0xC8, arg(0))
                u.mem_write(owner + 0x9C, bytes(u.mem_read(arg(1), 12)))
                return_from_sink(u, 28, owner)
                return
            if pc == 0x30000410:
                self.uninitialized += 1
                return_from_sink(u, 0, u.reg_read(UC_X86_REG_EAX))
                return
        return super().hook(u, pc, size, data)

    def prepare(self, discard, anim_reset_on_reapply=False):
        self.subject(dict(actor_kind=6, type_kind=7, selected=False,
                          deactivated=True, affects_allies=True))
        self.write(self.actor + 0x14, 1)
        self.write(self.actor + 0x10, 7)
        self.write(self.actor + 0x6C, 1000)
        self.u.mem_write(self.actor + 0x90, b'\1')
        self.u.mem_write(self.actor + 0x9C, struct.pack('<iii', 2688, 2688, 0))
        self.write(self.actor + 0x520, self.typ)
        self.runtime = self.read(self.actor + 0x154)
        self.write(self.runtime, self.actor)
        self.u.reg_write(UC_X86_REG_ESI, self.runtime)
        self.u.reg_write(UC_X86_REG_ESP, 0x2000E000)
        # Actual TechnoExt floating baseline initializer; no calculated result.
        run_checked(self.u, 0x10042D98, 0x10042DC8, count=100)
        self.check_code()
        self.anim_vtable = self.alloc(0x100)
        self.write(self.anim_vtable + 0xF8, 0x30000410)
        for vector in (0x8B4150, 0xB0F670):
            self.write(vector + 4, self.alloc(64))
            self.write(vector + 8, 16)
            self.write(vector + 0x10, 0)
        self.anim_type = self.alloc(0x400)
        self.call(0x427530, self.text('VISUAL'), ecx=self.anim_type)
        self.wh = self.read(self.whext)
        self.u.mem_write(self.wh + 0x24, b'AE_TEST\0')
        self.call(0x10051890, self.wh, ecx=self.whext)
        ini = self.alloc(4)
        self.write(ini, 0x887048)
        self.reads = {('AE_TEST', 'AttachEffect.Duration'): '5',
                      ('AE_TEST', 'AttachEffect.Animation'): 'VISUAL'}
        if discard:
            self.reads[('AE_TEST', 'AttachEffect.DiscardOnEntry')] = 'yes'
        if anim_reset_on_reapply:
            self.reads[('AE_TEST', 'AttachEffect.AnimResetOnReapply')] = 'yes'
        self.call(0x10058A50, ini, ecx=self.whext + 0x88)
        assert self.read(self.whext + 0x94) == self.anim_type
        self.call(0x100523E0, self.text('3%'), ecx=self.read(self.whext + 0x24))
        self.u.mem_write(self.wh + 0x124, struct.pack('<f', .7))
        table = self.alloc(0x100000)
        self.write(0x87F924, table)
        self.write(self.cell + 0xE4, self.actor)
        self.write(table + (10 + 10 * 512) * 4, self.cell)
        self.source_house = self.read(self.firer + 0x21C)
        self.call(0x100460A0, ecx=self.runtime)
        self.constructed = self.uninitialized = 0
        return self

    def observation(self):
        vector = self.runtime + 0x20
        begin, end = self.read(vector), self.read(vector + 4)
        assert end >= begin and (end - begin) % 16 == 0
        remaining, animation, source_retained, iterations = [], [], [], []
        for at in range(begin, end, 16):
            anim = self.read(at + 4)
            remaining.append(signed(self.read(at + 8)))
            animation.append(bool(anim))
            source_retained.append(self.read(at + 12) == self.source_house)
            iterations.append(self.u.mem_read(anim + 0x195, 1)[0] if anim else None)
        return dict(remaining=remaining, animations=animation,
                    owner_matches_source=source_retained, iterations=iterations,
                    hidden=bool(self.u.mem_read(vector + 0x10, 1)[0]),
                    ctor_count=self.constructed, uninit_count=self.uninitialized)

    def step(self, action):
        if action in ('hit', 'reapply_other_source'):
            house = self.source_house if action == 'hit' else self.read(self.actor + 0x21C)
            self.call(0x10052700, self.actor + 0x9C, house, ecx=self.whext)
        elif action == 'tick':
            self.call(0x10058E00, self.runtime, ecx=self.runtime + 0x20)
        elif action in ('limbo_on', 'limbo_off'):
            self.u.mem_write(self.actor + 0x81, b'\1' if action == 'limbo_on' else b'\0')
        elif action in ('cloak_on', 'cloak_off'):
            # Explicit Techno+220 state input: 1 cloaking, 0 uncloaked. The
            # native Create/update gates execute; the cloak producer does not.
            self.write(self.actor + 0x220, int(action == 'cloak_on'))
        elif action == 'pointer_expiry':
            frame = self.alloc(0x40)
            self.write(frame + 0x20, self.actor)
            self.write(frame + 0x24, self.animations[0])
            self.call(0x10046F20, frame)
        elif action == 'remove':
            self.call(0x100588E0, ecx=self.runtime + 0x20)
        elif action == 'dtor':
            self.call(0x10042C30, ecx=self.runtime + 0x20)
        elif action != 'prepared':
            raise ValueError(action)
        self.check_code()
        return dict(action=action, **self.observation())


def run_cases(ares, cases):
    images = (ares, native.image_bytes())
    histories = []
    for name, discard, reset, actions in cases:
        probe = AttachEffectProbe(images).prepare(discard, reset)
        histories.append(dict(name=name, discard_on_entry=discard,
                              anim_reset_on_reapply=reset,
                              steps=[probe.step(action) for action in actions]))
    return histories


def extra_controls(ares):
    """The two replacement regressions can be executed independently."""
    return run_cases(ares, [
        ('discard_then_hit_before_unhide', True, False,
         ['prepared', 'hit', 'remove', 'limbo_off', 'hit', 'tick', 'dtor']),
        ('reset_reapply_and_cloak_transition', False, True,
         ['prepared', 'hit', 'hit', 'cloak_on', 'hit', 'tick',
          'cloak_off', 'tick', 'dtor']),
    ])


def generate(ares):
    cases = [
        ('repeat_limbo_pause', False, False,
         ['prepared', 'hit', 'tick', 'reapply_other_source', 'limbo_on', 'tick',
          'limbo_off', 'tick', 'tick', 'tick', 'tick', 'tick']),
        ('external_animation_expiry', False, False,
         ['prepared', 'hit', 'pointer_expiry', 'tick', 'tick', 'tick', 'tick', 'tick']),
        ('remove_unlimbo_destruct', False, False,
         ['prepared', 'hit', 'remove', 'limbo_on', 'tick', 'limbo_off', 'tick', 'dtor']),
        ('discard_on_entry', True, False,
         ['prepared', 'hit', 'remove', 'limbo_on', 'tick', 'limbo_off', 'tick', 'dtor']),
    ]
    return dict(schema_version=1, controls=run_cases(ares, cases) + extra_controls(ares))


def metadata():
    result = provenance(
        scope='Six native-executed Ares 3.0 ordinary ground-warhead AttachEffect histories; not whole-game parity.',
        assumptions=[
            'Synthetic AE_TEST warhead and VISUAL AnimType use actual constructors, native registration/Find and AttachEffect reader. Duration5, verse3%, CellSpread0.7 and DiscardOnEntry are explicit inputs; no mod names, assets, extraction or physical INI loader.',
            'Original collector10076660, house gate10051B70, warhead attach10052700/10058520, update10058E00, pointer expiry10046F20/10058770, Remove-vector100588E0 and vector destructor10042C30/10059130 execute unchanged. Static active hook callers are Techno Remove6F6AC9 and DTOR6F4500; full hook/container lookup is excluded.',
            'Victim is an ordinary live independent ground Building with no Team/Temporal/transport manager, explicit deactivated input and identity stat modifiers. Existing world virtual queries and linked-cell/type/category/house backing are fixture inputs. Actual native floating-baseline initialization executes.',
            'AnimClass constructor, SetOwnerObject and virtual UnInit are supplied world services. Constructor retains allocated this/type/coordinate/vtable; it does not supply iteration255, attached source, counter or admission. Their internal RNG, rendering, lifetime and detach/callback cascades are excluded.',
            'ctor_count/uninit_count count calls at the supplied AnimConstructor/virtual UnInit service boundaries, not complete animation construction/destruction or memory retirement.',
            'Limbo byte changes and an externally expiring animation notification are explicit inputs. Only the Remove histories execute the actual Remove-vector callee. No whole boarding, unloading, death scheduler, parent Bullet/EMP or full campaign/mod parity is claimed.',
            'The two replacement controls add actual native AnimResetOnReapply reading and explicit Techno+220 cloak-state inputs1/0. Create100586E0 publishes a new pointer before old UnInit, then initializes its owner/iterations/source. Full StartCloaking/Uncloaking, cloak capability admission and their lifecycle/RNG producers are excluded.',
            'Actual Anim allocation wrapper executes; pure heap new/delete use existing Probe arena/services. FPCW0E7F and three exact native-seeded RNG states are inherited from EmpProbe; unchanged code/RNG states checked after every call, guarded RNG/Detach entry calls fail. No-draw claims exclude supplied animation services.',
        ],
        substitutions=['Existing EmpProbe/Probe PE mapping, arena, call runner, CRT thread, lexical INI transport and declared world services; no new VM, parser or gameplay implementation.'],
        entry_points=dict(warhead_constructor=0x10051890, reader=0x10058A50,
                          animation_type_constructor=0x427530, create=0x10052700,
                          attach=0x10058520, update=0x10058E00,
                          pointer_expiry=0x10046F20, remove_vector=0x100588E0,
                          destructor_vector=0x10042C30, animation_uninit=0x10059130))
    result.update(ares_sha256=ARES_SHA256, control_count=6,
                  executable='Yuri Revenge gamemd.exe preferred image base0x00400000',
                  actual_execution=True,
                  execution_batches='Four baseline histories originally executed; two replacement histories executed separately and appended without rerunning retained baselines. The full generator replays all six for subsequent --check.',
                  command='python -m tools.rules_oracle.ares_attach_effect --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --check')
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
                                 'vm_owner': Path(__file__).with_name('ares_prerequisites.py'),
                                 'emp_owner': Path(__file__).with_name('ares_emp.py'),
                                 'runner': Path(native.__file__)})


if __name__ == '__main__':
    main()
