//! On-device embeddings on the paraphrase set (#2510): the lamp half of
//! `training/memory-embed/bench.py`, which runs on a laptop only.
//!
//! A measure, built only with `--features embed-bench`; nothing in the product
//! links it. It reads the fixture, an ONNX sentence encoder and its
//! `tokenizer.json`, checks both against a sha256 list before loading them,
//! embeds every kept thing as `passage: ...` and every question as
//! `query: ...` (multilingual-e5's prefixes), one at a time as the lamp would,
//! tokenizing included, and ranks each question's own language by cosine.
//! Mean pooling over the attention mask and an L2 norm, exactly as bench.py
//! does. It prints one JSON line per thread count: counts and timings only,
//! never the fixture's text. Given several thread counts, it runs itself once
//! per count, so each line's peak RSS is that count's own process.
//!
//! Each set (`--fixture a.json,b.json`) is also ranked, on the same kept
//! things, by the store's full-text search (FTS5 with the store's own
//! `unicode61 remove_diacritics 2` tokenizer, [`search::text`] and
//! [`search::query_with`], both the first version's query and recall_v2's)
//! and by the voice's kept-list word match, mirrored from
//! `crates/voice/src/remember.rs` ([`content_words`], [`kept_words_rank`]),
//! so an embedder's recall is read against what the lamp has now. See
//! docs/memory-evaluation.md, "Embeddings on a lamp".

use lamp_memory::search;
use ort::{
    environment::Environment,
    ep,
    logging::LogLevel,
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};
use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::Instant,
};
use tokenizers::{Tokenizer, TruncationParams};

const DEPTHS: [usize; 3] = [1, 3, 5];
const LANGS: [&str; 2] = ["en", "vi"];
/// The ways each set is ranked, as they are reported.
const METHODS: [&str; 4] = ["embed", "fts", "fts_v2", "kept_words"];
const USAGE: &str = "usage: lamp-memory-embed-bench --fixture SET.json[,SET2.json] --model MODEL.onnx \
--tokenizer tokenizer.json --sha256 downloads.sha256 --runtime libonnxruntime \
[--runtime-sha256 HEX] [--threads 1,4] [--arena on|off] [--rounds 10] [--max-tokens 512] \
[--query-prefix 'query: '] [--passage-prefix 'passage: ']";

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
struct Fixture {
    kept: Vec<Kept>,
    questions: Vec<Question>,
}

#[derive(Deserialize)]
struct Kept {
    id: u64,
    lang: String,
    text: String,
}

#[derive(Deserialize)]
struct Question {
    lang: String,
    text: String,
    kept: u64,
}

struct Args {
    fixtures: Vec<PathBuf>,
    model: PathBuf,
    tokenizer: PathBuf,
    manifest: PathBuf,
    runtime: PathBuf,
    runtime_sha256: Option<String>,
    threads: Vec<usize>,
    /// ONNX Runtime's CPU memory arena and memory pattern, both on or both off.
    arena: bool,
    rounds: usize,
    max_tokens: usize,
    query_prefix: String,
    passage_prefix: String,
}

fn parse_args(mut given: impl Iterator<Item = String>) -> Result<Args> {
    let mut values: BTreeMap<String, String> = BTreeMap::new();
    while let Some(flag) = given.next() {
        if flag == "--help" || flag == "-h" {
            return Err(USAGE.into());
        }
        let known = [
            "--fixture",
            "--model",
            "--tokenizer",
            "--sha256",
            "--runtime",
            "--runtime-sha256",
            "--threads",
            "--arena",
            "--rounds",
            "--max-tokens",
            "--query-prefix",
            "--passage-prefix",
        ];
        if !known.contains(&flag.as_str()) {
            return Err(format!("unknown argument {flag:?}\n{USAGE}"));
        }
        let value = given
            .next()
            .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))?;
        values.insert(flag, value);
    }
    let path = |flag: &str| {
        values
            .get(flag)
            .map(PathBuf::from)
            .ok_or_else(|| format!("{flag} is required\n{USAGE}"))
    };
    let number = |flag: &str, default: usize| -> Result<usize> {
        match values.get(flag) {
            None => Ok(default),
            Some(value) => value
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("{flag} takes a positive number")),
        }
    };
    let threads = values
        .get("--threads")
        .map_or("1,4", String::as_str)
        .split(',')
        .map(|part| part.trim().parse::<usize>().ok().filter(|n| *n > 0))
        .collect::<Option<Vec<_>>>()
        .filter(|list| !list.is_empty())
        .ok_or("--threads takes positive numbers, comma-separated")?;
    let arena = match values.get("--arena").map(String::as_str) {
        None | Some("on") => true,
        Some("off") => false,
        Some(_) => return Err("--arena takes on or off".into()),
    };
    let fixtures = path("--fixture")?
        .to_string_lossy()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if fixtures.is_empty() {
        return Err(format!("--fixture names no file\n{USAGE}"));
    }
    Ok(Args {
        fixtures,
        model: path("--model")?,
        tokenizer: path("--tokenizer")?,
        manifest: path("--sha256")?,
        runtime: path("--runtime")?,
        runtime_sha256: values
            .get("--runtime-sha256")
            .map(|hex| hex.trim().to_ascii_lowercase()),
        threads,
        arena,
        rounds: number("--rounds", 10)?,
        max_tokens: number("--max-tokens", 512)?,
        query_prefix: values
            .get("--query-prefix")
            .cloned()
            .unwrap_or_else(|| "query: ".into()),
        passage_prefix: values
            .get("--passage-prefix")
            .cloned()
            .unwrap_or_else(|| "passage: ".into()),
    })
}

