<!-- Adapted from V1's skills/skill-creator/SKILL.md (autonomous-ai/lampOS, main), Apache License 2.0 (its LICENSE.txt). Changed for the lamp: one skill, drafted, checked with the lamp's own check and fixed in this folder; V1's evals, benchmarks and viewer left out. -->
# Making a lamp skill

You make one skill for this lamp from what a person said. The file rules.md in
this folder holds the skill's form and the person's request; where it and this
guide differ, rules.md wins.

## 1. Capture the intent

Take the intent from the person's words in rules.md alone: there is no one to
ask. Answer for yourself:

1. What should the lamp do? Only the steps rules.md lists (a light, a gesture,
   words said, a cue, and any other it offers).
2. When should it do it? The one trigger rules.md lists that fits, with the
   times of day only when the person named them.
3. What did they ask that the form can't do? Leave it out and name it on
   rules.md's left-out line; never invent a field.

If rules.md says a skill like this can't be made, answer its one line and stop.

A skill does nothing that would surprise the person who asked for it.

## 2. Write a draft

Make the folder draft/NAME in this folder, NAME the skill's name: lowercase
letters, digits and hyphens. Write in it, as rules.md says:

- skill.toml: kind, version, permissions, the trigger, ask and its steps.
- SKILL.md: front matter with name (NAME, the same as the folder) and a
  one-line description, then a few plain lines on what it does and when.
- scripts/ only when rules.md offers code, and only the files it allows.

## 3. Check it with the lamp's own check

From this folder run, with NAME the draft's name, this and no other command:

    /opt/lampOS/skills/current/lamp-skills pack --out /tmp/lamp-check draft/NAME

"packed" means the lamp's check passed. Anything else is its refusal: read its
words, fix the draft and run the check again. Fix it at most twice, then answer
with the best draft.

## 4. Answer

Write answer.md in this folder: each of the draft's files as its own fenced
block, as rules.md says, and its left-out line when something was left out.
Nothing else goes in answer.md. The lamp checks the answer once more; if it
refuses it, a new make gives you its words in rules.md, and the same steps.
