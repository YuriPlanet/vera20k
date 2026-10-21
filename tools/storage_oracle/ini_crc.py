"""Original CCINIClass::GetCRC over physically loaded INI files.

Each control loads one file through the original INI constructor 535B30,
CCFileClass 4739F0 and ReadCCFile 4741F0 with the digest/comments arguments the
game passes for that file, then calls GetCRC 476D80 and records its EAX, the
SHA-1 latch +0x40 and digest +0x41. When GetCRC hashes (latch clear), every
SHAPipe::Put 69E9B0 call SaveToPipe 526470 makes on GetCRC's pipe is observed
read-only, so the serialized stream is recorded exactly.

Retail controls read the checkout's machine-local ``ini/`` (case-insensitive)
and record only file name, byte length and native results; synthetic controls
store their bytes inline. A separate run executes the original composite
679D90 (rules + MPModes ids 1..9 + COOPCAMPMD.INI) on fresh native loads.

    python -m tools.storage_oracle.ini_crc [--check | --write]
"""
from pathlib import Path
import hashlib
import struct

ROOT = Path(__file__).resolve().parents[2]
INI_DIR = ROOT / "ini"

GET_CRC, READ_CC_FILE, SAVE_TO_PIPE, SHA_PUT = 0x476D80, 0x4741F0, 0x526470, 0x69E9B0
GET_CRC_SAVE_CALL, GET_CRC_SAVE_RETURN = 0x476DDA, 0x476DDF
RULES_COMPOSITE, ART_CRC, AI_CRC = 0x679D90, 0x679EC0, 0x679ED0

MODE_IDS = range(1, 10)
SYNTHETIC = [
    ("empty_file", b""),
    ("missing_file", None),
    ("single_entry", b"[A]\nK=V\n"),
    ("duplicate_key", b"[A]\nK=1\nK=2\nL=3\n"),
    ("duplicate_section_both_nonempty", b"[A]\nK=1\n[B]\nX=9\n[A]\nM=2\nK=3\n"),
    ("empty_section_then_nonempty", b"[A]\n[B]\nX=9\n[A]\nK=1\n"),
    ("empty_section_only", b"[A]\n[B]\nX=1\n"),
    ("spacing_comments_blank_lines",
     b"; lead comment\n\n[A] \n  K  =  V ; trailing\nL=W;x\n\n; mid\nM = \n;N=1\n"),
    ("key_without_equals", b"[A]\nNoEquals\nK=V\n=Empty\n"),
    ("value_whitespace_and_tabs", b"[A]\n\tK\t=\t V V \t\nL=  x  \n"),
    ("header_trailing_text", b"[A]junk\nK=V\n[B ] more\nX=1\n"),
    ("crlf_and_case", b"[a]\r\nk=v\r\n[A]\r\nK=V\r\n"),
    ("value_trailing_whitespace",
     b"[A]\nT1=x  \nT2=x   \nT3=x\t\t\nT4=x \t \nT5=x \r\nT6=x\t\nT7=y  "),
    ("leading_space_bounds_trailing_trim", b"[A]\nK=  x  \nL=   y\t\t\t\t\nM= z \nN=w\n"),
    ("unterminated_last_line", b"[A]\nK=V\nL=W"),
    ("unterminated_last_header", b"[A]\nK=V\n[B]\nX=1\n[C]"),
    ("value_edges", b"  [C]\nE1=;c\nE2=a=b\nE3= ;\n E4 = q;r ;s\n[D]\nZ\t= 1\n"),
]


def retail_dir():
    if not INI_DIR.is_dir():
        raise FileNotFoundError(f"Retail INI directory {INI_DIR} is missing (python -m tools.prepare_checkout)")
    return {path.name.upper(): path for path in INI_DIR.iterdir() if path.is_file()}


def signed(value):
    return struct.unpack("<i", struct.pack("<I", value & 0xFFFFFFFF))[0]


class Inputs:
    """Declared physical files only; None is an absent file."""

    def __init__(self, files):
        self.files = files

    def lookup(self, name):
        key = name.upper()
        if key not in self.files:
            raise ValueError(("Undeclared INI CRC input", name))
        return self.files[key]


