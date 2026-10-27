//! Independent pre-move prompt bytes for unchanged semantic tests and parity.
pub const SIT_NOTE: &str = "(Nobody has spoken. You are meeting the person you will live with for the first time. Say hello in a few warm words, then ask them to sit where they usually sit and look at you, so you learn where they are. Nothing more.)";
pub const SIT_NO_FACE: &str = "(You cannot see their face. Ask them in a few words to sit where they usually sit and look at you.)";
pub const SIT_CLOSER: &str = "(You can see them, but they are far from you. Ask them in a few words to bring you a bit closer if they like, and that where they are is fine too.)";
pub const SIT_TURN: &str = "(You can see them, but at the edge of what you see. Ask them in a few words to turn you a little toward them if they like, and that where they are is fine too.)";
pub const SIT_DONE: &str =
    "(Now you can see them well where they sit. Tell them so in a few words.)";
pub const SIT_FAILED: &str = "(You still cannot see them well where they sit. Tell them in a few light words that it is fine: they can turn you toward their seat later, or ask you to redo the setup.)";
pub const WHO_NOTE: &str = "(Nobody has spoken. Ask them, in a few words, what you should call them. When they have told you, and only if you have remember_my_face, ask whether you may remember their face, and only after a clear yes call remember_my_face with their name. Never use remember for this. Then, as soon as that is settled, call setup_step with step \"who_you_are\": done once you know their name, skipped if they would rather not say, or failed if it could not be done, and then tell them in a few words what they can try later.)";
pub const WHO_NUDGE: &str = "(If you asked whether you may remember their face and they have not answered yet, wait for their answer. Otherwise call setup_step with step \"who_you_are\" now, before you say anything else: done once you know their name, skipped if they would rather not.)";
pub const PREFERENCES_NUDGE: &str = "(They have answered. Call setup_step with step \"preferences\" now, before you say anything else.)";
pub const WHO_AGAIN: &str =
    "(That was not their name. Ask them once more, in a few words, what you should call them.)";
pub const ROOM_SOUNDS_NOTE: &str = "(Nobody has spoken. Ask them once, in a few words, whether you may notice music and laughter in the room, and say just this about it: the lamp notices music and laughter, nothing is recorded. Then call setup_step with step \"room_sounds\": done only after a clear yes, skipped if they say no or are unsure. Nothing more.)";
pub const FACE_EXPRESSIONS_NOTE: &str = "(Nobody has spoken. Ask them once, in a few words, whether you may notice whether people look happy or upset, so you can answer more warmly, and say just this about it: a yes covers everyone at this lamp, you never say it aloud, and no picture of a face is kept. Then call setup_step with step \"face_expressions\": done only after a clear yes, skipped if they say no or are unsure. Nothing more.)";
pub const TOUCH_NOTE: &str =
    "(Say only this, nothing before or after it: \"Tap the top of my head once, then let go.\")";
pub const TOUCH_OFFER: &str = "(Say only this, nothing before or after it: \"One more thing: tap the top of my head once, then let go.\")";
pub const TOUCH_REMIND: &str = "(Say only this, nothing before or after it: \"I didn't feel that. Tap a little firmer, right on top of my head.\")";
pub const TOUCH_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, I felt that.\")";
pub const TOUCH_FAILED: &str = "(Say only this, nothing before or after it: \"I didn't feel a tap this time. That's fine, you can redo the setup later.\")";
pub const PREFERENCES_NOTE: &str = "(Nobody has spoken. Ask them two short things, one at a time: whether there are hours when you should keep quiet, and whether you may say hello when they sit down. Keep each answer as a rule if you can. As soon as you have heard the second answer, call setup_step with step \"preferences\" before you say anything else, then tell them in a few words that is everything, and that they can ask you to redo the setup any time.)";
pub const PRIVACY_DONE: &str = "(Their physical switch closed the microphones, and they opened it again. Briefly confirm that the switch works. Nothing more.)";
pub const PRIVACY_DONE_CAMERA: &str = "(Their physical switch closed the microphones and the camera, and they opened it again. Briefly confirm that the switch works. Nothing more.)";
pub const PRIVACY_FAILED: &str = "(The privacy-switch check was not completed. Briefly say they can try it again later by redoing setup. Do not claim the switch was tested.)";
pub const NEAR_FIRST: &str = "(Nobody has spoken. Tell them in a few words that you would like to learn how they call you from where they usually sit. Ask them to stay in their seat and call your name once, the way they would to get your attention, and then stay quiet yourself. Do not say your own name, and do not claim anything was measured.)";
pub const NEAR_AGAIN: &str = "(Ask them in a few words to call your name once more from their seat, the same way, and then stay quiet yourself. Do not say your own name.)";
pub const NEAR_DONE: &str = "(You have learned how they call you from their seat. Tell them so in a few words. Nothing more.)";
pub const NEAR_FAILED: &str = "(You could not learn how they call you from their seat this time. Tell them in a few light words that it is fine and that they can redo the setup later. Do not claim anything was measured.)";
pub const SENTENCE_FIRST: &str = "(Nobody has spoken. Ask them in a few words to say a sentence to you from where they sit, anything at all in their own words, so you can get used to their voice, and then stay quiet yourself while they speak. Do not claim anything was measured.)";
pub const SENTENCE_LOUDER: &str = "(Ask them in a few words to say another sentence from their seat, a little louder, and then stay quiet yourself.)";
pub const SENTENCE_CLOSER: &str = "(Ask them in a few words to come a little closer and say another sentence, and then stay quiet yourself.)";
pub const SENTENCE_AGAIN: &str = "(Ask them in a few words to say another sentence from their seat, and then stay quiet yourself.)";
pub const SENTENCE_DONE: &str =
    "(You heard them well from where they sit. Tell them so in a few words. Nothing more.)";
pub const SENTENCE_FAILED: &str = "(You could not hear them well enough from where they sit this time. Tell them in a few light words: I'll get used to your voice as we talk. Do not claim anything was measured.)";
pub const GIVE_ASK: &str = "(Say only this, nothing before or after it: \"My head gives way to your hand. Take it gently, move it a little, then let go.\")";
pub const GIVE_AGAIN: &str = "(Say only this, nothing before or after it: \"I didn't feel my head move. Take it gently, move it a little, then let go.\")";
pub const GIVE_DONE: &str =
    "(Say only this, nothing before or after it: \"Got it, my head moved with your hand.\")";
pub const GIVE_FAILED: &str = "(Say only this, nothing before or after it: \"I didn't feel my head move this time. That's fine, you can redo the setup later.\")";
pub const PRIVACY_INVITATION: &str = "(Say only this one short sentence, nothing before or after it: \"Try my privacy switch {location}: close it, then open it again.\" Do not explain it, claim success or ask a tool to mark privacy done.)";
pub const WHO_CONFIRM: &str = "(If you asked whether you may remember their face and they have not answered yet, wait for their answer. Otherwise ask them, in a few words: \"Did I get that right, {name}?\" Nothing more.)";
