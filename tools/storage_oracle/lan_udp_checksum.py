"""Execute bounded LAN transport primitives without OS hooks.

Payloads and response-time fields are explicit transport inputs. Only original
7B1FD0/64DE80,48BA10 and5414C0 supply the outputs; no Python behavior model.
"""

from pathlib import Path
import struct

from tools.native_oracle import SCRATCH, call, finish_vectors, provenance


def generate():
    controls = [("empty", b"")]
    pattern = bytes.fromhex("ff00807fefcdab89feffffff78563412")
    controls += [(f"prefix_{length}", pattern[:length]) for length in range(1, 17)]
    controls += [("word_carry", bytes.fromhex("ffffffffffffffffffffffff")),
                 ("private_payload_limit", bytes(range(256)) * 2 + bytes(range(46)))]
    cases = []
    for name, payload in controls:
        # Nonzero bytes beyond the payload detect incorrect final-word masking.
        block = bytearray(bytes([0xA5]) * (0x20 + len(payload) + 4))
        struct.pack_into("<I", block, 0x10, len(payload))
        block[0x20:0x20 + len(payload)] = payload
        result = call(0x7B1FD0, stack_args=[SCRATCH], writes={SCRATCH: bytes(block)},
                      dumps={"block": (SCRATCH, len(block))},
                      timeout_instr=10_000, timeout_us=1_000_000,
                      required_addresses=() if not payload else (0x64DE80,))
        observed = bytes.fromhex(result["dumps"]["block"])
        if observed[:0x1C] != block[:0x1C] or observed[0x20:] != block[0x20:]:
            raise AssertionError(f"unexpected packet mutation in {name}")
        cases.append(dict(name=name, payload=payload.hex(),
                          checksum=struct.unpack_from("<I", observed, 0x1C)[0]))
    response_time_cases = []
    for name, total, count, mean, elapsed in [
        ("first_max", 0, 0, 0, 0xFFFFFFFF),
        ("division_wrap", 0xFFFFFFF0, 1, 0xFFFFFFF0, 0x30),
        ("sample_256", 2550, 255, 10, 11),
        ("tail_127", 2560, 256, 10, 137),
        ("tail_128", 2560, 256, 10, 138),
        ("logical_shift", 0x80000000, 256, 0x800000, 0x800000),
    ]:
        state = bytearray(0x30)
        struct.pack_into("<III", state, 0x14, total, count, mean)
        result = call(0x48BA10, ecx=SCRATCH, stack_args=[elapsed],
                      writes={SCRATCH: bytes(state)}, dumps={"state": (SCRATCH, len(state))},
                      required_addresses=(0x48BA60,), timeout_instr=1000, timeout_us=100_000)
        observed = bytes.fromhex(result["dumps"]["state"])
        # The original also updates its diagnostic peak at+20. The consumer
        # uses only the authoritative sum/count/mean fields at+14/+18/+1C.
        if observed[:0x14] != state[:0x14] or observed[0x24:] != state[0x24:]:
            raise AssertionError(f"unexpected response storage mutation in {name}")
        response_time_cases.append(dict(name=name, before=[total, count, mean],
                                        elapsed=elapsed,
                                        after=list(struct.unpack_from("<III", observed, 0x14))))

    # Both initialized flags are set; there are no private peers. This reaches
    # the actual early count check before the broadcast loop, without callbacks.
    manager = bytearray(0xB0)
    manager[4:6] = b"\x01\x01"
    result = call(0x5414C0, ecx=SCRATCH,
                  stack_args=[SCRATCH + 0x1000, 0, 1, 0xFFFFFFFF],
                  writes={SCRATCH: bytes(manager)}, dumps={"manager": (SCRATCH, len(manager))},
                  required_addresses=(0x5414E1, 0x5415DF),
                  timeout_instr=1000, timeout_us=100_000)
    if bytes.fromhex(result["dumps"]["manager"]) != manager:
        raise AssertionError("zero-peer send mutated manager state")
    return dict(source="unicorn/gamemd.exe", cases=cases,
                response_time_cases=response_time_cases,
                empty_peer_broadcast=dict(peer_count=0, returned=result["eax"]))


def metadata():
    return provenance(
        scope="Whole UDP packet checksum7B1FD0/64DE80, response update48BA10 and zero-peer private send5414C0",
        assumptions=[
            "Explicit payload byte vectors are placed in packet+20, length at+10. "
            "Packet storage and bytes after each payload startA5; output is read at+1C. "
            "Empty input, all four byte remainders, high bits, carry and558-byte input "
            "are bounded numerical controls, not live UDP or reliability observations.",
            "Only checksum storage may change. Full native return and, for nonempty "
            "input, execution of64DE80 are required. There is no Python checksum model.",
            "Response controls supply CommBuffer sum/count/mean at+14/+18/+1C and "
            "one elapsed wall-clock value; original48BA10 produces all retained outputs. "
            "Samples255/256, remainder7F/80, unsigned high bits and wrapping sums are "
            "bounded arithmetic inputs. Diagnostic peak+20 is allowed to change but "
            "is outside the Rust response consumer; other storage must remain unchanged.",
            "Private-send control supplies initialized flags+4/+5, peer count+44=0, "
            "zero payload length and destinationFFFFFFFF. Required instructions5414E1 "
            "and5415DF establish the early rejection; no connection callbacks run.",
        ],
        substitutions=[],
        entry_points=dict(checksum=0x7B1FD0, fold=0x64DE80,
                          response_time=0x48BA10, private_send=0x5414C0),
    )


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
