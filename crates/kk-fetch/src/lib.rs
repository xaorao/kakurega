//! Fetching a web page is an egress path, so it has one door. Every request passes the same checks:
//! https only, no credentials in the URL, a fixed port, no credential-named query parameters, an address that is public
//! (checked after name resolution, for every address the name resolves to), a connection pinned to the checked address
//! so a second DNS answer cannot redirect it, every redirect checked again from the start, a size cap and a text-only
//! content type. Cookies and authorisation headers are never sent.
//!
//! The address and redirect rules follow the behaviour documented in Hermes Agent's `tools/url_safety.py` (MIT);
//! the implementation is written for this project. See NOTICE.md.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
pub use url::Url;

pub const MAX_URL: usize = 2048;
pub const MAX_REDIRECTS: usize = 4;
pub const MAX_BODY: usize = 1 << 20;
pub const MAX_TEXT_CHARS: usize = 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    BadUrl(&'static str),
    BlockedAddress,
    NoAddress,
    TooManyRedirects,
    Status(u16),
    NotText,
    Transport(String),
}

// ---- addresses ----

fn v4_public(a: Ipv4Addr) -> bool {
    let o = a.octets();
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_private()
        || a.is_link_local()
        || a.is_broadcast()
        || a.is_multicast()
        || o[0] == 0
        || (o[0] == 168 && o[1] == 63 && o[2] == 129 && o[3] == 16) // Azure wire server, a public-range address every Azure VM can reach
        || (o[0] == 100 && (64..128).contains(&o[1])) // carrier-grade NAT, includes 100.100.100.200
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF protocol assignments
        || (o[0] == 192 && o[1] == 0 && o[2] == 2) // documentation
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // benchmarking
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113)
        || o[0] >= 240) // reserved
}

pub fn ip_allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => v4_public(a),
        IpAddr::V6(a) => {
            if let Some(m) = a.to_ipv4_mapped() {
                return v4_public(m);
            }
            let s = a.segments();
            // forms that wrap an IPv4 address: NAT64 (64:ff9b::/96), IPv4-translated (::ffff:0:a.b.c.d), 6to4 (2002::/16)
            if s[0] == 0x0064 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
                return v4_public(Ipv4Addr::new(
                    (s[6] >> 8) as u8,
                    s[6] as u8,
                    (s[7] >> 8) as u8,
                    s[7] as u8,
                ));
            }
            if s[..4] == [0, 0, 0, 0] && s[4] == 0xffff && s[5] == 0 {
                return v4_public(Ipv4Addr::new(
                    (s[6] >> 8) as u8,
                    s[6] as u8,
                    (s[7] >> 8) as u8,
                    s[7] as u8,
                ));
            }
            if s[0] == 0x2002 {
                return v4_public(Ipv4Addr::new(
                    (s[1] >> 8) as u8,
                    s[1] as u8,
                    (s[2] >> 8) as u8,
                    s[2] as u8,
                ));
            }
            !(a.is_unspecified()
                || a.is_loopback()
                || a.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link local
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || (s[0] == 0x2001 && s[1] == 0) // Teredo
                || (s[0] & 0xffc0) == 0xfec0) // site local (deprecated)
        }
    }
}

// ---- URLs ----

const CREDENTIAL_QUERY_NAMES: &[&str] = &[
    "access_token",
    "api_key",
    "apikey",
    "auth_token",
    "authorization",
    "awsaccesskeyid",
    "client_secret",
    "credential",
    "credentials",
    "jwt",
    "password",
    "passwd",
    "secret",
    "session_id",
    "signature",
    "token",
    "x-amz-security-token",
    "x-amz-signature",
];

/// Checks everything about a URL that does not need the network.
pub fn validate_url(raw: &str) -> Result<Url, FetchError> {
    if raw.len() > MAX_URL || raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(FetchError::BadUrl(
            "the address is too long or contains spaces or control characters",
        ));
    }
    let u = Url::parse(raw).map_err(|_| FetchError::BadUrl("not a valid address"))?;
    if u.scheme() != "https" {
        return Err(FetchError::BadUrl("only https addresses are fetched"));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err(FetchError::BadUrl(
            "an address with a user name or password is never fetched",
        ));
    }
    if u.port().is_some_and(|p| p != 443) {
        return Err(FetchError::BadUrl("only the standard https port is used"));
    }
    // a trailing dot names the same host ("localhost." is localhost), so it must not slip past the name checks
    let host = u
        .host_str()
        .ok_or(FetchError::BadUrl("no host"))?
        .to_ascii_lowercase()
        .trim_end_matches('.')
        .to_string();
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".lan")
        || host.ends_with(".home.arpa")
        || host == "metadata.google.internal"
        || host == "metadata.goog"
    {
        return Err(FetchError::BadUrl("that is a local or internal name"));
    }
    if let Ok(ip) = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        if !ip_allowed(ip) {
            return Err(FetchError::BlockedAddress);
        }
    }
    if u.query_pairs().any(|(k, v)| {
        !v.is_empty() && CREDENTIAL_QUERY_NAMES.contains(&k.to_ascii_lowercase().as_str())
    }) {
        return Err(FetchError::BadUrl(
            "the address carries a credential-named parameter",
        ));
    }
    Ok(u)
}

// ---- the fetcher ----

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub location: Option<String>,
    pub content_type: String,
    pub body: Vec<u8>,
}

pub trait Dns {
    fn resolve(&self, host: &str) -> Vec<IpAddr>;
}

pub trait Transport {
    /// One GET with no cookies and no credentials, connecting only to `pinned`, reading at most `max_body` bytes.
    fn get(
        &self,
        url: &Url,
        pinned: &[SocketAddr],
        max_body: usize,
    ) -> Result<Response, FetchError>;

    /// Like `get`, but a successful (2xx) body is written to `out` as it arrives instead of being collected, reading at most
    /// `max_body` bytes. Other statuses write nothing. The default collects through `get`; a real transport streams.
    fn stream(
        &self,
        url: &Url,
        pinned: &[SocketAddr],
        max_body: u64,
        out: &mut dyn std::io::Write,
    ) -> Result<Response, FetchError> {
        let mut r = self.get(url, pinned, usize::try_from(max_body).unwrap_or(usize::MAX))?;
        if (200..300).contains(&r.status) {
            out.write_all(&r.body)
                .map_err(|e| FetchError::Transport(e.to_string()))?;
        }
        r.body = Vec::new();
        Ok(r)
    }
}

