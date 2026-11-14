//! What the frame link (`--frame-link`, #6543) costs the eyes' loop, per
//! frame, on this machine: the loop's frame work (the look's decode and
//! shrink, `Picture::from_jpeg`, of a 640x480 JPEG) timed with no link, with
//! the link bound and no reader, and with one reader that asks for every
//! frame it can (far more than lamp-seen's one in 5 s). Not part of the
//! product: a made-up picture, a socket in a private folder under /tmp,
//! removed at the end; nothing else is written or kept.
//!
//! `cargo run --release -p lamp-eyes --example frame_link_bench -- [--frames N]`
use lamp_camera::frames::{FrameClient, Role};
use lamp_eyes::{
    frame_link::{Config, FrameLink, Read},
    picture::Picture,
};
use lamp_turn_owner::BootId;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const MAX: usize = 4 * 1024 * 1024;

fn boot(n: u8) -> BootId {
    BootId::new([n; 16]).expect("a boot")
}

/// A 640x480 picture with detail enough for a camera-sized JPEG.
fn picture() -> Vec<u8> {
    let (width, height) = (640usize, 480usize);
    let mut seed = 0x2545_f491_u32;
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed & 0x1f) as u8;
            rgb.push(((x * 255 / width) as u8).wrapping_add(noise));
            rgb.push(((y * 255 / height) as u8).wrapping_add(noise));
            rgb.push((((x + y) % 256) as u8).wrapping_add(noise));
        }
    }
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 80)
        .encode(&rgb, 640, 480, jpeg_encoder::ColorType::Rgb)
        .expect("encoded");
    jpeg
}

struct Times(Vec<Duration>);
impl Times {
    fn line(&mut self, name: &str) -> String {
        self.0.sort();
        let n = self.0.len();
        let at = |q: f64| self.0[((n as f64 - 1.0) * q).round() as usize];
        let mean = self.0.iter().sum::<Duration>() / n as u32;
        format!(
            "{name:<34} mean {:>8.1} us  median {:>8.1} us  p99 {:>8.1} us  worst {:>8.1} us",
            mean.as_secs_f64() * 1e6,
            at(0.5).as_secs_f64() * 1e6,
            at(0.99).as_secs_f64() * 1e6,
            at(1.0).as_secs_f64() * 1e6,
        )
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut frames = 1_000usize;
    while let Some(flag) = args.next() {
        match (flag.as_str(), args.next().and_then(|n| n.parse().ok())) {
            ("--frames", Some(n)) if n > 10 => frames = n,
            _ => {
                eprintln!("usage: frame_link_bench [--frames N]");
                std::process::exit(2);
            }
        }
    }
    let jpeg = picture();
    println!(
        "{frames} frames of a 640x480 JPEG, {} bytes, one thread each",
        jpeg.len()
    );
    let dir = PathBuf::from(format!("/tmp/lef-bench-{}", std::process::id()));
    fs::create_dir(&dir).expect("bench folder");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("private");
    let socket = dir.join("camera.frames");
    let started = Instant::now();
    let now_us = move || started.elapsed().as_micros() as u64 + 1;
    let read = |sequence: u64, at_us: u64| Read {
        worker: boot(2),
        epoch: 1,
        sequence,
        width: 640,
        height: 480,
        driver_sequence: sequence as u32,
        driver_timestamp: None,
        dequeue_started_us: at_us,
        dequeue_completed_us: at_us,
    };

    // Warm up the decoder.
    for _ in 0..20 {
        let _ = Picture::from_jpeg(&jpeg).expect("decoded");
    }

    // No link.
    let mut times = Times(Vec::with_capacity(frames));
    for _ in 0..frames {
        let at = Instant::now();
        let _ = std::hint::black_box(Picture::from_jpeg(&jpeg).expect("decoded"));
        times.0.push(at.elapsed());
    }
    println!("{}", times.line("no link"));

    // The bench's reader is this process, not lamp-seen.service: let it in.
    let mut link = FrameLink::new_allowing(
        Config {
            path: socket.clone(),
            controller: boot(1),
            reader: boot(3),
        },
        MAX,
        now_us(),
        std::sync::Arc::new(|_| true),
    )
    .expect("link");
    link.turn(Some(1), now_us()).expect("bound");

    // Bound, nobody connected.
    let (mut whole, mut alone) = (
        Times(Vec::with_capacity(frames)),
        Times(Vec::with_capacity(frames)),
    );
    for sequence in 1..=frames as u64 {
        let at = Instant::now();
        link.turn(Some(1), now_us()).expect("turn");
        let _ = link
            .offer(Some(1), &read(sequence, now_us()), &jpeg, now_us())
            .expect("offer");
        let linked = at.elapsed();
        let _ = std::hint::black_box(Picture::from_jpeg(&jpeg).expect("decoded"));
        whole.0.push(at.elapsed());
        alone.0.push(linked);
    }
    println!("{}", whole.line("link bound, no reader: loop"));
    println!("{}", alone.line("link bound, no reader: link only"));

    // One reader asking for every frame it can.
    let stop = Arc::new(AtomicBool::new(false));
    let got = Arc::new(AtomicU64::new(0));
    let reader = {
        let (stop, got, socket) = (Arc::clone(&stop), Arc::clone(&got), socket.clone());
        std::thread::spawn(move || {
            let mut client = FrameClient::new(&socket, Role::Perception, boot(3), boot(1), MAX);
            let mut out = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                if !client.connect().unwrap_or(false) {
                    std::thread::sleep(Duration::from_micros(200));
                    continue;
                }
                if client.outstanding().is_none() && client.request(2_000_000).is_err() {
                    continue;
                }
                match client.poll(&mut out) {
                    Ok(Some(_)) => {
                        got.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(None) => std::thread::sleep(Duration::from_micros(200)),
                    Err(_) => {}
                }
            }
        })
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    while !link.connected() && Instant::now() < deadline {
        link.turn(Some(1), now_us()).expect("turn");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(link.connected(), "the reader did not connect");
    let (mut whole, mut alone) = (
        Times(Vec::with_capacity(frames)),
        Times(Vec::with_capacity(frames)),
    );
    let mut taken = 0u64;
    for sequence in 1..=frames as u64 {
        let at = Instant::now();
        link.turn(Some(1), now_us()).expect("turn");
        taken += u64::from(
            link.offer(Some(1), &read(sequence, now_us()), &jpeg, now_us())
                .expect("offer"),
        );
        let linked = at.elapsed();
        let _ = std::hint::black_box(Picture::from_jpeg(&jpeg).expect("decoded"));
        whole.0.push(at.elapsed());
        alone.0.push(linked);
    }
    // Let the last frame finish.
    let end = Instant::now() + Duration::from_millis(200);
    while Instant::now() < end {
        link.turn(Some(1), now_us()).expect("turn");
        std::thread::sleep(Duration::from_millis(1));
    }
    stop.store(true, Ordering::Relaxed);
    reader.join().expect("reader");
    println!("{}", whole.line("one reader, every frame: loop"));
    println!("{}", alone.line("one reader, every frame: link only"));
    println!(
        "frames offered {taken}, received {}, link counts {:?}",
        got.load(Ordering::Relaxed),
        link.link_counts()
    );
    drop(link);
    let _ = fs::remove_dir_all(&dir);
}
