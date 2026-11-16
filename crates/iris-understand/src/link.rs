//! The hostile mark `link` (card R5-10): a link that shows one web address
//! and goes to another owner. `iris-clean` writes each HTML link whose text
//! is not its target as `[text](target)`, so this module reads the clean
//! text. Mail text is data: the code only compares host names.

use iris_senders::{near_copy, registrable};

/// The most bytes in front of `](` that can hold the shown text. A longer
/// text is not a web address that a person reads. The bound keeps the scan
/// linear in the length of the text.
const SHOWN_LIMIT: usize = 512;

/// True when the clean text `text` has a deceptive link: the link text is a
/// web address with the host `S`, the target is an `http` or `https` URL
/// with the host `T`, and `S` and `T` have different registrable domains.
/// Such a link is deceptive when one of these is true:
/// - DMARC does not pass for the `From` domain (`authenticated` is false).
/// - `T` is a near copy of `S` (`login.shop-secure.example.net` for
///   `www.shop.example.com`).
///
/// A sender that DMARC proves to own the domain of `S` can link through any
/// redirect, for example a click counter of its mail service.
pub fn deceptive(text: &str, from_domain: &str, authenticated: bool) -> bool {
    let owner = registrable(&from_domain.to_ascii_lowercase());
    let mut at = 0;
    while let Some(i) = text[at..].find("](") {
        let mid = at + i;
        at = mid + 2;
        let Some(shown) = shown_text(&text[..mid]).and_then(shown_host) else {
            continue;
        };
        let end = text[at..]
            .find(|c: char| c == ')' || c.is_whitespace())
            .map_or(text.len(), |j| at + j);
        let Some(target) = target_host(&text[at..end]) else {
            continue;
        };
        let theirs = registrable(&shown);
        if theirs == registrable(&target) || (authenticated && theirs == owner) {
            continue;
        }
        if !authenticated || near_copy(&target, &shown) {
            return true;
        }
    }
    false
}

/// The text between the last `[` of `before` and its end, on one line.
fn shown_text(before: &str) -> Option<&str> {
    let mut from = before.len().saturating_sub(SHOWN_LIMIT);
    while !before.is_char_boundary(from) {
        from += 1;
    }
    let tail = &before[from..];
    let open = tail.rfind('[')?;
    let shown = &tail[open + 1..];
    (!shown.contains([']', '\n'])).then_some(shown)
}

/// The host of a link text that is a web address: `https://www.x.com/a`,
/// `www.x.com` or `x.com`. A text with other words is not a web address.
fn shown_host(shown: &str) -> Option<String> {
    let s = shown.trim();
    let lower = s.to_ascii_lowercase();
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    let host = rest.split(['/', '?', '#', ':']).next()?;
    // A host name has letters in its last label: `1.5` is a number.
    let last = host.rsplit('.').next()?;
    (host_ok(host) && last.len() >= 2 && last.bytes().all(|b| b.is_ascii_alphabetic()))
        .then(|| host.trim_end_matches('.').to_string())
}

/// The host of an `http` or `https` target, after a user part
/// (`https://shop.example.com@other.example.net/` goes to `other`).
fn target_host(target: &str) -> Option<String> {
    let lower = target.to_ascii_lowercase();
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    host_ok(host).then(|| host.trim_end_matches('.').to_string())
}

/// Letters, digits, `-` and `.`, with at least one inner dot.
fn host_ok(host: &str) -> bool {
    let h = host.trim_end_matches('.');
    h.contains('.')
        && h.split('.').all(|l| !l.is_empty())
        && h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOP: &str =
        "[https://www.shop.example.com/account](https://login.shop-secure.example.net/verify?u=1)";

    #[test]
    fn a_near_copy_target_is_deceptive_also_from_an_authenticated_sender() {
        assert!(deceptive(SHOP, "mx.example.org", true));
        assert!(deceptive(SHOP, "mx.example.org", false));
        assert!(deceptive(
            &format!("Hi.\n\nGo to {SHOP} now."),
            "x.example.org",
            true
        ));
    }

    #[test]
    fn another_owner_is_deceptive_only_without_dmarc() {
        let t = "Read [www.news.example.com](https://click.mailer.example.net/r/1)";
        assert!(deceptive(t, "list.example.org", false));
        assert!(!deceptive(t, "list.example.org", true));
    }

    #[test]
    fn the_proven_owner_of_the_shown_domain_can_redirect() {
        // A brand counts clicks on a hyphen domain of its own name.
        let t = "[shop.example.com](https://click.shop-mail.example.net/x)";
        assert!(!deceptive(t, "news.shop.example.com", true));
        assert!(deceptive(t, "news.shop.example.com", false));
    }

    #[test]
    fn a_user_part_does_not_hide_the_target() {
        let t = "[https://www.shop.example.com](https://www.shop.example.com@evil.example.net/)";
        assert!(deceptive(t, "evil.example.net", false));
    }

    #[test]
    fn honest_links_and_other_texts_are_not_deceptive() {
        for t in [
            "[www.shop.example.com](https://shop.example.com/a)",
            "[https://a.shop.example.com](https://b.shop.example.com/x)",
            "[Click here](https://evil.example.net/)",
            "[1.5](https://evil.example.net/)",
            "[a@shop.example.com](https://evil.example.net/)",
            "[www.shop.example.com](mailto:a@evil.example.net)",
            "[www.shop.example.com](javascript:run())",
            "[www.shop.example.com]\n(https://evil.example.net/)",
            "[www.shop.\nexample.com](https://evil.example.net/)",
            "no link here",
            "](",
            "[](https://evil.example.net)",
        ] {
            assert!(!deceptive(t, "x.example.org", false), "{t}");
        }
    }

    #[test]
    fn random_text_does_not_panic_and_stays_linear() {
        let pieces = [
            "[",
            "]",
            "(",
            ")",
            "](",
            "https://",
            "http://",
            "www.",
            "shop",
            ".example.com",
            "-secure",
            ".net",
            "@",
            ":",
            "/",
            "\n",
            " ",
            "é",
            "\u{0}",
            "a",
        ];
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..2000 {
            let mut t = String::new();
            for _ in 0..(x % 64) {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                t.push_str(pieces[(x % pieces.len() as u64) as usize]);
            }
            deceptive(&t, "example.org", x.is_multiple_of(2));
        }
        // A long line of link openers ends fast.
        let long = "[a](".repeat(40_000) + "]( ";
        let start = std::time::Instant::now();
        assert!(!deceptive(&long, "example.org", false));
        assert!(start.elapsed().as_secs() < 5);
    }
}
