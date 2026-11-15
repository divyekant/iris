//! Tracking parameters in links.

/// Query parameters that only track the reader. Each name that starts with
/// `utm_` goes out too.
const TRACKING: [&str; 24] = [
    "fbclid",
    "gclid",
    "dclid",
    "gbraid",
    "wbraid",
    "msclkid",
    "yclid",
    "twclid",
    "ttclid",
    "igshid",
    "mc_cid",
    "mc_eid",
    "_hsenc",
    "_hsmi",
    "__hssc",
    "__hstc",
    "__hsfp",
    "hsctatracking",
    "mkt_tok",
    "oly_anon_id",
    "oly_enc_id",
    "vero_id",
    "_ke",
    "ck_subscriber_id",
];

/// Remove the tracking parameters from one URL.
pub fn clean_url(url: &str) -> String {
    let (main, frag) = url.split_at(url.find('#').unwrap_or(url.len()));
    let Some((base, query)) = main.split_once('?') else {
        return url.to_string();
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| {
            let k = p.split('=').next().unwrap_or("").to_ascii_lowercase();
            !p.is_empty() && !k.starts_with("utm_") && !TRACKING.contains(&k.as_str())
        })
        .collect();
    let mut out = base.to_string();
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    out.push_str(frag);
    out
}

/// Remove the tracking parameters from each `http` and `https` URL in a text.
pub fn clean_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = find_url(rest) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        // Parentheses inside a URL are kept when they are balanced.
        let mut depth = 0usize;
        let mut end = tail.len();
        for (k, c) in tail.char_indices() {
            match c {
                '(' => depth += 1,
                ')' if depth > 0 => depth -= 1,
                _ if c.is_whitespace() || ")<>\"'[]{}|\\^`".contains(c) => {
                    end = k;
                    break;
                }
                _ => {}
            }
        }
        while end > 0 && b".,;:!?".contains(&tail.as_bytes()[end - 1]) {
            end -= 1;
        }
        out.push_str(&clean_url(&tail[..end]));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn find_url(s: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(j) = s[from..].find("http") {
        let i = from + j;
        let t = &s[i..];
        if t.starts_with("http://") || t.starts_with("https://") {
            return Some(i);
        }
        from = i + 4;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_parameters_go_out() {
        assert_eq!(
            clean_url(
                "https://shop.example.com/p?id=4&utm_source=mail&UTM_Medium=x&fbclid=abc#top"
            ),
            "https://shop.example.com/p?id=4#top"
        );
        assert_eq!(
            clean_url("https://a.example.com/?utm_x=1"),
            "https://a.example.com/"
        );
        assert_eq!(
            clean_url("https://a.example.com/x"),
            "https://a.example.com/x"
        );
    }

    #[test]
    fn urls_in_text_and_links() {
        assert_eq!(
            clean_urls(
                "See https://a.example.com/?utm_a=1&k=2. And [x](http://b.example.com/?gclid=9)"
            ),
            "See https://a.example.com/?k=2. And [x](http://b.example.com/)"
        );
        assert_eq!(clean_urls("httpx http:/ é https"), "httpx http:/ é https");
        assert_eq!(
            clean_urls("(see https://w.example.org/Foo_(bar)?utm_source=m)"),
            "(see https://w.example.org/Foo_(bar))"
        );
    }
}
