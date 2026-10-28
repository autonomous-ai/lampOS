//! Finite, private saved preparation; no filesystem, publication or authority.
//!
//! Framing is checked without allocating. Context and derived tables have no
//! second semantic parser: externally protected context and fresh closed
//! preparation must reproduce their exact bounded encodings.
use super::*;
use std::ops::Range;

const MAGIC: &[u8; 8] = b"LAMPPLAN";
const VERSION: u32 = 1;
const MOST_ENCODED: usize = 40 * 1024 * 1024;
const MOST_CONTEXT: usize = 128 * 1024;
const MOST_ROWS: usize = 2 * 1024 * 1024;
const MOST_ORDER: usize = 1024;
const MOST_OPTIONAL: usize = 64 * 1024;
const OPTIONAL: u32 = 0x8000_0000;
const CAPS: [usize; 6] = [
    MOST_CONTEXT,
    manifest::MOST_ENVELOPE_BYTES as usize,
    MOST_BUNDLE_BYTES,
    MOST_HEAD_INPUT_BYTES as usize,
    MOST_ROWS,
    MOST_ORDER,
];

/// Exact bytes and their identity, never a durable-publication acknowledgement.
pub(crate) struct SavedOrdinaryPlan {
    encoded: Vec<u8>,
    artifact: Artifact,
}

/// Verified finite data, not execution admission or first-apply qualification.
pub(crate) struct CheckedOrdinaryPlan<'a> {
    saved: SavedOrdinaryPlan,
    prepared: PreparedOrdinary<'a>,
}

impl fmt::Debug for SavedOrdinaryPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SavedOrdinaryPlan")
            .field("encoded_bytes", &self.encoded.len())
            .finish()
    }
}

impl fmt::Debug for CheckedOrdinaryPlan<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckedOrdinaryPlan")
            .field("encoded_bytes", &self.saved.encoded.len())
            .field("rows", &self.prepared.rows.len())
            .field("projected_steps", &self.prepared.projected_order.len())
            .finish()
    }
}

impl SavedOrdinaryPlan {
    #[allow(
        dead_code,
        reason = "Staged private container; no storage or execution wiring"
    )]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.encoded
    }

    #[allow(
        dead_code,
        reason = "Staged private container; no storage or execution wiring"
    )]
    pub(crate) fn artifact(&self) -> &Artifact {
        &self.artifact
    }
}

impl CheckedOrdinaryPlan<'_> {
    #[allow(
        dead_code,
        reason = "Staged private container; no storage or execution wiring"
    )]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.saved.as_bytes()
    }

    #[allow(
        dead_code,
        reason = "Staged private container; no storage or execution wiring"
    )]
    pub(crate) fn artifact(&self) -> &Artifact {
        self.saved.artifact()
    }

    #[cfg(test)]
    pub(super) fn prepared(&self) -> &PreparedOrdinary<'_> {
        &self.prepared
    }
}

// The cap is checked before every reserve/append. A late failure returns no
// partial output. No caller-provided string or table is cloned wholesale.
struct Writer {
    bytes: Vec<u8>,
    most: usize,
}

impl Writer {
    fn new(most: usize) -> Self {
        Self {
            bytes: Vec::new(),
            most,
        }
    }
    fn put(&mut self, bytes: &[u8]) -> Result<(), Refused> {
        let length = self.bytes.len().checked_add(bytes.len()).ok_or(Refused)?;
        require(length <= self.most)?;
        self.bytes.try_reserve(bytes.len()).map_err(|_| Refused)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    fn u8(&mut self, value: u8) -> Result<(), Refused> {
        self.put(&[value])
    }
    fn u32(&mut self, value: u32) -> Result<(), Refused> {
        self.put(&value.to_le_bytes())
    }
    fn u64(&mut self, value: u64) -> Result<(), Refused> {
        self.put(&value.to_le_bytes())
    }
    fn index(&mut self, value: usize) -> Result<(), Refused> {
        self.u32(u32::try_from(value).map_err(|_| Refused)?)
    }
    fn blob(&mut self, bytes: &[u8], most: usize) -> Result<(), Refused> {
        require(bytes.len() <= most)?;
        // Include the prefix in the admission before either append.
        require(
            self.bytes
                .len()
                .checked_add(8)
                .and_then(|n| n.checked_add(bytes.len()))
                .is_some_and(|n| n <= self.most),
        )?;
        self.u64(u64::try_from(bytes.len()).map_err(|_| Refused)?)?;
        self.put(bytes)
    }
    fn optional_text(&mut self, text: Option<&str>, most: usize) -> Result<(), Refused> {
        self.u8(u8::from(text.is_some()))?;
        if let Some(text) = text {
            self.blob(text.as_bytes(), most)?;
        }
        Ok(())
    }
    fn optional_u64(&mut self, value: Option<u64>) -> Result<(), Refused> {
        self.u8(u8::from(value.is_some()))?;
        if let Some(value) = value {
            self.u64(value)?;
        }
        Ok(())
    }
    fn artifact(&mut self, value: &Artifact) -> Result<(), Refused> {
        self.put(&sha_bytes(&value.sha256)?)?;
        self.u64(value.size)
    }
}

fn sha_bytes(value: &str) -> Result<[u8; 32], Refused> {
    require(hex(value, 64))?;
    let mut result = [0; 32];
    for (out, pair) in result.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        let digit = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
        *out = (digit(pair[0]) << 4) | digit(pair[1]);
    }
    Ok(result)
}

