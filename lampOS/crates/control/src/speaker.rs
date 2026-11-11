//! LOCAL DEMO ONLY (call demo on lamp-e82d, never pushed): a paired computer
//! with the `speak` grant may send binary frames of 16 kHz mono S16LE, played
//! at once on the lamp's speaker (`device_speaker`, the person's volume).
//! One `aplay` a connection, started at a frame and let go 300 ms after the
//! last one, so the lamp's own voice has its speaker back between sounds.
use std::{
    io::Write,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const LET_GO_AFTER: Duration = Duration::from_millis(300);

struct Playing {
    child: Child,
    last: Instant,
}

#[derive(Default)]
pub struct Player {
    playing: Arc<Mutex<Option<Playing>>>,
}

fn end(playing: &mut Option<Playing>) {
    if let Some(mut was) = playing.take() {
        drop(was.child.stdin.take());
        let _ = was.child.kill();
        let _ = was.child.wait();
    }
}

impl Player {
    pub fn play(&mut self, pcm: &[u8]) {
        let mut playing = self.playing.lock().expect("speaker");
        if playing
            .as_mut()
            .is_some_and(|was| !matches!(was.child.try_wait(), Ok(None)))
        {
            end(&mut playing);
        }
        if playing.is_none() {
            let Ok(child) = Command::new("aplay")
                .args([
                    "-q", "-D", "device_speaker", "-t", "raw", "-f", "S16_LE", "-c", "1", "-r",
                    "16000", "--buffer-time=120000",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            else {
                return;
            };
            *playing = Some(Playing {
                child,
                last: Instant::now(),
            });
            // Lets the speaker go once the frames stop.
            let shared = self.playing.clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(50));
                    let mut playing = shared.lock().expect("speaker");
                    match playing.as_ref() {
                        None => return,
                        Some(was) if was.last.elapsed() >= LET_GO_AFTER => {
                            end(&mut playing);
                            return;
                        }
                        Some(_) => {}
                    }
                }
            });
        }
        if let Some(was) = playing.as_mut() {
            was.last = Instant::now();
            let wrote = was
                .child
                .stdin
                .as_mut()
                .is_some_and(|stdin| stdin.write_all(pcm).is_ok());
            if !wrote {
                end(&mut playing);
            }
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        end(&mut self.playing.lock().expect("speaker"));
    }
}
