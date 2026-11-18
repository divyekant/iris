//! The few headers that the sender rules read. Mail text is data: this code
//! only compares bytes and follows nothing in the text.

/// The most bytes of a message that [`Head::parse`] reads. A message with
/// no blank line is all head, so the limit bounds the memory of one call.
pub(crate) const HEAD_MAX: usize = 256 * 1024;

/// The headers of one message, in order from the top, with folded lines
/// joined. Names are lowercase.
pub struct Head(Vec<(String, String)>);

/// The authserv-id of the `Authentication-Results` header of Gmail.
pub const GMAIL_AUTHSERV: &str = "mx.google.com";

/// The authserv-id of the `Authentication-Results` header that the provider
/// of an account adds on receipt: the only one to trust. Only Gmail has one.
/// For another provider, any sender can write a header with the Gmail ID
/// (docs/specs/2026-11-17-imap-provider.md, section 8).
pub fn trusted_authserv(provider: &str) -> Option<&'static str> {
    (provider == "gmail").then_some(GMAIL_AUTHSERV)
}

impl Head {
    pub fn parse(raw: &[u8]) -> Head {
        let raw = &raw[..raw.len().min(HEAD_MAX)];
        // The first empty line ends the head, with CRLF or LF line ends.
        let end = (0..raw.len())
            .find(|&i| {
                raw[i] == b'\n'
                    && (raw.get(i + 1) == Some(&b'\n')
                        || (raw.get(i + 1) == Some(&b'\r') && raw.get(i + 2) == Some(&b'\n')))
            })
            .unwrap_or(raw.len());
        let text = String::from_utf8_lossy(&raw[..end]);
        let mut out: Vec<(String, String)> = Vec::new();
        for line in text.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.starts_with([' ', '\t']) {
                if let Some(last) = out.last_mut() {
                    last.1.push(' ');
                    last.1.push_str(line.trim());
                }
            } else if let Some((name, value)) = line.split_once(':') {
                out.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
            }
        }
        Head(out)
    }

    /// The first header with this lowercase name.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The `From` address (lowercase) and display name.
    pub(crate) fn from(&self) -> Option<(String, Option<String>)> {
        addresses(self.get("from")?).into_iter().next()
    }

    /// Each address of `To`, `Cc` and `Bcc`, lowercase, with `true` for an
    /// address of `Bcc`. A sent message keeps its `Bcc` header.
    pub(crate) fn recipients(&self) -> Vec<(String, bool)> {
        self.0
            .iter()
            .filter(|(n, _)| n == "to" || n == "cc" || n == "bcc")
            .flat_map(|(n, v)| addresses(v).into_iter().map(move |(a, _)| (a, n == "bcc")))
            .collect()
    }

    /// The (address, display name) of each `To`, `Cc` and `Bcc` entry.
    pub(crate) fn recipient_names(&self) -> Vec<(String, Option<String>)> {
        self.0
            .iter()
            .filter(|(n, _)| n == "to" || n == "cc" || n == "bcc")
            .flat_map(|(_, v)| addresses(v))
            .collect()
    }

    /// Spec 1 section 6.7: `List-Unsubscribe`, `List-Id`, `Precedence: bulk`,
    /// or `Auto-Submitted` other than `no`.
    pub fn automated(&self) -> bool {
        // The value without comments and parameters.
        let first_word = |v: &str| {
            strip_comments(v)
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        };
        self.get("list-unsubscribe").is_some()
            || self.get("list-id").is_some()
            || self
                .get("precedence")
                .is_some_and(|v| first_word(v) == "bulk")
            || self
                .get("auto-submitted")
                .is_some_and(|v| first_word(v) != "no")
    }

    /// True when the `Authentication-Results` header that the provider added
    /// says DKIM passes for `domain` (or a parent domain of it). Only the top
    /// header with the authserv-id `authserv` ([`trusted_authserv`]) counts.
    /// Gmail puts its header above each header that the sender wrote, and a
    /// sender can write any result (spec 1 section 6.7). With no `authserv`,
    /// no header counts.
    pub(crate) fn dkim_pass(&self, domain: &str, authserv: Option<&str>) -> bool {
        let Some(authserv) = authserv else {
            return false;
        };
        let gmail = self.0.iter().find_map(|(n, v)| {
            if n != "authentication-results" {
                return None;
            }
            let v = strip_comments(v);
            let (id, rest) = v.split_once(';').unwrap_or((&v, ""));
            id.trim()
                .eq_ignore_ascii_case(authserv)
                .then(|| rest.to_string())
        });
        let Some(ar) = gmail else {
            return false;
        };
        let mut parts = ar.split(';');
        parts.any(|part| {
            let mut words = part.split_whitespace();
            if !words
                .next()
                .is_some_and(|w| w.eq_ignore_ascii_case("dkim=pass"))
            {
                return false;
            }
            words.any(|w| {
                let w = w.to_ascii_lowercase();
                let d = w.strip_prefix("header.d=").or_else(|| {
                    w.strip_prefix("header.i=")
                        .map(|i| i.rsplit('@').next().unwrap_or(""))
                });
                d.is_some_and(|d| {
                    !d.is_empty() && (domain == d || domain.ends_with(&format!(".{d}")))
                })
            })
        })
    }
}

