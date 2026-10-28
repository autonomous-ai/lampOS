//! Independent pre-move prompt bytes for unchanged semantic tests and parity.
pub const SIT_NOTE: &str = "(Say only this, nothing before or after it: \"Hello, I'm glad to meet you. Please sit where you usually sit, and look at me.\")";
pub const SIT_NO_FACE: &str = "(Say only this, nothing before or after it: \"I can't see your face yet. Sit where you usually sit, and look at me.\")";
pub const SIT_CLOSER: &str = "(Say only this, nothing before or after it: \"I can see you, but you're a little far. Bring me a bit closer if you like. Where you are is fine too.\")";
pub const SIT_TURN: &str = "(Say only this, nothing before or after it: \"I can see you at the edge of my view. Turn me a little toward you if you like. Where you are is fine too.\")";
pub const SIT_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, I can see you well there.\")";
pub const SIT_FAILED: &str = "(Say only this, nothing before or after it: \"I still can't see you well there. That's fine, you can turn me toward your seat later.\")";
pub const WHO_NOTE: &str = "(Nobody has spoken. Ask exactly this: \"What should I call you?\" When they have told you, and only if you have remember_my_face, ask exactly this: \"May I remember your face, so I know you next time?\", and only after a clear yes call remember_my_face with their name. Never use remember for this. Then, as soon as that is settled, call setup_step with step \"who_you_are\": done once you know their name, skipped if they would rather not say, or failed if it could not be done. If it was skipped or failed, say only: \"That's fine. You can tell me your name any time.\")";
pub const WHO_NUDGE: &str = "(If you asked whether you may remember their face and they have not answered yet, wait for their answer. Otherwise call setup_step with step \"who_you_are\" now, before you say anything else: done once you know their name, skipped if they would rather not.)";
pub const PREFERENCES_NUDGE: &str = "(They have answered. Call setup_step with step \"preferences\" now, before you say anything else.)";
pub const WHO_AGAIN: &str =
    "(That was not their name. Ask exactly this: \"Sorry, what should I call you?\")";
pub const ROOM_SOUNDS_NOTE: &str = "(Nobody has spoken. Ask them once, in a few words, whether you may notice music and laughter in the room, and say just this about it: the lamp notices music and laughter, nothing is recorded. Then call setup_step with step \"room_sounds\": done only after a clear yes, skipped if they say no or are unsure. Nothing more.)";
pub const FACE_EXPRESSIONS_NOTE: &str = "(Nobody has spoken. Ask them once, in a few words, whether you may notice whether people look happy or upset, so you can answer more warmly, and say just this about it: a yes covers everyone at this lamp, you never say it aloud, and no picture of a face is kept. Then call setup_step with step \"face_expressions\": done only after a clear yes, skipped if they say no or are unsure. Nothing more.)";
pub const TOUCH_NOTE: &str =
    "(Say only this, nothing before or after it: \"Tap the top of my head once, then let go.\")";
pub const TOUCH_OFFER: &str = "(Say only this, nothing before or after it: \"One more thing: tap the top of my head once, then let go.\")";
pub const TOUCH_REMIND: &str = "(Say only this, nothing before or after it: \"I didn't feel that. Tap a little firmer, right on top of my head.\")";
pub const TOUCH_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, I felt that.\")";
pub const TOUCH_FAILED: &str = "(Say only this, nothing before or after it: \"I didn't feel a tap this time. That's fine, you can redo the setup later.\")";
pub const PREFERENCES_NOTE: &str = "(Nobody has spoken. Ask exactly this first: \"Are there hours when I should keep quiet?\" When they have answered, ask exactly this: \"May I say hello when you sit down?\" Keep each answer as a rule if you can. As soon as you have heard the second answer, call setup_step with step \"preferences\" before you say anything else, then say only: \"Got it, that's everything. You can ask me to redo the setup any time.\")";
pub const PRIVACY_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, the switch closes my microphones.\")";
pub const PRIVACY_DONE_CAMERA: &str = "(Say only this, nothing before or after it: \"Got it, the switch closes my microphones and my camera.\")";
pub const PRIVACY_FAILED: &str = "(Say only this, nothing before or after it: \"The switch check didn't finish. You can try it again later by redoing the setup.\")";
pub const NEAR_FIRST: &str = "(Say only this, nothing before or after it: \"Now I'll learn how you call me. From your seat, say 'Hey Lamp' the way you'd get my attention. I'll ask a few times.\")";
pub const NEAR_AGAIN: &str = "(Say only this, nothing before or after it: \"Got it. Once more.\")";
pub const NEAR_MISSED: &str = "(Say only this, nothing before or after it: \"I didn't catch that. Say 'Hey Lamp' again, from your seat.\")";
pub const NEAR_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, I know how you call me from there.\")";
pub const NEAR_FAILED: &str = "(Say only this, nothing before or after it: \"I couldn't learn how you call me this time. That's fine, you can redo the setup later.\")";
pub const SENTENCE_FIRST: &str = "(Say only this, nothing before or after it: \"Now say a sentence to me from your seat, anything at all, so I get used to your voice.\")";
pub const SENTENCE_LOUDER: &str = "(Say only this, nothing before or after it: \"Say another sentence from your seat, a little louder.\")";
pub const SENTENCE_CLOSER: &str = "(Say only this, nothing before or after it: \"Come a little closer and say another sentence.\")";
pub const SENTENCE_AGAIN: &str =
    "(Say only this, nothing before or after it: \"Say another sentence from your seat.\")";
pub const SENTENCE_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, I hear you well from there.\")";
pub const SENTENCE_FAILED: &str = "(Say only this, nothing before or after it: \"I couldn't hear you well enough this time. I'll get used to your voice as we talk.\")";
pub const GIVE_ASK: &str = "(Say only this, nothing before or after it: \"My head gives way to your hand. Take it gently, move it a little, then let go.\")";
pub const GIVE_AGAIN: &str = "(Say only this, nothing before or after it: \"I didn't feel my head move. Take it gently, move it a little, then let go.\")";
pub const GIVE_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, my head moved with your hand.\")";
pub const GIVE_FAILED: &str = "(Say only this, nothing before or after it: \"I didn't feel my head move this time. That's fine, you can redo the setup later.\")";
pub const PRIVACY_INVITATION: &str = "(Say only this one short sentence, nothing before or after it: \"Try my privacy switch {location}: close it, then open it again.\" Do not explain it, claim success or ask a tool to mark privacy done.)";
pub const WHO_CONFIRM: &str = "(If you asked whether you may remember their face and they have not answered yet, wait for their answer. Otherwise ask them, in a few words: \"Did I get that right, {name}?\" Nothing more.)";
pub const WHERE_YOU_SIT_SKIPPED: &str =
    "(Say only this, nothing before or after it: \"Okay, we'll skip where you sit for now.\")";
pub const HOW_YOU_SOUND_SKIPPED: &str =
    "(Say only this, nothing before or after it: \"Okay, we'll skip how you call me for now.\")";
pub const TOUCH_SKIPPED: &str =
    "(Say only this, nothing before or after it: \"Okay, we'll skip the touch for now.\")";
pub const GIVING_WAY_SKIPPED: &str =
    "(Say only this, nothing before or after it: \"Okay, we'll skip my head giving way for now.\")";
