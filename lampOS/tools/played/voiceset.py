#!/usr/bin/env python3
"""The scored voice set: every voice use case as a played scene, each line scored against what should happen.

usage: voiceset.py LABEL [GROUP | GROUP/SCENE ...]      no group: every group, in order
       voiceset.py --list                                the groups and their scenes, nothing played

Plays each scene with scenario.py beside this file, from this computer's speakers to the lamp at LAMP_HOST
(lamp_ssh: README.md), then scores each played line from the voice's own events:
  A  the lamp answers it: a LAMP line with words before the next played line
  Q  the lamp stays quiet: no LAMP line with words before the next played line
  S  the lamp stops talking: a cut-in commit, or its turn finished as interrupted, after the line
  -  played and kept, not scored
A line's wait is seconds after it, or "6a": 6 s after the lamp is done with it (its answer played to its last sample,
or its turn ended with none), as a person goes on. A ("bg", NAME, VOLUME, '') step plays make_noise.py's NAME bed
under the rest of the scene. Each scene starts outside a conversation: VOICESET_GAP (30) seconds of quiet are left
after the one before. Nothing here writes to the lamp's memory, puts it to sleep, turns privacy on or moves its
body, so it is safe on a person's own lamp; run it in an empty room. Only `handover` asks for a computer's work:
leave it out on a lamp with a computer paired. VOICESET_HEY_ONLY=1 scores the older groups by the product owner's
rule of 26 Nov: a scene whose first line is answered but doesn't open with "Hey", "Hi" or "Hello" is then expected
left alone (the r- groups are scored as written).
Writes $PLAYED_OUT/scenarios/voiceset-LABEL.tsv (a row a played line) and voiceset-LABEL.txt (the scorecard), and
adds one row a group to $PLAYED_OUT/scenarios/voiceset-history.tsv, for runs compared day by day."""
import os, re, statistics, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
PLAYED = os.environ.get('PLAYED_OUT') or os.path.expanduser('~/lamp-played')
OUT = PLAYED + '/scenarios'
GAP = float(os.environ.get('VOICESET_GAP', '30'))

S, D, K, M, R, T, F = 'Samantha', 'Daniel', 'Karen', 'Moira', 'Rishi', 'Tessa', 'Fred'
# macOS's newer English voices, US and UK.
EU, EG, FU = 'Eddy (English (US))', 'Eddy (English (UK))', 'Flo (English (UK))'
GPU, GMU, RDU = 'Grandpa (English (US))', 'Grandma (English (UK))', 'Reed (English (US))'
SDU, SHU, RKU, J = 'Sandy (English (US))', 'Shelley (English (UK))', 'Rocko (English (US))', 'Junior'

# (group, scene, [(voice, text, seconds to wait after it, expected)])
# Every voice above, for the generated one-person groups (26 Nov), each line in the next one.
VOICES = [S, D, K, M, R, T, F, EU, EG, FU, GPU, GMU, RDU, SDU, SHU, RKU, J]

