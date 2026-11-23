//! Offline only: the body's own anti-tip arithmetic for poses read from stdin,
//! so a tool that models it (`tools/reach/body.py`) can be checked against it.
//!
//!     cargo run -q -p lamp-body --example cog_eval < poses.txt
//!
//! Each line in: five counts by servo id (base yaw, shoulder, elbow, head roll,
//! head pitch), separated by spaces. Each line out: `cog::off_axis_mm` and
//! `cog::lean_back_mm` for it, separated by a space.
use lamp_body::cog;
use std::io::{self, BufRead, Write};

fn main() -> io::Result<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let line = line?;
        let counts: Vec<f32> = line
            .split_whitespace()
            .filter_map(|word| word.parse().ok())
            .collect();
        let Ok(pose) = <[f32; 5]>::try_from(counts) else {
            writeln!(out, "bad line")?;
            continue;
        };
        writeln!(
            out,
            "{} {}",
            cog::off_axis_mm(pose),
            cog::lean_back_mm(pose)
        )?;
    }
    out.flush()
}