/// A sha256 list as `shasum -c` reads it: `{relative name: digest}`.
fn read_manifest(text: &str) -> Result<BTreeMap<String, String>> {
    let mut listed = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (digest, name) = line
            .split_once(char::is_whitespace)
            .ok_or_else(|| format!("unreadable sha256 line {line:?}"))?;
        listed.insert(
            name.trim().trim_start_matches('*').to_string(),
            digest.to_ascii_lowercase(),
        );
    }
    Ok(listed)
}

/// The listed name a path is, as bench.py matches it: the whole path or its
/// ending at a folder boundary, the longest listed name winning.
fn listed_name<'a>(path: &str, listed: &'a BTreeMap<String, String>) -> Option<&'a str> {
    let given = path.trim_start_matches("./");
    listed
        .keys()
        .filter(|name| given == name.as_str() || given.ends_with(&format!("/{name}")))
        .max_by_key(|name| name.len())
        .map(String::as_str)
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut block = vec![0u8; 1 << 20];
    loop {
        let read = file
            .read(&mut block)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&block[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Refuses a file the list does not name or whose bytes differ.
fn verify(path: &Path, listed: &BTreeMap<String, String>) -> Result<()> {
    let shown = path.to_string_lossy();
    let name = listed_name(&shown, listed)
        .ok_or_else(|| format!("refusing {shown}: not in the sha256 list"))?;
    if sha256_file(path)? != listed[name] {
        return Err(format!(
            "refusing {shown}: its sha256 does not match the list's {name}"
        ));
    }
    Ok(())
}

/// Mean of the token vectors the mask keeps, then L2-normalised: bench.py's
/// `(hidden * mask).sum(0) / mask.sum()` and `/ norm`.
fn mean_pool_normalise(hidden: &[f32], dims: usize, mask: &[i64]) -> Result<Vec<f32>> {
    if dims == 0 || hidden.len() != dims * mask.len() {
        return Err("the encoder's output does not match its tokens".into());
    }
    let mut vector = vec![0f32; dims];
    let mut weight = 0f32;
    for (row, keep) in hidden.chunks(dims).zip(mask) {
        let keep = *keep as f32;
        weight += keep;
        for (sum, value) in vector.iter_mut().zip(row) {
            *sum += value * keep;
        }
    }
    if weight == 0.0 {
        return Err("no token to pool".into());
    }
    vector.iter_mut().for_each(|value| *value /= weight);
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm == 0.0 || !norm.is_finite() {
        return Err("a zero or non-finite embedding".into());
    }
    vector.iter_mut().for_each(|value| *value /= norm);
    Ok(vector)
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The pool's ids by cosine, highest first; ties keep the pool's order, as
/// Python's stable `sorted` does.
fn rank(query: &[f32], pool: &[(u64, &[f32])]) -> Vec<u64> {
    let mut scored: Vec<(f32, u64)> = pool.iter().map(|(id, v)| (dot(query, v), *id)).collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|(_, id)| id).collect()
}

/// `crates/voice/src/remember.rs`'s `SMALL_WORDS`, copied as it is: lamp-memory
/// cannot depend on lamp-voice, which depends on it. Keep the two the same.
const SMALL_WORDS: &[&str] = &[
    "a", "about", "all", "also", "am", "an", "and", "any", "are", "around", "at", "back", "be",
    "bit", "but", "can", "could", "did", "do", "does", "down", "for", "from", "get", "give", "go",
    "going", "got", "had", "has", "have", "he", "hello", "her", "here", "hers", "hey", "hi", "him",
    "his", "how", "i", "i'm", "in", "into", "is", "it", "it's", "its", "just", "know", "lamp",
    "let", "like", "little", "make", "me", "mine", "more", "my", "need", "no", "not", "now", "of",
    "off", "ok", "okay", "on", "one", "or", "our", "out", "please", "really", "remember", "right",
    "said", "say", "see", "she", "so", "some", "tell", "than", "thank", "thanks", "that", "the",
    "their", "them", "then", "there", "these", "they", "think", "this", "those", "to", "today",
    "too", "up", "us", "very", "want", "wants", "was", "way", "we", "well", "were", "what",
    "what's", "whats", "when", "where", "which", "who", "why", "will", "with", "would", "yes",
    "you", "your", "yours",
];

/// `remember.rs`'s `content_words`, copied as it is: lower-cased, of three
/// letters or more and not [`SMALL_WORDS`], a possessive's `'s` and a
/// plural's `s` taken off.
fn content_words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = text
        .to_lowercase()
        .replace('\u{2019}', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|word| word.trim_matches('\''))
        .map(|word| word.strip_suffix("'s").unwrap_or(word))
        .filter(|word| word.chars().count() >= 3 && !SMALL_WORDS.contains(word))
        .map(|word| {
            word.strip_suffix('s')
                .filter(|stem| stem.chars().count() >= 3)
                .unwrap_or(word)
                .to_owned()
        })
        .collect();
    words.sort();
    words.dedup();
    words
}

/// `remember.rs`'s `hits_told` order: the kept things sharing a content word
/// with the question, most shared first, then the newest (a higher id is
/// newer, as `at_unix_ms` is there). The live turn is told the first three
/// (`HITS`); the whole order is kept here so @5 can be read too.
fn kept_words_rank(question: &str, pool: &[(u64, &str)]) -> Vec<u64> {
    let wanted = content_words(question);
    let mut found: Vec<(usize, u64)> = pool
        .iter()
        .filter_map(|(id, text)| {
            let shared = content_words(text.trim())
                .iter()
                .filter(|word| wanted.contains(word))
                .count();
            (shared > 0).then_some((shared, *id))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    found.into_iter().map(|(_, id)| id).collect()
}

/// One language's kept things in an in-memory FTS5 table made as the
/// store's (`days.sql`, `search.rs`): its tokenizer, bodies in
/// [`search::text`]'s form, ranked by bm25 then the newest, as `days.rs`'s
/// `find` orders them.
struct FtsIndex {
    connection: Connection,
}

impl FtsIndex {
    fn new(pool: &[(u64, &str)]) -> Result<Self> {
        let fail = |error: rusqlite::Error| format!("FTS5: {error}");
        let connection = Connection::open_in_memory().map_err(fail)?;
        connection
            .execute_batch(
                "CREATE VIRTUAL TABLE search USING fts5(
                     body, tokenize='unicode61 remove_diacritics 2'
                 );",
            )
            .map_err(fail)?;
        for (id, text) in pool {
            let rowid = i64::try_from(*id).map_err(|_| "an id past i64".to_string())?;
            connection
                .execute(
                    "INSERT INTO search(rowid, body) VALUES (?1, ?2)",
                    params![rowid, search::text(text)],
                )
                .map_err(fail)?;
        }
        Ok(Self { connection })
    }

    /// The store's query for `question` ([`search::query_with`]; `second`
    /// is recall_v2's, common words left out), best first; nothing when it
    /// makes no query.
    fn rank(&self, question: &str, second: bool) -> Result<Vec<u64>> {
        let Some(query) = search::query_with(question, second) else {
            return Ok(Vec::new());
        };
        let fail = |error: rusqlite::Error| format!("FTS5: {error}");
        let mut statement = self
            .connection
            .prepare_cached(
                "SELECT rowid FROM search WHERE search MATCH ?1 ORDER BY rank, rowid DESC",
            )
            .map_err(fail)?;
        let ids = statement
            .query_map(params![query], |row| row.get::<_, i64>(0))
            .map_err(fail)?
            .collect::<rusqlite::Result<Vec<i64>>>()
            .map_err(fail)?;
        Ok(ids
            .into_iter()
            .filter_map(|id| u64::try_from(id).ok())
            .collect())
    }
}

/// The word methods' rankings of one set: `{method: [(lang, wanted, ranked)]}`,
/// each question ranked within its own language, as the embedder is.
fn word_rankings(fixture: &Fixture) -> Result<BTreeMap<&'static str, Vec<Asked<'_>>>> {
    let mut ranked: BTreeMap<&'static str, Vec<Asked<'_>>> = BTreeMap::new();
    for lang in LANGS {
        let pool: Vec<(u64, &str)> = fixture
            .kept
            .iter()
            .filter(|entry| entry.lang == lang)
            .map(|entry| (entry.id, entry.text.as_str()))
            .collect();
        let index = FtsIndex::new(&pool)?;
        for question in fixture.questions.iter().filter(|q| q.lang == lang) {
            let asked = |order: Vec<u64>| (question.lang.as_str(), question.kept, order);
            ranked
                .entry("fts")
                .or_default()
                .push(asked(index.rank(&question.text, false)?));
            ranked
                .entry("fts_v2")
                .or_default()
                .push(asked(index.rank(&question.text, true)?));
            ranked
                .entry("kept_words")
                .or_default()
                .push(asked(kept_words_rank(&question.text, &pool)));
        }
    }
    Ok(ranked)
}

/// A question's language, the kept thing it asks for and a method's order.
type Asked<'a> = (&'a str, u64, Vec<u64>);

/// Recall@1/@3/@5 per language: `{"@1": {"en": .., "vi": ..}, ...}`.
fn recall_table(asked: &[Asked<'_>]) -> serde_json::Value {
    DEPTHS
        .iter()
        .map(|depth| (format!("@{depth}"), json!(recall(asked, *depth))))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// Recall@k per language in percent, one decimal, as bench.py rounds it.
fn recall(asked: &[Asked<'_>], depth: usize) -> BTreeMap<String, f64> {
    let mut found: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (lang, wanted, ranked) in asked {
        let entry = found.entry((*lang).to_string()).or_default();
        entry.0 += usize::from(ranked.iter().take(depth).any(|id| id == wanted));
        entry.1 += 1;
    }
    found
        .into_iter()
        .map(|(lang, (hit, total))| {
            let share = 100.0 * hit as f64 / total as f64;
            (lang, one_decimal(share))
        })
        .collect()
}

/// bench.py's percentile: the sorted value at `round(share * (n - 1))`,
/// Python's round (half to even).
fn percentile(values: &[f64], share: f64) -> f64 {
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let at = (share * (ordered.len() - 1) as f64).round_ties_even() as usize;
    ordered[at.min(ordered.len() - 1)]
}

/// One decimal, half to even, as Python's `round(value, 1)`: 13 of 16 is
/// 81.2, as bench.py prints it.
fn one_decimal(value: f64) -> f64 {
    (value * 10.0).round_ties_even() / 10.0
}

/// The process's peak resident memory in MiB: `VmHWM` on Linux, else
/// `getrusage`'s `ru_maxrss` (bytes on macOS, KiB elsewhere).
fn peak_rss_mb() -> Option<f64> {
    if let Ok(status) = fs::read_to_string("/proc/self/status")
        && let Some(kib) = status
            .lines()
            .find_map(|line| line.strip_prefix("VmHWM:"))
            .and_then(|rest| {
                rest.trim()
                    .trim_end_matches("kB")
                    .trim()
                    .parse::<f64>()
                    .ok()
            })
    {
        return Some(kib / 1024.0);
    }
    let usage = nix::sys::resource::getrusage(nix::sys::resource::UsageWho::RUSAGE_SELF).ok()?;
    let scale = if cfg!(target_os = "macos") {
        1.0
    } else {
        1024.0
    };
    Some(usage.max_rss() as f64 * scale / f64::from(1 << 20))
}

struct Encoder {
    session: Session,
    tokenizer: Tokenizer,
    token_types: bool,
    tokens: usize,
}

impl Encoder {
    fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|error| format!("tokenizer: {error}"))?;
        let ids: Vec<i64> = encoding.get_ids().iter().map(|&id| i64::from(id)).collect();
        let mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| i64::from(m))
            .collect();
        let count = ids.len();
        self.tokens += count;
        let shape = [1usize, count];
        let tensor = |values: Vec<i64>| {
            Tensor::from_array((shape, values)).map_err(|error| format!("input: {error}"))
        };
        let mut inputs = vec![
            ("input_ids", tensor(ids)?.into_dyn()),
            ("attention_mask", tensor(mask.clone())?.into_dyn()),
        ];
        if self.token_types {
            inputs.push(("token_type_ids", tensor(vec![0; count])?.into_dyn()));
        }
        let outputs = self
            .session
            .run(inputs)
            .map_err(|error| format!("ONNX Runtime run: {error}"))?;
        let (shape, hidden) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|error| format!("output: {error}"))?;
        let dims = shape
            .last()
            .and_then(|&d| usize::try_from(d).ok())
            .unwrap_or(0);
        mean_pool_normalise(hidden, dims, &mask)
    }
}

