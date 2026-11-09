//! Gmail on the lamp: a small IMAP client (LOGIN, LIST, EXAMINE, UID SEARCH,
//! UID FETCH of headers and text, LOGOUT) over TLS to `imap.gmail.com:993`,
//! and SMTP over TLS to `smtp.gmail.com:465` (AUTH PLAIN or LOGIN, one
//! message). `recent` reads the inbox; `search` reads All Mail, as Gmail's
//! own search does (the mailbox LIST flags `\All`, else the inbox). A
//! mailbox is only ever opened with EXAMINE, read-only, and every body is
//! fetched with `BODY.PEEK`, so reading marks nothing read.
//!
//! One-time codes and security mails (sign-in codes, password resets,
//! sign-in alerts, verification) are never returned: [`is_security_mail`].
//!
//! The app password is only in the LOGIN and AUTH lines; every failure is
//! said as [`Failure`], never with what the server said.
//!
//! A link made with a Google sign-in (OAuth, for an account that can't
//! make an app password, as Google Workspace's often can't) reads and
//! sends through Gmail's REST API instead: the calls and their answers
//! here, the requests made by `serve.rs` with the token.
use crate::{catalog, net, reads};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use serde_json::{Value, json};
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
/// Said after a mail's preview when its whole text came past the lamp's
/// 1 MB answer limit (`net::BODY_MOST`) and was cut, so it doesn't read.
pub const TOO_LONG: &str = "(the rest of this mail is too long to read here)";
/// What stands in for a security mail.
pub const SECURITY_LEFT_OUT: &str = "a security mail, left out";
/// What stands in for a security item of any other service.
pub const SECURITY_MESSAGE_LEFT_OUT: &str = "a security message, left out";

/// Why a mail exchange failed, said without anything the server sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The login was refused.
    Refused,
    /// Gmail's REST API refused a Google sign-in's token.
    SignInRefused,
    /// No answer, a broken one, or a command refused after login.
    Unreached,
}

impl Failure {
    pub fn words(self) -> &'static str {
        match self {
            Self::Refused => "Gmail refused the app password",
            Self::SignInRefused => "Google refused the Gmail sign-in",
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
    /// One mail by its id (over IMAP its UID in the inbox, or
    /// [`ALL_MAIL_ID`] and its UID in All Mail; Gmail's own id over the
    /// REST API), with its text.
    Message(String),
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

/// Phrases that make a mail a security one, lower case, Vietnamese with
/// and without its accents too.
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
    "new password",
    "change your password",
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
    "token=",
    // Vietnamese: a verification or sign-in code, a password, signing in.
    "mã xác minh",
    "ma xac minh",
    "mã xác thực",
    "ma xac thuc",
    "mã otp",
    "ma otp",
    "mã đăng nhập",
    "ma dang nhap",
    "mã bảo mật",
    "ma bao mat",
    "mật khẩu",
    "mat khau",
    "đặt lại mật khẩu",
    "dat lai mat khau",
    "đăng nhập",
    "dang nhap",
    "steam guard",
];

/// Short words that count only as whole words.
const SECURITY_WORDS: &[&str] = &["otp", "2fa", "mfa", "pin"];

/// Words that make a mail a security one when a code ([`has_code_number`],
/// [`has_mixed_code`]) is within [`NEAR`] of them.
const CODE_WORDS: &[&str] = &["code", "mã", "token", "pin", "verification"];

/// How far from a code word a code still counts, in bytes either way.
const NEAR: usize = 200;

/// Words that ask to enter or confirm something: a security text only with
/// a code-shaped token next to them, within [`NEAR_WEAK`] in the same
/// clause ([`weak_code_near`]), never a date, a year, a price, a version, a
/// flight, a room or a street number.
const WEAK_CODE_WORDS: &[&str] = &[
    "enter",
    "confirm",
    "confirmation",
    "verify",
    "nhập",
    "xác nhận",
    "xac nhan",
];

/// How far from a weak code word a code still counts, in bytes either way.
const NEAR_WEAK: usize = 80;

/// Words before a number that say it is no code ("room 1204", "build
/// 4521", "flight 123").
const NUMBER_LABELS: &[&str] = &[
    "room", "suite", "floor", "build", "version", "ver", "flight", "gate", "seat", "order",
    "invoice", "apt", "unit", "door", "phòng", "phong", "tầng", "tang", "số", "so",
];

/// What ends a clause between a weak code word and a code.
const CLAUSE_ENDS: &[char] = &[',', ';', '!', '?', '\n'];

/// What stands before or after a price.
const MONEY: &[char] = &['$', '€', '£', '¥', '₫', 'đ', '%'];

/// Words in a link's path that make it a sign-in or verification link.
const LINK_PATH_WORDS: &[&str] = &[
    "magic", "verify", "login", "log-in", "signin", "sign-in", "auth", "confirm", "reset", "otp",
];

/// Words that ask to follow a link, beside which a long opaque value in it
/// is a sign-in link's secret.
const FOLLOW_WORDS: &[&str] = &["click", "link", "tap", "nhấn", "bấm"];

/// The shortest opaque query value taken for a link's secret.
const OPAQUE_LEAST: usize = 24;

/// Signing in, which makes a mail a security one with a link or a code
/// number anywhere in it ("Use 482913 to sign in", "Log in to Notion:
/// https://…").
const SIGN_IN_WORDS: &[&str] = &[
    "sign in",
    "sign-in",
    "signin",
    "log in",
    "log-in",
    "login",
    "đăng nhập",
    "dang nhap",
];

/// What starts a link.
const LINK_STARTS: &[&str] = &["http://", "https://", "www."];

fn whole_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// A number standing alone in a text: its bytes, how many digits, and its
/// groups and what split them ("482 913", "2026-11-04").
struct Number {
    start: usize,
    end: usize,
    digits: usize,
    groups: Vec<usize>,
    splits: Vec<char>,
}

impl Number {
    /// A date: 4-2-2 or 2-2-4 digits split by dashes or dots.
    fn is_date(&self) -> bool {
        (self.groups == [4, 2, 2] || self.groups == [2, 2, 4])
            && self.splits.iter().all(|split| matches!(split, '-' | '.'))
    }

