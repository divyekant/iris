//! The hostile mark `link` (card R5-10): a link that shows one web address
//! and goes to another owner. `iris-clean` lists each HTML link whose text
//! is not its target (`Clean::links`), with the href as the HTML gives it.
//! Mail text is data: the code only compares host names.

use iris_clean::Link;
use iris_senders::registrable;
use url::{Host, Url};

/// True when the message has a deceptive link: the link text is a web
/// address with the host `S`, and the target is an `http` or `https` URL
/// that a browser opens on another owner. The owner of a host is its
/// registrable domain by the public suffix list, with both hosts in
/// punycode. Iris reads the target with the WHATWG URL standard, as a
/// browser does. The link is deceptive when one of these is true:
/// - The target host is an IP address, or Iris cannot read it, or the
///   target has a user part, a backslash or a percent sign in front of its
///   path.
/// - The target host has another owner than `S`.
///
/// Only one sender can link `S` to another owner: a sender that owns `S`,
/// as DMARC proves (`authenticated` and the `From` domain has the owner of
/// `S`). It can link through any redirect, for example a click counter of
/// its mail service. DMARC for another domain proves nothing about `S`.
///
/// `cut` is true when `iris-clean` could not list each link (a very large
/// body). Iris cannot check the links that went out, so the message counts
/// as deceptive.
pub fn deceptive(links: &[Link], cut: bool, from_domain: &str, authenticated: bool) -> bool {
    if cut {
        return true;
    }
    let owner = registrable(&from_domain.to_ascii_lowercase());
    for Link { text, href } in links {
        let Some(shown) = shown_host(text) else {
            continue;
        };
        let theirs = registrable(&shown);
        if authenticated && theirs == owner {
            continue;
        }
        match target_host(href) {
            Target::NotWeb => {}
            Target::Host(t) if registrable(&t) == theirs => {}
            Target::Suspect | Target::Host(_) => return true,
        }
    }
    false
}

/// The host of a link text that is a web address: `https://www.x.com/a`,
/// `www.x.com` or `x.com`. A text with other words is not a web address. A
/// text that starts with `http://`, `https://` or `www.` is one web address
/// also across lines (`https://bank.com<br>/login`).
fn shown_host(shown: &str) -> Option<String> {
    let lower = shown.trim().to_lowercase();
    let lower = if ["http://", "https://", "www."]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        lower.split_whitespace().collect::<String>()
    } else if lower.contains(char::is_whitespace) {
        return None;
    } else {
        lower
    };
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    let host = rest
        .split(['/', '?', '#', ':'])
        .next()?
        .trim_end_matches('.');
    // A host name has letters in its last label: `1.5` is a number. A
    // punycode label (`xn--p1ai`) is letters in another form.
    let last = host.rsplit('.').next()?;
    // Letters of each script count: a person reads `pаypal.com` as a web
    // address, whatever the script of its letters.
    let ok = host.contains('.')
        && host.split('.').all(|l| !l.is_empty())
        && host
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '.')
        && (last.starts_with("xn--")
            || last.chars().count() >= 2 && last.chars().all(char::is_alphabetic));
    if !ok {
        return None;
    }
    // One form for each host: punycode, as the URL parser gives a target.
    match Host::parse(host) {
        Ok(Host::Domain(d)) => Some(d),
        _ => None,
    }
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

