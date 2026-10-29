---
name: head-pat
description: When someone pets the lamp's head, a hand sliding left and right across it, the lamp answers with a short response, at most once in 10 seconds, and its base gives a small happy wiggle two seconds after the hand lifts (Head pat wiggle). Silent while asleep, through the whole setup walk, in quiet hours and while it listens, thinks or speaks.
---

# Head pat

A pet is a hand sliding left and right across the lamp's head, so its two
head pads take turns being touched alone. Controls tells it apart from a
tap and a still hand (docs/controls.md, "A pet") and says it on
`controls/pet`, only with `LAMP_HEAD_PET=on`.

When it does, the lamp answers once with a short response. By default it
says one of these lines, picked at random, exactly as written, never the one it said last:

> Mm, that's nice. / Oh, I like that. / Hehe, that tickles. / Aw, thank
> you. / That feels good. / Mmm. More of that, please. / Oh, hello to you
> too.

The response is chosen in `skill.toml`, not in code: the line (and its
words), a non-word giggle from a sound file, or a purr with a warm breath
of light. Changing it needs no pull request, only that file.

It answers once a pet and at most once in 10 seconds, so a second pet
soon after a line still gets one, and never says the same line twice in a
row. It stays silent while the
lamp sleeps (and for the pet that woke it), through the whole setup walk
(a step pending or not), in quiet hours (22:00 to 07:00, or when the time is not known),
and while the lamp listens, thinks or speaks, or within 3 seconds of it,
so a pet that stopped its words gets nothing. A pet held back is let go,
never answered late.

When the hand lifts, the lamp's base also gives a small, quick, happy
wiggle, every pet, at most once in 4 seconds: that is the skill
`head-pat-wiggle`, beside this one, since it may move while the lamp
speaks. So a pet always gets the wiggle and a line at most every 10
seconds.

A pet never puts the lamp to sleep, however long; a still hand kept on the
head still does after 3 seconds, as before.

If asked about it: "If you stroke my head from side to side, I'll notice.
I like it."
