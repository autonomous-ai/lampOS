//! A spike for a skill's own code (#6071, [docs/skill-code.md]): a short
//! Rhai script run in a sandbox with an operation budget (its fuel), a wall
//! time, and caps on strings, arrays, maps, variables and call depth.
//!
//! The script never acts. Each host function it calls records one [`Call`]
//! in a [`Plan`], and a run that breaks a limit gives no plan at all, so
//! nothing of it is half made. Making a plan is `lamp-skills`' (not wired
//! here): each call held to the skill's permissions, as a step's is.
//!
//! Two host functions so far, `light(colour)` and `say(text)`. Before a
//! script runs, [`check`] names every function it calls; one that is
//! neither a host function nor the script's own is refused as something
//! the lamp can't do yet ([`Refused::CantYet`]), never run.
//!
//! [docs/skill-code.md]: ../../docs/skill-code.md
use rhai::{ASTNode, Dynamic, Engine, EvalAltResult, Expr, Map, Scope, Stmt};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    rc::Rc,
    time::{Duration, Instant},
};

/// The host functions a script may call, each recording one [`Call`].
pub const HOST: [&str; 2] = ["light", "say"];

/// Words a script may not use: no `eval` of a string, no function pointer
/// or call by a computed name (so [`check`] sees every call), no printing.
const DISABLED: [&str; 8] = [
    "eval",
    "Fn",
    "call",
    "curry",
    "print",
    "debug",
    "is_def_fn",
    "is_def_var",
];

/// What one run may use. [`Limits::default`] is the design's
/// (docs/skill-code.md, "Limits, and the kill").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Rhai operations a run, its fuel.
    pub operations: u64,
    /// The longest a run may take, by the wall clock.
    pub wall: Duration,
    /// The longest string, in bytes.
    pub string_bytes: usize,
    /// The most items in an array.
    pub array_items: usize,
    /// The most entries in a map.
    pub map_entries: usize,
    /// The most variables in scope at once.
    pub variables: usize,
    /// The deepest call of a script's own functions.
    pub call_levels: usize,
    /// The deepest expression, at the top and in a function.
    pub expression_depth: (usize, usize),
    /// The most bytes of a script's source.
    pub source_bytes: usize,
    /// The most calls in a plan.
    pub calls: usize,
    /// The most says in a plan: the say service takes one at a time.
    pub says: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            operations: 100_000,
            wall: Duration::from_millis(50),
            string_bytes: 1024,
            array_items: 64,
            map_entries: 64,
            variables: 64,
            call_levels: 16,
            expression_depth: (32, 16),
            source_bytes: 8 * 1024,
            calls: 16,
            says: 1,
        }
    }
}

/// One thing a run asks the lamp to do: what a `[[do]]` step makes
/// (`lamp_skills::Call`), as the spike's two host functions record it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    /// `light(colour)`: the light to this colour.
    Light(String),
    /// `say(text)`: these words said as they are written.
    Say(String),
}

/// What a run gives back: the calls to make, in order, and what it used.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub calls: Vec<Call>,
    /// Rhai operations the run took.
    pub operations: u64,
}

/// Why a script made no plan. [`Refused::plainly`] is the line the person
/// hears.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Its source is over [`Limits::source_bytes`].
    TooBig { bytes: usize },
    /// It isn't a script the lamp reads: the parser's words, with the line.
    Unreadable(String),
    /// It calls a function the lamp doesn't have: can't yet, never "not
    /// allowed" (#6066's fourth stay).
    CantYet(String),
    /// It ran out of operations.
    OutOfFuel,
    /// It ran past its wall time.
    OutOfTime,
    /// It made a string, an array, a map, a variable or a call deeper or
    /// bigger than its limits.
    TooMuchMemory(String),
    /// Its plan has more calls, or says, than a run may make.
    TooManyCalls,
    /// A host function was given what its step would refuse.
    BadInput(String),
    /// Anything else the run stopped on.
    Failed(String),
}