/// With `arena` off, the CPU execution provider's memory arena and the
/// session's memory pattern are both turned off, so each run's buffers are
/// freed after it instead of kept for the next.
fn open_session(
    environment: &Environment,
    model: &Path,
    threads: usize,
    arena: bool,
) -> Result<Session> {
    Session::builder(environment)
        .and_then(|builder| Ok(builder.with_log_level(LogLevel::Error)?))
        .and_then(|builder| Ok(builder.with_optimization_level(GraphOptimizationLevel::All)?))
        .and_then(|builder| Ok(builder.with_intra_threads(threads)?))
        .and_then(|builder| Ok(builder.with_inter_threads(1)?))
        .and_then(|builder| {
            Ok(builder.with_execution_providers([ep::CPU::default()
                .with_arena_allocator(arena)
                .build()])?)
        })
        .and_then(|builder| Ok(builder.with_memory_pattern(arena)?))
        .and_then(|mut builder| builder.commit_from_file(model))
        .map_err(|error| format!("ONNX Runtime session: {error}"))
}

/// A set read from its file: its name (the file's stem) and its things.
struct Set {
    name: String,
    fixture: Fixture,
}

fn read_set(path: &Path) -> Result<Set> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let fixture: Fixture = serde_json::from_str(&text)
        .map_err(|error| format!("fixture {}: {error}", path.display()))?;
    let name = path.file_stem().map_or_else(
        || path.display().to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    Ok(Set { name, fixture })
}

