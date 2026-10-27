//! Actual cached context and Engine dispatch across recognition resumption.
use super::*;
use crate::{
    memory_scope::{RequestKey, RequestScope, ScopeSnapshot},
    second_provider::{Asker, ChatEvent, Listener, Prepared, Services, Times, answer, net},
};
use futures_util::{
    StreamExt,
    future::BoxFuture,
    stream::{self, BoxStream},
};
use serde_json::json;
use std::sync::atomic::AtomicUsize;

const NAME: &str = "Fixture context owner";
const NOTE: &str = "PRIVATE CACHED NOTE";
const HISTORY: &str = "PRIVATE EARLIER QUESTION";
const REPLY: &str = "PRIVATE EARLIER RESPONSE";
const BOOT: &str = "0123456789abcdef";
const PUBLISHER: &str = "0123456789abcdef0123456789abcdef";

fn publish_v3(known: &Path, state: &str) {
    let now = lamp_ipc::monotonic_us();
    let value = if state == "seen" {
        json!({"version":3,"camera_hold":1,"camera_began_us":1,"state":state,"name":NAME,"id":null,"boot":BOOT,"face":"f7",
            "publisher":PUBLISHER,"generation":1,
            "observed_us":now,"permission_until_us":now.checked_add(1_500_000).unwrap()})
    } else {
        json!({"version":3,"camera_hold":1,"camera_began_us":1,"state":state,"boot":BOOT,"publisher":PUBLISHER,"generation":1,
            "observed_us":now,"permission_until_us":now.checked_add(1_500_000).unwrap()})
    };
    let source = crate::speaking_faces::known_speaker_of(known);
    let temporary = source.with_extension("new");
    std::fs::write(&temporary, value.to_string()).unwrap();
    std::fs::rename(temporary, source).unwrap();
}
#[derive(Default)]
struct Chat {
    messages: Mutex<Vec<Vec<Message>>>,
}
impl Services for Chat {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        panic!("the already-transcribed fixture needs no listener")
    }
    fn chat(&self, messages: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
        self.messages.lock().unwrap().push(messages);
        stream::iter([Ok(ChatEvent::Done)]).boxed()
    }
    fn speak(&self, _: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        panic!("the context-only response contains no sentence")
    }
}
struct ThreadRestore(crate::thread::Thread);
impl ThreadRestore {
    fn seed(person: &str) -> Self {
        let mut thread = crate::thread::THREAD.lock().unwrap();
        let previous = std::mem::take(&mut *thread);
        let now = Instant::now();
        thread.at_lamp(Some(person.into()));
        thread.seen_now(Some(person.into()));
        thread.opening(now);
        thread.person(HISTORY, true, now);
        thread.lamp_for(88, REPLY, now, Some(person));
        Self(previous)
    }
}
impl Drop for ThreadRestore {
    fn drop(&mut self) {
        *crate::thread::THREAD.lock().unwrap() = std::mem::take(&mut self.0);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Boundary {
    Selection,
    FinalSelection,
    NameFormatter,
}

fn context_case(boundary: Boundary, speculative: bool) {
    let _one = crate::provider_worker::tests::one_thread();
    let person = crate::remember::person_id(NAME);
    let _thread = ThreadRestore::seed(&person);
    let directory = crate::process::SessionDirectory::create().unwrap();
    let known = directory.path.canonicalize().unwrap().join("known");
    std::fs::write(&known, NAME).unwrap();
    publish_v3(&known, "seen");
    let authority = crate::camera_authority::fixture::Fixture::start(
        known.clone(),
        [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ],
        1,
    );
    let mut owner = RequestScope::looking_later(
        RequestKey {
            worker: BootId::new([61; 16]).unwrap(),
            session: 1,
            request: 2,
        },
        ScopeSnapshot::of(NAME),
        authority.cache(),
    );
    let key = owner.lease().key();
    assert!(owner.speaker_recorder().unwrap().seal(
        key,
        &[format!("{BOOT}/f7")],
        Instant::now() + Duration::from_secs(10),
    ));
    assert_eq!(authority.ready(&owner.lease()), PermissionState::Ready);
    assert_eq!(
        private_context_state(Some(&known), &owner.lease()),
        PermissionState::Ready
    );
    let original = owner.lease().private_speaker().cloned().unwrap();
    let sessions = Arc::new(Mutex::new(Sessions {
        scoped: Some(Some(person.clone())),
        note: Some(NOTE.into()),
        ..Sessions::default()
    }));
    let interrupt = Arc::new(AtomicBool::new(boundary != Boundary::Selection));
    let formatting = known.clone();
    let context = memory_context(
        sessions.clone(),
        Some(PathBuf::from("/unused/cache-only-no-rpc")),
        Some(known.clone()),
        true,
        move |_, person, memory, lease, history| {
            let interrupt = interrupt.swap(false, Ordering::AcqRel);
            if interrupt && boundary == Boundary::NameFormatter {
                publish_v3(&formatting, "checking");
            }
            let name = known_name_told(true, Some(&formatting), lease);
            if interrupt && boundary == Boundary::NameFormatter {
                assert!(name.is_empty());
                // A separate validator resumes before the final selector.
                publish_v3(&formatting, "seen");
                assert_eq!(
                    private_context_state(Some(&formatting), lease.unwrap()),
                    PermissionState::Ready
                );
            }
            let told = disclosure_message(
                false,
                "Synthetic character.",
                None,
                &(memory + &name),
                None,
                person.as_deref(),
                history,
            );
            if interrupt && boundary == Boundary::FinalSelection {
                publish_v3(&formatting, "checking");
            }
            told
        },
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let pending = Arc::new(AtomicUsize::new(0));
    let (called, observed, resuming) = (calls.clone(), pending.clone(), known.clone());
    let context = Arc::new(move |lease: &crate::memory_scope::ScopeLease| {
        if called.fetch_add(1, Ordering::AcqRel) == 0 && boundary == Boundary::Selection {
            publish_v3(&resuming, "checking");
        }
        let told = context(Some(lease), Chosen::ByLease);
        if told.pending_private_context {
            assert!(told.text.is_empty() && told.history.is_empty());
            observed.fetch_add(1, Ordering::AcqRel);
            // Resume before Engine receives the callback's result. A later
            // mutable-latch read cannot discover its incomplete composition.
            publish_v3(&resuming, "seen");
            assert_eq!(
                private_context_state(Some(&resuming), lease),
                PermissionState::Ready
            );
        }
        told
    });
    let services = Arc::new(Chat::default());
    let validate = known.clone();
    let mut engine = Engine::new(
        services.clone(),
        Told::default,
        Arc::new(|_, _, _, _| panic!("no tools in the context fixture")),
    )
    .announcing(false)
    .guarding_private_context(Arc::new(move |lease| {
        private_context_state(Some(&validate), lease)
    }));
    Arc::get_mut(&mut engine.shared).unwrap().lease_system = Some(context);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        if speculative {
            let prepared = Prepared::try_begin(
                &engine.shared,
                Some(&owner.lease()),
                "synthetic question".into(),
                &[],
                None,
                false,
            );
            assert!(
                prepared.is_none(),
                "{boundary:?}: no incomplete speculative chat"
            );
            assert!(services.messages.lock().unwrap().is_empty());
        }
        let shared = engine.shared.clone();
        let answering = answer(
            &shared,
            (2, Some(owner.lease())),
            "synthetic question".into(),
            Asker::Spoken,
            None,
            None,
            Vec::new(),
            false,
            Times::new(Instant::now()),
            None,
            None,
        );
        tokio::pin!(answering);
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                tokio::select! {
                    _ = &mut answering => break,
                    event = engine.output.recv() => assert!(event.is_some()),
                }
            }
        })
        .await
        .expect("context resumption is bounded");
    });
    assert_eq!(pending.load(Ordering::Acquire), 1, "{boundary:?}");
    assert_eq!(calls.load(Ordering::Acquire), 2, "{boundary:?}");
    assert_eq!(owner.lease().private_speaker(), Some(&original));
    assert_eq!(owner.lease().speaker(), Some("0123456789abcdef/f7"));
    assert!(!owner.lease().private_context_lost());
    let messages = services.messages.lock().unwrap();
    assert_eq!(messages.len(), 1, "{boundary:?}: exactly one final chat");
    let payload = serde_json::to_string(&messages[0]).unwrap();
    for expected in [NOTE, HISTORY, REPLY, NAME] {
        assert!(
            payload.contains(expected),
            "{boundary:?}: missing {expected}"
        );
    }
    assert!(
        sessions.lock().unwrap().asked.is_none(),
        "composition never rereads the store"
    );
}
#[test]
fn pending_composition_rebuilds_cached_note_history_and_name_before_one_final_chat() {
    for boundary in [
        Boundary::Selection,
        Boundary::FinalSelection,
        Boundary::NameFormatter,
    ] {
        context_case(boundary, false);
    }
}
#[test]
fn pending_prepared_composition_never_freezes_reduced_messages() {
    for boundary in [
        Boundary::Selection,
        Boundary::FinalSelection,
        Boundary::NameFormatter,
    ] {
        context_case(boundary, true);
    }
}

