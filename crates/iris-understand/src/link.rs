//! The hostile mark `link` (card R5-10): a link that shows one web address
//! and goes to another owner. `iris-clean` writes each HTML link whose text
//! is not its target as `[text](target)`, so this module reads the clean
//! text. Mail text is data: the code only compares host names.

use iris_senders::near_copy;
use url::{Host, Url};

/// The registrable domain of a host by the public suffix list: two hosts
/// with the same registrable domain have one owner. A host that is a public
/// suffix itself is its own owner.
fn registrable(host: &str) -> String {
    psl::domain_str(host).unwrap_or(host).to_string()
}

/// True when the clean text `text` has a deceptive link: the link text is a
/// web address with the host `S`, and the target is an `http` or `https`
/// URL that a browser opens on another owner. Iris reads the target with
/// the WHATWG URL standard, as a browser does. The link is deceptive in
/// each of these cases, unless DMARC proves that the sender owns the domain
/// of `S` (then it can link through any redirect, for example a click
/// counter of its mail service):
/// - The target host is an IP address, or Iris cannot read it (for
///   example a host that is not ASCII), or the target has a user part, a
///   backslash or a percent sign in front of its path.
/// - The shown host is not ASCII (`pаypal.com` with a Cyrillic `а`), or the
///   target host has a punycode label (`xn--`) and another owner: look-alike
///   letters that a person cannot tell apart.
/// - The target host `T` has another registrable domain than `S`, and DMARC
///   does not pass for the `From` domain (`authenticated` is false).
/// - `T` has another registrable domain than `S` and looks like `S`: a near
///   copy (`login.shop-secure.example.net` for `www.shop.example.com`), or
///   a host with the name of `S` in front of another domain
///   (`shop.example.com.evil.example.net`).
pub fn deceptive(text: &str, from_domain: &str, authenticated: bool) -> bool {
    let owner = registrable(&from_domain.to_ascii_lowercase());
    for (shown, target) in links(text) {
        let Some(shown) = shown_host(shown) else {
            continue;
        };
        let theirs = registrable(&shown);
        if authenticated && theirs == owner {
            continue;
        }
        match target_host(target) {
            Target::NotWeb => {}
            Target::Suspect => return true,
            Target::Host(t) if registrable(&t) == theirs => {}
            Target::Host(t) => {
                if !authenticated
                    || !shown.is_ascii()
                    || t.split('.').any(|l| l.starts_with("xn--"))
                    || near_copy(&t, &shown)
                    || embeds(&t, &theirs)
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Each `[shown](target)` of the text, in one pass. The shown text has no
/// `[`, `]` or line end. The target ends at `)` or at white space:
/// `iris-clean` percent-encodes both in a link target.
fn links(text: &str) -> impl Iterator<Item = (&str, &str)> {
    let mut open = None;
    let mut it = text.char_indices().peekable();
    std::iter::from_fn(move || {
        while let Some((i, c)) = it.next() {
            match c {
                '[' => open = Some(i + 1),
                '\n' => open = None,
                ']' => {
                    let Some(o) = open.take() else { continue };
                    if it.peek().map(|&(_, n)| n) != Some('(') {
                        continue;
                    }
                    let from = i + 2;
                    let end = text[from..]
                        .find(|c: char| c == ')' || c.is_whitespace())
                        .map_or(text.len(), |j| from + j);
                    // A target is not a link text: the scan goes on after
                    // it, so each character is read once.
                    while it.next_if(|&(j, _)| j < end).is_some() {}
                    return Some((&text[o..i], &text[from..end]));
                }
                _ => {}
            }
        }
        None
    })
}

/// The host of a link text that is a web address: `https://www.x.com/a`,
/// `www.x.com` or `x.com`. A text with other words is not a web address.
fn shown_host(shown: &str) -> Option<String> {
    let lower = shown.trim().to_lowercase();
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    let host = rest
        .split(['/', '?', '#', ':'])
        .next()?
        .trim_end_matches('.');
    // A host name has letters in its last label: `1.5` is a number.
    let last = host.rsplit('.').next()?;
    // Letters of each script count: a person reads `pаypal.com` as a web
    // address, whatever the script of its letters.
    let ok = host.contains('.')
        && host.split('.').all(|l| !l.is_empty())
        && host
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '.')
        && last.chars().count() >= 2
        && last.chars().all(char::is_alphabetic);
    ok.then(|| host.to_string())
}

/// What a browser opens for a link target.
enum Target {
    /// Not an `http` or `https` URL, for example `mailto:`.
    NotWeb,
    /// An IP address, or a web URL that Iris cannot read.
    Suspect,
    /// The host name, lowercase, with no end dot.
    Host(String),
}

fn target_host(target: &str) -> Target {
    // The URL standard drops leading control characters and spaces.
    let target = target.trim_start_matches(|c: char| c <= ' ');
    let lower = target.to_ascii_lowercase();
    let web = lower.starts_with("http:") || lower.starts_with("https:");
    // A user part, a backslash or a percent sign in front of the path
    // makes a browser open a host that the text does not show
    // (`https://shop.com@evil.net`, `https://evil.net\@shop.com`). A real
    // link never needs them.
    let after = lower.split_once(':').map_or("", |(_, a)| a);
    let rest = after.trim_start_matches(['/', '\\']);
    let slashes = &after[..after.len() - rest.len()];
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if web && (slashes.contains('\\') || authority.contains(['@', '\\', '%'])) {
        return Target::Suspect;
    }
    match Url::parse(target) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => match u.host() {
            Some(Host::Domain(d)) => Target::Host(d.trim_end_matches('.').to_string()),
            _ => Target::Suspect,
        },
        Ok(_) => Target::NotWeb,
        Err(_) if web => Target::Suspect,
        Err(_) => Target::NotWeb,
    }
}

/// True when the name label of the registrable domain `theirs` (`shop` of
/// `shop.com`) is a label, or a hyphen part of a label, of `target` in front
/// of its own registrable domain (`shop.com.evil.net`, `shop.evil.net`).
fn embeds(target: &str, theirs: &str) -> bool {
    let Some(name) = theirs.split('.').next().filter(|n| n.len() >= 3) else {
        return false;
    };
    let own = registrable(target);
    let front = target
        .strip_suffix(own.as_str())
        .unwrap_or("")
        .trim_end_matches('.');
    front.split(['.', '-']).any(|part| part == name)
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
    fn the_target_is_the_host_that_a_browser_opens() {
        // Each target opens evil.example.net, or a host that Iris cannot read.
        for target in [
            "https://evil.example.net\\@shop.example.com/",
            "https://evil%2eexample%2enet/",
            "HTTPS://EVIL.EXAMPLE.NET./",
            "https:evil.example.net/x",
            "https://192.0.2.7/login",
            "https://0xc0.0.2.7/login",
            "https://[2001:db8::1]/login",
            "https://p\u{430}ypal.example.net/",
        ] {
            let t = format!("[shop.example.com]({target})");
            assert!(deceptive(&t, "evil.example.net", false), "{target}");
        }
        // An IP or an unreadable host is deceptive also from a sender that
        // passes DMARC for another domain.
        for target in [
            "https://192.0.2.7/login",
            "https://p\u{430}ypal.example.net/",
            "https://shop.example.com@evil.example.net/",
            "https://evil.example.net\\@shop.example.com/",
            "https://evil%2eexample%2enet/",
            "https:\\\\evil.example.net\\@shop.example.com/",
            "\u{1}https://shop.example.com@evil.example.net/",
        ] {
            let t = format!("[shop.example.com]({target})");
            assert!(deceptive(&t, "mx.example.org", true), "{target}");
        }
    }

    #[test]
    fn a_shown_host_in_front_of_another_domain_is_deceptive() {
        for target in [
            "https://shop.example.com.evil.example.net/",
            "https://shop.evil.example.net/",
            "https://login-shop.evil.example.net/",
        ] {
            let t = format!("[www.shop.com]({target})");
            assert!(deceptive(&t, "evil.example.net", true), "{target}");
        }
        // A longer word that holds the name is another name.
        assert!(!deceptive(
            "[shop.com](https://shopping.example.net/)",
            "example.net",
            true
        ));
    }

    #[test]
    fn two_owners_under_a_public_suffix_are_two_owners() {
        let t = "[victim.me.uk](https://attacker.me.uk/x)";
        assert!(deceptive(t, "attacker.me.uk", false));
        let t = "[www.victim.me.uk](https://login.victim.me.uk/x)";
        assert!(!deceptive(t, "attacker.me.uk", false));
        let t = "[victim.github.io](https://attacker.github.io/x)";
        assert!(deceptive(t, "mx.example.org", false));
    }

    #[test]
    fn look_alike_letters_are_deceptive_also_from_an_authenticated_sender() {
        for t in [
            "[paypal.example.com](https://xn--pypal-4ve.example.net/)",
            "[p\u{430}ypal.example.com](https://login.example.net/)",
            "[P\u{430}YPAL.com](https://xn--pypal-4ve.com/)",
        ] {
            assert!(deceptive(t, "mx.example.org", true), "{t}");
        }
        // The same owner: an honest link to an IDN domain.
        let t = "[www.xn--mnchen-3ya.de](https://xn--mnchen-3ya.de/)";
        assert!(!deceptive(t, "mx.example.org", true));
    }

    #[test]
    fn a_tab_in_a_target_cannot_hide_the_host() {
        // iris-clean percent-encodes white space in a target.
        let t = "[https://bank.example.com](https://bank.example.com%09@evil.example.net/x)";
        assert!(deceptive(t, "evil.example.net", true));
    }

    #[test]
    fn a_long_shown_address_counts() {
        let shown = format!("https://www.shop.example.com/{}", "a".repeat(2000));
        let t = format!("Go: [{shown}](https://evil.example.net/x)");
        assert!(deceptive(&t, "evil.example.net", false));
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
