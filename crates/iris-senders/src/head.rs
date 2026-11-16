//! The few headers that the sender rules read. Mail text is data: this code
//! only compares bytes and follows nothing in the text.

/// The headers of one message, in order from the top, with folded lines
/// joined. Names are lowercase.
pub(crate) struct Head(Vec<(String, String)>);

impl Head {
    pub(crate) fn parse(raw: &[u8]) -> Head {
        let end = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .or_else(|| raw.windows(2).position(|w| w == b"\n\n"))
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
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
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

    /// Spec 1 section 6.7: `List-Unsubscribe`, `List-Id`, `Precedence: bulk`,
    /// or `Auto-Submitted` other than `no`.
    pub(crate) fn automated(&self) -> bool {
        let first_word = |v: &str| {
            v.split(';')
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

    /// True when the `Authentication-Results` header that Gmail added says
    /// DKIM passes for `domain` (or a parent domain of it). Only the top
    /// header with the authserv-id `mx.google.com` counts. Gmail puts its
    /// header above each header that the sender wrote, and a sender can
    /// write any result (spec 1 section 6.7).
    pub(crate) fn dkim_pass(&self, domain: &str) -> bool {
        let gmail = self.0.iter().find_map(|(n, v)| {
            if n != "authentication-results" {
                return None;
            }
            let v = strip_comments(v);
            let (id, rest) = v.split_once(';').unwrap_or((&v, ""));
            id.trim()
                .eq_ignore_ascii_case("mx.google.com")
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
fn strip_comments(v: &str) -> String {
    let mut depth = 0u32;
    v.chars()
        .filter(|c| {
            match c {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                _ => return depth == 0,
            }
            false
        })
        .collect()
}

/// Split an address list at the commas that are outside quotes and angle
/// brackets. Each entry gives (lowercase address, display name).
pub(crate) fn addresses(v: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let (mut quoted, mut angle, mut start) = (false, false, 0);
    let mut push = |s: &str| {
        let s = s.trim();
        let (addr, name) = match (s.rfind('<'), s.rfind('>')) {
            (Some(a), Some(b)) if a < b => (&s[a + 1..b], s[..a].trim()),
            _ => (s, ""),
        };
        let addr = addr.trim().to_ascii_lowercase();
        if addr.contains('@') {
            // shortcut: an RFC 2047 encoded name stays encoded; decode it if
            // a reader of the name needs the text.
            let name = name.trim_matches('"').trim();
            out.push((addr, (!name.is_empty()).then(|| name.to_string())));
        }
    };
    for (i, c) in v.char_indices() {
        match c {
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
    out
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
        assert!(h.dkim_pass("mail.shop.example.com"));
        assert!(h.dkim_pass("a.mail.shop.example.com"));
        assert!(!h.dkim_pass("shop.example.com"));
        // The text in a comment does not count.
        assert!(!h.dkim_pass("evil.example"));

        // Another provider's header above Gmail's does not hide Gmail's result.
        let other = "Authentication-Results: mx.other.example; dkim=fail header.d=shop.example.com\r\nAuthentication-Results: mx.google.com; dkim=pass header.d=shop.example.com\r\n\r\n";
        assert!(Head::parse(other.as_bytes()).dkim_pass("shop.example.com"));
        let none =
            "Authentication-Results: mx.other.example; dkim=pass header.d=shop.example.com\r\n\r\n";
        assert!(!Head::parse(none.as_bytes()).dkim_pass("shop.example.com"));
        // Gmail puts its header on top. A header lower down is the sender's.
        let fail = "Authentication-Results: mx.google.com; dkim=fail header.d=shop.example.com\r\nAuthentication-Results: mx.google.com; dkim=pass header.d=shop.example.com\r\n\r\n";
        assert!(!Head::parse(fail.as_bytes()).dkim_pass("shop.example.com"));
        // "example.com" must not match the end of "badexample.com".
        let near = "Authentication-Results: mx.google.com; dkim=pass header.d=example.com\r\n\r\n";
        assert!(!Head::parse(near.as_bytes()).dkim_pass("badexample.com"));
    }
}
