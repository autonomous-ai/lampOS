---
name: radio-drama
description: Tell or play a story. The device plays real narrated audio stories (Autonomous Radio Drama, ~5-minute episodes) on its speaker, so use this for any request to tell, read or play a story, a bedtime story, an audio or radio drama — random, by genre (romance, horror, revenge, fantasy…) or by title — and to continue, resume, skip to the next or previous episode, stop the story, or ask what radio drama is. Do not make up a story yourself. NOT for songs (music skill).
---

# Radio Drama

Autonomous Radio Drama is a catalog of short serialized stories (about 25 series, 6–24 episodes of ~5 minutes each, mostly English, a few Spanish). Stories and audio come from the Autonomous backend with this device's own key; the backend decides which episodes are open. The helper `scripts/radio_drama.py` in this skill's directory (on OpenClaw `/root/.openclaw/workspace/skills/radio-drama`) does everything: catalog, choosing, fetching the episode, playing it through the speaker, chaining the next episodes, and remembering where each person stopped. Run it with the **terminal** tool from that directory (never a browser or code-execution tool — it needs the device's files and speaker); it prints one JSON object.

```bash
python3 scripts/radio_drama.py [--person {name}] <command> [options]
```

`--person` goes **before** the command. Pass the identified speaker (`{name}` from the injected context) so each person keeps their own progress; omit it when nobody is identified. Never invent a name.

**Spoken output:** Everything outside HW markers in your reply is read aloud. Run the helper first. When it starts an episode it already says the title and episode aloud (`"announced": true`) and the speaker is then busy with the story, so reply with **only the emotion marker and no words**. In every other case give **one short sentence** (under 20 words) based on its JSON. Do not read the synopsis unless asked. Keep command choice and JSON reading in the native thinking channel; no preamble.

## Pick the command

| User says | Command |
|---|---|
| "Tell me a story", "play a radio drama", "something random" | `play --random` |
| "A romance story", "something scary", "a revenge drama" | `play --random --genre "<genre words>"` |
| "Play *The Vampire Prince's Deadly Bride*" (any part of a title) | `play --story "<title words>"` — resumes where that person stopped |
| "…from the beginning", "start over" | `play --story "<title>" --restart` |
| "Episode 4 of …" | `play --story "<title>" --episode 4` |
| "Continue", "keep listening", "resume my story", "what happened next" | `continue` |
| "Next episode" / "previous episode" | `next` / `previous` |
| "Stop the story", "pause" | `stop` |
| "What am I listening to?", "where did I stop?" | `status` |
| "What stories / genres do you have?" | `genres` or `list [--genre G]` |
| "What is radio drama?", "how do I use it?" | `status`, then explain (see below) |
| "One I've already heard is fine", "play it again" | add `--include-heard` to `play --random` |
| "A Spanish story", "una historia en español" | `play --random --language es` (also `genres`/`list --language es`) |

Random, `genres` and `list` default to English stories; `--language all` covers every language. `--story` searches titles in every language. Genre words are matched loosely against the genre, tropes and title, so pass the user's words: `--genre romance`, `--genre "werewolf"`, `--genre revenge`, `--genre horror`, `--genre comedy`, `--genre fantasy`. Call `genres` if you need the real list.

Random never picks a story this person has already started, unless `--include-heard`. Episodes chain automatically (up to 5 back to back); when the user talks to the robot, playback pauses and `continue` picks up a few seconds before where it stopped.

## When the user asks what this is or how to use it

Questions like "what is radio drama?", "what stories do you have?", "how do I listen to a story?", "what can I say?". Explain from the facts below, in the user's language. Do not play anything unless asked.

1. Run `status` first. If `last_story` is present and not `done`, mention it at the end: "You were on episode {episode} of *{title}* — say *continue* to pick it up."
2. **By voice: keep it to three short sentences** and offer to start one. For example:
   > "Radio Drama is short audio stories from Autonomous, about five minutes an episode. Say *tell me a story*, ask for a genre like romance or horror, or name a story — and say *continue* any time to pick up where you stopped. Want me to start one?"
3. **In chat (Telegram, web), or when they ask for more detail**, give the full guide:

   - **What it is:** serialized audio dramas from Autonomous Radio — about 25 stories, up to 24 episodes each, ~5 minutes per episode, mostly in English with a few in Spanish. Genres include romance, romantic comedy and fantasy, revenge and family drama, werewolf and gothic romance, medical drama, progression fantasy and horror.
   - **Start a story:** "tell me a story" (a random one you haven't heard), "play a romance story" / "something scary" (by genre), "play the vampire story" (by any words from the title).
   - **While listening:** "next episode", "previous episode", "episode 4 of …", "stop the story". Episodes play back to back, up to five in a row.
   - **Pick up later:** "continue my story" resumes a few seconds before where you stopped — even after talking to me in between. "Start it over" plays from episode 1.
   - **Memory:** I remember each person's stories and where they stopped, and random picks skip stories you've already started unless you say a repeat is fine.
   - **Free episodes:** without a subscription the first 5 episodes of every story are free; a subscription opens every episode. If `status` shows `subscribed: true`, say their subscription is active (until `access_until`).
   - **What's available:** "what genres do you have?" or "list the romance stories" (uses `genres` / `list`).

Only state what is listed here: no languages beyond English and Spanish, no download, no skipping within an episode (only whole episodes), no story recommendations beyond genre and title matching.

## Read the JSON

| JSON | Say |
|---|---|
| `"action": "playing"`, `"announced": true` | Nothing spoken — reply with the emotion marker only (the helper named the episode; speech during the story is refused). In chat, write "Playing *{story}*, episode {episode}." |
| `"action": "playing"`, `"announced": false` | "Here's *{story}*, episode {episode}." — add "picking up where you left off" when `resumed_at_s` > 0 |
| `"action": "stopped"` | "Paused *{story}*. Say continue anytime." |
| `"reason": "locked"` | "The first {open_through} episodes are free; the rest need a Radio Drama subscription." In chat, add `subscribe_url` when present. Offer another story. |
| `"reason": "all_heard"` | "You've heard every {genre} story — want one again or a different genre?" (then `--include-heard` if yes) |
| `"reason": "story_not_found"` | Ask which story they mean; `list` can help. |
| `"reason": "story_finished"` | "You finished *{story}*!" Offer `--restart` or a new random story. |
| `"reason": "nothing_to_continue"` | Offer a random story. |
| `"reason": "speaker_busy"` / `"busy"` / `"unavailable"` / `"download_failed"` | "I can't play stories right now, try again in a moment." |
| `also_matched` present | Mention the alternatives only if the user seems unsure. |

Pair a playing reply with `[HW:/emotion:{"emotion":"happy","intensity":0.7}]` (use `curious` for horror/mystery). Stopping needs no emotion.

## Examples

| Input | Run | Reply |
|---|---|---|
| "Tell me a romance story" | `python3 scripts/radio_drama.py --person {name} play --random --genre romance` | `[HW:/emotion:{"emotion":"happy","intensity":0.7}]` |
| "Continue my story" | `python3 scripts/radio_drama.py --person {name} continue` | `[HW:/emotion:{"emotion":"happy","intensity":0.6}]` |
| "Play the vampire one" | `python3 scripts/radio_drama.py --person {name} play --story "vampire"` | `[HW:/emotion:{"emotion":"curious","intensity":0.7}]` |
| "Stop the story" | `python3 scripts/radio_drama.py --person {name} stop` | Paused. Say continue anytime. |

## Notes

- Answer in the user's language but keep titles as they are.
- The helper reads the backend URL and the device key from the device config by itself — never read, print or pass the key, and never call the backend yourself.
- Playback starts after a few seconds while the rest of the episode downloads. Progress lives in `/root/local/radio-drama/<person>.json`; recent episodes are cached in `/tmp/radio-drama/` (about 60 MB at most).
- To stop music use the music skill; `stop` here also stops whatever HAL is playing.
