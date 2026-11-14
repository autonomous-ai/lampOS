# The voice and character the owner chooses

Issue #5394. The owner picks the lamp's voice and writes a few words about
its character in the app; the app sends them down to the lamp, and the lamp
speaks in that voice and takes those words into its character. Off unless
`LAMP_CHOSEN_VOICE=on` on both `lamp-cloud` (beside `LAMP_CLOUD=on` and
`LAMP_SETTINGS_CHANGE=on`) and `lamp-voice`. With it off, neither kind is in
`system.info`'s `can`, both are answered "not on this lamp", and the voice
reads neither file: it speaks and answers exactly as before. The product's
units set it in both since 12 November (#6590).

## The messages

Over the same MQTT data path as the other settings kinds ([cloud.md](cloud.md)),
each answered `starting` at once, then `success` or `failure`, as
`language.set` is.

| Kind | Data | `success` data |
|---|---|---|
| `voice.set` | V1's `tts.set` shape: `{"provider"?, "voice", "language"?, "speed"?}`. `provider` absent or `"elevenlabs"`. `voice` is a name from Backend2's `lamp-2-tts-catalog`; every name there is in the lamp's own table (`lamp_msgs::chosen_voice::ELEVENLABS_VOICES`), which is what is checked. `""` goes back to the default. `language` and `speed` are taken and left alone: the language stays `language.set`'s, the speed 1.0 | `{"provider": "elevenlabs", "voice", "speed": 1.0}` |
| `character.set` | `{"text", "energy"?}`: the owner's own words, plain text on one line, at most 200 characters. `""` clears them. `energy` (#6893), a whole number 1 to 5, is how lively the lamp moves; absent or `null`, it is worked out from the words ("Energy" below). 0, 6 or anything else is refused | `{"text", "energy"}`: the level kept, 1 when the words were cleared |
| `voice.get` | `{}`: answered at once, nothing written; `lamp-cloud` reads the files as it reads the language | `{"provider": "elevenlabs", "voice", "text", "energy"}`: `""` for none kept, and for a kept value that does not fit the rules (as the voice takes it: the default voice, no words); `energy` 1 when none is kept |

A voice the lamp lacks, another provider, or words that break the rules get
`failure` with the reason, and nothing is written: the voice in use stays.
The reason never holds the value sent.

`system.info` says `"character_energy_supported": true` whenever
`character.set` is taken, so the app shows the energy picker only to a lamp
that keeps it.

## Energy

Issue #6893. One level, 1 to 5, kept with the character words, for the
body's movement only: the voice and what it says do not change with it.
**Level 1 is how the lamp always moved, and the default**: no level kept,
one that does not read, or no `lamp-cloud` is level 1.

The owner may pick it (`"energy"` in `character.set`, kept as `chosen`).
Otherwise root's helper works it out from the words once, when they are
kept (`lamp_msgs::chosen_voice::energy_of`, kept as `derived`), from a fixed
table, no model asked: start at 1, add each word's weight found as a whole
word or phrase (lowercase), hold to 1 to 5.

| Weight | English | Vietnamese |
|---|---|---|
| +2 (lively) | energetic, lively, excited, hyper, bubbly, playful | năng động, sôi nổi, hào hứng, hiếu động, nghịch ngợm |
| +1 (cheerful) | cheerful, upbeat, fun, curious, friendly | vui vẻ, vui tươi, vui tính, tò mò, thân thiện |
| −2 (calm) | calm, gentle, quiet, sleepy, serene, shy | điềm tĩnh, nhẹ nhàng, trầm lặng, buồn ngủ, thanh thản, nhút nhát |

The Vietnamese words are kept pending the first builder's answer on #6893
(English-only keyword table?): a separate list (`VI_WORDS`) dropped in one
line if the table is English only.

So "energetic, playful and excited" is 5, "cheerful and curious" 3, "calm
and quiet" or "a wise old owl" 1, and "playful but gentle" 1. "funny" is not
"fun": whole words only.

It is kept in `/etc/lampOS/setup/energy` (644 root): the level on the first
line, `chosen` or `derived` on the second; removed when the words are
cleared, wiped by the reset. A change of the energy alone restarts nothing.
`lamp-cloud` reads the file every 5 s and says it on `character/energy`
(`lamp_msgs/Energy {level, chosen}`, [msgs.md](msgs.md)); inert data, so no
switch of its own: the body reads it only with its own switch (#6893's later
pieces).

## The character words' rules

The words are refused, never rewritten, when they hold a line break or any
control character, a line or paragraph separator, a mark that turns the
text's direction, or any of `<` `>` `[` `]` `{` `}` `` ` ``,
or are longer than 200 characters (`lamp_msgs::chosen_voice::fit_character`,
checked by `lamp-cloud`, again by root's helper, and again by the voice when
it reads the file). So the words stay one plain sentence and cannot open a
new section, a role or a tag in the prompt. The voice tells them after the
lamp's own character (and after the language's rule), in quotes, as a wish
about its manner that never changes the rules above it
(`lamp_msgs::chosen_voice::told`).

## Where they are kept, and when they take

Root's helper (`lamp-setup change`) keeps them beside the language, one line
each, 644 root: `/etc/lampOS/setup/voice` and `/etc/lampOS/setup/character`
([setup.md](setup.md), "Changed after setup"). Updates leave `/etc/lampOS`
alone, so both are kept across updates. The voice reads them at start, so
after a change that altered a file the helper restarts `lamp-voice` once it
rests between turns (5 s, at most 45 s), under lamp-update's lock, as for a
new language: the next line is in the new voice and the next answer has the
words. The same value again restarts nothing. A change made while the one
before it still holds the lock (until its voice has restarted) waits up to
8 s for it and then applies, so the app hears `success`; past that it is
refused, "the lamp is still applying a change; try again in a moment", and
nothing is written.

The chosen voice wins over the language's voice (`second_config.rs`): a
Vietnamese lamp told "Rachel" speaks Vietnamese in Rachel's voice, with the
language's model. It applies only to ElevenLabs, the product's voice.

## The reset

The 5 s reset's plan wipes both files, and the energy
(`crates/setup/fixtures/reset-plan-product.json`): the lamp is back to the
default voice, no words, and level 1. The language is kept, as before.

## A bad value

A file holding a voice this build lacks, or words that break the rules (a
file written by hand, or a voice taken out of the table by a later build),
is said once in the journal ("the chosen voice is not used …: the default
voice", never the value) and left out: the default voice, or no words.
Nothing is spoken about it and the voice starts as usual.

Each start says the voice it speaks with, once, after the language's and the
chosen voice are applied: `lamp-voice: the voice speaks as Rachel` (the
chosen voice, the language's, or the default). A voice set in `second.json`
by id is said as "a voice id from the configuration", never the id. This is
how a tester tells the chosen voice from the default.

## Not built yet

`tts.preview` and a preset list (`character.list`) are not in this piece
(the lead's scope, 5 Nov). A preset is just words the app sends
as `character.set`.

## Lamp check

Not run yet. With the switch on in both units: `voice.set {"voice": "Adam"}`
from the app, then a spoken question is answered in Adam's voice (the
journal's `the chosen voice is now Adam`, then `lamp-voice.service
restarted`, then `lamp-voice: the voice speaks as Adam`); `character.set
{"text": "very short and dry, like a tired butler"}`, then the next answer reads that way; a 5 s reset, then the
default voice and no words.
