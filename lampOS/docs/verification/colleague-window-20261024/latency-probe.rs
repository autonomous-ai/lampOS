use std::{hint::black_box, time::{Duration, Instant}};
const SAME_SPEAKER_CARRIES_FOR: Duration = Duration::from_secs(20);
#[path = "/private/tmp/third-carry-colleague-window/lampOS/crates/voice/src/second_provider/worker/carry_window.rs"]
mod carry_window;
fn main() {
 let mut values=Vec::new();
 for _ in 0..100 {
  let start=Instant::now();
  for _ in 0..10_000 {
   let mut w=carry_window::CarryWindow::default();
   w.begin(black_box(1)); w.answered(1,60_000_000);
   w.remember(2,65_000_000,100_000_000);w.held(2);
   assert!(black_box(w.extra_allows(black_box(85_000_000))));
   assert!(!black_box(w.extra_allows(black_box(85_000_001))));
   w.clear();
  }
  values.push(start.elapsed().as_nanos() as f64 / 10_000.0);
 }
 values.sort_by(f64::total_cmp);
 println!("100 batches of 10000 complete policy cycles; nanoseconds per cycle: median {:.1}, p95 batch-average {:.1}, max batch-average {:.1}",values[50],values[95],values[99]);
}