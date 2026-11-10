//! A PDF attachment's text. PDF is untrusted input and its parser
//! (`pdf-extract` on `lopdf`) panics freely, can recurse without bound on
//! a form that draws itself, and inflates every stream it reads; so the
//! parse never runs in the service's own process. [`read`] hands the bytes
//! to a child of the same binary (`lamp-services --pdf-text`, [`child`])
//! over its standard input, one at a time (another's turn waited on at
//! most 2 s too), and kills it at [`PARSE_WITHIN`] (2 s) from its start: a
//! panic, a stack overflow, a loop or a failed allocation ends the child,
//! never the service, and the read is named "can't read this PDF".
//!
//! **The child's memory bound is a resource limit**, not the checks below:
//! on Linux its address space and data (`RLIMIT_AS`, `RLIMIT_DATA`, soft
//! and hard) are set to [`CHILD_MEMORY_MOST`] (128 MiB) in the forked child
//! before it runs this binary, and again by the child itself first thing
//! ([`cap_memory`]); a child whose limit can't be set is never started, or
//! exits before it reads anything. An allocation past it fails in the
//! child, which aborts, and the read is named "can't read this PDF". The
//! unit's system-call filter takes `setrlimit` and `prlimit64` back from
//! `~@resources` for this (`deploy/product/lamp-services.service`); with
//! no capability a limit is only ever lowered. Elsewhere (a developer's
//! Mac) there is no such limit.
//!
//! Before anything is parsed, early refusals only, none of them the bound:
//! at most [`PDF_MOST`] (2 MB) is taken (more is refused before a child is
//! started); an encrypted PDF (`/Encrypt`) is refused; and in the child,
//! its names read as `lopdf` reads them (`#xx` escapes), a PDF is refused
//! with a filter whose output isn't counted (`LZWDecode`,
//! `ASCII85Decode`), with a `/Filter` array of more than one filter, or
//! with `/Columns`, `/Colors` or `/BitsPerComponent` past [`COLUMNS_MOST`],
//! [`COLORS_MOST`] or [`BITS_MOST`] (a predictor's rows are sized by them
//! before its data is); and every stream is inflated once with a running
//! count (a `stream` keyword's spaces before its line end as `lopdf` takes
//! them), refused past [`INFLATED_MOST`] in all. Its error output goes
//! nowhere: a panic's message, which can quote the file, never reaches the
//! journal. At most [`TEXT_MOST`] characters of text come back.
use super::{TEXT_MOST, tidy};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

/// The largest PDF read, in bytes, decoded.
pub const PDF_MOST: usize = 2 << 20;
/// How long one PDF's parse may take, its child's start included.
pub const PARSE_WITHIN: Duration = Duration::from_secs(2);
/// The most all of a PDF's streams may inflate to, in bytes.
pub const INFLATED_MOST: usize = 16 << 20;
/// What a PDF too large to read is told.
pub const PDF_TOO_LARGE: &str = "this PDF is too large to read here (over 2 MB)";
/// What an encrypted, broken or too slow PDF is told.
pub const PDF_CANT_READ: &str = "can't read this PDF";
/// The child's argument.
pub const CHILD_ARG: &str = "--pdf-text";
/// The most of the child's answer read: its text in UTF-8.
const OUT_MOST: u64 = (TEXT_MOST * 4 + 64) as u64;
/// Filters whose output the inflate count doesn't bound.
const UNBOUNDED_FILTERS: &[&[u8]] = &[b"LZWDecode", b"ASCII85Decode"];
/// The child's address space and data, in bytes: the real bound on its
/// memory (Linux only), room for this binary, a 2 MB PDF, its streams
/// inflated to [`INFLATED_MOST`] and `lopdf`'s objects.
pub const CHILD_MEMORY_MOST: u64 = 128 << 20;
/// The most `/Columns` a PDF may give a predictor before it is refused.
pub const COLUMNS_MOST: u64 = 4_096;
/// The most `/Colors` a PDF may give a predictor before it is refused.
pub const COLORS_MOST: u64 = 32;
/// The most `/BitsPerComponent` a PDF may give before it is refused.
pub const BITS_MOST: u64 = 32;

/// One PDF parsed at a time.
static ONE: Mutex<()> = Mutex::new(());

