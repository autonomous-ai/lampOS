#![allow(dead_code)]
use std::{collections::VecDeque,sync::Mutex,time::{Duration,Instant,SystemTime}};
const KEPT:Duration=Duration::from_secs(30);
const EYES_EVERY:Duration=Duration::from_millis(500);
const MOST_LOOKS:usize=600;
const FEWEST_LOOKS:usize=3;
type Speakers=Vec<String>;
#[derive(Clone,Copy,PartialEq)]pub enum SpeakingState {Yes,No,Unknown}
#[derive(Clone,Copy)]pub struct Speaking{state:SpeakingState}
pub struct Face{id:String,facing:Option<bool>,speaking:Option<Speaking>}
pub struct Faces{faces:Vec<Face>}
mod lamp_msgs{pub use crate::Face;}
mod memory_scope{pub(crate) fn qualified_speaker(speaker: &str) -> bool {
    if !(19..=28).contains(&speaker.len()) {
        return false;
    }
    let Some((boot, face)) = speaker.split_once('/') else {
        return false;
    };
    let Some(digits) = face.strip_prefix('f') else {
        return false;
    };
    boot.len() == 16
        && boot
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && (1..=10).contains(&digits.len())
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && digits.parse::<u32>().is_ok_and(|id| id > 0)
}
}
struct Look {
    at: Instant,
    /// The eyes' boot its frame was published under; empty for none.
    boot: String,
    /// The passing ids that read speaking in this look.
    speaking: Vec<String>,
    /// The original frame contains exactly one face, positively speaking.
    /// Used only by the strict two-look exception, before filtering others.
    single_positive: bool,
    /// Every passing id in this look, speaking or not
    /// ([`SpeakingFaces::in_view_at`]).
    tracked: Vec<String>,
    /// Of those, the ones turned to the lamp (`facing`), for a line the lamp
    /// began itself (#2287's follow-up, [`SpeakingFaces::turned_speakers`]).
    turned: Vec<String>,
    /// The faces turned to the lamp whose mouth read still (`no`), for
    /// [`SpeakingFaces::turned_only_silent`].
    turned_silent: Vec<String>,
    /// The faces turned to the lamp whose mouth was not read (`unknown`, or
    /// not judged at all) in this look.
    turned_unread: Vec<String>,
}

#[derive(Default)]
struct Looks {
    kept: VecDeque<Look>,
    /// The last frame counted, by its publisher's boot and sequence.
    frame: Option<(String, u64)>,
    /// Each of the eyes' boots, newest last, with the time its first frame
    /// was read: the boot a file the eyes wrote belongs to
    /// ([`SpeakingFaces::known_with_boot`]). Instance names only, so kept
    /// past [`KEPT`] and [`SpeakingFaces::forget`].
    boots: VecDeque<(String, SystemTime)>,
}

/// Boots of the eyes remembered with their first frame's time.
const MOST_BOOTS: usize = 8;
/// The last [`KEPT`] of looks, fed by one reader and judged at each line's
/// end. Each of the eyes' frames counts once, however often it is read (the
/// reviewer on #2008), so a short line can't pass the bar on one frame.
#[derive(Default)]
pub struct SpeakingFaces {
    looks: Mutex<Looks>,
}