/// Writes through to `inner` but refuses to pass on more than `limit` bytes in total, whatever the transport does.
struct Capped<'a> {
    inner: &'a mut dyn std::io::Write,
    left: u64,
}

impl std::io::Write for Capped<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.len() as u64 > self.left {
            return Err(std::io::Error::other("the body is larger than allowed"));
        }
        self.left -= buf.len() as u64;
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub requested: String,
    pub final_url: String,
    pub title: String,
    pub text: String,
    pub truncated: bool,
}

pub struct Fetcher<'a> {
    pub dns: &'a dyn Dns,
    pub transport: &'a dyn Transport,
}

fn text_type(ct: &str) -> bool {
    let t = ct
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        t.as_str(),
        "text/html"
            | "application/xhtml+xml"
            | "text/plain"
            | "text/markdown"
            | "application/json"
            | "text/xml"
            | "application/xml"
    )
}

impl Fetcher<'_> {
    /// The addresses a connection to `url` may use: every address the name resolves to must be public, and the connection is
    /// pinned to exactly those.
    fn pin(&self, url: &Url) -> Result<Vec<SocketAddr>, FetchError> {
        let host = url
            .host_str()
            .ok_or(FetchError::BadUrl("no host"))?
            .to_string();
        let ips: Vec<IpAddr> = match host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
        {
            Ok(ip) => vec![ip],
            Err(_) => self.dns.resolve(&host),
        };
        if ips.is_empty() {
            return Err(FetchError::NoAddress);
        }
        // a mixed answer is how rebinding attacks start
        if ips.iter().any(|ip| !ip_allowed(*ip)) {
            return Err(FetchError::BlockedAddress);
        }
        Ok(ips
            .iter()
            .take(8)
            .map(|ip| SocketAddr::new(*ip, 443))
            .collect())
    }

    /// Where a redirect goes, held to the same rules as the first address.
    fn redirect_target(current: &Url, r: &Response) -> Result<Url, FetchError> {
        let loc = r.location.clone().ok_or(FetchError::Status(r.status))?;
        let next = current
            .join(&loc)
            .map_err(|_| FetchError::BadUrl("a redirect to an invalid address"))?;
        validate_url(next.as_str())
    }

    /// Fetches `raw` and writes the body to `out` as it arrives, never more than `max` bytes. The same rules as a page apply
    /// to every hop (https only, no credentials, public addresses only, pinned connection, redirects re-validated), and the
    /// body of a redirect or an error answer is never written. The cap is enforced here as well as by the transport.
    pub fn fetch_to(
        &self,
        raw: &str,
        max: u64,
        out: &mut dyn std::io::Write,
    ) -> Result<Url, FetchError> {
        let mut current = validate_url(raw)?;
        for _hop in 0..=MAX_REDIRECTS {
            let pinned = self.pin(&current)?;
            let mut capped = Capped {
                inner: out,
                left: max,
            };
            let r = self.transport.stream(&current, &pinned, max, &mut capped)?;
            if matches!(r.status, 301 | 302 | 303 | 307 | 308) {
                current = Self::redirect_target(&current, &r)?;
                continue;
            }
            if !(200..300).contains(&r.status) {
                return Err(FetchError::Status(r.status));
            }
            return Ok(current);
        }
        Err(FetchError::TooManyRedirects)
    }

    /// The checked GET shared by page reading and search: every hop is validated, resolved and pinned from the start.
    fn get_checked(&self, raw: &str) -> Result<(Url, Response), FetchError> {
        let mut current = validate_url(raw)?;
        for _hop in 0..=MAX_REDIRECTS {
            let pinned = self.pin(&current)?;
            let r = self.transport.get(&current, &pinned, MAX_BODY)?;
            if matches!(r.status, 301 | 302 | 303 | 307 | 308) {
                current = Self::redirect_target(&current, &r)?;
                continue;
            }
            if !(200..300).contains(&r.status) {
                return Err(FetchError::Status(r.status));
            }
            if !text_type(&r.content_type) {
                return Err(FetchError::NotText);
            }
            return Ok((current, r));
        }
        Err(FetchError::TooManyRedirects)
    }

    pub fn fetch(&self, raw: &str) -> Result<Page, FetchError> {
        let (current, r) = self.get_checked(raw)?;
        let body = String::from_utf8_lossy(&r.body[..r.body.len().min(MAX_BODY)]).into_owned();
        let html = r.content_type.to_ascii_lowercase().contains("html");
        let (title, mut text) = if html {
            extract_html(&body)
        } else {
            (String::new(), clean_text(&body))
        };
        let mut truncated = r.body.len() >= MAX_BODY;
        if text.chars().count() > MAX_TEXT_CHARS {
            text = text.chars().take(MAX_TEXT_CHARS).collect();
            truncated = true;
        }
        Ok(Page {
            requested: raw.to_string(),
            final_url: current.to_string(),
            title,
            text,
            truncated,
        })
    }

    /// The raw body of a JSON answer from a search provider, under the same rules as any page.
    pub fn fetch_json(&self, raw: &str) -> Result<Vec<u8>, FetchError> {
        let (_, r) = self.get_checked(raw)?;
        if !r.content_type.to_ascii_lowercase().contains("json") {
            return Err(FetchError::NotText);
        }
        if r.body.len() >= MAX_BODY {
            return Err(FetchError::BadUrl("the search answer was too large"));
        }
        Ok(r.body)
    }
}

// ---- search ----

pub const MAX_QUERY_CHARS: usize = 200;
pub const MAX_HITS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// The freshness windows a SearXNG-style provider understands.
pub const RECENCY_WINDOWS: [&str; 4] = ["day", "week", "month", "year"];

/// The exact address that would be requested for a query at a SearXNG-style provider.
pub fn search_url(provider: &str, query: &str) -> Result<Url, FetchError> {
    search_url_with(provider, query, None)
}

