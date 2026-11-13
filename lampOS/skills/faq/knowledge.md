# How to use the lamp: the product knowledge

The FAQ skill's notes (#5790). Each section below is told to the model
whole when a person asks about its topic, and nothing above the first
section, nor any line that is wholly a comment, ever is. Written from
`lampOS/docs/` (the owner's guide `docs/onboarding-guide.html` for the
app's words) as the product ships them; a fact behind a switch names it in
a comment with its product default from `docs/switches.md`, which
`crates/skills/tests/it/faq.rs` holds. A skill named in backticks is told
with whether it is on this lamp now. Only what the docs state: if unsure,
leave it out. Keep each section short: it is said in one to three
sentences.

## abilities
Say "Hey Lamp", then ask in your own words. I can:
- chat, and answer what you'd ask a friend; "Lamp, stop" stops me mid-answer.
- set timers and reminders, and tell you which you have; I say a reminder when it's due (`reminders`).
- change my light: on, off, brighter, dimmer, warmer, cooler or a colour.
- set a light scene (`light-scenes`) for reading, focus, relaxing, a movie, the night or energy.
- put on a light show (`light-show`).
- point the light (`point-light`) at something on your desk in front of me.
- turn my volume up or down, or mute.
- look things up, like tomorrow's weather.
<!-- LAMP_SECOND_SEARCH=deepseek in the product -->
- dance, when you ask (`dance`).
- read and, once you say yes, write in the accounts you link in the app, such as Gmail and Google Calendar.
<!-- LAMP_SERVICES, LAMP_SERVICES_WRITE: on in the product -->
- hand work to Claude Code or Codex on your computer, and tell you when it's done.
<!-- LAMP_TASKS_ASKED, LAMP_TASKS_SIGN_IN: on in the product -->
- learn new skills: make one by voice or in the app, or add one from the Skills Store.
On my own I say hello when you come back (`welcome-back`), give short nudges after a long sitting, droop a little when the air is stuffy (`stuffy-air`), and sleep at night (`night-sleep`).

## connectors
You link your accounts in the Autonomous app: Skills, then Connectors. The services there are Gmail, Google Calendar and Drive, Figma, GitHub, Linear, Asana, a Facebook Page, Slack, monday.com, HubSpot, Amplitude and Notion. Ahrefs can't be linked in the app yet. Spotify and other music services aren't among them yet.
<!-- LAMP_SERVICES: on in the product -->
Once one is linked, just ask me out loud, like "what's on my calendar today?". I read things to you; before I send an email or make anything, I read it back and ask, and do it only if you say yes.
<!-- LAMP_SERVICES_WRITE: on in the product -->
Linking is done in the app, never by telling me: I never ask for a password, key or code.
Removing a link in the app deletes it from the lamp, but doesn't cancel it at the service: to be sure, remove access there too.
Anyone who talks to me out loud at the lamp can use the linked accounts, so link only what you're happy to have read aloud at the desk; a text from the app can't use them.
<!-- LAMP_PRIVATE_ROOM: on in the product (lamp-voice.service): every spoken line is the owner's (services::owner_of); off, only the owner, seen and heard. A line not heard at the lamp never reaches them (services::NOT_HEARD) -->

## controls
My head:
- a tap: I listen, or wake up if I'm asleep.
- touch it while I'm talking and I stop.
- keep your hand on it for three seconds and I go to sleep.
- pet it, sliding your hand from side to side, and I wiggle and say something happy (`head-pat`).
<!-- LAMP_HEAD_TOUCH, LAMP_HEAD_PET: on in the product -->
My button:
- a short press stops me.
- hold it about two seconds and I mute.
- hold it five to fifteen seconds and let go, and I turn off; my ring glows amber while you hold.
<!-- LAMP_SHUTDOWN: on in the product -->
My privacy switch: slide it and both my microphones and my camera are off until you slide it back.
<!-- The eyes open the camera only while the voice permits the microphone (docs/eyes.md, docs/privacy-switch.md); LAMP_CAMERA_FOLLOWS_EYES: on in the product -->
<!-- LAMP_PRIVACY_CONTROLS: on in the product -->
To start over from scratch, push a paperclip into the hole marked RESET on the back of my base, beside the round POWER button, and hold it five seconds. That erases everything.

## light
Just ask: "turn the light on", "brighter", "dimmer", "make it warmer", or a colour.
<!-- LAMP_LIGHT_REQUESTS: on in the product -->
Light scenes (`light-scenes`): ask for a reading, focus, relaxing, movie, night or energising light. It stays until you change it; for reading and focus I also lean over the desk for a minute.
Light show (`light-show`): ask for a party, music or calm light show, under a minute; talk to me to stop it.
Point the light (`point-light`): "light my book", and I bend over the desk, light it and take a look; if it's off to my left or right I turn a little toward it. I come back up after two minutes. If I can't see it, move it in front of me: I don't search for it.
I can't yet keep the light on until a set time.

## reminders
Ask out loud: "set a timer for ten minutes", "remind me to call Mom at six", or "remind me every hour to stretch". A reminder can repeat every day, every week on a day you name, or every so often, from every 15 minutes up to once a day.
<!-- LAMP_SCHEDULE_TOOLS, LAMP_SCHEDULE_EVERY: on in the product -->
Ask "what reminders do I have?" to hear them, and ask me to cancel or snooze one.
When one is due I look at you and say it once (`reminders`); if I couldn't say it on time, I say it's late.
Reminders aren't in the app yet: you set them by voice.
<!-- LAMP_APP_SCHEDULE: off -->

## nudges
After a long sitting I give one short nudge at a time, never more than a couple an hour:
- a stretch after about 50 minutes (`stretch-break`).
- a sip of water (`sip-of-water`), resting your eyes (`rest-your-eyes`), standing up (`stand-up`), a short break (`comfort-break`), sitting back when you lean in (`sit-back`), and winding down in the hour before bed (`wind-down`).
<!-- LAMP_NUDGES_PRESENT: on in the product -->
To stop one, say "not today" or "stop the water reminders": I snooze it until tomorrow morning, or turn it off until you ask again. "Stop all the nudges" stops every one.
Quiet focus (`quiet-focus`) is an extra skill, not yet on every lamp. Where it's on, after about 45 minutes head down, when you look up I say a short "nicely done", and "stop the nudges" stops it too.

## guard_mode
Guard mode (`guard-mode`) is an extra skill, not yet on every lamp. Where it's on, it watches your desk while you're out: say "Lamp, keep an eye on things while I'm out", out loud at the lamp. A faint blue mark on my ring shows it's on.
It texts you when someone comes to your desk, only if texts are on for your lamp. It keeps no pictures or recordings.
<!-- LAMP_PRESENCE: on in the product; LAMP_SKILL_MESSAGES, LAMP_MESSAGES: off -->
Say "I'm back" to stop it. Sliding the privacy switch ends it too.

## skills
Add a skill: in the app, Skills, then Store, and tap Add. No Add button yet? I'm finishing my first update: look again in up to half an hour. Every skill in the Store is checked and signed. Look for things (`look-for-things`), "where are my keys?", is one there.
<!-- LAMP_SKILLS_STORE: on in the product -->
Make one by voice: "Lamp, make me a skill that dims the light at 10 pm." I make it and ask "Shall I turn it on?": only your spoken yes at the lamp turns it on.
<!-- LAMP_MAKE_SKILL: on in the product -->
Or type it in the app: Skills, then Make a skill. Skills you make can use my light and how I move.
<!-- LAMP_APP_MAKE_SKILL, LAMP_SKILL_UPLOAD: on in the product -->
Proud of one? Tap Share on it in the app, if it's on for your lamp; the team reviews it before it appears in the Store.
<!-- LAMP_SKILLS_SHARE: on in the product -->
In the app's skills page each skill has a switch to turn it off or on. I can't remove a skill by voice.
<!-- LAMP_SKILLS_PAGE: on in the product -->

## updates
I update myself: you don't need to do anything. I check about every 15 minutes and switch only at a quiet moment, when nobody has talked to me for a while, so never mid-conversation.
If something isn't right after an update, I go back to the build before by myself.
In the app, Check for update starts a check now.
<!-- LAMP_APP_UPDATE: on in the product -->

## unplug
Turn me off before you unplug me: tap the off button in the app, or hold my button five to fifteen seconds and let go. I say "Going to sleep now. Unplug me when my light turns green."
<!-- LAMP_SHUTDOWN: on in the product -->
Pulling the plug while I'm on can damage my memory card.
If an update is being installed, wait until it's done, then try again.
I can't turn myself off by voice yet.

## memory
I keep a journal of our talks and what you ask me to remember, like "remember I take my coffee black". I keep no recordings and no pictures.
To forget: "forget that" for the last thing, "forget what I said about my bike" for one subject, or "forget everything about me", and I ask once to be sure.
<!-- LAMP_MEMORY_FORGET_ALL, LAMP_MEMORY_FORGET_ABOUT: on in the product -->
Your face: I keep it only if you say yes. Say "Lamp, remember my face" any time, or ask me to forget your face.
To erase everything, the RESET hole on the back of my base, held five seconds with a paperclip, starts me over.

## setup
To set me up: plug me in where you usually sit, open the Autonomous app and tap Add Lamp, then pick your Wi-Fi and type its password in the app. Then I talk you through four short steps: where you sit, your name and face, a pet on top of my head, and saying "Hey Lamp". Say "skip" for any step.
<!-- LAMP_ONBOARDING: on in the product -->
To do it again, say "Lamp, let's redo the setup", or redo one step: "Lamp, can we redo the touch step?"
Changing my Wi-Fi from the app isn't there yet.
<!-- LAMP_SETTINGS_WIFI: off -->

## about
I'm made by Autonomous, and I run Autonomous's open-source OS for physical AI.
You can buy me on Autonomous's website, at autonomous.ai/lamp. The price and shipping are on that page.
<!-- The product page: robots/lamp/README.md, the root README.md. No price here: it can change, so the page tells it -->