/// A PDF's text, at most [`TEXT_MOST`] characters, parsed in a child
/// process of this binary; [`PDF_TOO_LARGE`] or [`PDF_CANT_READ`] when it
/// isn't read.
pub async fn read(bytes: Vec<u8>) -> Result<String, &'static str> {
    // A unit test's binary is no `lamp-services`: the parse in place,
    // panics caught. The child itself is tested from `tests/pdf_child.rs`.
    #[cfg(test)]
    {
        refused(&bytes)?;
        std::panic::catch_unwind(|| text(&bytes)).unwrap_or(Err(PDF_CANT_READ))
    }
    #[cfg(not(test))]
    {
        read_with(program(), bytes).await
    }
}

/// [`read`] with the child's program named: the service's own binary in
/// the product, the built `lamp-services` in a test.
pub async fn read_with(program: PathBuf, bytes: Vec<u8>) -> Result<String, &'static str> {
    refused(&bytes)?;
    tokio::task::spawn_blocking(move || run_child(&program, &bytes))
        .await
        .unwrap_or(Err(PDF_CANT_READ))
}

/// Why a PDF isn't handed to a child at all: too large, or encrypted.
pub fn refused(bytes: &[u8]) -> Result<(), &'static str> {
    if bytes.len() > PDF_MOST {
        return Err(PDF_TOO_LARGE);
    }
    if find(bytes, b"/Encrypt").is_some() {
        return Err(PDF_CANT_READ);
    }
    Ok(())
}

/// This binary, by `/proc/self/exe` on the lamp (still the one running
/// after an update has replaced the file), else its path.
#[cfg(not(test))]
fn program() -> PathBuf {
    let own = Path::new("/proc/self/exe");
    if own.exists() {
        return own.to_path_buf();
    }
    std::env::current_exe().unwrap_or_else(|_| own.to_path_buf())
}