/// `search_url`, limited to results from the last day, week, month or year when a window is given.
pub fn search_url_with(
    provider: &str,
    query: &str,
    recency: Option<&str>,
) -> Result<Url, FetchError> {
    if let Some(w) = recency {
        if !RECENCY_WINDOWS.contains(&w) {
            return Err(FetchError::BadUrl(
                "the freshness window must be day, week, month or year",
            ));
        }
    }
    let mut u = validate_url(provider)?;
    if u.query().is_some() {
        return Err(FetchError::BadUrl(
            "the search provider address must not have a query",
        ));
    }
    let q = query.trim();
    if q.is_empty() || q.chars().count() > MAX_QUERY_CHARS || q.chars().any(|c| c.is_control()) {
        return Err(FetchError::BadUrl(
            "the search text must be 1 to 200 plain characters",
        ));
    }
    u.query_pairs_mut()
        .append_pair("q", q)
        .append_pair("format", "json")
        .append_pair("safesearch", "1");
    if let Some(w) = recency {
        u.query_pairs_mut().append_pair("time_range", w);
    }
    Ok(u)
}

pub const MAX_PER_HOST: usize = 3;
pub const MAX_FAILED_ENGINES: usize = 6;

/// What a SearXNG-style search site answered: the addresses worth choosing from, and the engines behind it that failed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchAnswer {
    pub hits: Vec<Hit>,
    /// "engine: reason" for each engine the site could not ask, so an empty or thin list can be explained.
    pub failed_engines: Vec<String>,
}

fn short(s: &str, n: usize) -> String {
    clean_text(s).chars().take(n).collect()
}

/// Results of a SearXNG-style JSON answer. Only addresses that could be fetched are kept; titles and snippets are cleaned
/// text and are still untrusted. A knowledge box (infobox) with an address comes first, one host fills at most
/// `MAX_PER_HOST` places so a single site cannot crowd out the rest, and at most `MAX_HITS` are kept.
pub fn parse_answer(body: &[u8]) -> SearchAnswer {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return SearchAnswer::default();
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut per_host: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut hits = Vec::new();
    let mut take = |url: &str, title: &str, snippet: &str| {
        if hits.len() >= MAX_HITS {
            return;
        }
        let Ok(u) = validate_url(url) else { return };
        let host = u.host_str().unwrap_or_default().to_ascii_lowercase();
        if per_host.get(&host).copied().unwrap_or(0) >= MAX_PER_HOST || !seen.insert(u.to_string())
        {
            return;
        }
        *per_host.entry(host).or_insert(0) += 1;
        hits.push(Hit {
            title: short(title, 200),
            url: u.to_string(),
            snippet: short(snippet, 400),
        });
    };
    for b in v["infoboxes"].as_array().into_iter().flatten() {
        let url = b["urls"]
            .as_array()
            .and_then(|a| a.iter().find_map(|u| u["url"].as_str()));
        if let (Some(url), Some(content)) = (url, b["content"].as_str()) {
            take(url, b["infobox"].as_str().unwrap_or(""), content);
        }
    }
    for r in v["results"].as_array().into_iter().flatten() {
        if let (Some(url), Some(title)) = (r["url"].as_str(), r["title"].as_str()) {
            take(url, title, r["content"].as_str().unwrap_or(""));
        }
    }
    let failed_engines = v["unresponsive_engines"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let pair = e.as_array()?;
            Some(format!(
                "{}: {}",
                short(pair.first()?.as_str()?, 30),
                short(pair.get(1)?.as_str()?, 45)
            ))
        })
        .take(MAX_FAILED_ENGINES)
        .collect();
    SearchAnswer {
        hits,
        failed_engines,
    }
}

/// The addresses of a search answer; see `parse_answer`.
pub fn parse_hits(body: &[u8]) -> Vec<Hit> {
    parse_answer(body).hits
}

/// Why a search request failed, in words the owner can act on.
pub fn explain_search_failure(e: &FetchError) -> String {
    match e {
        FetchError::Status(403) => "the search site refuses JSON answers; its owner must list json under search.formats in its settings (a stock SearXNG install does not)".into(),
        FetchError::Status(429) => "the search site says too many requests were made; wait a while and try again".into(),
        FetchError::Status(c @ 500..=599) => format!("the search site is unavailable right now (HTTP {c}); try again later"),
        FetchError::Status(c) => format!("the search site answered HTTP {c}"),
        FetchError::BadUrl(why) => format!("the search address was refused: {why}"),
        FetchError::BlockedAddress => "the search site's address is not a public address, so it was not contacted".into(),
        FetchError::NoAddress => "the search site's name did not resolve".into(),
        FetchError::TooManyRedirects => "the search site redirected too many times".into(),
        FetchError::NotText => "the search site did not answer with JSON (many sites only send web pages; a SearXNG site must list json under search.formats, or ask a different site)".into(),
        FetchError::Transport(_) => "the search site could not be reached".into(),
    }
}

// ---- text extraction ----

fn clean_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for c in s.chars() {
        let hidden = (c.is_control() && !matches!(c, '\n' | '\t'))
            || matches!(c as u32, 0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x2064 | 0x2066..=0x2069 | 0xfeff);
        if hidden {
            continue;
        }
        if c.is_whitespace() {
            if !last_space || c == '\n' {
                out.push(if c == '\n' { '\n' } else { ' ' });
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim().to_string()
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|e| *e <= 10);
        let Some(e) = end else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..e];
        let rep = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => ent
                .strip_prefix('#')
                .and_then(|n| {
                    if let Some(h) = n.strip_prefix(['x', 'X']) {
                        u32::from_str_radix(h, 16).ok()
                    } else {
                        n.parse().ok()
                    }
                })
                .and_then(char::from_u32),
        };
        match rep {
            Some(c) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Title and readable text of an HTML page. Scripts, styles and templates are dropped with their contents; nothing is
/// executed or followed. This is a reader, not a parser: unusual markup yields less text, never a different page.
pub fn extract_html(html: &str) -> (String, String) {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 2);
    let mut title = String::new();
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let rest = &lower[i..];
            if rest.starts_with("<!--") {
                i += rest.find("-->").map(|e| e + 3).unwrap_or(rest.len());
                continue;
            }
            let name: String = rest[1..]
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            let closing = rest[1..].starts_with('/');
            let tag_end = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
            if !closing
                && matches!(
                    name.as_str(),
                    "script" | "style" | "noscript" | "template" | "svg" | "iframe"
                )
            {
                let close = format!("</{name}");
                i += rest[tag_end..]
                    .find(&close)
                    .map(|e| tag_end + e)
                    .and_then(|c| rest[c..].find('>').map(|g| c + g + 1))
                    .unwrap_or(rest.len());
                continue;
            }
            if name == "title" && !closing {
                if let Some(e) = rest[tag_end..].find("</title") {
                    title = clean_text(&decode_entities(&html[i + tag_end..i + tag_end + e]));
                }
            }
            if matches!(
                name.as_str(),
                "p" | "br"
                    | "div"
                    | "li"
                    | "tr"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "section"
                    | "article"
                    | "pre"
            ) {
                out.push('\n');
            }
            i += tag_end;
        } else {
            let next = html[i..].find('<').map(|e| i + e).unwrap_or(html.len());
            out.push_str(&html[i..next]);
            i = next;
        }
    }
    (title, clean_text(&decode_entities(&out)))
}

