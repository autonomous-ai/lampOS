//! Gmail on the lamp: a small IMAP client (LOGIN, EXAMINE INBOX, UID SEARCH,
//! UID FETCH of headers and text, LOGOUT) over TLS to `imap.gmail.com:993`,
//! and SMTP over TLS to `smtp.gmail.com:465` (AUTH PLAIN or LOGIN, one
//! message). The mailbox is only ever opened with EXAMINE, read-only, and
//! every body is fetched with `BODY.PEEK`, so reading marks nothing read.
//!
//! One-time codes and security mails (sign-in codes, password resets,
//! sign-in alerts, verification) are never returned: [`is_security_mail`].
//!
//! The app password is only in the LOGIN and AUTH lines; every failure is
//! said as [`Failure`], never with what the server said.
use crate::{catalog, net};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

pub const IMAP_HOST: &str = "imap.gmail.com";
pub const IMAP_PORT: u16 = 993;
pub const SMTP_HOST: &str = "smtp.gmail.com";
pub const SMTP_PORT: u16 = 465;
/// How long one whole mail exchange may take.
pub const WITHIN: Duration = Duration::from_secs(20);
/// How long connecting may take.
pub const CONNECT_WITHIN: Duration = Duration::from_secs(5);
/// The longest protocol line read.
const LINE_MOST: usize = 65_536;
/// The longest literal read, and all of one response's.
const LITERAL_MOST: usize = 262_144;
const RESPONSE_MOST: usize = 2 << 20;
/// The most mails one list returns.
pub const LIST_MOST: usize = 10;
/// How much of a message's text is fetched for a list and for one message.
const PREVIEW_FETCH: usize = 4_096;
const MESSAGE_FETCH: usize = 65_536;
/// How much of a text is kept, in characters.
pub const PREVIEW_MOST: usize = 300;
pub const TEXT_MOST: usize = 3_000;
/// What stands in for a security mail.
pub const SECURITY_LEFT_OUT: &str = "a security mail, left out";

/// Why a mail exchange failed, said without anything the server sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The login was refused.
    Refused,
    /// No answer, a broken one, or a command refused after login.
    Unreached,
}

impl Failure {
    pub fn words(self) -> &'static str {
        match self {
            Self::Refused => "Gmail refused the app password",
            Self::Unreached => "Gmail couldn't be reached",
        }
    }
}

/// One mail as it is returned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MailItem {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub text: String,
}

/// What is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailAsk {
    /// The newest mails, at most this many.
    Recent(usize),
    /// Gmail's own search words (`X-GM-RAW`), at most this many.
    Search(String, usize),
    /// One mail by its id (its UID), with its text.
    Message(u32),
}

/// One message to send, from the account itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub text: String,
}

/// What reads and sends mail; a fake in tests.
pub trait Mail {
    fn login(&self, user: &str, pass: &str) -> impl Future<Output = Result<(), Failure>>;
    fn read(
        &self,
        user: &str,
        pass: &str,
        ask: &MailAsk,
    ) -> impl Future<Output = Result<Vec<MailItem>, Failure>>;
    fn send(
        &self,
        user: &str,
        pass: &str,
        out: &Outgoing,
    ) -> impl Future<Output = Result<(), Failure>>;
}

// --- Security mails ---------------------------------------------------------

/// Phrases that make a mail a security one, lower case.
const SECURITY_PHRASES: &[&str] = &[
    "verification code",
    "verify your",
    "verification link",
    "confirm your email",
    "confirm your account",
    "confirm your identity",
    "one-time",
    "one time pass",
    "one time code",
    "passcode",
    "security code",
    "login code",
    "log-in code",
    "sign-in code",
    "signin code",
    "sign in code",
    "authentication code",
    "access code",
    "2-step",
    "two-step",
    "two-factor",
    "password reset",
    "reset your password",
    "reset password",
    "forgot your password",
    "your password was changed",
    "password has been changed",
    "new sign-in",
    "sign-in attempt",
    "sign in attempt",
    "new login",
    "new device signed",
    "security alert",
    "suspicious",
    "unusual sign-in",
    "unusual activity",
    "account recovery",
    "recovery code",
    "magic link",
];

/// Short words that count only as whole words.
const SECURITY_WORDS: &[&str] = &["otp", "2fa", "mfa", "pin"];

fn whole_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// A run of 4 to 8 digits standing alone (a code), within `text`.
fn has_code_number(text: &str) -> bool {
    let mut run = 0;
    let mut alone = true;
    let mut previous: Option<char> = None;
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            if run == 0 {
                alone = !previous.is_some_and(|p| p.is_alphanumeric());
            }
            run += 1;
        } else {
            if run > 0 && (4..=8).contains(&run) && alone && !c.is_alphanumeric() {
                return true;
            }
            run = 0;
        }
        previous = Some(c);
    }
    false
}

