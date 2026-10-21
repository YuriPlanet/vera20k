# Optional Ares EVA voices

Build with `--features ares` to enable extra EVA voice tags. Omit that feature
for the original three voice columns and original Side binding behavior. An
INI setting cannot enable this code in a retail build.

Register extra tags in the fixed `EVAMD.INI`, alongside its ordinary events:

```ini
[EVATypes]
0=ExampleVoice

[DialogList]
0=EVA_Example

[EVA_Example]
Allied=EXAMPLEA
Russian=EXAMPLER
Yuri=EXAMPLEY
ExampleVoice=EXAMPLEV
```

The event's selected sample is a physical filename without `.WAV`, limited
to eight native bytes. A missing extra column is silent; it does not fall
back to Yuri. Tags retain their first registration order and compare without
ASCII case. `Allied`, `Russian` and `Yuri` remain the built-in tags.

In Rules, select a tag for an existing Side:

```ini
[ExampleSide]
EVA.Tag=ExampleVoice
```

Each reached Rules pass resolves this key against the same fixed EVAMD catalog.
Missing, empty and unknown values retain the previous selection. Unregistered
`none` or `<none>` clears it; a registered tag with that spelling wins instead.
EVAMD is not a map/mode Rules layer. Saved Side state retains the resolved index.

Fresh/successful saved-load adoption binds from `Scenario.Side`. The queue holds
event identities and selects the sample at dequeue, so a later explicit binding
also affects an already queued event. Resetting the queue preserves that binding.

Reader limits, native control coverage and exclusions are documented in the
[native component evidence](../tools/spatial_oracle/ares_eva.md). This feature
does not load Ares DLLs or establish complete mod compatibility.