    /// A code: 4 to 8 digits, not a date.
    fn is_code(&self) -> bool {
        (4..=8).contains(&self.digits) && !self.is_date()
    }
}

/// The numbers standing alone within `text`: digits, unbroken or in groups
/// split by one space, dash or dot, or a dash between spaces ("482913",
/// "482 913", "482-913", "482.913", "482 - 913", "4 8 2 9 1 3").
fn numbers(text: &str) -> Vec<Number> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let byte = |at: usize| chars.get(at).map_or(text.len(), |(byte, _)| *byte);
    let digit = |at: usize| chars.get(at).is_some_and(|(_, c)| c.is_ascii_digit());
    let mut found = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if !digit(at) || (at > 0 && chars[at - 1].1.is_alphanumeric()) {
            at += 1;
            continue;
        }
        let mut number = Number {
            start: byte(at),
            end: 0,
            digits: 0,
            groups: Vec::new(),
            splits: Vec::new(),
        };
        let mut end = at;
        loop {
            let from = end;
            while digit(end) {
                end += 1;
            }
            number.digits += end - from;
            number.groups.push(end - from);
            let c = |at: usize| chars.get(at).map(|(_, c)| *c);
            let gap = if matches!(c(end), Some(' ' | '-' | '.')) && digit(end + 1) {
                1
            } else if c(end) == Some(' ')
                && matches!(c(end + 1), Some('-' | '–'))
                && c(end + 2) == Some(' ')
                && digit(end + 3)
            {
                3
            } else {
                break;
            };
            number.splits.push(chars[end + gap / 2].1);
            end += gap;
        }
        number.end = byte(end);
        if !chars.get(end).is_some_and(|(_, c)| c.is_alphanumeric()) {
            found.push(number);
        }
        at = end.max(at + 1);
    }
    found
}

/// A code standing alone within `text`: a number of 4 to 8 digits that is
/// not a date ([`numbers`]).
fn has_code_number(text: &str) -> bool {
    numbers(text).iter().any(Number::is_code)
}

/// Whether `number` in `all` is a code in the plainest sense: not a year,
/// a price, a decimal, or a number labelled as a room, a build, a flight
/// and the like ([`NUMBER_LABELS`]).
fn plain_code(all: &str, number: &Number) -> bool {
    let before = all[..number.start].trim_end_matches(' ');
    let after = all[number.end..].trim_start_matches(' ');
    let year = number.groups.len() == 1
        && number.digits == 4
        && all[number.start..number.end]
            .parse::<u32>()
            .is_ok_and(|year| (1900..=2099).contains(&year));
    let money = before.ends_with(MONEY)
        || after.starts_with(MONEY)
        || after.starts_with("vnd")
        || after.starts_with("usd");
    let decimal = number.groups.len() == 2 && number.groups[1] == 2 && number.splits[0] == '.';
    // A time of day after a word saying when ("the meeting at 1430"), as
    // a date is (the Reviewer-lead's P3 on #5070).
    let time = number.groups.len() == 1
        && number.digits == 4
        && all[number.start..number.end]
            .parse::<u32>()
            .is_ok_and(|hhmm| hhmm / 100 <= 23 && hhmm % 100 <= 59)
        && before
            .rsplit(|c: char| !c.is_alphanumeric())
            .next()
            .is_some_and(|word| TIME_WORDS.contains(&word));
    let labelled = before.ends_with('#')
        || before
            .rsplit(|c: char| !c.is_alphanumeric())
            .next()
            .is_some_and(|word| NUMBER_LABELS.contains(&word));
    number.is_code() && !year && !money && !decimal && !time && !labelled
}

/// Words a time of day follows ([`plain_code`]): "at 1430", "by 0900".
const TIME_WORDS: &[&str] = &[
    "at", "by", "from", "until", "till", "before", "after", "lúc", "luc",
];

/// Whether `between` keeps a weak code word and a code in one clause.
fn one_clause(between: &str) -> bool {
    !between.contains(CLAUSE_ENDS) && !between.contains(". ") && !between.ends_with('.')
}

