use crate::{
    BarrierOutcome, BarrierPolicy, BarrierReport, BarrierWait, Discard, EVENT_QUEUE_CAPACITY,
    Error, Event, INPUT_QUEUE_CAPACITY, ImageLedger, ImageOffer, ImageReceipt, ImageRefusal,
    ImageRevocation, Lineage, MAX_IMAGE_AGE, MAX_IMAGE_BYTES, MAX_IMAGES_PER_REQUEST,
    MAX_INPUT_AGE, MAX_INPUT_SAMPLES, MAX_OPEN_ACTIVITY_REQUESTS, MAX_RETAINED_SAMPLES,
    MAX_WIRE_BYTES, OUTPUT_RATE, RequestId, Result, Resumption, ResumptionHandle, SessionConfig,
    SessionId, StartRelease, State, SuccessorStart, Timeouts, Write, wire,
};
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
// Deadlines use the runtime clock so scripted tests can cover minutes of
// conversation without waiting for them. Capture ages use system monotonic time
// unless an explicit fixture clock supplies their acquisition-time domain.
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{Notify, mpsc, watch},
    task::JoinHandle,
    time::{Instant as Clock, MissedTickBehavior, timeout},
};
use tokio_tungstenite::{
    WebSocketStream, connect_async_with_config,
    tungstenite::{self, Message, protocol::WebSocketConfig},
};

#[derive(Clone, Copy)]
struct Stopped {
    error: Error,
    observed_at: Clock,
}

#[cfg(test)]
struct DiscardPublicationPause {
    published: std::sync::mpsc::Sender<()>,
    proceed: std::sync::mpsc::Receiver<()>,
}

#[derive(Clone)]
pub(crate) enum CaptureClockSource {
    System,
    #[cfg(any(test, feature = "fixture-clock"))]
    Fixture(Arc<dyn crate::testing::CaptureClock>),
}
impl CaptureClockSource {
    fn now(&self) -> Instant {
        match self {
            Self::System => Instant::now(),
            #[cfg(any(test, feature = "fixture-clock"))]
            Self::Fixture(clock) => clock.now(),
        }
    }
}

struct Shared {
    capture_clock: CaptureClockSource,
    // Test-only deterministic preemption between discard's existing atomics.
    #[cfg(test)]
    discard_publication_pause: Mutex<Option<DiscardPublicationPause>>,
    retired_through: AtomicU64,
    /// Through which request input was abandoned and not continued.
    discarded_through: AtomicU64,
    submitted_through: AtomicU64,
    wake: Notify,
    stop: watch::Sender<Option<Stopped>>,
    resumption: Mutex<Option<ResumptionPoint>>,
    // An invalidation can arrive on a resumed connection before it issues a
    // local handle. Retain that fact for the supervisor's inherited point.
    resumption_invalidated: AtomicBool,
    /// Highest request whose End has been queued.
    ended_through: AtomicU64,
    images: Mutex<Images>,
    /// Wakes an image write that must be abandoned.
    image_abort: Notify,
    /// The caller wants a new session in this one's place at the next
    /// moment nothing is owed on it.
    renew: AtomicBool,
    /// The functions nothing is to be said on account of ([`crate::Tool::silent`]).
    silent_tools: Vec<String>,
    /// The calls the service withdrew most recently, newest last, noted as
    /// the cancellation is read off the connection: before its
    /// [`Event::ToolCallsCancelled`] is taken, which may wait behind the
    /// sound before it.
    withdrawn_calls: Mutex<VecDeque<String>>,
}

/// How many withdrawn calls a session remembers: many more than may wait at
/// once ([`crate::MAX_TOOL_CALLS`]).
const WITHDRAWN_KEPT: usize = 64;

/// How long one image write may take. Commands that arrive meanwhile are
/// held, in order, and sent afterwards; audio must stay well inside
/// [`MAX_INPUT_AGE`].
const IMAGE_WRITE: Duration = Duration::from_millis(400);
/// Commands held during one image write, beyond the command queue itself.
const HELD_COMMANDS: usize = 64;

struct PendingImage {
    request: RequestId,
    frame: u64,
    grant: u64,
    captured_at: Instant,
    offered_at: Clock,
    jpeg: Vec<u8>,
}
#[derive(Default)]
struct Images {
    /// One slot. The newest image of the current request wins.
    pending: Option<PendingImage>,
    /// The request being counted and how many of its images were accepted.
    counted: (u64, u8),
    ledger: ImageLedger,
    /// Every camera grant up to this one has been revoked.
    revoked_grant: Option<u64>,
    /// The grant of the image being written now.
    in_flight: Option<u64>,
    /// That write must stop.
    abandon: bool,
}
impl Images {
    fn revoked(&self, grant: u64) -> bool {
        self.revoked_grant.is_some_and(|revoked| grant <= revoked)
    }
}

/// Complete by its markers: start of image and end of image. Nothing in
/// between is parsed here.
fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes.starts_with(&[0xFF, 0xD8, 0xFF]) && bytes.ends_with(&[0xFF, 0xD9])
}

/// The latest point from which the service said this session can be resumed.
#[derive(Clone, Debug)]
pub struct ResumptionPoint {
    handle: ResumptionHandle,
    current: bool,
    advertised_session: SessionId,
}
impl ResumptionPoint {
    pub fn handle(&self) -> &ResumptionHandle {
        &self.handle
    }
    /// Whether the latest applicable service metadata advertised this point
    /// as current. This does not acknowledge which client messages the point
    /// includes or guarantee the model's exact restored context.
    pub fn is_current(&self) -> bool {
        self.current
    }
    pub(crate) fn advertised_session(&self) -> SessionId {
        self.advertised_session
    }
    pub(crate) fn invalidate(&mut self) {
        self.current = false;
    }
}
impl Shared {
    fn images(&self) -> std::sync::MutexGuard<'_, Images> {
        self.images.lock().unwrap_or_else(PoisonError::into_inner)
    }
    /// The first failure is kept. A local close always takes precedence, so a
    /// privacy stop still drops output queued behind an earlier remote failure.
    fn fail(&self, error: Error) {
        self.stop.send_if_modified(|value| {
            if value.is_none()
                || (error == Error::Closed && value.is_some_and(|stop| stop.error != Error::Closed))
            {
                *value = Some(Stopped {
                    error,
                    observed_at: Clock::now(),
                });
                true
            } else {
                false
            }
        });
        self.wake.notify_one();
    }
}