impl SpeakingFaces{    pub fn look(&self, at: Instant, frame: Option<((&str, u64), &Faces)>) {
        let Ok(mut looks) = self.looks.lock() else {
            return;
        };
        match frame {
            Some(((boot, seq), _))
                if looks
                    .frame
                    .as_ref()
                    .is_some_and(|(last, at)| last == boot && *at == seq) =>
            {
                return;
            }
            Some(((boot, seq), _)) => looks.frame = Some((boot.to_owned(), seq)),
            None => {
                if looks.frame.is_none()
                    && looks
                        .kept
                        .back()
                        .is_some_and(|last| at.saturating_duration_since(last.at) < EYES_EVERY)
                {
                    return;
                }
                looks.frame = None;
            }
        }
        let speaking_faces: Vec<_> = frame
            .map(|(_, faces)| {
                faces
                    .faces
                    .iter()
                    .filter(|face| {
                        face.speaking
                            .as_ref()
                            .is_some_and(|speaking| speaking.state == SpeakingState::Yes)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let single_positive = frame.is_some_and(|((boot, _), faces)| {
            faces.faces.len() == 1
                && speaking_faces.len() == 1
                && crate::memory_scope::qualified_speaker(&format!("{boot}/{}", faces.faces[0].id))
        });
        let speaking = speaking_faces.iter().map(|face| face.id.clone()).collect();
        let tracked = frame
            .map(|(_, faces)| faces.faces.iter().map(|face| face.id.clone()).collect())
            .unwrap_or_default();
        let turned = speaking_faces
            .iter()
            .filter(|face| face.facing == Some(true))
            .map(|face| face.id.clone())
            .collect();
        let state = |face: &&lamp_msgs::Face| face.speaking.map(|speaking| speaking.state);
        let turned_faces: Vec<_> = frame
            .map(|(_, faces)| {
                faces
                    .faces
                    .iter()
                    .filter(|face| face.facing == Some(true))
                    .collect()
            })
            .unwrap_or_default();
        let turned_silent = turned_faces
            .iter()
            .filter(|face| state(face) == Some(SpeakingState::No))
            .map(|face| face.id.clone())
            .collect();
        let turned_unread = turned_faces
            .iter()
            .filter(|face| !matches!(state(face), Some(SpeakingState::Yes | SpeakingState::No)))
            .map(|face| face.id.clone())
            .collect();
        let boot = frame
            .map(|((boot, _), _)| boot.to_owned())
            .unwrap_or_default();
        if !boot.is_empty() && !looks.boots.iter().any(|(seen, _)| *seen == boot) {
            // The wall time of `at`, for comparing with a file's time.
            let first = SystemTime::now()
                .checked_sub(Instant::now().saturating_duration_since(at))
                .unwrap_or(SystemTime::UNIX_EPOCH);
            looks.boots.push_back((boot.clone(), first));
            while looks.boots.len() > MOST_BOOTS {
                looks.boots.pop_front();
            }
        }
        let kept = &mut looks.kept;
        while kept
            .front()
            .is_some_and(|look| at.saturating_duration_since(look.at) > KEPT)
            || kept.len() >= MOST_LOOKS
        {
            kept.pop_front();
        }
        kept.push_back(Look {
            at,
            boot,
            speaking,
            single_positive,
            tracked,
            turned,
            turned_silent,
            turned_unread,
        });
    }
    pub(crate) fn positive_speaker(&self, from: Instant, to: Instant) -> Speakers {
        let Ok(looks) = self.looks.lock() else {
            return Vec::new();
        };
        let mut speaker: Option<String> = None;
        let mut frames = 0;
        let mut spoken = 0;
        let mut single_positive = 0;
        for look in looks
            .kept
            .iter()
            .filter(|look| look.at >= from && look.at <= to)
        {
            frames += 1;
            single_positive += usize::from(look.single_positive);
            if !look.speaking.is_empty() {
                spoken += 1;
            }
            for id in &look.speaking {
                let qualified = format!("{}/{id}", look.boot);
                match &speaker {
                    Some(seen) if *seen != qualified => return Vec::new(),
                    None => speaker = Some(qualified),
                    _ => {}
                }
            }
        }
        if (frames >= FEWEST_LOOKS && spoken * 2 >= frames) || (frames == 2 && single_positive == 2)
        {
            speaker.into_iter().collect()
        } else {
            Vec::new()
        }
    }
    pub fn enrollment_speaker(&self, from: Instant, to: Instant) -> Option<String> {
        if to < from || to.duration_since(from) > KEPT {
            return None;
        }
        let looks = self.looks.lock().ok()?;
        let during: Vec<_> = looks
            .kept
            .iter()
            .filter(|look| look.at >= from && look.at <= to)
            .collect();
        let mut boot = None;
        let mut speaker = None;
        let mut positive = 0;
        for look in &during {
            if !look.boot.is_empty() {
                if boot.is_some_and(|boot| boot != &look.boot) {
                    return None;
                }
                boot = Some(&look.boot);
            }
            for id in &look.speaking {
                if look.boot.is_empty() || speaker.is_some_and(|speaker| speaker != id) {
                    return None;
                }
                speaker = Some(id);
            }
            positive += usize::from(!look.speaking.is_empty());
        }
        if positive < FEWEST_LOOKS || positive * 2 < during.len() {
            return None;
        }
        Some(format!("{}/{}", boot?, speaker?))
    }
}

fn faces()->Faces{Faces{faces:vec![Face{id:"f1".into(),facing:Some(true),speaking:Some(Speaking{state:SpeakingState::Yes})}]}}
const BOOT:&str="0123456789abcdef";
#[test]fn same_frame_without_gap_cannot_admit(){let e=SpeakingFaces::default();let t=Instant::now();let f=faces();for n in 0..3{e.look(t+Duration::from_millis(n*100),Some(((BOOT,1),&f)));}assert!(e.positive_speaker(t,t+Duration::from_secs(1)).is_empty());}
#[test]fn read_error_then_same_frame_cannot_admit(){let e=SpeakingFaces::default();let t=Instant::now();let f=faces();e.look(t,Some(((BOOT,1),&f)));e.look(t+Duration::from_millis(100),None);e.look(t+Duration::from_millis(200),Some(((BOOT,1),&f)));assert!(e.positive_speaker(t,t+Duration::from_secs(1)).is_empty(),"one distinct frame must not authorize disclosure");}
#[test]fn repeated_read_errors_cannot_create_enrollment(){let e=SpeakingFaces::default();let t=Instant::now();let f=faces();for n in 0..5{e.look(t+Duration::from_millis(n*100),if n%2==0{Some(((BOOT,1),&f))}else{None});}assert!(e.enrollment_speaker(t,t+Duration::from_secs(1)).is_none(),"one distinct frame must not authorize enrollment");}
#[test]fn two_fresh_frames_still_admit(){let e=SpeakingFaces::default();let t=Instant::now();let f=faces();e.look(t,Some(((BOOT,1),&f)));e.look(t+Duration::from_millis(100),Some(((BOOT,2),&f)));assert_eq!(e.positive_speaker(t,t+Duration::from_secs(1)),vec![format!("{BOOT}/f1")]);}