fn target_host(href: &str) -> Target {
    // The steps of the URL standard before the parse: one rule with the
    // link list of iris-clean.
    let target = iris_clean::browser_href(href);
    let target = target.as_str();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One link that shows `shown` and goes to `href`.
    fn one(shown: &str, href: &str, from: &str, authenticated: bool) -> bool {
        let link = Link {
            text: shown.into(),
            href: href.into(),
        };
        deceptive(&[link], false, from, authenticated)
    }

    const SHOWN: &str = "https://www.shop.example.com/account";
    const NEAR: &str = "https://login.shop-secure.example.net/verify?u=1";

    #[test]
    fn a_near_copy_target_is_deceptive_also_from_an_authenticated_sender() {
        assert!(one(SHOWN, NEAR, "mx.example.org", true));
        assert!(one(SHOWN, NEAR, "mx.example.org", false));
        assert!(one(
            "paypal.me.uk",
            "https://paypa1.me.uk/",
            "paypa1.me.uk",
            true
        ));
    }

    #[test]
    fn another_owner_is_deceptive_also_with_dmarc_for_another_domain() {
        let (s, t) = (
            "www.news.example.com",
            "https://click.mailer.example.net/r/1",
        );
        assert!(one(s, t, "list.example.org", false));
        assert!(one(s, t, "list.example.org", true));
        // A sender with its own domain and DMARC links a bank address to it.
        assert!(one(
            "https://bank.com/login",
            "https://evil.example.net/login",
            "evil.example.net",
            true
        ));
    }

    #[test]
    fn the_proven_owner_of_the_shown_domain_can_redirect() {
        // A brand counts clicks on a hyphen domain of its own name.
        let (s, t) = ("shop.com", "https://click.shop-mail.net/x");
        assert!(!one(s, t, "news.shop.com", true));
        assert!(one(s, t, "news.shop.com", false));
    }

    #[test]
    fn the_target_is_the_host_that_a_browser_opens() {
        // Each target opens evil.example.net, or a host that Iris cannot
        // read, also for a sender that passes DMARC for another domain.
        for href in [
            "https://evil.example.net\\@shop.example.com/",
            "https://evil%2eexample%2enet/",
            "https://shop.example.com@evil.example.net/",
            "https://shop.example.com\t@evil.example.net/",
            "https://shop.example.com\n@evil.example.net/",
            "https:\\\\evil.example.net\\@shop.example.com/",
            "\u{1}https://shop.example.com@evil.example.net/",
            " https://shop.example.com@evil.example.net/ ",
            "https://192.0.2.7/login",
            "https://0xc0.0.2.7/login",
            "https://[2001:db8::1]/login",
            "https://xn--pypal-4ve.example.net/",
        ] {
            assert!(
                one("shop.example.com", href, "mx.example.org", true),
                "{href:?}"
            );
        }
        // Without DMARC, any other owner is deceptive.
        for href in ["HTTPS://EVIL.EXAMPLE.NET./", "https:evil.example.net/x"] {
            assert!(
                one("shop.example.com", href, "evil.example.net", false),
                "{href}"
            );
        }
    }

    #[test]
    fn a_shown_host_in_front_of_another_domain_is_deceptive() {
        for href in [
            "https://shop.example.com.evil.example.net/",
            "https://shop.evil.example.net/",
            "https://login-shop.evil.example.net/",
        ] {
            assert!(
                one("www.shop.com", href, "evil.example.net", true),
                "{href}"
            );
        }
    }

    #[test]
    fn two_owners_under_a_public_suffix_are_two_owners() {
        assert!(one(
            "victim.me.uk",
            "https://attacker.me.uk/x",
            "attacker.me.uk",
            false
        ));
        assert!(!one(
            "www.victim.me.uk",
            "https://login.victim.me.uk/x",
            "a.me.uk",
            false
        ));
        assert!(one(
            "victim.github.io",
            "https://attacker.github.io/x",
            "x.example.org",
            false
        ));
    }

    #[test]
    fn look_alike_letters_are_deceptive_also_from_an_authenticated_sender() {
        for (s, t) in [
            ("paypal.example.com", "https://xn--pypal-4ve.example.net/"),
            ("p\u{430}ypal.example.com", "https://login.example.net/"),
            ("PAYPAL.com", "https://xn--pypal-4ve.com/"),
            ("https://bank.xn--p1ai", "https://login.example.net/"),
            ("bank.\u{440}\u{444}/login", "https://login.example.net/"),
        ] {
            assert!(one(s, t, "mx.example.org", true), "{s}");
        }
        // The same owner in two forms: an honest link to an IDN domain.
        for (s, t) in [
            ("www.xn--mnchen-3ya.de", "https://xn--mnchen-3ya.de/"),
            ("www.m\u{fc}nchen.de", "https://www.m\u{fc}nchen.de/"),
            ("www.M\u{dc}NCHEN.de", "https://xn--mnchen-3ya.de/a"),
            ("xn--mnchen-3ya.de", "https://www.m\u{fc}nchen.de/"),
            ("https://bank.xn--p1ai/x", "https://bank.\u{440}\u{444}/"),
        ] {
            assert!(!one(s, t, "x.org", false), "{s} {t}");
        }
    }

    #[test]
    fn a_shown_address_across_lines_or_long_counts() {
        assert!(one(
            "https://bank.example.com\n/login",
            "https://evil.example.net/",
            "e.net",
            false
        ));
        assert!(one(
            "www.bank.example.com\n/login",
            "https://evil.example.net/",
            "e.net",
            false
        ));
        let long = format!("https://www.shop.example.com/{}", "a".repeat(2000));
        assert!(one(
            &long,
            "https://evil.example.net/x",
            "evil.example.net",
            false
        ));
    }

    #[test]
    fn a_cut_list_is_deceptive() {
        assert!(deceptive(&[], true, "shop.example.com", true));
        assert!(!deceptive(&[], false, "shop.example.com", false));
    }

    #[test]
    fn honest_links_and_other_texts_are_not_deceptive() {
        for (s, t) in [
            ("www.shop.example.com", "https://shop.example.com/a"),
            ("https://a.shop.example.com", "https://b.shop.example.com/x"),
            ("Click here", "https://evil.example.net/"),
            ("Visit\nbank.example.com", "https://evil.example.net/"),
            ("1.5", "https://evil.example.net/"),
            ("a@shop.example.com", "https://evil.example.net/"),
            ("www.shop.example.com", "mailto:a@evil.example.net"),
            ("www.shop.example.com", "javascript:run()"),
            ("www.shop.example.com", "/relative/path"),
            ("", "https://evil.example.net"),
        ] {
            assert!(!one(s, t, "x.example.org", false), "{s} {t}");
        }
    }

    #[test]
    fn random_links_do_not_panic() {
        let pieces = [
            "[",
            "]",
            "(",
            ")",
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
            "\\",
            "%",
            "\n",
            "\t",
            " ",
            "é",
            "\u{0}",
            "a",
            "xn--",
        ];
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = |n: usize| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x % n as u64) as usize
        };
        for _ in 0..3000 {
            let mut part = || {
                (0..next(24))
                    .map(|_| pieces[next(pieces.len())])
                    .collect::<String>()
            };
            let (s, t) = (part(), part());
            one(&s, &t, "example.org", next(2) == 0);
        }
    }
}
