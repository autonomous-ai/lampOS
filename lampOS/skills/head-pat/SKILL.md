---
name: head-pat
description: When someone pets the lamp's head, a hand sliding left and right across it, the lamp answers with a short response, at most once a minute. Silent while asleep, during setup, in quiet hours and while it listens, thinks or speaks.
---

# Head pat

A pet is a hand sliding left and right across the lamp's head, so its two
head pads take turns being touched alone. Controls tells it apart from a
tap and a still hand (docs/controls.md, "A pet") and says it on
`controls/pet`, only with `LAMP_HEAD_PET=on`.

When it does, the lamp answers once with a short response. By default it
says one of these lines, picked at random, exactly as written:

> Mm, that's nice. / Oh, I like that. / Hehe, that tickles. / Aw, thank
> you. / That feels good. / Mmm. More of that, please. / Oh, hello to you
> too.

The response is chosen in `skill.toml`, not in code: the line (and its
words), a non-word giggle from a sound file, or a purr with a warm breath
of light. Changing it needs no pull request, only that file.

It answers once a pet and at most once a minute. It stays silent while the
lamp sleeps (and for the pet that woke it), while an onboarding step is
pending, in quiet hours (22:00 to 07:00, or when the time is not known),
and while the lamp listens, thinks or speaks, or within 10 seconds of it,
so a pet that stopped its words gets nothing. A pet held back is let go,
never answered late.

A pet never puts the lamp to sleep, however long; a still hand kept on the
head still does after 3 seconds, as before.

If asked about it: "If you stroke my head from side to side, I'll notice.
I like it."