fn context(expected: &ExpectedOrdinary<'_>) -> Result<Vec<u8>, Refused> {
    check_expected(expected)?;
    let mut out = Writer::new(MOST_CONTEXT);
    out.u32(expected.derivation)?;
    out.u8(u8::from(expected.ordinary_enabled))?;
    out.blob(expected.transaction.as_bytes(), 64)?;
    out.artifact(expected.envelope)?;
    out.blob(expected.channel.as_bytes(), 40)?;
    out.u64(expected.sequence)?;
    out.blob(expected.commit.as_bytes(), 40)?;
    out.put(&expected.trusted_key_sha256)?;
    out.blob(expected.refresh.transaction.as_bytes(), 64)?;
    out.u64(expected.refresh.sequence)?;
    for artifact in [
        &expected.refresh.envelope,
        &expected.refresh.guard,
        &expected.refresh.writer,
        &expected.refresh.previous_writer,
        &expected.refresh.product_before,
    ] {
        out.artifact(artifact)?;
    }
    out.artifact(expected.product.artifact)?;
    out.u64(expected.product.for_sequence)?;
    out.optional_u64(expected.product.installed_sequence)?;
    out.u8(u8::from(expected.product.refused))?;
    out.index(expected.product.records.len())?;
    for record in expected.product.records {
        out.blob(record.path.as_bytes(), 511)?;
        out.optional_text(record.recorded, 64)?;
        out.optional_text(record.writing, 64)?;
    }
    out.artifact(expected.preimages)?;
    out.artifact(expected.head_inputs)?;
    out.blob(expected.root.as_os_str().as_bytes(), MOST_CONTEXT_PATH)?;
    out.blob(
        expected.installed_units.as_os_str().as_bytes(),
        MOST_CONTEXT_PATH,
    )?;
    Ok(out.bytes)
}

// Action tags are exact known semantics: None=0, Write=1, Remove=2,
// Record=3, Keep=4 followed by its bounded exact reason bytes.
fn action(out: &mut Writer, value: Option<&Act>) -> Result<(), Refused> {
    match value {
        None => out.u8(0),
        Some(Act::Write) => out.u8(1),
        Some(Act::Remove) => out.u8(2),
        Some(Act::Record) => out.u8(3),
        Some(Act::Keep(reason)) => {
            out.u8(4)?;
            out.blob(reason.as_bytes(), 64)
        }
    }
}

fn rows(prepared: &PreparedOrdinary<'_>) -> Result<Vec<u8>, Refused> {
    require(prepared.rows.len() <= manifest::MOST_SETTINGS)?;
    let mut out = Writer::new(MOST_ROWS);
    out.index(prepared.rows.len())?;
    for row in &prepared.rows {
        require(row.signed_index < prepared.rows.len())?;
        out.index(row.signed_index)?;
        action(&mut out, row.initial.as_ref())?;
        out.u8(u8::from(row.effective.is_some()))?;
        if let Some(step) = &row.effective {
            let setting = &step.setting;
            out.blob(setting.path.as_bytes(), 511)?;
            out.optional_text(setting.text.as_deref(), MOST_PINS_BYTES)?;
            out.optional_text(setting.sha256.as_deref(), 64)?;
            out.optional_text(setting.mode.as_deref(), 4)?;
            out.u8(u8::from(setting.removed))?;
            action(&mut out, Some(&step.act))?;
        }
    }
    out.u8(u8::from(prepared.has_content_changes))?;
    Ok(out.bytes)
}

fn order(prepared: &PreparedOrdinary<'_>) -> Result<Vec<u8>, Refused> {
    require(prepared.projected_order.len() <= manifest::MOST_SETTINGS)?;
    let mut out = Writer::new(MOST_ORDER);
    out.index(prepared.projected_order.len())?;
    for &index in &prepared.projected_order {
        require(index < prepared.rows.len())?;
        out.index(index)?;
    }
    Ok(out.bytes)
}

fn copy(bytes: &[u8]) -> Result<Vec<u8>, Refused> {
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len()).map_err(|_| Refused)?;
    out.extend_from_slice(bytes);
    Ok(out)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], Refused> {
        let end = self.at.checked_add(count).ok_or(Refused)?;
        let value = self.bytes.get(self.at..end).ok_or(Refused)?;
        self.at = end;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32, Refused> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| Refused)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, Refused> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().map_err(|_| Refused)?,
        ))
    }
}

