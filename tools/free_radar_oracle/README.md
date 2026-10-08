# Original FreeRadar cases

Run `python -m tools.free_radar_oracle.oracle` from the repository root, with
`VERA20K_GAMEMD_EXE` or `RA2_DIR` pointing to the pinned retail executable.
The command compares results without writing the fixture.

The preserved original fixture has SHA256
`f434e555f94f9f68cbcd8247a000fdf93bf661df79611152d251045d8930ef4d`.
Its `probe_sha256` identifies the original local evidence producer; the portable
runner here uses the shared checked-execution owner and independently compares
every parser, reset, decision and nonlocal result. The executable SHA is
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.

The runner supplies a cached Basic section/entry with the original computed
key CRC, Scenario/House memory, empty provider vector, power values and timer
state. It executes original `68A5E3..68A61A`, `5295F0`, `4A1DE0`, reset
`68383C..683842`, and House decision `508DF0..508F2F`, with no substituted
calls. The nonlocal case executes the full House body. Local decisions stop
before radar/UI side effects.

There are 28 parser/prior cases and 70 empty-provider decisions. Rust tests
(`sim::radar`) compare all 70 through the House's radar outage timer
(`HouseClass+0x2B0`, which `CreateRadarOutage @ 0x0050BCD0` starts); power
blackout is a separate mechanism.
No provider scan, live runtime, complete INI loader or sidebar pixel match is
asserted by these prepared calls.