/// A code-shaped token next to the weak code word at `at`: within
/// [`NEAR_WEAK`] bytes in the same clause, a plain code number
/// ([`plain_code`]) or a letters-and-digits word that changes between
/// letters and digits at least twice ("F7K2Q", not "VN123" or "10am").
fn weak_code_near(all: &str, at: usize, len: usize) -> bool {
    let from = at.saturating_sub(NEAR_WEAK);
    let to = at + len + NEAR_WEAK;
    let between = |start: usize, end: usize| {
        if end <= at {
            &all[end..at]
        } else if start >= at + len {
            &all[at + len..start]
        } else {
            ""
        }
    };
    let number = numbers(all).iter().any(|number| {
        number.end >= from
            && number.start <= to
            && one_clause(between(number.start, number.end))
            && plain_code(all, number)
    });
    let mixed = all
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .any(|word| {
            let start = word.as_ptr() as usize - all.as_ptr() as usize;
            let end = start + word.len();
            let changes = word
                .as_bytes()
                .windows(2)
                .filter(|pair| pair[0].is_ascii_digit() != pair[1].is_ascii_digit())
                .count();
            end >= from
                && start <= to
                && (4..=8).contains(&word.len())
                && word.chars().all(|c| c.is_ascii_alphanumeric())
                && changes >= 2
                && one_clause(between(start, end))
        });
    number || mixed
}

/// Whether a text holds something a secret could be: a code number, a
/// letters-and-digits code or a link. A security text without one may
/// have its secret beside it (`reads::without_security`).
pub fn has_secret_shape(text: &str) -> bool {
    let text = text.to_lowercase();
    has_code_number(&text)
        || has_mixed_code(&text)
        || LINK_STARTS.iter().any(|link| text.contains(link))
}

/// A code of letters and digits standing alone within `text`: a word of 4
/// to 8 ASCII letters and digits with both in it ("F7K2Q").
fn has_mixed_code(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|word| {
        (4..=8).contains(&word.len())
            && word.chars().all(|c| c.is_ascii_alphanumeric())
            && word.chars().any(|c| c.is_ascii_digit())
            && word.chars().any(|c| c.is_ascii_alphabetic())
    })
}

/// A link that signs in or verifies: a word of [`LINK_PATH_WORDS`] in its
/// path or query, or, with a word asking to follow it ([`FOLLOW_WORDS`]),
/// a long opaque value in its query.
fn has_login_link(all: &str) -> bool {
    let follow = FOLLOW_WORDS.iter().any(|word| all.contains(word));
    all.match_indices("http").any(|(at, _)| {
        let link = all[at..]
            .split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')'))
            .next()
            .unwrap_or("");
        let Some(rest) = link
            .strip_prefix("https://")
            .or_else(|| link.strip_prefix("http://"))
        else {
            return false;
        };
        let Some((_, path)) = rest.split_once(['/', '?']) else {
            return false;
        };
        // Whole hyphenated words and each of their parts
        // ("/verify-email?t=x", "/magic-link"): any value on such a path is
        // its secret, whatever its length.
        let words = path
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .flat_map(|word| std::iter::once(word).chain(word.split('-')))
            .any(|word| LINK_PATH_WORDS.contains(&word));
        let opaque = follow
            && path
                .split(['?', '&'])
                .filter_map(|pair| pair.split_once('='))
                .any(|(_, value)| {
                    value.len() >= OPAQUE_LEAST
                        && value.chars().all(|c| {
                            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '%')
                        })
                });
        words || opaque
    })
}

/// `all`'s text within [`NEAR`] bytes of `at`, on character boundaries.
fn around(all: &str, at: usize, len: usize) -> &str {
    let mut start = at.saturating_sub(NEAR);
    while !all.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (at + len + NEAR).min(all.len());
    while !all.is_char_boundary(end) {
        end += 1;
    }
    &all[start..end]
}

/// Whether a text is a security one: a one-time code, a password reset, a
/// sign-in alert or link, or a verification, in English or Vietnamese.
/// Such a text is never spoken, forwarded or kept: it is dropped from what
/// is returned and said only as [`SECURITY_LEFT_OUT`]. A text that says
/// "code" (or "mã", "token", "pin", "verification") within [`NEAR`] of a 4
/// to 8 digit number (not a date) or a letters-and-digits code counts too,
/// as does "enter", "confirm" or "verify" with a code-shaped token next to
/// it ([`weak_code_near`]); so does
/// one that says "sign in" or "log in" with a link or such a number
/// anywhere, and one with a sign-in or verification link
/// ([`has_login_link`]). Read composed (NFC). It errs on hiding: a discount
/// code or a postal code near a number is left out too.
pub fn is_security_mail(from: &str, subject: &str, text: &str) -> bool {
    use unicode_normalization::UnicodeNormalization;
    // Composed (NFC) first: Vietnamese written with combining accents
    // matches the phrases as written here.
    let all = format!("{from}\n{subject}\n{text}")
        .nfc()
        .collect::<String>()
        .to_lowercase();
    if SECURITY_PHRASES.iter().any(|phrase| all.contains(phrase))
        || SECURITY_WORDS.iter().any(|word| whole_word(&all, word))
    {
        return true;
    }
    let near_code = CODE_WORDS.iter().any(|word| {
        all.match_indices(word).any(|(at, _)| {
            // "code" also within "passcode" or "barcode"; the rest whole.
            let whole = *word == "code"
                || (!all[..at]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric)
                    && !all[at + word.len()..]
                        .chars()
                        .next()
                        .is_some_and(char::is_alphanumeric));
            let near = around(&all, at, word.len());
            whole && (has_code_number(near) || has_mixed_code(near))
        })
    }) || WEAK_CODE_WORDS.iter().any(|word| {
        all.match_indices(word).any(|(at, _)| {
            let whole = !all[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
                && !all[at + word.len()..]
                    .chars()
                    .next()
                    .is_some_and(char::is_alphanumeric);
            whole && weak_code_near(&all, at, word.len())
        })
    });
    let signs_in = SIGN_IN_WORDS.iter().any(|word| all.contains(word))
        && (LINK_STARTS.iter().any(|link| all.contains(link)) || has_code_number(&all));
    near_code || signs_in || has_login_link(&all)
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

/// Before a UID from All Mail in a mail's id over IMAP (`all41`); a bare
/// UID (`41`) is the inbox's. A UID names a mail only within its mailbox,
/// so the id carries which one, and `message` opens that one again.
pub const ALL_MAIL_ID: &str = "all";

/// A mail's id over IMAP: whether its UID is All Mail's, and the UID.
fn imap_id(id: &str) -> Option<(bool, u32)> {
    let (in_all, uid) = match id.strip_prefix(ALL_MAIL_ID) {
        Some(uid) => (true, uid),
        None => (false, id),
    };
    if uid.is_empty() || !uid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((in_all, uid.parse().ok()?))
}

/// Whether `id` is one an IMAP list gives.
pub fn imap_id_ok(id: &str) -> bool {
    imap_id(id).is_some()
}

/// An IMAP quoted string's text, from just after its opening quote, and
/// what follows its closing quote.
fn imap_unquoted(text: &str) -> Option<(String, &str)> {
    let mut out = String::new();
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?.1),
            '"' => return Some((out, &text[at + 1..])),
            c => out.push(c),
        }
    }
    None
}

