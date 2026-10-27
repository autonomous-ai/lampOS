//! Capped derived pins text for a future saved plan, never write authority.
//!
//! Ordinary callers keep their existing formatter. This internal entry point
//! owns its staged steps, so a failed evaluation returns no partial plan.
use super::{HeadInputs, project_with_formatter};
use crate::{apply::Step, manifest::Setting};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::Path,
};

/// Conservative derived-output cap, including the ordinary trailing newline.
/// This is not a bound on parsing, existing inputs or preexisting step text.
const MOST_DERIVED_BYTES: usize = 1024 * 1024;
const REFUSED: &str = "bounded head projection refused";

/// Evaluate the existing policy with a capped streaming formatter. Callers
/// still bind/authenticate inputs and bound the parser/read side independently.
/// On error, the owned staged vector is dropped rather than returned partially
/// projected. This confers no admission, storage, durability or effects.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "Future private plan preparation; no runtime caller is wired"
    )
)]
pub(crate) fn project(
    inputs: &mut dyn HeadInputs,
    root: &Path,
    units: &Path,
    carried: &BTreeMap<String, String>,
    settings: &[Setting],
    mut steps: Vec<Step>,
) -> Result<Vec<Step>, String> {
    project_with_formatter(
        inputs,
        root,
        units,
        carried,
        settings,
        &mut steps,
        |object| text(object, MOST_DERIVED_BYTES),
    )
    .map_err(|_| REFUSED.to_owned())?;
    Ok(steps)
}

struct Output {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|next| *next <= self.limit)
            .ok_or_else(|| io::Error::other(REFUSED))?;
        if self.bytes.capacity() < next {
            let capacity = self
                .bytes
                .capacity()
                .max(1024)
                .saturating_mul(2)
                .min(self.limit)
                .max(next);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| io::Error::other(REFUSED))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn text(
    object: &serde_json::Map<String, serde_json::Value>,
    limit: usize,
) -> Result<String, String> {
    let mut out = Output {
        bytes: Vec::new(),
        limit,
    };
    // No intermediate pretty String: every serializer write and the newline
    // pass the same pre-reservation check. Partial bytes stay private on error.
    serde_json::to_writer_pretty(&mut out, object).map_err(|_| REFUSED.to_owned())?;
    out.write_all(b"\n").map_err(|_| REFUSED.to_owned())?;
    String::from_utf8(out.bytes).map_err(|_| REFUSED.to_owned())
}

#[cfg(test)]
mod tests;
