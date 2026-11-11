//! What the lamp may do with each service: the hosts its token may go to
//! (V1's `OFFICIAL_HOSTS`, `skills/connectors/scripts/connector.py`), the
//! read calls it may make (method and path), and the write calls a spoken
//! "send" may make. GraphQL services read with queries only.

/// An HTTP method the lamp uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

/// One service the lamp knows.
#[derive(Debug)]
pub struct Service {
    /// V1's connector code, the store's file name and the tools' `service`.
    pub code: &'static str,
    /// The name a person hears.
    pub name: &'static str,
    /// The hosts its token may be sent to: one of these or a subdomain.
    pub hosts: &'static [&'static str],
    /// The base of its HTTP API; `None` for an MCP service. Gmail's is for
    /// a Google sign-in only: an app password reads over IMAP.
    pub api: Option<&'static str>,
    /// Its reads are GraphQL queries (`POST /graphql`), never a mutation.
    pub graphql: bool,
    /// The read calls allowed: a method and a path pattern, where `*` is
    /// one segment and a last `**` one or more.
    pub reads: &'static [(Method, &'static str)],
    /// The write calls a confirmed write may make.
    pub writes: &'static [(Method, &'static str)],
    /// For an MCP service, its endpoint (Streamable HTTP) when the link
    /// names none; its token then goes to `hosts` exactly (no subdomain),
    /// and its reads and writes are tools sorted by `mcp.rs`.
    pub mcp: Option<&'static str>,
}

use Method::{Delete, Get, Patch, Post};

/// Figma's reads, under either of its codes.
const FIGMA_READS: &[(Method, &str)] = &[
    (Get, "/v1/me"),
    (Get, "/v1/files/*"),
    (Get, "/v1/files/*/comments"),
    (Get, "/v1/teams/*/projects"),
    (Get, "/v1/projects/*/files"),
];
/// Figma's one write, a comment on a file.
const FIGMA_WRITES: &[(Method, &str)] = &[(Post, "/v1/files/*/comments")];