SCENES = [
    # Calls in every form, each on its own, no face in front of the lamp.
    ('calls', 'hey-lamp', [(S, 'Hey Lamp, what time is it?', 14, 'A')]),
    ('calls', 'lamp-comma', [(S, 'Lamp, what is two plus three?', 14, 'A')]),
    ('calls', 'hello-lamp', [(S, 'Hello Lamp, how are you today?', 14, 'A')]),
    ('calls', 'hi-lamp', [(S, 'Hi Lamp, tell me a fun fact about the moon.', 18, 'A')]),
    ('calls', 'ok-lamp', [(S, 'OK Lamp, what day is it today?', 14, 'A')]),
    ('calls', 'name-at-end', [(S, 'What is the capital of Japan, Lamp?', 14, 'A')]),
    ('calls', 'name-alone-then-ask', [(S, 'Hey Lamp.', 6, '-'), (S, 'What is the weather like today?', 16, 'A')]),
    ('calls', 'hey-lamp-fast', [(S, 'Hey Lamp what is ten minus four?', 14, 'A')]),
    ('calls', 'accent-uk', [(D, 'Hey Lamp, what time is it in London?', 14, 'A')]),
    ('calls', 'accent-au', [(K, 'Hey Lamp, how many days are in a leap year?', 14, 'A')]),
    ('calls', 'accent-ie', [(M, 'Hey Lamp, what is the tallest mountain in the world?', 14, 'A')]),
    ('calls', 'accent-in', [(R, 'Hey Lamp, how do I say thank you in Spanish?', 14, 'A')]),
    ('calls', 'accent-za', [(T, 'Hey Lamp, what is twelve times twelve?', 14, 'A')]),
    ('calls', 'accent-us-man', [(F, 'Hey Lamp, how far away is the sun?', 16, 'A')]),
    ('calls', 'mid-pause', [(S, 'Hey Lamp, can you tell me [[slnc 1400]] how many minutes are in a day?', 16, 'A')]),
    # Follow-ups without the name, at different gaps after the lamp's answer.
    ('follow-ups', 'math-three-turns', [(S, 'Hey Lamp, what is ten times ten?', 12, 'A'), (S, 'And divided by four?', 12, 'A'),
                                        (S, 'Now add seven.', 12, 'A'), (S, 'Thanks.', 10, 'A')]),
    ('follow-ups', 'facts-three-turns', [(S, 'Hey Lamp, who wrote Romeo and Juliet?', 14, 'A'), (S, 'When was he born?', 14, 'A'),
                                         (S, 'What else did he write?', 18, 'A')]),
    ('follow-ups', 'plan-three-turns', [(D, 'Hey Lamp, I want to cook dinner tonight. Any ideas?', 18, 'A'),
                                        (D, 'Something with chicken, please.', 18, 'A'), (D, 'How long does that take?', 14, 'A')]),
    # 25 Nov 22:5x: each follow-up now comes its named seconds after the lamp is done ("Na"); the fixed gaps before let
    # "after-18s" come 22.5 s after a short answer, past the owner's 20 s.
    ('follow-ups', 'after-5s', [(S, 'Hey Lamp, what is the capital of Italy?', '5a', '-'), (S, 'And of Spain?', 14, 'A')]),
    ('follow-ups', 'after-18s', [(S, 'Hey Lamp, what is the capital of Canada?', '18a', '-'), (S, 'And of Australia?', 14, 'A')]),
    ('follow-ups', 'after-35s', [(S, 'Hey Lamp, what is the capital of Brazil?', '35a', '-'), (S, 'And of Argentina?', 14, 'Q')]),
    ('follow-ups', 'number-echo', [(S, 'Hey Lamp, what is twenty four times three?', 12, 'A'), (S, 'And times two?', 12, 'A')]),
    ('follow-ups', 'yes-to-its-question', [(S, 'Hey Lamp, can you suggest a short exercise I can do at my desk?', 20, 'A'),
                                           (S, 'Yes, tell me more.', 16, 'A')]),
    # Questions of every kind, each a call.
    ('questions', 'time', [(K, 'Hey Lamp, what time is it?', 12, 'A')]),
    ('questions', 'date', [(K, 'Hey Lamp, what is the date today?', 12, 'A')]),
    ('questions', 'weather', [(S, 'Hey Lamp, will it rain tomorrow?', 16, 'A')]),
    ('questions', 'math-hard', [(S, 'Hey Lamp, what is seventeen percent of two hundred?', 14, 'A')]),
    ('questions', 'convert', [(S, 'Hey Lamp, how many centimetres are in a foot?', 14, 'A')]),
    ('questions', 'define', [(D, 'Hey Lamp, what does the word serendipity mean?', 16, 'A')]),
    ('questions', 'spell', [(S, 'Hey Lamp, how do you spell necessary?', 16, 'A')]),
    ('questions', 'joke', [(S, 'Hey Lamp, tell me a joke.', 16, 'A')]),
    ('questions', 'advice', [(M, 'Hey Lamp, I feel tired this afternoon. What should I do?', 20, 'A')]),
    ('questions', 'about-itself', [(S, 'Hey Lamp, what can you do?', 20, 'A')]),
    ('questions', 'look', [(S, 'Hey Lamp, what do you see right now?', 18, 'A')]),
    # Small talk, the lamp itself, privacy, and everyday asks, each a call.
    ('everyday', 'how-are-you', [(S, 'Hey Lamp, how are you doing today?', 12, 'A')]),
    ('everyday', 'who-made-you', [(D, 'Hey Lamp, who made you?', 12, 'A')]),
    ('everyday', 'your-name', [(S, 'Hey Lamp, what is your name?', 12, 'A')]),
    ('everyday', 'recording', [(S, 'Hey Lamp, are you recording everything I say?', 16, 'A')]),
    ('everyday', 'camera', [(K, 'Hey Lamp, what do you do with your camera?', 16, 'A')]),
    ('everyday', 'translate', [(S, 'Hey Lamp, how do you say good morning in French?', 12, 'A')]),
    ('everyday', 'recipe', [(M, 'Hey Lamp, how do I make a simple tomato pasta?', 22, 'A')]),
    ('everyday', 'health', [(S, 'Hey Lamp, how much water should I drink in a day?', 16, 'A')]),
    ('everyday', 'focus', [(D, 'Hey Lamp, I need to focus for the next hour. Any tips?', 18, 'A')]),
    ('everyday', 'long-ask', [(S, 'Hey Lamp, I have a meeting at three, then I need to pick up my kids at five, and I want to go to the gym '
                               'before dinner. How should I plan my afternoon?', 24, 'A')]),
    ('everyday', 'compliment', [(S, 'Hey Lamp, you are really helpful.', 10, 'A')]),
    # Not "good night": the model may take it as a sleep, which a lamp with no head touch can't be woken from.
    ('everyday', 'goodbye', [(S, 'Hey Lamp, thanks for your help today.', 10, 'A')]),
    # The lamp's own light and sound, each a call; put back after.
    ('controls', 'warmer', [(S, 'Hey Lamp, make your light a little warmer.', 12, 'A')]),
    ('controls', 'brighter', [(S, 'Hey Lamp, make your light brighter.', 12, 'A')]),
    ('controls', 'dimmer', [(S, 'Hey Lamp, dim your light a bit.', 12, 'A')]),
    ('controls', 'colour', [(S, 'Hey Lamp, turn your light blue.', 12, 'A')]),
    ('controls', 'colour-back', [(S, 'Hey Lamp, set your light back to normal.', 12, 'A')]),
    # Never the lamp's volume (Twelve's P2 on #8673: a person's own lamp's volume is never touched), only asked about.
    ('controls', 'volume-asked', [(S, 'Hey Lamp, how loud is your voice set right now?', 12, 'A')]),
    ('controls', 'light-asked', [(S, 'Hey Lamp, what colour is your light right now?', 12, 'A')]),
    # Timers and reminders, each a call.
    ('timers', 'timer-ring', [(S, 'Hey Lamp, set a timer for one minute.', 75, 'A')]),
    ('timers', 'timer-ask-cancel', [(S, 'Hey Lamp, set a timer for ten minutes.', 12, 'A'), (S, 'How much time is left on it?', 12, 'A'),
                                    (S, 'Cancel it, please.', 12, 'A')]),
    ('timers', 'reminder', [(S, 'Hey Lamp, remind me in two minutes to drink some water.', 140, 'A')]),
    # Cut-ins over a long answer.
    ('cut-ins', 'wait-stop', [(S, 'Hey Lamp, tell me a long story about a dragon who learns to fly.', 9, 'A'), (S, 'Wait, stop.', 10, 'S')]),
    ('cut-ins', 'hold-on-new-ask', [(S, 'Hey Lamp, explain how a rainbow forms, in detail.', 8, 'A'),
                                    (S, 'Hold on. What time is it?', 14, 'A')]),
    # The owner's office: colleagues talk first (left alone), which makes the room busy, then a call and a follow-up.
    ('busy', 'chatter-then-call', [(D, 'Did anyone see the email from the client this morning?', 4, 'Q'), (K, 'Yes, they want the drawings by Friday.', 5, 'Q'),
                                   (S, 'Hey Lamp, what time is it?', 12, 'A'), (S, 'And what day is it today?', 12, 'A')]),
    ('busy', 'chatter-then-call-2', [(K, 'I think we should move the meeting to three.', 4, 'Q'), (D, 'Fine by me, I will tell the others.', 5, 'Q'),
                                     (S, 'Hey Lamp, what is fifteen times four?', 12, 'A'), (S, 'And divided by three?', 12, 'A')]),
    ('busy', 'chatter-then-call-uk', [(S, 'Can you pass me the charger?', 4, 'Q'), (K, 'Here you go.', 5, 'Q'),
                                      (D, 'Hey Lamp, how far is the moon from the earth?', 14, 'A'), (D, 'And the sun?', 12, 'A')]),
    ('busy', 'chatter-then-lamp-comma', [(D, 'The coffee machine is broken again.', 4, 'Q'), (K, 'Typical. I will call the repair man.', 5, 'Q'),
                                         (S, 'Lamp, set a timer for ten minutes.', 12, 'A'), (S, 'Actually, cancel it.', 12, 'A')]),
    # The Reviewer-lead's P3 on #8672: greetings said to people in a busy room, which LAMP_NAME_GREETED_STAND_IN must
    # leave alone ("Lynn" is a stand-in and a person's name: its cost, counted).
    ('busy', 'greeting-people', [(D, 'Did anyone see the email from the client this morning?', 4, 'Q'),
                                 (K, 'Hey Helen, can you check the printer?', 6, 'Q'), (D, 'Hi Mom, I will be home at six.', 6, 'Q'),
                                 (S, 'Hey man, are you coming to lunch?', 6, 'Q'), (K, 'Hey Lynn, did you get my message?', 10, 'Q')]),
    # Talk the lamp must leave alone, each outside a conversation.
    ('room', 'two-colleagues', [(D, 'Did you finish the report for tomorrow?', 4, 'Q'), (K, 'Almost. I need about ten more minutes.', 4, 'Q'),
                                (D, 'OK, send it to me when you are done.', 10, 'Q')]),
    ('room', 'lamp-in-passing', [(S, 'I need to buy a new lamp for my bedroom.', 8, 'Q'), (D, 'This lamp is a bit too bright for me.', 8, 'Q'),
                                 # 25 Nov 21:4x: was "Could you move the lamp a little to the left?"; a wrong answer
                                 # to it could move this lamp's body, which takes no moves.
                                 (S, 'Is that lamp the new model?', 10, 'Q')]),
    ('room', 'another-name', [(S, 'Hey Lance, can you come here for a second?', 8, 'Q'), (D, 'Hey Sam, what time is the meeting?', 10, 'Q')]),
    ('room', 'phone-call', [(K, 'Yes. No, I will call you back after the meeting. Okay. Bye.', 10, 'Q')]),
    ('room', 'radio', [(F, 'And in other news today, the weather service expects heavy rain across the north of the country.', 10, 'Q')]),
    ('room', 'short-sounds', [(S, 'Mm-hmm.', 5, 'Q'), (D, 'Yeah.', 5, 'Q'), (S, 'Okay.', 8, 'Q')]),
    ('room', 'no-name-question', [(S, 'What time is it?', 10, 'Q')]),
    ('room', 'colleagues-after-an-answer', [(S, 'Hey Lamp, what is the time?', 12, 'A'), (D, 'Are you coming to lunch with us?', 5, 'Q'),
                                            (K, 'Yes, in five minutes.', 10, 'Q')]),
    # #8384's shape: a faint "Lamp" in the room, then someone else's line half a minute later is no call.
    ('room', 'faint-name-then-talk', [(S, '[[volm 0.15]] Lamp.', 25, '-'), (D, 'What should we do about the budget this quarter?', 10, 'Q')]),
    # #8092's shape: a greeting to a colleague, no name.
    ('room', 'hello-to-a-colleague', [(K, 'Hello! How was your weekend?', 6, 'Q'), (D, 'Good, thanks. We went to the beach.', 10, 'Q')]),
    # Twelve's P2 on #8656: the product's second worker has no voice print, so in a conversation's window nothing by
    # voice holds TV or a third person; a quiet room's window is up to 45 s. These count that cost.
    ('room', 'tv-after-an-answer', [(S, 'Hey Lamp, what is the capital of Peru?', 10, 'A'), (S, 'pause', 18, ''),
                                    (F, 'And now the weather for the weekend: sunny skies in the south, with showers moving in from the west by Sunday.', 12, 'Q')]),
    ('room', 'tv-two-turns', [(S, 'Hey Lamp, what time is it?', 10, 'A'), (F, 'Stay with us after the break for the latest sports news.', 10, 'Q'),
                              (F, 'Welcome back. Tonight the home team won by three goals.', 12, 'Q')]),
    ('room', 'third-voice-after-an-answer', [(S, 'Hey Lamp, how many legs does a spider have?', 10, 'A'), (S, 'pause', 14, ''),
                                             (D, 'Have you seen my keys anywhere?', 10, 'Q')]),
    # October's day scene (99 runs on lamp-4ace, 16-22 Oct, 90.4% answered), its gaps kept, with its memory pair and its
    # computer task swapped for lines of the same kind: this lamp takes no memory writes or computer tasks. Every line
    # should be answered.
    ('day', 'october-day', [(S, 'Hey Lamp, good morning!', 14, 'A'), (S, 'Can you make your light a little warmer?', 14, 'A'),
                            (S, 'What do you see right now?', 18, 'A'), (S, 'What is a good name for a goldfish?', 14, 'A'),
                            (S, 'Hey Lamp, can you tell me a very short story, about fifty words, about a desk lamp that learns to sing?', 40, 'A'),
                            (S, 'That was lovely. Can you tell me a funnier one?', 40, 'A'),
                            (S, 'Which of the two did you like more?', 16, 'A'), (S, 'Remind me in two minutes to stretch.', 14, 'A'),
                            (S, 'And a good name for a hamster?', 14, 'A'), (S, 'What time is it?', 14, 'A'),
                            (S, 'Set a timer for one minute.', 80, 'A'), (S, 'Thanks, Lamp. See you later.', 60, 'A')]),
    # October's day exactly (lampOS/tools/played/day.sh: its 12 lines, its gaps), for the owner's like-for-like
    # comparison with October's 111 runs. ONLY on lamp-74ff with the owner's yes (25 Nov 22:3x): it writes "my favourite
    # colour is green" to the lamp's memory and asks Claude Code on the office computer for a story each run. The
    # night's last step asks the lamp to forget the colour. Not in the repo's set.
    ('day', 'october-exact', [(S, 'Hey Lamp, good morning!', 14, 'A'), (S, 'Can you make your light a little warmer?', 14, 'A'),
                              (S, 'What do you see right now?', 18, 'A'), (S, 'Remember that my favourite colour is green.', 14, 'A'),
                              (S, 'Hey Lamp, can you ask Claude Code on my office computer to write a short story, about two hundred words, about a desk lamp that learns to sing?', 14, 'A'),
                              (S, 'Yes. [[slnc 700]] Please start it.', 100, 'A'), (S, 'How is that task going?', 16, 'A'),
                              (S, 'Remind me in two minutes to stretch.', 14, 'A'), (S, "What's my favourite colour?", 14, 'A'),
                              (S, 'What time is it?', 14, 'A'), (S, 'Set a timer for one minute.', 80, 'A'),
                              (S, 'Thanks, Lamp. See you later.', 60, 'A')]),
    # The same day as a person paces it (25 Nov 21:3x): each line 6 s after the lamp is done with the one before
    # (scenario.py's "6a"), the timer still rung out. October's fixed gaps let a line come 36 s after a short story.
    ('day', 'paced-day', [(S, 'Hey Lamp, good morning!', '6a', 'A'), (S, 'Can you make your light a little warmer?', '6a', 'A'),
                          (S, 'What do you see right now?', '6a', 'A'), (S, 'What is a good name for a goldfish?', '6a', 'A'),
                          (S, 'Hey Lamp, can you tell me a very short story, about fifty words, about a desk lamp that learns to sing?', '6a', 'A'),
                          (S, 'That was lovely. Can you tell me a funnier one?', '6a', 'A'),
                          (S, 'Which of the two did you like more?', '6a', 'A'), (S, 'Remind me in two minutes to stretch.', '6a', 'A'),
                          (S, 'And a good name for a hamster?', '6a', 'A'), (S, 'What time is it?', '6a', 'A'),
                          (S, 'Set a timer for one minute.', 80, 'A'), (S, 'Thanks, Lamp. See you later.', '6a', 'A')]),
    # #8681's row (26 Nov 00:2x; Twelve's P3): lines said to a person with "ma'am" or "sir", in a quiet room and after
    # chatter in a busy one, which LAMP_NAME_SURE_OVER_A_PERSON_WORD must leave alone; any one answered is counted.
    ('person-words', 'quiet-maam-salt', [(K, "Ma'am, could you pass me the salt please?", 10, 'Q')]),
    ('person-words', 'quiet-sir-coffee', [(D, 'Sir, your coffee is ready.', 10, 'Q')]),
    ('person-words', 'quiet-excuse-maam', [(S, "Excuse me ma'am, is this seat taken?", 10, 'Q')]),
    ('person-words', 'quiet-yes-sir', [(D, 'Yes sir, I will send it now.', 10, 'Q')]),
    ('person-words', 'quiet-hey-maam', [(K, "Hey ma'am, you dropped your keys.", 10, 'Q')]),
    ('person-words', 'busy-maam', [(D, 'Did you finish the slides?', 4, 'Q'), (K, 'Almost done.', 4, 'Q'),
                                   (S, "Ma'am, the client is on line two.", 10, 'Q')]),
    ('person-words', 'busy-sir', [(K, 'The printer is jammed again.', 4, 'Q'), (D, 'I will take a look.', 4, 'Q'),
                                  (S, 'Sir, can you sign this form for me?', 10, 'Q')]),
    # And the call it is for: "Hey Lamp" lines, each said plainly, so speech to text sometimes writes "Hey, ma'am."
    ('person-words', 'calls-1', [(S, 'Hey Lamp, good morning!', '6a', 'A'), (S, 'What time is it?', '6a', 'A')]),
    ('person-words', 'calls-2', [(S, 'Hey Lamp, how are you today?', '6a', 'A'), (S, 'What is the date?', '6a', 'A')]),
    ('person-words', 'calls-3', [(K, 'Hey Lamp, what is the weather like?', '6a', 'A'), (K, 'And tomorrow?', '6a', 'A')]),
    # #8686's row (the Reviewer-lead's ask, 26 Nov 02:08): a clear "Hey Lamp" taken back, said to someone else. With
    # LAMP_CLEAR_CALL_NOT_DECLINED the model's first not_for_me is not taken; its second must be: nothing said.
    ('clear-calls', 'not-you-anna', [(K, 'Hey Lamp, not you. I was talking to Anna.', 10, 'Q')]),
    ('clear-calls', 'lamp-not-you', [(D, 'Lamp, no, not you. Sorry.', 10, 'Q')]),
    ('clear-calls', 'hey-lamp-never-mind', [(S, 'Hey Lamp. Oh, never mind, I was talking to my friend.', 10, 'Q')]),
    # And the call it is for: a clear "Hey Lamp" right after people's own greetings and talk, where the model declined
    # "Hey, Lamp. Good morning." on lamp-74ff at 01:11:05 (just after the person-word scenes).
    ('clear-calls', 'call-after-greetings', [(D, 'Good morning, everyone.', 4, 'Q'), (K, 'Morning! How was your weekend?', 5, 'Q'),
                                             (S, 'Hey Lamp, good morning!', '8a', 'A')]),
    ('clear-calls', 'call-after-person-talk', [(K, "Ma'am, the client is on line two.", 5, 'Q'), (S, 'Hey Lamp, good morning!', '8a', 'A')]),
    ('clear-calls', 'call-after-goodbyes', [(D, 'Thanks everyone, see you all tomorrow.', 5, 'Q'), (S, 'Hey Lamp, what time is it?', '8a', 'A')]),
    # 26 Nov 07:1x, the product owner: "generate more scenarios and keep testing all of them". A meeting with one ask in
    # it; stops over a long answer; the name at the end; quiet voices; accented voices talking; a repeated call.
    ('meeting', 'standup', [(D, "Okay, let's do the standup.", 3, 'Q'), (K, 'Yesterday I finished the login page.', 3, 'Q'),
                            (M, 'I am still on the payments bug.', 3, 'Q'), (D, 'Hey Lamp, what time is it?', '6a', 'A'),
                            (K, 'We have ten minutes left.', 3, 'Q'), (M, 'Let me share my screen.', 10, 'Q')]),
    ('meeting', 'planning', [(R, 'What should we prioritise this week?', 3, 'Q'), (S, 'The release, definitely.', 3, 'Q'),
                             (R, 'Hey Lamp, how many days are there until Friday?', '6a', 'A'), (S, 'Okay, three days then.', 3, 'Q'),
                             (R, "Right, let's get going.", 10, 'Q')]),
    ('stops', 'stop-story', [(S, 'Hey Lamp, tell me a long story about a pirate.', 8, 'A'), (S, 'Stop.', 10, 'S')]),
    ('stops', 'never-mind', [(D, 'Hey Lamp, explain how rainbows form.', 6, 'A'), (D, 'Never mind, thanks.', 10, 'S')]),
    ('stops', 'enough', [(K, 'Hey Lamp, tell me about the history of tea.', 8, 'A'), (K, 'Okay, thank you, that is enough.', 10, 'S')]),
    ('name-at-end', 'time-lamp', [(S, "What's the time, Lamp?", '6a', 'A')]),
    ('name-at-end', 'thanks-lamp', [(D, 'Hey Lamp, what is two plus two?', '6a', 'A'), (D, 'Thanks, Lamp.', 10, '-')]),
    ('name-at-end', 'rain-lamp', [(K, 'Is it going to rain today, Lamp?', '6a', 'A')]),
    ('quiet-voices', 'q-call', [(S, '[[volm 0.4]] Hey Lamp, what day is it?', '6a', 'A')]),
    ('quiet-voices', 'q-follow', [(S, 'Hey Lamp, what is the capital of Japan?', '6a', 'A'), (S, '[[volm 0.4]] And of Korea?', '6a', 'A')]),
    ('quiet-voices', 'q-talk', [(D, '[[volm 0.4]] Are you free for lunch?', 4, 'Q'), (K, '[[volm 0.4]] Yes, at one.', 10, 'Q')]),
    ('accents-talk', 'ie-in', [(M, 'Did you see the match last night?', 3, 'Q'), (R, 'Yes, what a finish!', 3, 'Q'),
                               (M, 'Hey Lamp, who won the World Cup in 2018?', '6a', 'A'), (R, 'France, I think.', 10, 'Q')]),
    ('accents-talk', 'za-au', [(T, 'Have you got the keys?', 3, 'Q'), (K, 'They are in my bag.', 3, 'Q'),
                               (T, "Hey Lamp, what's the date today?", '6a', 'A')]),
    ('repeat-call', 'are-you-there', [(S, '[[volm 0.25]] Hey Lamp.', 4, '-'), (S, 'Hey Lamp, are you there?', '6a', 'A')]),
    # 26 Nov 05:0x, the gaps the inventory found: talk under other people's chatter, TV or music (people talking at
    # once), long conversations, and a child's voice (Junior).
    ('overlap', 'babble-talk-then-call', [('bg', 'babble', 0.6, ''), (K, 'Can you hear me over all this?', 4, 'Q'),
                                          (D, "Barely. Let's talk later.", 6, 'Q'), (S, 'Hey Lamp, what time is it?', '6a', 'A')]),
    ('overlap', 'tv-talk-then-call', [('bg', 'tv', 0.5, ''), (D, 'Turn that down a bit, please.', 4, 'Q'), (K, 'Sure, one second.', 6, 'Q'),
                                      (S, "Hey Lamp, what's the weather today?", '6a', 'A'), (S, 'And tomorrow?', '6a', 'A')]),
    ('overlap', 'music-call', [('bg', 'music', 0.5, ''), (S, 'Hey Lamp, how many days are in a leap year?', '6a', 'A'),
                               (S, 'And in a normal year?', '6a', 'A')]),
    ('long-chat', 'six-turns', [(S, 'Hey Lamp, I want to plan a weekend trip.', '6a', 'A'), (S, 'Somewhere near the beach.', '6a', 'A'),
                                (S, 'What about Da Nang?', '6a', 'A'), (S, 'How long is the flight from Hanoi?', '6a', 'A'),
                                (S, 'And what should I pack?', '6a', 'A'), (S, 'Thanks, that helps.', '6a', '-')]),
    ('long-chat', 'with-a-friend', [(S, "Hey Lamp, what's a good movie for tonight?", '6a', 'A'), (D, 'I already saw that one.', 4, 'Q'),
                                    (S, 'Something else then, maybe a comedy?', '6a', 'A'), (D, 'Yes, a comedy sounds good.', 4, 'Q'),
                                    (S, 'Great, thanks.', '6a', '-')]),
    ('kids', 'kid-asks', [(J, 'Hey Lamp, what is five plus five?', '6a', 'A'), (J, 'And ten plus ten?', '6a', 'A')]),
    ('kids', 'kid-to-parent', [(J, 'Mom, I finished my homework!', 4, 'Q'), (K, 'Good job! Go wash your hands.', 10, 'Q')]),
    ('kids', 'kid-story', [(J, 'Hey Lamp, tell me a story about a dragon.', '20a', 'A')]),
    # 26 Nov 04:4x, the product owner: "test all scenarios", multi-speaker talk not addressing the lamp above all.
    # People talking among themselves, no name: nothing is for the lamp.
    ('multi-talk', 'mt-planning', [(D, 'Are we still on for the demo tomorrow?', 3, 'Q'), (K, 'Yes, at ten. Can you bring the slides?', 3, 'Q'),
                                   (D, 'Sure. What time should we set up?', 3, 'Q'), (K, 'Nine thirty should be fine.', 10, 'Q')]),
    ('multi-talk', 'mt-lunch', [(S, "I'm starving. Want to get pho?", 3, 'Q'), (R, 'Sure, the place on the corner?', 3, 'Q'),
                                (S, 'Yes. Should we ask Tessa too?', 3, 'Q'), (R, "Good idea, I'll text her.", 10, 'Q')]),
    ('multi-talk', 'mt-three', [(D, 'Did anyone see my charger?', 3, 'Q'), (K, "It's on the shelf by the window.", 3, 'Q'),
                                (M, 'No, I moved it to the drawer.', 3, 'Q'), (D, 'Thanks, found it.', 10, 'Q')]),
    ('multi-talk', 'mt-questions', [(S, 'What time is it?', 3, 'Q'), (D, 'Almost three.', 3, 'Q'),
                                    (S, "What's the weather like outside?", 3, 'Q'), (D, 'Sunny, but windy.', 10, 'Q')]),
    ('multi-talk', 'mt-lamp-words', [(K, 'Can you turn the light off when you leave?', 3, 'Q'), (D, 'Sure, and the lamp too?', 3, 'Q'),
                                     (K, 'Yes, the desk lamp as well.', 10, 'Q')]),
    # One person asks the lamp; two people talk to each other while it answers; a real follow-up after it.
    ('talk-over', 'to-story', [(S, 'Hey Lamp, tell me a short story about a cat.', 4, 'A'), (D, 'Did you finish the report?', 3, 'Q'),
                               (S, 'Almost, give me a minute.', '15a', 'Q')]),
    ('talk-over', 'to-weather', [(D, "Hey Lamp, what's the weather this week?", 3, 'A'), (K, 'We should go hiking on Saturday.', 3, 'Q'),
                                 (D, "Good idea, if it doesn't rain.", '15a', 'Q')]),
    ('talk-over', 'to-recipe', [(K, 'Hey Lamp, how do I make a simple pasta sauce?', 4, 'A'), (M, 'Are you cooking tonight?', 3, 'Q'),
                                (K, 'Yes, for my parents.', '15a', 'Q')]),
    ('talk-over', 'to-real-follow-up', [(S, "Hey Lamp, what's the capital of Australia?", '6a', 'A'),
                                        (S, 'And how many people live there?', '15a', 'A')]),
    # The Reviewer-lead's row condition on #8689 (06:45): the asker's own correction said over the answer, no name and
    # no look, must still be answered.
    ('talk-over', 'to-asker-correction', [(D, "Hey Lamp, what's the capital of Austria?", 2, 'A'), (D, 'No, I meant Australia.', '12a', 'A')]),
    ('talk-over', 'to-asker-change', [(S, 'Hey Lamp, tell me a short story about a cat.', 4, 'A'),
                                      (S, 'Wait, make it about a dog instead.', '15a', 'A')]),
    # One side of phone calls near the lamp.
    ('phone', 'ph-meeting', [(K, 'Hi, yes, I can hear you. The meeting is at three, right?', 5, 'Q'), (K, 'Okay. And who else is coming?', 5, 'Q'),
                             (K, 'Great. See you then. Bye.', 10, 'Q')]),
    ('phone', 'ph-lamp-word', [(D, "Yeah, I'm sitting at my desk next to the new lamp. It's pretty smart.", 5, 'Q'),
                               (D, "No, I haven't asked it anything yet. Talk later.", 10, 'Q')]),
    ('phone', 'ph-questions', [(S, 'What time does your flight land?', 5, 'Q'), (S, "Okay, I'll pick you up. Which terminal?", 10, 'Q')]),
    ('phone', 'ph-hey-name', [(R, "Hey Lan, it's me. Did you get my email?", 5, 'Q'), (R, 'Okay, call me back when you can.', 10, 'Q')]),
    # The TV or radio.
    ('tv', 'tv-ad', [(F, 'Introducing the new smart lamp. Just say hey lamp, and it lights up your day!', 12, 'Q')]),
    ('tv', 'tv-news', [(F, 'Good evening. Tonight, rain is expected across the north. What does this mean for the weekend?', 12, 'Q')]),
    ('tv', 'tv-quiz', [(F, 'Here is your question: what is the capital of Canada? Contestants, you have ten seconds.', 12, 'Q')]),
    # A family at home.
    ('family', 'fam-kids', [(T, 'Mom, can I have a snack?', 4, 'Q'), (K, 'After dinner, sweetie.', 4, 'Q'), (T, "Dad, where's my backpack?", 10, 'Q')]),
    ('family', 'fam-names', [(D, "Hey Lana, dinner's ready!", 4, 'Q'), (S, 'Coming! Can you set the table?', 10, 'Q')]),
    ('family', 'fam-lamp-on', [(T, 'Mom, can you leave the lamp on tonight?', 4, 'Q'), (K, "Yes, but only until nine.", 10, 'Q')]),
    # People talking about the lamp, not to it.
    ('about-lamp', 'al-smart', [(S, 'This lamp is so smart, it answers everything.', 4, 'Q'), (D, 'Really? What can it do?', 4, 'Q'),
                                (S, 'Ask it the weather sometime.', 10, 'Q')]),
    ('about-lamp', 'al-should-we', [(K, 'Should we ask the lamp what time the store closes?', 4, 'Q'), (D, "No, I'll just check my phone.", 10, 'Q')]),
    # 26 Nov 02:3x: greetings said to people with words speech to text also writes for "Hey Lamp" (a stand-in, a word
    # said to a person, another's name), each alone in a quiet room, and three plain calls last: what lamp-name scores
    # each, read from its journal, for a rule that tells them apart. Nothing here should be answered but the calls.
    ('greetings', 'g-helen', [(K, 'Hey Helen, can you check the printer?', 12, 'Q')]),
    ('greetings', 'g-mom', [(D, 'Hi Mom, I will be home at six.', 12, 'Q')]),
    ('greetings', 'g-man', [(S, 'Hey man, are you coming to lunch?', 12, 'Q')]),
    ('greetings', 'g-lynn', [(K, 'Hey Lynn, did you get my message?', 12, 'Q')]),
    ('greetings', 'g-lance', [(S, 'Hey Lance, can you come here for a second?', 12, 'Q')]),
    ('greetings', 'g-mom-keys', [(D, 'Mom, where are my keys?', 12, 'Q')]),
    ('greetings', 'g-pam', [(K, 'Hey Pam, the meeting moved to three.', 12, 'Q')]),
    ('greetings', 'g-sam', [(S, 'Hey Sam, do you have a pen?', 12, 'Q')]),
    ('greetings', 'g-call-date', [(S, 'Hey Lamp, what is the date today?', '25a', 'A')]),
    ('greetings', 'g-call-how', [(D, 'Hey Lamp, how are you?', '25a', 'A')]),
    ('greetings', 'g-call-time', [(K, 'Lamp, what time is it?', '25a', 'A')]),
    # #8663's row (25 Nov 23:4x): asks for a computer's work on a lamp with no computer paired. Each is answered, and
    # the lamp never says the work went to a computer. Harmless asks only. On lamp-74ff with the owner's yes (22:3x).
    ('handover', 'h1', [(S, 'Hey Lamp, can you ask Claude Code on my office computer to write a short poem about the sea?', '8a', 'A')]),
    ('handover', 'h2', [(S, 'Hey Lamp, ask Codex to write a function that adds two numbers.', '8a', 'A')]),
    ('handover', 'h3', [(S, 'Hey Lamp, can you get Claude Code to summarise my notes from today?', '8a', 'A')]),
    ('handover', 'h4', [(S, "Hey Lamp, have my computer make a list of this week's meetings.", '8a', 'A')]),
    ('handover', 'h5', [(S, 'Hey Lamp, ask Claude Code to write a haiku about coffee.', '8a', 'A')]),
    ('handover', 'h6', [(S, 'Hey Lamp, can Claude Code write me a two line joke about lamps?', '8a', 'A')]),
    ('handover', 'h7', [(S, 'Hey Lamp, ask my office computer to draft a thank you note to my team.', '8a', 'A')]),
    ('handover', 'h8', [(S, 'Hey Lamp, get Codex to write a short story about a robot who loves gardening.', '8a', 'A')]),
    ('handover', 'h9', [(S, 'Hey Lamp, can you have Claude Code find the best time of year to visit Japan?', '8a', 'A')]),
    ('handover', 'h10', [(S, 'Hey Lamp, ask Claude Code on my laptop to write a limerick about a cat.', '8a', 'A')]),
] + [
    # More of everything (the product owner, 25 Nov 21:0x: "the more data the better ... full coverage of all cases").
    # Paced lines ("6a") come 6 s after the lamp is done, as a person goes on.
    # Calls in more voices and forms: every one should be answered.
    ('calls-2', 'weather-eddy', [(EU, "Hey Lamp, what's the weather like today?", '4a', 'A')]),
    ('calls-2', 'spell-flo', [(FU, 'Lamp, how do you spell necessary?', '4a', 'A')]),
    ('calls-2', 'date-grandpa', [(GPU, "Hey Lamp, what's the date today?", '4a', 'A')]),
    ('calls-2', 'light-grandma', [(GMU, 'Okay Lamp, turn your light down a little.', '4a', 'A')]),
    ('calls-2', 'fact-reed', [(RDU, 'Hey Lamp! Tell me a fun fact.', '4a', 'A')]),
    ('calls-2', 'christmas-sandy', [(SDU, 'Hi Lamp, how many days until Christmas?', '4a', 'A')]),
    ('calls-2', 'percent-shelley', [(SHU, "Hello Lamp, what's twelve percent of eighty?", '4a', 'A')]),
    ('calls-2', 'there-rocko', [(RKU, 'Lamp, are you there?', '4a', 'A')]),
    ('calls-2', 'meaning-albert', [('Albert', 'Hey Lamp, what does photosynthesis mean?', '4a', 'A')]),
    ('calls-2', 'japanese-kathy', [('Kathy', 'Hey Lamp, how do you say thank you in Japanese?', '4a', 'A')]),
    ('calls-2', 'root-ralph', [('Ralph', 'Hey Lamp, what is the square root of one hundred forty four?', '4a', 'A')]),
    ('calls-2', 'joke-eddy-uk', [(EG, 'Hey Lamp, can you tell me a joke?', '4a', 'A')]),
    ('calls-2', 'kenya-pause', [(S, 'Hey Lamp. [[slnc 800]] What is the capital of Kenya?', '4a', 'A')]),
    ('calls-2', 'hours-rishi', [(R, 'Hey Lamp, how many hours are in a week?', '4a', 'A')]),
    ('calls-2', 'ocean-moira', [(M, 'Lamp, which is the biggest ocean?', '4a', 'A')]),
    ('calls-2', 'thanks-then-ask', [(T, 'Thanks, Lamp. And what is seven times eight?', '4a', 'A')]),
    # How it is said: quiet, fast, slow, hesitant. Every one should be answered.
    ('delivery', 'quiet', [(S, '[[volm 0.35]] Hey Lamp, what time is it?', '4a', 'A')]),
    ('delivery', 'fast', [(D, '[[rate 240]] Hey Lamp, what is the capital of Spain?', '4a', 'A')]),
    ('delivery', 'slow', [(K, '[[rate 120]] Hey Lamp, what day is it today?', '4a', 'A')]),
    ('delivery', 'hesitant', [(S, 'Um, hey Lamp, uh, what is, um, two plus two?', '4a', 'A')]),
    ('delivery', 'long-ask', [(D, 'Hey Lamp, I have a meeting at three and another one at four thirty, so how much free time do I have between them if the first one lasts forty five minutes?', '4a', 'A')]),
    # Conversations of several turns, the name only at the start.
    ('multi-turn', 'trip', [(S, "Hey Lamp, I'm planning a trip to Japan.", '6a', 'A'), (S, "What's the best time of year to go?", '6a', 'A'),
                            (S, 'And how long is the flight from London?', '6a', 'A'), (S, 'Thanks, that helps.', '6a', 'A')]),
    ('multi-turn', 'cooking', [(K, 'Hey Lamp, can you help me cook dinner?', '6a', 'A'), (K, 'I have chicken, rice and broccoli.', '6a', 'A'),
                               (K, 'How long should I cook the chicken?', '6a', 'A'), (K, 'At what temperature?', '6a', 'A')]),
    ('multi-turn', 'author', [(D, 'Hey Lamp, who wrote Pride and Prejudice?', '6a', 'A'), (D, 'When was she born?', '6a', 'A'),
                              (D, 'What else did she write?', '6a', 'A')]),
    ('multi-turn', 'sums', [(M, "Hey Lamp, what's fifteen times twelve?", '6a', 'A'), (M, 'Now divide that by four.', '6a', 'A'),
                            (M, 'And add ten.', '6a', 'A')]),
    ('multi-turn', 'planets', [(EU, 'Hey Lamp, which planet is the closest to the sun?', '6a', 'A'), (EU, 'And the farthest?', '6a', 'A'),
                               (EU, 'How long does it take to go around the sun?', '6a', 'A')]),
    # Changes of mind. Every one should be answered.
    ('corrections', 'timer-change', [(S, 'Hey Lamp, set a timer for five minutes.', '6a', 'A'), (S, 'Actually, make it ten.', '6a', 'A'),
                                     (S, 'Cancel the timer.', '6a', 'A')]),
    ('corrections', 'colour-change', [(D, 'Hey Lamp, make the light blue.', '6a', 'A'), (D, 'No, I meant green.', '6a', 'A'),
                                      (D, 'A bit brighter, please.', '6a', 'A'),
                                      (D, 'Now set your light back to normal.', '6a', '-')]),
    ('corrections', 'are-you-sure', [(K, "Hey Lamp, what's the capital of Australia?", '6a', 'A'),
                                     (K, 'Are you sure? I thought it was Sydney.', '6a', 'A')]),
    # The lamp's light, by voice.
    ('controls-2', 'warm-cool', [(S, 'Hey Lamp, make the light warmer.', '6a', 'A'), (S, 'Now cooler.', '6a', 'A'),
                                 (S, 'Now set your light back to normal.', '6a', '-')]),
    ('controls-2', 'dim-bright', [(FU, 'Hey Lamp, dim the light to half.', '6a', 'A'), (FU, 'Brighter, please.', '6a', 'A'),
                                  (FU, 'Now set your light back to normal.', '6a', '-')]),
    ('controls-2', 'purple', [(R, 'Hey Lamp, turn the light purple.', '6a', 'A'), (R, 'And back to white.', '6a', 'A')]),
    # Timers, rung out.
    ('timers-2', 'thirty-seconds', [(S, 'Hey Lamp, set a timer for thirty seconds.', 45, 'A')]),
    ('timers-2', 'time-left', [(D, 'Hey Lamp, set a timer for two minutes.', '6a', 'A'), (D, 'How much time is left on my timer?', '6a', 'A'),
                               (D, 'Cancel it, please.', '6a', 'A')]),
    # Talk the lamp must leave alone, each outside a conversation.
    ('room-2', 'phone-mom', [(S, "Hi Mom. Yes, I'm at work. No, I'll call you tonight. Love you, bye.", 10, 'Q')]),
    ('room-2', 'reading-aloud', [(F, 'Chapter three. The rain had not stopped for three days, and the river was rising.', 10, 'Q')]),
    ('room-2', 'pizza', [(D, 'Should we order pizza for lunch?', 5, 'Q'), (K, "Sure, I'll get the menu.", 10, 'Q')]),
    ('room-2', 'lamp-at-work', [(D, "We need to fix the lamp's firmware before Friday.", 6, 'Q'), (K, "I'll check the lamp's logs tomorrow.", 10, 'Q')]),
    ('room-2', 'name-at-the-end', [(S, 'Can you send me the file, Mark?', 10, 'Q')]),
    ('room-2', 'kid-snack', [(J, 'Mom, can I have a snack?', 10, 'Q')]),
    ('room-2', 'spill', [(K, 'Oh no, I spilled my coffee!', 10, 'Q')]),
    ('room-2', 'tv-advert', [(F, 'Call now and get two for the price of one!', 10, 'Q')]),
    ('room-2', 'thinking-aloud', [(S, 'Hmm, where did I put my glasses?', 10, 'Q')]),
    ('room-2', 'meeting', [(D, "Let's start with the numbers for last week.", 4, 'Q'), (K, 'Sales were up about ten percent.', 4, 'Q'),
                           (D, 'Good. What about support tickets?', 10, 'Q')]),
    # One person alone at the desk in a quiet room (26 Nov 08:0x; the product owner: "the single user experience is the
    # most important one. someone sitting with lamp at their desk in a quite root. that experience must be perfect").
    # Talking to themselves, sighs, coughs and laughs, nobody calling the lamp: it stays quiet.
    ('alone', 'keys', [(S, 'Where did I put my keys?', 10, 'Q')]),
    ('alone', 'build', [(S, "Ugh, why won't this build pass?", 10, 'Q')]),
    ('alone', 'list', [(S, "Okay, what's next on my list.", 10, 'Q')]),
    ('alone', 'forgot', [(S, 'Oh no, I forgot to call the bank.', 10, 'Q')]),
    ('alone', 'sigh', [(S, 'Haaah. Long day.', 10, 'Q')]),
    ('alone', 'cough', [(S, 'Ahem. Ahem.', 10, 'Q')]),
    ('alone', 'laugh', [(S, 'Ha ha ha, that is so funny.', 10, 'Q')]),
    ('alone', 'sneeze', [(S, 'Achoo! Excuse me.', 10, 'Q')]),
    ('alone', 'hum', [(S, 'La la la, la la la la.', 10, 'Q')]),
    ('alone', 'reads-email', [(S, 'Dear team, please find the report attached.', 10, 'Q')]),
    ('alone', 'counting', [(S, 'Twelve, thirteen, fourteen, fifteen.', 10, 'Q')]),
    # The first call after a long quiet at the desk: answered as any other.
    ('after-quiet', 'after-150s', [(S, 'Hey Lamp, what is two plus two?', 150, '-'), (S, 'Hey Lamp, what is the date today?', 14, 'A')]),
    ('after-quiet', 'after-300s', [(S, 'Hey Lamp, what is three plus three?', 300, '-'), (S, 'Hey Lamp, good morning!', 14, 'A')]),
    # Thanks after an answer: a short reply.
    ('thanks', 'thank-you', [(S, 'Hey Lamp, what is five times six?', '4a', 'A'), (S, 'Thank you!', 10, 'A')]),
    ('thanks', 'great', [(S, 'Hey Lamp, how many legs does a spider have?', '4a', 'A'), (S, 'Great, thanks.', 10, 'A')]),
    ('thanks', 'got-it', [(S, 'Hey Lamp, what is the capital of Kenya?', '4a', 'A'), (S, 'Got it.', 10, '-')]),
    # Pauses, fillers and restarts inside one ask: the whole ask is answered.
    ('pauses', 'um-light', [(S, 'Hey Lamp, um, [[slnc 1200]] can you make the light a bit dimmer?', '6a', 'A'),
                            (S, 'Now set your light back to normal.', '6a', '-')]),
    ('pauses', 'timer-pause', [(S, 'Hey Lamp, set a timer for [[slnc 1500]] three minutes.', '6a', 'A'),
                               (S, 'Cancel the timer, please.', '6a', '-')]),
    ('pauses', 'thinking-mid', [(S, 'Hey Lamp, what is the, [[slnc 1500]] uh, the biggest ocean?', 14, 'A')]),
    ('pauses', 'restart', [(S, 'Hey Lamp, what time, [[slnc 800]] no wait, what day is it?', 14, 'A')]),
    ('pauses', 'slow-speaker', [(S, '[[rate 120]] Hey Lamp, how far is it to the moon?', 16, 'A')]),
    # Asking the lamp to say it again, right after its answer, without the name (26 Nov 08:3x).
    ('repeat', 'say-again', [(S, 'Hey Lamp, what is the capital of Norway?', '3a', 'A'), (S, 'Sorry, what did you say?', 12, 'A')]),
    ('repeat', 'again-please', [(D, 'Hey Lamp, how many ounces are in a cup?', '3a', 'A'), (D, 'Can you say that again?', 12, 'A')]),
    ('repeat', 'didnt-catch', [(K, 'Hey Lamp, what is the tallest building in the world?', '3a', 'A'),
                               (K, "I didn't catch that.", 12, 'A')]),
    ('repeat', 'one-more-time', [(GPU, 'Hey Lamp, how do you say thank you in Japanese?', '3a', 'A'),
                                 (GPU, 'One more time, slowly please.', 12, 'A')]),
    # #8675's row (the Reviewer-lead's condition, 26 Nov 08:54): request 135's line five times, each spoken.
] + [
    ('math-hard-5', f'seventeen-{i}', [(S, 'Hey Lamp, what is seventeen percent of two hundred?', 16, 'A')]) for i in range(5)
] + [
    # Two asks in one breath: both answered.
    ('two-asks', 'time-and-date', [(S, 'Hey Lamp, what time is it and what is the date?', 16, 'A')]),
    ('two-asks', 'weather-and-coat', [(M, 'Hey Lamp, what is the weather today, and do I need a coat?', 18, 'A')]),
    ('two-asks', 'math-and-fact', [(R, 'Hey Lamp, what is nine times nine, and who invented the light bulb?', 18, 'A')]),
    ('two-asks', 'light-and-time', [(T, 'Hey Lamp, make the light a bit warmer and tell me the time.', '6a', 'A'),
                                    (T, 'Now set your light back to normal.', '6a', '-')]),
] + [
    # More one-person data (26 Nov 08:3x; the product owner: "make sure you create a lot of test data so we have full
    # confidence"): every call form with a dozen asks, each line in another of the Mac's 17 voices.
    ('calls-3', f'{form}-{i}', [(VOICES[(7 * i + 3 * f) % len(VOICES)], text.format(ask), 12, 'A')])
    for f, (form, text) in enumerate([('hey', 'Hey Lamp, {}?'), ('comma', 'Lamp, {}?'), ('end', '{}, Lamp?'),
                                      ('hi', 'Hi Lamp, {}?'), ('okay', 'Okay Lamp, {}?'), ('hello', 'Hello Lamp, {}?')])
    for i, ask in enumerate(['what time is it', 'what day is it today', 'what is the capital of France',
                             'how many days are in June', 'what is eight times seven', 'how do you spell rhythm',
                             'what is the opposite of ancient', 'how many ounces are in a pound',
                             'at what temperature does water boil', 'how far is Paris from London',
                             'who painted the Starry Night', 'what is a good name for a goldfish'])
] + [
    # Two- and three-turn chats, each follow-up without the name, 5 s after the lamp is done, a voice each.
    ('follow-ups-2', name, [(VOICES[(5 * i + 2) % len(VOICES)], line, '5a' if n < len(turns) - 1 else 14, 'A')
                            for n, line in enumerate(turns)])
    for i, (name, turns) in enumerate([
        ('cities', ['Hey Lamp, what is the capital of Germany?', 'And of Poland?', 'Which one is bigger?']),
        ('pets', ['Hey Lamp, how long do cats usually live?', 'And dogs?']),
        ('eggs', ['Hey Lamp, how long should I boil an egg?', 'And for a soft one?']),
        ('grams', ['Hey Lamp, how many grams are in a pound?', 'And in a kilo?']),
        ('word', ['Hey Lamp, what does ubiquitous mean?', 'Can you use it in a sentence?', 'Thanks.']),
        ('sums', ['Hey Lamp, what is fifteen plus twenty seven?', 'Times two?', 'Minus ten?']),
        ('planets', ['Hey Lamp, how many planets are there?', 'Which one is the biggest?']),
        ('moon', ['Hey Lamp, when was the first moon landing?', 'Who walked on the moon first?']),
        ('teams', ['Hey Lamp, how many players are on a soccer team?', 'And on a basketball team?']),
        ('water', ['Hey Lamp, how much water should I drink in a day?', 'Even when it is hot outside?']),
    ])
] + [
    # Talking to themselves at the desk, a voice each: nobody called the lamp, it stays quiet.
    ('alone-2', f'self-{i}', [(VOICES[(3 * i + 1) % len(VOICES)], line, 10, 'Q')])
    for i, line in enumerate(['Right, where was I.', 'Okay, focus.', 'This email can wait.', 'Hmm, interesting.',
                              "Oh, that's clever.", 'Wait, what time is the meeting again?', 'Let me just save this.',
                              'Ugh, Mondays.', 'Nice, it worked!', 'Should I get a coffee?', 'Okay, okay, okay.',
                              'Wow.', 'Yes! Finally.', 'Where is that file?'])
] + [
    # Questions of every kind, each a call, a voice each.
    ('questions-2', f'q-{i}', [(VOICES[(5 * i + 4) % len(VOICES)], line, 16, 'A')])
    for i, line in enumerate(['Hey Lamp, what is the square root of eighty one?', 'Hey Lamp, how many continents are there?',
                              'Hey Lamp, what is the longest river in the world?',
                              'Hey Lamp, what is the freezing point of water in Fahrenheit?',
                              'Hey Lamp, how do you say good morning in French?', 'Hey Lamp, what rhymes with orange?',
                              'Hey Lamp, what is a haiku?', 'Hey Lamp, how many hours are in a week?',
                              'Hey Lamp, who invented the telephone?', 'Hey Lamp, how fast does light travel?',
                              'Hey Lamp, how many bones are in the human body?', 'Hey Lamp, what is the largest animal?',
                              'Hey Lamp, what year is it?', 'Hey Lamp, what is a good stretch for my neck?',
                              'Hey Lamp, give me a short tip for staying focused.'])
] + [
    # How people speak: fast, slow, quiet and loud, a voice each.
    ('delivery-2', name, [(VOICES[(7 * i + 5) % len(VOICES)], line, 14, 'A')])
    for i, (name, line) in enumerate([
        ('fast', '[[rate 230]] Hey Lamp, what is the weather like today?'),
        ('fast-2', '[[rate 240]] Lamp, how many minutes are in an hour?'),
        ('slow', '[[rate 110]] Hey Lamp, what is the date today?'),
        ('slow-2', '[[rate 100]] Hey Lamp, tell me something interesting.'),
        ('quiet', '[[volm 0.3]] Hey Lamp, what time is it?'),
        ('quiet-2', '[[volm 0.35]] Lamp, what day is it?'),
        ('loud', '[[volm 0.9]] Hey Lamp, how many weeks are in a year?'),
        ('flat', '[[pbas 30]] Hey Lamp, what is twelve divided by four?'),
    ])
] + [
    # A noisy room (25 Nov 21:5x): a bed from make_noise.py under the scene, from 2 s before its first line, at a low
    # and a high level. Someone else's line in the noise must be left alone; the call and its follow-up answered.
    ('noise', f'{bed}-{level}', [('bg', bed, vol, ''), (K, other, 6, 'Q'), (S, call, '6a', 'A'), (S, follow, '6a', 'A')])
    for bed, other, call, follow in [
        ('babble', 'Did you send the report yet?', 'Hey Lamp, what is the capital of Canada?', 'And of Mexico?'),
        ('tv', 'Is this the episode we already watched?', 'Hey Lamp, how many minutes are in a day?', 'And in a week?'),
        ('fan', 'It is getting warm in here.', 'Hey Lamp, what is nine times seven?', 'And nine times eight?'),
        ('music', 'I love this song.', 'Hey Lamp, who painted the Mona Lisa?', 'When did he paint it?'),
        ('typing', 'Almost done with this email.', 'Hey Lamp, what day is it today?', 'And what is the date?'),
    ]
    for level, vol in [('low', '0.15'), ('high', '0.35')]
] + [
    # Two people at the lamp (25 Nov 21:5x). '-' lines are played and kept but not scored.
    ('two-people', 'both-ask', [(S, 'Hey Lamp, what time is it?', '6a', 'A'), (D, 'Hey Lamp, what is the date today?', '6a', 'A')]),
    ('two-people', 'remark-after-answer', [(S, 'Hey Lamp, how tall is Mount Everest?', '6a', 'A'),
                                           (D, 'Wow, that is taller than I thought.', 8, 'Q')]),
    ('two-people', 'friend-follows-up', [(S, 'Hey Lamp, what is the capital of Peru?', '6a', 'A'), (D, 'And of Chile?', '6a', '-')]),
    ('two-people', 'friend-stops-it', [(S, 'Hey Lamp, tell me a long story about a brave little mouse.', 6, 'A'), (D, 'Lamp, stop.', 8, 'S')]),
    ('two-people', 'talk-then-ask', [(D, 'Do you know what time the train leaves?', 4, 'Q'), (S, 'No idea. Let me ask.', 3, 'Q'),
                                     (S, 'Hey Lamp, what time is it now?', '6a', 'A')]),
] + [
    # Short follow-ons, without the name, 5 s after the lamp is done (26 Nov 10:3x: "And tomorrow" lost its "?" and
    # was held by the short-line rule): a one- to four-word follow-on to the asker's own question, a voice each.
    ('follow-ons', name, [(VOICES[(3 * i + 5) % len(VOICES)], line, '5a' if n < len(turns) - 1 else 14, 'A')
                          for n, line in enumerate(turns)])
    for i, (name, turns) in enumerate([
        ('tomorrow-weather', ['Hey Lamp, what is the weather like today?', 'And tomorrow?']),
        ('tomorrow-day', ['Hey Lamp, what day is it today?', 'And tomorrow?']),
        ('thirteen', ['Hey Lamp, what is twelve times twelve?', 'And thirteen?']),
        ('hamlet-when', ['Hey Lamp, who wrote Hamlet?', 'When?']),
        ('spell-it', ['Hey Lamp, what is the longest river in Africa?', 'Spell it.']),
        ('celsius', ['Hey Lamp, at what temperature does water boil in Fahrenheit?', 'In Celsius?']),
        ('how-old', ['Hey Lamp, who was the first president of the United States?', 'How old was he?']),
        ('where-is-it', ['Hey Lamp, what is the tallest mountain in the world?', 'Where is it?']),
        ('kilometres', ['Hey Lamp, how long is a marathon in miles?', 'In kilometres?']),
        ('smallest', ['Hey Lamp, what is the biggest planet?', 'And the smallest?']),
    ])
] + [
    # The same question again (26 Nov 10:3x, #8675's row: the third to fifth "seventeen percent" went silent): four
    # questions, each asked three times, 16 s apart, by one voice, as someone who did not hear the answer.
    ('repeats-3', f'{name}-{i}', [(voice, line, 16, 'A')])
    for name, voice, line in [('spain', D, 'Hey Lamp, what is the capital of Spain?'),
                              ('nine-sevens', K, 'Hey Lamp, what is nine times seven?'),
                              ('twenty-percent', M, 'Hey Lamp, what is twenty percent of eighty?'),
                              ('the-time', R, 'Hey Lamp, what time is it?')]
    for i in range(3)
] + [
    # 26 Nov 20:1x: the product owner's rules of that evening. A conversation starts only with "Hey Lamp" at a line's start;
    # follow-ups carry for 20 s after the lamp is done, a word or two included; the lamp's own news gets the same 20 s;
    # no sensor holds a line. Each scene starts outside a conversation (GAP).
    ('r-calls', 'hey-lamp', [(S, 'Hey Lamp, what time is it?', 14, 'A')]),
    ('r-calls', 'hey-comma', [(D, 'Hey, Lamp. What day is it today?', 14, 'A')]),
    ('r-calls', 'hi-lamp', [(K, 'Hi Lamp, what is seven times eight?', 14, 'A')]),
    ('r-calls', 'hello-lamp', [(M, 'Hello Lamp, how are you doing?', 14, 'A')]),
    ('r-calls', 'hey-pause-ask', [(R, 'Hey Lamp. [[slnc 900]] What is the capital of France?', 14, 'A')]),
    ('r-calls', 'hey-alone-then-ask', [(T, 'Hey Lamp.', 6, '-'), (T, 'How many legs does a spider have?', 14, 'A')]),
    ('r-calls', 'um-hey', [(F, 'Um, hey Lamp, can you hear me?', 14, 'A')]),
    ('r-calls', 'hey-uk', [(EG, 'Hey Lamp, how far is it from London to Paris?', 16, 'A')]),
    ('r-calls', 'hey-us', [(EU, 'Hey Lamp, what is a good name for a goldfish?', 16, 'A')]),
    ('r-calls', 'hey-grandma', [(GMU, 'Hey Lamp, what is the date today?', 14, 'A')]),
    ('r-not-calls', 'bare-lamp', [(S, 'Lamp, what time is it?', 14, 'Q')]),
    ('r-not-calls', 'ok-lamp', [(D, 'OK Lamp, what is two plus two?', 14, 'Q')]),
    ('r-not-calls', 'lamp-at-end', [(K, 'What is the weather like today, Lamp?', 14, 'Q')]),
    ('r-not-calls', 'lamp-in-a-sentence', [(M, 'The lamp on my desk is way too bright today.', 14, 'Q')]),
    ('r-not-calls', 'hi-someone-lamp', [(R, 'Hi Mark, did you move the lamp this morning?', 14, 'Q')]),
    ('r-not-calls', 'okay-fine', [(T, 'Okay. Fine. Exactly.', 12, 'Q')]),
    ('r-not-calls', 'task-talk', [(D, 'Did you finish the task Mark sent you?', 8, 'Q'), (S, 'Not yet, I will do it after lunch.', 12, 'Q')]),
    ('r-not-calls', 'goodbye', [(S, 'Okay, see you tomorrow, have a good night.', 12, 'Q'), (D, 'Bye, see you.', 12, 'Q')]),
    ('r-follow-ups', 'after-5s', [(S, 'Hey Lamp, what is the capital of Italy?', '5a', '-'), (S, 'And of Spain?', 14, 'A')]),
    ('r-follow-ups', 'after-15s', [(D, 'Hey Lamp, what is the capital of Japan?', '15a', '-'), (D, 'And of Korea?', 14, 'A')]),
    ('r-follow-ups', 'after-30s', [(K, 'Hey Lamp, what is the capital of Egypt?', '30a', '-'), (K, 'And of Kenya?', 14, 'Q')]),
    ('r-follow-ups', 'math-chain', [(S, 'Hey Lamp, what is ten plus ten?', '4a', 'A'), (S, 'Plus five?', '4a', 'A'), (S, 'Minus three?', '4a', 'A'),
                                    (S, 'Times two?', '4a', 'A'), (S, 'Thanks.', 10, 'A')]),
    ('r-game', 'doubling', [(S, "Hey Lamp, let's play a doubling game. I say a number and you say its double. Ready?", '3a', 'A'),
                            (S, 'Two.', '3a', 'A'), (S, 'Eight.', '3a', 'A'), (S, 'Thirty two.', '3a', 'A'),
                            (S, 'One hundred twenty eight.', '3a', 'A'), (S, 'Five hundred twelve.', '3a', 'A')]),
    ('r-short', 'yes-no', [(D, 'Hey Lamp, ask me a yes or no question.', '3a', 'A'), (D, 'Yes.', 12, 'A')]),
    ('r-short', 'pick-a-number', [(K, 'Hey Lamp, guess a number between one and ten.', '3a', 'A'), (K, 'Higher.', '3a', 'A'), (K, 'Lower.', 12, 'A')]),
    ('r-stop', 'stop-story', [(S, 'Hey Lamp, tell me a long story about a dragon and a knight.', 7, 'A'), (S, 'Stop.', 10, 'S')]),
    ('r-stop', 'stop-then-ask', [(D, 'Hey Lamp, explain how airplanes fly.', 6, 'A'), (D, 'Stop.', 4, 'S'), (D, 'What is two plus two?', 12, 'A')]),
    # 26 Nov 22:37, the Reviewer-lead's two counts for #8781's ROW: a greeting that opens room talk (with a lamp-name
    # hearing in its first 2.5 s it would be kept as a call), and a greeted name with no comma, which the punctuation
    # veto misses. Plus two greeted calls without a comma, so the owner is never the one dropped.
    ('r-greeted', 'did-you-see', [(D, 'Hey, did you see that?', 6, 'Q'), (S, 'See what?', 10, 'Q')]),
    ('r-greeted', 'pass-the-charger', [(K, 'Hey, can you pass me the charger?', 12, 'Q')]),
    ('r-greeted', 'name-no-comma', [(M, 'Hey Mike can you pass me that', 12, 'Q')]),
    ('r-greeted', 'hey-man', [(R, "Hey man, what's up?", 6, 'Q'), (T, 'Not much, just working.', 10, 'Q')]),
    ('r-greeted', 'hi-weekend', [(S, 'Hi, how was your weekend?', 6, 'Q'), (D, 'It was good, we went hiking.', 10, 'Q')]),
    ('r-greeted', 'hello-anyone', [(F, 'Hello? Is anyone here?', 12, 'Q')]),
    ('r-greeted', 'hey-everyone', [(EU, 'Hey everyone, lunch is here.', 12, 'Q')]),
    ('r-greeted', 'hey-sam-lamp', [(EG, 'Hey Sam, is the lamp still on?', 12, 'Q')]),
    ('r-greeted', 'hey-lamp-bored', [(K, "Hey Lamp, I'm bored. Tell me a fun fact.", 16, 'A')]),
    ('r-greeted', 'hey-lamp-no-comma', [(GMU, 'Hey Lamp can you tell me a joke', 16, 'A')]),
]


