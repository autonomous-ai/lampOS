# Relaxing-music source example

This is an original, quiet 48-second instrumental sketch. Eight overlapping
chords support sixteen sparse upper notes; no melody or recording is borrowed.
The score, integer renderer and resulting recording are Apache-2.0 licensed
(`PROVENANCE.md`, `LICENSE`).

This folder deliberately has no WAV and does not belong in live `skills/`.
The host renderer stages a complete installable folder under the ignored
`artifacts/skills/relaxing-music/staged/relaxing-music/`. Rendering is a build
step, never a lamp service or a playback-time operation. It uses only Rust's
standard library.

## Build and inspect

Run from the `lampOS` workspace root, using the same source revision as the
requested-audio implementation:

```sh
cargo test -p lamp-skills --example render_relaxing_music
cargo run -p lamp-skills --release --example render_relaxing_music
```

The renderer writes `relaxing.wav`, `skill.toml`, `SKILL.md`, `README.md`,
`PROVENANCE.md`, `score.rs`, and `LICENSE` into the generated folder. It
prints the exact frame count, byte count, peak and largest sliding 10 ms
sum of squared samples. An identical generated file is reused; a different
file or a symlink is refused, so a rebuild cannot silently replace edits.
When intentionally changing the score, remove or move the previous generated
folder before rendering the new version.

The WAV is precisely 1,152,000 frames (48 seconds), 24,000 Hz, mono,
signed 16-bit PCM with a canonical 44-byte WAV header: 2,304,044 bytes.
It fades in over two seconds and out over three. Whole-track scaling limits
every sample to an absolute value of 900, without clipping. That peak is
below -31 dBFS, so **every** 10 ms RMS window is below -30 dBFS and the
peak is also below -12 dBFS. Tests check all sliding 240-frame windows,
including ones crossing note and chord boundaries. No normalization or
volume increase occurs on the lamp.

The tests check exact bytes, timing, fades, non-silent musical structure,
WAV fields and conservative amplitude bounds. They do not establish
perceived quality, acoustic volume or cancellation latency on a lamp.
Listen to the generated piece at the intended product volume during an
authorized device check before enabling the feature there.

## Package and install

The current requested-audio implementation must be present: older builds
correctly refuse the new `requested_audio` step. The complete generated folder
has an installable manifest at version `1.0.0`, one tool,
`play_relaxing_music`, and only `[permissions] speak = true`. It has no
automatic trigger, topic reads, body or light permission.
Its second `[[do]]` step has `after = "say"` and
`requested_audio = "relaxing.wav"` after the first step's short confirmation.

```sh
cargo run -p lamp-skills --bin lamp-skills -- pack \
  --out artifacts/skills/relaxing-music/packages \
  artifacts/skills/relaxing-music/staged/relaxing-music
```

This uses the existing package format and its SHA-256/size checks, not a
new installer or a runtime build hook. Keep the resulting
`packages/relaxing-music-1.0.0.tar` out of Git. The package and disposable
installation stay beside the staged skill, so neither can accidentally be
included when it is packed again.

For a host-only installation check, use a disposable directory:

```sh
mkdir -p artifacts/skills/relaxing-music/installed
cargo run -p lamp-skills --bin lamp-skills -- install \
  --dir artifacts/skills/relaxing-music/installed \
  artifacts/skills/relaxing-music/packages/relaxing-music-1.0.0.tar
```

On an authorized lamp, `lamp-skills install PACKAGE.tar` installs through the
existing service's skill folder; nothing in this example deploys to a device.

`LAMP_REQUESTED_AUDIO` enables playback only when its value is exactly `on`.
It is off by default; packaging and installation do not change the switch.
Enable it only as part of the separately validated service configuration.
The person must request music aloud. Voice waits for the confirmation's final
playback receipt, then uses the same owner and existing speaker worker.
Stop, interruption, privacy and speaker mute consume the request; no repeat
or later restart is scheduled. Messages requests are refused even when
`speak=true`. No automatic volume change, network request, alternate player
or extra decoder process is part of this example.
