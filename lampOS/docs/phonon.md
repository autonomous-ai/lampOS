# Phonon: a voice that runs on the lamp (#8754, built, off until a person chooses it)

Phonon is a third choice on the app's Voice page, beside the lamp's own
voice and a person's own ElevenLabs voice. It is Gradium's Phonon 31m
text-to-speech model, run on the lamp's CPU by `lamp-phonon`. It needs no
key, and once the model is on the lamp the speech never leaves it.

- **Model:** `gradium/phonon-31m-en`, 31 M parameters, English only,
  **Apache 2.0**. Its q8 weights are 64.5 MB. Phonon 40m and Phonon 40m
  multilingual are CC BY-NC 4.0 and are never fetched: nothing in lampOS
  names them, and a test pins the source to the 31m repository.
- **Runtime:** the `ptts` crate 0.7.1 (MIT OR Apache-2.0), pure Rust on
  the `xn` tensor library: no Python, no ONNX runtime, no C or C++ build.
- **Voices:** 24 of the `gradium/phonon-voices` catalog's American and
  British English voices (Apache 2.0), each a 256 KB file. Freya is the
  default. Elodie-Rose is left out because the app takes only letters and
  digits, and the character voices are left out because they aren't a
  lamp's.
- **Output:** 24 kHz mono, streamed frame by frame (about 80 ms each). That
  is the voice's own rate, so nothing is resampled.

## How it is chosen

Through the keys helper, as a person's own voice is (`docs/own-keys.md`),
but with no key and no check against a service:

- **Only where it is installed:** the helper offers and takes Phonon only
  when `lamp-phonon.service` is in `/etc/systemd/system`,
  `/usr/lib/systemd/system` or `/lib/systemd/system`. The product's install
  doesn't carry it yet, so on a product lamp nothing below appears:
  `keys.get` has no Phonon offer, and `keys.set` for `phonon` is refused
  with code `not_on_this_lamp`, the person's ElevenLabs key and voice
  untouched. Otherwise a choice would remove their voice for a service
  that isn't there.
- **`keys.get`** lists one more offer where it is installed and the lamp
  speaks English (the language file is missing, empty or `en`):
  `{"service":"phonon","name":"On this lamp","kind":"voice","runs":"lamp","models":[],"base_url":false,"fields":["voice_id"],"voices":["Freya", …]}`.
  It also reports `"phonon"` as the chosen voice's name, or `null`.
- **`keys.set`** `{"service":"phonon","voice_id":"Toby"}` keeps the name
  (any case) in `/etc/lampOS/voice/own/phonon.voice` (0600). A key, any
  other field, a voice not offered, a lamp in another language
  (`unavailable`) or a lamp without the unit (`not_on_this_lamp`) is
  refused, and nothing is kept or removed.
- **`keys.remove`** `{"service":"phonon"}` removes it. The lamp's own
  voice comes back.
- **One own voice at a time:** choosing Phonon removes a person's own
  ElevenLabs key and voice, and keeping an ElevenLabs voice removes Phonon.
- After a change the helper waits for a quiet moment, restarts
  `lamp-phonon.service` (which starts only while the choice exists), then
  the voice.

## How it speaks

`lamp-phonon serve` reads the choice as its credential (`own_phonon.voice`,
the choice alone, never the keys beside it). The first time it starts, it
fetches the model and voices from their pinned Hugging Face commits into
`/var/lib/lampos-phonon/phonon-31m-en/`. Each file is written as `.part`
and renamed only when its size and SHA-256 match the pins in
`lamp_msgs::phonon`. Kept files are hashed again at every start. Then it
loads the model, says one unheard warm-up word, and serves
`/run/lampos-phonon/phonon.sock`.