def observe_stream(m):
    """Capture SaveToPipe output to GetCRC's own SHA pipe; no state is written."""
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
    state = dict(pipe=None, chunks=[])

    def hook(u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == GET_CRC_SAVE_CALL:
            state["pipe"] = m.read32(sp)
        elif pc == GET_CRC_SAVE_RETURN:
            state["pipe"] = None
        elif pc == SHA_PUT and state["pipe"] is not None and u.reg_read(UC_X86_REG_ECX) == state["pipe"]:
            buffer, length = m.read32(sp + 4), m.read32(sp + 8)
            state["chunks"].append(bytes(u.mem_read(buffer, length)))

    handles = [m.u.hook_add(UC_HOOK_CODE, hook, begin=address, end=address)
               for address in (GET_CRC_SAVE_CALL, GET_CRC_SAVE_RETURN, SHA_PUT)]
    return state, handles


def get_crc(m, ini):
    state, handles = observe_stream(m)
    try:
        latch_before = m.u.mem_read(ini + 0x40, 1)[0]
        m.phase = "get_crc"
        value = m.invoke(GET_CRC, ini) & 0xFFFFFFFF
    finally:
        for handle in handles:
            m.u.hook_del(handle)
    digest = bytes(m.u.mem_read(ini + 0x41, 20))
    stream = b"".join(state["chunks"]) if latch_before == 0 else None
    return dict(latch_before=latch_before, latch_after=m.u.mem_read(ini + 0x40, 1)[0],
                crc_unsigned=value, crc_signed=signed(value), digest_hex=digest.hex(),
                stream=stream, put_calls=len(state["chunks"]))


def load_and_crc(m, name, digest, comments):
    from unicorn.x86_const import UC_X86_REG_EAX
    ini, _ = m.load_physical_ini(name, digest=digest, comments=comments, require=False)
    returned = m.u.reg_read(UC_X86_REG_EAX)
    loaded = dict(read_cc_file=signed(returned), section_count=m.read32(ini + 0x2C),
                  latch_after_load=m.u.mem_read(ini + 0x40, 1)[0],
                  digest_after_load_hex=bytes(m.u.mem_read(ini + 0x41, 20)).hex())
    return ini, dict(loaded, **get_crc(m, ini))


# Section literals LoadAllGameModes passes to LoadGameModeFromINI 5D7590
# (pushes at 0x5D7D3C..0x5D7E24).
MODE_SECTION_LITERALS = (0x830C00, 0x830BF4, 0x830BEC, 0x830BE4, 0x830BD8, 0x830BCC)
COOPCAMP_LITERAL = 0x81F3C0  # 679E02, CCFileClass name in 679D90


def mode_files(m):
    """MPModesMD id -> file token through the original physical reader.

    ReadString 528A10 runs on the native MPModesMD load; the comma split of the
    returned value (third token, trimmed) stands in for Wstring getToken.
    """
    ini, _ = m.load_physical_ini("MPMODESMD.INI")
    rows = {}
    for literal in MODE_SECTION_LITERALS:
        section = m.string(literal)
        for mode in MODE_IDS:
            value, length = m.string_read(ini, section, str(mode))
            if length > 0:
                assert mode not in rows, mode
                rows[mode] = dict(section=section, file=value.split(",")[2].strip())
    assert sorted(rows) == list(MODE_IDS), rows
    return rows


def stream_facts(stream, digest_hex):
    return dict(stream_bytes=len(stream), put_stream_sha1_is_digest=hashlib.sha1(stream).hexdigest() == digest_hex)


def parabomb(raw, stream):
    """Structural facts about ARTMD's duplicate [PARABOMB]; no retail bytes."""
    def sections(lines):
        rows, current = [], None
        for line in lines:
            text = line.strip()
            if text.startswith(b"[") and b"]" in text:
                current = None
                if text[1:text.index(b"]")] == b"PARABOMB":
                    current = dict(entry_lines=0); rows.append(current)
            elif current is not None and text and not text.startswith(b";") and b"=" in text:
                current["entry_lines"] += 1
        return rows
    return dict(source_headers=sections(raw.splitlines()), native_stream_headers=sections(stream.splitlines()))


def generate():
    from tools.rules_oracle.weapon_speed_order import retail_history
    files = retail_dir()

    def raw(name):
        path = files.get(name.upper())
        if path is None:
            raise FileNotFoundError(f"Retail {name} is missing from {INI_DIR}")
        return path.read_bytes()

    declared = {"MPMODESMD.INI": raw("MPMODESMD.INI")}
    m = retail_history(Inputs(declared))
    modes = mode_files(m)
    m.close()

    retail = [("RULESMD.INI", 0, 0), ("ARTMD.INI", 0, 0), ("AIMD.INI", 1, 0),
              *((modes[mode]["file"], 0, 0) for mode in MODE_IDS), ("COOPCAMPMD.INI", 0, 0)]
    contrast = [("AIMD.INI", 0, 0)]
    declared = {name.upper(): raw(name) for name, _, _ in retail}
    synthetic_names = {name: f"CTRL{index:02d}.INI" for index, (name, _) in enumerate(SYNTHETIC)}
    declared.update({synthetic_names[name]: body for name, body in SYNTHETIC})
    m = retail_history(Inputs(declared))
    retail_rows = []
    for name, digest, comments in retail + contrast:
        _, row = load_and_crc(m, name, digest, comments)
        stream = row.pop("stream")
        if stream is not None:
            row.update(stream_facts(stream, row["digest_hex"]))
        if name == "ARTMD.INI":
            row["parabomb"] = parabomb(declared[name.upper()], stream)
        retail_rows.append(dict(file=name, bytes=len(declared[name.upper()]), digest_argument=digest,
                                comments_argument=comments, game_load=(name, digest, comments) in retail, **row))
    synthetic_rows = []
    for name, body in SYNTHETIC:
        _, row = load_and_crc(m, synthetic_names[name], 0, 0)
        stream = row.pop("stream")
        synthetic_rows.append(dict(name=name, file=synthetic_names[name], absent=body is None,
                                   input=None if body is None else body.decode("ascii"),
                                   stream=stream.decode("ascii"), **row,
                                   **stream_facts(stream, row["digest_hex"])))
    m.check_code()
    m.close()
    return dict(schema_version=1, modes={str(k): v for k, v in modes.items()},
                retail=retail_rows, synthetic=synthetic_rows, composite=composite(modes, raw))


def composite(modes, raw):
    """Original 679D90/679EC0/679ED0 on fresh native loads into the game's owners."""
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_EAX
    from tools.rules_oracle.weapon_speed_order import retail_history
    from tools.spatial_oracle.building_body_rules import dwords
    names = ["RULESMD.INI", "ARTMD.INI", "AIMD.INI", "COOPCAMPMD.INI",
             *(modes[mode]["file"] for mode in MODE_IDS)]
    m = retail_history(Inputs({name.upper(): raw(name) for name in names}))
    rules, _ = m.load_physical_ini("RULESMD.INI")
    m.u.mem_write(0x887048, dwords(rules))
    m.load_physical_ini("ARTMD.INI", address=0x887180)          # 52D053 / 679F0D
    m.load_physical_ini("AIMD.INI", address=0x887128, digest=1, require=False)  # 52D378
    ai_load = m.u.reg_read(UC_X86_REG_EAX)
    # Seeded MultiplayerGameMode registry (DynamicVector at ABFDA0): id +28,
    # file Wstring +2C, cached INI +38 left null so original 5D67B0 loads it.
    items = m.alloc(4 * len(MODE_IDS))
    for index, mode in enumerate(MODE_IDS):
        obj = m.alloc(0x40)
        m.u.mem_write(obj, bytes(0x40))
        m.u.mem_write(obj + 0x28, dwords(mode, m.cstring(modes[mode]["file"])))
        m.u.mem_write(items + 4 * index, dwords(obj))
    m.u.mem_write(0xABFD70, bytes([m.u.mem_read(0xABFD70, 1)[0] | 1]))
    m.u.mem_write(0xABFDA0, dwords(0x7EED20, items, len(MODE_IDS)) + bytes([1, 0, 0, 0]) +
                  dwords(len(MODE_IDS), 10))
    assert m.u.mem_read(0xB053B8, 1)[0] == 0
    parts = []

    def hook(u, pc, _size, _data):
        parts.append(dict(at=f"{pc:08X}", eax=u.reg_read(UC_X86_REG_EAX)))
    handles = [m.u.hook_add(UC_HOOK_CODE, hook, begin=a, end=a) for a in (0x679DA5, 0x679DD2, 0x679E4D)]
    m.phase = "rules_composite"
    rules_value = m.invoke(RULES_COMPOSITE, 0) & 0xFFFFFFFF
    for handle in handles:
        m.u.hook_del(handle)
    m.phase = "art_crc"
    art = m.invoke(ART_CRC, 0) & 0xFFFFFFFF
    m.phase = "ai_crc"
    ai = m.invoke(AI_CRC, 0) & 0xFFFFFFFF
    m.check_code()
    m.close()
    labels = {"00679DA5": "rules", "00679DD2": "mode", "00679E4D": "coopcamp"}
    components = [dict(component=labels[p["at"]], crc_unsigned=p["eax"] & 0xFFFFFFFF,
                       crc_signed=signed(p["eax"])) for p in parts]
    for row, mode in zip([c for c in components if c["component"] == "mode"], MODE_IDS):
        row["mode"] = mode
    xor = rules_value ^ ai ^ art
    return dict(ai_read_cc_file=signed(ai_load), components=components,
                rules=dict(unsigned=rules_value, signed=signed(rules_value)),
                art=dict(unsigned=art, signed=signed(art)), ai=dict(unsigned=ai, signed=signed(ai)),
                rules_xor_ai_xor_art=dict(unsigned=xor, signed=signed(xor)))


def metadata():
    from tools.native_oracle import provenance
    return provenance(
        scope="Original CCINIClass::GetCRC 476D80 over physically loaded INI files: retail RULESMD/ARTMD/AIMD, "
              "the nine MPModesMD mode INIs and COOPCAMPMD (plus an AIMD digest=0 contrast), seventeen synthetic "
              "parser/serializer controls with their exact SaveToPipe streams, and one original Rules/Art/AI "
              "composite run (679D90, 679EC0, 679ED0)",
        assumptions=[
            "Each control runs the original INI constructor 535B30 (latch +40 cleared, vtable 7E1AF4, as the "
            "allocation at 52CF56..52CF60), CCFileClass 4739F0 and ReadCCFile 4741F0 with the game's arguments: "
            "RULESMD (0,0) at 52CF73, ARTMD (0,0) at 52D053/679F0D, AIMD (1,0) at 52D378, mode INIs (0,0) at "
            "5D685A, COOPCAMPMD (0,0) at 679E3B. GetCRC then runs unchanged; EAX is its CRCEngine 4A1DE0 result.",
            "Retail bytes are read from the checkout's machine-local ini/ (case-insensitive) and are not stored; "
            "only file name, byte length and native outputs are. Mode file names come from original ReadString "
            "528A10 on the native MPMODESMD load for the six LoadAllGameModes section literals; the third comma "
            "token is split and trimmed in Python in place of Wstring getToken.",
            "SaveToPipe streams are every SHAPipe::Put 69E9B0 call on GetCRC's own pipe between 476DDA and "
            "476DDF, observed read-only. put_stream_sha1_is_digest is a capture-completeness check only.",
            "Composite: [887048] holds a fresh native RULESMD load, ART/AI load into their static owners 887180/"
            "887128. The MultiplayerGameMode registry at ABFDA0 is seeded with nine id-sorted objects carrying id "
            "+28, file name Wstring +2C and a null cached INI +38, so original 5D67B0 constructs, checks Exists and "
            "loads each mode file. The native StringTable/Initializer construction of those objects does not run. "
            "COOPCAMPMD is loaded by 679D90 itself with its latch B053B8 clear. Later in-game edits to these INI "
            "objects before the CRC, map/LANGRULE layering and the network exchange are outside coverage.",
        ],
        substitutions=["Existing RetailRulesHistory/BulletReader Windows/file/heap/CRT/TLS platform transport and "
                       "declared physical file bytes only; no INI parser, SaveToPipe, SHA, CRCEngine or composite "
                       "body is replaced. Original mapped code bytes are checked unchanged."],
        entry_points=dict(ini_ctor=0x535B30, file_ctor=0x4739F0, read_cc_file=READ_CC_FILE,
                          load_from_straw=0x525A60, save_to_pipe=SAVE_TO_PIPE, sha_put=SHA_PUT,
                          get_crc=GET_CRC, crc_engine=0x4A1DE0, read_string=0x528A10,
                          mode_ini=0x5D67B0, rules_composite=RULES_COMPOSITE, art_crc=ART_CRC, ai_crc=AI_CRC))


if __name__ == "__main__":
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata,
                   source_paths={"producer": Path(__file__),
                                 "physical_rules_owner": ROOT / "tools/rules_oracle/weapon_speed_order.py"})