/// The child started, the bytes written to it, its text read, and the
/// child killed when [`PARSE_WITHIN`] has passed.
fn run_child(program: &Path, bytes: &[u8]) -> Result<String, &'static str> {
    // Another PDF's parse first, waited on at most PARSE_WITHIN too.
    let waited = Instant::now();
    let _one = loop {
        match ONE.try_lock() {
            Ok(one) => break one,
            Err(std::sync::TryLockError::Poisoned(one)) => break one.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) if waited.elapsed() < PARSE_WITHIN => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(std::sync::TryLockError::WouldBlock) => return Err(PDF_CANT_READ),
        }
    };
    let started = Instant::now();
    let mut command = Command::new(program);
    // The child gets none of the service's environment.
    command
        .env_clear()
        .arg(CHILD_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // A limit that can't be set fails the spawn: never parsed without it.
    let mut child = limited(&mut command).spawn().map_err(|_| PDF_CANT_READ)?;
    let stdin = child.stdin.take();
    let input = bytes.to_vec();
    // A child that stops reading never holds the write past its kill.
    let writer = std::thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(&input);
        }
    });
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(stdout) = stdout {
            let _ = stdout.take(OUT_MOST).read_to_end(&mut out);
        }
        out
    });
    let finished = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if started.elapsed() < PARSE_WITHIN => {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let _ = writer.join();
    let out = reader.join().unwrap_or_default();
    if !finished {
        return Err(PDF_CANT_READ);
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// `command` with its memory capped at [`CHILD_MEMORY_MOST`] in the forked
/// child, before it runs the program ([`cap_memory`]); as is off Linux.
pub fn limited(command: &mut Command) -> &mut Command {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: between fork and exec only async-signal-safe calls are
        // made: `cap_memory` is a few `prlimit64` system calls (rustix's
        // own, no libc wrapper), with nothing allocated or locked; its
        // error is an OS error code, no allocation either.
        #[allow(unsafe_code)]
        unsafe {
            command.pre_exec(cap_memory);
        }
    }
    command
}

/// This process's address space and data capped at [`CHILD_MEMORY_MOST`],
/// soft and hard (Linux); a hard limit already lower is kept. Nothing is
/// allocated: it runs between fork and exec too.
#[cfg(target_os = "linux")]
pub fn cap_memory() -> std::io::Result<()> {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
    for resource in [Resource::As, Resource::Data] {
        let cap = Rlimit {
            current: Some(CHILD_MEMORY_MOST),
            maximum: Some(CHILD_MEMORY_MOST),
        };
        if let Err(err) = setrlimit(resource, cap) {
            // Raising a hard limit that is already lower is refused: then
            // that lower one is the bound, if the soft one is under it.
            let now = getrlimit(resource);
            if now
                .current
                .is_none_or(|current| current > CHILD_MEMORY_MOST)
            {
                return Err(err.into());
            }
        }
    }
    Ok(())
}

/// `lamp-services --pdf-text`: a PDF on standard input, its text on
/// standard output; nothing on error output, and a failure only in the
/// exit code. Never in the service's own process.
pub fn child() -> ExitCode {
    // A panic's message can quote the file: said nowhere.
    std::panic::set_hook(Box::new(|_| {}));
    // The memory cap again, from inside (the spawn set it before exec):
    // nothing is read without it.
    #[cfg(target_os = "linux")]
    if cap_memory().is_err() {
        return ExitCode::FAILURE;
    }
    let mut bytes = Vec::new();
    if std::io::stdin()
        .lock()
        .take(PDF_MOST as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return ExitCode::FAILURE;
    }
    match std::panic::catch_unwind(|| text(&bytes)) {
        Ok(Ok(text)) => {
            let mut out = std::io::stdout().lock();
            if out
                .write_all(text.as_bytes())
                .and_then(|()| out.flush())
                .is_ok()
            {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        _ => ExitCode::FAILURE,
    }
}

/// A PDF's text, at most [`TEXT_MOST`] characters: the checks of
/// [`refused`], [`shapes_bounded`] and [`inflates_within`], then the parse.
/// In the child only ([`child`]); in tests, under `catch_unwind`.
pub fn text(bytes: &[u8]) -> Result<String, &'static str> {
    refused(bytes)?;
    if !shapes_bounded(bytes) || !inflates_within(bytes, INFLATED_MOST) {
        return Err(PDF_CANT_READ);
    }
    let doc = pdf_extract::Document::load_mem(bytes).map_err(|_| PDF_CANT_READ)?;
    if doc.is_encrypted() || doc.was_encrypted() {
        return Err(PDF_CANT_READ);
    }
    let mut capped = Capped::default();
    let parsed = {
        let mut output = pdf_extract::PlainTextOutput::new(&mut capped);
        pdf_extract::output_doc(&doc, &mut output)
    };
    // A parse stopped by the cap is a whole read of what fits.
    if parsed.is_err() && !capped.full {
        return Err(PDF_CANT_READ);
    }
    // `tidy` marks a cut with "…": all in all at most TEXT_MOST.
    let text = tidy(&capped.text, TEXT_MOST - 1);
    if text.is_empty() {
        // A scan, or text drawn as shapes: nothing to read.
        return Err(PDF_CANT_READ);
    }
    Ok(text)
}

/// Whether every stream of `bytes`, inflated as `lopdf` would (zlib, else
/// raw deflate past its header), comes to at most `most` bytes in all;
/// false too with a filter that count doesn't bound. Each stream is
/// inflated from just after its `stream` line to the deflate data's own
/// end, at least what `lopdf` inflates from its `/Length`.
pub fn inflates_within(bytes: &[u8], most: usize) -> bool {
    use flate2::read::{DeflateDecoder, ZlibDecoder};
    if UNBOUNDED_FILTERS
        .iter()
        .any(|filter| find(bytes, filter).is_some())
    {
        return false;
    }
    let mut left = most;
    let mut at = 0;
    while let Some(found) = find(&bytes[at..], b"stream") {
        let keyword = at + found;
        at = keyword + b"stream".len();
        // `lopdf` takes spaces and tabs between `stream` and its line end.
        while matches!(bytes.get(at), Some(b' ' | b'\t')) {
            at += 1;
        }
        let start = match &bytes[at..] {
            [b'\r', b'\n', ..] => at + 2,
            [b'\n', ..] | [b'\r', ..] => at + 1,
            _ => continue,
        };
        let data = &bytes[start..];
        let (mut out, ok) = inflated(ZlibDecoder::new(data), left);
        if !ok && out == 0 && data.len() > 2 {
            // As `lopdf`: raw deflate when zlib gave nothing.
            out = inflated(DeflateDecoder::new(&data[2..]), left).0;
        }
        if out > left {
            return false;
        }
        left -= out;
    }
    true
}

/// Whether a PDF's shapes are within what is parsed, its names read as
/// `lopdf` reads them ([`names_read`]): no filter the inflate count doesn't
/// bound, no `/Filter` array of more than one filter (each stage inflates
/// the last one's output, uncounted), and no `/Columns`, `/Colors` or
/// `/BitsPerComponent` past their bounds (a predictor sizes two rows by
/// them before it reads any data). Early refusals: [`CHILD_MEMORY_MOST`]
/// is the bound.
pub fn shapes_bounded(bytes: &[u8]) -> bool {
    let names = names_read(bytes);
    if find(&names, b"/Encrypt").is_some()
        || UNBOUNDED_FILTERS
            .iter()
            .any(|filter| find(&names, filter).is_some())
    {
        return false;
    }
    for at in keys(&names, b"/Filter") {
        let rest = past_space(&names, at);
        if names.get(rest) == Some(&b'[') {
            let inside = &names[rest + 1..];
            let end = inside
                .iter()
                .position(|byte| *byte == b']')
                .unwrap_or(inside.len());
            if inside[..end].iter().filter(|byte| **byte == b'/').count() > 1 {
                return false;
            }
        }
    }
    for (key, most) in [
        (&b"/Columns"[..], COLUMNS_MOST),
        (b"/Colors", COLORS_MOST),
        (b"/BitsPerComponent", BITS_MOST),
    ] {
        if keys(&names, key).any(|at| number_past(&names, past_space(&names, at), most)) {
            return false;
        }
    }
    true
}

/// `bytes` with each name's `#xx` escapes read (`/Fil#74er` is `/Filter`
/// to `lopdf`), all else as it is.
fn names_read(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut in_name = false;
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        if byte == b'/' {
            in_name = true;
        } else if in_name && !is_regular(byte) {
            in_name = false;
        } else if in_name && byte == b'#' {
            let hex = bytes
                .get(at + 1..at + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok());
            if let Some(hex) = hex {
                out.push(hex);
                at += 3;
                continue;
            }
        }
        out.push(byte);
        at += 1;
    }
    out
}

/// A PDF's regular character: neither white space nor a delimiter.
fn is_regular(byte: u8) -> bool {
    !matches!(
        byte,
        0 | b'\t'
            | b'\n'
            | 0x0c
            | b'\r'
            | b' '
            | b'('
            | b')'
            | b'<'
            | b'>'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'/'
            | b'%'
    )
}

/// Where each whole `key` (a name, `/Columns`, not `/ColumnsX`) of
/// `bytes` ends.
fn keys<'a>(bytes: &'a [u8], key: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    bytes
        .windows(key.len())
        .enumerate()
        .filter(move |(at, window)| {
            *window == key
                && bytes
                    .get(at + key.len())
                    .is_none_or(|next| !is_regular(*next))
        })
        .map(move |(at, _)| at + key.len())
}