impl Refused {
    /// The person's line, after "I couldn't make that: ".
    pub fn plainly(&self) -> String {
        match self {
            Self::TooBig { .. } => "its code is too long".into(),
            Self::Unreadable(_) => "its code couldn't be read".into(),
            Self::CantYet(name) => format!("the lamp can't do {name} yet"),
            Self::OutOfFuel | Self::OutOfTime => "its code didn't finish in time".into(),
            Self::TooMuchMemory(_) => "its code needed too much memory".into(),
            Self::TooManyCalls => "its code asked for too many things at once".into(),
            Self::BadInput(why) => format!("its code asked for {why}"),
            Self::Failed(_) => "its code stopped with an error".into(),
        }
    }
}

/// What a check finds in a script before it runs: the host functions it
/// calls (what the say-back names and its permissions must give).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uses {
    pub host: BTreeSet<String>,
}

/// Parses `source` and names every function it calls. One that is neither
/// in [`HOST`] nor defined by the script is [`Refused::CantYet`].
pub fn check(source: &str, limits: &Limits) -> Result<Uses, Refused> {
    let engine = engine(limits, None);
    let ast = compile(&engine, source, limits)?;
    let own: BTreeSet<String> = ast.iter_functions().map(|f| f.name.to_string()).collect();
    let mut host = BTreeSet::new();
    let mut unknown = None;
    ast.walk(&mut |path: &[ASTNode]| {
        // A call is an expression, or a statement of its own (`say(..);`).
        let call = match path.last() {
            Some(ASTNode::Expr(Expr::FnCall(call, _) | Expr::MethodCall(call, _)))
            | Some(ASTNode::Stmt(Stmt::FnCall(call, _))) => call,
            _ => return true,
        };
        // An operator (`+`, `==`, `+=`) is the engine's own, not a call.
        if call.op_token.is_some() {
            return true;
        }
        let name = call.name.as_str();
        if HOST.contains(&name) {
            host.insert(name.to_string());
        } else if !own.contains(name) {
            unknown = Some(name.to_string());
            return false;
        }
        true
    });
    match unknown {
        Some(name) => Err(Refused::CantYet(name)),
        None => Ok(Uses { host }),
    }
}

/// Checks and runs `source` once with `event` as a read-only map, under
/// `limits`, and gives back its plan, or why it made none.
pub fn run(source: &str, event: &[(&str, Value)], limits: &Limits) -> Result<Plan, Refused> {
    check(source, limits)?;
    let plan = Rc::new(RefCell::new(Plan::default()));
    let engine = engine(limits, Some(&plan));
    let ast = compile(&engine, source, limits)?;
    let mut scope = Scope::new();
    let map: Map = event
        .iter()
        .map(|(key, value)| ((*key).into(), value.dynamic()))
        .collect();
    scope.push_constant("event", map);
    engine
        .run_ast_with_scope(&mut scope, &ast)
        .map_err(|error| refused(*error))?;
    drop(engine);
    let plan =
        Rc::try_unwrap(plan).map_or_else(|shared| shared.borrow().clone(), RefCell::into_inner);
    Ok(plan)
}

/// A value of the trigger's event a script reads.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Text(String),
}

impl Value {
    fn dynamic(&self) -> Dynamic {
        match self {
            Self::Bool(value) => (*value).into(),
            Self::Int(value) => (*value).into(),
            Self::Text(value) => value.clone().into(),
        }
    }
}