/// Whether a mail is a security one: a one-time code, a password reset, a
/// sign-in alert or a verification. Such a mail is never spoken, forwarded
/// or kept: it is dropped from what is returned and said only as
/// [`SECURITY_LEFT_OUT`]. A mail that says "code" near a 4 to 8 digit
/// number counts too.
pub fn is_security_mail(from: &str, subject: &str, text: &str) -> bool {
    let all = format!("{from}\n{subject}\n{text}").to_lowercase();
    if SECURITY_PHRASES.iter().any(|phrase| all.contains(phrase))
        || SECURITY_WORDS.iter().any(|word| whole_word(&all, word))
    {
        return true;
    }
    all.match_indices("code").any(|(at, _)| {
        let mut start = at.saturating_sub(80);
        while !all.is_char_boundary(start) {
            start -= 1;
        }
        let mut end = (at + 84).min(all.len());
        while !all.is_char_boundary(end) {
            end += 1;
        }
        has_code_number(&all[start..end])
    })
}

// --- MIME -------------------------------------------------------------------

/// A message's header fields, unfolded, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Headers(Vec<(String, String)>);

impl Headers {
    /// The fields of `raw`, up to its first blank line.
    pub fn parse(raw: &[u8]) -> Headers {
        let text = String::from_utf8_lossy(raw);
        let mut fields: Vec<(String, String)> = Vec::new();
        for line in text.split('\n') {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                break;
            }
            if line.starts_with([' ', '\t']) {
                if let Some((_, value)) = fields.last_mut() {
                    value.push(' ');
                    value.push_str(line.trim());
                }
            } else if let Some((name, value)) = line.split_once(':') {
                fields.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
            }
        }
        Headers(fields)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(field, _)| field.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Text in a charset, as far as the lamp reads one: UTF-8 (and ASCII) or
/// Latin-1; anything else read as UTF-8, lossily.
fn charset_text(bytes: &[u8], charset: &str) -> String {
    let charset = charset.trim().trim_matches('"').to_ascii_lowercase();
    if ["iso-8859-1", "latin1", "windows-1252"].contains(&charset.as_str()) {
        bytes.iter().map(|&b| b as char).collect()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Quoted-printable decoded.
fn quoted_printable(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            if bytes[i + 1..].starts_with(b"\r\n") {
                i += 3;
                continue;
            }
            if bytes[i + 1..].starts_with(b"\n") {
                i += 2;
                continue;
            }
            let hex = bytes
                .get(i + 1..i + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok());
            if let Some(byte) = hex {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Base64 decoded, leniently: whitespace dropped, a cut-off end ignored.
fn base64_lenient(bytes: &[u8]) -> Vec<u8> {
    let clean: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let whole = clean.len() / 4 * 4;
    STANDARD.decode(&clean[..whole]).unwrap_or_default()
}

/// RFC 2047's encoded words (`=?charset?B|Q?text?=`) decoded; the space
/// between two of them dropped.
pub fn decode_words(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let (before, after) = rest.split_at(start);
        let decoded = (|| {
            let inner = &after[2..];
            let (charset, inner) = inner.split_once('?')?;
            let (encoding, inner) = inner.split_once('?')?;
            let end = inner.find("?=")?;
            let text = &inner[..end];
            let bytes = match encoding {
                "B" | "b" => base64_lenient(text.as_bytes()),
                "Q" | "q" => quoted_printable(text.replace('_', " ").as_bytes()),
                _ => return None,
            };
            let used = 2 + charset.len() + 1 + encoding.len() + 1 + end + 2;
            Some((charset_text(&bytes, charset), used))
        })();
        match decoded {
            Some((text, used)) => {
                if !(last_was_word && before.trim().is_empty()) {
                    out.push_str(before);
                }
                out.push_str(&text);
                rest = &after[used..];
                last_was_word = true;
            }
            None => {
                out.push_str(before);
                out.push_str("=?");
                rest = &after[2..];
                last_was_word = false;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A header parameter (`boundary`, `charset`), unquoted.
fn param(value: &str, name: &str) -> Option<String> {
    value.split(';').skip(1).find_map(|part| {
        let (key, value) = part.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().trim_matches('"').to_owned())
    })
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|window| window == needle)
}

/// A body part's headers and body.
fn split_part(part: &[u8]) -> (Headers, &[u8]) {
    let part = part
        .strip_prefix(b"\r\n")
        .or_else(|| part.strip_prefix(b"\n"))
        .unwrap_or(part);
    match find(part, b"\r\n\r\n")
        .map(|at| (at, 4))
        .or_else(|| find(part, b"\n\n").map(|at| (at, 2)))
    {
        Some((at, gap)) => (Headers::parse(&part[..at]), &part[at + gap..]),
        None => (Headers::parse(part), &[]),
    }
}

/// HTML as plain text: script and style dropped, tags removed, the common
/// entities read.
pub fn strip_html(html: &str) -> String {
    let mut out = String::new();
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while i < html.len() {
        if lower[i..].starts_with("<script") || lower[i..].starts_with("<style") {
            let close = if lower[i..].starts_with("<script") {
                "</script"
            } else {
                "</style"
            };
            // Past the closing tag's own `>`.
            i = lower[i..]
                .find(close)
                .and_then(|at| lower[i + at..].find('>').map(|end| i + at + end + 1))
                .unwrap_or(html.len());
            continue;
        }
        let c = html[i..].chars().next().unwrap_or(' ');
        if c == '<' {
            i = lower[i..].find('>').map_or(html.len(), |at| i + at + 1);
            out.push(' ');
            continue;
        }
        out.push(c);
        i += c.len_utf8();
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// One part's text: (plain or not, the text), multiparts searched for a
/// plain part first.
fn part_text(headers: &Headers, body: &[u8], depth: u8) -> Option<(bool, String)> {
    let content_type = headers.get("content-type").unwrap_or("text/plain");
    let kind = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if kind.starts_with("multipart/") {
        if depth >= 4 {
            return None;
        }
        let boundary = param(content_type, "boundary")?;
        let delimiter = format!("--{boundary}");
        let mut texts = Vec::new();
        let mut rest = body;
        while let Some(at) = find(rest, delimiter.as_bytes()) {
            let part = &rest[..at];
            if !texts.is_empty() || at > 0 {
                let (part_headers, part_body) = split_part(part);
                if !part.iter().all(u8::is_ascii_whitespace) {
                    texts.push(part_text(&part_headers, part_body, depth + 1));
                }
            }
            rest = &rest[at + delimiter.len()..];
            if rest.starts_with(b"--") {
                break;
            }
        }
        // A part cut off by the fetch's limit still counts.
        if !rest.starts_with(b"--") && !rest.iter().all(u8::is_ascii_whitespace) {
            let (part_headers, part_body) = split_part(rest);
            texts.push(part_text(&part_headers, part_body, depth + 1));
        }
        let texts: Vec<(bool, String)> = texts.into_iter().flatten().collect();
        return texts
            .iter()
            .find(|(plain, _)| *plain)
            .or_else(|| texts.first())
            .cloned();
    }
    let decoded = match headers
        .get("content-transfer-encoding")
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("base64") => base64_lenient(body),
        Some("quoted-printable") => quoted_printable(body),
        _ => body.to_vec(),
    };
    let text = charset_text(
        &decoded,
        &param(content_type, "charset").unwrap_or_default(),
    );
    match kind.as_str() {
        "text/plain" | "" => Some((true, text)),
        "text/html" => Some((false, strip_html(&text))),
        _ => None,
    }
}

/// Whitespace tidied and the text cut to `most` characters, with "…".
pub fn tidy(text: &str, most: usize) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(&line);
    }
    match out.char_indices().nth(most) {
        Some((at, _)) => format!("{}…", &out[..at]),
        None => out,
    }
}

/// A message's readable text from its headers and body.
pub fn body_text(headers: &Headers, body: &[u8], most: usize) -> String {
    tidy(
        &part_text(headers, body, 0)
            .map(|(_, text)| text)
            .unwrap_or_default(),
        most,
    )
}

// --- IMAP -------------------------------------------------------------------

/// One server response: its text, each literal replaced by `{}`, and each
/// literal with the text just before it.
#[derive(Clone, Debug, Default)]
struct Response {
    text: String,
    literals: Vec<(String, Vec<u8>)>,
}

/// An IMAP session on any stream: TLS on the lamp, a script in tests.
pub struct Imap<S> {
    io: BufReader<S>,
    tag: u32,
}

/// An IMAP quoted string, or `None` for text that one can't carry (a line
/// break, a NUL, anything not ASCII).
fn imap_quoted(text: &str) -> Option<String> {
    if !text.is_ascii() || text.chars().any(|c| c.is_ascii_control()) {
        return None;
    }
    Some(format!(
        "\"{}\"",
        text.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

async fn read_line<S: AsyncRead + Unpin>(io: &mut BufReader<S>) -> Result<Vec<u8>, Failure> {
    let mut line = Vec::new();
    loop {
        let buffer = io.fill_buf().await.map_err(|_| Failure::Unreached)?;
        if buffer.is_empty() {
            return Err(Failure::Unreached);
        }
        if let Some(at) = buffer.iter().position(|&b| b == b'\n') {
            line.extend_from_slice(&buffer[..=at]);
            io.consume(at + 1);
            break;
        }
        let n = buffer.len();
        line.extend_from_slice(buffer);
        io.consume(n);
        if line.len() > LINE_MOST {
            return Err(Failure::Unreached);
        }
    }
    if line.len() > LINE_MOST {
        return Err(Failure::Unreached);
    }
    Ok(line)
}

/// A line's literal size, when it ends with `{n}`.
fn literal_size(line: &str) -> Option<usize> {
    let open = line.strip_suffix('}')?.rfind('{')?;
    line[open + 1..line.len() - 1].parse().ok()
}

impl<S: AsyncRead + AsyncWrite + Unpin> Imap<S> {
    pub fn new(stream: S) -> Self {
        Imap {
            io: BufReader::new(stream),
            tag: 0,
        }
    }

    async fn response(&mut self) -> Result<Response, Failure> {
        let mut response = Response::default();
        let mut total = 0;
        loop {
            let raw = read_line(&mut self.io).await?;
            let line = String::from_utf8_lossy(&raw)
                .trim_end_matches(['\r', '\n'])
                .to_owned();
            match literal_size(&line) {
                Some(size) => {
                    total += size;
                    if size > LITERAL_MOST || total > RESPONSE_MOST {
                        return Err(Failure::Unreached);
                    }
                    let label = line[..line.rfind('{').unwrap_or(0)].to_owned();
                    let mut literal = vec![0; size];
                    self.io
                        .read_exact(&mut literal)
                        .await
                        .map_err(|_| Failure::Unreached)?;
                    response.text.push_str(&label);
                    response.text.push_str("{}");
                    response.literals.push((label, literal));
                }
                None => {
                    response.text.push_str(&line);
                    return Ok(response);
                }
            }
        }
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<(), Failure> {
        let stream = self.io.get_mut();
        stream
            .write_all(bytes)
            .await
            .map_err(|_| Failure::Unreached)?;
        stream.flush().await.map_err(|_| Failure::Unreached)
    }

    /// The server's greeting.
    pub async fn greet(&mut self) -> Result<(), Failure> {
        let greeting = self.response().await?;
        if greeting.text.starts_with("* OK") || greeting.text.starts_with("* PREAUTH") {
            Ok(())
        } else {
            Err(Failure::Unreached)
        }
    }

    /// One command, and with `literal` a last argument sent as a literal.
    /// Its untagged responses on OK; `Refused` on NO or BAD.
    async fn run(
        &mut self,
        command: &str,
        literal: Option<&[u8]>,
    ) -> Result<Vec<Response>, Failure> {
        self.tag += 1;
        let tag = format!("A{}", self.tag);
        match literal {
            None => {
                self.write(format!("{tag} {command}\r\n").as_bytes())
                    .await?
            }
            Some(literal) => {
                self.write(format!("{tag} {command} {{{}}}\r\n", literal.len()).as_bytes())
                    .await?;
                let go_on = self.response().await?;
                if !go_on.text.starts_with('+') {
                    return Err(Failure::Refused);
                }
                let mut rest = literal.to_vec();
                rest.extend_from_slice(b"\r\n");
                self.write(&rest).await?;
            }
        }
        let mut untagged = Vec::new();
        loop {
            let response = self.response().await?;
            if let Some(status) = response.text.strip_prefix(&format!("{tag} ")) {
                return if status.starts_with("OK") {
                    Ok(untagged)
                } else {
                    Err(Failure::Refused)
                };
            }
            if response.text.starts_with("* ") {
                untagged.push(response);
            } else {
                return Err(Failure::Unreached);
            }
        }
    }

    /// LOGIN; a refusal is [`Failure::Refused`].
    pub async fn login(&mut self, user: &str, pass: &str) -> Result<(), Failure> {
        let (Some(user), Some(pass)) = (imap_quoted(user), imap_quoted(pass)) else {
            return Err(Failure::Refused);
        };
        self.run(&format!("LOGIN {user} {pass}"), None)
            .await
            .map(|_| ())
    }

    /// EXAMINE: the mailbox opened read-only.
    pub async fn examine(&mut self, mailbox: &str) -> Result<(), Failure> {
        let mailbox = imap_quoted(mailbox).ok_or(Failure::Unreached)?;
        self.run(&format!("EXAMINE {mailbox}"), None)
            .await
            .map(|_| ())
            .map_err(|_| Failure::Unreached)
    }

    /// UID SEARCH: every UID found, oldest first. `words` are Gmail's own
    /// search (`X-GM-RAW`); none is every mail.
    pub async fn search(&mut self, words: Option<&str>) -> Result<Vec<u32>, Failure> {
        let found = match words {
            None => self.run("UID SEARCH ALL", None).await,
            Some(words) => match imap_quoted(words) {
                Some(quoted) => {
                    self.run(&format!("UID SEARCH X-GM-RAW {quoted}"), None)
                        .await
                }
                None => {
                    if words.chars().any(char::is_control) {
                        return Err(Failure::Unreached);
                    }
                    self.run("UID SEARCH CHARSET UTF-8 X-GM-RAW", Some(words.as_bytes()))
                        .await
                }
            },
        }
        .map_err(|_| Failure::Unreached)?;
        let mut uids: Vec<u32> = found
            .iter()
            .filter_map(|response| response.text.strip_prefix("* SEARCH"))
            .flat_map(|rest| rest.split_whitespace().filter_map(|n| n.parse().ok()))
            .collect();
        uids.sort_unstable();
        Ok(uids)
    }

    /// UID FETCH of each mail's From, Subject and Date, and the first
    /// `text_most` bytes of its text, read with PEEK (nothing marked read).
    pub async fn fetch(
        &mut self,
        uids: &[u32],
        text_most: usize,
        keep: usize,
    ) -> Result<Vec<MailItem>, Failure> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let fetched = self
            .run(
                &format!(
                    "UID FETCH {set} (UID BODY.PEEK[HEADER.FIELDS (FROM SUBJECT DATE CONTENT-TYPE CONTENT-TRANSFER-ENCODING)] BODY.PEEK[TEXT]<0.{text_most}>)"
                ),
                None,
            )
            .await
            .map_err(|_| Failure::Unreached)?;
        let mut items: Vec<(u32, MailItem)> = fetched
            .iter()
            .filter(|response| response.text.contains(" FETCH ("))
            .filter_map(|response| {
                let uid: u32 = response
                    .text
                    .split("UID ")
                    .nth(1)?
                    .split(|c: char| !c.is_ascii_digit())
                    .next()?
                    .parse()
                    .ok()?;
                let head = response
                    .literals
                    .iter()
                    .find(|(label, _)| label.contains("HEADER"))
                    .map(|(_, bytes)| Headers::parse(bytes))
                    .unwrap_or_default();
                let body = response
                    .literals
                    .iter()
                    .find(|(label, _)| label.contains("TEXT]"))
                    .map_or(&[][..], |(_, bytes)| bytes.as_slice());
                let field = |name| decode_words(head.get(name).unwrap_or(""));
                Some((
                    uid,
                    MailItem {
                        id: uid.to_string(),
                        from: field("from"),
                        subject: field("subject"),
                        date: field("date"),
                        text: body_text(&head, body, keep),
                    },
                ))
            })
            .collect();
        items.sort_by_key(|item| std::cmp::Reverse(item.0));
        Ok(items.into_iter().map(|(_, item)| item).collect())
    }

    /// LOGOUT, its answer not waited on for long.
    pub async fn logout(&mut self) {
        let _ = tokio::time::timeout(Duration::from_secs(2), self.run("LOGOUT", None)).await;
    }
}

/// A whole read: login, EXAMINE INBOX, the search or the one mail, logout.
pub async fn read_mail<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    user: &str,
    pass: &str,
    ask: &MailAsk,
) -> Result<Vec<MailItem>, Failure> {
    let mut imap = Imap::new(stream);
    imap.greet().await?;
    imap.login(user, pass).await?;
    imap.examine("INBOX").await?;
    let items = match ask {
        MailAsk::Recent(most) => {
            let uids = imap.search(None).await?;
            let newest = &uids[uids.len().saturating_sub(*most)..];
            imap.fetch(newest, PREVIEW_FETCH, PREVIEW_MOST).await?
        }
        MailAsk::Search(words, most) => {
            let uids = imap.search(Some(words)).await?;
            let newest = &uids[uids.len().saturating_sub(*most)..];
            imap.fetch(newest, PREVIEW_FETCH, PREVIEW_MOST).await?
        }
        MailAsk::Message(uid) => imap.fetch(&[*uid], MESSAGE_FETCH, TEXT_MOST).await?,
    };
    imap.logout().await;
    Ok(items)
}

/// A login and nothing more, for `set`.
pub async fn check_login<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    user: &str,
    pass: &str,
) -> Result<(), Failure> {
    let mut imap = Imap::new(stream);
    imap.greet().await?;
    imap.login(user, pass).await?;
    imap.logout().await;
    Ok(())
}

// --- SMTP -------------------------------------------------------------------

/// Whether an address is one plain mailbox: `local@domain.tld`, nothing a
/// header or a command could be split on.
pub fn address_ok(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    address.len() <= 254
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && address
            .chars()
            .all(|c| c.is_ascii_graphic() && !"<>()[],;:\\\"@".contains(c) || c == '@')
        && address.matches('@').count() == 1
}

/// RFC 5322's date for a Unix time, in UTC.
pub fn rfc5322_date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} +0000",
        WEEKDAYS[days.rem_euclid(7) as usize],
        day,
        MONTHS[(month - 1) as usize],
        year,
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// A header value as an encoded word when it isn't plain ASCII.
fn header_text(text: &str) -> String {
    if text.is_ascii() {
        text.to_owned()
    } else {
        format!("=?UTF-8?B?{}?=", STANDARD.encode(text))
    }
}

/// The message, ready for DATA: plain text, UTF-8, its body in base64 (so
/// no line of it can end the DATA early).
pub fn message(from: &str, out: &Outgoing, date: &str, id: &str) -> Vec<u8> {
    let mut text = format!("From: {from}\r\nTo: {}\r\n", out.to.join(", "));
    if !out.cc.is_empty() {
        text.push_str(&format!("Cc: {}\r\n", out.cc.join(", ")));
    }
    text.push_str(&format!(
        "Subject: {}\r\nDate: {date}\r\nMessage-ID: <{id}@lamp.local>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n",
        header_text(&out.subject)
    ));
    let body = STANDARD.encode(out.text.replace("\r\n", "\n").replace('\n', "\r\n"));
    for line in body.as_bytes().chunks(76) {
        text.push_str(std::str::from_utf8(line).unwrap_or(""));
        text.push_str("\r\n");
    }
    text.into_bytes()
}

/// One SMTP reply: its code and its lines' texts.
async fn smtp_reply<S: AsyncRead + Unpin>(
    io: &mut BufReader<S>,
) -> Result<(u16, Vec<String>), Failure> {
    let mut lines = Vec::new();
    loop {
        let raw = read_line(io).await?;
        let line = String::from_utf8_lossy(&raw)
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        let code: u16 = line
            .get(..3)
            .and_then(|code| code.parse().ok())
            .ok_or(Failure::Unreached)?;
        let more = line.as_bytes().get(3) == Some(&b'-');
        lines.push(line.get(4..).unwrap_or("").to_owned());
        if !more {
            return Ok((code, lines));
        }
        if lines.len() > 100 {
            return Err(Failure::Unreached);
        }
    }
}

async fn smtp_say<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut BufReader<S>,
    line: &[u8],
) -> Result<(u16, Vec<String>), Failure> {
    let stream = io.get_mut();
    stream
        .write_all(line)
        .await
        .map_err(|_| Failure::Unreached)?;
    stream.flush().await.map_err(|_| Failure::Unreached)?;
    smtp_reply(io).await
}

/// One message sent: EHLO, AUTH (PLAIN, else LOGIN), MAIL, RCPT for each
/// recipient, DATA, QUIT. A refused AUTH is [`Failure::Refused`]; anything
/// else refused is [`Failure::Unreached`] and nothing is sent.
pub async fn send_mail<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    user: &str,
    pass: &str,
    out: &Outgoing,
    message: &[u8],
) -> Result<(), Failure> {
    let mut io = BufReader::new(stream);
    let expect = |(code, _): (u16, Vec<String>), wanted: &[u16]| {
        if wanted.contains(&code) {
            Ok(())
        } else {
            Err(Failure::Unreached)
        }
    };
    expect(smtp_reply(&mut io).await?, &[220])?;
    let (code, lines) = smtp_say(&mut io, b"EHLO lamp\r\n").await?;
    if code != 250 {
        return Err(Failure::Unreached);
    }
    let mechanisms: Vec<String> = lines
        .iter()
        .filter_map(|line| {
            line.to_ascii_uppercase()
                .strip_prefix("AUTH ")
                .map(str::to_owned)
        })
        .flat_map(|rest| {
            rest.split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();
    let (user_ok, pass_ok) = (
        !user.chars().any(char::is_control),
        !pass.chars().any(char::is_control),
    );
    if !user_ok || !pass_ok {
        return Err(Failure::Refused);
    }
    let authed = if mechanisms.iter().any(|m| m == "PLAIN") {
        let token = STANDARD.encode(format!("\0{user}\0{pass}"));
        smtp_say(&mut io, format!("AUTH PLAIN {token}\r\n").as_bytes())
            .await?
            .0
    } else if mechanisms.iter().any(|m| m == "LOGIN") {
        expect(smtp_say(&mut io, b"AUTH LOGIN\r\n").await?, &[334])?;
        expect(
            smtp_say(&mut io, format!("{}\r\n", STANDARD.encode(user)).as_bytes()).await?,
            &[334],
        )?;
        smtp_say(&mut io, format!("{}\r\n", STANDARD.encode(pass)).as_bytes())
            .await?
            .0
    } else {
        return Err(Failure::Unreached);
    };
    if authed != 235 {
        return Err(Failure::Refused);
    }
    expect(
        smtp_say(&mut io, format!("MAIL FROM:<{user}>\r\n").as_bytes()).await?,
        &[250],
    )?;
    for recipient in out.to.iter().chain(&out.cc) {
        if !address_ok(recipient) {
            return Err(Failure::Unreached);
        }
        expect(
            smtp_say(&mut io, format!("RCPT TO:<{recipient}>\r\n").as_bytes()).await?,
            &[250, 251],
        )?;
    }
    expect(smtp_say(&mut io, b"DATA\r\n").await?, &[354])?;
    let mut data = message.to_vec();
    data.extend_from_slice(b".\r\n");
    expect(smtp_say(&mut io, &data).await?, &[250])?;
    let _ = tokio::time::timeout(Duration::from_secs(2), smtp_say(&mut io, b"QUIT\r\n")).await;
    Ok(())
}

// --- Gmail over TLS -----------------------------------------------------------

/// The real Gmail: TLS to its own hosts only.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gmail;

async fn connect(
    host: &str,
    port: u16,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, Failure> {
    if !catalog::host_allowed("gmail", host) {
        return Err(Failure::Unreached);
    }
    let config = Arc::new(net::tls().map_err(|_| Failure::Unreached)?);
    let tcp = tokio::time::timeout(CONNECT_WITHIN, tokio::net::TcpStream::connect((host, port)))
        .await
        .map_err(|_| Failure::Unreached)?
        .map_err(|_| Failure::Unreached)?;
    let name =
        rustls::pki_types::ServerName::try_from(host.to_owned()).map_err(|_| Failure::Unreached)?;
    tokio::time::timeout(
        CONNECT_WITHIN,
        tokio_rustls::TlsConnector::from(config).connect(name, tcp),
    )
    .await
    .map_err(|_| Failure::Unreached)?
    .map_err(|_| Failure::Unreached)
}

impl Mail for Gmail {
    async fn login(&self, user: &str, pass: &str) -> Result<(), Failure> {
        tokio::time::timeout(WITHIN, async {
            check_login(connect(IMAP_HOST, IMAP_PORT).await?, user, pass).await
        })
        .await
        .map_err(|_| Failure::Unreached)?
    }

    async fn read(&self, user: &str, pass: &str, ask: &MailAsk) -> Result<Vec<MailItem>, Failure> {
        tokio::time::timeout(WITHIN, async {
            read_mail(connect(IMAP_HOST, IMAP_PORT).await?, user, pass, ask).await
        })
        .await
        .map_err(|_| Failure::Unreached)?
    }

    async fn send(&self, user: &str, pass: &str, out: &Outgoing) -> Result<(), Failure> {
        let id = crate::writes::random_id();
        let text = message(user, out, &rfc5322_date(crate::now_secs()), &id);
        tokio::time::timeout(WITHIN, async {
            send_mail(connect(SMTP_HOST, SMTP_PORT).await?, user, pass, out, &text).await
        })
        .await
        .map_err(|_| Failure::Unreached)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    /// Run a future on a small runtime, as each request does.
    fn run<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    /// A server that answers a script: for each line the client sends, the
    /// next canned reply; all the client sent is returned.
    async fn scripted(
        server: tokio::io::DuplexStream,
        greeting: &str,
        replies: Vec<String>,
    ) -> String {
        let mut io = BufReader::new(server);
        let mut heard = String::new();
        io.get_mut().write_all(greeting.as_bytes()).await.unwrap();
        for reply in replies {
            let mut line = String::new();
            if io.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            // DATA's message comes as many lines, up to the lone dot.
            if heard.ends_with("DATA\r\n") {
                while line != ".\r\n" {
                    heard.push_str(&line);
                    line.clear();
                    io.read_line(&mut line).await.unwrap();
                }
            }
            heard.push_str(&line);
            io.get_mut().write_all(reply.as_bytes()).await.unwrap();
        }
        heard
    }

    #[test]
    fn security_mails_are_found() {
        for (from, subject, text) in [
            (
                "Google <no-reply@accounts.google.com>",
                "Security alert",
                "",
            ),
            ("x@y.z", "Your verification code", ""),
            (
                "x@y.z",
                "Hello",
                "Your code is 482913. It expires in 10 minutes.",
            ),
            ("x@y.z", "Reset your password", "Click the link"),
            ("x@y.z", "New sign-in to your account", ""),
            ("x@y.z", "Your OTP", ""),
            ("x@y.z", "Đăng nhập", "Mã code: 1234"),
            ("x@y.z", "Login", "Use 2FA to finish"),
            ("x@y.z", "Welcome", "Please verify your email address"),
        ] {
            assert!(is_security_mail(from, subject, text), "{subject} {text}");
        }
        for (from, subject, text) in [
            (
                "mom@home.test",
                "Dinner on Sunday",
                "Bring the 2024 photos, ok?",
            ),
            ("boss@work.test", "Notes", "The code review is at 3 pm."),
            (
                "shop@x.test",
                "Order 12345678 shipped",
                "Your parcel is on its way",
            ),
            ("x@y.z", "Topic", "Shopping list: apples"),
        ] {
            assert!(!is_security_mail(from, subject, text), "{subject} {text}");
        }
    }

    #[test]
    fn mime_text_is_read_from_parts_and_encodings() {
        assert_eq!(
            decode_words("=?UTF-8?B?SMOgIE7hu5lp?= =?UTF-8?Q?_ng=C3=A0y?= x"),
            "Hà Nội ngày x"
        );
        assert_eq!(decode_words("plain =?bad"), "plain =?bad");
        let headers = Headers::parse(
            b"Content-Type: multipart/alternative;\r\n boundary=\"b1\"\r\nSubject: hi\r\n\r\n",
        );
        let body = b"--b1\r\nContent-Type: text/html\r\n\r\n<p>html</p>\r\n--b1\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nH=C3=A0 soft=\r\nbreak\r\n--b1--\r\n";
        assert_eq!(body_text(&headers, body, 100), "Hà softbreak");
        let html =
            Headers::parse(b"Content-Type: text/html\r\nContent-Transfer-Encoding: base64\r\n\r\n");
        let encoded = STANDARD.encode("<style>x{}</style><b>Bold</b> &amp; more");
        assert_eq!(body_text(&html, encoded.as_bytes(), 100), "Bold & more");
        assert_eq!(tidy("a  b\n\n\n c", 100), "a b\n\nc");
        assert_eq!(tidy("abcdef", 3), "abc…");
    }

    #[test]
    fn imap_reads_read_only_and_never_marks_a_mail() {
        let (client, server) = duplex(1 << 16);
        let header =
            "From: Mom <mom@home.test>\r\nSubject: Dinner\r\nDate: Mon, 2 Nov 2026\r\n\r\n";
        let text = "See you at 7";
        let fetch = format!(
            "* 1 FETCH (UID 41 BODY[HEADER.FIELDS (FROM SUBJECT DATE CONTENT-TYPE CONTENT-TRANSFER-ENCODING)] {{{}}}\r\n{header} BODY[TEXT]<0> {{{}}}\r\n{text})\r\nA4 OK done\r\n",
            header.len(),
            text.len()
        );
        let replies = vec![
            "A1 OK logged in\r\n".to_owned(),
            "* 3 EXISTS\r\nA2 OK [READ-ONLY] examined\r\n".to_owned(),
            "* SEARCH 41\r\nA3 OK\r\n".to_owned(),
            fetch,
            "* BYE\r\nA5 OK\r\n".to_owned(),
        ];
        let (items, heard) = run(async {
            let server = tokio::spawn(scripted(server, "* OK Gimap ready\r\n", replies));
            let items = read_mail(
                client,
                "a@b.c",
                "pass word",
                &MailAsk::Search("from:mom".into(), 5),
            )
            .await;
            (items, server.await.unwrap())
        });
        let items = items.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "41");
        assert_eq!(items[0].subject, "Dinner");
        assert_eq!(items[0].from, "Mom <mom@home.test>");
        assert_eq!(items[0].text, "See you at 7");
        let lines: Vec<&str> = heard.lines().collect();
        assert_eq!(lines[0], "A1 LOGIN \"a@b.c\" \"pass word\"");
        assert_eq!(lines[1], "A2 EXAMINE \"INBOX\"");
        assert_eq!(lines[2], "A3 UID SEARCH X-GM-RAW \"from:mom\"");
        assert!(lines[3].starts_with("A4 UID FETCH 41 (UID BODY.PEEK[HEADER.FIELDS"));
        assert!(lines[3].contains("BODY.PEEK[TEXT]<0.4096>"));
        assert!(
            !heard.contains("SELECT") && !heard.contains("STORE") && !heard.contains("BODY[TEXT]")
        );
        // A refused login is said as refused, without the server's words.
        let (client, server) = duplex(1 << 12);
        let refused = run(async {
            tokio::spawn(scripted(
                server,
                "* OK\r\n",
                vec!["A1 NO [AUTHENTICATIONFAILED] Invalid credentials planted\r\n".into()],
            ));
            check_login(client, "a@b.c", "x").await
        });
        assert_eq!(refused, Err(Failure::Refused));
        // A password that can't be quoted never reaches the wire.
        let (client, server) = duplex(1 << 12);
        let broken = run(async {
            let server = tokio::spawn(scripted(server, "* OK\r\n", vec![]));
            let result = check_login(client, "a@b.c", "x\r\nA9 DELETE INBOX").await;
            (result, server)
        });
        assert_eq!(broken.0, Err(Failure::Refused));
    }

    #[test]
    fn smtp_sends_one_message_after_auth() {
        let (client, server) = duplex(1 << 16);
        let out = Outgoing {
            to: vec!["mom@home.test".into()],
            cc: vec!["dad@home.test".into()],
            subject: "Chào".into(),
            text: "Home by 7.\n.\nLove".into(),
        };
        let text = message("a@b.c", &out, &rfc5322_date(0), "id1");
        let replies = vec![
            "250-smtp.gmail.com\r\n250-AUTH LOGIN PLAIN\r\n250 SMTPUTF8\r\n".to_owned(),
            "235 ok\r\n".into(),
            "250 ok\r\n".into(),
            "250 ok\r\n".into(),
            "250 ok\r\n".into(),
            "354 go\r\n".into(),
            "250 queued\r\n".into(),
            "221 bye\r\n".into(),
        ];
        let (sent, heard) = run(async {
            let server = tokio::spawn(scripted(server, "220 ready\r\n", replies));
            let sent = send_mail(client, "a@b.c", "app pw", &out, &text).await;
            (sent, server.await.unwrap())
        });
        sent.unwrap();
        let plain = STANDARD.encode("\0a@b.c\0app pw");
        assert!(heard.contains(&format!("AUTH PLAIN {plain}\r\n")));
        assert!(heard.contains("RCPT TO:<mom@home.test>\r\nRCPT TO:<dad@home.test>\r\nDATA\r\n"));
        assert!(heard.contains("Subject: =?UTF-8?B?"));
        assert!(heard.contains("Date: Thu, 01 Jan 1970 00:00:00 +0000"));
        // A refused AUTH sends nothing more.
        let (client, server) = duplex(1 << 12);
        let (refused, heard) = run(async {
            let server = tokio::spawn(scripted(
                server,
                "220 ready\r\n",
                vec!["250 AUTH PLAIN\r\n".into(), "535 no\r\n".into()],
            ));
            let refused = send_mail(client, "a@b.c", "x", &out, &text).await;
            (refused, server.await.unwrap())
        });
        assert_eq!(refused, Err(Failure::Refused));
        assert!(!heard.contains("MAIL FROM"));
    }

    #[test]
    fn addresses_and_dates_are_plain() {
        assert!(address_ok("a.b+c@d.example"));
        for bad in [
            "a",
            "a@b",
            "a@@b.c",
            "a b@c.d",
            "<a@b.c>",
            "a@b.c,d@e.f",
            "a@b.c\r\n",
            "@b.c",
        ] {
            assert!(!address_ok(bad), "{bad}");
        }
        assert_eq!(
            rfc5322_date(1_793_664_000),
            "Tue, 03 Nov 2026 00:00:00 +0000"
        );
    }
}
