# Get your lamp working with your computer: Harness

**Draft for #5413. Not live until the Harness team has checked every line marked TO CHECK, and until a lamp build carrying PR #5379 is on stable (see "Notes for reviewers" at the end; remove that section before publishing).**

Your lamp can hand work to the AI helpers on your computer, such as Claude Code. You ask out loud. The lamp tells you when the work is done. A program called **Harness** is what carries the work from the lamp to your computer. This page shows what that is like and the three steps that set it up. It takes about ten minutes.

## What you can ask your lamp to do

You talk to the lamp as you always do. The lamp says back what it understood, and nothing starts until you say yes.

1. **Ask for a piece of work.** "Ask Claude Code to write a poem about the rain." The lamp repeats the task. You say yes. The lamp says Claude Code has it, and that it will tell you when it is done.
2. **Ask for something real from your work.** "Ask Claude Code to look at why my build is slow and tell me what it finds." The lamp gives the work to your computer.
3. **Hear how it went, without asking.** When the helper finishes, the lamp tells you at a good moment, in its own words, along with what the helper found. If the helper stops to ask you something, the lamp tells you that too. You answer it at your computer.

Good to know:

- The lamp only gives work to the helper you name. If you name one that isn't set up on your computer, the lamp says so and hands the work to nobody else.
- Nothing starts without your spoken yes to the exact task.
- The lamp does not know the work is really finished or right. It tells you what the helper reported.
- The lamp hands work to **computers**. It does not hand work to a phone. **TO CHECK (Harness team):** #5413 mentions "your computer or phone"; the lamp's code lists computers only.

## Set it up in three steps

You need your Autonomous app on your phone, a computer you want the lamp to work with, and your lamp already set up and online. You do not install anything on the lamp: it gets Harness on its own after it is online. **TO CHECK (Harness team and mobile):** this needs a lamp build that carries PR #5379.

### Step 1. Install the Harness app on your computer

Get the Harness app on the computer you want the lamp to use and open it.

- **Picture or video plan:** one screenshot of the Harness download page with the download button circled, then one of the Harness app open on its first screen. Caption: "Install Harness on your computer."
- **TO CHECK (Harness team):** the address of Harness's own install page, and which computers it supports. The only Harness address the lamp itself gives people is harness.autonomous.ai (for typing the code in step 3). No install page was found, so none is linked here.

### Step 2. Sign in to Harness

Sign in to Harness on the screen it shows you. Harness is made by the Harness team, so follow what its own sign-in asks for.

- **Picture or video plan:** one screenshot of Harness's sign-in screen, then one of the signed-in home screen. Hide your name and email in anything you publish.
- **TO CHECK (Harness team):** which account Harness asks for and how it signs in. This guide does not describe it: Harness owns it, and nothing here invents another way to sign in.

### Step 3. Connect Harness to your lamp

1. In the Autonomous app, open the Harness card for your lamp and start the connection. The app shows an 8-digit code.
2. In the Harness app on your computer, open **Machines**, then **Approve a device**, and type the code. (You can also type it at harness.autonomous.ai.)
3. The Autonomous app then asks "Sign in as" and shows your email. Choose yes if it is yours. Choose no if it is not.

That's it. Within a minute or two the lamp knows your computer, and you can ask it for work.

- If you take too long, the code runs out. Start again from the Autonomous app.
- If you answer no, or the app says it didn't work, nothing is connected. Start again from the app.
- Anyone who hears the code gets nothing from it: the lamp only connects after **you** answer yes to your own email.
- **Picture or video plan:** a 20-second screen recording of the three actions in order (the app showing the code, the code typed in Harness, the yes in the app), with the code blurred. Then one photo or clip of someone speaking to the lamp and the lamp answering, for the first real ask.
- **TO CHECK (mobile):** the exact names on the app's Harness card and sign-in screen. The app's screens are in a repository the writer of this draft could not read.

## Try it

Say: "Ask Claude Code to write a short poem about the rain." Say yes when the lamp asks. In a little while the lamp tells you how it went.

## If something doesn't work

- **The lamp says it isn't set up to hand work over yet.** Harness isn't on the lamp yet, or isn't connected. Do step 3 again.
- **The lamp says it can't find a computer.** The lamp learns your computers from Harness. Check Harness is open and signed in on the computer, and wait a minute.
- **You named a helper the lamp says isn't set up.** Set that helper up on your computer, or ask for one that is.

## Where people meet this guide