/// The engine for one check or run: no library at all, only the limits and,
/// for a run, the host functions recording into `plan`. Built anew each
/// time, so the wall clock is the run's own.
fn engine(limits: &Limits, plan: Option<&Rc<RefCell<Plan>>>) -> Engine {
    let mut engine = Engine::new_raw();
    for symbol in DISABLED {
        engine.disable_symbol(symbol);
    }
    engine
        .set_max_operations(limits.operations)
        .set_max_string_size(limits.string_bytes)
        .set_max_array_size(limits.array_items)
        .set_max_map_size(limits.map_entries)
        .set_max_variables(limits.variables)
        .set_max_call_levels(limits.call_levels)
        .set_max_expr_depths(limits.expression_depth.0, limits.expression_depth.1);
    let Some(plan) = plan else {
        return engine;
    };
    let began = Instant::now();
    let wall = limits.wall;
    let counted = Rc::clone(plan);
    engine.on_progress(move |operations| {
        counted.borrow_mut().operations = operations;
        (began.elapsed() > wall).then(|| Dynamic::from(TIME_UP))
    });
    let (calls, says) = (limits.calls, limits.says);
    let lights = Rc::clone(plan);
    engine.register_fn(
        "light",
        move |colour: &str| -> Result<(), Box<EvalAltResult>> {
            let colour = colour.trim();
            if colour.is_empty()
                || colour.len() > 24
                || !colour.chars().all(|c| c.is_ascii_lowercase() || c == ' ')
            {
                return Err(bad_input(format!("a light of {colour:?}")));
            }
            record(&lights, Call::Light(colour.into()), calls, says)
        },
    );
    let words = Rc::clone(plan);
    engine.register_fn("say", move |text: &str| -> Result<(), Box<EvalAltResult>> {
        let text = text.trim();
        if text.is_empty() {
            return Err(bad_input("an empty say".into()));
        }
        record(&words, Call::Say(text.into()), calls, says)
    });
    engine
}

/// What `on_progress` stops a run with when its wall time is up.
const TIME_UP: &str = "lamp: out of time";
/// How a host function's refusal of its input is told apart.
const BAD_INPUT: &str = "lamp: bad input: ";
/// How a plan over its calls is told apart.
const TOO_MANY: &str = "lamp: too many calls";

fn bad_input(why: String) -> Box<EvalAltResult> {
    format!("{BAD_INPUT}{why}").into()
}

fn record(
    plan: &Rc<RefCell<Plan>>,
    call: Call,
    most: usize,
    most_says: usize,
) -> Result<(), Box<EvalAltResult>> {
    let mut plan = plan.borrow_mut();
    let says = plan
        .calls
        .iter()
        .filter(|made| matches!(made, Call::Say(_)))
        .count();
    if plan.calls.len() >= most || (matches!(call, Call::Say(_)) && says >= most_says) {
        return Err(TOO_MANY.into());
    }
    plan.calls.push(call);
    Ok(())
}

fn compile(engine: &Engine, source: &str, limits: &Limits) -> Result<rhai::AST, Refused> {
    if source.len() > limits.source_bytes {
        return Err(Refused::TooBig {
            bytes: source.len(),
        });
    }
    engine
        .compile(source)
        .map_err(|error| Refused::Unreadable(error.to_string()))
}

fn refused(error: EvalAltResult) -> Refused {
    match error {
        EvalAltResult::ErrorTooManyOperations(_) => Refused::OutOfFuel,
        EvalAltResult::ErrorTerminated(..) => Refused::OutOfTime,
        EvalAltResult::ErrorDataTooLarge(what, _) => Refused::TooMuchMemory(what),
        EvalAltResult::ErrorTooManyVariables(_) => Refused::TooMuchMemory("variables".into()),
        EvalAltResult::ErrorStackOverflow(_) => Refused::TooMuchMemory("call depth".into()),
        EvalAltResult::ErrorFunctionNotFound(name, _) => Refused::CantYet(name),
        EvalAltResult::ErrorRuntime(value, _) => {
            let text = value.to_string();
            if let Some(why) = text.strip_prefix(BAD_INPUT) {
                Refused::BadInput(why.into())
            } else if text == TOO_MANY {
                Refused::TooManyCalls
            } else {
                Refused::Failed(text)
            }
        }
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => refused(*inner),
        other => Refused::Failed(other.to_string()),
    }
}