fn run(args: &Args) -> Result<()> {
    let manifest = fs::read_to_string(&args.manifest)
        .map_err(|error| format!("{}: {error}", args.manifest.display()))?;
    let listed = read_manifest(&manifest)?;
    verify(&args.model, &listed)?;
    verify(&args.tokenizer, &listed)?;
    let sets = args
        .fixtures
        .iter()
        .map(|path| read_set(path))
        .collect::<Result<Vec<_>>>()?;
    let rss_start = peak_rss_mb();

    // Checked before the library is loaded, as the model and tokenizer are.
    let runtime_sha256 = sha256_file(&args.runtime)?;
    if let Some(wanted) = &args.runtime_sha256
        && *wanted != runtime_sha256
    {
        return Err(format!(
            "refusing {}: its sha256 {runtime_sha256} is not the given {wanted}",
            args.runtime.display()
        ));
    }
    let environment = ort::init_from(&args.runtime)
        .map_err(|error| format!("ONNX Runtime {}: {error}", args.runtime.display()))?
        .with_name("lamp-memory-embed-bench")
        .with_telemetry(false)
        .build()
        .map_err(|error| error.to_string())?;
    environment.set_log_level(LogLevel::Error);
    let rss_runtime = peak_rss_mb();

    for &threads in &args.threads {
        // Cold: from the tokenizer's and session's making to the first
        // embedding out (the model's bytes are in the page cache already,
        // read for their sha256).
        let started = Instant::now();
        let mut tokenizer =
            Tokenizer::from_file(&args.tokenizer).map_err(|error| format!("tokenizer: {error}"))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: args.max_tokens,
                ..TruncationParams::default()
            }))
            .map_err(|error| format!("tokenizer: {error}"))?;
        tokenizer.with_padding(None);
        let rss_tokenizer = peak_rss_mb();
        let session = open_session(&environment, &args.model, threads, args.arena)?;
        let token_types = session
            .inputs()
            .iter()
            .any(|input| input.name() == "token_type_ids");
        let mut encoder = Encoder {
            session,
            tokenizer,
            token_types,
            tokens: 0,
        };
        let load_ms = started.elapsed().as_secs_f64() * 1000.0;
        let rss_session = peak_rss_mb();
        let first = sets
            .iter()
            .flat_map(|set| set.fixture.questions.first())
            .next()
            .ok_or("no question in any set")?;
        encoder.embed(&format!("{}{}", args.query_prefix, first.text))?;
        let cold_ms = started.elapsed().as_secs_f64() * 1000.0;
        let rss_first = peak_rss_mb();

        let mut reported = serde_json::Map::new();
        let mut dims = 0;
        for set in &sets {
            let fixture = &set.fixture;
            let mut passages: BTreeMap<u64, Vec<f32>> = BTreeMap::new();
            for entry in &fixture.kept {
                let vector = encoder.embed(&format!("{}{}", args.passage_prefix, entry.text))?;
                passages.insert(entry.id, vector);
            }
            let mut asked = Vec::new();
            for question in &fixture.questions {
                let vector = encoder.embed(&format!("{}{}", args.query_prefix, question.text))?;
                let pool: Vec<(u64, &[f32])> = fixture
                    .kept
                    .iter()
                    .filter(|entry| entry.lang == question.lang)
                    .map(|entry| (entry.id, passages[&entry.id].as_slice()))
                    .collect();
                asked.push((question.lang.as_str(), question.kept, rank(&vector, &pool)));
            }
            dims = passages.values().next().map_or(0, Vec::len);
            let mut by_method = word_rankings(fixture)?;
            by_method.insert("embed", asked);
            let per_language = LANGS
                .iter()
                .map(|lang| {
                    let kept = fixture.kept.iter().filter(|e| e.lang == *lang).count();
                    let questions = fixture.questions.iter().filter(|q| q.lang == *lang).count();
                    (
                        (*lang).to_string(),
                        json!({"kept": kept, "questions": questions}),
                    )
                })
                .collect::<serde_json::Map<_, _>>();
            let recall = METHODS
                .iter()
                .map(|method| {
                    (
                        (*method).to_string(),
                        recall_table(by_method.get(method).map_or(&[][..], Vec::as_slice)),
                    )
                })
                .collect::<serde_json::Map<_, _>>();
            reported.insert(
                set.name.clone(),
                json!({
                    "kept": fixture.kept.len(),
                    "questions": fixture.questions.len(),
                    "per_language": per_language,
                    "recall": recall,
                }),
            );
        }

        let mut query_ms = Vec::new();
        let mut passage_ms = Vec::new();
        encoder.tokens = 0;
        for _ in 0..args.rounds {
            for set in &sets {
                for question in &set.fixture.questions {
                    let text = format!("{}{}", args.query_prefix, question.text);
                    let started = Instant::now();
                    encoder.embed(&text)?;
                    query_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                }
                for entry in &set.fixture.kept {
                    let text = format!("{}{}", args.passage_prefix, entry.text);
                    let started = Instant::now();
                    encoder.embed(&text)?;
                    passage_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                }
            }
        }
        let embeds = query_ms.len() + passage_ms.len();
        let stats = |values: &[f64], shares: &[(&str, f64)]| {
            shares
                .iter()
                .map(|(name, share)| {
                    (
                        (*name).to_string(),
                        json!(one_decimal(percentile(values, *share))),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
        };
        let mb = |value: Option<f64>| value.map(f64::round);
        let report = json!({
            "name": "lamp-memory-embed-bench",
            "threads": threads,
            "arena": if args.arena { "on" } else { "off" },
            "dims": dims,
            "rounds": args.rounds,
            "sets": reported,
            "query_embeds": query_ms.len(),
            "passage_embeds": passage_ms.len(),
            "mean_tokens": one_decimal(encoder.tokens as f64 / embeds.max(1) as f64),
            "load_ms": load_ms.round(),
            "cold_ms": cold_ms.round(),
            "query_ms": stats(&query_ms, &[("p50", 0.5), ("p90", 0.9), ("p95", 0.95)]),
            "passage_ms": stats(&passage_ms, &[("p50", 0.5), ("p90", 0.9)]),
            "mean_query_ms": one_decimal(query_ms.iter().sum::<f64>() / query_ms.len().max(1) as f64),
            "rss_start_mb": mb(rss_start),
            "rss_peak_after_mb": {
                "runtime": mb(rss_runtime),
                "tokenizer": mb(rss_tokenizer),
                "session": mb(rss_session),
                "first_embed": mb(rss_first),
            },
            "rss_peak_mb": mb(peak_rss_mb()),
            "model_sha256": sha256_file(&args.model)?,
            "runtime_sha256": runtime_sha256,
            "os": env::consts::OS,
            "arch": env::consts::ARCH,
        });
        println!("{report}");
    }
    Ok(())
}

/// One argument list per thread count: the given arguments without their
/// `--threads`, then `--threads N`. A process's peak RSS never resets, so each
/// count gets a process of its own.
fn per_thread_invocations(given: &[String], threads: &[usize]) -> Vec<Vec<String>> {
    let mut rest = Vec::new();
    let mut at = given.iter();
    while let Some(flag) = at.next() {
        if flag == "--threads" {
            at.next();
        } else {
            rest.push(flag.clone());
        }
    }
    threads
        .iter()
        .map(|count| {
            let mut one = rest.clone();
            one.extend(["--threads".to_string(), count.to_string()]);
            one
        })
        .collect()
}

/// Runs this bench once per thread count and prints each child's JSON line.
fn run_each_in_own_process(given: &[String], threads: &[usize]) -> Result<()> {
    let bench = env::current_exe().map_err(|error| format!("this bench's path: {error}"))?;
    for invocation in per_thread_invocations(given, threads) {
        let output = Command::new(&bench)
            .args(&invocation)
            .stderr(std::process::Stdio::inherit())
            .output()
            .map_err(|error| format!("{}: {error}", bench.display()))?;
        if !output.status.success() {
            return Err(format!(
                "the {} run failed ({})",
                invocation.join(" "),
                output.status
            ));
        }
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }
    Ok(())
}

fn main() -> ExitCode {
    let given: Vec<String> = env::args().skip(1).collect();
    let outcome = parse_args(given.iter().cloned()).and_then(|args| {
        if args.threads.len() > 1 {
            run_each_in_own_process(&given, &args.threads)
        } else {
            run(&args)
        }
    });
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-memory-embed-bench: {error}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pooling_keeps_only_masked_tokens_and_normalises() {
        // Two kept tokens [3, 0] and [1, 4] average to [2, 2]; the masked
        // third token is ignored.
        let hidden = [3.0, 0.0, 1.0, 4.0, 100.0, 100.0];
        let vector = mean_pool_normalise(&hidden, 2, &[1, 1, 0]).unwrap();
        let half = 1.0 / 2f32.sqrt();
        assert!((vector[0] - half).abs() < 1e-6 && (vector[1] - half).abs() < 1e-6);
        assert!((dot(&vector, &vector) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn pooling_refuses_a_mismatch_or_nothing_to_pool() {
        assert!(mean_pool_normalise(&[1.0, 2.0, 3.0], 2, &[1, 1]).is_err());
        assert!(mean_pool_normalise(&[1.0, 2.0], 2, &[0]).is_err());
        assert!(mean_pool_normalise(&[0.0, 0.0], 2, &[1]).is_err());
    }

    #[test]
    fn ranking_is_by_cosine_and_ties_keep_the_pool_order() {
        let a = [1.0, 0.0];
        let b = [0.0, 1.0];
        let c = [0.6, 0.8];
        let pool: Vec<(u64, &[f32])> = vec![(1, &b), (2, &c), (3, &a), (4, &a)];
        assert_eq!(rank(&[1.0, 0.0], &pool), vec![3, 4, 2, 1]);
    }

    #[test]
    fn recall_counts_hits_within_each_depth_per_language() {
        let asked = vec![
            ("en", 1, vec![1, 2, 3, 4, 5]),
            ("en", 2, vec![3, 4, 2, 1, 5]),
            ("en", 9, vec![1, 2, 3, 4, 5]),
            ("vi", 7, vec![8, 9, 10, 11, 7]),
        ];
        assert_eq!(recall(&asked, 1)["en"], 33.3);
        assert_eq!(recall(&asked, 3)["en"], 66.7);
        assert_eq!(recall(&asked, 3)["vi"], 0.0);
        assert_eq!(recall(&asked, 5)["vi"], 100.0);
    }

    #[test]
    fn one_decimal_rounds_half_to_even_as_python() {
        assert_eq!(one_decimal(100.0 * 13.0 / 16.0), 81.2);
        assert_eq!(one_decimal(100.0 * 15.0 / 16.0), 93.8);
        assert_eq!(one_decimal(100.0 * 9.0 / 16.0), 56.2);
    }

    #[test]
    fn percentile_matches_bench_py_rounding() {
        let values: Vec<f64> = (1..=4).map(f64::from).collect();
        // round(0.5 * 3) = round(1.5) = 2 in Python: the third value.
        assert_eq!(percentile(&values, 0.5), 3.0);
        let values: Vec<f64> = (1..=6).map(f64::from).collect();
        // round(0.5 * 5) = round(2.5) = 2: the third value, half to even.
        assert_eq!(percentile(&values, 0.5), 3.0);
        assert_eq!(percentile(&values, 0.95), 6.0);
    }

    #[test]
    fn manifest_names_match_whole_folder_endings_only() {
        let listed = read_manifest(
            "# comment\nAB  e5-int8/onnx/model_quantized.onnx\ncd *onnx/tokenizer.json\n",
        )
        .unwrap();
        assert_eq!(listed["e5-int8/onnx/model_quantized.onnx"], "ab");
        assert_eq!(
            listed_name("/scratch/e5-int8/onnx/model_quantized.onnx", &listed),
            Some("e5-int8/onnx/model_quantized.onnx")
        );
        assert_eq!(
            listed_name("x/onnx/tokenizer.json", &listed),
            Some("onnx/tokenizer.json")
        );
        assert_eq!(listed_name("myonnx/tokenizer.json", &listed), None);
        assert_eq!(listed_name("model_quantized.onnx", &listed), None);
    }

    #[test]
    fn arguments_default_to_one_and_four_threads_and_e5_prefixes() {
        let args = parse_args(
            [
                "--fixture",
                "f",
                "--model",
                "m",
                "--tokenizer",
                "t",
                "--sha256",
                "s",
                "--runtime",
                "r",
            ]
            .map(String::from)
            .into_iter(),
        )
        .unwrap();
        assert_eq!(args.threads, vec![1, 4]);
        assert_eq!(
            (args.query_prefix.as_str(), args.passage_prefix.as_str()),
            ("query: ", "passage: ")
        );
        assert_eq!(args.runtime_sha256, None);
        assert!(parse_args(["--threads", "0"].map(String::from).into_iter()).is_err());
        assert!(parse_args(["--model"].map(String::from).into_iter()).is_err());
    }

    #[test]
    fn arena_defaults_on_and_fixtures_split_on_commas() {
        let base = [
            "--model",
            "m",
            "--tokenizer",
            "t",
            "--sha256",
            "s",
            "--runtime",
            "r",
        ];
        let parse = |extra: &[&str]| {
            parse_args(
                base.iter()
                    .chain(extra)
                    .map(|part| (*part).to_string())
                    .collect::<Vec<_>>()
                    .into_iter(),
            )
        };
        let args = parse(&["--fixture", "a.json, b.json"]).unwrap();
        assert!(args.arena);
        assert_eq!(
            args.fixtures,
            vec![PathBuf::from("a.json"), PathBuf::from("b.json")]
        );
        assert!(!parse(&["--fixture", "a", "--arena", "off"]).unwrap().arena);
        assert!(parse(&["--fixture", "a", "--arena", "no"]).is_err());
        assert!(parse(&["--fixture", ","]).is_err());
    }

    /// The copy is checked against the voice's own list, so the two cannot
    /// drift apart unseen.
    #[test]
    fn small_words_are_the_voices_own() {
        let voice = include_str!("../../../voice/src/remember.rs");
        let start = voice
            .find("const SMALL_WORDS: &[&str] = &[")
            .expect("remember.rs has SMALL_WORDS");
        let body = &voice[start..];
        let body = &body[body.find("= &[").unwrap() + 4..body.find("];").unwrap()];
        let theirs: Vec<&str> = body
            .split(',')
            .map(|word| word.trim().trim_matches('"'))
            .filter(|word| !word.is_empty())
            .collect();
        assert_eq!(theirs, SMALL_WORDS);
        assert!(voice.contains("fn content_words(text: &str) -> Vec<String> {"));
    }

    #[test]
    fn content_words_drop_small_words_possessives_and_plurals() {
        assert_eq!(
            content_words("What's my sister\u{2019}s favourite colours?"),
            vec!["colour", "favourite", "sister"]
        );
        // Three letters or more: most Vietnamese syllables of two are dropped.
        assert_eq!(content_words("Tôi đỗ xe ở đâu?"), vec!["tôi", "đâu"]);
    }

    #[test]
    fn kept_words_rank_by_shared_words_then_the_newest() {
        let pool = [
            (1, "My favourite colour is green."),
            (2, "The colour of my car is silver."),
            (3, "I left the car in the garage."),
            (4, "Nothing alike here."),
        ];
        // "colour" in 1 and 2, "car" in 2 and 3: 2 shares two.
        assert_eq!(
            kept_words_rank("what colour is my car", &pool),
            vec![2, 3, 1]
        );
        assert_eq!(kept_words_rank("which colour", &pool), vec![2, 1]);
        assert!(kept_words_rank("the and is", &pool).is_empty());
    }

    #[test]
    fn fts_folds_vietnamese_accents_and_ranks_within_its_pool() {
        let pool = [
            (1, "Tôi uống cà phê với sữa"),
            (2, "Chị gái tôi mở quán cà phê"),
            (3, "Đường đi"),
        ];
        let index = FtsIndex::new(&pool).unwrap();
        // Accents and đ fold: "duong" finds "Đường".
        assert_eq!(index.rank("duong", false).unwrap(), vec![3]);
        let first = index.rank("tôi uống cà phê thế nào", false).unwrap();
        assert_eq!(first.first(), Some(&1));
        // recall_v2 leaves out "tôi" but still finds both by "cà phê".
        let second = index.rank("tôi uống cà phê thế nào", true).unwrap();
        assert_eq!(second.first(), Some(&1));
        assert!(index.rank("là gì", true).unwrap().is_empty());
    }

    /// The partial-overlap set is what it says: at least 30 pairs, half
    /// Vietnamese, each question sharing a content word with its kept thing,
    /// and a distractor sharing a word with it too.
    #[test]
    fn the_overlap_set_shares_some_words_and_has_distractors() {
        let set = read_set(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/overlap-recall.json"
        )))
        .unwrap();
        let fixture = &set.fixture;
        assert!(fixture.questions.len() >= 30);
        let vi = fixture.questions.iter().filter(|q| q.lang == "vi").count();
        assert_eq!(2 * vi, fixture.questions.len());
        for question in &fixture.questions {
            let wanted = search::query_with(&question.text, true).unwrap();
            let words: Vec<&str> = wanted.split(" OR ").map(|w| w.trim_matches('"')).collect();
            let shares = |text: &str| {
                let body = search::text(text);
                let body: Vec<&str> = body.split(|c: char| !c.is_alphanumeric()).collect();
                words.iter().any(|word| body.contains(word))
            };
            let target = fixture.kept.iter().find(|e| e.id == question.kept).unwrap();
            assert_eq!(target.lang, question.lang);
            assert!(
                shares(&target.text),
                "question for {} shares no word",
                target.id
            );
            assert!(
                fixture
                    .kept
                    .iter()
                    .any(|e| e.id != target.id && e.lang == question.lang && shares(&e.text)),
                "question for {} has no distractor sharing a word",
                target.id
            );
        }
        let ranked = word_rankings(fixture).unwrap();
        assert_eq!(ranked["kept_words"].len(), fixture.questions.len());
    }

    #[test]
    fn several_thread_counts_run_one_process_each_with_the_same_other_arguments() {
        let given: Vec<String> = [
            "--fixture",
            "f",
            "--threads",
            "1,4",
            "--runtime",
            "r",
            "--runtime-sha256",
            "AB",
        ]
        .map(String::from)
        .into();
        let args = parse_args(
            given
                .iter()
                .cloned()
                .chain(["--model", "m", "--tokenizer", "t", "--sha256", "s"].map(String::from)),
        )
        .unwrap();
        assert_eq!(args.runtime_sha256.as_deref(), Some("ab"));
        let runs = per_thread_invocations(&given, &args.threads);
        let rest = ["--fixture", "f", "--runtime", "r", "--runtime-sha256", "AB"];
        assert_eq!(runs.len(), 2);
        for (run, count) in runs.iter().zip(["1", "4"]) {
            let mut wanted: Vec<&str> = rest.to_vec();
            wanted.extend(["--threads", count]);
            assert_eq!(run, &wanted);
            // Each child parses back to exactly one thread count.
            let child =
                parse_args(run.iter().cloned().chain(
                    ["--model", "m", "--tokenizer", "t", "--sha256", "s"].map(String::from),
                ))
                .unwrap();
            assert_eq!(child.threads, vec![count.parse::<usize>().unwrap()]);
        }
    }
}