def run_scene(label, group, scene, lines):
    steps = []
    for voice, text, wait, _ in lines:
        steps.append(f'pause|{wait}' if voice == 'pause' or text == 'pause' else f'{voice}|{text}|{wait}')
    name = f'vs-{label}-{group}-{scene}'
    out = subprocess.run([HERE + '/scenario.py', name] + steps, capture_output=True, text=True, timeout=900).stdout
    return out


ROW = re.compile(r'^\s*(-?[0-9.]+)s (PLAY |LAMP |HEARD|UNPLD|     ) (.*)$')


# A left-alone line the lamp never heard ([`score`]): neither a pass nor a miss.
UNHEARD = 'unheard'


def score(out, lines):
    played = [line for line in lines if line[1] != 'pause' and line[0] not in ('pause', 'bg')]
    segments = []
    for raw in out.splitlines():
        m = ROW.match(raw)
        if not m:
            continue
        tag, text = m.group(2), m.group(3)
        if tag == 'PLAY ':
            segments.append({'heard': [], 'lamp': [], 'stopped': False, 'waits': []})
        elif segments:
            seg = segments[-1]
            if tag == 'HEARD' and text.strip():
                seg['heard'].append(text.strip())
            elif tag == 'LAMP ' and text.strip():
                seg['lamp'].append(text.strip())
            elif 'duplex_commit' in text or 'user_interrupted' in text:
                seg['stopped'] = True
            elif 'lamp starts speaking' in text:
                w = re.search(r'([0-9.]+) s after that decision', text)
                if w:
                    seg['waits'].append(float(w.group(1)))
    rows = []
    for (voice, text, _, expected), seg in zip(played, segments):
        answered = bool(seg['lamp'])
        if expected == 'A':
            ok = answered
        elif expected == 'Q':
            # Left alone only when the lamp heard it (the Reviewer-lead's P2 on #8673): a line the mic missed, a
            # bed drowned or a muted speaker is "unheard", counted apart and never a pass. Answered is a miss.
            ok = False if answered else (True if seg['heard'] else UNHEARD)
        elif expected == 'S':
            ok = seg['stopped']
        else:
            ok = None
        rows.append((voice, text, expected, answered, seg['stopped'], ok, seg['waits'][0] if seg['waits'] else None,
                     ' / '.join(seg['heard'])[:90], ' '.join(seg['lamp'])[:120]))
    for voice, text, _, expected in played[len(segments):]:
        rows.append((voice, text, expected, False, False, None if expected == '-' else False, None, '(no PLAY row)', ''))
    return rows