#[derive(Clone)]
pub struct InputSender {
    commands: mpsc::Sender<Command>,
    shared: Arc<Shared>,
    state: watch::Receiver<State>,
}
impl InputSender {
    /// Queue a new admitted request. This synchronously retires earlier output
    /// from this handle; root must ALSO revoke/flush already delivered playback.
    pub fn try_start(&self, request: RequestId) -> Result<()> {
        self.ensure_ready()?;
        if request.get() <= self.shared.retired_through.load(Ordering::Acquire)
            || self
                .shared
                .submitted_through
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                    (request.get() > old).then_some(request.get())
                })
                .is_err()
        {
            return Err(Error::StaleRequest);
        }
        self.shared
            .retired_through
            .fetch_max(request.get() - 1, Ordering::AcqRel);
        self.shared.wake.notify_one();
        self.enqueue(Command::Start(request))
    }
    /// `captured_at` is acquisition time in this process's Instant domain, not
    /// queue receipt time. A process wrapper must preserve IPC acquisition age
    /// when translating its shared OS monotonic timestamps into this domain.
    pub fn try_audio(
        &self,
        request: RequestId,
        sequence: u64,
        captured_at: Instant,
        samples: &[i16],
    ) -> Result<()> {
        self.ensure_ready()?;
        if request.get() <= self.shared.retired_through.load(Ordering::Acquire) {
            return Err(Error::StaleRequest);
        }
        check_age(captured_at, &self.shared.capture_clock)?;
        if samples.is_empty() || samples.len() > MAX_INPUT_SAMPLES {
            return Err(Error::InvalidAudio);
        }
        self.enqueue(Command::Audio(Audio {
            request,
            sequence,
            captured_at,
            samples: samples.to_vec(),
        }))
    }
    /// A whole request in writing: nothing is heard, the service is asked
    /// to answer `text` at once. It takes the place of a start, the audio
    /// and an end, and like a start it retires every earlier request. Only
    /// for a moment when nothing is owed on the connection; one sent while
    /// an earlier answer is unsettled is not written and ends unanswered.
    ///
    /// `picture` is one JPEG to go with the note, for a request that is the
    /// lamp taking a look: it is part of this request and of no other.
    pub fn try_written(
        &self,
        request: RequestId,
        text: &str,
        picture: Option<&[u8]>,
    ) -> Result<()> {
        self.ensure_ready()?;
        if picture.is_some_and(|jpeg| !is_jpeg(jpeg)) {
            return Err(Error::InvalidConfiguration);
        }
        wire::encode_written_turn(text, picture)?;
        if request.get() <= self.shared.retired_through.load(Ordering::Acquire)
            || self
                .shared
                .submitted_through
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                    (request.get() > old).then_some(request.get())
                })
                .is_err()
        {
            return Err(Error::StaleRequest);
        }
        self.shared
            .retired_through
            .fetch_max(request.get() - 1, Ordering::AcqRel);
        self.shared.wake.notify_one();
        self.enqueue(Command::Written(
            request,
            text.into(),
            picture.map(<[u8]>::to_vec),
        ))?;
        self.shared
            .ended_through
            .fetch_max(request.get(), Ordering::AcqRel);
        Ok(())
    }
    /// A written note for `request`, which must be the request whose input
    /// is open: what the lamp is to respond to when nobody spoke (someone
    /// has sat down in front of it), or beside what was said. It is sent
    /// inside the request's activity, after its audio and just before its
    /// end, and only if the request is put to the service at all: a
    /// request that is given up takes its note with it. The newest note
    /// for a request is the one sent.
    pub fn try_note(&self, request: RequestId, text: &str) -> Result<()> {
        self.ensure_ready()?;
        if request.get() <= self.shared.retired_through.load(Ordering::Acquire) {
            return Err(Error::StaleRequest);
        }
        // Checked here so that a refusal reaches the caller, not the socket.
        wire::encode_note(text)?;
        self.enqueue(Command::Note(request, text.into()))
    }
    /// Tell the service something it should know and not answer: no
    /// request, no answer, nothing owed. Written only at a moment when no
    /// request is open and no answer is unsettled; at any other moment it
    /// is dropped, and whoever offered it offers it again later if it still
    /// holds.
    pub fn try_context(&self, text: &str) -> Result<()> {
        self.ensure_ready()?;
        wire::encode_context(text)?;
        self.enqueue(Command::Context(text.into()))
    }
    /// Answer a function call ([`Event::ToolCall`]) by its identifier and
    /// name. An answer that is not an object reaches the service as
    /// `{"result": ...}`. Refused here when it is too large to send. An
    /// answer to a call that is no longer waiting (withdrawn, answered, its
    /// request ended) is accepted here and dropped by the session, which
    /// reports [`Discard::StaleToolResponse`].
    pub fn try_tool_response(
        &self,
        id: &str,
        name: &str,
        response: &serde_json::Value,
    ) -> Result<()> {
        self.ensure_ready()?;
        let silent = self.shared.silent_tools.iter().any(|tool| tool == name);
        let message = wire::encode_tool_response(id, name, response, silent)?;
        self.enqueue(Command::ToolResponse(id.into(), message))
    }
    pub fn try_end(&self, request: RequestId) -> Result<()> {
        self.enqueue(Command::End(request))?;
        self.shared
            .ended_through
            .fetch_max(request.get(), Ordering::AcqRel);
        Ok(())
    }
    /// Offer one JPEG as visual context for `request`, which must be the
    /// request now speaking. `frame` is the caller's own token and comes
    /// back in the [`ImageReceipt`]. The image never enters the audio queue:
    /// it waits in one slot, is sent after audio already queued, inside the
    /// request's own activity, and is dropped unsent if the request ends or
    /// is cancelled first. A refusal changes nothing else.
    ///
    /// `grant` is the caller's number for the camera permission the frame
    /// was captured under, increasing with every new permission. It is what
    /// [`Self::revoke_images`] withdraws.
    pub fn try_image(
        &self,
        request: RequestId,
        frame: u64,
        grant: u64,
        captured_at: Instant,
        jpeg: &[u8],
    ) -> std::result::Result<ImageOffer, ImageRefusal> {
        if self.ensure_ready().is_err() {
            return Err(ImageRefusal::NotConnected);
        }
        if jpeg.len() > MAX_IMAGE_BYTES {
            return Err(ImageRefusal::TooLarge);
        }
        if !is_jpeg(jpeg) {
            return Err(ImageRefusal::NotJpeg);
        }
        if self
            .shared
            .capture_clock
            .now()
            .checked_duration_since(captured_at)
            .is_none_or(|age| age >= MAX_IMAGE_AGE)
        {
            return Err(ImageRefusal::TooOld);
        }
        let id = request.get();
        if id != self.shared.submitted_through.load(Ordering::Acquire)
            || id <= self.shared.retired_through.load(Ordering::Acquire)
            || id <= self.shared.ended_through.load(Ordering::Acquire)
        {
            return Err(ImageRefusal::NotCurrent);
        }
        let mut images = self.shared.images();
        // Checked under the same lock a revocation takes, so an offer and a
        // revocation cannot pass each other.
        if images.revoked(grant) {
            return Err(ImageRefusal::Revoked);
        }
        if images.counted.0 != id {
            images.counted = (id, 0);
        }
        let replacing = match images.pending.as_ref().map(|pending| pending.request) {
            Some(pending) if pending == request => true,
            // An unsent image of an earlier request has nowhere to go.
            Some(_) => {
                images.ledger.dropped += 1;
                false
            }
            None => false,
        };
        if replacing {
            images.ledger.replaced += 1;
        } else if images.counted.1 >= MAX_IMAGES_PER_REQUEST {
            return Err(ImageRefusal::LimitReached);
        } else {
            images.counted.1 += 1;
        }
        images.ledger.accepted += 1;
        images.pending = Some(PendingImage {
            request,
            frame,
            grant,
            captured_at,
            offered_at: Clock::now(),
            jpeg: jpeg.to_vec(),
        });
        drop(images);
        self.shared.wake.notify_one();
        Ok(if replacing {
            ImageOffer::ReplacedUnsent
        } else {
            ImageOffer::Queued
        })
    }
    /// Withdraw every image offered under camera grant `through` or an
    /// earlier one, and refuse any more of them. Synchronous, and independent
    /// of the request: the person's audio is untouched unless an image was
    /// already being written. That write cannot be undone, so it is abandoned
    /// together with its connection, which ends as [`Error::InputCancelled`]
    /// and is replaced like any lost connection. Nothing held behind the
    /// image is sent.
    pub fn revoke_images(&self, through: u64) -> ImageRevocation {
        let mut images = self.shared.images();
        images.revoked_grant = Some(images.revoked_grant.map_or(through, |old| old.max(through)));
        let mut found = ImageRevocation::Nothing;
        if images
            .pending
            .as_ref()
            .is_some_and(|pending| pending.grant <= through)
        {
            images.pending = None;
            images.ledger.revoked += 1;
            found = ImageRevocation::DroppedUnsent;
        }
        if images.in_flight.is_some_and(|grant| grant <= through) {
            images.abandon = true;
            found = ImageRevocation::AbandonedInFlight;
        }
        drop(images);
        if found == ImageRevocation::AbandonedInFlight {
            self.shared.image_abort.notify_one();
        }
        self.shared.wake.notify_one();
        found
    }
    /// What became of the images offered on this connection.
    pub fn image_ledger(&self) -> ImageLedger {
        self.shared.images().ledger
    }
    /// Ask for this connection to end at the next moment nothing is owed on
    /// it, exactly as it does when the service announces a close: an answer
    /// in progress completes first, and the connection then ends with
    /// [`Error::ServerGoAway`] for its owner to replace. For a session that
    /// has stopped being useful (one that has stopped answering, or grown
    /// long) without having failed.
    pub fn renew_when_idle(&self) {
        self.shared.renew.store(true, Ordering::Release);
        self.shared.wake.notify_one();
    }
    /// Whether the service withdrew the call `id` ([`Event::ToolCallsCancelled`]).
    /// Known as soon as the cancellation is read off the connection, even
    /// while its event still waits behind sound nobody has taken: a caller
    /// about to do what a call asked looks here first.
    pub fn call_withdrawn(&self, id: &str) -> bool {
        self.shared
            .withdrawn_calls
            .lock()
            .is_ok_and(|withdrawn| withdrawn.iter().any(|gone| gone == id))
    }
    /// Permanent local retirement is synchronous and cannot be blocked by a full
    /// audio queue. Network interruption follows asynchronously. This does not
    /// revoke permits or flush samples already handed to an external speaker.
    pub fn retire(&self, request: RequestId) {
        self.shared
            .retired_through
            .fetch_max(request.get(), Ordering::AcqRel);
        self.shared.wake.notify_one();
    }
    /// Retirement of input that is abandoned, as opposed to continued by a
    /// newer request. Like [`Self::retire`] it is synchronous and cannot be
    /// blocked by a full queue.
    ///
    /// With [`SuccessorStart::AfterFirstOutput`], whatever of the requests up
    /// to `request` is still kept here, not yet handed to the socket, is
    /// dropped and never written, where `retire` leaves it to be written in
    /// order as the opening of whatever continues it. What has already been
    /// handed to the socket cannot be recalled and is not dropped: it is
    /// counted and reported as sent in [`Event::InputDiscarded`]. Without
    /// that setting nothing is ever kept and this is exactly `retire`.
    pub fn discard(&self, request: RequestId) {
        // Retirement is published first. From that store on no audio of the
        // request is accepted or taken, so that when the session sees the
        // abandonment and drops what it kept, nothing of the request can
        // still arrive behind that. Published the other way round, a block
        // accepted between the two stores outlived the drop and was written.
        self.shared
            .retired_through
            .fetch_max(request.get(), Ordering::AcqRel);
        #[cfg(test)]
        {
            let pause = self
                .shared
                .discard_publication_pause
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(pause) = pause {
                pause.published.send(()).unwrap();
                pause.proceed.recv().unwrap();
            }
        }
        self.shared
            .discarded_through
            .fetch_max(request.get(), Ordering::AcqRel);
        self.shared.wake.notify_one();
    }
    pub fn shutdown(&self) {
        self.shared.fail(Error::Closed);
    }
    /// Whether the input queue has a free place now, so the next command
    /// offered is taken rather than ending the connection as
    /// [`Error::Backpressure`].
    pub fn has_room(&self) -> bool {
        self.has_room_for(1)
    }
    /// Whether the input queue has `commands` free places now, so that many
    /// commands offered one after another are all taken.
    pub fn has_room_for(&self, commands: usize) -> bool {
        self.commands.capacity() >= commands
    }
    fn ensure_ready(&self) -> Result<()> {
        if self.shared.stop.borrow().is_some() || *self.state.borrow() != State::Ready {
            return Err(Error::Closed);
        }
        Ok(())
    }
    fn enqueue(&self, command: Command) -> Result<()> {
        self.ensure_ready()?;
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    self.shared.fail(Error::Backpressure);
                    Error::Backpressure
                }
                mpsc::error::TrySendError::Closed(_) => Error::Closed,
            })
    }
}

