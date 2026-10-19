# Texting the lamp from the Autonomous app (#1673)

A person can text the lamp from the Autonomous app and get an answer in
text, and the news of a task it handed over when nobody is at the lamp.
**Off unless `LAMP_MESSAGES=on`**, with `LAMP_CLOUD=on`.

The design was agreed on #1673 (the first builder, 19 October 07:3x and
08:05).

## The wire: V1's own

The app and the backend already speak V1's chat pair (`docs/mqtt.md` in the
repository above lampOS, "`chat.send` + `chat.event`"), so they need nothing
new for a text and its reply:

```
app ──HTTP──▶ backend ──fa: chat.send──▶ lamp-cloud ──chat/in──▶ the voice
app ◀── SSE ── backend ◀── fd: chat.event × N ── lamp-cloud ◀──chat/out── the voice
```

- **`chat.send`** (`{"message", "session_id", "speak"}`) is acked at once on
  `fd_channel` with `{"run_id", "session_id"}`, and the text goes on the bus
  as `chat/in` (`lamp_msgs::ChatIn`). `speak` is off unless asked: a text
  from another room never makes the lamp talk. A photo or a file is refused
  for now ("photos and files aren't read on this lamp yet"): a bus message
  is at most 60,000 bytes, so they will go through a file.
- **Nothing to answer:** with nothing on the lamp listening on `chat/in`, the
  text is refused at once ("nothing on this lamp answers texts right now"),
  not after a minute.
- **The answer** comes back on `chat/out` (`lamp_msgs::ChatOut`) in pieces,
  and goes out as `chat.event`s whose `event` is V1's `MonitorEvent`, which
  the app's chat already reads:
  - the reply's words so far as `assistant_delta`, gathered for 250 ms as
    V1's, with the words since the last in `summary`, an empty `id` and
    `detail.coalesced: true`;
  - the whole reply as `chat_response`, `state: "final"`, with a 200
    character `summary` and the text in `detail.message`, which ends the run;
  - a fault as `chat_response`, `state: "error"`, with `error`;
  - a task's news, sent while nobody is at the lamp, as `task_news`, with no
    run.
- **A run nothing answers** within 60 s ends as `chat_response`,
  `state: "error"`, "The lamp didn't answer in time. Please try again.", so
  the app never waits on a lamp that will not answer. Pieces for a run that
  has ended are dropped, as V1 mirrors only the runs it acked.

`lamp-cloud` looks for pieces every 50 ms beside its wait for the broker's
events (`crate::chat`), only with the switch on. Nothing in it waits on the
voice. The look never cuts that wait short: the MQTT client's wait is where it
makes its connection and writes its packets, so one dropped part way would
lose a connection half made, and a broker slower than 50 ms to answer would
never be joined.

## Who answers: the voice, in text

With `LAMP_MESSAGES=on`, the coordinator passes `--messages` to its provider
workers, after `--misheard-hum` and `--holding-answer`, and the second route's worker answers
texts (`crate::messages`). That worker runs only with the second route's
config (`LAMP_VOICE_SECOND_CONFIG`), standing by beside Gemini's or as the
only voice. Without that config, nothing takes `chat/in`, and `lamp-cloud`
refuses each text at once. The coordinator's journal says so once: `LAMP_MESSAGES=on, but texts need the second route
(LAMP_VOICE_SECOND_CONFIG), which is not set: each text is refused`.

It's a text conversation beside the spoken one, never in it. It never uses
the room's Live session or the speaker.

- **What answers:** the second route's text client, a client of its own on
  the same model (`second_provider::net::Chat`).
- **What it's told:** the same system message as a spoken turn (character,
  notes, memory), and beside it, a note that this conversation is typed:
  "(For you to know, not to answer: this conversation is typed, in the
  Autonomous app on the person's phone, not spoken at the lamp. They may be
  in another room or away. Answer in text, briefly and as yourself; you can't
  see or hear them now, and nothing you write is said aloud.)"
  (`messages::NOTE`). The spoken thread's history isn't sent.
- **Each chat** (the app's `session_id`) keeps its own 12 messages, and a
  chat quiet for 30 minutes starts again with nothing kept.
- **The reply** goes on `chat/out` as it comes (`delta`), then whole
  (`final`). A model that can't be reached is a fault the person can read:
  "Sorry, I can't answer just now. Please try again in a moment."
- **Functions** are the voice's own, run as a spoken turn's are, but typed,
  not heard. One text may make up to 4 rounds of calls.
- **A task asked for in a text** is started only by a "yes" typed in the same
  chat within 120 s. Each chat's texts are numbered in a block of their own,
  apart from the spoken ones (`messages::same_chat`, `agent_tasks::may_confirm`).
  A spoken yes never starts a typed task, nor a typed yes a spoken one.
- **The journal:** `lamp-voice second: texts from the Autonomous app are
  answered here (LAMP_MESSAGES=on)` once at the start, and `a text got no
  answer: (why)` when the model failed.
- **Not yet:** `speak` (say the reply aloud too) is carried, but not acted
  on; photos and files are refused before they reach the voice.

## News when you're away (later)

When the tasks part has a task's news and nobody is at the lamp (no face and
no voice for 5 minutes, one presence signal shared with #1677), the news goes
to the app as `task_news` instead of into an empty room. That one event is
new on the wire: the backend relays a device-started event, and the app
shows it (#1673's app and backend half, after #1396).

## Tests

- `lamp_cloud::chat::tests`: a `chat.send`'s text, session and `speak`, and
  the refusals; words gathered for 250 ms, sent before the whole reply, which
  ends the run; a run unanswered for a minute; a fault and task news; the
  summary's length; each run's own name.
- `lamp_cloud::online::tests::a_text_is_acked_with_its_run_and_its_answer_goes_out_as_chat_events`:
  through the test broker, the ack on `fd_channel` with its run and session,
  then the answer's `chat.event`; without the switch, V1's refusal.
- `lamp_cloud::online::tests::a_slow_connack_is_waited_for_while_texts_are_taken`:
  a broker that answers CONNECT after 300 ms, six of the looks, is joined at
  the first try, and only once.
- `crates/cloud/tests/texts.rs`, through `Real` on a bus of the test's own: a
  text on `chat/in` and its answer from a stand-in voice back as V1's events;
  refused at once with nothing listening, and "not on this lamp" with the
  switch off; task news with no text asked first.
- `lamp_msgs::chat::tests`: the two types' checks and their wire.
- `messages::tests`: a text answered in pieces, then whole, and told it is
  typed; a chat keeps its own exchanges and another chat none; a function
  the model calls is run typed and its result given back; a model that
  can't answer is a fault the person can read; each chat numbers its texts
  in a block of its own; a chat keeps 12 messages and forgets after half an
  hour; through a bus, the worker answers a text from `chat/in` on
  `chat/out`.
- `agent_tasks::tests::a_typed_yes_counts_only_in_the_chat_that_asked_and_soon`,
  with the spoken test's addition that a typed yes never starts a spoken task.
- `provider_worker::tests::going_on_asks::messages_is_read_after_misheard_hum_and_the_holding_answer`
  and `coordinator::second::tests::a_second_route_is_there_standing_by_or_alone_and_not_with_gemini_alone`.
