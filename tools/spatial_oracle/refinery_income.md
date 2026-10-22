# Refinery deposit arithmetic

`python -m tools.spatial_oracle.refinery_income --check` compares 191 original
House credit calls, 258 original Unit numeric interiors and twelve native
field-reader controls. Set
`VERA20K_GAMEMD_EXE` to the local retail executable. `--write` explicitly
regenerates this corpus; existing money corpora remain separate.

The full `HouseClass::Add_Tiberium_Credits` body at `004F9610..004F9666`
executes, including both original `007C5F00` conversions. It converts the
entire score-plus-amount-times-5 expression, then the entire
balance-plus-signed-Value-times-IncomeMult-times-amount expression. It returns
the supplied amount on the x87 stack. Inputs preserve raw float32 words,
four independently supplied signed32 Tiberium Values, slot index and signed
initial balance/score. Whole amounts 1 through 40 accompany fractional,
negative, extreme, subnormal, infinity and quiet-NaN component controls.
The named 40-unit, Value25, IncomeMult `.9` control credits899 from balance0.

The Unit rows execute `0073E40C` through the **unexecuted** boundary
`0073E4DA` or `0073E4DC`. Their supplied register/stack state is ESI=Unit,
EBX=recipient House, EBP=selected slot and the signed effective purifier
count at ESP+30. The original `GetAmount 006C9680` reads the selected cargo
slot. The caller computes count times raw PurifierBonus times amount and
narrows it to float32 at `0073E441`. Original `RemoveAmount 006C96B0`
then drains cargo. A positive removed amount invokes the base House credit
call; only an ordered positive narrowed bonus invokes the second call.
Each call records its raw amount and wallet before and after, preserving
the two conversions' history. The positive path clears the Unit stage.
Two empty controls retain stage123 and stop before `0073E4DC`.

The supplied pattern `3EAA7EF9` is an arbitrary raw input below `.333`'s
nearest-rounded float32 value; the numeric rows do not parse it. The separate
`3EAA7EFA` input matches the native no-percent `.333` field reader, while
the native `33.3%` reader produces `3EAA7EF9`. Positive,
zero and negative effective counts and bonuses exercise narrowing and
gate behavior; signed extreme counts are already-combined inputs. The
country/difficulty rules and virtual-count wrapping that produce this
effective count are outside this interior.

Twelve additional `reader_rows` controls establish both field readers for
`1.2`, `.9`, `.333`, `33.3%`, absent input and retained absence after a native
`1.2` read. IncomeMult executes the original Country constructor `005113F0`
and complete Country reader `00511850`. PurifierBonus executes exact caller
`0066FC56` through unexecuted `0066FC7B`, including `ReadDouble 005283D0`
and float store `0066FC75`. Its initial `.25` field is supplied; the full
Rules constructor and reader do not run. Each row records the actual
ReadDouble default double bits, section/key and final float field bits.
The existing BulletReader supplies cached INI strings, heap and CRT TLS;
original scanner initialization `007C8F5E` and decimal parsing execute.
Physical INI/archive loading is excluded.

The ambient x87 control is `0E7F`: masked exceptions, 53-bit precision,
truncate. Every executable PE section is compared with its original mapped
bytes before and after every row. No original instruction or gameplay
callback is replaced. The inherited fixture's heap/CRT transports are
guarded and never entered. Original `0065C6D0` initializes complete Main
and Scenario RNG buffers before the measured interval; their 1012 bytes
each remain unchanged afterwards.

Deposit rows are component comparisons with supplied House, country, Rules,
Tiberium, cargo and stack data. They execute no native constructors,
configuration readers, harvesting history, map lookup, refinery smoke or
radio, full Unit dispatch, slave deposit routine, native save loading or
mission suffix. The Unit cargo controls are positive whole amounts up to
40; arbitrary raw amount controls apply to the House primitive. Fractional
storage representation remains a separate question. The corpus publishes
computed results and explicit boundaries, without executable bytes or
executable identity hashes. Reader rows separately use the existing
Country fixture's supplied complete Main/Scenario/MapGen buffers and
assert all three unchanged, alongside its action-timer sentinel.
Their measured field-read intervals begin after fixture/native Country
constructor setup.
