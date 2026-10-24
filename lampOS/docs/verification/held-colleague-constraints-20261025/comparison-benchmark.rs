mod same_voice {
pub fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.iter().chain(b).any(|value| !value.is_finite()) {
        return None;
    }
    let (mut dot, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = (f64::from(x), f64::from(y));
        dot += x * y;
        aa += x * x;
        bb += y * y;
    }
    if aa == 0.0 || bb == 0.0 {
        return None;
    }
    Some((dot / (aa.sqrt() * bb.sqrt())).clamp(-1.0, 1.0) as f32)
}

}
const CARRY_NEARER_BY: f32 = 0.10;
#[path = "/private/tmp/third-carry-colleague-window/lampOS/crates/voice/src/second_provider/worker/held_colleagues.rs"]
mod held_colleagues;
struct CachedVoice { current: Vec<f32> }
impl CachedVoice { fn print_of(&mut self, request: u64) -> Option<Vec<f32>> { (request == 100).then(|| self.current.clone()) } }
struct Old { held_colleague: Option<Option<Vec<f32>>>, voices: Option<CachedVoice> }
impl Old {
    fn nearer_than_the_colleague(&mut self, request: u64, likeness: Option<f32>) -> bool {
        let Some(colleague) = self.held_colleague.clone() else {
            return true;
        };
        let other = self
            .voices
            .as_mut()
            .and_then(|voices| voices.print_of(request))
            .zip(colleague)
            .and_then(|(print, colleague)| crate::same_voice::cosine(&print, &colleague));
        let nearer = likeness
            .zip(other)
            .is_some_and(|(own, other)| own >= other + CARRY_NEARER_BY);
        if !nearer {
            eprintln!(
                "lamp-voice second: request {request}: its voice ({likeness:?}) is not nearer the conversation's than the voice held as someone else's in this carry ({other:?}), by {CARRY_NEARER_BY}: the voice alone carries nothing (LAMP_SAME_SPEAKER_CARRIES)"
            );
        }
        nearer
    }

}
fn timed(label: &str, mut check: impl FnMut() -> bool, expected: bool) {
    let at = std::time::Instant::now();
    assert_eq!(std::hint::black_box(check()), expected);
    let first = at.elapsed().as_nanos();
    let mut samples = Vec::with_capacity(10000);
    for _ in 0..10000 {
        let at = std::time::Instant::now();
        let result = std::hint::black_box(check());
        let ns = at.elapsed().as_nanos();
        assert_eq!(result, expected);
        samples.push(ns);
    }
    let mut ordered=samples.clone(); ordered.sort_unstable();
    println!("{} first_ns={} p50_ns={} p95_ns={} p99_ns={} max_ns={} raw_ns={:?}",label,first,ordered[5000],ordered[9500],ordered[9900],ordered[9999],samples);
}
fn main() {
    let mut owner=vec![0.0;192];owner[0]=1.0;
    let mut visitor=vec![0.0;192];visitor[1]=1.0;
    let mut old=Old { held_colleague: Some(Some(visitor.clone())), voices: Some(CachedVoice { current: owner.clone() }) };
    timed("parent_one_known",||old.nearer_than_the_colleague(100,Some(1.0)),true);
    for count in [1,8] {
        let mut held=held_colleagues::HeldColleagues::default();held.begin(1);
        for request in 2..2+count { held.hold(request,Some(visitor.clone())); }
        timed(if count==1 {"candidate_one_known"} else {"candidate_eight_known"},||held.nearer(100,Some(1.0),|of|(of==100).then(||owner.clone())),true);
    }
    let mut held=held_colleagues::HeldColleagues::default();held.begin(1);
    for request in 2..10 {held.hold(request,None);}
    let mut maximum_reads=0;
    timed("candidate_eight_missing",||{
        let mut reads=0;
        let answer=held.nearer(100,Some(1.0),|of|{reads+=1;(of==100).then(||owner.clone())});
        assert!(reads<=9);maximum_reads=maximum_reads.max(reads);answer
    },false);
    println!("maximum_missing_cache_lookups={maximum_reads}");
}
