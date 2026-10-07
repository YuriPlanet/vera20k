# Foot locomotor visual-character dependency (static byte evidence)

Binary: Steam gamemd.exe SHA256 3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600. Every JSON is tools.native_inspect output with native identity and exact requested range, Capstone5.0.7. Ghidra8089 gamemd.exe identified the same Steam source path and image base00400000. Ghidra pseudocode/names were leads; the conclusions below rely on original bytes.

Foot4DA4E0 receives force and house. At4DA4E8 it reads Foot+674's ILocomotion pointer; if absent it uses base. At4DA506/507 it passes force and that interface to its stdcall virtual+34 at4DA50A. Nonzero return at4DA50D/50F goes directly to4DA51E return; zero invokes703860 at4DA519 with the original force/house. There is no RNG draw, timer write or detach in this wrapper or the constant leaf.

All eight retail-installed families' immutable ILoco+34 slots are55ABC0. Original55ABC0 bytes33C0C20800 are XOR EAX,EAX; RET8, without any instance/state access. Therefore no movement phase, underwater state, warp progress, piggyback field or observer can turn this particular leaf nonzero.

| Family | Registered CLSID address | Registration factory ctor | COM CreateInstance | Native loco ctor | ILoco vtable | Actual +4 vtable store |
| --- | --- | --- | --- | --- | --- | --- |
| Drive |7E9A30 (4A582741...)|6C3F40|6C4010 -> ctor at6C404C|4AF540|7E7EB0|4AF5C8|
| Hover |7E9A40 (4A582742...)|6C4240|6C4310 -> ctor at6C434C|513C20|7EACFC|513C97|
| Walk |7E9A60 (4A582744...)|6C46C0|6C4790 -> ctor at6C47CC|75AA90|7F69F8|75AAE5|
| Fly |7E9A80 (4A582746...)|6C49C0|6C4A90 -> ctor at6C4ACC|4CC9A0|7E89F4|4CCA12|
| Teleport |7E9A90 (4A582747...)|6C4B40|6C4C10 -> ctor at6C4C4C|718000|7F5000|718064|
| Ship |7E9AB0 (2BEA74E1...)|6C4E40|6C4F10 -> ctor at6C4F4C|69EC50|7F2D8C|69ECD8|
| Jumpjet |7E9AC0 (92612C46...)|6C40C0|6C4190 -> ctor at6C41CF|54AC40|7ECD68|54ACC9|
| Rocket |7E9AD0 (B7B49766...)|6C43C0|6C4490 -> ctor at6C44CC|661EC0|7F0B1C|661F1F|

registered-guid-bytes.json holds original 16-byte GUID encodings. registration-prefix-6bd080.json and registration-6bd140.json pair each CLSID with an original class-factory constructor in6BDxxx. Each family-factory.json records the class-factory vtable assignment plus CreateInstance's actual direct call to that locomotor constructor. factory-vtables.json records actual IClassFactory+0C slots, completing the binding rather than assuming neighboring factory function names. Each family-constructor.json establishes the instance+4 ILocomotion table; each family-vtable.json establishes+34=55ABC0. These boundaries support current Ship SUB/DLPH and Teleport warp users once the selected production type/installed kind is established. This follow-up does not independently parse a specific selected scenario/type INI, construct its native Unit, or execute a full draw scene.

The active installed-kind roster is owned by src/rules/locomotor_type.rs, including existing production-reader retail tests. The constant leaf creates no missing stateful port within those eight classes; avoid adding a duplicated wrapper/second703860 owner merely to emulate this constant call.

Counterexample outside that roster: dormant Tunnel ILoco7F5A24+34 stores7291D0. Its original body checks interface+14==4; that state returns4+bool(force), otherwise0. Thus Tunnel can override with4/5. Tunnel has no retail installed-kind entry; this is not evidence of active ordinary YR reachability. Mech/DropPod and dynamic externally-modified vtables are not exhaustively inspected here. The static equivalence is bounded to the eight exact constructor-installed original tables, not a universal all-locomotor/all-game parity assertion.

No Cargo/GPU runs, source edits, game starts, native emulation, publication or shared Ghidra edits were performed. Saved evidence is inspection only, not a new native execution golden.
