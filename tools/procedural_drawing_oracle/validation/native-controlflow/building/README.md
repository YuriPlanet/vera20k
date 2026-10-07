# Building visual-character inspection (read-only)

Checked executable: Steam gamemd.exe SHA256 3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600. Saved packets use tools.native_inspect, Capstone 5.0.7. Static body/caller/data evidence only; no executed building parity claim.

Building vtable 7E3EBC+68 holds4544A0. Shared SHP705E24 invokes virtual+68 with(0,NULL), so rendering uses the Building override. Building vtable+2C holds459EC0; original body459EC0 is MOV EAX,6; RET. Thus7038D5 virtual RTTI followed by7038D8 CMP EAX,6 really is Building.

4544A7 reads Building+6ED. Zero delegates at4545B7..4545C3 to canonical703860 with unchanged two arguments. Preserve that one existing canonical owner. Nonzero takes independent branches and does not run canonical Type+C9A Invisible checks.

Signed-byte stage comparisons4544B5/B7 and4545A7/A9 return1 for positive1..5 (also negative signed-byte values), return2 for6..10. Positive11..127 reach visibility gates. Force argument nonzero requires nonnull explicit house and sensor query5657A0->4870D0 using physical Location+9C. Screen argument0 returns3 for owned+41A, screen sensor virtual+328, or null B73550; otherwise it requires mode globalA8B238 nonzero (the noncampaign branch), nonnull owner+21C/current HouseA83D4C and mutual4F9A50 alliance, yielding3; failure yields5. This mirrors the canonical fully-cloaked observation decision but the state source is distinct.

Building+328 resolves70D420. It derives location through actual virtual+48, maps with565730, queries4870D0 using current House+30, or returnsfalse without current House. This is not simply the stored anchor cell; the Building virtual+48 is its foundation-relative center447AC0 (this inspection did not execute that producer).

Canonical703860 checks Type+C9A and owned+41A before state/map-editor/RTTI6 early normal return. Therefore disabling canonical query for every Building would regress native Invisible behavior. With represented Building+6ED defaultzero, calling canonical703860 is the native delegating branch. Do not claim it reproduces nonzero override/lifecycle: src/render/radar_visibility.rs documents no Rust production writer for Building+6ED yet.

building-4544a0-call-candidates.json contains no decoded direct CALL/JMP references; virtual dispatch is established instead by the vtable bytes and active705E24 caller. Linear scans do not prove exhaustive callers or reachability.

No Rust/tool source edits, Cargo, game launch, or native emulation were performed in this read-only follow-up. No RNG/timer/detach instructions occur in the inspected override body; no claim about upstream stage writers or lifecycle follows from that observation.