1. **In the Autonomous app, right after the lamp's setup.** Suggested words: "Your lamp can hand work to the AI helpers on your computer. Set it up in three steps." One button, "Show me", opens this guide. A second, "Not now", closes it. **TO CHECK (mobile, graythedev):** where the guide is hosted.
2. **On the app's Harness card, when the lamp has no Harness yet.** The card links to this guide, and may link to the App Store. Suggested words: "Connect your computer so the lamp can hand it work."

The lamp itself says nothing new about Harness on its own: that is out of scope for #5413 and is the lead's call.

---

## Notes for reviewers (remove before publishing)

Every claim in this guide, with where it comes from in `origin/lamp-voice` at 39fddfe02 (lampOS/ paths).

| Claim | Source |
|---|---|
| The lamp repeats a task, you say yes, nothing starts without it | docs/tasks.md:151 (spoken yes to the exact task), docs/tasks.md:74 |
| "Claude Code has it, I'll tell you when it's done" | docs/tasks.md:1735-1745 ("Handing over") |
| Lamp tells you at a good moment, unasked, how it went | docs/tasks.md:1735-1745; `LAMP_TASK_NEWS=skill` at deploy/product/lamp-voice.service:338 |
| A question from the helper is told, and you answer at your computer | docs/tasks.md:1750 and :1777 (`needs-input`, the 21 Oct case) |
| Only the named helper gets the work | deploy/product/lamp-voice.service:310 (`LAMP_TASKS_NAMED_ONLY=on`), docs/switches.md:409 |
| Computers are called by their names | `LAMP_TASKS_MACHINE_NAMES` is **off** in the product (docs/switches.md:407). Example 2 therefore does not name a computer: only the first computer is called `office` (docs/switches.md:534), and other names need that switch. **TO CHECK (voice owner):** when the names switch goes on, a line like "on my office computer" can come back |
| Completion is "unverified" | docs/tasks.md:30-34 |
| Sign-in is Harness's own, not described here | docs/tasks.md:9-15 (Tasks implements no pairing, login or credential); `HARNESS_LEAST`, crates/update/src/install.rs:854 (the lamp's code sign-in only passes the answer on). **TO CHECK (Harness team)** |
| Work goes to computers | docs/tasks.md:1030 (targets from `harness machines`), docs/switches.md:534 |
| The lamp gets Harness by itself after it is online | docs/switches.md:454 (`LAMP_HARNESS_AT_ONBOARDING`). **Off in the product today:** crates/update/src/harness_install/product.rs:42 (`PRODUCT_DEFAULT = false`). PR #5379 turns it on |
| The app signs Harness in by an 8-digit code | docs/switches.md:406 (`LAMP_TASKS_SIGN_IN_APP`), docs/cloud.md:123. **Off in the product today:** deploy/product/lamp-tasks.service:36 has no `--sign-in-by-code`. PR #5379 adds it |
| The code is typed in Harness: Machines, Approve a device, or harness.autonomous.ai | crates/voice/src/sign_in.rs:358-359, docs/tasks.md:1206 |
| The app asks "Sign in as <email>?" and takes your yes or no | docs/tasks.md:1209 and :1220-1228; docs/cloud.md:123 |
| Anyone who hears the code gains nothing | docs/tasks.md:1209-1211 ("No session exists before the answer") |
| The code runs out; start again | docs/tasks.md:1193-1195 (a code lives 10 minutes), docs/tasks.md:1224-1228 (an unanswered question ends after two minutes). Times are left out of the plain text on purpose |
| After a yes, the lamp looks at once for your computers | docs/tasks.md:1237-1238. The targets file is read again once a minute: docs/tasks.md:986-988 |
| The lamp says plain words when Harness isn't there | docs/tasks.md:1015-1020 (`handovers_off`, `no_harness`, `no_computer`, `not_signed_in`) |
| The Harness version the build installs | 0.5.39: crates/update/src/harness_install/product.rs:51; held there by `LAMP_HARNESS_HOLD_UPDATES` (off today; docs/switches.md:455). Not mentioned to customers |

### TO CHECK

- **Harness team:** Harness's own install page and supported computers (step 1); the account and sign-in screen (step 2); the exact Harness menu words "Machines" and "Approve a device" (taken from the lamp's own spoken instruction, crates/voice/src/sign_in.rs:358); whether harness.autonomous.ai is the right address to type the code at; whether a phone can be a target.
- **Mobile (graythedev):** the app's Harness card and the "after setup" screen words and buttons; the hosting place; whether the App Store link goes on the card.
- **Release:** this guide is true only for a lamp build carrying PR #5379 (Harness installed at onboarding and signed in from the app). On stable today the lamp has no Harness and no app sign-in.
- **Pictures:** none made yet. Each step has a shot plan above. No names, emails or real codes in any capture.