pub struct Connection {
    input: InputSender,
    events: mpsc::Receiver<Event>,
    state: watch::Receiver<State>,
    task: JoinHandle<()>,
    ready_at: Clock,
}
impl Connection {
    pub fn input(&self) -> InputSender {
        self.input.clone()
    }
    pub fn state(&self) -> State {
        self.observed_state().0
    }
    /// Source-local observation time, retained even when the supervisor is
    /// backpressured. This is not when a remote network failure occurred.
    pub(crate) fn observed_state(&self) -> (State, Clock) {
        match *self.input.shared.stop.borrow() {
            Some(stop) => (
                if stop.error == Error::Closed {
                    State::Closed
                } else {
                    State::Disconnected(stop.error)
                },
                stop.observed_at,
            ),
            None => (*self.state.borrow(), self.ready_at),
        }
    }
    pub(crate) fn ready_at(&self) -> Clock {
        self.ready_at
    }
    pub fn subscribe_state(&self) -> watch::Receiver<State> {
        self.state.clone()
    }
    pub fn shutdown(&self) {
        self.input.shutdown();
    }
    /// Latest retained resumption point, if the configuration allows retention
    /// and the service has issued one. Remains readable after the connection ends.
    pub fn resumption(&self) -> Option<ResumptionPoint> {
        self.input
            .shared
            .resumption
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub(crate) fn resumption_invalidated(&self) -> bool {
        self.input
            .shared
            .resumption_invalidated
            .load(Ordering::Acquire)
    }
    /// Cancellation-safe receiver. Already queued retired audio/text is dropped,
    /// while lifecycle events retain their original lineage for bookkeeping.
    /// Output received before a remote or transport failure is still delivered,
    /// in order, ahead of the end of the stream: a failure must not truncate an
    /// answer that had already arrived. [`Connection::state`] reports the
    /// failure, and input is refused, as soon as it happens; the stream ends
    /// only after that buffered output has been taken. A local shutdown drops
    /// queued output.
    pub async fn next_event(&mut self) -> Option<Event> {
        while let Some(event) = self.events.recv().await {
            let output_request = match &event {
                Event::Audio { lineage, .. }
                | Event::OutputTranscript { lineage, .. }
                | Event::ModelText { lineage, .. } => Some(lineage.request.get()),
                _ => None,
            };
            if output_request.is_some_and(|id| {
                self.input
                    .shared
                    .stop
                    .borrow()
                    .is_some_and(|stop| stop.error == Error::Closed)
                    || id <= self.input.shared.retired_through.load(Ordering::Acquire)
            }) {
                continue;
            }
            return Some(event);
        }
        None
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.input.shutdown();
        self.task.abort();
    }
}

/// Establish TLS, send setup, and wait for setupComplete before returning Ready.
/// Set to anything, each session says on stderr what every server message
/// looked like: field names and a few schema enumerations, never audio,
/// text or a handle ([`wire::shape`]). For finding out what a service
/// really sends; a run of one shape is said once.
pub const SHAPES_ENVIRONMENT: &str = "LAMP_GEMINI_SHAPES";

/// No implicit reconnect, resumption, tools, or hidden agent task.
pub async fn connect(config: SessionConfig, session: SessionId) -> Result<Connection> {
    config.timeouts.validate()?;
    let request = config.request()?;
    let (socket, _) = timeout(
        config.timeouts.connect,
        connect_async_with_config(request, Some(socket_config()), true),
    )
    .await
    .map_err(|_| Error::ConnectTimeout)?
    .map_err(transport_error)?;
    setup_and_spawn(socket, config, session).await
}

pub(crate) fn socket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(16_384)
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_WIRE_BYTES + 4096)
        .max_message_size(Some(MAX_WIRE_BYTES))
        .max_frame_size(Some(MAX_WIRE_BYTES))
}

pub(crate) async fn setup_and_spawn<S>(
    socket: WebSocketStream<S>,
    config: SessionConfig,
    session: SessionId,
) -> Result<Connection>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    setup_and_spawn_with_capture_clock(socket, config, session, CaptureClockSource::System).await
}

pub(crate) async fn setup_and_spawn_with_capture_clock<S>(
    mut socket: WebSocketStream<S>,
    config: SessionConfig,
    session: SessionId,
    capture_clock: CaptureClockSource,
) -> Result<Connection>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    send(
        &mut socket,
        Message::text(wire::encode_setup(&config)?),
        config.timeouts.write,
        Write::Setup,
    )
    .await?;
    let setup = async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or(Error::PeerClosed { code: None })?
                .map_err(transport_error)?;
            match message {
                Message::Text(text) => {
                    let frame = wire::decode_server(text.as_bytes())?;
                    if frame.setup_complete {
                        return Ok(Clock::now());
                    }
                    if frame.content.is_some()
                        || frame.go_away.is_some()
                        || frame.voice_activity.is_some()
                    {
                        return Err(Error::UnexpectedResponse);
                    }
                }
                Message::Binary(bytes) => {
                    let frame = wire::decode_server(&bytes)?;
                    if frame.setup_complete {
                        return Ok(Clock::now());
                    }
                    if frame.content.is_some()
                        || frame.go_away.is_some()
                        || frame.voice_activity.is_some()
                    {
                        return Err(Error::UnexpectedResponse);
                    }
                }
                Message::Ping(bytes) => {
                    send(
                        &mut socket,
                        Message::Pong(bytes),
                        config.timeouts.write,
                        Write::Keepalive,
                    )
                    .await?
                }
                Message::Pong(_) => {}
                Message::Close(frame) => return Err(closed(frame)),
                Message::Frame(_) => return Err(Error::MalformedMessage),
            }
        }
    };
    let ready_at = timeout(config.timeouts.setup, setup)
        .await
        .map_err(|_| Error::SetupTimeout)??;
    let (commands_tx, commands) = mpsc::channel(INPUT_QUEUE_CAPACITY);
    let (events_tx, events) = mpsc::channel(EVENT_QUEUE_CAPACITY);
    let (state_tx, state) = watch::channel(State::Ready);
    let (stop, stop_rx) = watch::channel(None);
    let shared = Arc::new(Shared {
        capture_clock,
        #[cfg(test)]
        discard_publication_pause: Mutex::new(None),
        retired_through: AtomicU64::new(0),
        discarded_through: AtomicU64::new(0),
        submitted_through: AtomicU64::new(0),
        wake: Notify::new(),
        stop,
        resumption: Mutex::new(None),
        resumption_invalidated: AtomicBool::new(false),
        ended_through: AtomicU64::new(0),
        images: Mutex::new(Images::default()),
        image_abort: Notify::new(),
        renew: AtomicBool::new(false),
        silent_tools: config
            .tools
            .iter()
            .filter(|tool| tool.silent)
            .map(|tool| tool.name.clone())
            .collect(),
        withdrawn_calls: Mutex::new(VecDeque::new()),
    });
    let input = InputSender {
        commands: commands_tx,
        shared: shared.clone(),
        state: state.clone(),
    };
    let actor = Actor {
        socket,
        session,
        timeouts: config.timeouts,
        retain: config.resumption != Resumption::Off,
        silent_calls: Vec::new(),
        shapes: std::env::var_os(SHAPES_ENVIRONMENT).is_some(),
        last_shape: String::new(),
        policy: config.barrier,
        wait: config.barrier_wait,
        reports: config.barrier_reports,
        successor_start: config.successor_start,
        held: None,
        output_buffer: config.output_buffer,
        commands,
        events: events_tx,
        state: state_tx,
        outbox: VecDeque::new(),
        outbox_bytes: 0,
        waiting_since: None,
        gated: false,
        purged_through: 0,
        discard_applied: 0,
        open_activity: OpenActivity::default(),
        shared,
        stop: stop_rx,
        current: None,
        barrier: None,
        last_responder: None,
        stale_terminal: None,
        go_away: false,
        activity_open: false,
        last_request: 0,
        read_at: Clock::now(),
        ping_at: Clock::now(),
    };
    // The actor publishes its terminal state before it lets go of the event
    // sender, so a receiver that sees the end of the stream never reads Ready
    // and loses the sanitized failure, even on a multithreaded runtime.
    let task = tokio::spawn(actor.run());
    Ok(Connection {
        input,
        events,
        state,
        task,
        ready_at,
    })
}

