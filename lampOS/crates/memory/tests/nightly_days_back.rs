//! The days a forget marks for the night (`lamp_memory::nightly::DAYS_BACK`)
//! are the days a pass may still run for (`lamp_voice::nightly::DAYS_BACK`):
//! the memory mirrors the voice's number, since lamp-voice never links it
//! (Thirteen's P2 on #5345). A change to either fails here.
#[test]
fn a_forget_marks_every_day_the_voice_may_still_pass() {
    assert_eq!(
        i64::from(lamp_memory::nightly::DAYS_BACK),
        lamp_voice::nightly::DAYS_BACK
    );
}
