//! What became of one chat request's connection, as far as the lamp can know
//! it: whether the client opened a new one for it or took one it kept, how
//! long a new one took to open, and how far the request got. The client says
//! none of that for a request that never answers, which is the one that
//! matters (#4710), so a layer on its connector watches each opening. A
//! connection is watched, never touched: what is sent stays the same.
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tower_layer::Layer;
use tower_service::Service;

/// How far the newest connection a client began has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opening {
    Going,
    Opened(Duration),
    Failed(Duration),
}

/// Told how a new connection's opening ended.
pub type OpeningTold = Arc<dyn Fn(Opening) + Send + Sync>;

/// A client's connector, watched ([`Layer`]).
#[derive(Clone, Default)]
pub(crate) struct Opens(Arc<OpensState>);

#[derive(Default)]
struct OpensState {
    began: AtomicU64,
    newest: Mutex<Option<Opening>>,
    told: Option<OpeningTold>,
}

impl Opens {
    fn telling(told: OpeningTold) -> Self {
        Self(Arc::new(OpensState {
            told: Some(told),
            ..OpensState::default()
        }))
    }

    /// How many connections the client has begun to open.
    pub(crate) fn began(&self) -> u64 {
        self.0.began.load(Ordering::SeqCst)
    }

    fn newest(&self) -> Option<Opening> {
        self.0.newest.lock().ok().and_then(|newest| *newest)
    }

    fn set(&self, opening: Opening) {
        if let Ok(mut newest) = self.0.newest.lock() {
            *newest = Some(opening);
        }
    }

    fn ended(&self, opening: Opening) {
        self.set(opening);
        if let Some(told) = &self.0.told {
            told(opening);
        }
    }
}

impl<S> Layer<S> for Opens {
    type Service = Watched<S>;

    fn layer(&self, inner: S) -> Watched<S> {
        Watched {
            inner,
            opens: self.clone(),
        }
    }
}

/// The connector's service, with its openings counted and timed.
#[derive(Clone)]
pub(crate) struct Watched<S> {
    inner: S,
    opens: Opens,
}

impl<S, R> Service<R> for Watched<S>
where
    S: Service<R>,
    S::Future: Send + 'static,
    S::Response: 'static,
    S::Error: 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, request: R) -> Self::Future {
        self.opens.0.began.fetch_add(1, Ordering::SeqCst);
        self.opens.set(Opening::Going);
        let opens = self.opens.clone();
        let opening = self.inner.call(request);
        Box::pin(async move {
            let began = Instant::now();
            let opened = opening.await;
            opens.ended(match &opened {
                Ok(_) => Opening::Opened(began.elapsed()),
                Err(_) => Opening::Failed(began.elapsed()),
            });
            opened
        })
    }
}

/// Which connection a request went on.
enum Connection {
    Kept,
    New(Opens),
}

/// What a round's two tries tell once between them: both may meet the same
/// refused key or fall from the same near address, and each still goes where
/// that sends it.
#[derive(Default)]
struct ToldOnce {
    fall: AtomicBool,
    refusal: AtomicBool,
}

/// How far one try of a chat request got, in the journal's words.
#[derive(Default)]
pub struct Reach {
    connection: Mutex<Option<Connection>>,
    answer_begun: AtomicBool,
    once: Arc<ToldOnce>,
}

impl Reach {
    pub(super) fn went(&self, connection: Option<Opens>) {
        if let Ok(mut went) = self.connection.lock() {
            *went = Some(connection.map_or(Connection::Kept, Connection::New));
        }
    }

    pub(super) fn answer_began(&self) {
        self.answer_begun.store(true, Ordering::SeqCst);
    }

    /// Whether the response's head came: the connection is plainly alive,
    /// though no word or call has yet.
    pub fn answer_has_begun(&self) -> bool {
        self.answer_begun.load(Ordering::SeqCst)
    }

    /// `answer begun` (the response's head came, no word or call yet), `sent`
    /// (handed to an open connection, which says nothing of whether it got
    /// anywhere), or `nothing`.
    pub fn told(&self) -> String {
        if self.answer_has_begun() {
            return "answer begun".to_owned();
        }
        let Ok(connection) = self.connection.lock() else {
            return "nothing".to_owned();
        };
        match &*connection {
            None => "nothing",
            Some(Connection::Kept) => "sent, on a kept connection",
            Some(Connection::New(opens)) => match opens.newest() {
                Some(Opening::Opened(_)) => "sent, on a new connection",
                _ => "nothing, its new connection not open",
            },
        }
        .to_owned()
    }
}

/// What a chat request is watched with: where it says how far it got, and
/// with `opening_told` it goes on a new connection of its own, on a client
/// that keeps none.
#[derive(Clone, Default)]
pub struct Watch {
    reach: Arc<Reach>,
    opening_told: Option<OpeningTold>,
}

impl Watch {
    /// A try on a new connection of its own, `told` how its opening ended.
    pub fn on_a_new_connection(told: OpeningTold) -> Self {
        Self {
            reach: Arc::default(),
            opening_told: Some(told),
        }
    }

    /// [`Self::on_a_new_connection`] for the second try of a round whose first
    /// reached `first`: between them they tell a fall and a refusal once.
    pub fn second_to(first: &Reach, told: OpeningTold) -> Self {
        Self {
            reach: Arc::new(Reach {
                once: first.once.clone(),
                ..Reach::default()
            }),
            ..Self::on_a_new_connection(told)
        }
    }

    /// Whether this try is the round's first to fall from a near address.
    pub fn first_to_tell_a_fall(&self) -> bool {
        !self.reach.once.fall.swap(true, Ordering::SeqCst)
    }

    /// Whether this try is the round's first to have its key refused.
    pub fn first_to_tell_a_refusal(&self) -> bool {
        !self.reach.once.refusal.swap(true, Ordering::SeqCst)
    }

    pub fn reach(&self) -> Arc<Reach> {
        self.reach.clone()
    }

    /// The watcher of a new client for this try, when it is one on a new
    /// connection.
    pub(super) fn own_opens(&self) -> Option<Opens> {
        self.opening_told.clone().map(Opens::telling)
    }
}