/// Past white space and comments from `at`.
fn past_space(bytes: &[u8], mut at: usize) -> usize {
    loop {
        match bytes.get(at) {
            Some(0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ') => at += 1,
            Some(b'%') => {
                while !matches!(bytes.get(at), None | Some(b'\n' | b'\r')) {
                    at += 1;
                }
            }
            _ => return at,
        }
    }
}

/// Whether the integer at `at` (a sign, then digits) is past `most`.
fn number_past(bytes: &[u8], at: usize, most: u64) -> bool {
    let at = match bytes.get(at) {
        Some(b'+' | b'-') => at + 1,
        _ => at,
    };
    let mut value: u64 = 0;
    for byte in bytes[at.min(bytes.len())..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
    {
        value = value
            .saturating_mul(10)
            .saturating_add(u64::from(byte - b'0'));
        if value > most {
            return true;
        }
    }
    false
}

/// How much `reader` inflates to, counted up to `most + 1` (what it gave
/// before an error counts too), and whether it ended without one.
fn inflated(mut reader: impl Read, most: usize) -> (usize, bool) {
    let mut buf = [0u8; 8192];
    let mut total = 0;
    while total <= most {
        match reader.read(&mut buf) {
            Ok(0) => return (total, true),
            Ok(n) => total += n,
            Err(_) => return (total, false),
        }
    }
    (total, true)
}

/// The parse's text, stopped once it holds twice [`TEXT_MOST`]
/// characters (room for the spacing `tidy` takes out): a long PDF's later
/// pages are never laid out.
#[derive(Default)]
struct Capped {
    text: String,
    chars: usize,
    full: bool,
}

impl std::fmt::Write for &mut Capped {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        for c in s.chars() {
            if self.chars >= TEXT_MOST * 2 {
                self.full = true;
                return Err(std::fmt::Error);
            }
            self.text.push(c);
            self.chars += 1;
        }
        Ok(())
    }
}

impl<'a> pdf_extract::ConvertToFmt for &'a mut Capped {
    type Writer = &'a mut Capped;
    fn convert(self) -> Self::Writer {
        self
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A one-page PDF whose page says `words` in Helvetica, its content
    /// stream inflated or not, with a correct cross-reference table.
    pub(crate) fn tiny_pdf(words: &str) -> Vec<u8> {
        let content = format!("BT /F1 12 Tf 72 720 Td ({words}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (at, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", at + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    /// A stream that inflates far past its size: `n` zero bytes deflated.
    fn bomb(n: usize) -> Vec<u8> {
        use flate2::{Compression, write::ZlibEncoder};
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&vec![0u8; n]).unwrap();
        let data = encoder.finish().unwrap();
        let mut out = b"%PDF-1.4\n1 0 obj\n<< /Filter /FlateDecode /Length ".to_vec();
        out.extend_from_slice(format!("{} >>\nstream\n", data.len()).as_bytes());
        out.extend_from_slice(&data);
        out.extend_from_slice(b"\nendstream\nendobj\n%%EOF\n");
        out
    }

    fn quiet<T>(f: impl FnOnce() -> T + std::panic::UnwindSafe) -> std::thread::Result<T> {
        std::panic::catch_unwind(f)
    }

    #[test]
    fn a_tiny_pdf_reads_its_words() {
        let pdf = tiny_pdf("Invoice total 4417 dong");
        let read = quiet(|| text(&pdf)).expect("no panic");
        assert_eq!(read.as_deref(), Ok("Invoice total 4417 dong"));
    }

    #[test]
    fn an_encrypted_a_broken_or_an_empty_pdf_is_named_not_read() {
        let mut encrypted = tiny_pdf("secret");
        let at = encrypted.len() - 30;
        encrypted.splice(at..at, b"/Encrypt 9 0 R ".iter().copied());
        assert_eq!(refused(&encrypted), Err(PDF_CANT_READ));
        for garbage in [
            b"not a pdf at all".to_vec(),
            b"%PDF-1.7\n1 0 obj << /Type /Catalog".to_vec(),
            Vec::new(),
            tiny_pdf(""),
        ] {
            let read = quiet(|| text(&garbage)).unwrap_or(Err("panicked"));
            assert!(read.is_err(), "{read:?}");
        }
    }

    #[test]
    fn an_over_size_pdf_is_refused_before_any_parse() {
        let big = vec![b' '; PDF_MOST + 1];
        assert_eq!(refused(&big), Err(PDF_TOO_LARGE));
        assert_eq!(text(&big), Err(PDF_TOO_LARGE));
        // Never a child: a program that doesn't exist isn't even tried.
        let read = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(read_with("/nonexistent/lamp-services".into(), big));
        assert_eq!(read, Err(PDF_TOO_LARGE));
    }

    #[test]
    fn a_stream_that_inflates_past_the_bound_is_not_parsed() {
        assert!(inflates_within(&tiny_pdf("fine"), INFLATED_MOST));
        let bomb = bomb(INFLATED_MOST + 1);
        assert!(bomb.len() < PDF_MOST, "{}", bomb.len());
        assert!(!inflates_within(&bomb, INFLATED_MOST));
        assert_eq!(text(&bomb), Err(PDF_CANT_READ));
        let mut lzw = tiny_pdf("x");
        lzw.extend_from_slice(b"% /LZWDecode\n");
        assert!(!inflates_within(&lzw, INFLATED_MOST));
    }

    /// A one-stream PDF: `dict` (inside `<< >>`, with its `/Length`),
    /// `keyword` (`stream` and what follows it to its line end), `data`.
    fn one_stream(dict: &str, keyword: &str, data: &[u8]) -> Vec<u8> {
        let mut out = b"%PDF-1.4\n1 0 obj\n<< ".to_vec();
        out.extend_from_slice(format!("{dict} /Length {} >>\n{keyword}", data.len()).as_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream\nendobj\n%%EOF\n");
        out
    }

    fn deflated(data: &[u8]) -> Vec<u8> {
        use flate2::{Compression, write::ZlibEncoder};
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    /// Third's HOLD: each filter of an array inflates the last one's
    /// output, which the count never sees; refused, escaped names too.
    #[test]
    fn a_filter_array_of_more_than_one_is_refused() {
        let twice = deflated(&deflated(&vec![0u8; 1 << 20]));
        for dict in [
            "/Filter [/FlateDecode /FlateDecode]",
            "/Filter[/FlateDecode/FlateDecode]",
            "/Fil#74er [ /Fl#61teDecode %x\n /FlateDecode ]",
        ] {
            let pdf = one_stream(dict, "stream\n", &twice);
            assert!(!shapes_bounded(&pdf), "{dict}");
            assert_eq!(text(&pdf), Err(PDF_CANT_READ), "{dict}");
        }
        // One filter, in an array or not, is counted as before.
        for dict in ["/Filter [/FlateDecode]", "/Filter /FlateDecode"] {
            assert!(
                shapes_bounded(&one_stream(dict, "stream\n", &twice)),
                "{dict}"
            );
        }
    }

    /// Third's HOLD: a predictor's two rows are sized by `/Columns` (and
    /// `/Colors`, `/BitsPerComponent`) before any data is read: 256 MiB
    /// each from a tiny stream. Refused; a cross-reference stream's own
    /// small predictor still reads.
    #[test]
    fn a_predictor_with_huge_columns_is_refused() {
        let data = deflated(b"\x02\x00");
        for dict in [
            "/Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 268435456 /Colors 1 /BitsPerComponent 8 >>",
            "/Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 4097 >>",
            "/Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 4 /Colors 99999999 >>",
            "/Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 4 /BitsPerComponent 9999999 >>",
            "/Filter /FlateDecode /DecodeParms << /Predictor 12 /Col#75mns %c\n +268435456 >>",
        ] {
            let pdf = one_stream(dict, "stream\n", &data);
            assert!(!shapes_bounded(&pdf), "{dict}");
            assert_eq!(text(&pdf), Err(PDF_CANT_READ), "{dict}");
        }
        let xref = "/Type /XRef /Filter /FlateDecode /DecodeParms << /Columns 5 /Predictor 12 >> /W [1 3 1]";
        assert!(shapes_bounded(&one_stream(xref, "stream\n", &data)));
        assert!(shapes_bounded(&tiny_pdf("fine")));
    }

    /// Third's HOLD: `lopdf` takes spaces and tabs between `stream` and its
    /// line end; the count now does too, so such a bomb is refused.
    #[test]
    fn a_stream_keyword_with_spaces_is_still_counted() {
        let data = deflated(&vec![0u8; INFLATED_MOST + 1]);
        for keyword in ["stream   \n", "stream \t\r\n", "stream\t\r"] {
            let pdf = one_stream("/Filter /FlateDecode", keyword, &data);
            assert!(pdf.len() < PDF_MOST);
            assert!(!inflates_within(&pdf, INFLATED_MOST), "{keyword:?}");
            assert_eq!(text(&pdf), Err(PDF_CANT_READ), "{keyword:?}");
        }
    }

    /// The child's cap as the spawn sets it, before exec: what a program
    /// started that way is held to.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_limited_child_is_held_to_its_memory_cap() {
        let out = limited(Command::new("/bin/cat").arg("/proc/self/limits"))
            .output()
            .unwrap();
        let limits = String::from_utf8_lossy(&out.stdout);
        let cap = CHILD_MEMORY_MOST.to_string();
        for name in ["Max address space", "Max data size"] {
            let line = limits.lines().find(|line| line.starts_with(name)).unwrap();
            let values: Vec<&str> = line[name.len()..].split_whitespace().collect();
            assert_eq!(&values[..2], [cap.as_str(), cap.as_str()], "{line}");
        }
    }

    #[test]
    fn a_long_pdf_is_cut_to_the_text_bound() {
        let words = "word ".repeat(1_500);
        let read = quiet(|| text(&tiny_pdf(&words)))
            .expect("no panic")
            .unwrap();
        assert!(read.chars().count() <= TEXT_MOST, "{}", read.len());
        assert!(read.starts_with("word word"));
    }
}