/// The mailbox a `LIST` response names when it is flagged `\All` (RFC
/// 6154), as the server spells it: modified UTF-7, sent back unchanged,
/// so a localized name (`[Gmail]/T&HqU-t c&HqM- th&AbA-`) opens as
/// `[Gmail]/All Mail` does. The name may come quoted, as an atom or as a
/// literal.
fn all_mail_name(response: &Response) -> Option<String> {
    if !response.text.get(..7)?.eq_ignore_ascii_case("* LIST ") {
        return None;
    }
    let (flags, rest) = response.text[7..].strip_prefix('(')?.split_once(')')?;
    let flagged = |name: &str| {
        flags
            .split_whitespace()
            .any(|flag| flag.eq_ignore_ascii_case(name))
    };
    if !flagged("\\All") || flagged("\\Noselect") || flagged("\\NonExistent") {
        return None;
    }
    // The hierarchy delimiter: one quoted character, or NIL.
    let rest = rest.trim_start();
    let rest = match rest.strip_prefix('"') {
        Some(quoted) => imap_unquoted(quoted)?.1,
        None => rest
            .get(..3)
            .filter(|nil| nil.eq_ignore_ascii_case("NIL"))
            .map(|_| &rest[3..])?,
    };
    let name = rest.trim();
    let name = if name == "{}" {
        String::from_utf8(response.literals.last()?.1.clone()).ok()?
    } else if let Some(quoted) = name.strip_prefix('"') {
        let (name, after) = imap_unquoted(quoted)?;
        if !after.trim().is_empty() {
            return None;
        }
        name
    } else if !name.is_empty() && !name.contains([' ', '(', ')', '{', '"', '\\']) {
        name.to_owned()
    } else {
        return None;
    };
    // Only a name EXAMINE carries quoted (modified UTF-7 always is).
    imap_quoted(&name).is_some().then_some(name)
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

    /// LIST "" "*": the name of All Mail, the mailbox flagged `\All`, or
    /// none when no mailbox is (the account shows no All Mail over IMAP)
    /// or the LIST is refused.
    pub async fn all_mail(&mut self) -> Result<Option<String>, Failure> {
        match self.run("LIST \"\" \"*\"", None).await {
            Ok(listed) => Ok(listed.iter().find_map(all_mail_name)),
            Err(Failure::Refused) => Ok(None),
            Err(_) => Err(Failure::Unreached),
        }
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

/// A whole read: login, the mailbox opened read-only (`recent` the inbox;
/// `search` All Mail, else the inbox; `message` the one its id names), the
/// search or the one mail, logout.
pub async fn read_mail<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    user: &str,
    pass: &str,
    ask: &MailAsk,
) -> Result<Vec<MailItem>, Failure> {
    let mut imap = Imap::new(stream);
    imap.greet().await?;
    imap.login(user, pass).await?;
    let items = match ask {
        MailAsk::Recent(most) => {
            imap.examine("INBOX").await?;
            let uids = imap.search(None).await?;
            let newest = &uids[uids.len().saturating_sub(*most)..];
            imap.fetch(newest, PREVIEW_FETCH, PREVIEW_MOST).await?
        }
        MailAsk::Search(words, most) => {
            let all = imap.all_mail().await?;
            imap.examine(all.as_deref().unwrap_or("INBOX")).await?;
            let uids = imap.search(Some(words)).await?;
            let newest = &uids[uids.len().saturating_sub(*most)..];
            let mut items = imap.fetch(newest, PREVIEW_FETCH, PREVIEW_MOST).await?;
            if all.is_some() {
                for item in &mut items {
                    item.id = format!("{ALL_MAIL_ID}{}", item.id);
                }
            }
            items
        }
        MailAsk::Message(id) => {
            let (in_all, uid) = imap_id(id).ok_or(Failure::Unreached)?;
            let mailbox = if in_all {
                imap.all_mail().await?
            } else {
                Some("INBOX".to_owned())
            };
            match mailbox {
                Some(mailbox) => {
                    imap.examine(&mailbox).await?;
                    let mut items = imap.fetch(&[uid], MESSAGE_FETCH, TEXT_MOST).await?;
                    for item in &mut items {
                        item.id.clone_from(id);
                    }
                    items
                }
                // All Mail no longer shown over IMAP: no such mail here.
                None => Vec::new(),
            }
        }
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

// --- Gmail's REST API (a Google sign-in) --------------------------------------

/// Gmail's REST API, which only a Google sign-in's token goes to.
pub const API: &str = "https://gmail.googleapis.com";
/// The longest id of Gmail's own taken.
const API_ID_MOST: usize = 64;

/// Whether `id` is one of Gmail's own: letters and digits, bounded.
pub fn api_id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= API_ID_MOST && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// A URL under the account's own (`/gmail/v1/users/me/…`).
fn api_url(segments: &[&str], query: &[(&str, String)]) -> Result<String, String> {
    let mut all = vec!["gmail", "v1", "users", "me"];
    all.extend_from_slice(segments);
    reads::url(API, &all, query)
}

/// The account's profile (`users.getProfile`): `set`'s check of a sign-in.
pub fn api_profile_url() -> Result<String, String> {
    api_url(&["profile"], &[])
}

/// The ids of the inbox's newest mails, at most `most`, or of those Gmail's
/// own search `words` finds in all mail (`users.messages.list`): a search
/// names no label, as Gmail's own search, and Spam and Trash stay out
/// (`includeSpamTrash` is left at its default, false).
pub fn api_list_url(words: Option<&str>, most: usize) -> Result<String, String> {
    let most = ("maxResults", most.clamp(1, LIST_MOST).to_string());
    let query = match words {
        None => vec![("labelIds", "INBOX".to_owned()), most],
        Some(words) => vec![most, ("q", words.to_owned())],
    };
    api_url(&["messages"], &query)
}

/// One mail (`users.messages.get`): its From, Subject and Date with Gmail's
/// short preview, or with `whole` every part, its text read here.
pub fn api_message_url(id: &str, whole: bool) -> Result<String, String> {
    if !api_id_ok(id) {
        return Err("a mail's id is the one a list gave".into());
    }
    let query: Vec<(&str, String)> = if whole {
        vec![("format", "full".into())]
    } else {
        std::iter::once(("format", "metadata".to_owned()))
            .chain(["From", "Subject", "Date"].map(|name| ("metadataHeaders", name.to_owned())))
            .collect()
    };
    api_url(&["messages", id], &query)
}

/// Where one message is sent (`users.messages.send`).
pub fn api_send_url() -> Result<String, String> {
    api_url(&["messages", "send"], &[])
}

/// A list's ids, newest first as Gmail gives them, at most [`LIST_MOST`].
pub fn api_ids(text: &str) -> Vec<String> {
    let value: Value = serde_json::from_str(text).unwrap_or_default();
    value["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|listed| listed["id"].as_str())
        .filter(|id| api_id_ok(id))
        .take(LIST_MOST)
        .map(str::to_owned)
        .collect()
}

/// The account's address, from its profile.
pub fn api_address(text: &str) -> Option<String> {
    let value: Value = serde_json::from_str(text).ok()?;
    value["emailAddress"]
        .as_str()
        .filter(|address| address_ok(address))
        .map(str::to_owned)
}

/// A part's header fields, as the API lists them.
fn api_headers(part: &Value) -> Headers {
    Headers(
        part["headers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|field| {
                Some((
                    field["name"].as_str()?.to_ascii_lowercase(),
                    field["value"].as_str()?.to_owned(),
                ))
            })
            .collect(),
    )
}

/// One part's text, as [`part_text`] reads a MIME part: (plain or not, the
/// text), multiparts searched for a plain part first. The API gives each
/// part's body already decoded from its transfer encoding, in base64url.
fn api_part_text(part: &Value, depth: u8) -> Option<(bool, String)> {
    let kind = part["mimeType"].as_str().unwrap_or("").to_ascii_lowercase();
    if kind.starts_with("multipart/") {
        if depth >= 4 {
            return None;
        }
        let texts: Vec<(bool, String)> = part["parts"]
            .as_array()?
            .iter()
            .filter_map(|part| api_part_text(part, depth + 1))
            .collect();
        return texts
            .iter()
            .find(|(plain, _)| *plain)
            .or_else(|| texts.first())
            .cloned();
    }
    let data = part["body"]["data"].as_str()?;
    let bytes = URL_SAFE_NO_PAD.decode(data.trim_end_matches('=')).ok()?;
    let headers = api_headers(part);
    let charset = param(headers.get("content-type").unwrap_or(""), "charset").unwrap_or_default();
    let text = charset_text(&bytes, &charset);
    match kind.as_str() {
        "text/plain" => Some((true, text)),
        "text/html" => Some((false, strip_html(&text))),
        _ => None,
    }
}

/// One mail from `users.messages.get`'s answer: with `whole` its text (at
/// most [`TEXT_MOST`]), else Gmail's preview (at most [`PREVIEW_MOST`]).
pub fn api_item(text: &str, whole: bool) -> Option<MailItem> {
    let value: Value = serde_json::from_str(text).ok()?;
    let id = value["id"].as_str().filter(|id| api_id_ok(id))?;
    let payload = &value["payload"];
    let head = api_headers(payload);
    let field = |name| decode_words(head.get(name).unwrap_or(""));
    let text = if whole {
        let text = api_part_text(payload, 0).map(|(_, text)| text);
        tidy(&text.unwrap_or_default(), TEXT_MOST)
    } else {
        // The preview comes with its HTML entities.
        tidy(
            &strip_html(value["snippet"].as_str().unwrap_or("")),
            PREVIEW_MOST,
        )
    };
    Some(MailItem {
        id: id.to_owned(),
        from: field("from"),
        subject: field("subject"),
        date: field("date"),
        text,
    })
}

/// The send's body: the same message SMTP sends, in base64url as `raw`.
pub fn api_send_body(from: &str, out: &Outgoing, date: &str, id: &str) -> Value {
    json!({"raw": URL_SAFE_NO_PAD.encode(message(from, out, date, id))})
}

// --- Gmail over TLS -----------------------------------------------------------

/// The real Gmail: TLS to its IMAP and SMTP hosts only.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gmail;

async fn connect(
    host: &str,
    port: u16,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, Failure> {
    // The app password never goes to the REST host.
    if ![IMAP_HOST, SMTP_HOST].contains(&host) || !catalog::host_allowed("gmail", host) {
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
            // Vietnamese, with and without accents.
            ("x@y.z", "Xin chào", "Mã xác minh của bạn là 482913"),
            ("x@y.z", "", "Mã xác thực: 482913"),
            ("x@y.z", "", "Ma xac thuc: 482913"),
            ("x@y.z", "", "Mã OTP của bạn"),
            ("x@y.z", "", "Đây là mã đăng nhập"),
            ("x@y.z", "", "Bạn vừa đổi mật khẩu"),
            ("x@y.z", "", "Bam vao day de dat lai mat khau"),
            ("x@y.z", "", "Mã của bạn là 482 913"),
            // Digits split by a space or a dash.
            ("x@y.z", "Hello", "Your code is 482 913"),
            ("x@y.z", "Hello", "Your code is 482-913"),
            ("x@y.z", "Hello", "Your code: 4 8 2 9 1 3"),
            // Signing in with a number or a link.
            ("x@y.z", "Hi", "Use 482913 to sign in. Do not share."),
            (
                "x@y.z",
                "Notion",
                "Log in to Notion: https://notion.so/loginwithemail?t=abc",
            ),
            (
                "x@y.z",
                "Hi",
                "Click https://x.test/a?token=abc to continue",
            ),
            ("x@y.z", "Account", "Choose a new password here"),
            // Dots and a spaced dash between groups.
            ("x@y.z", "Hi", "Your code: 482 - 913"),
            ("x@y.z", "Hi", "Your code: 482.913"),
            // Letters and digits.
            ("x@y.z", "Steam", "Steam Guard code: F7K2Q"),
            ("x@y.z", "Hi", "Your access key is ready. Code: F7K2Q"),
            // Enter or confirm with a number.
            ("x@y.z", "Hi", "Enter 482913 to confirm"),
            // Four digits that could be a time, but not after a word
            // saying when: still a code.
            ("x@y.z", "Hi", "Your verification code is 1430"),
            ("x@y.z", "Order", "Your confirmation number: 482913"),
            // A sign-in link with no sign-in words and no token=.
            ("x@y.z", "Hi", "Open https://app.test/magic/abc to continue"),
            ("x@y.z", "Hi", "Go to https://x.test/auth/cb?c=1"),
            // A short value on a sign-in path, hyphenated too.
            ("x@y.z", "Hi", "Open https://x.test/magic?t=xyz"),
            ("x@y.z", "Hi", "Open https://x.test/verify-email?t=xyz"),
            ("x@y.z", "Hi", "Open https://x.test/en/signin?t=xyz"),
            // A weak word with a code next to it.
            ("x@y.z", "Hi", "Confirm with 4821 at the desk"),
            ("x@y.z", "Hi", "Enter A7B3C9 in the app"),
            (
                "x@y.z",
                "Hi",
                "Click this link: https://x.test/go?k=Zq8x0PjL3mW9vB2nT7yR4sKd",
            ),
            // Vietnamese with combining accents (NFD).
            ("x@y.z", "", "Ma\u{0303} xa\u{0301}c minh: 482913"),
            // A code far from the word "code".
            (
                "x@y.z",
                "Your code",
                "Hello! We are glad you are here, and we hope you enjoy every part of it. Here it is, just for you, from the whole team: 482913",
            ),
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
            ("x@y.z", "Shipping", "Shipping 12345678 left today"),
            ("x@y.z", "Trip", "Ma and I land at 1430 on Friday"),
            // "enter", "confirm" and "verify" beside no code.
            ("x@y.z", "Party", "Confirm attendance for 4 Nov 2026"),
            ("x@y.z", "Visit", "Enter via door B, 1600 Amphitheatre"),
            (
                "x@y.z",
                "Release",
                "Release v1.2.3 build 4521: please verify",
            ),
            ("x@y.z", "Order", "Confirm your order of $1299.00"),
            ("x@y.z", "Trip", "Flight VN123 confirmation"),
            ("x@y.z", "Event", "Please confirm by replying"),
            ("x@y.z", "Event", "Please confirm by 2026-11-04"),
            // A time of day after a word saying when.
            ("x@y.z", "Meeting", "Please confirm the meeting at 1430"),
            ("x@y.z", "Visit", "Enter the building at 0900 tomorrow"),
            ("x@y.z", "Họp", "Vui lòng xác nhận cuộc họp lúc 1430"),
            (
                "x@y.z",
                "Meeting",
                "Please verify the numbers. Room 1204 at noon",
            ),
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

    /// A UID FETCH's answer for one mail, tagged `tag`.
    fn fetched(tag: &str, uid: u32, from: &str, subject: &str, text: &str) -> String {
        let header = format!("From: {from}\r\nSubject: {subject}\r\nDate: Mon, 2 Nov 2026\r\n\r\n");
        format!(
            "* 1 FETCH (UID {uid} BODY[HEADER.FIELDS (FROM SUBJECT DATE CONTENT-TYPE CONTENT-TRANSFER-ENCODING)] {{{}}}\r\n{header} BODY[TEXT]<0> {{{}}}\r\n{text})\r\n{tag} OK done\r\n",
            header.len(),
            text.len()
        )
    }

    /// Gmail's LIST of a Vietnamese account: All Mail is `[Gmail]/Tất cả
    /// thư`, in modified UTF-7.
    const LIST_VI: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n\
        * LIST (\\HasChildren \\Noselect) \"/\" \"[Gmail]\"\r\n\
        * LIST (\\HasNoChildren \\Trash) \"/\" \"[Gmail]/Th&APk-ng r&AOE-c\"\r\n\
        * LIST (\\All \\HasNoChildren) \"/\" \"[Gmail]/T&HqU-t c&HqM- th&AbA-\"\r\n";

    /// One read against a script, and all the client sent.
    fn scripted_read(
        ask: MailAsk,
        replies: Vec<String>,
    ) -> (Result<Vec<MailItem>, Failure>, String) {
        let (client, server) = duplex(1 << 16);
        run(async {
            let server = tokio::spawn(scripted(server, "* OK Gimap ready\r\n", replies));
            let items = read_mail(client, "a@b.c", "pw", &ask).await;
            (items, server.await.unwrap())
        })
    }

    /// The owner's 9 Nov: an order number's search found the mail in the
    /// inbox but never an archived one, which Gmail's own search finds in
    /// All Mail. A search reads All Mail (found by its `\All` flag, its
    /// name localized), `recent` the inbox, and `message` the mailbox its
    /// id names.
    #[test]
    fn imap_search_reads_all_mail_and_recent_the_inbox() {
        let all = "\"[Gmail]/T&HqU-t c&HqM- th&AbA-\"";
        let (items, heard) = scripted_read(
            MailAsk::Search("4417".into(), 5),
            vec![
                "A1 OK\r\n".into(),
                format!("{LIST_VI}A2 OK listed\r\n"),
                "A3 OK [READ-ONLY] examined\r\n".into(),
                "* SEARCH 7\r\nA4 OK\r\n".into(),
                fetched("A5", 7, "Shop", "Order 4417 shipped", "On its way"),
                "A6 OK\r\n".into(),
            ],
        );
        let items = items.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "all7");
        assert_eq!(items[0].subject, "Order 4417 shipped");
        let lines: Vec<&str> = heard.lines().collect();
        assert_eq!(lines[1], "A2 LIST \"\" \"*\"");
        assert_eq!(lines[2], format!("A3 EXAMINE {all}"));
        assert_eq!(lines[3], "A4 UID SEARCH X-GM-RAW \"4417\"");
        assert!(lines[4].starts_with("A5 UID FETCH 7 (UID BODY.PEEK[HEADER.FIELDS"));
        // The search's id reads the same mail, in the same mailbox.
        let (items, heard) = scripted_read(
            MailAsk::Message("all7".into()),
            vec![
                "A1 OK\r\n".into(),
                format!("{LIST_VI}A2 OK listed\r\n"),
                "A3 OK [READ-ONLY] examined\r\n".into(),
                fetched("A4", 7, "Shop", "Order 4417 shipped", "On its way"),
                "A5 OK\r\n".into(),
            ],
        );
        let items = items.unwrap();
        assert_eq!(items[0].id, "all7");
        assert_eq!(items[0].text, "On its way");
        let lines: Vec<&str> = heard.lines().collect();
        assert_eq!(lines[1], "A2 LIST \"\" \"*\"");
        assert_eq!(lines[2], format!("A3 EXAMINE {all}"));
        assert!(lines[3].starts_with("A4 UID FETCH 7 ("));
        assert!(lines[3].contains("BODY.PEEK[TEXT]<0.65536>"));
        // `recent` stays the inbox, with no LIST.
        let (items, heard) = scripted_read(
            MailAsk::Recent(3),
            vec![
                "A1 OK\r\n".into(),
                "A2 OK [READ-ONLY] examined\r\n".into(),
                "* SEARCH 40 41\r\nA3 OK\r\n".into(),
                fetched("A4", 41, "Mom", "Dinner", "See you at 7"),
                "A5 OK\r\n".into(),
            ],
        );
        assert_eq!(items.unwrap()[0].id, "41");
        let lines: Vec<&str> = heard.lines().collect();
        assert_eq!(lines[1], "A2 EXAMINE \"INBOX\"");
        assert_eq!(lines[2], "A3 UID SEARCH ALL");
        assert!(!heard.contains("LIST"));
        // An inbox UID reads from the inbox, with no LIST.
        let (items, heard) = scripted_read(
            MailAsk::Message("41".into()),
            vec![
                "A1 OK\r\n".into(),
                "A2 OK [READ-ONLY] examined\r\n".into(),
                fetched("A3", 41, "Mom", "Dinner", "See you at 7"),
                "A4 OK\r\n".into(),
            ],
        );
        assert_eq!(items.unwrap()[0].id, "41");
        assert_eq!(heard.lines().nth(1), Some("A2 EXAMINE \"INBOX\""));
        assert!(!heard.contains("LIST"));
        // An All Mail id when All Mail is no longer shown: no such mail,
        // and no other mailbox's mail of that UID.
        let (items, heard) = scripted_read(
            MailAsk::Message("all7".into()),
            vec![
                "A1 OK\r\n".into(),
                "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\nA2 OK\r\n".into(),
                "A3 OK\r\n".into(),
            ],
        );
        assert_eq!(items.unwrap(), Vec::new());
        assert!(!heard.contains("EXAMINE") && !heard.contains("FETCH"));
    }

    #[test]
    fn all_mail_is_found_by_its_flag_however_its_name_comes() {
        let name = |text: &str, literal: Option<&str>| {
            all_mail_name(&Response {
                text: text.to_owned(),
                literals: literal
                    .map(|bytes| vec![("x".to_owned(), bytes.as_bytes().to_vec())])
                    .unwrap_or_default(),
            })
        };
        assert_eq!(
            name(
                "* LIST (\\All \\HasNoChildren) \"/\" \"[Gmail]/All Mail\"",
                None
            )
            .as_deref(),
            Some("[Gmail]/All Mail")
        );
        assert_eq!(
            name(
                "* list (\\HasNoChildren \\all) \"/\" \"[Gmail]/T&HqU-t c&HqM- th&AbA-\"",
                None
            )
            .as_deref(),
            Some("[Gmail]/T&HqU-t c&HqM- th&AbA-")
        );
        // Quoted with escapes, an atom, a literal, a NIL delimiter.
        assert_eq!(
            name("* LIST (\\All) \"\\\\\" \"Mail \\\"all\\\"\"", None).as_deref(),
            Some("Mail \"all\"")
        );
        assert_eq!(
            name("* LIST (\\All) NIL AllMail", None).as_deref(),
            Some("AllMail")
        );
        assert_eq!(
            name("* LIST (\\All) \"/\" {}", Some("[Gmail]/Alle Nachrichten")).as_deref(),
            Some("[Gmail]/Alle Nachrichten")
        );
        // Not All Mail, not selectable, or a name EXAMINE can't carry.
        assert_eq!(name("* LIST (\\HasNoChildren) \"/\" \"INBOX\"", None), None);
        assert_eq!(
            name("* LIST (\\All \\Noselect) \"/\" \"[Gmail]/All\"", None),
            None
        );
        assert_eq!(
            name("* LIST (\\All) \"/\" {}", Some("All\r\nA9 DELETE INBOX")),
            None
        );
        assert_eq!(name("* LIST (\\All) \"/\" \"unclosed", None), None);
        assert_eq!(
            name("* LSUB (\\All) \"/\" \"[Gmail]/All Mail\"", None),
            None
        );
    }

    #[test]
    fn imap_reads_read_only_and_never_marks_a_mail() {
        let (client, server) = duplex(1 << 16);
        let header =
            "From: Mom <mom@home.test>\r\nSubject: Dinner\r\nDate: Mon, 2 Nov 2026\r\n\r\n";
        let text = "See you at 7";
        let fetch = format!(
            "* 1 FETCH (UID 41 BODY[HEADER.FIELDS (FROM SUBJECT DATE CONTENT-TYPE CONTENT-TRANSFER-ENCODING)] {{{}}}\r\n{header} BODY[TEXT]<0> {{{}}}\r\n{text})\r\nA5 OK done\r\n",
            header.len(),
            text.len()
        );
        // No mailbox flagged `\All` (All Mail hidden from IMAP): a search
        // reads the inbox, as before.
        let replies = vec![
            "A1 OK logged in\r\n".to_owned(),
            "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Sent Mail\"\r\nA2 OK\r\n".to_owned(),
            "* 3 EXISTS\r\nA3 OK [READ-ONLY] examined\r\n".to_owned(),
            "* SEARCH 41\r\nA4 OK\r\n".to_owned(),
            fetch,
            "* BYE\r\nA6 OK\r\n".to_owned(),
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
        assert_eq!(lines[1], "A2 LIST \"\" \"*\"");
        assert_eq!(lines[2], "A3 EXAMINE \"INBOX\"");
        assert_eq!(lines[3], "A4 UID SEARCH X-GM-RAW \"from:mom\"");
        assert!(lines[4].starts_with("A5 UID FETCH 41 (UID BODY.PEEK[HEADER.FIELDS"));
        assert!(lines[4].contains("BODY.PEEK[TEXT]<0.4096>"));
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