/// Remove RFC 5322 comments `( ... )`, which can hold any text.
/// A `(` in a quoted string is text, not a comment.
fn strip_comments(v: &str) -> String {
    let (mut depth, mut quoted) = (0u32, false);
    v.chars()
        .filter(|c| {
            match c {
                '"' if depth == 0 => quoted = !quoted,
                '(' if !quoted => depth += 1,
                ')' if !quoted && depth > 0 => depth -= 1,
                _ => {}
            }
            depth == 0 && !(*c == ')' && !quoted)
        })
        .collect()
}

/// Decode RFC 2047 encoded words of a display name with the decoder of
/// `iris-clean`, which decodes the `Subject` of a message.
pub fn decode_name(n: &str) -> String {
    if !n.contains("=?") {
        return n.trim().to_string();
    }
    let one = n.replace(['\r', '\n'], " ");
    iris_clean::clean(format!("Subject: {one}\r\n\r\n").as_bytes())
        .subject
        .trim()
        .to_string()
}

/// The comment right after a bare address in one entry, as in
/// `ana@example.org (Ana Lee)`.
fn comment_name(entry: &str, addr: &str) -> Option<String> {
    let low = entry.to_ascii_lowercase();
    let at = low.find(addr)? + addr.len();
    let rest = entry.get(at..)?.trim_start();
    let inner = rest.strip_prefix('(')?;
    let end = inner.find(')')?;
    Some(inner[..end].trim().to_string()).filter(|n| !n.is_empty())
}

/// Split an address list at the commas that are outside quotes and angle
/// brackets. Each entry gives (lowercase address, display name).
pub fn addresses(v: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    each_address(v, |a, name| out.push((a, name)));
    out
}