// No allocation, nested decode or payload copy precedes complete framing
// admission. Optional tags are nonsemantic, unique and retained verbatim.
fn frame(bytes: &[u8]) -> Result<[Range<usize>; 6], Refused> {
    require(bytes.len() <= MOST_ENCODED)?;
    let mut input = Cursor { bytes, at: 0 };
    require(input.take(8)? == MAGIC)?;
    require(input.u32()? == VERSION && input.u32()? == ORDINARY_DERIVATION)?;
    let count = usize::try_from(input.u32()?).map_err(|_| Refused)?;
    require((6..=22).contains(&count))?;
    let mut ranges = std::array::from_fn(|_| 0..0);
    let mut tags = [0; 16];
    let mut optional_bytes = 0usize;
    for i in 0..count {
        let tag = input.u32()?;
        let length = usize::try_from(input.u64()?).map_err(|_| Refused)?;
        if i < 6 {
            require(
                tag == u32::try_from(i + 1).map_err(|_| Refused)?
                    && length != 0
                    && length <= CAPS[i],
            )?;
        } else {
            let previous = &tags[..i - 6];
            require(tag >= OPTIONAL && !previous.contains(&tag))?;
            optional_bytes = optional_bytes.checked_add(length).ok_or(Refused)?;
            require(optional_bytes <= MOST_OPTIONAL)?;
            tags[i - 6] = tag;
        }
        let start = input.at;
        input.take(length)?;
        if i < 6 {
            ranges[i] = start..input.at;
        }
    }
    require(input.at == bytes.len())?;
    Ok(ranges)
}

/// Compute identity only after constructing all bytes; no self-hash cycle.
#[allow(
    dead_code,
    reason = "Staged private container; no storage or execution wiring"
)]
pub(crate) fn save(prepared: &PreparedOrdinary<'_>) -> Result<SavedOrdinaryPlan, Refused> {
    let context = context(prepared._expected)?;
    let rows = rows(prepared)?;
    let order = order(prepared)?;
    let payloads = [
        context.as_slice(),
        &prepared.envelope,
        prepared.preimages.encoded(),
        prepared.head_inputs.as_bytes(),
        &rows,
        &order,
    ];
    let mut out = Writer::new(MOST_ENCODED);
    out.put(MAGIC)?;
    out.u32(VERSION)?;
    out.u32(ORDINARY_DERIVATION)?;
    out.u32(6)?;
    for (i, payload) in payloads.into_iter().enumerate() {
        require(!payload.is_empty() && payload.len() <= CAPS[i])?;
        out.index(i + 1)?;
        out.u64(u64::try_from(payload.len()).map_err(|_| Refused)?)?;
        out.put(payload)?;
    }
    let artifact = Artifact::of(&out.bytes);
    Ok(SavedOrdinaryPlan {
        encoded: out.bytes,
        artifact,
    })
}

/// Verify original bytes, independently supplied context and full closed
/// re-preparation. Neither decoding nor equality authenticates that context.
#[allow(
    dead_code,
    reason = "Staged private container; no storage or execution wiring"
)]
pub(crate) fn reopen<'a>(
    expected: &'a ExpectedOrdinary<'a>,
    expected_plan: &Artifact,
    trusted_key: &[u8; 32],
    offered_writer: &[u8],
    encoded: Vec<u8>,
) -> Result<CheckedOrdinaryPlan<'a>, Refused> {
    require(
        encoded.len() <= MOST_ENCODED
            && known_artifact(expected_plan, MOST_ENCODED as u64)
            && expected_plan.size == encoded.len() as u64,
    )?;
    expected_plan
        .check(MOST_ENCODED as u64)
        .map_err(|_| Refused)?;
    let artifact = Artifact::of(&encoded);
    require(expected_plan.matches(&artifact))?;
    let ranges = frame(&encoded)?;
    require(encoded[ranges[0].clone()] == context(expected)?)?;
    let envelope = &encoded[ranges[1].clone()];
    let raw = &encoded[ranges[2].clone()];
    let head = &encoded[ranges[3].clone()];
    // Each bounded existing decoder checks exact externally selected identity.
    let preimages = Bundle::decode(copy(raw)?, expected.transaction, expected.preimages)
        .map_err(|_| Refused)?;
    let head_inputs = head_touch::capture::codec::decode(
        head,
        &sha_bytes(&expected.head_inputs.sha256)?,
        expected.head_inputs.size,
    )
    .map_err(|_| Refused)?;
    let prepared = prepare(
        expected,
        envelope,
        trusted_key,
        offered_writer,
        preimages,
        head_inputs,
    )?;
    require(
        encoded[ranges[4].clone()] == rows(&prepared)?
            && encoded[ranges[5].clone()] == order(&prepared)?,
    )?;
    Ok(CheckedOrdinaryPlan {
        saved: SavedOrdinaryPlan { encoded, artifact },
        prepared,
    })
}