The voice (`Voice::Phonon` in `second_provider/services.rs`) asks it one
sentence per connection: a JSON line `{"say": text, "voice": name}`, with
the speech tags taken out. The answer is frames: a little-endian `u32`
sample count, then that many `i16` samples. A count of `0` ends the
sentence, and `u32::MAX` means it failed. The voice plays each frame as it
comes.

- **Barge-in and stop:** the voice drops the speech, which closes the
  connection. The service's next write fails, and it drops the model's
  stream, which stops and joins its generation threads. On a Mac this took
  10 to 20 ms. The next sentence starts at once.
- **When Phonon can't say a sentence** (the model is still being fetched,
  the service stopped or crashed, or it failed before its first sound), the
  lamp's one voice says that sentence as it is, with no notice. The first
  time, the journal says so once. The next sentence asks Phonon again.
- **One sentence at a time:** `lamp-phonon` makes one sentence at a time
  for every asker, the speech cache's lines (below) included. A reply's
  piece waits for the piece before it at most 10 s
  (`PHONON_TURN_WITHIN`), then Phonon has 1.5 s for its first sound
  (`FIRST_WITHIN`, within a 2 s bound on the whole start). If either runs
  out (another asker's sentence still being made, or sound sent slowly),
  the lamp's one voice says the piece, with its own full 4 s window. Before,
  a piece waiting behind a say line ran out the 4 s window just as Phonon
  gave up, and went unsaid; a slow sentence held back every piece after it.
- **Not English:** with the lamp in another language, the voice journals
  that Phonon is chosen but speaks English only, and uses the lamp's one
  voice.
- Phonon is never "near" and never at another address. While Phonon
  speaks, the lamp's one voice is not warmed: neither its connection as a
  line begins (`LAMP_SECOND_SPEECH_WARM`) nor its socket as an answer
  begins (`LAMP_SECOND_SPEECH_SOCKET`). Before, a test lamp on #8768
  journalled "the speech connection opened ahead" at every line, a request
  to the lamp's ElevenLabs voice that Phonon never needed.
- **Every other line the lamp says itself** (the test lamp's row on #8768):
  the say service's lines (`/run/lampos-say/say.sock`: a Store app's or a
  paired computer's exact words, a notice) and the cached fixed lines
  (`say/tts_cache.rs`: the shutdown's, the sleep line, the owner's fixed
  lines, the offline rings' clips) are rendered by the speech cache in the
  voice's main process, apart from the route's worker. It reads the
  choice the same way (`LAMP_OWN_KEYS`, `own_phonon.voice`, the lamp in
  English), so while Phonon is chosen it asks `lamp-phonon` on its socket
  for them too; one journal line, "the speech cache keeps its lines in
  Phonon on the lamp, voice <name>". Before, every such line was the
  lamp's own ElevenLabs voice. Their key names the voice:
  `phonon:<voice>` with the model `phonon-31m-en`, never the lamp's lines
  or a person's ElevenLabs voice's. A line Phonon can't say (not serving
  yet, stopped, a failure, no sound within the render's bound) is said in
  the lamp's own voice, never left unsaid, and kept, if at all, under the
  lamp's key; the next line asks Phonon again. Two lines in a row Phonon
  fails give way to the lamp's own for 10 minutes (`GIVE_WAY_FOR`), so a
  busy or failing `lamp-phonon` costs at most two lines their whole bound;
  one journal line, "Phonon on the lamp failed two lines in a row (<why>);
  the lamp's own voice for the next 600 s, then it again". A line kept
  in Phonon's voice already still plays it in that time; past it Phonon is
  asked again, and a line it says starts the count over. A long say line
  rendered piece by piece as it plays stays in the lamp's voice from the
  first piece Phonon fails to its end, never back to Phonon in the middle.
  A line said in the lamp's voice in Phonon's place is never kept under
  Phonon's key: a warmed fixed line is kept under the lamp's key and
  warmed in Phonon's voice on a later try, and an offline reminder's clip
  is not kept and is rendered again after the clips' backoff. A say line
  with any letter of another script (Japanese, Chinese, Korean, Cyrillic
  and so on) is never asked of Phonon, whose model speaks English only:
  the lamp's own voice says it, and it is no failure. Lines in other
  languages written in the Latin script are not told apart, and still go
  to Phonon. A fixed line not kept in
  Phonon's voice yet plays the lamp's kept one where nothing may wait
  (the shutdown's, the sleep line), and is kept in Phonon's voice the next
  time the say service asks for it. The lines warmed at start fall back the
  same way, so a lamp whose `lamp-phonon` was still fetching its model
  keeps them in the lamp's voice until then. The app's voice preview in a named voice (#7485) is
  always the lamp's own service, never Phonon.

## Headroom and the big cores

`lamp-phonon` runs as its own process in `lampos-voice.slice`, held to two
cores' time (`CPUQuota=200%`) and to `MemoryHigh=450M` / `MemoryMax=600M`.
The rest of the voice keeps its real-time threads and its own `MemoryMin`.
A crash or a hang there never takes the voice down: the voice falls back
for that sentence, and the service restarts.

At start, before any thread, it pins itself to the lamp's fastest cores:
those with the highest `cpuinfo_max_freq` under
`/sys/devices/system/cpu/cpu*/cpufreq`. Every thread it starts later, the
model's included, stays on them. The journal names the cores it picked,
for example `on the fastest cores [6, 7] (2002 MHz)`. With no cpufreq, or
with every core as fast, it stays on every core. `LAMP_PHONON_PIN=all`
turns pinning off. The model's tensor work uses one thread by default
(`LAMP_PHONON_THREADS`, 1 to 4), beside its own two generation threads
(the flow LM and the decoder).

## Measured

**lamp-361f** (Allwinner: 6 Cortex-A55 at 1.79 GHz as cpu0-5, 2
Cortex-A76 at 2.0 GHz as cpu6-7), with the first bench kit (a generic
aarch64 build), at nice 10 with the lamp busy:

| How it ran | Real-time factor | First audio | Peak memory |
|---|---|---|---|
| Unpinned, 1 thread | 1.02-1.26 | 244-475 ms | 198-338 MB |
| Unpinned, 2 threads | 1.49-2.09 | | |
| Unpinned, 8 threads | 1.59-2.05 | | |
| **On cpu6-7, 1 thread** | **0.80-0.86 (worst 0.954)** | **158-283 ms** | **198 MB** |
| On cpu6-7, 2 threads | 1.11-1.19 | | |

On the A76 cores, a barge-in stopped generation in 175 ms. Freya and Toby
were heard on the lamp, and the quality was fine. This is why it pins to
the big cores with one thread.

**Mac** (Apple M-series, arm64), three sentences warm, 1 thread: the first
audio in 18 ms on average, at a real-time factor of 0.095, with about
300 MB peak memory. Over the socket the first audio took 36 to 40 ms, or
97 ms for a sentence asked right after a barge-in. On the Mac, none of
these changed the real-time factor: a build without the dot-product
instructions (0.097-0.099, with the first audio 27-30 ms), a primed voice
session, or a smaller chunk of text.

**Still to measure on a lamp:** a build with the ARMv8.2 dot-product
instructions. `xn`'s q8 matrix kernels use `sdot` only when built with
`target-feature=+dotprod`, and a generic `aarch64-unknown-linux-gnu`
build, CI's included, has it off. Both the A55 and the A76 have it. Kit 2
compares generic, `+dotprod`, `target-cpu=cortex-a76` and
`target-cpu=cortex-a55` builds, each pinned to the big cores.

The first builder's bars must all hold on the lamp before this goes past
test lamps: the first audio no slower than today's voice, under 1× real
time, headroom kept for the rest of the voice, and barge-in stopping it.
To measure a lamp with the model kept:

    sudo -u lampos /opt/lampOS/phonon/current/lamp-phonon bench --state /var/lib/lampos-phonon

## Trying it on a test lamp without the app

`lamp-phonon` is in `package-binaries.json`'s `optional` list. The
product's install (`deploy/product/`, `lamp-update install`) does not
carry it yet; that follows once the bars hold on a lamp. On a test lamp
running a build of this branch, with `LAMP_OWN_KEYS=on` in
`lamp-voice.service` (the product's since #8367):

1. Install the unit: copy `deploy/units/lamp-phonon.service` to
   `/etc/systemd/system/`, replace `@LAMP_USER@` with `lampos` and
   `@PHONON_SHA256@` with the folder of the installed `lamp-phonon` (or
   point `ExecStart=` at the binary), then `systemctl daemon-reload` and
   `systemctl enable lamp-phonon.service`. From then on the app's Voice
   page offers "On this lamp" too, and choosing it there does steps 2
   and 3's restarts itself.
2. Choose a voice, as root's keys helper would keep it (the helper itself
   answers only `lamp-cloud`):

       echo Freya | sudo install -m 0600 /dev/stdin /etc/lampOS/voice/own/phonon.voice
       sudo systemctl restart lamp-phonon.service lamp-voice.service

   Any name from `lamp_msgs::phonon::VOICES` works, in any case.
3. Check the journal: `lamp-phonon` says which cores it picked, that the
   model was fetched or checked, and that it is warm. `lamp-voice` says
   `the voice is Phonon on the lamp, voice Freya`. Until `lamp-phonon` is
   warm, the lamp speaks in its own voice.
4. Put it back: `sudo rm /etc/lampOS/voice/own/phonon.voice`, then
   `sudo systemctl restart lamp-phonon.service lamp-voice.service`. The
   unit's condition then keeps `lamp-phonon` stopped. To free the space,
   remove `/var/lib/lampos-phonon/`.

This is the same file and the same restarts the app's `keys.set` makes, so
it tests the voice's path as the app would use it.

## Tests

- `cargo test -p lamp-msgs phonon`: the pins (Apache model only, commits,
  SHA-256s), the voices, English only, the request and the framing.
- `-p lamp-phonon`: the big cores found from cpufreq (lamp-361f's cpu6-7),
  one tensor thread by default; a file not matching its pin never kept; the socket
  answering a sentence whole, a bad request or a failure as a failure, a
  closed connection stopping the sentence, and the next sentence said
  after a stopped one.
- `-p lamp-voice phonon`: the client's packets, tags removed, no service or
  a failure being an error before any speech, a dropped speech closing the
  connection; `Voice::Phonon` speaking each sentence with the lamp's voice
  never asked, and falling back without a service; never near; the choice
  only when switched on, chosen and English; a reply's pieces from the
  services the worker builds (`worker::services`) said by Phonon with the
  lamp's voice neither asked nor warmed
  (`a_reply_from_the_services_the_worker_builds_is_said_by_phonon_when_chosen`);
  the speech cache's say lines and fixed lines said by Phonon, the lamp's
  voice for a line it can't say, never the voice preview
  (`say/tts_cache.rs`: `a_say_line_and_a_fixed_line_are_said_by_phonon_while_it_is_chosen`,
  `a_line_phonon_cannot_say_is_said_in_the_lamps_own_and_phonon_is_asked_again`,
  `the_voice_preview_is_never_phonons`,
  `the_phonon_render_asks_lamp_phonon_in_the_chosen_voice_and_fails_without_it`,
  and the pin
  `the_lamps_voice_and_the_persons_elevenlabs_voice_render_as_before`).
- `-p lamp-setup --test it keys`: offered only in English and only where
  the unit is installed (refused `not_on_this_lamp` without it, the
  ElevenLabs files untouched), chosen without a key, one own voice at a
  time, refusals keeping nothing, and removal.
- `-p lamp-cloud keys`: `keys.set` passes a voice name and refuses a key.
