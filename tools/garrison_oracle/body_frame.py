"""Execute stock YR GetCurrentFrame for completed garrison bodies.

Reverify from the repository root (Python needs Unicorn 2.1.4 and the original
executable configured through VERA20K_GAMEMD_EXE or RA2_DIR):
    python -m tools.garrison_oracle.body_frame --check
    cargo test -p vera20k --lib completed_garrison_body_frames_match_native_oracle
The first command checks native execution against body_frame.json without
writing; the Rust test checks the production selector against that same file.
body_frame.meta.json records assumptions and coverage.

Native evidence, checked in Ghidra 2026-09-10: project testProsjekt,
/gamemd.exe, x86:LE:32:default, image base 0x00400000.

BuildingClass+0x534 is the body animation state, NOT a health-derived damage
flag. Earlier garrison reports missed its writers and incorrectly concluded
that healthy occupied buildings retain empty frame 0:
- Constructor 0x0043B740 initializes +0x534 to -1.
- GrandOpening 0x00447780 stages its argument in +0x538, writing +0x534 at
  0x004477C1 when uninitialized, starting state 0, or in map-editor mode.
  That state indexes the type's animation triples at +0xF04, stride 12.
- Guard pushes 1 at 0x0044995D and calls GrandOpening at 0x00449961.
  Construction selects 0 at 0x00449B36 and requests 1 at 0x00449AC9 on completion.
- GetCurrentFrame 0x0043EF90 checks +0x534 at 0x0043EFC6. Nonzero reaches
  CanBeOccupied at 0x0043F02C: occupants select 2; damage adds 1 at ConditionRed,
  or ConditionYellow for TechLevel > 0. TechLevel -1 collapses 3 to 1.
  DrawBody calls GetCurrentFrame at 0x0043D7B5/0x0043D7CB/0x0043D8D4.
- SetDamagedState 0x00451EE0 uses a separate IsDamaged flag for overlay variants;
  it does not define +0x534.

Production Rust: src/app/presentation/instances/shp.rs::building_frame_index.
Build-up/down branches precede completed body selection; the health-derived
flag remains for overlay variants. The atlas already requests body frames 0..3.

The 54 cases cover empty/occupied, TechLevel -1/0/5, and health around both
stock thresholds at strength 2000 (CABUNK01). Original virtual table and helper
instructions run without replacement. The fixture supplies initialized idle
building/type/rules data, not map loading, boarding or animation scheduling.
This is bounded frame-selection equivalence, not all-input or GPU parity.

Validation 2026-09-10: native --check and Rust comparison passed; release and
clippy passed with existing warnings. Full library suite: 8587 passed, 7 failed,
119 ignored. Four untouched capture tests reject macOS /var symlinks; three
assume Windows paths on Unix (independently reviewed as unrelated).
On Apple M4/Metal, a release CABUNK01/GI fixture showed reinforced occupied
artwork and restored plain concrete after D ejected the soldier; the user also
confirmed it. This was Rust visual smoke, not a native screenshot comparison.
The temporary fixture required a player construction yard to avoid immediate
match end and 1024x768 to avoid an unrelated 640x480 scissor validation error.
"""
from pathlib import Path
import struct
from tools.native_oracle import SCRATCH, call, finish_vectors, provenance

ENTRY = 0x0043EF90
BUILDING, TYPE, RULES = SCRATCH, SCRATCH + 0x1000, SCRATCH + 0x3000

def native_frame(health, occupants, tech_level):
    b = bytearray(0x720)
    t = bytearray(0x1800)
    r = bytearray(0x1800)
    for offset, value in [(0, 0x007E3EBC), (0x6c, health), (0x520, TYPE),
                          (0x534, 1), (0x694, occupants)]:
        struct.pack_into('<I', b, offset, value)
    struct.pack_into('<I', t, 0xa0, 2000)
    struct.pack_into('<i', t, 0x634, tech_level)
    t[0x157b] = 1
    struct.pack_into('<d', r, 0x1700, 0.5)
    struct.pack_into('<d', r, 0x1708, 0.25)
    return call(ENTRY, ecx=BUILDING, writes={BUILDING: bytes(b), TYPE: bytes(t),
        RULES: bytes(r), 0x008871E0: struct.pack('<I', RULES)},
        required_addresses=[0x0043F02C, 0x004581F0, 0x005F5C60],
        timeout_instr=1000)['eax']

def generate():
    return {'source': 'unicorn/gamemd.exe', 'function': hex(ENTRY), 'cases': [
        dict(health=h, occupants=o, tech_level=t, frame=native_frame(h,o,t))
        for t in [-1, 0, 5] for o in [0, 8]
        for h in [1, 499, 500, 501, 999, 1000, 1001, 1999, 2000]]}

if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='54 completed CanBeOccupied body-frame cases, strength 2000; not all inputs or GPU pixels',
            assumptions=['Building state 1 from native Guard/GrandOpening 0x44995D/0x447780',
                         'Original BuildingClass vtable and type/health/occupant accessors',
                         'Supplied object/type/rules data; stock ConditionYellow=.5 and ConditionRed=.25',
                         'No laser fence, firestorm or gate flags; fresh emulator per case'],
            substitutions=[], entry_points={'GetCurrentFrame': ENTRY}))
