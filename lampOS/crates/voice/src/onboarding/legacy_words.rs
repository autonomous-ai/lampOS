//! Independent pre-move prompt bytes for unchanged semantic tests and parity.
pub const SIT_NOTE: &str = "(Say only this, nothing before or after it: \"Let's get set up first! It'll only take a few minutes. Please follow my instructions step-by-step, and once we're done, you can ask me anything. Sit where you usually sit and look at me. I'll tell you when I see you.\")";
pub const SIT_OFFER: &str = "(Say only this, nothing before or after it: \"Hello, I'm glad to meet you. Sit where you usually sit and look at me. I'll tell you when I see you.\")";
pub const SIT_NO_FACE: &str = "(Say only this, nothing before or after it: \"I can't see your face yet. Sit where you usually sit, and look at me.\")";
pub const SIT_CLOSER: &str = "(Say only this, nothing before or after it: \"You're a little far. Move me closer to you, or stay as you are.\")";
pub const SIT_TURN: &str = "(Say only this, nothing before or after it: \"I can see you at the edge of my view. Turn my head toward you until I'm facing you, or stay as you are.\")";
pub const SIT_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, I can see you well there.\")";
pub const SIT_FAILED: &str = "(Say only this, nothing before or after it: \"I still can't see you well there. That's fine, you can turn me toward your seat later.\")";
pub const WHO_NOTE: &str = "(Nobody has spoken. Ask exactly this: \"What should I call you?\" When they have told you, and only if you have remember_my_face, ask exactly this: \"May I remember your face, so I know you next time?\", and only after a clear yes call remember_my_face with their name. Never use remember for this. Then, as soon as that is settled, call setup_step with step \"who_you_are\": done once you know their name, skipped if they would rather not say, or failed if it could not be done. If it was skipped or failed, say only: \"That's fine. You can tell me your name any time.\")";
pub const WHO_NUDGE: &str = "(If you asked whether you may remember their face and they have not answered yet, wait for their answer. Otherwise call setup_step with step \"who_you_are\" now, before you say anything else: done once you know their name, skipped if they would rather not.)";
pub const PREFERENCES_NUDGE: &str = "(They have answered. Call setup_step with step \"preferences\" now, before you say anything else.)";
pub const WHO_AGAIN: &str =
    "(That was not their name. Ask exactly this: \"Sorry, what should I call you?\")";
pub const ROOM_SOUNDS_NOTE: &str = "(Nobody has spoken. Ask exactly this: \"May I notice music and laughter in the room? Nothing is recorded. Just say yes or no.\" If you can't tell their answer, ask exactly this once: \"Sorry, was that a yes or a no?\" Then call setup_step with step \"room_sounds\": done only after a clear yes, skipped if they say no or are unsure. Then say only: \"Got it, I'll notice music and laughter.\" after a yes, or \"Okay, I won't.\" otherwise.)";
pub const FACE_EXPRESSIONS_NOTE: &str = "(Nobody has spoken. Ask exactly this: \"May I notice when people look happy or upset, so I can answer more warmly? A yes covers everyone at this lamp. I never say it aloud and keep no pictures. Just say yes or no.\" If you can't tell their answer, ask exactly this once: \"Sorry, was that a yes or a no?\" Then call setup_step with step \"face_expressions\": done only after a clear yes, skipped if they say no or are unsure. Then say only: \"Got it, I'll notice how people look.\" after a yes, or \"Okay, I won't.\" otherwise.)";
pub const TOUCH_NOTE: &str = "(Say only this, nothing before or after it: \"OK, next step. Tap the top of my head once, then let go.\")";
pub const TOUCH_OFFER: &str = "(Say only this, nothing before or after it: \"One more thing: tap the top of my head once, then let go.\")";
pub const TOUCH_REMIND: &str = "(Say only this, nothing before or after it: \"I didn't feel that. Touch the very top of my head with your fingertip, once.\")";
pub const TOUCH_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, I felt that.\")";
pub const TOUCH_FAILED: &str = "(Say only this, nothing before or after it: \"I didn't feel a tap this time. That's fine, you can redo the setup later. Just ask me to redo the setup.\")";
pub const PREFERENCES_NOTE: &str = "(Nobody has spoken. Ask exactly this first: \"Are there hours when I should keep quiet? For example, 'after ten at night', or 'no'.\" When they have answered, ask exactly this: \"May I say hello when you sit down?\" If you can't tell an answer, ask exactly this once: \"Sorry, could you say that again?\" Keep each answer as a rule if you can. As soon as you have heard the second answer, call setup_step with step \"preferences\" before you say anything else, then say only: \"Got it, that's everything. You can ask me to redo the setup any time.\")";
pub const PRIVACY_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, the switch closes my microphones.\")";
pub const PRIVACY_DONE_CAMERA: &str = "(Say only this, nothing before or after it: \"Got it, the switch closes my microphones and my camera.\")";
pub const PRIVACY_FAILED: &str = "(Say only this, nothing before or after it: \"The switch check didn't finish. You can try it again later by redoing the setup. Just ask me to redo the setup.\")";
pub const NEAR_FIRST: &str = "(Say only this, nothing before or after it: \"Now I'll learn how you call me. When I stop talking, say 'Hey Lamp' from your seat, the way you'd get my attention. I'll ask until I've heard you three times.\")";
pub const NEAR_AGAIN: &str =
    "(Say only this, nothing before or after it: \"Got it. Say 'Hey Lamp' once more.\")";