/// Composition is the controlled latch: expiry is published after the
/// admitted lease's positive check and before Engine's actual Services call.
#[test]
fn permission_expiry_after_composition_sends_no_private_chat() {
    let _one = crate::provider_worker::tests::one_thread();
    let _thread = ThreadRestore::seed(&crate::remember::person_id(NAME));
    for expire in [false, true] {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let known = directory.path.canonicalize().unwrap().join("known");
        publish_v3(&known, "seen");
        let authority = crate::camera_authority::fixture::Fixture::start(
            known.clone(),
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef,
            ],
            1,
        );
        let mut owner = RequestScope::looking_later(
            RequestKey {
                worker: BootId::new([62; 16]).unwrap(),
                session: 1,
                request: 2,
            },
            ScopeSnapshot::of(NAME),
            authority.cache(),
        );
        let key = owner.lease().key();
        assert!(owner.speaker_recorder().unwrap().seal(
            key,
            &[format!("{BOOT}/f7")],
            Instant::now() + Duration::from_secs(10),
        ));
        assert_eq!(authority.ready(&owner.lease()), PermissionState::Ready);
        assert_eq!(
            private_context_state(Some(&known), &owner.lease()),
            PermissionState::Ready
        );
        let services = Arc::new(Chat::default());
        let validate = known.clone();
        let composing = known.clone();
        let compositions = Arc::new(AtomicUsize::new(0));
        let counted = compositions.clone();
        let mut engine = Engine::new(
            services.clone(),
            Told::default,
            Arc::new(|_, _, _, _| panic!("no tool in expiry dispatch control")),
        )
        .announcing(false)
        .guarding_private_context(Arc::new(move |lease| {
            private_context_state(Some(&validate), lease)
        }));
        Arc::get_mut(&mut engine.shared).unwrap().lease_system = Some(Arc::new(move |lease| {
            assert_eq!(
                private_context_state(Some(&composing), lease),
                PermissionState::Ready
            );
            counted.fetch_add(1, Ordering::AcqRel);
            if expire {
                let source = crate::speaking_faces::known_speaker_of(&composing);
                let mut value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&source).unwrap()).unwrap();
                value["permission_until_us"] = json!(lamp_ipc::monotonic_us());
                let temporary = source.with_extension("new");
                std::fs::write(&temporary, value.to_string()).unwrap();
                std::fs::rename(temporary, source).unwrap();
            }
            Told {
                text: NOTE.into(),
                requires_speaker: true,
                ..Told::default()
            }
        }));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let shared = engine.shared.clone();
            let answering = answer(
                &shared,
                (2, Some(owner.lease())),
                "synthetic question".into(),
                Asker::Spoken,
                None,
                None,
                Vec::new(),
                false,
                Times::new(Instant::now()),
                None,
                None,
            );
            tokio::pin!(answering);
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    tokio::select! {
                        _ = &mut answering => break,
                        event = engine.output.recv() => assert!(event.is_some()),
                    }
                }
            })
            .await
            .expect("expiry dispatch control is bounded");
        });
        assert_eq!(compositions.load(Ordering::Acquire), 1);
        let sent = services.messages.lock().unwrap();
        assert_eq!(
            sent.len(),
            usize::from(!expire),
            "actual Services chat calls"
        );
        if !expire {
            assert!(serde_json::to_string(&sent[0]).unwrap().contains(NOTE));
        } else {
            assert!(owner.lease().private_context_lost());
        }
    }
}
