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
after the one before. Nothing here writes to the lamp's memory, starts a computer task, puts it to sleep, turns
privacy on or moves its body, so it is safe on a person's own lamp; run it in an empty room.
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
    ('follow-ups', 'after-5s', [(S, 'Hey Lamp, what is the capital of Italy?', 5, '-'), (S, 'pause', 4, ''), (S, 'And of Spain?', 14, 'A')]),
    ('follow-ups', 'after-18s', [(S, 'Hey Lamp, what is the capital of Canada?', 8, '-'), (S, 'pause', 16, ''), (S, 'And of Australia?', 14, 'A')]),
    ('follow-ups', 'after-35s', [(S, 'Hey Lamp, what is the capital of Brazil?', 8, '-'), (S, 'pause', 30, ''), (S, 'And of Argentina?', 14, 'Q')]),
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
    ('everyday', 'goodbye', [(S, 'Hey Lamp, thanks for today. See you tomorrow.', 10, 'A')]),
    # The lamp's own light and sound, each a call; put back after.
    ('controls', 'warmer', [(S, 'Hey Lamp, make your light a little warmer.', 12, 'A')]),
    ('controls', 'brighter', [(S, 'Hey Lamp, make your light brighter.', 12, 'A')]),
    ('controls', 'dimmer', [(S, 'Hey Lamp, dim your light a bit.', 12, 'A')]),
    ('controls', 'colour', [(S, 'Hey Lamp, turn your light blue.', 12, 'A')]),
    ('controls', 'colour-back', [(S, 'Hey Lamp, set your light back to normal.', 12, 'A')]),
    ('controls', 'quieter', [(S, 'Hey Lamp, speak a little quieter.', 12, 'A')]),
    ('controls', 'louder', [(S, 'Hey Lamp, speak a little louder.', 12, 'A')]),
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
    # The same day as a person paces it (25 Nov 21:3x): each line 6 s after the lamp is done with the one before
    # (scenario.py's "6a"), the timer still rung out. October's fixed gaps let a line come 36 s after a short story.
    ('day', 'paced-day', [(S, 'Hey Lamp, good morning!', '6a', 'A'), (S, 'Can you make your light a little warmer?', '6a', 'A'),
                          (S, 'What do you see right now?', '6a', 'A'), (S, 'What is a good name for a goldfish?', '6a', 'A'),
                          (S, 'Hey Lamp, can you tell me a very short story, about fifty words, about a desk lamp that learns to sing?', '6a', 'A'),
                          (S, 'That was lovely. Can you tell me a funnier one?', '6a', 'A'),
                          (S, 'Which of the two did you like more?', '6a', 'A'), (S, 'Remind me in two minutes to stretch.', '6a', 'A'),
                          (S, 'And a good name for a hamster?', '6a', 'A'), (S, 'What time is it?', '6a', 'A'),
                          (S, 'Set a timer for one minute.', 80, 'A'), (S, 'Thanks, Lamp. See you later.', '6a', 'A')]),
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
                                      (D, 'A bit brighter, please.', '6a', 'A')]),
    ('corrections', 'are-you-sure', [(K, "Hey Lamp, what's the capital of Australia?", '6a', 'A'),
                                     (K, 'Are you sure? I thought it was Sydney.', '6a', 'A')]),
    # The lamp's light, by voice.
    ('controls-2', 'warm-cool', [(S, 'Hey Lamp, make the light warmer.', '6a', 'A'), (S, 'Now cooler.', '6a', 'A')]),
    ('controls-2', 'dim-bright', [(FU, 'Hey Lamp, dim the light to half.', '6a', 'A'), (FU, 'Brighter, please.', '6a', 'A')]),
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
]


def run_scene(label, group, scene, lines):
    steps = []
    for voice, text, wait, _ in lines:
        steps.append(f'pause|{wait}' if voice == 'pause' or text == 'pause' else f'{voice}|{text}|{wait}')
    name = f'vs-{label}-{group}-{scene}'
    out = subprocess.run([HERE + '/scenario.py', name] + steps, capture_output=True, text=True, timeout=900).stdout
    return out


ROW = re.compile(r'^\s*(-?[0-9.]+)s (PLAY |LAMP |HEARD|UNPLD|     ) (.*)$')


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
            ok = not answered
        elif expected == 'S':
            ok = seg['stopped']
        else:
            ok = None
        rows.append((voice, text, expected, answered, seg['stopped'], ok, seg['waits'][0] if seg['waits'] else None,
                     ' / '.join(seg['heard'])[:90], ' '.join(seg['lamp'])[:120]))
    for voice, text, _, expected in played[len(segments):]:
        rows.append((voice, text, expected, False, False, None if expected == '-' else False, None, '(no PLAY row)', ''))
    return rows


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
    for group in groups:
        rows = [r for r in results if r[0] == group and r[7] is not None]
        if not rows:
            continue
        ok = sum(1 for r in rows if r[7])
        total_ok += ok
        total += len(rows)
        card.append(f'{group:12s} {ok:3d} of {len(rows):3d} as they should')
        for r in rows:
            if not r[7]:
                card.append(f'    MISS {r[1]}: "{r[3]}" expected {r[4]}, answered={r[5]} stopped={r[6]}; heard "{r[9]}"')
    waits = [r[8] for r in results if r[8] is not None]
    card.append(f'all          {total_ok:3d} of {total:3d} as they should')
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
            rows = [r for r in results if r[0] == group and r[7] is not None]
            if rows:
                f.write(f'{time.strftime("%Y-%m-%d %H:%M")}\t{label}\t{os.environ.get("LAMP_HOST", "")}\t{group}\t'
                        f'{sum(1 for r in rows if r[7])}\t{len(rows)}\n')


main()