// ---- the real network (kept apart so the rules above can be tested without it) ----

pub struct SystemDns;

impl Dns for SystemDns {
    fn resolve(&self, host: &str) -> Vec<IpAddr> {
        use std::net::ToSocketAddrs;
        (host, 443)
            .to_socket_addrs()
            .map(|it| it.map(|a| a.ip()).collect())
            .unwrap_or_default()
    }
}

#[derive(Debug)]
struct PinnedResolver {
    addrs: Vec<SocketAddr>,
}

impl ureq::unversioned::resolver::Resolver for PinnedResolver {
    fn resolve(
        &self,
        _uri: &ureq::http::Uri,
        _config: &ureq::config::Config,
        _timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let mut out = self.empty();
        for a in self.addrs.iter().take(8) {
            out.push(*a);
        }
        if out.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(out)
    }
}

pub struct UreqTransport {
    pub timeout: std::time::Duration,
}

impl Transport for UreqTransport {
    fn get(
        &self,
        url: &Url,
        pinned: &[SocketAddr],
        max_body: usize,
    ) -> Result<Response, FetchError> {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(self.timeout))
            .user_agent("Kakurega/0.1 (+local research; no cookies)")
            .build();
        let agent = ureq::Agent::with_parts(
            config,
            ureq::unversioned::transport::DefaultConnector::default(),
            PinnedResolver {
                addrs: pinned.to_vec(),
            },
        );
        let mut r = agent
            .get(url.as_str())
            .call()
            .map_err(|e| FetchError::Transport(e.to_string()))?;
        let status = r.status().as_u16();
        let location = r
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let content_type = r
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = r
            .body_mut()
            .with_config()
            .limit(max_body as u64)
            .read_to_vec()
            .map_err(|e| FetchError::Transport(e.to_string()))?;
        Ok(Response {
            status,
            location,
            content_type,
            body,
        })
    }

    fn stream(
        &self,
        url: &Url,
        pinned: &[SocketAddr],
        max_body: u64,
        out: &mut dyn std::io::Write,
    ) -> Result<Response, FetchError> {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(self.timeout))
            .user_agent("Kakurega/0.1 (+local research; no cookies)")
            .build();
        let agent = ureq::Agent::with_parts(
            config,
            ureq::unversioned::transport::DefaultConnector::default(),
            PinnedResolver {
                addrs: pinned.to_vec(),
            },
        );
        let mut r = agent
            .get(url.as_str())
            .call()
            .map_err(|e| FetchError::Transport(e.to_string()))?;
        let status = r.status().as_u16();
        let location = r
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let content_type = r
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if (200..300).contains(&status) {
            let mut reader = r.body_mut().with_config().limit(max_body).reader();
            std::io::copy(&mut reader, out).map_err(|e| FetchError::Transport(e.to_string()))?;
        }
        Ok(Response {
            status,
            location,
            content_type,
            body: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    struct FakeDns(BTreeMap<&'static str, Vec<IpAddr>>);
    impl Dns for FakeDns {
        fn resolve(&self, host: &str) -> Vec<IpAddr> {
            self.0.get(host).cloned().unwrap_or_default()
        }
    }

    struct FakeNet {
        pages: BTreeMap<&'static str, Response>,
        calls: RefCell<Vec<(String, Vec<SocketAddr>)>>,
    }
    impl Transport for FakeNet {
        fn get(
            &self,
            url: &Url,
            pinned: &[SocketAddr],
            _max: usize,
        ) -> Result<Response, FetchError> {
            self.calls
                .borrow_mut()
                .push((url.to_string(), pinned.to_vec()));
            self.pages
                .get(url.as_str())
                .cloned()
                .ok_or(FetchError::Transport("no such page".into()))
        }
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn page(status: u16, ct: &str, body: &str) -> Response {
        Response {
            status,
            location: None,
            content_type: ct.into(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn redirect(to: &str) -> Response {
        Response {
            status: 302,
            location: Some(to.into()),
            content_type: String::new(),
            body: vec![],
        }
    }

    #[test]
    fn fe01_only_public_addresses_pass_in_every_spelling() {
        for good in [
            "8.8.8.8",
            "93.184.216.34",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
        ] {
            assert!(ip_allowed(ip(good)), "{good}");
        }
        for bad in [
            "0.0.0.0",
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "169.254.170.2",
            "100.100.100.200",
            "100.64.0.1",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.9",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "::",
            "::1",
            "fe80::1",
            "fc00::1",
            "fd00:ec2::254",
            "ff02::1",
            "2001:db8::1",
            "2001::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::7f00:1",
            "64:ff9b::a9fe:a9fe",
            "::ffff:0:a00:1",
            "2002:7f00:1::1",
            "2002:0a00:1::",
        ] {
            assert!(!ip_allowed(ip(bad)), "{bad} must be blocked");
        }
    }

    #[test]
    fn fe02_urls_are_https_plain_and_free_of_credentials() {
        assert!(validate_url("https://example.org/a?q=rust+async").is_ok());
        assert!(validate_url("https://example.org:443/").is_ok());
        for (bad, _why) in [
            ("http://example.org/", "scheme"),
            ("ftp://example.org/", "scheme"),
            ("file:///etc/passwd", "scheme"),
            ("javascript:alert(1)", "scheme"),
            ("https://user:pw@example.org/", "credentials"),
            ("https://user@example.org/", "credentials"),
            ("https://example.org:8443/", "port"),
            ("https://localhost/", "local"),
            ("https://app.localhost/", "local"),
            ("https://printer.local/", "local"),
            ("https://db.internal/", "internal"),
            ("https://metadata.google.internal/", "metadata"),
            ("https://127.0.0.1/", "ip"),
            ("https://[::1]/", "ip"),
            ("https://169.254.169.254/latest/meta-data/", "ip"),
            ("https://example.org/?token=abc", "credential param"),
            ("https://example.org/?x=1&API_KEY=abc", "credential param"),
            ("https://example.org/ with space", "space"),
            ("https://exa\nmple.org/", "control"),
        ] {
            assert!(validate_url(bad).is_err(), "{bad}");
        }
        assert!(
            validate_url("https://example.org/?token=").is_ok(),
            "an empty value carries no credential"
        );
        assert!(
            validate_url("https://example.org/?code=7&key=value").is_ok(),
            "ordinary words that double as parameters are not blocked"
        );
        assert!(validate_url(&format!("https://example.org/{}", "a".repeat(2100))).is_err());
    }

    #[test]
    fn fe03_the_connection_is_pinned_to_the_checked_address_and_mixed_answers_are_refused() {
        let dns = FakeDns(
            [
                ("good.example", vec![ip("93.184.216.34")]),
                ("mixed.example", vec![ip("93.184.216.34"), ip("10.0.0.5")]),
                ("none.example", vec![]),
                ("meta.example", vec![ip("169.254.169.254")]),
            ]
            .into(),
        );
        let net = FakeNet {
            pages: [("https://good.example/", page(200, "text/html", "<p>hi</p>"))].into(),
            calls: RefCell::new(vec![]),
        };
        let f = Fetcher {
            dns: &dns,
            transport: &net,
        };
        assert!(f.fetch("https://good.example/").is_ok());
        assert_eq!(
            net.calls.borrow()[0].1,
            vec!["93.184.216.34:443".parse::<SocketAddr>().unwrap()],
            "the transport is told exactly which address to dial"
        );
        assert_eq!(
            f.fetch("https://mixed.example/").err(),
            Some(FetchError::BlockedAddress),
            "one private answer among public ones refuses the name"
        );
        assert_eq!(
            f.fetch("https://meta.example/").err(),
            Some(FetchError::BlockedAddress)
        );
        assert_eq!(
            f.fetch("https://none.example/").err(),
            Some(FetchError::NoAddress)
        );
        assert_eq!(
            net.calls.borrow().len(),
            1,
            "nothing was requested for refused names"
        );
    }

    #[test]
    fn fe04_every_redirect_is_checked_from_the_start_and_chains_are_short() {
        let dns = FakeDns(
            [
                ("a.example", vec![ip("93.184.216.34")]),
                ("b.example", vec![ip("93.184.216.35")]),
                ("evil.example", vec![ip("127.0.0.1")]),
            ]
            .into(),
        );
        let pages: BTreeMap<&'static str, Response> = [
            ("https://a.example/start", redirect("https://b.example/end")),
            ("https://b.example/end", page(200, "text/plain", "arrived")),
            (
                "https://a.example/to-http",
                redirect("http://b.example/end"),
            ),
            (
                "https://a.example/to-evil",
                redirect("https://evil.example/"),
            ),
            ("https://a.example/to-ip", redirect("https://10.0.0.1/")),
            (
                "https://a.example/to-creds",
                redirect("https://u:p@b.example/"),
            ),
            (
                "https://a.example/to-token",
                redirect("https://b.example/?token=abc"),
            ),
            ("https://a.example/relative", redirect("/end2")),
            (
                "https://a.example/end2",
                page(200, "text/plain", "relative ok"),
            ),
            ("https://a.example/loop", redirect("https://a.example/loop")),
        ]
        .into();
        let net = FakeNet {
            pages,
            calls: RefCell::new(vec![]),
        };
        let f = Fetcher {
            dns: &dns,
            transport: &net,
        };
        assert_eq!(
            f.fetch("https://a.example/start").unwrap().final_url,
            "https://b.example/end"
        );
        assert_eq!(
            f.fetch("https://a.example/relative").unwrap().text,
            "relative ok"
        );
        for (u, want) in [
            ("https://a.example/to-http", "BadUrl"),
            ("https://a.example/to-evil", "BlockedAddress"),
            ("https://a.example/to-ip", "BlockedAddress"),
            ("https://a.example/to-creds", "BadUrl"),
            ("https://a.example/to-token", "BadUrl"),
            ("https://a.example/loop", "TooManyRedirects"),
        ] {
            let e = format!("{:?}", f.fetch(u).err().unwrap());
            assert!(e.starts_with(want), "{u}: {e}");
        }
    }

    #[test]
    fn fe05_only_text_comes_back_and_page_text_is_a_reader_view_without_scripts_or_hidden_characters(
    ) {
        let dns = FakeDns([("a.example", vec![ip("93.184.216.34")])].into());
        let html = "<html><head><title>Rust &amp; async</title><style>p{color:red}</style><script>steal(document.cookie)</script></head><body><h1>Heading</h1><p>First &lt;b&gt; para&nbsp;one.</p><!-- hidden comment --><noscript>no js</noscript><p>Zero\u{200b}width and \u{202e}bidi gone</p><iframe src=x>frame</iframe></body></html>";
        let net = FakeNet {
            pages: [
                (
                    "https://a.example/page",
                    page(200, "text/html; charset=utf-8", html),
                ),
                ("https://a.example/img", page(200, "image/png", "PNG")),
                (
                    "https://a.example/pdf",
                    page(200, "application/pdf", "%PDF"),
                ),
                ("https://a.example/404", page(404, "text/html", "nope")),
                (
                    "https://a.example/big",
                    page(200, "text/plain", &"word ".repeat(40_000)),
                ),
            ]
            .into(),
            calls: RefCell::new(vec![]),
        };
        let f = Fetcher {
            dns: &dns,
            transport: &net,
        };
        let p = f.fetch("https://a.example/page").unwrap();
        assert_eq!(p.title, "Rust & async");
        assert!(
            p.text.contains("Heading")
                && p.text.contains("First <b> para one.")
                && p.text.contains("Zerowidth and bidi gone"),
            "{}",
            p.text
        );
        for gone in [
            "steal",
            "color:red",
            "hidden comment",
            "no js",
            "frame",
            "\u{200b}",
            "\u{202e}",
        ] {
            assert!(
                !p.text.contains(gone),
                "{gone} must not survive: {}",
                p.text
            );
        }
        assert_eq!(
            f.fetch("https://a.example/img").err(),
            Some(FetchError::NotText)
        );
        assert_eq!(
            f.fetch("https://a.example/pdf").err(),
            Some(FetchError::NotText)
        );
        assert_eq!(
            f.fetch("https://a.example/404").err(),
            Some(FetchError::Status(404))
        );
        let big = f.fetch("https://a.example/big").unwrap();
        assert!(big.truncated && big.text.chars().count() <= MAX_TEXT_CHARS);
        // hostile markup finishes and never panics
        for h in [
            "<",
            "<<<<",
            "<script",
            "<script>",
            "<!--",
            "&#",
            "&#x110000;",
            "&&&&;",
            "<title>",
            "<p><p><p>",
            &"<div>".repeat(50_000),
        ] {
            let _ = extract_html(h);
        }
    }

    #[test]
    #[ignore = "needs the internet"]
    fn fe06_the_real_transport_fetches_a_public_page_over_a_pinned_connection() {
        let t = UreqTransport {
            timeout: std::time::Duration::from_secs(20),
        };
        let f = Fetcher {
            dns: &SystemDns,
            transport: &t,
        };
        let p = f.fetch("https://example.com/").expect("fetch");
        assert!(
            p.title.to_lowercase().contains("example") && p.text.contains("documentation examples"),
            "{p:?}"
        );
        assert_eq!(
            f.fetch("https://127.0.0.1/").err(),
            Some(FetchError::BlockedAddress)
        );
        assert!(matches!(
            f.fetch("https://example.com/definitely-not-a-page-9f3a")
                .err(),
            Some(FetchError::Status(404))
        ));
    }

    #[test]
    fn fe07_search_address_is_exact_and_the_provider_is_held_to_the_same_rules() {
        let u = search_url("https://search.example/search", "  rust async  ").unwrap();
        assert_eq!(
            u.as_str(),
            "https://search.example/search?q=rust+async&format=json&safesearch=1"
        );
        for bad in [
            "http://search.example/search",
            "https://localhost/search",
            "https://user:pw@search.example/search",
            "https://search.example:8443/search",
            "https://search.example/search?token=abc",
            "https://search.example/search?x=1",
        ] {
            assert!(search_url(bad, "q").is_err(), "{bad}");
        }
        assert!(search_url("https://search.example/s", "   ").is_err());
        assert!(search_url("https://search.example/s", &"a".repeat(201)).is_err());
        assert!(search_url("https://search.example/s", "a\nb").is_err());
        let hostile = search_url("https://search.example/s", "x&q=other#frag y").unwrap();
        assert_eq!(
            hostile.query_pairs().count(),
            3,
            "the query text cannot add parameters: {hostile}"
        );
    }

    #[test]
    fn fe08_only_fetchable_addresses_survive_and_text_is_cleaned_and_capped() {
        let body = serde_json::json!({"results": [
            {"title": "Good\u{202e} one", "url": "https://a.example/x", "content": "snippet\u{200b} text"},
            {"title": "dup", "url": "https://a.example/x", "content": "again"},
            {"title": "local", "url": "https://127.0.0.1/admin", "content": "no"},
            {"title": "plain http", "url": "http://b.example/", "content": "no"},
            {"title": "creds", "url": "https://u:p@c.example/", "content": "no"},
            {"title": "no url", "content": "no"},
            {"url": "https://d.example/", "content": "no title"},
            {"title": "T".repeat(500), "url": "https://e.example/", "content": "S".repeat(900)},
        ]}).to_string();
        let hits = parse_hits(body.as_bytes());
        assert_eq!(
            hits.iter().map(|h| h.url.as_str()).collect::<Vec<_>>(),
            ["https://a.example/x", "https://e.example/"]
        );
        assert_eq!(
            (hits[0].title.as_str(), hits[0].snippet.as_str()),
            ("Good one", "snippet text")
        );
        assert_eq!(
            (
                hits[1].title.chars().count(),
                hits[1].snippet.chars().count()
            ),
            (200, 400)
        );
        assert!(parse_hits(b"not json").is_empty() && parse_hits(b"{}").is_empty());
        let many = serde_json::json!({"results": (0..40).map(|i| serde_json::json!({"title": "t", "url": format!("https://h{i}.example/"), "content": ""})).collect::<Vec<_>>()});
        assert_eq!(parse_hits(many.to_string().as_bytes()).len(), MAX_HITS);
    }

    #[test]
    fn fe20_a_real_shaped_answer_reports_failed_engines_infobox_and_caps_hits_per_host() {
        let mut results: Vec<serde_json::Value> = (0..6)
            .map(|i| serde_json::json!({"title": format!("one {i}"), "url": format!("https://same.example/p{i}"), "content": "c", "engine": "bing", "score": 1.0}))
            .collect();
        results.push(
            serde_json::json!({"title": "other", "url": "https://other.example/", "content": "c"}),
        );
        let body = serde_json::json!({
            "query": "q", "results": results,
            "infoboxes": [{"infobox": "A", "content": "A is a letter.", "urls": [{"title": "Wikipedia", "url": "https://en.wikipedia.org/wiki/A"}]},
                          {"infobox": "B", "content": "no link"},
                          {"infobox": "C", "content": "bad link", "urls": [{"url": "https://127.0.0.1/x"}]}],
            "unresponsive_engines": [["duckduckgo", "CAPTCHA"], ["brave", "HTTP error"], "junk", [1, 2]]
        })
        .to_string();
        let a = parse_answer(body.as_bytes());
        let urls: Vec<&str> = a.hits.iter().map(|h| h.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://en.wikipedia.org/wiki/A",
                "https://same.example/p0",
                "https://same.example/p1",
                "https://same.example/p2",
                "https://other.example/"
            ],
            "the box comes first, a host gives at most {MAX_PER_HOST}"
        );
        assert_eq!(a.hits[0].title, "A");
        assert_eq!(a.hits[0].snippet, "A is a letter.");
        assert_eq!(
            a.failed_engines,
            ["duckduckgo: CAPTCHA", "brave: HTTP error"]
        );
        // an infobox alone is a usable answer (a real one-letter query returned only that)
        let only = serde_json::json!({"results": [], "infoboxes": [{"content": "x", "urls": [{"url": "https://en.wikipedia.org/wiki/A"}]}]}).to_string();
        assert_eq!(parse_answer(only.as_bytes()).hits.len(), 1);
        assert_eq!(parse_hits(body.as_bytes()), a.hits);
        // engine names and reasons are cleaned and short, and only a few are kept
        let noisy = serde_json::json!({"results": [], "unresponsive_engines":
            (0..20).map(|i| serde_json::json!([format!("e{i}\u{202e}"), "x".repeat(300)])).collect::<Vec<_>>()}).to_string();
        let n = parse_answer(noisy.as_bytes());
        assert_eq!(n.failed_engines.len(), MAX_FAILED_ENGINES);
        assert!(n
            .failed_engines
            .iter()
            .all(|e| e.chars().count() <= 80 && !e.contains('\u{202e}')));
        assert!(
            parse_answer(b"not json").hits.is_empty()
                && parse_answer(b"[]").failed_engines.is_empty()
        );
    }

    #[test]
    fn fe22_a_freshness_window_is_one_of_four_words_and_is_part_of_the_address_shown() {
        for w in ["day", "week", "month", "year"] {
            let u = search_url_with("https://s.example/search", "rust", Some(w)).unwrap();
            assert_eq!(
                u.as_str(),
                format!("https://s.example/search?q=rust&format=json&safesearch=1&time_range={w}")
            );
        }
        assert_eq!(
            search_url_with("https://s.example/search", "rust", None).unwrap(),
            search_url("https://s.example/search", "rust").unwrap()
        );
        for bad in ["", "Month", "decade", "month&q=x", "day ", "None"] {
            assert!(
                search_url_with("https://s.example/search", "rust", Some(bad)).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn fe21_search_failures_are_explained_in_plain_words() {
        let say = |e: FetchError| explain_search_failure(&e);
        assert!(say(FetchError::Status(403)).contains("search.formats"));
        assert!(say(FetchError::Status(429)).contains("too many"));
        assert!(say(FetchError::Status(503)).contains("unavailable"));
        assert!(say(FetchError::Status(404)).contains("404"));
        assert!(say(FetchError::NotText).contains("search.formats"));
        assert!(say(FetchError::BadUrl("only https is allowed")).contains("only https"));
        for e in [
            FetchError::NotText,
            FetchError::BlockedAddress,
            FetchError::Transport("x".into()),
        ] {
            let m = say(e);
            assert!(
                !m.is_empty() && !m.contains("Transport(") && !m.contains("NotText"),
                "{m}"
            );
        }
    }

    #[test]
    fn fe09_a_search_answer_must_be_json_and_goes_through_the_same_address_checks() {
        let dns = FakeDns(
            [
                ("search.example", vec![ip("93.184.216.34")]),
                ("rebind.example", vec![ip("93.184.216.34"), ip("10.0.0.5")]),
            ]
            .into(),
        );
        let net = FakeNet {
            pages: [
                (
                    "https://search.example/s?q=a&format=json&safesearch=1",
                    page(200, "application/json; charset=utf-8", "{\"results\":[]}"),
                ),
                (
                    "https://search.example/s?q=b&format=json&safesearch=1",
                    page(200, "text/html", "<p>hi</p>"),
                ),
            ]
            .into(),
            calls: RefCell::new(vec![]),
        };
        let f = Fetcher {
            dns: &dns,
            transport: &net,
        };
        assert_eq!(
            f.fetch_json("https://search.example/s?q=a&format=json&safesearch=1")
                .unwrap(),
            b"{\"results\":[]}"
        );
        assert_eq!(
            f.fetch_json("https://search.example/s?q=b&format=json&safesearch=1"),
            Err(FetchError::NotText)
        );
        assert_eq!(
            f.fetch_json("https://rebind.example/s?q=a"),
            Err(FetchError::BlockedAddress)
        );
        assert!(net
            .calls
            .borrow()
            .iter()
            .all(|(_, p)| p.iter().all(|a| a.port() == 443)));
    }

    #[test]
    fn fe10_hosts_written_in_unusual_ways_are_read_the_way_a_browser_reads_them_and_refused() {
        // the Rust url crate follows the WHATWG rules, so the host checked here is the host a browser would dial
        for bad in [
            "https://2130706433/", // 127.0.0.1 as one decimal number
            "https://0x7f.1/",     // hex and short forms
            "https://0177.0.0.1/", // octal
            "https://127.1/",
            "https://%31%32%37.0.0.1/", // percent-encoded host
            "https://127\u{3002}0\u{3002}0\u{3002}1/", // ideographic full stops
            "https://[::ffff:127.0.0.1]/",
            "https://[::ffff:0:127.0.0.1]/",
            "https://[::1]/",
            "https://[fd00:ec2::254]/",   // AWS metadata over IPv6
            "https://[64:ff9b::7f00:1]/", // NAT64 wrapper of 127.0.0.1
            "https://[2002:7f00:1::]/",   // 6to4 wrapper of 127.0.0.1
            "https://169.254.169.254/",
            "https://169.254.169.253/",
            "https://169.254.170.2/",
            "https://100.100.100.200/",
            "https://168.63.129.16/", // Azure wire server
            "https://localhost./",    // trailing dot
            "https://metadata.google.internal./",
            "https://printer.local./",
        ] {
            assert!(validate_url(bad).is_err(), "{bad} must be refused");
        }
        // a backslash ends the host for a browser, so this is example.com and never the address after it
        let u = validate_url("https://example.com\\@127.0.0.1/").unwrap();
        assert_eq!(u.host_str(), Some("example.com"));
        for good in [
            "https://example.com/",
            "https://docs.rs/tokio",
            "https://[2606:4700:4700::1111]/",
            "https://1.1.1.1/",
            "https://168.63.129.17/",
        ] {
            assert!(validate_url(good).is_ok(), "{good}");
        }
    }

    // ---- streaming (fetch_to) ----

    fn stream_dns() -> FakeDns {
        FakeDns(
            [
                ("a.example", vec![ip("93.184.216.34")]),
                ("b.example", vec![ip("93.184.216.35")]),
                ("mixed.example", vec![ip("93.184.216.34"), ip("10.0.0.5")]),
                ("evil.example", vec![ip("127.0.0.1")]),
            ]
            .into(),
        )
    }

    fn net(pages: Vec<(&'static str, Response)>) -> FakeNet {
        FakeNet {
            pages: pages.into_iter().collect(),
            calls: RefCell::new(vec![]),
        }
    }

    #[test]
    fn fs01_a_streamed_body_reaches_the_sink_from_a_pinned_public_address_and_the_final_address_is_returned(
    ) {
        let dns = stream_dns();
        let n = net(vec![(
            "https://a.example/f.bin",
            page(200, "application/octet-stream", "0123456789"),
        )]);
        let f = Fetcher {
            dns: &dns,
            transport: &n,
        };
        let mut sink = Vec::new();
        let final_url = f
            .fetch_to("https://a.example/f.bin", 10, &mut sink)
            .unwrap();
        assert_eq!(sink, b"0123456789");
        assert_eq!(final_url.as_str(), "https://a.example/f.bin");
        assert_eq!(
            n.calls.borrow()[0].1,
            vec!["93.184.216.34:443".parse::<SocketAddr>().unwrap()]
        );
    }

    #[test]
    fn fs02_the_byte_cap_holds_even_when_the_transport_ignores_it() {
        let dns = stream_dns();
        // the fake transport hands over all 100 bytes whatever the limit says
        let n = net(vec![(
            "https://a.example/f.bin",
            page(200, "application/octet-stream", &"x".repeat(100)),
        )]);
        let f = Fetcher {
            dns: &dns,
            transport: &n,
        };
        let mut sink = Vec::new();
        let r = f.fetch_to("https://a.example/f.bin", 10, &mut sink);
        assert!(matches!(r, Err(FetchError::Transport(_))), "{r:?}");
        assert!(
            sink.len() <= 10,
            "no more than the cap was ever written: {}",
            sink.len()
        );
        // exactly the cap is fine, one byte less than the body is not
        let mut ok = Vec::new();
        assert!(f.fetch_to("https://a.example/f.bin", 100, &mut ok).is_ok());
        assert_eq!(ok.len(), 100);
        let mut one_short = Vec::new();
        assert!(f
            .fetch_to("https://a.example/f.bin", 99, &mut one_short)
            .is_err());
        assert!(one_short.len() <= 99);
    }

    #[test]
    fn fs03_redirects_are_followed_under_the_same_rules_and_their_bodies_are_never_written() {
        let dns = stream_dns();
        let mut redirect_with_body = redirect("https://b.example/end");
        redirect_with_body.body = b"PAGE OF LIES".to_vec();
        let n = net(vec![
            ("https://a.example/start", redirect_with_body),
            (
                "https://b.example/end",
                page(200, "application/octet-stream", "real"),
            ),
            (
                "https://a.example/to-http",
                redirect("http://b.example/end"),
            ),
            (
                "https://a.example/to-private",
                redirect("https://evil.example/end"),
            ),
            (
                "https://a.example/to-creds",
                redirect("https://user:pw@b.example/end"),
            ),
            (
                "https://a.example/missing",
                page(404, "text/html", "not found page"),
            ),
            ("https://a.example/loop", redirect("https://a.example/loop")),
        ]);
        let f = Fetcher {
            dns: &dns,
            transport: &n,
        };
        let mut sink = Vec::new();
        f.fetch_to("https://a.example/start", 100, &mut sink)
            .unwrap();
        assert_eq!(sink, b"real", "only the final body, never a redirect's");
        let mut http_sink = Vec::new();
        assert!(matches!(
            f.fetch_to("https://a.example/to-http", 100, &mut http_sink),
            Err(FetchError::BadUrl(_))
        ));
        assert!(http_sink.is_empty());
        let mut s = Vec::new();
        assert_eq!(
            f.fetch_to("https://a.example/to-private", 100, &mut s)
                .err(),
            Some(FetchError::BlockedAddress)
        );
        assert!(matches!(
            f.fetch_to("https://a.example/to-creds", 100, &mut s),
            Err(FetchError::BadUrl(_))
        ));
        assert_eq!(
            f.fetch_to("https://a.example/missing", 100, &mut s).err(),
            Some(FetchError::Status(404))
        );
        assert_eq!(
            f.fetch_to("https://a.example/loop", 100, &mut s).err(),
            Some(FetchError::TooManyRedirects)
        );
        let loops = n
            .calls
            .borrow()
            .iter()
            .filter(|c| c.0 == "https://a.example/loop")
            .count();
        assert_eq!(
            loops,
            MAX_REDIRECTS + 1,
            "a redirect loop is given up after the limit, not followed on"
        );
        assert!(s.is_empty(), "an error answer's body is not written: {s:?}");
    }

    #[test]
    fn fs04_refused_names_and_plain_http_make_no_request_at_all() {
        let dns = stream_dns();
        let n = net(vec![("https://a.example/f", page(200, "x", "body"))]);
        let f = Fetcher {
            dns: &dns,
            transport: &n,
        };
        let mut s = Vec::new();
        assert_eq!(
            f.fetch_to("https://mixed.example/f", 10, &mut s).err(),
            Some(FetchError::BlockedAddress)
        );
        assert_eq!(
            f.fetch_to("https://evil.example/f", 10, &mut s).err(),
            Some(FetchError::BlockedAddress)
        );
        assert!(f.fetch_to("http://a.example/f", 10, &mut s).is_err());
        assert!(f.fetch_to("https://127.0.0.1/f", 10, &mut s).is_err());
        assert!(f
            .fetch_to("https://user:pw@a.example/f", 10, &mut s)
            .is_err());
        assert!(n.calls.borrow().is_empty(), "{:?}", n.calls.borrow());
        assert!(s.is_empty());
    }

    #[test]
    #[ignore = "needs the internet"]
    fn fs05_live_the_real_transport_streams_a_real_page_and_stops_at_the_cap() {
        let dns = SystemDns;
        let net = UreqTransport {
            timeout: std::time::Duration::from_secs(30),
        };
        let f = Fetcher {
            dns: &dns,
            transport: &net,
        };
        let mut body = Vec::new();
        let url = f
            .fetch_to("https://example.com/", 1 << 20, &mut body)
            .unwrap();
        assert!(url.as_str().starts_with("https://example.com"));
        assert!(
            body.len() > 200,
            "a real page arrived: {} bytes",
            body.len()
        );
        assert!(String::from_utf8_lossy(&body)
            .to_lowercase()
            .contains("example"));
        // the same page with a cap far below its size is cut off, and no more than the cap was written
        let mut cut = Vec::new();
        let r = f.fetch_to("https://example.com/", 50, &mut cut);
        assert!(r.is_err(), "{r:?}");
        assert!(cut.len() <= 50, "{}", cut.len());
        // a missing page writes nothing
        let mut none = Vec::new();
        assert!(f
            .fetch_to(
                "https://example.com/definitely-not-a-page-xyz",
                1 << 20,
                &mut none
            )
            .is_err());
        assert!(none.is_empty(), "{}", none.len());
    }
}