/// Every service, in the order a list says them. A new token service is
/// one more entry here, with its reads in `reads.rs` and any write in
/// `writes.rs`; a new MCP service one more entry with `mcp`, and its tool
/// names in `mcp.rs`.
pub const SERVICES: &[Service] = &[
    Service {
        code: "gmail",
        name: "Gmail",
        // An app password goes over IMAP and SMTP only; a Google sign-in
        // (OAuth) over Gmail's REST API only, which refuses an app password.
        // `serve.rs` and `mail.rs` keep each to its own hosts.
        hosts: &["imap.gmail.com", "smtp.gmail.com", "gmail.googleapis.com"],
        api: Some("https://gmail.googleapis.com"),
        graphql: false,
        reads: &[
            (Get, "/gmail/v1/users/me/profile"),
            (Get, "/gmail/v1/users/me/messages"),
            (Get, "/gmail/v1/users/me/messages/*"),
            // One attachment's data (`users.messages.attachments.get`).
            (Get, "/gmail/v1/users/me/messages/*/attachments/*"),
        ],
        writes: &[(Post, "/gmail/v1/users/me/messages/send")],
        mcp: None,
    },
    // A Lamp 2's Google sign-in for Gmail (Queue #4710): send only, its
    // scopes the account's identity and `gmail.send`, so it reads nothing.
    // Renewed through the backend under this code, as Google Calendar is.
    Service {
        code: "gmail-app",
        name: "Gmail with Google sign-in",
        hosts: &["gmail.googleapis.com"],
        api: Some("https://gmail.googleapis.com"),
        graphql: false,
        reads: &[],
        writes: &[(Post, "/gmail/v1/users/me/messages/send")],
        mcp: None,
    },
    Service {
        code: "google_calendar",
        name: "Google Calendar",
        hosts: &["www.googleapis.com"],
        api: Some("https://www.googleapis.com"),
        graphql: false,
        // No `/users/me/calendarList`: the link holds only
        // `calendar.events`, which can't list calendars (#5385).
        reads: &[
            (Get, "/calendar/v3/calendars/*/events"),
            (Get, "/calendar/v3/calendars/*/events/*"),
        ],
        // Changing and cancelling one event by its id with
        // `LAMP_CALENDAR_EDITS` (`writes::prepare_edit`).
        writes: &[
            (Post, "/calendar/v3/calendars/*/events"),
            (Patch, "/calendar/v3/calendars/*/events/*"),
            (Delete, "/calendar/v3/calendars/*/events/*"),
        ],
        mcp: None,
    },
    Service {
        code: "google_drive",
        name: "Google Drive",
        hosts: &["www.googleapis.com"],
        api: Some("https://www.googleapis.com"),
        graphql: false,
        reads: &[
            (Get, "/drive/v3/about"),
            (Get, "/drive/v3/files"),
            (Get, "/drive/v3/files/*"),
            (Get, "/drive/v3/files/*/export"),
        ],
        // One new file in My Drive, uploaded whole (`writes::DRIVE_UPLOAD`):
        // never an update, a copy, a share or a delete.
        writes: &[(Post, "/upload/drive/v3/files")],
        mcp: None,
    },
    Service {
        code: "figma",
        name: "Figma",
        hosts: &["api.figma.com"],
        api: Some("https://api.figma.com"),
        graphql: false,
        reads: FIGMA_READS,
        writes: FIGMA_WRITES,
        mcp: None,
    },
    // The backend's own code for Figma; V1's table has both.
    Service {
        code: "figma-api",
        name: "Figma",
        hosts: &["api.figma.com"],
        api: Some("https://api.figma.com"),
        graphql: false,
        reads: FIGMA_READS,
        writes: FIGMA_WRITES,
        mcp: None,
    },
    Service {
        code: "github",
        name: "GitHub",
        hosts: &["api.github.com"],
        api: Some("https://api.github.com"),
        graphql: true,
        reads: &[
            (Post, "/graphql"),
            (Get, "/user"),
            (Get, "/user/repos"),
            (Get, "/notifications"),
            (Get, "/issues"),
            (Get, "/search/issues"),
            (Get, "/search/repositories"),
            (Get, "/repos/*/*"),
            (Get, "/repos/*/*/issues"),
            (Get, "/repos/*/*/issues/*"),
            (Get, "/repos/*/*/issues/*/comments"),
            (Get, "/repos/*/*/pulls"),
            (Get, "/repos/*/*/pulls/*"),
        ],
        writes: &[
            (Post, "/repos/*/*/issues"),
            (Post, "/repos/*/*/issues/*/comments"),
        ],
        mcp: None,
    },
    Service {
        code: "linear",
        name: "Linear",
        hosts: &["api.linear.app"],
        api: Some("https://api.linear.app"),
        graphql: true,
        reads: &[(Post, "/graphql")],
        writes: &[(Post, "/graphql")],
        mcp: None,
    },
    Service {
        code: "asana",
        name: "Asana",
        hosts: &["app.asana.com"],
        api: Some("https://app.asana.com"),
        graphql: false,
        reads: &[
            (Get, "/api/1.0/users/me"),
            (Get, "/api/1.0/workspaces"),
            (Get, "/api/1.0/projects"),
            (Get, "/api/1.0/projects/*"),
            (Get, "/api/1.0/projects/*/tasks"),
            (Get, "/api/1.0/tasks"),
            (Get, "/api/1.0/tasks/*"),
        ],
        writes: &[(Post, "/api/1.0/tasks")],
        mcp: None,
    },
    Service {
        code: "ahrefs",
        name: "Ahrefs",
        hosts: &["api.ahrefs.com"],
        api: Some("https://api.ahrefs.com"),
        graphql: false,
        // Ahrefs' API v3 reads with GET only.
        reads: &[(Get, "/v3/**")],
        writes: &[],
        mcp: None,
    },
    Service {
        code: "facebook",
        name: "Facebook Page",
        hosts: &["graph.facebook.com"],
        api: Some("https://graph.facebook.com"),
        graphql: false,
        // The first segment is the Graph API's version.
        reads: &[
            (Get, "/*/me"),
            (Get, "/*/me/accounts"),
            (Get, "/*/*"),
            (Get, "/*/*/posts"),
            (Get, "/*/*/feed"),
            (Get, "/*/*/comments"),
            (Get, "/*/*/insights"),
        ],
        writes: &[(Post, "/*/*/feed")],
        mcp: None,
    },
    // V1's MCP connectors. V1 takes each endpoint from the backend's
    // `mcp_url` and has no table of them; the lamp pins each to the
    // service's own hosted MCP server, and takes a fetched `mcp_url` only
    // on one of these hosts.
    Service {
        code: "slack",
        name: "Slack",
        hosts: &["mcp.slack.com"],
        api: None,
        graphql: false,
        reads: &[],
        writes: &[],
        mcp: Some("https://mcp.slack.com/mcp"),
    },
    Service {
        code: "mondaydotcom",
        name: "monday.com",
        hosts: &["mcp.monday.com"],
        api: None,
        graphql: false,
        reads: &[],
        writes: &[],
        mcp: Some("https://mcp.monday.com/mcp"),
    },
    Service {
        code: "hubspot",
        name: "HubSpot",
        hosts: &["mcp.hubspot.com"],
        api: None,
        graphql: false,
        reads: &[],
        writes: &[],
        mcp: Some("https://mcp.hubspot.com/"),
    },
    Service {
        code: "amplitude",
        name: "Amplitude",
        // Its US and EU data centres.
        hosts: &["mcp.amplitude.com", "mcp.eu.amplitude.com"],
        api: None,
        graphql: false,
        reads: &[],
        writes: &[],
        mcp: Some("https://mcp.amplitude.com/mcp"),
    },
    // The app's Notion sign-in is Notion's MCP OAuth, and its token is good
    // only at Notion's hosted MCP server: api.notion.com refused it (401) on
    // every read on lamp-9adf, 4 Nov 22:19-22:21.
    Service {
        code: "notion",
        name: "Notion",
        hosts: &["mcp.notion.com"],
        api: None,
        graphql: false,
        reads: &[],
        writes: &[],
        mcp: Some("https://mcp.notion.com/mcp"),
    },
];