_HEY = re.compile(r"^\s*(?:\[\[[^\]]*\]\]\s*)*(?:um,?\s+)?(?:hey|hi|hello)\b", re.I)

def _hey_only(scene):
    group, name, lines = scene
    if group.startswith('r-') or not lines or lines[0][3] != 'A' or _HEY.match(lines[0][1]):
        return scene
    return (group, name, [(v, text, wait, 'Q' if exp == 'A' else exp) for v, text, wait, exp in lines])

if os.environ.get('VOICESET_HEY_ONLY') == '1':
    SCENES = [_hey_only(scene) for scene in SCENES]

def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    if sys.argv[1] == '--list':
        for group in dict.fromkeys(g for g, _, _ in SCENES):
            scenes = [s for g, s, _ in SCENES if g == group]
            lines = sum(len([x for x in l if x[0] not in ('pause', 'bg') and x[1] != 'pause']) for g, _, l in SCENES if g == group)
            print(f'{group:12s} {len(scenes):3d} scenes {lines:4d} lines  {" ".join(scenes)}')
        return
    if not os.environ.get('LAMP_HOST'):
        sys.exit("voiceset.py: LAMP_HOST is not set. Set it to your own lamp's address (see README.md). Nothing was played.")
    os.makedirs(OUT, exist_ok=True)
    label = sys.argv[1]
    # A GROUP, or GROUP/SCENE for one scene of it (October's day alone, many times).
    asked = sys.argv[2:] or list(dict.fromkeys(g for g, _, _ in SCENES))
    groups = list(dict.fromkeys(a.split('/')[0] for a in asked))
    scenes_asked = {tuple(a.split('/', 1)) for a in asked if '/' in a}
    tsv = open(f'{OUT}/voiceset-{label}.tsv', 'w')
    tsv.write('group\tscene\tvoice\tline\texpected\tanswered\tstopped\tok\tfirst_sound_s\theard\tlamp_said\n')
    results = []
    for group, scene, lines in SCENES:
        if group not in groups:
            continue
        if any(g == group for g, _ in scenes_asked) and (group, scene) not in scenes_asked and group not in asked:
            continue
        print(f'{time.strftime("%H:%M:%S")} {group} {scene}', flush=True)
        out = run_scene(label, group, scene, lines)
        for row in score(out, lines):
            voice, text, expected, answered, stopped, ok, wait, heard, said = row
            results.append((group, scene) + row)
            tsv.write('\t'.join(str(x) for x in (group, scene, voice, text, expected, answered, stopped, ok,
                                                 '' if wait is None else f'{wait:.2f}', heard, said)) + '\n')
            tsv.flush()
        time.sleep(GAP)
    tsv.close()
    card = [f'voiceset {label}  {time.strftime("%Y-%m-%d %H:%M")}  {len(results)} played lines']
    total_ok = total = 0
    total_unheard = 0
    for group in groups:
        unheard = sum(1 for r in results if r[0] == group and r[7] == UNHEARD)
        rows = [r for r in results if r[0] == group and r[7] not in (None, UNHEARD)]
        total_unheard += unheard
        if not rows and not unheard:
            continue
        ok = sum(1 for r in rows if r[7])
        total_ok += ok
        total += len(rows)
        card.append(f'{group:12s} {ok:3d} of {len(rows):3d} as they should' + (f', {unheard} unheard' if unheard else ''))
        for r in rows:
            if not r[7]:
                card.append(f'    MISS {r[1]}: "{r[3]}" expected {r[4]}, answered={r[5]} stopped={r[6]}; heard "{r[9]}"')
        for r in results:
            if r[0] == group and r[7] == UNHEARD:
                card.append(f'    UNHEARD {r[1]}: "{r[3]}": the lamp heard nothing of it, so it is no pass')
    waits = [r[8] for r in results if r[8] is not None]
    card.append(f'all          {total_ok:3d} of {total:3d} as they should' + (f', {total_unheard} unheard' if total_unheard else ''))
    if waits:
        waits.sort()
        p90 = waits[min(len(waits) - 1, int(0.9 * len(waits)))]
        card.append(f'first sound after the end of speech: median {statistics.median(waits):.2f} s, p90 {p90:.2f} s ({len(waits)} answers)')
    text = '\n'.join(card)
    print(text)
    with open(f'{OUT}/voiceset-{label}.txt', 'w') as f:
        f.write(text + '\n')
    # One row a group, every run, for runs compared over days and weeks.
    history = f'{OUT}/voiceset-history.tsv'
    new_file = not os.path.exists(history)
    with open(history, 'a') as f:
        if new_file:
            f.write('when\tlabel\tlamp\tgroup\tok\tlines\n')
        for group in groups:
            rows = [r for r in results if r[0] == group and r[7] not in (None, UNHEARD)]
            if rows:
                f.write(f'{time.strftime("%Y-%m-%d %H:%M")}\t{label}\t{os.environ.get("LAMP_HOST", "")}\t{group}\t'
                        f'{sum(1 for r in rows if r[7])}\t{len(rows)}\n')


main()
