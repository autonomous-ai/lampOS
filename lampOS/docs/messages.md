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
  as `chat/in` (`lamp_msgs::ChatIn`). **`speak` is accepted and ignored:**
  the reply comes back in text only, and the lamp never says a text's reply
  aloud, even when the app sets it (the reviewer on #1713). A photo or a file is refused
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

[Requested local audio](requested-audio.md) requires a live spoken turn's
private context. A Messages function call never obtains it, including a
message carrying `speak=true`. A tool's model arguments, or text asking to
play a recording, cannot turn a Messages or Written request into a spoken
owner. The requested-audio switch does not change this boundary.

- **What answers:** the second route's text client, a client of its own on
  the same model (`second_provider::net::Chat`).
- **What it's told** (`second_provider::worker::texts_ends`): the character,
  the time and place, the room's memory note and the skills, and beside them
  a note that this conversation is typed: "(For you to know, not to answer:
  this conversation is typed, in the Autonomous app on the person's phone,
  not spoken at the lamp. They may be in another room or away. Answer in
  text, briefly and as yourself; you can't see or hear them now, and nothing
  you write is said aloud.)" (`messages::NOTE`).
  - **Never the room's spoken conversation:** not the thread a spoken turn is
    told (`system_message`'s `thread.recall`) and not its history. What was
    said at the lamp stays at the lamp: a text asking "what did they just ask
    you?" isn't given their words (the reviewer on #1713).
  - **The room's memory, never the person's in view:** the lamp can't tell
    who is texting. With the Days memory (`LAMP_MEMORY_DAYS=on`) no memory
    session is opened for a text, since that would close or change the
    spoken one's; a text is told only to call `recall`
    (`remember::told_for_texts`).
- **Each chat** (the app's `session_id`) keeps its own 12 messages, and a
  chat quiet for 30 minutes starts again with nothing kept.
- **The reply** goes on `chat/out` as it comes (`delta`), then whole
  (`final`). A model that can't be reached is a fault the person can read:
  "Sorry, I can't answer just now. Please try again in a moment."
  - **At most one message long:** a reply over 16,000 bytes (`CHAT_TEXT_BYTES`)
    is cut at a character's edge with "…" (`messages::fitted`), and no words
    past that are sent as `delta`, so every piece is one `lamp-cloud` can
    carry (the reviewer on #1713). The second route sets no output limit.
- **A text whose time is up** is passed over, never answered and its
  functions never run: `chat/in` is declared to last
  `lamp_msgs::CHAT_ANSWER_WITHIN` (60 s), the minute `lamp-cloud` waits
  before it ends the run, so a text still queued behind slow ones after
  that is not handed to the voice (the reviewer on #1713).
- **Functions** are the voice's own, run typed, never heard, and never as the
  person's at the lamp (`tools::answer_typed`). One text may make up to 4
  rounds of calls.
  - **Memory:** `remember`, `recall` and `forget` read and write the room's
    memory, never the person's in view.
  - **Faces:** `remember_my_face` and `forget_my_face` are refused, and
    so are the yes and no to being named (#1997): "Faces
    are kept and forgotten only when the person asks at the lamp, in
    person." This is so for every request not heard at the lamp, a request
    the lamp wrote itself too, as marks are (`tools::run_call`).
  - **The room's things:** the light, the volume, timers and skills that
    don't move the lamp run as a spoken line's do, the owner's to ask for
    from the app.
  - **Decided (the first builder, 19 Oct 11:53): a skill that moves the
    body is never run for a text,** Dance included. A lamp that moves
    because of a text nobody in the room sent looks wrong (CLAUDE.md's rule
    8). `lamp-skills` names the tools whose skill asks for the body
    (`[permissions] body`) in its tools file, as `body` beside `tools`
    (`docs/skills.md`). A text's call to one, by its own name or through
    `use_skill`, is refused before the skills are asked:
    "That one moves me, so I do it only when someone asks me at the lamp."
    (`tools::MOVES_ONLY_AT_THE_LAMP`). Said at the lamp, it runs as before.
- **A task asked for in a text** is started only by a "yes" typed in the same
  chat within 120 s. Each chat's texts are numbered in a block of their own,
  apart from the spoken ones (`messages::same_chat`, `agent_tasks::may_confirm`).
  A spoken yes never starts a typed task, nor a typed yes a spoken one.
- **The journal:** `lamp-voice second: texts from the Autonomous app are
  answered here (LAMP_MESSAGES=on)` once at the start, and `a text got no
  answer: (why)` when the model failed.
- **Not taken:** `speak` is carried and ignored, never said aloud; photos and
  files are refused before they reach the voice.

## News when you're away

- **Given with the next text** (built, off with the rest): news of work
  **asked for by text** that the lamp hasn't told aloud is given with the
  answer to the person's next text, once. That's work finished, stopped,
  waiting for the person, or a hand-over that failed; not progress.
  - **Only work asked for by text** (decided, the first builder, 19 Oct
    11:53): in a shared home, another person's work must not reach the
    owner's phone. Work asked for at the lamp is told at the lamp, as
    before. A task started by a typed "yes" is kept as a text's
    (`task_news::keep_typed`, `<store>:<task>`), in the process and in
    `tasks-typed` beside the file of what was told, so a worker started
    again still knows it.
  - The second route's worker reads the tasks part on a thread of its own
    every 10 s (`task_news::ForTexts`, with the worker's own tasks part), so
    a text never waits on it.
  - Up to 3 pieces go in the text's system message after
    `messages::NEWS`, each note without its spoken "(Nobody has spoken."
    opening (`messages::typed`).
  - Only once the whole reply has gone are they kept in the voice's file
    of what was told (`LAMP_TASKS_TOLD`), so the voice doesn't tell them
    aloud after: its own reader reads that file at each pass. Within one
    pass of it (10 s), both can still tell the same news. The tasks part is
    told so once, which closes the harness with `--close-told`. A fault
    gives none, and the news waits.
- **Unasked, when nobody was told** (built, off with the rest; the first
  builder, 20 Oct 21:14): news of work **asked for by text** that is still
  untold a minute after the texts' reader first has it (`UNASKED_AFTER`)
  goes to the phone as **one `task_news` piece on `chat/out`, with no run
  and no chat** (`messages::tell_unasked`), which `lamp-cloud` sends as
  `task_news`.
  - **The minute is the lamp's:** if someone is at the lamp, the voice's own
    reader tells it aloud in that time, and news told aloud is in the file
    of what was told, so the texts' reader no longer has it. Only work asked
    for by text, as above: work asked for at the lamp never goes to the
    phone.
  - **Its words are the model's only:** the second route's text model is
    given the character, the typed note and the news (`messages::UNASKED`,
    each note typed), and the piece is what it writes, cut to one message
    (`fitted`). No note, key or output of the work is added, and a function
    it calls is not run: nobody asked for anything.
  - **Once:** up to 3 pieces of news waiting together make one piece. Once
    sent, they're kept as told, so they're never sent again or given with a
    text. News told aloud, or given with a text, while the model writes is
    not sent. As with a text's news, within one pass of the voice's reader
    (10 s) the lamp can still say aloud news the phone has just had, when
    someone was at the lamp but it was busy for the whole minute.
  - **Only while something takes `chat/out`** (`lamp-cloud` with the
    switch on, a live subscriber on the bus): otherwise it isn't tried, and
    the news waits. A piece counts as told only once its publish reached
    `lamp-cloud` (delivered to a subscriber). One that reached nothing is
    a try, and is sent again a minute on (the first builder, 20 Oct 04:08).
    Past `lamp-cloud` it can still be lost: if the broker's link drops before
    the backend has it, it isn't told at the lamp or with the next text
    either (a `chat.event` isn't sent again, as a change's result is).
  - **A fault, or no words,** sends nothing and keeps nothing. It's tried
    again a minute on, at most 3 times (`UNASKED_TRIES`), and then the news
    waits for the person's next text.
  - Looked at once a second between texts. A text that comes while the
    model writes it ends that round, without using a try, and is answered at
    once, with the news: a text never waits on it.
  - **Off** with `LAMP_MESSAGES`: the worker then runs no texts at all
    (`files.messages`), and nothing goes on `chat/out`.
  - The backend's push to a phone that isn't open on the chat is its own
    (mobile-P1, #1673).

## Unasked texts (`messages/out`)

A skill's short line for the person's phone, which no text asked for: the
first is guard mode's "Someone was at your desk at 14:31, for about 2
minutes." ([guard-mode.md](guard-mode.md), #1685).

- **The topic:** `messages/out`, `lamp_msgs::MessageOut`: `from`
  (`skill:<name>`, stamped by `lamp-skills`, never by the skill's files) and
  `text`, one line of at most 280 bytes (`MESSAGE_TEXT_BYTES`), never a
  picture; and, set only by guard mode on a text that names someone
  (`LAMP_GUARD_NAMES=on`, #2562), `unnamed`, the same line as it reads with
  names off, held to the same bounds, and `named`, who it names as the
  eyes' person ids (`lamp_msgs::known::person_id`, at most 3), never their
  names: both or neither. Left out when there is no name, so an older
  reader reads the message as before. Guard mode sends a named text whose
  named line would be past the 280 bytes as its unnamed words alone, with
  neither field, rather than lose it. An event that lasts 10
  minutes (`MessageOut::LASTS`): older is old news.
- **Who sends:** `lamp-skills`, for a skill whose `[permissions]` give
  `messages = true`, checked again as each text leaves the program
  ([skills.md](skills.md), "A watch").
- **Carried up with `LAMP_MESSAGES=on`** (`lamp_cloud::unasked`): off
  unless set, the same switch as texts. `lamp-cloud` listens on
  `messages/out` from the start and sends each piece on `fd_channel` as
  the backend reads unasked news (the backend and the app, #1722, 21 Oct):
  V1's `chat.event`, the top-level `id` the lamp's own, no `run_id` and no
  `session_id`, and `data.event` a `task_news` event: its own `id`
  (`evt-N`) and `time` (RFC 3339), which together are the backend's
  `news_id`; `summary`, the text's first 200 characters; and
  `detail.message`, the whole text, with `role` and `channel` as an
  answer's. The skill's text only, unchanged: nothing added, no picture,
  no name. It is the shape the voice's unasked task news goes in (#2311),
  made by the same `chat::Stream`. The backend keeps it per lamp, for the
  owner only, for 90 days, and pushes it to the owner's phones; the app
  shows it in the lamp's chat.
  - **Bounded:** at most 6 pieces are taken in any hour
    (`unasked::PER_HOUR`), the backend's own push limit, whatever the skill
    sends (guard mode sends at most one every 10 minutes); a newer one past
    it is dropped, and the journal says so.
  - **Only lamp-skills':** only lines whose publisher is `lamp-skills`
    (`skills`, `unasked::SKILLS`) are taken. One another publisher puts on
    the topic (a test tool, a `lamp-bus` publish from a shell, a part that
    writes it later) is not sent, and the journal names that publisher
    once, never the text: `a text on messages/out from the publisher
    lamp_bus_cli wasn't sent to the phone: only lamp-skills' are, and later
    ones from it aren't journalled` (at most 8 publishers named).
  - **In order, once:** pieces go in the order heard; one heard again (the
    same publisher, boot and `seq`) is dropped.
  - **Offline:** while the broker is away, at most 4 wait in memory
    (`unasked::KEPT_OFFLINE`), the oldest dropped first, and go on
    reconnect. A piece is kept until the broker's ack; one the connection
    took before it goes again with the same event `id` and `time`, so the
    backend pushes it once. A restart of `lamp-cloud` loses those waiting,
    and the bus hands none older than 10 minutes.
  - **Named only as it goes, on the eyes' word then** (#2562): a text with
    `unnamed` is sent as its `text` only when it goes as it is taken, with
    the broker there, and the eyes' `tellable` beside their known file
    (`LAMP_CLOUD_KNOWN_FILE`, `/run/lampos-eyes/known` in the product
    unit), read just then (`lamp_msgs::known::still_tellable`, the same
    reading `lamp-skills` uses), still has a yes from each one in `named`.
    Otherwise (a yes taken back, even for one of them, a forget, no known
    file, or one that can't be read) it goes as its `unnamed` words, so a
    text that waited on the bus behind a slow call never names anyone who
    took their yes back: `a named text sent as unnamed: the eyes did not
    confirm each yes as it went`. Held because the broker is away, or owed
    after a failed send or one lost before its ack, it waits as its
    `unnamed` words, its event's `id` and `time` kept, so a held or
    retried text never names anyone: `a named text held as unnamed: the
    broker was away`. And only straight to the wire: with any publish of
    lamp-cloud's own handed to the client and not yet acknowledged, the
    client may hold a new one behind it (a broker's receive maximum), so it
    goes unnamed: `a named text sent unnamed: an earlier send was still
    unacknowledged`. Each line once each time.
  - **The journal:** one line a piece sent, the skill and the count, never
    the text: `a text from the skill guard-mode went to the phone (3 since
    the start)`; and one for a piece not sent, and why: `a text from the
    skill guard-mode wasn't sent to the phone: more than 6 in an hour`.

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
- `crates/cloud/tests/it/texts.rs`, through `Real` on a bus of the test's own: a
  text on `chat/in` and its answer from a stand-in voice back as V1's events;
  refused at once with nothing listening, and "not on this lamp" with the
  switch off; task news with no text asked first; a text on `chat/in`
  lasts its run's minute.
- `lamp_cloud::unasked::tests`: pieces in order and one heard again
  dropped; at most 6 in any hour; the newest 4 kept offline, those owed
  back first; the journal's lines name the skill, never the text.
- `lamp_cloud::online::tests::a_skills_line_goes_up_as_task_news_with_no_run_and_the_journal_has_no_text`:
  through the test broker, two of guard mode's lines as `chat.event`s,
  field by field, in order, and the journal's lines without their text;
  `without_the_switch_no_skills_line_goes_up`; and
  `a_skills_line_lost_with_the_connection_goes_again_unchanged`, through a
  broker that drops the first connection.
- `crates/cloud/tests/it/texts.rs::a_skills_line_on_messages_out_comes_off_the_bus_as_task_news`:
  through `Real` on a bus of the test's own, a line published as
  `lamp-skills` publishes it; none with the switch off.
- `lamp_msgs::chat::tests`: the two types' checks and their wire, and
  `a_text_lasts_as_long_as_its_run_waits`.
- `messages::tests`: a text answered in pieces, then whole, and told it is
  typed; a chat keeps its own exchanges and another chat none; a function
  the model calls is run typed and its result given back; a model that
  can't answer is a fault the person can read; each chat numbers its texts
  in a block of its own; a chat keeps 12 messages and forgets after half an
  hour; through a bus, the worker answers a text from `chat/in` on
  `chat/out`; a reply longer than a message is cut to fit, its words too;
  through a bus, a text whose time is up behind a slow one is passed over
  and the next answered.
- `second_provider::worker::tests::a_texts_system_message_never_has_the_rooms_spoken_conversation`:
  with a spoken exchange in the thread, the spoken turn's system message
  has it, and a text's (`texts_ends`) has neither it nor any history.
- `second_provider::worker::tests::the_worker_answers_a_text_without_the_rooms_spoken_conversation`:
  through a bus, as the worker runs its texts (`texts_task`, built from the
  character and settings alone, so the spoken `system` can't reach it), the
  model is never sent a line said at the lamp.
- `tools::tests::a_face_is_kept_or_forgotten_only_on_words_heard_at_the_lamp`:
  an unheard face call never reaches the eyes; a heard one does.
- `messages::tests::news_nobody_was_told_goes_to_the_phone_once_in_the_models_words`:
  nothing before the minute; at the minute exactly one `task_news` piece,
  with no run or chat, whose text is the model's words alone; it is kept as
  told and never sent again; the model is given the note typed.
- `messages::tests::news_for_the_phone_waits_on_a_fault_and_stops_after_its_tries`:
  a model with only a call and then no answer sends nothing and keeps
  nothing; it is tried at the minute, then a minute apart, three times,
  then waits for the next text.
- `messages::tests::news_for_the_phone_waits_for_a_listener_and_isnt_sent_once_told_aloud`:
  with nothing on `chat/out` it isn't tried; news told aloud during the
  round isn't sent or kept.
- `messages::tests::news_for_the_phone_lost_on_the_way_is_sent_again`: a
  piece that reached nothing is not kept, and is sent again a minute on.
- `messages::tests::a_text_never_waits_on_the_news_for_the_phone`: through a
  bus, a text during a 30 s round for the phone is answered within 2 s,
  with the news, and nothing more is sent. The fake model signals when the
  slow round begins before the test sends and times the text. A drop witness
  proves that pending round was cancelled before the text answer; a 30 s
  fake round cannot simply outlast the observation and pass. The full
  two-second observation for unwanted phone news remains on the host clock.
  An owned fixture aborts the worker and removes its private bus directory
  on early assertion failure; normal completion also awaits task cancellation.
- `messages::tests::news_waiting_is_given_with_the_next_texts_answer_once`:
  the next text's answer is given the news, typed, and keeps it as given;
  the text after has none; a model that can't answer gives none.
- `task_news::tests::news_for_texts_is_read_beside_them_and_kept_told_once_given`:
  through a stand-in tasks part with one task asked for by text and one at
  the lamp, only the text's is given, its note typed without the spoken
  opening; once given it is in the file of what was told, so the voice's
  own reader doesn't offer it aloud, and still offers the other.
- `agent_tasks::tests::work_asked_for_by_text_is_kept_as_a_texts`: a task
  said yes to by text is kept as a text's; one said yes to at the lamp
  isn't.
- `tools::tests::a_text_never_runs_a_skill_that_moves_the_body`: by its
  own name or through `use_skill`, a text's call to Dance is refused and
  the skills are never asked; a text runs one that doesn't move; at the
  lamp, Dance runs.
- `crates/voice/tests/it/typed_memory.rs`: with Sam in view, a spoken
  `remember` is kept as Sam's, and a typed `remember` and `recall` ask the
  room's memory alone.
- `agent_tasks::tests::a_typed_yes_counts_only_in_the_chat_that_asked_and_soon`,
  with the spoken test's addition that a typed yes never starts a spoken task.
- `provider_worker::tests::going_on_asks::messages_is_read_after_misheard_hum_and_the_holding_answer`
  and `coordinator::second::tests::a_second_route_is_there_standing_by_or_alone_and_not_with_gemini_alone`.