/// The Graph API version the lamp asks Facebook with.
pub const FACEBOOK_VERSION: &str = "v19.0";

/// The service a code names.
pub fn service(code: &str) -> Option<&'static Service> {
    SERVICES.iter().find(|service| service.code == code)
}

/// Whether `code`'s token may go to `host`: one of its hosts or a
/// subdomain of one (V1's `host_allowed`); for an MCP service one of its
/// hosts exactly.
pub fn host_allowed(code: &str, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    service(code).is_some_and(|service| {
        service.hosts.iter().any(|allowed| {
            host == *allowed
                || (service.mcp.is_none()
                    && host
                        .strip_suffix(allowed)
                        .is_some_and(|below| below.ends_with('.') && below.len() > 1))
        })
    })
}

/// Whether a path is plain: absolute, no `..`, no empty segment, no
/// encoded slash or backslash that a server might read as another path.
pub fn plain_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    path.starts_with('/')
        && !path.contains("//")
        && !path.contains('\\')
        && !lower.contains("%2f")
        && !lower.contains("%5c")
        && !lower.contains("%2e")
        && path
            .split('/')
            .all(|segment| segment != ".." && segment != ".")
}

/// Whether `path` fits `pattern`: segment by segment, `*` any one, a last
/// `**` one or more.
pub fn path_matches(pattern: &str, path: &str) -> bool {
    if !plain_path(path) {
        return false;
    }
    let path = path.strip_suffix('/').unwrap_or(path);
    let mut wanted = pattern.split('/');
    let mut given = path.split('/');
    loop {
        match (wanted.next(), given.next()) {
            (None, None) => return true,
            (Some("**"), Some(segment)) => return !segment.is_empty(),
            (Some("*"), Some(segment)) => {
                if segment.is_empty() {
                    return false;
                }
            }
            (Some(want), Some(segment)) => {
                if want != segment {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Whether a read may be made: its method and path on the service's
/// allow-list, and for GraphQL a query, never a mutation or subscription.
pub fn read_allowed(
    code: &str,
    method: Method,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<(), String> {
    let service = service(code).ok_or("the lamp doesn't know that service")?;
    if !service
        .reads
        .iter()
        .any(|(allowed, pattern)| *allowed == method && path_matches(pattern, path))
    {
        return Err(format!("the lamp doesn't read that from {}", service.name));
    }
    if service.graphql && path == "/graphql" {
        let query = body
            .and_then(|body| body.get("query"))
            .and_then(serde_json::Value::as_str)
            .ok_or("a GraphQL read needs its query")?;
        graphql_read_only(query)?;
    }
    Ok(())
}

/// Whether a confirmed write may make this call.
pub fn write_allowed(code: &str, method: Method, path: &str) -> bool {
    service(code).is_some_and(|service| {
        service
            .writes
            .iter()
            .any(|(allowed, pattern)| *allowed == method && path_matches(pattern, path))
    })
}

/// A GraphQL document that only reads: every operation a query (named or
/// the `{ … }` shorthand), none a `mutation` or `subscription`. Strings and
/// comments are skipped; only names at the top level (outside every brace)
/// are operation keywords, so a field called `mutation` is not one.
pub fn graphql_read_only(document: &str) -> Result<(), &'static str> {
    let bytes = document.as_bytes();
    let mut depth = 0usize;
    let mut operations = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        match byte {
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' if bytes[i..].starts_with(b"\"\"\"") => {
                i += 3;
                while i < bytes.len() && !bytes[i..].starts_with(b"\"\"\"") {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 2;
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'{' => {
                if depth == 0 {
                    operations += 1;
                }
                depth += 1;
            }
            b'}' => depth = depth.checked_sub(1).ok_or("the query is not well formed")?,
            b if b.is_ascii_alphabetic() || b == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &document[start..i];
                if depth == 0 && (word == "mutation" || word == "subscription") {
                    return Err("the lamp only reads: a GraphQL mutation is refused");
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if depth != 0 || operations == 0 {
        return Err("the query is not well formed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_token_goes_only_to_its_own_hosts() {
        assert!(host_allowed("github", "api.github.com"));
        assert!(host_allowed("google_drive", "www.googleapis.com"));
        assert!(host_allowed("google_calendar", "WWW.GoogleAPIs.com"));
        assert!(host_allowed("gmail", "imap.gmail.com"));
        assert!(host_allowed("gmail", "gmail.googleapis.com"));
        for (code, host) in [
            ("github", "api.github.com.evil.test"),
            ("github", "evilapi.github.com"),
            ("github", "github.com"),
            ("github", "api.linear.app"),
            ("notion", "notion.so"),
            ("gmail", "www.googleapis.com"),
            ("gmail", "gmail.com"),
            ("facebook", "facebook.com"),
            ("google_drive", "googleapis.com.test"),
            ("nobody", "api.github.com"),
            ("github", ".api.github.com.x"),
        ] {
            assert!(!host_allowed(code, host), "{code} to {host}");
        }
        // Every service's hosts are V1's, but Gmail's (IMAP and SMTP for an
        // app password, its own REST host for a Google sign-in).
        for service in SERVICES {
            assert!(!service.hosts.is_empty(), "{}", service.code);
            if let Some(api) = service.api {
                let host = api.strip_prefix("https://").unwrap();
                assert!(host_allowed(service.code, host), "{}", service.code);
            }
            // An MCP service's endpoint is on its own pinned host.
            if let Some(mcp) = service.mcp {
                let (host, _) = crate::net::split(mcp).unwrap();
                assert!(host_allowed(service.code, &host), "{}", service.code);
                assert!(service.reads.is_empty() && service.writes.is_empty());
            }
        }
        // An MCP host is pinned exactly: no subdomain, no other service's.
        assert!(host_allowed("amplitude", "mcp.eu.amplitude.com"));
        for (code, host) in [
            ("slack", "evil.mcp.slack.com"),
            ("slack", "slack.com"),
            ("slack", "mcp.monday.com"),
            ("hubspot", "api.hubapi.com"),
            ("amplitude", "amplitude.com"),
        ] {
            assert!(!host_allowed(code, host), "{code} to {host}");
        }
    }

    #[test]
    fn notion_is_read_and_written_through_its_own_mcp_server() {
        // The app's Notion sign-in gives an MCP token api.notion.com refuses.
        let notion = service("notion").unwrap();
        assert_eq!(notion.mcp, Some("https://mcp.notion.com/mcp"));
        assert!(notion.api.is_none() && notion.reads.is_empty() && notion.writes.is_empty());
        assert!(host_allowed("notion", "mcp.notion.com"));
        for host in [
            "api.notion.com",
            "notion.so",
            "x.mcp.notion.com",
            "notion.com",
        ] {
            assert!(!host_allowed("notion", host), "notion to {host}");
        }
    }

    #[test]
    fn paths_match_segment_by_segment() {
        assert!(path_matches("/repos/*/*/issues", "/repos/a/b/issues"));
        assert!(path_matches("/repos/*/*/issues", "/repos/a/b/issues/"));
        assert!(!path_matches("/repos/*/*/issues", "/repos/a/issues"));
        assert!(!path_matches("/repos/*/*/issues", "/repos/a/b/issues/1"));
        assert!(!path_matches("/repos/*/*/issues", "/repos/a//issues"));
        assert!(!path_matches("/repos/*/*/issues", "/repos/a/../issues"));
        assert!(!path_matches("/repos/*/*/issues", "/repos/a%2Fb/c/issues"));
        assert!(path_matches("/v3/**", "/v3/site-explorer/overview"));
        assert!(!path_matches("/v3/**", "/v3"));
        assert!(!path_matches("/v3/**", "/v4/x"));
    }

    #[test]
    fn each_service_reads_only_its_allow_list() {
        let ok = |code, method, path| read_allowed(code, method, path, None).is_ok();
        assert!(ok(
            "google_calendar",
            Get,
            "/calendar/v3/calendars/primary/events"
        ));
        assert!(!ok(
            "google_calendar",
            Post,
            "/calendar/v3/calendars/primary/events"
        ));
        assert!(!ok("google_calendar", Get, "/drive/v3/files"));
        assert!(!ok(
            "google_calendar",
            Get,
            "/calendar/v3/users/me/calendarList"
        ));
        assert!(ok("google_drive", Get, "/drive/v3/files/abc/export"));
        assert!(!ok("google_drive", Method::Patch, "/drive/v3/files/abc"));
        assert!(ok("figma", Get, "/v1/files/key/comments"));
        assert!(ok("figma-api", Get, "/v1/files/key/comments"));
        assert!(host_allowed("figma-api", "api.figma.com"));
        assert!(!ok("figma", Post, "/v1/files/key/comments"));
        assert!(ok("github", Get, "/repos/o/r/pulls/3"));
        assert!(!ok("github", Post, "/repos/o/r/issues"));
        assert!(!ok("github", Get, "/user/keys"));
        assert!(ok("asana", Get, "/api/1.0/tasks/12"));
        assert!(!ok("asana", Post, "/api/1.0/tasks"));
        assert!(ok("ahrefs", Get, "/v3/site-explorer/overview"));
        assert!(!ok("ahrefs", Post, "/v3/site-explorer/overview"));
        assert!(ok("facebook", Get, "/v19.0/123/posts"));
        assert!(!ok("facebook", Post, "/v19.0/123/feed"));
        assert!(ok("gmail", Get, "/gmail/v1/users/me/messages"));
        assert!(ok(
            "gmail",
            Get,
            "/gmail/v1/users/me/messages/18c2f3a4b5d6e7f8"
        ));
        assert!(!ok("gmail", Post, "/gmail/v1/users/me/messages/send"));
        assert!(!ok("gmail", Get, "/gmail/v1/users/me/settings/filters"));
        // An attachment's data; never deeper, never a write.
        assert!(ok(
            "gmail",
            Get,
            "/gmail/v1/users/me/messages/18c2/attachments/ANGj-x_1"
        ));
        assert!(!ok(
            "gmail",
            Get,
            "/gmail/v1/users/me/messages/18c2/attachments/a/b"
        ));
        assert!(!ok(
            "gmail",
            Post,
            "/gmail/v1/users/me/messages/18c2/attachments/a"
        ));
        assert!(write_allowed(
            "gmail",
            Post,
            "/gmail/v1/users/me/messages/send"
        ));
        assert!(!write_allowed(
            "gmail",
            Post,
            "/gmail/v1/users/me/messages/1/trash"
        ));
        // A Google sign-in for sending only: its send, no read, one host.
        assert!(write_allowed(
            "gmail-app",
            Post,
            "/gmail/v1/users/me/messages/send"
        ));
        assert!(!ok("gmail-app", Get, "/gmail/v1/users/me/messages"));
        assert!(!ok("gmail-app", Get, "/gmail/v1/users/me/profile"));
        assert!(host_allowed("gmail-app", "gmail.googleapis.com"));
        for host in ["imap.gmail.com", "smtp.gmail.com", "www.googleapis.com"] {
            assert!(!host_allowed("gmail-app", host), "{host}");
        }
        assert!(!ok("linear", Get, "/graphql"));
        // Writes have their own list, which reads never use.
        assert!(write_allowed(
            "github",
            Post,
            "/repos/o/r/issues/4/comments"
        ));
        assert!(!write_allowed("github", Post, "/user/repos"));
        assert!(!write_allowed("google_drive", Post, "/drive/v3/files"));
        // Drive's one write is a new file's upload: never an update, a
        // copy, a permission (a share), a delete or anything under it.
        assert!(write_allowed(
            "google_drive",
            Post,
            "/upload/drive/v3/files"
        ));
        for (method, path) in [
            (Method::Patch, "/upload/drive/v3/files/abc"),
            (Post, "/upload/drive/v3/files/abc"),
            (Method::Patch, "/drive/v3/files/abc"),
            (Post, "/drive/v3/files/abc/copy"),
            (Post, "/drive/v3/files/abc/permissions"),
            (Post, "/drive/v3/files/abc/comments"),
            (Post, "/upload/drive/v2/files"),
            (Post, "/upload/drive/v3/files/../files"),
            (Get, "/upload/drive/v3/files"),
        ] {
            assert!(!write_allowed("google_drive", method, path), "{path}");
        }
        assert!(!ok("google_drive", Post, "/upload/drive/v3/files"));
        assert!(!write_allowed(
            "google_calendar",
            Post,
            "/upload/drive/v3/files"
        ));
    }

    #[test]
    fn graphql_reads_refuse_every_mutation() {
        let read = |query: &str| {
            read_allowed("github", Post, "/graphql", Some(&json!({"query": query}))).is_ok()
        };
        assert!(read("{ viewer { login } }"));
        assert!(read("query Me { viewer { login } }"));
        assert!(read(
            "query($n: Int) { viewer { repositories(first: $n) { totalCount } } }"
        ));
        // A field or a string named mutation is not an operation.
        assert!(read("{ mutation: viewer { login } }"));
        assert!(read("{ search(query: \"mutation {\") { issueCount } }"));
        assert!(read("# mutation\n{ viewer { login } }"));
        for refused in [
            "mutation { addStar(input: {starrableId: \"x\"}) { clientMutationId } }",
            "  mutation M { deleteIssue(input: {}) { x } }",
            "query A { viewer { login } } mutation B { x }",
            "subscription { x }",
            "{ viewer { login } } # } mutation\nmutation { x }",
            "\"\"\"doc\"\"\" mutation { x }",
            "",
            "{ unclosed",
            "}{",
        ] {
            assert!(!read(refused), "{refused}");
        }
        assert!(
            read_allowed(
                "linear",
                Post,
                "/graphql",
                Some(&json!({"query": "mutation { x }"}))
            )
            .is_err()
        );
        assert!(read_allowed("linear", Post, "/graphql", None).is_err());
    }
}