/// [`addresses`] one entry at a time, with no list in memory: call `f`
/// with each (lowercase address, display name).
pub fn each_address(v: &str, mut f: impl FnMut(String, Option<String>)) {
    let (mut quoted, mut angle, mut start, mut comment) = (false, false, 0, 0u32);
    let mut push = |entry: &str| {
        let s = strip_comments(entry);
        let s = s.trim();
        let (addr, name) = match (s.rfind('<'), s.rfind('>')) {
            (Some(a), Some(b)) if a < b => (&s[a + 1..b], s[..a].trim()),
            _ => (s, ""),
        };
        let addr = addr.trim().to_ascii_lowercase();
        if addr.contains('@') {
            // `a@b.example (Ana Lee)`: the comment is the display name.
            let comment = match name.is_empty() {
                true => comment_name(entry, &addr),
                false => None,
            };
            let name = comment.as_deref().unwrap_or(name);
            // The sender rows keep the decoded name: the spoof check
            // compares it with the decoded name of a later `From`.
            let name = decode_name(&name.trim_matches('"').replace("\\\"", "\""));
            f(addr, (!name.is_empty()).then_some(name));
        }
    };
    for (i, c) in v.char_indices() {
        match c {
            '(' if !quoted => comment += 1,
            ')' if !quoted => comment = comment.saturating_sub(1),
            _ if comment > 0 => {}
            '"' if !angle => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            // Group syntax (RFC 5322 section 3.4): the group name ends at
            // `:` and the group ends at `;`.
            ':' if !quoted && !angle => start = i + 1,
            ',' | ';' if !quoted && !angle => {
                push(&v[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    push(&v[start..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_folded_headers_and_address_lists() {
        let h = Head::parse(
            b"From: \"Lee, Ana\" <Ana.Lee@Example.org>\r\nTo: a@example.com,\r\n b@example.net\r\nCc: <c@example.com>\r\n\r\nTo: body@example.com\r\n",
        );
        assert_eq!(
            h.from(),
            Some(("ana.lee@example.org".into(), Some("Lee, Ana".into())))
        );
        let to: Vec<String> = h.recipients().into_iter().map(|(a, _)| a).collect();
        assert_eq!(to, ["a@example.com", "b@example.net", "c@example.com"]);
        let h = Head::parse(b"Bcc: d@example.org\r\n\r\n");
        assert_eq!(h.recipients(), [("d@example.org".to_string(), true)]);
    }

    #[test]
    fn comments_are_not_part_of_an_address() {
        // A comment after a bare address is its display name.
        let h = Head::parse(
            b"From: lee@example.org (Lee, Ana)\r\nTo: \"Bo (work)\" <bo@example.net>, (x) cy@example.net\r\n\r\n",
        );
        assert_eq!(
            h.from(),
            Some(("lee@example.org".into(), Some("Lee, Ana".into())))
        );
        let to: Vec<String> = h.recipients().into_iter().map(|(a, _)| a).collect();
        assert_eq!(to, ["bo@example.net", "cy@example.net"]);
    }

    #[test]
    fn a_quote_in_a_comment_does_not_start_a_quoted_string() {
        let h = Head::parse(
            b"To: alice@example.org (said \"hello), bob@example.org\r\nCc: (a \"b\") cy@example.net, dee@example.net\r\n\r\n",
        );
        let to: Vec<String> = h.recipients().into_iter().map(|(a, _)| a).collect();
        assert_eq!(
            to,
            [
                "alice@example.org",
                "bob@example.org",
                "cy@example.net",
                "dee@example.net"
            ]
        );
    }

    #[test]
    fn mail_with_no_blank_line_reads_a_bounded_head() {
        let mut raw = b"From: a@example.org\r\nX-Big: ".to_vec();
        raw.resize(raw.len() + 8 * 1024 * 1024, b'a');
        let h = Head::parse(&raw);
        assert_eq!(h.from().unwrap().0, "a@example.org");
        let kept: usize = h.0.iter().map(|(n, v)| n.len() + v.len()).sum();
        assert!(kept <= HEAD_MAX, "{kept}");
    }

    #[test]
    fn the_first_blank_line_ends_the_head() {
        let h = Head::parse(b"From: a@example.org\n\nTo: body@example.net\r\n\r\nBody.\r\n");
        assert!(h.recipients().is_empty());
        let h = Head::parse(b"From: a@example.org\r\n\r\nTo: body@example.net\n\nBody.\n");
        assert!(h.recipients().is_empty());
    }

    #[test]
    fn reads_group_syntax() {
        let h = Head::parse(
            b"To: Friends: Ana <ana@example.org>, bob@example.org;, cy@example.net\r\nCc: Empty:;\r\n\r\n",
        );
        let to: Vec<String> = h.recipients().into_iter().map(|(a, _)| a).collect();
        assert_eq!(to, ["ana@example.org", "bob@example.org", "cy@example.net"]);
    }

    #[test]
    fn automated_headers() {
        let auto = |s: &str| Head::parse(s.as_bytes()).automated();
        assert!(auto("List-Id: <x.example.com>\r\n\r\n"));
        assert!(auto("Precedence: Bulk\r\n\r\n"));
        assert!(!auto("Precedence: junk\r\n\r\n"));
        assert!(auto("Auto-Submitted: auto-generated\r\n\r\n"));
        assert!(!auto("Auto-Submitted: no\r\n\r\n"));
        assert!(!auto("Auto-Submitted: no (human-generated)\r\n\r\n"));
        assert!(!auto("Auto-Submitted: (a person) no\r\n\r\n"));
        assert!(auto("Precedence: bulk (list mail)\r\n\r\n"));
        assert!(!auto("Subject: hi\r\n\r\n"));
    }

    #[test]
    fn only_the_top_gmail_authentication_results_header_counts() {
        let gmail = "Authentication-Results: mx.google.com;\r\n dkim=pass (fake: dkim=pass header.d=evil.example) header.i=@mail.shop.example.com;\r\n dmarc=pass\r\n\r\n";
        let h = Head::parse(gmail.as_bytes());
        // The account provider decides the trust: only Gmail adds the header.
        assert!(!h.dkim_pass("mail.shop.example.com", trusted_authserv("imap")));
        assert!(!h.dkim_pass("mail.shop.example.com", None));
        const G: Option<&str> = Some(GMAIL_AUTHSERV);
        assert_eq!(trusted_authserv("gmail"), G);
        assert!(h.dkim_pass("mail.shop.example.com", G));
        assert!(h.dkim_pass("a.mail.shop.example.com", G));
        assert!(!h.dkim_pass("shop.example.com", G));
        // The text in a comment does not count.
        assert!(!h.dkim_pass("evil.example", G));

        // Another provider's header above Gmail's does not hide Gmail's result.
        let other = "Authentication-Results: mx.other.example; dkim=fail header.d=shop.example.com\r\nAuthentication-Results: mx.google.com; dkim=pass header.d=shop.example.com\r\n\r\n";
        assert!(Head::parse(other.as_bytes()).dkim_pass("shop.example.com", G));
        let none =
            "Authentication-Results: mx.other.example; dkim=pass header.d=shop.example.com\r\n\r\n";
        assert!(!Head::parse(none.as_bytes()).dkim_pass("shop.example.com", G));
        // Gmail puts its header on top. A header lower down is the sender's.
        let fail = "Authentication-Results: mx.google.com; dkim=fail header.d=shop.example.com\r\nAuthentication-Results: mx.google.com; dkim=pass header.d=shop.example.com\r\n\r\n";
        assert!(!Head::parse(fail.as_bytes()).dkim_pass("shop.example.com", G));
        // "example.com" must not match the end of "badexample.com".
        let near = "Authentication-Results: mx.google.com; dkim=pass header.d=example.com\r\n\r\n";
        assert!(!Head::parse(near.as_bytes()).dkim_pass("badexample.com", G));
    }
}