struct Audio {
    request: RequestId,
    sequence: u64,
    captured_at: Instant,
    samples: Vec<i16>,
}
enum Command {
    Start(RequestId),
    Audio(Audio),
    Note(RequestId, String),
    Written(RequestId, String, Option<Vec<u8>>),
    Context(String),
    End(RequestId),
    /// The answer to the call with this identifier, already encoded.
    ToolResponse(String, String),
}
struct InputState {
    open: bool,
    last_sequence: Option<u64>,
    last_capture: Option<Instant>,
}
impl InputState {
    fn new() -> Self {
        Self {
            open: true,
            last_sequence: None,
            last_capture: None,
        }
    }
    fn accept(&mut self, audio: &Audio, clock: &CaptureClockSource) -> Result<()> {
        check_age(audio.captured_at, clock)?;
        if !self.open
            || self
                .last_sequence
                .is_some_and(|last| last.checked_add(1) != Some(audio.sequence))
            || self
                .last_capture
                .is_some_and(|last| audio.captured_at < last)
        {
            return Err(Error::InputSequence);
        }
        self.last_sequence = Some(audio.sequence);
        self.last_capture = Some(audio.captured_at);
        Ok(())
    }
}
struct Turn {
    lineage: Lineage,
    input: InputState,
    /// Cancelled locally while its input was still open. Its End drops it.
    retired: bool,
    /// The person finished, but activityEnd is withheld until an earlier
    /// response has settled.
    end_held: bool,
    /// Since when it has been withheld.
    held_at: Option<Clock>,
    /// activityEnd sent: the service now owes a response or a terminal event.
    /// Until then nothing the service sends can belong to this request.
    ended_at: Option<Clock>,
    first_output_at: Option<Clock>,
    progress_at: Option<Clock>,
    generation_at: Option<Clock>,
    output_samples: u64,
    audio_sequence: u64,
    /// Function calls the service made for this request and still waits on.
    tools: Vec<String>,
    /// The service made calls and has not yet completed the turn it made
    /// them in. That completion ends the model's asking, not the request:
    /// what it says once the calls are answered is still this request's.
    calls_turn_open: bool,
    /// A note to go beside what was said. It is written with the request's
    /// end and not before: a request that is given up leaves its activity
    /// open for the next one to continue, and a note already written would
    /// be read as the next one's.
    note: Option<String>,
}
impl Turn {
    fn new(lineage: Lineage) -> Self {
        Self {
            lineage,
            input: InputState::new(),
            retired: false,
            end_held: false,
            held_at: None,
            ended_at: None,
            first_output_at: None,
            progress_at: None,
            generation_at: None,
            output_samples: 0,
            audio_sequence: 0,
            tools: Vec::new(),
            calls_turn_open: false,
            note: None,
        }
    }
}
/// An earlier response that must settle before the newest request may be
/// committed with activityEnd. The newest request's audio is never held: the
/// service cannot answer an activity before its End, so everything that
/// arrives while a barrier stands belongs to the earlier response.
struct Barrier {
    /// Unknown when stray output arrives with no response on record.
    old: Option<Lineage>,
    /// When the earlier response was asked to stop, or first seen.
    since: Clock,
    /// That moment, then the latest event of the earlier response.
    last_event_at: Clock,
    /// What arrived for the earlier response and was withheld. Counts only.
    seen: BarrierReport,
    /// When the earlier request was committed, for as long as its response
    /// has sent nothing at all. Past `Timeouts::unanswered` from here the
    /// assumption policy takes it as not answered: there is nothing to stop.
    unanswered_since: Option<Clock>,
}
impl Barrier {
    fn new(old: Option<Lineage>) -> Self {
        let now = Clock::now();
        Self {
            old,
            since: now,
            last_event_at: now,
            seen: BarrierReport::default(),
            unanswered_since: None,
        }
    }
    fn count(&mut self, content: &wire::ServerContent) {
        let seen = &mut self.seen;
        if !content.audio.is_empty() {
            seen.audio_frames = seen.audio_frames.saturating_add(1);
            seen.audio_samples = seen
                .audio_samples
                .saturating_add(content.audio.len() as u64);
        }
        if !content.text.is_empty() {
            seen.text_frames = seen.text_frames.saturating_add(1);
        }
        seen.generation_complete = seen
            .generation_complete
            .saturating_add(u32::from(content.generation_complete));
        seen.interrupted = seen
            .interrupted
            .saturating_add(u32::from(content.interrupted));
        seen.completions_not_idle = seen
            .completions_not_idle
            .saturating_add(u32::from(content.turn_complete && !content.idle));
    }
}
/// A next request's `activityStart`, and its audio in order behind it, kept
/// here because the response they would interrupt has produced nothing yet.
struct Held {
    since: Clock,
    /// The start is still kept back. Once it is written, what remains is
    /// audio waiting its turn on the wire; new audio queues behind it.
    holding: bool,
    audio: VecDeque<Audio>,
    samples: usize,
}
/// Audio samples written into the activity now open, which has no End yet,
/// by the request they belong to and in order: what a request that continues
/// the activity will be heard after. One entry per request, oldest first, and
/// never more than [`MAX_OPEN_ACTIVITY_REQUESTS`] of them.
#[derive(Default)]
struct OpenActivity(VecDeque<(u64, u64)>);
impl OpenActivity {
    /// Account for `samples` of `request` about to be sent. Refused, with
    /// nothing changed, when that would take one entry more than the bound:
    /// nothing is sent that could not later be attributed.
    fn note(&mut self, request: u64, samples: u64) -> Result<()> {
        if let Some((_, total)) = self.0.back_mut().filter(|(last, _)| *last == request) {
            *total = total.saturating_add(samples);
            return Ok(());
        }
        if self.0.len() >= MAX_OPEN_ACTIVITY_REQUESTS {
            return Err(Error::OpenActivityOverrun);
        }
        self.0.push_back((request, samples));
        Ok(())
    }
    /// What the requests through `through` have sent. They are abandoned and
    /// this is their report: counted once, and forgotten.
    fn take_through(&mut self, through: u64) -> u64 {
        let mut sent = 0u64;
        while let Some((_, samples)) = self.0.front().filter(|(request, _)| *request <= through) {
            sent = sent.saturating_add(*samples);
            self.0.pop_front();
        }
        sent
    }
    /// The activity started or ended: nothing sent before is in it.
    fn clear(&mut self) {
        self.0.clear();
    }
}
/// Kept audio written per turn of the session loop once the start is out,
/// so that commands, incoming messages and a stop stay live while it drains.
const RELEASE_BLOCKS: usize = 32;
struct Actor<S> {
    socket: WebSocketStream<S>,
    session: SessionId,
    timeouts: Timeouts,
    retain: bool,
    /// Calls of silent functions still to be answered. Their request may
    /// have ended; the answer is owed to the service all the same.
    silent_calls: Vec<String>,
    /// Say on stderr what each server message looked like ([`wire::shape`]).
    shapes: bool,
    last_shape: String,
    policy: BarrierPolicy,
    wait: BarrierWait,
    reports: bool,
    successor_start: SuccessorStart,
    /// A next request's start and audio kept back, or still being written
    /// out after being kept back.
    held: Option<Held>,
    output_buffer: usize,
    commands: mpsc::Receiver<Command>,
    events: mpsc::Sender<Event>,
    state: watch::Sender<State>,
    /// Decoded events the bounded event queue has not accepted yet. Bounded by
    /// `output_buffer` bytes: beyond that the socket is not read.
    outbox: VecDeque<Event>,
    outbox_bytes: usize,
    /// Since when the consumer has accepted nothing while events waited.
    waiting_since: Option<Clock>,
    gated: bool,
    purged_through: u64,
    /// Through which request a discard has been applied.
    discard_applied: u64,
    /// What the activity now open has been sent, by request. Empty unless a
    /// start can be kept: only then is an abandonment reported at all.
    open_activity: OpenActivity,
    shared: Arc<Shared>,
    stop: watch::Receiver<Option<Stopped>>,
    /// The newest request: the only one that accepts input.
    current: Option<Turn>,
    barrier: Option<Barrier>,
    /// Most recent request that produced output: the only possible owner of an
    /// output transcript that trails its audio.
    last_responder: Option<Lineage>,
    /// When a stray `interrupted` was dropped whose companion completion has
    /// not been seen yet. Only honored within `Timeouts::barrier` of it.
    stale_terminal: Option<Clock>,
    go_away: bool,
    activity_open: bool,
    last_request: u64,
    read_at: Clock,
    ping_at: Clock,
}
fn cost(event: &Event) -> usize {
    64 + match event {
        Event::Audio { pcm, .. } => pcm.len() * 2,
        Event::OutputTranscript { text, .. }
        | Event::ModelText { text, .. }
        | Event::UncorrelatedInputTranscript { text, .. } => text.len(),
        // Bounded by the codec; counted at its bound.
        Event::ToolCall { .. } => crate::MAX_TOOL_JSON_BYTES,
        _ => 0,
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> Actor<S> {
    async fn run(mut self) {
        let result = self.serve().await;
        let terminal = match result {
            Ok(()) | Err(Error::Closed) => {
                self.shared.fail(Error::Closed);
                State::Closed
            }
            Err(error) => {
                // Refuse input and report the failure now. Output that had
                // already arrived is still handed over, in order, below.
                self.shared.fail(error);
                State::Disconnected(error)
            }
        };
        self.state.send_replace(terminal);
        // A consumer that has stopped taking events is the failure itself;
        // every other failure leaves earlier, valid output worth delivering.
        if !matches!(result, Ok(()) | Err(Error::Closed | Error::Backpressure)) {
            self.drain().await;
        }
    }
    /// After a failure: keep handing buffered output to a consumer that keeps
    /// taking it, for as long as it does. Nothing is read or sent any more.
    async fn drain(&mut self) {
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            if self
                .stop
                .borrow()
                .is_some_and(|stop| stop.error == Error::Closed)
            {
                return;
            }
            self.purge_retired();
            self.flush();
            if self.outbox.is_empty() || self.delivery_expired() {
                return;
            }
            tokio::select! {
                _ = capacity(&self.events) => {},
                _ = self.shared.wake.notified() => {},
                _ = tick.tick() => {},
            }
        }
    }
    async fn serve(&mut self) -> Result<()> {
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            if let Some(stop) = *self.stop.borrow() {
                return Err(stop.error);
            }
            self.retire_active().await?;
            self.apply_discard();
            self.purge_retired();
            self.enforce_barrier().await?;
            self.drain_held().await?;
            self.flush();
            self.close_unanswered();
            self.check_deadlines()?;
            self.send_image().await?;
            if (self.go_away || self.shared.renew.load(Ordering::Acquire))
                && self.current.is_none()
                && self.barrier.is_none()
                && self.outbox.is_empty()
            {
                // Nothing is owed on this connection: end it at a clean
                // boundary instead of waiting for the announced close.
                return Err(Error::ServerGoAway);
            }
            if self.ping_at.elapsed() >= self.timeouts.keepalive {
                self.write(
                    Message::Ping(Vec::new().into()),
                    self.timeouts.write,
                    Write::Keepalive,
                )
                .await?;
                self.ping_at = Clock::now();
            }
            // Output is buffered up to a fixed amount of audio so the answer is
            // received at network speed whatever the playback speed. Only past
            // that bound does the socket go unread and transport flow control
            // hold the service back. Commands, local retirement and shutdown
            // stay live throughout.
            let waiting = !self.outbox.is_empty();
            let gated = self.gated;
            tokio::select! {
                _ = self.shared.wake.notified() => {},
                _ = tick.tick() => {},
                // This actor is the only sender, so the released slot is still
                // free when the next iteration flushes the outbox into it.
                ready = capacity(&self.events), if waiting => ready?,
                command = self.commands.recv() => match command {
                    // Discard this command if a stop is already recorded.
                    // The next turn of the loop ends the session with that
                    // stop. A write already in progress is ended by `write`.
                    Some(_) if self.stop.borrow().is_some() => {}
                    Some(command) => self.command(command).await?,
                    None => return Ok(()),
                },
                incoming = self.socket.next(), if !gated => {
                    // Likewise a message read after a stop is not acted on:
                    // it could release a held End or answer a ping.
                    if self.stop.borrow().is_some() {
                        continue;
                    }
                    let incoming = incoming
                        .ok_or(Error::PeerClosed { code: None })?
                        .map_err(transport_error)?;
                    self.read_at = Clock::now();
                    self.message(incoming).await?;
                    // One server message per scheduling turn: a burst already in
                    // the socket buffer cannot starve the caller's control tick.
                    tokio::task::yield_now().await;
                }
            }
        }
    }
    /// Send the waiting image, if there is one and it still belongs to the
    /// request now speaking. Audio already queued goes first. While the image
    /// is being written, commands that arrive are held and sent afterwards in
    /// order, so a slow write cannot overflow the command queue.
    ///
    /// Two things stop the write at once and send nothing that was held
    /// behind it: a shutdown, and a revocation of the image's camera grant.
    /// Part of the image may be on the wire by then, so the connection is
    /// abandoned either way and never used again.
    async fn send_image(&mut self) -> Result<()> {
        if self.shared.images().pending.is_none() || !self.commands.is_empty() {
            return Ok(());
        }
        if self.held.is_some() {
            // Behind the kept start and audio: it waits with them, under
            // its own age and revocation rules.
            return Ok(());
        }
        let retired_through = self.retired_through();
        let image = {
            let mut images = self.shared.images();
            let Some(image) = images.pending.take() else {
                return Ok(());
            };
            let current = self.activity_open
                && image.request.get() > retired_through
                && self.current.as_ref().is_some_and(|turn| {
                    turn.lineage.request == image.request && turn.input.open && !turn.retired
                });
            let fresh = self
                .shared
                .capture_clock
                .now()
                .checked_duration_since(image.captured_at)
                .is_some_and(|age| age < MAX_IMAGE_AGE);
            if images.revoked(image.grant) {
                images.ledger.revoked += 1;
                return Ok(());
            }
            if !current || !fresh {
                images.ledger.dropped += 1;
                return Ok(());
            }
            // From here a revocation finds it in flight, under this lock.
            images.in_flight = Some(image.grant);
            images.abandon = false;
            image
        };
        let message = Message::text(wire::encode_image(&image.jpeg)?);
        let started = Clock::now();
        let mut held = VecDeque::new();
        let mut open = true;
        let written = {
            let write = send(&mut self.socket, message, IMAGE_WRITE, Write::Image);
            tokio::pin!(write);
            loop {
                tokio::select! {
                    biased;
                    _ = self.stop.changed() => {
                        if let Some(stop) = *self.stop.borrow() {
                            break Err(stop.error);
                        }
                    }
                    _ = self.shared.image_abort.notified() => {
                        if self.shared.images().abandon {
                            break Err(Error::InputCancelled);
                        }
                    }
                    result = &mut write => break result,
                    command = self.commands.recv(), if open && held.len() < HELD_COMMANDS => {
                        match command {
                            Some(command) => held.push_back(command),
                            None => open = false,
                        }
                    }
                }
            }
        };
        let now = Clock::now();
        // Completion and revocation meet under this lock, and only here. A
        // revocation that found the image in flight has told its caller the
        // connection is abandoned; that holds even if the write then finished
        // in the same instant. Whichever takes the lock first decides: either
        // the image is no longer in flight and the revocation finds nothing,
        // or the abandonment is latched and the write does not count.
        let written = {
            let mut images = self.shared.images();
            images.in_flight = None;
            let written = if std::mem::take(&mut images.abandon) {
                Err(Error::InputCancelled)
            } else {
                written
            };
            match written {
                Ok(()) => {
                    images.ledger.sent += 1;
                    images.ledger.last_sent = Some(ImageReceipt {
                        request: image.request,
                        frame: image.frame,
                        bytes: image.jpeg.len() as u32,
                        waited: now.duration_since(image.offered_at),
                        written_in: now.duration_since(started),
                    });
                }
                Err(Error::InputCancelled) => images.ledger.revoked += 1,
                Err(_) => {}
            }
            written
        };
        // An abandoned write ends the connection here. What was held behind
        // it is dropped with `held`, unsent.
        written?;
        for command in held {
            // A shutdown or failure recorded while the image was written, or
            // while an earlier held command was: nothing further is sent.
            if let Some(stop) = *self.stop.borrow() {
                return Err(stop.error);
            }
            self.command(command).await?;
        }
        Ok(())
    }
    fn delivery_expired(&self) -> bool {
        self.waiting_since
            .is_some_and(|since| since.elapsed() >= self.timeouts.deliver)
    }
    /// A committed request with no output for `Timeouts::unanswered` is one
    /// the service chose not to answer. It ends here without an answer; the
    /// connection and the conversation are kept.
    fn close_unanswered(&mut self) {
        let Some(bound) = self.timeouts.unanswered else {
            return;
        };
        if self.gated {
            // The socket is deliberately unread: an answer may be waiting.
            return;
        }
        let Some(turn) = self.current.as_ref() else {
            return;
        };
        let Some(ended) = turn.ended_at else {
            return;
        };
        let waited = ended.elapsed();
        if turn.first_output_at.is_some() || waited < bound {
            return;
        }
        let lineage = turn.lineage;
        self.current = None;
        self.emit(Event::Unanswered { lineage, waited });
    }
    fn check_deadlines(&self) -> Result<()> {
        if self.delivery_expired() {
            return Err(Error::Backpressure);
        }
        if self.gated {
            // The socket is deliberately unread, so neither liveness nor
            // response progress can be judged until reading resumes.
            return Ok(());
        }
        if self.read_at.elapsed() >= self.timeouts.read {
            return Err(Error::ReadTimeout);
        }
        let Some(turn) = self.current.as_ref() else {
            return Ok(());
        };
        let Some(ended) = turn.ended_at else {
            return Ok(());
        };
        if ended.elapsed() >= self.timeouts.turn {
            return Err(Error::ResponseTimeout);
        }
        match (turn.first_output_at, turn.generation_at) {
            (None, _) => {
                if ended.elapsed() >= self.timeouts.response {
                    return Err(Error::ResponseTimeout);
                }
            }
            (Some(_), None) => {
                if turn
                    .progress_at
                    .is_some_and(|at| at.elapsed() >= self.timeouts.stall)
                {
                    return Err(Error::StalledResponse);
                }
            }
            (Some(first), Some(generated)) => {
                // The service withholds the idle completion while it assumes the
                // generated audio is still playing in real time.
                let playback = Duration::from_micros(
                    turn.output_samples.saturating_mul(1_000_000) / u64::from(OUTPUT_RATE),
                );
                let due = (first + playback).max(generated) + self.timeouts.completion;
                if Clock::now() >= due {
                    return Err(Error::CompletionTimeout);
                }
            }
        }
        Ok(())
    }
    /// Every write the session makes after setup, other than an image, goes
    /// through here, which makes a stop a fence on the wire:
    ///
    /// - No write begins once a stop is recorded, whatever led to it: a
    ///   queued command, a held End released by an incoming completion, a
    ///   keepalive, the answer to a ping.
    /// - A write waiting on the transport is abandoned when a stop is
    ///   recorded. The stop is looked at before the transport on every
    ///   poll, so the transport is not offered another byte after it.
    ///
    /// What the transport had already accepted stays accepted and cannot
    /// be recalled. That can be a whole frame: a write may be waiting on the
    /// transport's flush with every byte of its message already taken. What
    /// had not been accepted is dropped with the connection, which is not
    /// used again. A stop recorded by another thread while this task is
    /// inside a poll is seen at the next poll; that one poll may still hand
    /// the transport the rest of that one message, and nothing after it.
    ///
    /// An image is written by `send_image`, which ends on a stop in the
    /// same way and has its own accounting.
    async fn write(&mut self, message: Message, deadline: Duration, what: Write) -> Result<()> {
        if let Some(stop) = *self.stop.borrow() {
            return Err(stop.error);
        }
        let write = send(&mut self.socket, message, deadline, what);
        tokio::pin!(write);
        loop {
            tokio::select! {
                biased;
                changed = self.stop.changed() => {
                    if let Some(stop) = *self.stop.borrow() {
                        return Err(stop.error);
                    }
                    if changed.is_err() {
                        // Defensive only. The stop channel closing is not
                        // something dropping every input handle can cause:
                        // this session holds the sending side itself,
                        // through `Shared`. Without this arm a closed
                        // channel would make this loop spin.
                        return Err(Error::Closed);
                    }
                }
                result = &mut write => return result,
            }
        }
    }
    async fn start_activity(&mut self) -> Result<()> {
        if !self.activity_open {
            self.write(
                Message::text(wire::ACTIVITY_START),
                self.timeouts.write,
                Write::Activity,
            )
            .await?;
            self.activity_open = true;
            self.open_activity.clear();
        }
        Ok(())
    }
    /// Commit the current request: from here on the service owes it a response.
    ///
    /// Retirement is read again here, at the last moment before the End.
    /// What leads here can have awaited other writes, the kept start and the
    /// audio behind it above all, and a request retired during one of them
    /// must not be committed: its End would ask for an answer nobody owns.
    /// Its audio stays in the open activity, as for any request retired
    /// before its End. A retirement recorded while the End itself is being
    /// written comes too late for this and is the ordinary case of a
    /// committed request being superseded.
    async fn commit(&mut self) -> Result<()> {
        let retired_through = self.retired_through();
        if self
            .current
            .as_ref()
            .is_some_and(|turn| turn.lineage.request.get() <= retired_through)
        {
            self.current = None;
            return Ok(());
        }
        // The note that goes beside what was said, now that it is certain
        // the request is being put to the service.
        if let Some(note) = self.current.as_mut().and_then(|turn| turn.note.take()) {
            self.write(
                Message::text(wire::encode_note(&note)?),
                self.timeouts.write,
                Write::Note,
            )
            .await?;
        }
        self.write(
            Message::text(wire::ACTIVITY_END),
            self.timeouts.write,
            Write::Activity,
        )
        .await?;
        self.activity_open = false;
        self.open_activity.clear();
        if let Some(turn) = self.current.as_mut() {
            turn.end_held = false;
            turn.held_at = None;
            turn.ended_at = Some(Clock::now());
        }
        Ok(())
    }
    fn retired_through(&self) -> u64 {
        self.shared.retired_through.load(Ordering::Acquire)
    }
    async fn retire_active(&mut self) -> Result<()> {
        let retired_through = self.retired_through();
        let Some(turn) = self.current.as_mut() else {
            return Ok(());
        };
        if turn.lineage.request.get() > retired_through {
            return Ok(());
        }
        if turn.ended_at.is_some() {
            // Committed: its response becomes the earlier stream that must
            // settle. Explicit activityStart is the only interruption request
            // available with provider activity detection disabled.
            let produced = turn.first_output_at.is_some();
            let mut barrier = Barrier::new(Some(turn.lineage));
            if !produced {
                barrier.unanswered_since = turn.ended_at;
            }
            self.barrier = Some(barrier);
            self.current = None;
            if matches!(
                self.successor_start,
                SuccessorStart::AfterFirstOutput { .. }
            ) && !produced
            {
                // Nothing to interrupt yet, and an interruption of nothing
                // is not confirmed. Keep the start back until there is.
                self.held = Some(Held {
                    since: Clock::now(),
                    holding: true,
                    audio: VecDeque::new(),
                    samples: 0,
                });
                return Ok(());
            }
            return self.start_activity().await;
        }
        if turn.input.open {
            // No activityEnd was sent, so the service owes this request
            // nothing. Its End drops it without asking for a response.
            turn.retired = true;
        } else {
            // Its End was withheld and now never needs sending.
            self.current = None;
        }
        Ok(())
    }
    async fn enforce_barrier(&mut self) -> Result<()> {
        let Some(barrier) = self.barrier.as_ref() else {
            return Ok(());
        };
        if barrier.since.elapsed() >= self.timeouts.response {
            // Unsettled this long after being asked to stop. Waiting on is
            // not an option under either policy or either wait: the next
            // request would never be committed.
            self.report_barrier(BarrierOutcome::TimedOut);
            return Err(Error::BarrierTimeout);
        }
        if self.policy == BarrierPolicy::AssumeAfterQuiet
            && let Some(bound) = self.timeouts.unanswered
            && barrier
                .unanswered_since
                .is_some_and(|since| since.elapsed() >= bound)
        {
            // Committed that long ago and nothing since: by the rule that
            // closes a current request, the service chose not to answer it.
            // Nothing is left to interrupt or to wait for.
            let superseded = barrier.old.map(|lineage| lineage.request);
            let successor = self.current.as_ref().map(|turn| turn.lineage.request);
            self.barrier = None;
            self.emit(Event::BarrierAssumed {
                session: self.session,
                superseded,
                successor,
            });
            self.release_start(StartRelease::Unanswered).await?;
            return self.release().await;
        }
        if let Some(held) = self.held.as_ref().filter(|held| held.holding) {
            // The earlier response has not been asked to stop yet, so
            // there is no silence after an interruption to count. Only the
            // patience runs, and past it the start is written anyway.
            let expired = matches!(
                self.successor_start,
                SuccessorStart::AfterFirstOutput { patience } if held.since.elapsed() >= patience
            );
            if expired {
                self.release_start(StartRelease::Patience).await?;
            }
            return Ok(());
        }
        // The quiet bound counts from the earlier response's last event. With
        // `AtCommit` it counts only while a request's End is being withheld,
        // and from no earlier than that: only then is anything waiting on
        // the earlier response. That restart protects a person who is still
        // speaking from a failure. An assumption fails nobody, so under it
        // the silence already observed since the interruption counts, and a
        // request spoken for longer than the bound is committed at once.
        let mut quiet_from = barrier.last_event_at;
        if self.wait == BarrierWait::AtCommit {
            match self.current.as_ref().and_then(|turn| turn.held_at) {
                // Nothing is waiting to be committed: only the absolute
                // bound above applies.
                None => return Ok(()),
                Some(held_at) if self.policy == BarrierPolicy::Require => {
                    quiet_from = quiet_from.max(held_at);
                }
                Some(_) => {}
            }
        }
        if quiet_from.elapsed() < self.timeouts.barrier {
            return Ok(());
        }
        match self.policy {
            BarrierPolicy::Require => {
                self.report_barrier(BarrierOutcome::TimedOut);
                Err(Error::BarrierTimeout)
            }
            BarrierPolicy::AssumeAfterQuiet => {
                // Asked to stop and silent for the whole bound. The service
                // never confirmed it, so whatever follows is flagged.
                let superseded = barrier.old.map(|lineage| lineage.request);
                let successor = self.current.as_ref().map(|turn| turn.lineage.request);
                self.barrier = None;
                self.emit(Event::BarrierAssumed {
                    session: self.session,
                    superseded,
                    successor,
                });
                self.release().await
            }
        }
    }
    /// Say how a barrier ended and what stood behind it. Call before the
    /// barrier is taken down.
    fn report_barrier(&mut self, outcome: BarrierOutcome) {
        if !self.reports {
            return;
        }
        let Some(barrier) = self.barrier.as_ref() else {
            return;
        };
        let current = self.current.as_ref();
        let report = BarrierReport {
            superseded: barrier.old.map(|lineage| lineage.request),
            successor: current.map(|turn| turn.lineage.request),
            waited: barrier.since.elapsed(),
            quiet: barrier.last_event_at.elapsed(),
            successor_input_open: current.is_some_and(|turn| turn.input.open),
            end_withheld: current
                .and_then(|turn| turn.held_at)
                .map(|held_at| held_at.elapsed()),
            ..barrier.seen
        };
        self.emit(Event::Barrier {
            session: self.session,
            outcome,
            report,
        });
    }
    /// The earlier response has settled: a withheld End may go out now.
    async fn release(&mut self) -> Result<()> {
        if self.barrier.is_none()
            && self.held.is_none()
            && self.current.as_ref().is_some_and(|turn| turn.end_held)
        {
            self.commit().await?;
        }
        Ok(())
    }
    fn holding(&self) -> bool {
        self.held.as_ref().is_some_and(|held| held.holding)
    }
    /// A block is about to be handed to the socket in the open activity.
    /// One that cannot be accounted for is refused here, before it is sent.
    fn note_sent(&mut self, audio: &Audio) -> Result<()> {
        if !matches!(
            self.successor_start,
            SuccessorStart::AfterFirstOutput { .. }
        ) {
            // Nothing is kept, so an abandonment reports nothing: no account.
            return Ok(());
        }
        self.open_activity
            .note(audio.request.get(), audio.samples.len() as u64)
    }
    /// Apply an abandonment recorded by [`InputSender::discard`]: drop what
    /// is still kept of the abandoned requests, and say what was dropped and
    /// what had already gone. Only where something can be kept at all;
    /// otherwise an abandonment is a retirement and nothing more.
    fn apply_discard(&mut self) {
        let through = self.shared.discarded_through.load(Ordering::Acquire);
        let applied = self.discard_applied;
        if through <= applied {
            return;
        }
        self.discard_applied = through;
        if !matches!(
            self.successor_start,
            SuccessorStart::AfterFirstOutput { .. }
        ) {
            return;
        }
        let mut dropped = 0;
        if let Some(held) = self.held.as_mut() {
            held.audio.retain(|audio| {
                let abandoned = audio.request.get() <= through;
                if abandoned {
                    dropped += audio.samples.len();
                }
                !abandoned
            });
            held.samples -= dropped;
        }
        let dropped = dropped as u64;
        if let Some(barrier) = self.barrier.as_mut() {
            barrier.seen.discarded_samples = barrier.seen.discarded_samples.saturating_add(dropped);
        }
        // What was handed to the socket before this is in an activity that
        // has no End: it cannot be taken back and is not called discarded.
        // Only the abandoned requests' own samples: a newer request's audio
        // in the same activity is not theirs. Reported here once and no
        // longer remembered.
        let sent = self.open_activity.take_through(through);
        if (dropped > 0 || sent > 0)
            && let Ok(through) = RequestId::new(through)
        {
            self.emit(Event::InputDiscarded {
                session: self.session,
                through,
                dropped_samples: dropped,
                sent_samples: sent,
            });
        }
    }
    /// Stop keeping the next request's start back: write it, and let what
    /// was kept behind it follow in order. With a barrier still standing
    /// this is the interruption, and its quiet bound counts from here.
    async fn release_start(&mut self, by: StartRelease) -> Result<()> {
        let Some(held) = self.held.as_mut().filter(|held| held.holding) else {
            return Ok(());
        };
        held.holding = false;
        let (kept, samples) = (held.since.elapsed(), held.samples as u64);
        if let Some(barrier) = self.barrier.as_mut() {
            barrier.last_event_at = Clock::now();
            barrier.seen.start_held = Some(kept);
            barrier.seen.start_released_by = Some(by);
            barrier.seen.retained_samples = samples;
        }
        self.start_activity().await?;
        self.drain_held().await
    }
    /// Write kept audio, oldest first, a bounded number of blocks at a time.
    /// When none is left, a withheld End may go out if nothing else holds it.
    async fn drain_held(&mut self) -> Result<()> {
        let SuccessorStart::AfterFirstOutput { patience } = self.successor_start else {
            return Ok(());
        };
        // Kept here on purpose for up to `patience`: that much older than
        // audio is otherwise allowed to be, and no older.
        let bound = MAX_INPUT_AGE + patience;
        for _ in 0..RELEASE_BLOCKS {
            // An abandonment recorded while the block before this one was
            // being written is applied before this one is taken.
            self.apply_discard();
            let Some(held) = self.held.as_mut().filter(|held| !held.holding) else {
                return Ok(());
            };
            let Some(audio) = held.audio.pop_front() else {
                break;
            };
            held.samples -= audio.samples.len();
            let age = self
                .shared
                .capture_clock
                .now()
                .checked_duration_since(audio.captured_at)
                .filter(|age| *age < bound)
                .ok_or(Error::StaleInput)?;
            let deadline = self.timeouts.write.min(bound - age);
            // An abandoned request's block cannot be here: its retirement
            // was published before its abandonment, so none was accepted
            // after the drop. Were one here all the same, it is not written.
            if audio.request.get() <= self.discard_applied {
                continue;
            }
            // Handed to the socket from here: no longer droppable.
            self.note_sent(&audio)?;
            self.write(
                Message::text(wire::encode_audio(&audio.samples)?),
                deadline,
                Write::Audio,
            )
            .await?;
        }
        if self
            .held
            .as_ref()
            .is_some_and(|held| !held.holding && held.audio.is_empty())
        {
            self.held = None;
            return self.release().await;
        }
        Ok(())
    }
    async fn command(&mut self, command: Command) -> Result<()> {
        match command {
            Command::Start(request) => {
                if request.get() <= self.last_request
                    || self.current.as_ref().is_some_and(|turn| turn.input.open)
                {
                    return Err(Error::OverlappingInput);
                }
                self.last_request = request.get();
                let lineage = Lineage {
                    session: self.session,
                    request,
                };
                // try_start retired every earlier request. A committed one
                // becomes the barrier; one whose End was withheld was never
                // committed, and its audio stays in the activity this request
                // continues. It cannot receive a response of its own.
                self.retire_active().await?;
                self.current = Some(Turn::new(lineage));
                if !self.holding() {
                    self.start_activity().await?;
                }
                self.emit(Event::InputStarted {
                    lineage,
                    waiting_for_barrier: self.barrier.is_some(),
                });
            }
            Command::Audio(audio) => {
                if audio.request.get() <= self.retired_through() {
                    return Ok(());
                }
                let turn = self
                    .current
                    .as_mut()
                    .filter(|turn| turn.lineage.request == audio.request)
                    .ok_or(Error::StaleRequest)?;
                turn.input.accept(&audio, &self.shared.capture_clock)?;
                let age = check_age(audio.captured_at, &self.shared.capture_clock)?;
                if let Some(held) = self.held.as_mut() {
                    // Behind the kept start, or behind kept audio that is
                    // still being written: in order either way.
                    held.samples += audio.samples.len();
                    held.audio.push_back(audio);
                    if held.samples > MAX_RETAINED_SAMPLES {
                        if !held.holding {
                            // Being written out and still not keeping up.
                            return Err(Error::Backpressure);
                        }
                        self.release_start(StartRelease::Overflow).await?;
                    }
                    return Ok(());
                }
                let deadline = self.timeouts.write.min(MAX_INPUT_AGE - age);
                self.note_sent(&audio)?;
                self.write(
                    Message::text(wire::encode_audio(&audio.samples)?),
                    deadline,
                    Write::Audio,
                )
                .await?;
            }
            Command::Written(request, text, picture) => {
                if request.get() <= self.last_request
                    || self.current.as_ref().is_some_and(|turn| turn.input.open)
                {
                    return Err(Error::OverlappingInput);
                }
                self.last_request = request.get();
                let lineage = Lineage {
                    session: self.session,
                    request,
                };
                self.retire_active().await?;
                // Written only onto a connection that owes nothing: with an
                // earlier answer unsettled, or an activity left open, what
                // came back could not be told apart. The request then stands
                // committed and unanswered, and is closed as such.
                let clear = self.barrier.is_none() && self.held.is_none() && !self.activity_open;
                let mut turn = Turn::new(lineage);
                turn.input.open = false;
                turn.ended_at = Some(Clock::now());
                self.current = Some(turn);
                if clear {
                    self.write(
                        Message::text(wire::encode_written_turn(&text, picture.as_deref())?),
                        self.timeouts.write,
                        if picture.is_some() {
                            Write::Picture
                        } else {
                            Write::Written
                        },
                    )
                    .await?;
                }
                self.emit(Event::InputStarted {
                    lineage,
                    waiting_for_barrier: !clear,
                });
            }
            Command::Context(text) => {
                // Only when nothing is owed, as for a renewal: inside a
                // request it would be taken for part of what was asked, and
                // behind an unsettled answer for the next thing said.
                let between = self.current.is_none()
                    && self.barrier.is_none()
                    && self.held.is_none()
                    && !self.activity_open
                    && self.outbox.is_empty();
                if between {
                    self.write(
                        Message::text(wire::encode_context(&text)?),
                        self.timeouts.write,
                        Write::Context,
                    )
                    .await?;
                }
            }
            Command::Note(request, text) => {
                if request.get() <= self.retired_through() {
                    return Ok(());
                }
                // Kept until the request is committed. A request that is
                // given up never has it written.
                wire::encode_note(&text)?;
                self.current
                    .as_mut()
                    .filter(|turn| turn.lineage.request == request && turn.input.open)
                    .ok_or(Error::StaleRequest)?
                    .note = Some(text);
            }
            Command::End(request) => {
                let retired_through = self.retired_through();
                let turn = self
                    .current
                    .as_mut()
                    .filter(|turn| turn.lineage.request == request && turn.input.open)
                    .ok_or(Error::StaleRequest)?;
                turn.input.open = false;
                if turn.retired || request.get() <= retired_through {
                    // Cancelled before commit. Sending activityEnd would ask
                    // for a response nobody owns; leave the activity open for
                    // the next admitted request to continue.
                    self.current = None;
                } else if self.barrier.is_some() || self.held.is_some() {
                    // Withheld until the earlier response settles, so output
                    // after this End can only belong to this request; and
                    // until every block kept before it has been written.
                    turn.end_held = true;
                    turn.held_at = Some(Clock::now());
                } else {
                    self.commit().await?;
                }
            }
            Command::ToolResponse(id, message) => {
                let waiting = self
                    .current
                    .as_mut()
                    .and_then(|turn| {
                        let index = turn.tools.iter().position(|pending| *pending == id)?;
                        turn.tools.swap_remove(index);
                        // The service generates again from here: the silence
                        // it is allowed is counted from the answer.
                        turn.progress_at = Some(Clock::now());
                        Some(())
                    })
                    .is_some();
                let silent = self
                    .silent_calls
                    .iter()
                    .position(|pending| *pending == id)
                    .map(|index| self.silent_calls.swap_remove(index))
                    .is_some();
                if !waiting && !silent {
                    self.discard(Discard::StaleToolResponse);
                    return Ok(());
                }
                self.write(Message::text(message), self.timeouts.write, Write::Tool)
                    .await?;
            }
        }
        Ok(())
    }
    /// Function calls belong to the committed request, like any output. The
    /// service sends nothing more for it until each is answered, so a call
    /// nobody can own is answered here as not done, with why
    /// ([`crate::NOT_DONE_STILL_SPEAKING`]), and not reported.
    async fn tool_calls(&mut self, calls: Vec<crate::ToolCall>) -> Result<()> {
        // Apply any local retirement first so ownership below is current.
        self.retire_active().await?;
        let owner = self
            .current
            .as_mut()
            .filter(|turn| turn.ended_at.is_some() && turn.tools.len() < crate::MAX_TOOL_CALLS);
        let Some(turn) = owner else {
            for call in &calls {
                let silent = self.shared.silent_tools.contains(&call.name);
                let message = wire::encode_tool_response(
                    &call.id,
                    &call.name,
                    &serde_json::json!({"not_done": crate::NOT_DONE_STILL_SPEAKING}),
                    silent,
                )?;
                self.write(Message::text(message), self.timeouts.write, Write::Tool)
                    .await?;
            }
            self.discard(Discard::UnownedToolCall);
            return Ok(());
        };
        let lineage = turn.lineage;
        let now = Clock::now();
        turn.first_output_at.get_or_insert(now);
        turn.progress_at = Some(now);
        // A call after a completed generation reopens it, as audio does.
        turn.generation_at = None;
        // The service says nothing more on account of a silent function,
        // so the turn it asks in is the last of the request, and its
        // answer is owed whether or not the request is still there.
        for call in &calls {
            if self.shared.silent_tools.contains(&call.name) {
                if self.silent_calls.len() == crate::MAX_TOOL_CALLS {
                    self.silent_calls.remove(0);
                }
                self.silent_calls.push(call.id.clone());
            } else {
                turn.tools.push(call.id.clone());
                turn.calls_turn_open = true;
            }
        }
        self.stale_terminal = None;
        self.last_responder = Some(lineage);
        for call in calls {
            self.emit(Event::ToolCall { lineage, call });
        }
        Ok(())
    }
    fn decode(&mut self, bytes: &[u8]) -> Result<wire::ServerFrame> {
        if self.shapes {
            // A run of messages of one shape, such as an answer's audio, is
            // said once.
            let shape = wire::shape(bytes);
            if shape != self.last_shape {
                eprintln!("lamp-gemini session {}: {shape}", self.session.get());
                self.last_shape = shape;
            }
        }
        let decoded = if self.retain {
            wire::decode_server_retaining(bytes)
        } else {
            wire::decode_server(bytes)
        };
        if let Err(
            error @ (Error::MalformedMessage
            | Error::UnsupportedMessage
            | Error::MessageTooLarge
            | Error::InvalidAudio),
        ) = &decoded
        {
            // A message this codec could not read, not a refusal by the
            // service. What was received before a failure is still
            // delivered, so this says what that message looked like.
            self.emit(Event::Refused {
                session: self.session,
                error: *error,
                shape: wire::shape(bytes),
            });
        }
        decoded
    }
    async fn message(&mut self, message: Message) -> Result<()> {
        let frame = match message {
            Message::Text(text) => self.decode(text.as_bytes())?,
            Message::Binary(bytes) => self.decode(&bytes)?,
            Message::Ping(bytes) => {
                self.write(Message::Pong(bytes), self.timeouts.write, Write::Keepalive)
                    .await?;
                return Ok(());
            }
            Message::Pong(_) => return Ok(()),
            Message::Close(frame) => return Err(closed(frame)),
            Message::Frame(_) => return Err(Error::MalformedMessage),
        };
        if frame.setup_complete {
            return Err(Error::UnexpectedResponse);
        }
        if let Some(update) = frame.resumption {
            if let Some(barrier) = self.barrier.as_mut() {
                let count = if update.resumable {
                    &mut barrier.seen.resumable_updates
                } else {
                    &mut barrier.seen.unresumable_updates
                };
                *count = count.saturating_add(1);
            }
            self.shared
                .resumption_invalidated
                .store(update.handle.is_none(), Ordering::Release);
            let mut point = self
                .shared
                .resumption
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match update.handle {
                Some(handle) => {
                    *point = Some(ResumptionPoint {
                        handle,
                        current: true,
                        advertised_session: self.session,
                    });
                }
                // The previous handle still resumes, but only up to its own point.
                None => {
                    if let Some(point) = point.as_mut() {
                        point.current = false;
                    }
                }
            }
        }
        if let Some(notice) = frame.go_away
            && !self.go_away
        {
            self.go_away = true;
            self.emit(Event::GoAway {
                session: self.session,
                time_left: notice.time_left,
            });
        }
        if let Some(usage) = frame.usage {
            self.emit(Event::Usage {
                session: self.session,
                usage,
            });
        }
        if let Some(activity) = frame.voice_activity {
            self.emit(Event::VoiceActivity {
                session: self.session,
                kind: activity.kind,
                audio_offset: activity.audio_offset,
            });
        }
        if !frame.tool_cancellations.is_empty() {
            if let Some(turn) = self.current.as_mut() {
                turn.tools
                    .retain(|pending| !frame.tool_cancellations.contains(pending));
            }
            self.silent_calls
                .retain(|pending| !frame.tool_cancellations.contains(pending));
            if let Ok(mut withdrawn) = self.shared.withdrawn_calls.lock() {
                for id in &frame.tool_cancellations {
                    if withdrawn.len() == WITHDRAWN_KEPT {
                        withdrawn.pop_front();
                    }
                    withdrawn.push_back(id.clone());
                }
            }
            self.emit(Event::ToolCallsCancelled {
                session: self.session,
                ids: frame.tool_cancellations,
            });
        }
        if !frame.tool_calls.is_empty() {
            self.tool_calls(frame.tool_calls).await?;
        }
        if let Some(content) = frame.content {
            self.content(content).await?;
        }
        Ok(())
    }
    fn discard(&mut self, reason: Discard) {
        self.emit(Event::Discarded {
            session: self.session,
            reason,
        });
    }
    /// The service gives output transcripts no ordering relative to audio or
    /// completion. One that arrives before the current request has produced
    /// output cannot describe it, so it stays with the previous responder.
    fn output_transcript(&mut self, transcript: wire::Transcript) {
        let owner = match &self.current {
            Some(turn)
                if turn.ended_at.is_some()
                    && (turn.first_output_at.is_some() || self.last_responder.is_none()) =>
            {
                Some(turn.lineage)
            }
            _ => self.last_responder,
        };
        match owner {
            Some(lineage) => self.emit(Event::OutputTranscript {
                lineage,
                text: transcript.text,
                finished: transcript.finished,
            }),
            None => self.discard(Discard::UnownedOutput),
        }
    }
    async fn content(&mut self, mut content: wire::ServerContent) -> Result<()> {
        if let Some(transcript) = content.input_transcript.take() {
            self.emit(Event::UncorrelatedInputTranscript {
                session: self.session,
                text: transcript.text,
                finished: transcript.finished,
            });
        }
        if content.waiting_for_input {
            if let Some(barrier) = self.barrier.as_mut() {
                barrier.seen.waiting_for_input = barrier.seen.waiting_for_input.saturating_add(1);
            }
            let awaiting = self.current.as_ref().and_then(|turn| {
                let ended = turn.ended_at?;
                turn.first_output_at
                    .is_none()
                    .then(|| (turn.lineage, ended.elapsed()))
            });
            self.emit(Event::WaitingForInput {
                session: self.session,
                awaiting,
            });
        }
        let transcript = content.output_transcript.take();
        let payload = !content.audio.is_empty() || !content.text.is_empty();
        let terminal = content.interrupted || content.turn_complete;
        if payload || terminal || content.generation_complete {
            // Apply any local retirement first so ownership below is current.
            self.retire_active().await?;
            if self
                .current
                .as_ref()
                .is_some_and(|turn| turn.ended_at.is_some())
            {
                self.response(content, payload);
            } else {
                self.earlier(content, payload).await?;
            }
        }
        if let Some(transcript) = transcript {
            self.output_transcript(transcript);
        }
        Ok(())
    }
    /// No request is committed, so the service cannot be answering one.
    /// Everything here belongs to an earlier response.
    async fn earlier(&mut self, content: wire::ServerContent, payload: bool) -> Result<()> {
        let now = Clock::now();
        let producing = payload || content.generation_complete;
        let settled = content.turn_complete && content.idle;
        let known = self.barrier.as_ref().and_then(|barrier| barrier.old);
        if let Some(barrier) = self.barrier.as_mut() {
            barrier.count(&content);
            // It has sent something: no longer a response that never came.
            barrier.unanswered_since = None;
        }
        if let Some(lineage) = known {
            // A response this client retired. Its output is dropped here; its
            // lifecycle is still reported under its own name.
            if producing {
                self.last_responder = Some(lineage);
            }
            if content.interrupted {
                self.emit(Event::Interrupted { lineage });
            }
            if content.turn_complete {
                self.emit(Event::TurnComplete {
                    lineage,
                    idle: content.idle,
                });
            }
        } else {
            if content.interrupted && !content.turn_complete {
                self.stale_terminal = Some(now);
            }
            self.discard(if content.interrupted {
                Discard::LateInterruption
            } else if self.current.is_none() {
                Discard::UnownedOutput
            } else if producing {
                Discard::LateOutput
            } else {
                Discard::LateTerminal
            });
        }
        if settled {
            if self.holding()
                && let Some(barrier) = self.barrier.as_mut()
            {
                // Completed with nothing to interrupt: recorded before the
                // report, written after the barrier is down.
                barrier.seen.start_held = self.held.as_ref().map(|held| held.since.elapsed());
                barrier.seen.start_released_by = Some(StartRelease::Completion);
                barrier.seen.retained_samples =
                    self.held.as_ref().map_or(0, |held| held.samples as u64);
            }
            self.report_barrier(BarrierOutcome::Settled);
            if self.barrier.take().is_some() {
                self.release_start(StartRelease::Completion).await?;
                self.release().await?;
            }
        } else if let Some(barrier) = self.barrier.as_mut() {
            barrier.last_event_at = now;
            if producing {
                // The earlier response has begun: now there is something
                // to interrupt, and an interruption that is confirmed.
                self.release_start(StartRelease::FirstOutput).await?;
            }
        } else if producing {
            // A response is still producing after its barrier, or with none
            // on record. Hold the next commit until it settles, exactly as
            // for a response this client retired itself.
            self.barrier = Some(Barrier::new(None));
        }
        Ok(())
    }
    /// The current request is committed and no earlier response is pending.
    fn response(&mut self, content: wire::ServerContent, payload: bool) {
        let barrier = self.timeouts.barrier;
        let Some(turn) = self.current.as_mut() else {
            return;
        };
        let lineage = turn.lineage;
        let now = Clock::now();
        if content.interrupted {
            // This client has not asked to interrupt this request, and nothing
            // else can with provider activity detection disabled. The event is
            // the delayed result of an earlier activityStart; it must neither
            // cancel this request nor confirm that anyone spoke.
            if !content.turn_complete {
                self.stale_terminal = Some(now);
            }
            return self.discard(Discard::LateInterruption);
        }
        if payload || content.generation_complete {
            self.stale_terminal = None;
            turn.first_output_at.get_or_insert(now);
            turn.progress_at = Some(now);
            self.last_responder = Some(lineage);
        }
        let sequence = turn.audio_sequence;
        if !content.audio.is_empty() {
            turn.audio_sequence = sequence.saturating_add(1);
            turn.output_samples = turn
                .output_samples
                .saturating_add(content.audio.len() as u64);
            // Audio after a completed generation reopens it.
            turn.generation_at = None;
        }
        // The service ends the turn in which it made calls as soon as it
        // has made them, and answers in another once they are answered. On
        // lamp-4ace on 2026-10-13 that completion came 4 ms after the call.
        // It is the end of the asking, not of the request.
        let asking = turn.calls_turn_open;
        if asking && content.turn_complete {
            turn.calls_turn_open = false;
            turn.progress_at = Some(now);
        }
        if content.generation_complete && !asking {
            turn.generation_at = Some(now);
        }
        let answered = turn.first_output_at.is_some();
        if !content.audio.is_empty() {
            self.emit(Event::Audio {
                lineage,
                sequence,
                pcm: content.audio,
            });
        }
        for text in content.text {
            self.emit(Event::ModelText { lineage, text });
        }
        if asking {
            return;
        }
        if content.generation_complete {
            self.emit(Event::GenerationComplete { lineage });
        }
        if content.turn_complete {
            let companion = self
                .stale_terminal
                .take()
                .is_some_and(|at| at.elapsed() < barrier);
            if companion && !answered {
                // Companion of the stray interruption dropped just before.
                return self.discard(Discard::LateTerminal);
            }
            self.emit(Event::TurnComplete {
                lineage,
                idle: content.idle,
            });
            if content.idle {
                self.current = None;
            } else if let Some(turn) = self.current.as_mut() {
                turn.progress_at = Some(now);
            }
        }
    }
    fn emit(&mut self, event: Event) {
        self.outbox_bytes += cost(&event);
        self.outbox.push_back(event);
        self.flush();
    }
    /// Retired output still waiting here would only be skipped by the receiver
    /// later. Drop it now so a cancelled long answer frees its buffer at once.
    fn purge_retired(&mut self) {
        let retired_through = self.retired_through();
        if retired_through == self.purged_through {
            return;
        }
        self.purged_through = retired_through;
        self.outbox.retain(|event| match event {
            Event::Audio { lineage, .. }
            | Event::OutputTranscript { lineage, .. }
            | Event::ModelText { lineage, .. } => lineage.request.get() > retired_through,
            _ => true,
        });
        self.outbox_bytes = self.outbox.iter().map(cost).sum();
    }
    fn flush(&mut self) {
        let mut accepted = false;
        while let Some(event) = self.outbox.pop_front() {
            let bytes = cost(&event);
            match self.events.try_send(event) {
                Ok(()) => {
                    accepted = true;
                    self.outbox_bytes = self.outbox_bytes.saturating_sub(bytes);
                }
                Err(mpsc::error::TrySendError::Full(event)) => {
                    self.outbox.push_front(event);
                    break;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    self.outbox.clear();
                    self.outbox_bytes = 0;
                    break;
                }
            }
        }
        // The delivery bound measures a consumer taking nothing at all, not
        // one that is slower than the network, as real-time playback always is.
        if self.outbox.is_empty() {
            self.waiting_since = None;
        } else if accepted || self.waiting_since.is_none() {
            self.waiting_since = Some(Clock::now());
        }
        let gated = self.outbox_bytes >= self.output_buffer;
        if self.gated && !gated {
            // Time spent not reading is this client's doing, not a stall.
            let now = Clock::now();
            self.read_at = now;
            if let Some(progress) = self
                .current
                .as_mut()
                .and_then(|turn| turn.progress_at.as_mut())
            {
                *progress = now;
            }
        }
        self.gated = gated;
    }
}

