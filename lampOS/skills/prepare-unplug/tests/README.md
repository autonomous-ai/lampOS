# Preparation is not readiness

`requested` checks that the model's explicit tool call produces only a guarded
body request, with no early speech, motion clip or light call. `never-on-its-own`
has no trigger; the host loader tests also verify that a trigger or extra step
cannot be combined with preparation.

The body simulator tests cover physically settling versus a stalled arm and
invalid readbacks. Skills' receipt tests cover stale, missing, wrong-source,
wrong-request and failed completion; Expression tests cover its matching asleep
publication and privacy. These are host tests, not a power-cut or motor-safety
measurement. An authorized lamp run is required before enabling the skill.