pub const NEAR_MISSED: &str = "(Say only this, nothing before or after it: \"I didn't catch that. Say 'Hey Lamp' again, from your seat.\")";
pub const NEAR_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, that's three.\")";
pub const NEAR_FAILED: &str = "(Say only this, nothing before or after it: \"I couldn't learn how you call me this time. That's fine, you can redo the setup later. Just ask me to redo the setup.\")";
pub const SENTENCE_FIRST: &str = "(Say only this, nothing before or after it: \"Now say any sentence to me, the way you'd talk to me.\")";
pub const SENTENCE_LOUDER: &str = "(Say only this, nothing before or after it: \"Say another sentence from your seat, a little louder.\")";
pub const SENTENCE_CLOSER: &str = "(Say only this, nothing before or after it: \"Come a little closer and say another sentence.\")";
pub const SENTENCE_AGAIN: &str =
    "(Say only this, nothing before or after it: \"Say another sentence from your seat.\")";
pub const SENTENCE_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, I hear you well from there.\")";
pub const SENTENCE_FAILED: &str = "(Say only this, nothing before or after it: \"I couldn't hear you well enough this time. I'll get used to your voice as we talk.\")";
pub const GIVE_ASK: &str = "(Say only this, nothing before or after it: \"Hold my base with one hand. With the other, tilt my head gently to one side. Let go when I say so.\")";
pub const GIVE_AGAIN: &str = "(Say only this, nothing before or after it: \"I didn't feel my head move. Hold my base with one hand. With the other, tilt my head gently to one side. Let go when I say so.\")";
pub const GIVE_FELT: &str =
    "(Say only this, nothing before or after it: \"That's it, now let go.\")";
pub const GIVE_PROMPT: &str = "(Say only this, nothing before or after it: \"Hold my base with one hand. With the other, tilt my head gently to one side, then let go.\")";
pub const GIVE_MORE: &str =
    "(Say only this, nothing before or after it: \"A little more, then let go.\")";
pub const GIVE_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, my head moved with your hand.\")";
pub const GIVE_FAILED: &str = "(Say only this, nothing before or after it: \"I didn't feel my head move this time. That's fine, you can redo the setup later. Just ask me to redo the setup.\")";
pub const PRIVACY_INVITATION: &str = "(Say only this, nothing before or after it: \"Find my privacy switch {location}. Close it, then open it again.\" Do not explain it, claim success or ask a tool to mark privacy done.)";
pub const WHO_CONFIRM: &str = "(If you asked whether you may remember their face and they have not answered yet, wait for their answer. Otherwise ask them, in a few words: \"Did I get that right, {name}?\" Nothing more.)";
pub const WHERE_YOU_SIT_SKIPPED: &str =
    "(Say only this, nothing before or after it: \"Okay, we'll skip where you sit for now.\")";
pub const HOW_YOU_SOUND_SKIPPED: &str =
    "(Say only this, nothing before or after it: \"Okay, we'll skip how you call me for now.\")";
pub const TOUCH_SKIPPED: &str =
    "(Say only this, nothing before or after it: \"Okay, we'll skip the touch for now.\")";
pub const GIVING_WAY_SKIPPED: &str =
    "(Say only this, nothing before or after it: \"Okay, we'll skip my head giving way for now.\")";
