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
    let host = u
        .host_str()
        .ok_or(FetchError::BadUrl("no host"))?
        .to_ascii_lowercase();
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
    pub fn fetch(&self, raw: &str) -> Result<Page, FetchError> {
        let mut current = validate_url(raw)?;
        for _hop in 0..=MAX_REDIRECTS {
            let host = current
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
            // every address the name resolves to must be public: a mixed answer is how rebinding attacks start
            if ips.iter().any(|ip| !ip_allowed(*ip)) {
                return Err(FetchError::BlockedAddress);
            }
            let pinned: Vec<SocketAddr> = ips
                .iter()
                .take(8)
                .map(|ip| SocketAddr::new(*ip, 443))
                .collect();
            let r = self.transport.get(&current, &pinned, MAX_BODY)?;
            if matches!(r.status, 301 | 302 | 303 | 307 | 308) {
                let loc = r.location.ok_or(FetchError::Status(r.status))?;
                let next = current
                    .join(&loc)
                    .map_err(|_| FetchError::BadUrl("a redirect to an invalid address"))?;
                current = validate_url(next.as_str())?;
                continue;
            }
            if !(200..300).contains(&r.status) {
                return Err(FetchError::Status(r.status));
            }
            if !text_type(&r.content_type) {
                return Err(FetchError::NotText);
            }
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
            return Ok(Page {
                requested: raw.to_string(),
                final_url: current.to_string(),
                title,
                text,
                truncated,
            });
        }
        Err(FetchError::TooManyRedirects)
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
}
