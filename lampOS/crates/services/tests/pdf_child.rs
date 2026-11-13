//! A PDF's text through the real child, `lamp-services --pdf-text`
//! (`mail::pdf`): read, broken, a child that aborts (as on a failed
//! allocation) or never ends, and, on Linux, the child's memory cap.
use lamp_services::mail::pdf::{PARSE_WITHIN, PDF_CANT_READ, read_with};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// These tests one at a time: the reads share one turn in the process,
/// and each measures its own.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turn() -> std::sync::MutexGuard<'static, ()> {
    TURN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn run<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

fn lamp_services() -> PathBuf {
    env!("CARGO_BIN_EXE_lamp-services").into()
}

/// A one-page PDF whose page says `words`, with a correct
/// cross-reference table.
fn tiny_pdf(words: &str) -> Vec<u8> {
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

/// First use and a repeat, each a child of its own, within the bound
/// (the other tests here may hold the one-at-a-time turn meanwhile).
#[test]
fn the_child_reads_a_pdfs_text_within_its_bound() {
    let _turn = turn();
    for _ in 0..2 {
        let started = Instant::now();
        let read = run(read_with(lamp_services(), tiny_pdf("Total due 4417")));
        let took = started.elapsed();
        eprintln!("a PDF's text through the child: {took:?}");
        assert_eq!(read.as_deref(), Ok("Total due 4417"));
        assert!(took < PARSE_WITHIN, "{took:?}");
    }
}

#[test]
fn a_broken_pdf_or_a_failed_child_is_named_not_read() {
    let _turn = turn();
    for bytes in [
        b"not a pdf".to_vec(),
        b"%PDF-1.7\n1 0 obj << /Type /Catalog".to_vec(),
    ] {
        let read = run(read_with(lamp_services(), bytes));
        assert_eq!(read, Err(PDF_CANT_READ));
    }
    let read = run(read_with("/usr/bin/false".into(), tiny_pdf("x")));
    assert_eq!(read, Err(PDF_CANT_READ));
}

/// A child that never ends (here one that never reads its input nor
/// answers) is killed at 2 s and the read named, never waited on; a 2 MB
/// input it never reads doesn't hold the read past the kill either.
#[test]
fn a_child_that_never_ends_is_killed_at_its_bound() {
    use std::os::unix::fs::PermissionsExt;
    let _turn = turn();
    let dir = tempfile::tempdir().unwrap();
    let hangs = dir.path().join("hangs");
    std::fs::write(&hangs, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&hangs, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut big = tiny_pdf("x");
    big.resize(2 << 20, b' ');
    let started = Instant::now();
    let read = run(read_with(hangs, big));
    let took = started.elapsed();
    assert_eq!(read, Err(PDF_CANT_READ));
    assert!(took >= PARSE_WITHIN, "{took:?}");
    assert!(took < PARSE_WITHIN + Duration::from_secs(1), "{took:?}");
}

/// A child that aborts, as one does when an allocation fails at its
/// memory cap, is a plain "can't read this PDF", never a failure of the
/// service.
#[test]
fn a_child_that_aborts_is_named_not_read() {
    use std::os::unix::fs::PermissionsExt;
    let _turn = turn();
    let dir = tempfile::tempdir().unwrap();
    let aborts = dir.path().join("aborts");
    std::fs::write(&aborts, "#!/bin/sh\nkill -ABRT $$\n").unwrap();
    std::fs::set_permissions(&aborts, std::fs::Permissions::from_mode(0o755)).unwrap();
    let read = run(read_with(aborts, tiny_pdf("x")));
    assert_eq!(read, Err(PDF_CANT_READ));
}

/// The real child caps its own memory first thing, before it reads its
/// input (the second line beside the spawn's, `mail::pdf::limited`).
#[cfg(target_os = "linux")]
#[test]
fn the_child_caps_its_own_memory_before_it_reads() {
    use lamp_services::mail::pdf::{CHILD_ARG, CHILD_MEMORY_MOST};
    use std::process::{Command, Stdio};
    let _turn = turn();
    // Standard input held open: the child waits on it, capped.
    let mut child = Command::new(lamp_services())
        .arg(CHILD_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let cap = CHILD_MEMORY_MOST.to_string();
    let started = Instant::now();
    let capped = loop {
        let limits =
            std::fs::read_to_string(format!("/proc/{}/limits", child.id())).unwrap_or_default();
        let capped = limits
            .lines()
            .find(|line| line.starts_with("Max address space"))
            .is_some_and(|line| line.split_whitespace().nth(3) == Some(cap.as_str()));
        if capped || started.elapsed() > Duration::from_secs(2) {
            break capped;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = child.kill();
    let _ = child.wait();
    assert!(capped);
}