async fn capacity(events: &mpsc::Sender<Event>) -> Result<()> {
    events.reserve().await.map(drop).map_err(|_| Error::Closed)
}

/// Quota and usage refusals are recognised from the close reason so they are
/// not retried as ordinary disconnects. The reason selects a fixed category
/// and is dropped here; it is never retained, logged or returned.
fn closed(frame: Option<tungstenite::protocol::CloseFrame>) -> Error {
    let Some(frame) = frame else {
        return Error::PeerClosed { code: None };
    };
    let reason = frame.reason.to_ascii_lowercase();
    if [
        "quota",
        "resource_exhausted",
        "resource exhausted",
        "usage limit",
        "rate limit",
    ]
    .iter()
    .any(|marker| reason.contains(marker))
    {
        return Error::QuotaExceeded;
    }
    Error::PeerClosed {
        code: Some(frame.code.into()),
    }
}

fn check_age(captured_at: Instant, clock: &CaptureClockSource) -> Result<Duration> {
    clock
        .now()
        .checked_duration_since(captured_at)
        .filter(|age| *age < MAX_INPUT_AGE)
        .ok_or(Error::StaleInput)
}
/// Write `message` to the service within `deadline`. One that does not
/// finish in time is `write`, named in its [`Error::WriteTimeout`] with its
/// size and deadline.
async fn send<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    message: Message,
    deadline: Duration,
    write: Write,
) -> Result<()> {
    let kib = u32::try_from(message.len().div_ceil(1024)).unwrap_or(u32::MAX);
    let deadline_ms = u32::try_from(deadline.as_millis()).unwrap_or(u32::MAX);
    timeout(deadline, socket.send(message))
        .await
        .map_err(|_| Error::WriteTimeout {
            write,
            kib,
            deadline_ms,
        })?
        .map_err(transport_error)
}
fn transport_error(error: tungstenite::Error) -> Error {
    let mapped = match &error {
        tungstenite::Error::Http(response) => match response.status().as_u16() {
            401 | 403 => Error::Authentication,
            429 => Error::RateLimited,
            500..=599 => Error::ServerUnavailable,
            _ => Error::Transport,
        },
        tungstenite::Error::Capacity(_) => Error::MessageTooLarge,
        tungstenite::Error::WriteBufferFull(_) => Error::Backpressure,
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
            Error::PeerClosed { code: None }
        }
        _ => Error::Transport,
    };
    // `Transport` keeps no cause, so it is said here (#705: lamp-4ace's 166
    // transport drops could not be told apart).
    if mapped == Error::Transport {
        eprintln!("lamp-gemini: the link failed: {}", transport_cause(&error));
    }
    mapped
}

/// What failed under the link, by kind: never the URL, which carries the
/// key, so a URL error and an HTTP answer are named, not quoted.
fn transport_cause(error: &tungstenite::Error) -> String {
    match error {
        tungstenite::Error::Io(io) => format!("io {:?}: {io}", io.kind()),
        tungstenite::Error::Tls(tls) => format!("tls: {tls}"),
        tungstenite::Error::Protocol(protocol) => format!("protocol: {protocol}"),
        tungstenite::Error::Http(response) => format!("http {}", response.status().as_u16()),
        tungstenite::Error::Url(_) => "url".to_owned(),
        tungstenite::Error::Utf8(_) => "utf8".to_owned(),
        tungstenite::Error::AttackAttempt => "attack attempt".to_owned(),
        _ => "other".to_owned(),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod capture_clock_tests;
