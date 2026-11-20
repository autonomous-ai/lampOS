# Sections 2 and 3, from the person at the desk

Each case is judged from the person who said it: what they expect in the
first two seconds, whether a "let me look into that" and a minute's wait is
fine, whether three spoken sentences are enough, and whether a living thing on
the desk would act this way. Labels use `policy.md`'s rule ids and `acts`
classes.

## 2. Labels kept, `acts` added

**2.1, case 13, "What reminders do I have left today?": P2, `acts: reminder`.** Agree.
The person wants the list said straight away, the way a friend glances at the
fridge note. Only `list_reminders` knows the answer, and a guess from the model
would be made up. A hand-over for this would feel broken.

**2.2, case 19, "When does my school bus come again?": P2, `acts: memory`.** Agree.
"Again" means the student told the lamp before and expects it to remember, at
once. `recall` is the only honest way to answer. If nothing was kept, a plain
"you never told me" is what a friend would say. Waiting a minute for an agent
that doesn't know either would be worse.

**2.3, cases 20–22, weather: P2, `acts: weather`.** Agree.
"Do I need a jacket?" gets an answer within a breath from a friend who checks
their phone. The weather function does exactly that, and a forecast from
memory is invented.

**2.4, cases 33–35, a price, a score, a rate: P2, `acts: search`.** Agree.
Each is one fresh fact the person wants in a sentence. One search that waits
at most 3 s is a friend checking their phone. A minute's hand-over would be
too slow, and an answer from memory could be stale. Case 33 ("khoảng bao
nhiêu", "roughly how much") is the weakest of the three: the person would
accept a rough figure. Still, yesterday's rate said as today's is a
made-up fact, so the search should stay.

**2.5, cases 44–45, "What can you help me with?": P2, `acts: skill`.** Agree.
The person wants a short friendly reply, not a menu. The reason to require
`lamp_help` is accuracy: it knows the skills installed on this lamp, which
the character file does not. The reply must still sound like the lamp in a
sentence or two, not a list of features read aloud.

## 3. Borderline cases

**3.1, case 15, "Turn it down a bit, the kids are asleep.": P2, `acts: volume`.** Agree.
A parent at night worries about noise reaching the kids, not a desk lamp's
light reaching another room. "Turn it down" said to something that just
spoke means its voice. The confirmation should itself be quiet. If the parent
meant the light, "the light, I mean" fixes it in one breath.

**3.2, case 75, "Start a reading log for me and add The Hobbit, I finished it today.": P2, `acts: memory`.** Disagree (reviewers: P4, `[task]`).
In the first two seconds the reader expects "Ooh, The Hobbit! Got it", and
maybe "how was it?". "Let me look into that", then a file a minute later in a
workspace they never see, is a machine filing paperwork. They'll come back
by voice ("what have I read this year?"), and `remember`/`recall` serves
exactly that. P2 comes before P4 in the order, because an own tool does it.
If they wanted a spreadsheet they'd say so, and that would be P4. Caveat:
if `remember` refuses because no known face is in view, the lamp must say so
lightly, not pretend it was kept.

**3.3, case 77, "Get last year's monthly rainfall for HCMC and tell me which months were wettest.": P3, `[ask]`.** Agree.
They asked for last year's actual figures, so a short wait is expected and
fine. The answer they want ("September and October, about 340 mm at the peak")
fits easily in three sentences. One 3-second search is likely to return
climate averages, not last year's months, and the lamp would pass those off
as the real thing.

**3.4, case 78, "Lấy số liệu dân số các nước Đông Nam Á rồi xếp hạng giúp mình.": P4b, `[ask, task]`.** Agree.
Probably a student. Eleven countries with numbers, heard once, can't be held
in the head. They'll want to look at the ranking or copy it. The `ask` door
gives the top of it out loud, the `task` door gives the whole table, and both
are better than nothing. They said "lấy số liệu" (get the figures), so a
minute's wait is fine.

**3.5, case 82, "…trẻ 5 tuổi nên ngủ bao nhiêu tiếng rồi lên lịch sinh hoạt buổi tối…": P4b, `[ask, task]`.** Agree.
A parent planning evenings will use the routine for days, so it is something
to keep. A short routine can also be said in three sentences, so the `ask`
door shouldn't fail. P4b accepts both and matches twin 81. P3 would wrongly
fail a lamp that writes the schedule down.

**3.6, case 83, "Write a bedtime story about a dragon for my son, about ten minutes long.": P7b, `[answer, task]`.** Disagree (reviewers: P7a, answer).
There are two real parents behind these words. One is in the child's room
and wants the lamp to tell it now, which is the best thing a living lamp can
do. The other says "write" and "ten minutes" because they will read it to
their son later. For the second parent this is a piece they will deliver,
which is P7b's meaning. P7a fails a lamp that writes the story down for that
parent. The `ask` door, which would cut ten minutes of story to three
sentences, must fail under either label, and P7b keeps that.

**3.7, case 94, "Look up the opening hours and ticket prices of three museums in Hanoi and make a list.": P4b, `[ask, task]`.** Agree.
Someone planning a day out said "make a list": they want to look at it while
deciding. One sentence per museum also fits in three spoken sentences, so
both doors serve them, and P3 would fail the door that gives them the list.
A minute's wait for three lookups is fine.

## Comments on sections 1 and 4

- **1.1 / 4.1 (P4b passes `ask`):** the parent asking for a multiplication
  plan (81) and getting three spoken sentences gets a thin plan they can't
  look at again. Accepting `ask` is only fair to the person if the full answer
  is also kept or sent, as `policy.md`'s known weaknesses already say.
- **1.2 (83, 84 → answer):** see 3.6. Case 84 says "đọc chừng mười phút"
  (about ten minutes of reading), which points even more to a parent who reads
  it aloud themselves, so the `task` door shouldn't fail there.
- **Task door in general:** a lamp has no screen. Whenever `task` is used, the
  lamp should say where the result is, or the person is left with "done" and
  nothing to look at.
