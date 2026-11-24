//! Steps 1 to 3 of understanding on hand-written messages: headers, clean
//! text and hostile marks. Pure functions: no store, no model.

use iris_understand::{analyze, Context, Kind, Mark};

const NOW: i64 = 1_767_225_600_000;

fn crlf(raw: &str) -> Vec<u8> {
    raw.replace('\n', "\r\n").into_bytes()
}

/// Analyze `raw` with these labels, known persons and earlier texts.
fn run(raw: &str, labels: &[&str], known: &[(&str, &str)], same: &[&str]) -> Vec<Mark> {
    analysis(raw, labels, known, same).marks
}

fn analysis(
    raw: &str,
    labels: &[&str],
    known: &[(&str, &str)],
    same: &[&str],
) -> iris_understand::Analysis {
    let labels: Vec<String> = labels.iter().map(|l| l.to_string()).collect();
    let same: Vec<String> = same.iter().map(|s| s.to_string()).collect();
    let lookup = |name: &str| {
        known
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, a)| a.to_string())
            .collect()
    };
    let cx = Context {
        me: "owner@example.com",
        kind: Kind::Stranger,
        labels: &labels,
        authserv: Some(iris_understand::head::GMAIL_AUTHSERV),
        internal_date: None,
        now_ms: NOW,
        known_person: &lookup,
        existing: &[],
        thread: &[],
        same_sender: &same,
        known: &[],
    };
    analyze(&crlf(raw), &cx)
}

const ANA: &[(&str, &str)] = &[("Ana Lee", "ana@example.org")];

#[test]
fn a_dmarc_fail_for_the_from_domain_is_a_mark() {
    let raw = "Authentication-Results: mx.google.com; dkim=fail header.d=shop.example; dmarc=fail (p=REJECT) header.from=shop.example
From: Shop <billing@shop.example>
Subject: Pay

Pay now.
";
    assert_eq!(run(raw, &[], &[], &[]), vec![Mark::Dmarc]);
    // A DMARC result for another domain is not about this From.
    let other = raw.replace("header.from=shop.example", "header.from=other.example");
    assert!(run(&other, &[], &[], &[]).is_empty());
}

#[test]
fn a_forged_lower_auth_header_does_not_count() {
    let raw = "Authentication-Results: mx.google.com; dkim=none; spf=fail; dmarc=fail (p=NONE) header.from=evil.example
Authentication-Results: mx.google.com; dkim=pass header.d=evil.example; dmarc=pass header.from=evil.example
From: Ana Lee <ana@evil.example>
Subject: Hi

Pay this.
";
    assert_eq!(run(raw, &[], ANA, &[]), vec![Mark::Dmarc, Mark::Spoof]);
}

#[test]
fn a_known_name_from_another_address_is_a_spoof_unless_it_passes() {
    let raw = "From: Ana Lee <ana@other.example>
Subject: Hi

Call me.
";
    assert_eq!(run(raw, &[], ANA, &[]), vec![Mark::Spoof]);
    let pass = format!(
        "Authentication-Results: mx.google.com; dkim=pass header.d=other.example; dmarc=pass header.from=other.example\n{raw}"
    );
    assert!(run(&pass, &[], ANA, &[]).is_empty());
    // The known address itself is not a spoof.
    let own = raw.replace("ana@other.example", "Ana@Example.org");
    assert!(run(&own, &[], ANA, &[]).is_empty());
}

#[test]
fn a_shared_name_of_two_known_people_is_not_a_spoof() {
    let known = &[
        ("Ana Lee", "ana@example.org"),
        ("Ana Lee", "ana.lee@example.net"),
    ];
    let raw = "From: Ana Lee <ana.lee@example.net>\nSubject: Hi\n\nCall me.\n";
    assert!(run(raw, &[], known, &[]).is_empty());
}

#[test]
fn an_encoded_known_name_is_compared_after_decoding() {
    let raw = "From: =?utf-8?B?QW5hIExlZQ==?= <ana@evil.example>\nSubject: Hi\n\nPay this.\n";
    let a = analysis(raw, &[], ANA, &[]);
    assert_eq!(a.head.from_name.as_deref(), Some("Ana Lee"));
    assert_eq!(a.marks, vec![Mark::Spoof]);
}

#[test]
fn a_comment_after_a_bare_address_is_the_display_name() {
    let raw = "From: ana@evil.example (Ana Lee)\nSubject: Hi\n\nPay this.\n";
    let a = analysis(raw, &[], ANA, &[]);
    assert_eq!(a.head.from_name.as_deref(), Some("Ana Lee"));
    assert_eq!(a.marks, vec![Mark::Spoof]);
}

#[test]
fn the_spam_label_is_a_mark() {
    let raw = "From: x@example.net\nSubject: Win\n\nWin now.\n";
    assert_eq!(run(raw, &["INBOX", "SPAM"], &[], &[]), vec![Mark::Spam]);
    assert!(run(raw, &["INBOX"], &[], &[]).is_empty());
}

#[test]
fn a_repeated_end_block_of_the_same_sender_goes_out() {
    let raw = "From: Bo Ren <bo@example.org>\nSubject: Re: Lease\n\nCan you call me today?\n\nBo Ren\nLease Office, 12 High Street\n";
    let earlier = "Here is the plan.\n\nBo Ren\nLease Office, 12 High Street";
    let a = analysis(raw, &[], &[], &[earlier]);
    assert_eq!(a.text.text.trim(), "Can you call me today?");
    // With no earlier text, the block stays.
    let a = analysis(raw, &[], &[], &[]);
    assert!(a.text.text.contains("12 High Street"));
}

#[test]
fn the_text_of_an_attachment_is_kept_with_its_name() {
    let raw = "From: Shop <shop@example.com>
Subject: Your papers
Content-Type: multipart/mixed; boundary=z

--z
Content-Type: text/plain

See the file.
--z
Content-Type: text/plain; name=invoice.txt
Content-Disposition: attachment; filename=invoice.txt

Invoice 77: total 50.00 USD.
--z--
";
    let a = analysis(raw, &[], &[], &[]);
    assert_eq!(a.text.attachments[0].0, "invoice.txt");
    assert!(a.text.attachment_text[0].1.contains("Invoice 77"));
    assert!(!a.text.text.contains("Invoice 77"));
}

#[test]
fn the_date_is_internal_date_then_the_header_then_now() {
    let raw = "From: a@example.org\nDate: Wed, 31 Dec 2025 09:00:00 +0000\nSubject: x\n\nHi.\n";
    let header = 1_767_171_600_000;
    let date = |raw: &str, internal: Option<i64>| {
        let lookup = |_: &str| Vec::new();
        let cx = Context {
            me: "owner@example.com",
            kind: Kind::Stranger,
            labels: &[],
            authserv: Some(iris_understand::head::GMAIL_AUTHSERV),
            internal_date: internal,
            now_ms: NOW,
            known_person: &lookup,
            existing: &[],
            thread: &[],
            same_sender: &[],
            known: &[],
        };
        analyze(&crlf(raw), &cx).date(&cx)
    };
    assert_eq!(date(raw, Some(42)), 42);
    assert_eq!(date(raw, None), header);
    let no_date = "From: a@example.org\nSubject: x\n\nHi.\n";
    assert_eq!(date(no_date, None), NOW);
    let huge =
        "From: a@example.org\nDate: 1 Jan 9223372036854775807 00:00 +0000\nSubject: x\n\nHi.\n";
    assert_eq!(date(huge, None), NOW);
}

#[test]
fn a_dmarc_result_needs_the_from_domain() {
    // A DMARC fail with no header.from domain is no evidence about From.
    let fail = "Authentication-Results: mx.google.com; dmarc=fail (p=REJECT)\nFrom: Shop <billing@shop.example>\nSubject: Pay\n\nPay now.\n";
    assert!(run(fail, &[], &[], &[]).is_empty());
    // A DMARC pass for another domain does not clear a spoof.
    let other = "Authentication-Results: mx.google.com; dkim=pass header.d=other.example; dmarc=pass header.from=else.example\nFrom: Ana Lee <ana@other.example>\nSubject: Hi\n\nCall me.\n";
    assert_eq!(run(other, &[], ANA, &[]), vec![Mark::Spoof]);
    let none = "Authentication-Results: mx.google.com; dkim=pass header.d=other.example; dmarc=pass\nFrom: Ana Lee <ana@other.example>\nSubject: Hi\n\nCall me.\n";
    assert_eq!(run(none, &[], ANA, &[]), vec![Mark::Spoof]);
}

#[test]
fn every_recipient_is_kept() {
    let to: Vec<String> = (0..250).map(|i| format!("p{i}@example.org")).collect();
    let raw = format!(
        "From: a@example.org\nTo: {}\nSubject: x\n\nHi.\n",
        to.join(", ")
    );
    let a = analysis(&raw, &[], &[], &[]);
    assert_eq!(a.head.to, to);
}

#[test]
fn attachment_text_after_32000_characters_is_kept() {
    let long = "word ".repeat(10_000);
    let raw = format!(
        "From: Shop <shop@example.com>
Subject: Papers
Content-Type: multipart/mixed; boundary=z

--z
Content-Type: text/plain

See the file.
--z
Content-Type: text/plain; name=invoice.txt
Content-Disposition: attachment; filename=invoice.txt

{long}
Total due: 77.00 USD.
--z--
"
    );
    let a = analysis(&raw, &[], &[], &[]);
    assert!(a.text.attachment_text[0]
        .1
        .contains("Total due: 77.00 USD."));
}

#[test]
fn the_cut_mark_stays_after_the_repeated_end_goes_out() {
    let filler: String = (0..4_000)
        .map(|i| format!("line {i} of the report\n"))
        .collect();
    let mk = |first: &str| {
        format!("From: Bo Ren <bo@example.org>\nSubject: Report\n\n{first}\n\n{filler}")
    };
    let earlier = iris_understand::Text::of(&crlf(&mk("The first report."))).text;
    assert!(earlier.ends_with(iris_clean::CUT_MARK));
    let a = analysis(&mk("The second report."), &[], &[], &[&earlier]);
    assert!(a.text.cut);
    assert!(a.text.text.starts_with("The second report."));
    assert!(
        a.text.text.ends_with(iris_clean::CUT_MARK),
        "{}",
        &a.text.text[..200.min(a.text.text.len())]
    );
}

/// HTML mail from a sender that passes DMARC for `evil.example.net`, with
/// one link that shows `www.shop.com` and goes to `target`.
fn html_link(target: &str) -> String {
    format!(
        "Authentication-Results: mx.google.com; dkim=pass header.i=@evil.example.net; dmarc=pass (p=NONE) header.from=evil.example.net
From: Shop <alerts@evil.example.net>
Subject: Your account
Content-Type: text/html; charset=utf-8

<p>Verify your account: <a href=\"{target}\">https://www.shop.com/account</a></p>
"
    )
}

#[test]
fn a_link_that_shows_a_brand_and_goes_elsewhere_is_a_mark() {
    for target in [
        "https://login.shop-secure.example.net/v",
        "https://shop.com.evil.example.net/v",
        "https://evil.example.net\\@www.shop.com/v",
        "https://192.0.2.7/v",
        "https://www.shop.com&#9;@evil.example.net/v",
        "https://www.shop.com&#10;@evil.example.net/v",
        "https:www.shop.com.evil.example.net/v",
        "https:\\\\evil.example.net\\@www.shop.com/v",
        " ht&#9;tps://www.shop.com@evil.example.net/v",
    ] {
        assert_eq!(
            run(&html_link(target), &[], &[], &[]),
            vec![Mark::Link],
            "{target}"
        );
        // Your sent mail never gets the mark.
        assert!(
            run(&html_link(target), &["SENT"], &[], &[]).is_empty(),
            "{target}"
        );
    }
    // A shown address across two lines.
    let lines = html_link("https://login.shop-secure.example.net/v").replace(
        "https://www.shop.com/account</a>",
        "https://www.shop.com<br>/account</a>",
    );
    assert_eq!(run(&lines, &[], &[], &[]), vec![Mark::Link]);
    // The real target of the shown address.
    assert!(run(&html_link("https://www.shop.com/account"), &[], &[], &[]).is_empty());
    // DMARC for its own domain proves nothing about the shown domain.
    assert_eq!(
        run(
            &html_link("https://click.evil.example.net/r/1"),
            &[],
            &[],
            &[]
        ),
        vec![Mark::Link]
    );
    // The proven owner of the shown domain can link through a click counter.
    let own =
        html_link("https://click.mailer.example.org/r/1").replace("evil.example.net", "shop.com");
    assert!(run(&own, &[], &[], &[]).is_empty(), "{own}");
}

/// HTML mail with no DMARC result and the body `body`.
fn html_body(body: &str) -> String {
    format!(
        "From: Shop <alerts@shop-mail.example.org>\nSubject: Your account\n\
         Content-Type: text/html; charset=utf-8\n\n{body}\n"
    )
}

#[test]
fn the_link_check_reads_the_text_that_a_person_sees() {
    for body in [
        "<a href=\"https://evil.example.net/\">https://bank.exa&#x200b;mple.com</a>",
        "<a href=\"https://evil.example.net/\">www.bank&shy;.example.com</a>",
        "<blockquote><a href=\"https://evil.example.net/\">https://bank.example.com<br>/login</a></blockquote>",
        "<blockquote><blockquote><a href=\"https://evil.example.net/\"><p>www.bank.example.com</p><p>/login</p></a></blockquote></blockquote>",
        "<ul><li><a href=\"https://evil.example.net/\"><b>https://bank.example.com</b></a></li></ul>",
        "<a href=\"https://evil.example.net/\">https://&#x202e;moc.elpmaxe.knab</a>",
        "<a href=\"https://evil.example.net/\">&nbsp;https://bank.example.com&nbsp;</a>",
        "<a href=\"https://evil.example.net/\">https://bank.<br>example.com/login</a>",
    ] {
        assert_eq!(run(&html_body(body), &[], &[], &[]), vec![Mark::Link], "{body}");
    }
    // The same texts with the real target.
    for body in [
        "<a href=\"https://bank.example.com/\">https://bank.exa&#x200b;mple.com</a>",
        "<blockquote><a href=\"https://bank.example.com/\">https://bank.example.com<br>/login</a></blockquote>",
    ] {
        assert!(run(&html_body(body), &[], &[], &[]).is_empty(), "{body}");
    }
}

#[test]
fn each_name_of_one_domain_is_the_same_domain() {
    // A header can name a domain in Unicode (`münchen.de`) and an
    // authentication result in punycode (`xn--mnchen-3ya.de`), or the
    // other way. Each check compares one form.
    let forms = [
        ("münchen.de", "xn--mnchen-3ya.de"),
        ("xn--mnchen-3ya.de", "münchen.de"),
        ("MÜNCHEN.de", "xn--mnchen-3ya.de"),
    ];
    for (from, auth) in forms {
        let head = format!(
            "Authentication-Results: mx.google.com; dkim=pass header.d={auth}; dmarc=pass header.from={auth}\n"
        );
        // The owner of the shown domain links through a click counter.
        let own = format!(
            "{head}From: Shop <alerts@{from}>\nSubject: Your account\nContent-Type: text/html; charset=utf-8\n\n\
             <p><a href=\"https://click.mailer.example.org/r/1\">https://www.münchen.de/konto</a></p>\n"
        );
        assert!(run(&own, &[], &[], &[]).is_empty(), "{from} {auth}");
        // A known name from another address that passes DMARC and DKIM for
        // its own domain is not a spoof.
        let name = format!("{head}From: Ana Lee <ana@{from}>\nSubject: Hi\n\nCall me.\n");
        assert!(run(&name, &[], ANA, &[]).is_empty(), "{from} {auth}");
    }
    // A DMARC fail for the punycode form is a fail for the Unicode form.
    let fail = "Authentication-Results: mx.google.com; dmarc=fail header.from=xn--mnchen-3ya.de\n\
                From: Shop <alerts@münchen.de>\nSubject: Hi\n\nHi.\n";
    assert_eq!(run(fail, &[], &[], &[]), vec![Mark::Dmarc]);
}

/// Decision D32 (issue 371): an address in the display name of another
/// owner than the `From` address is a spoof. List forms with `via` and
/// addresses of the same owner are not.
#[test]
fn an_address_in_the_name_of_another_owner_is_a_spoof() {
    let raw = |from: &str| format!("From: {from}\nSubject: Hi\n\nPay this.\n");
    for from in [
        "\"ceo@bank.example\" <x@evil.example.net>",
        "\"CEO <ceo@bank.example>\" <x@evil.example.net>",
        "\"Bank (support@bank.example)\" <x@evil.example.net>",
        "\"ceo\u{FF20}bank.example\" <x@evil.example.net>",
        "=?utf-8?q?ceo=40bank=2Eexample?= <x@evil.example.net>",
    ] {
        assert_eq!(run(&raw(from), &[], &[], &[]), vec![Mark::Spoof], "{from}");
    }
    for from in [
        "\"x@evil.example.net\" <x@evil.example.net>",
        "\"support@bank.example\" <noreply@mail.bank.example>",
        "\"ana@friends.example via Book Club\" <club@lists.example>",
        "\"ana@friends.example (via Book Club)\" <club@lists.example>",
        "\"ana@friends.example [Via: Book Club]\" <club@lists.example>",
        "\"Ana Lee\" <ana@friends.example>",
        "\"Ana @ home\" <ana@friends.example>",
        "\"twitter: @ana\" <ana@friends.example>",
    ] {
        assert!(run(&raw(from), &[], &[], &[]).is_empty(), "{from}");
    }
    // Your sent mail never gets the mark.
    let sent = raw("\"ceo@bank.example\" <x@evil.example.net>");
    assert!(run(&sent, &["SENT"], &[], &[]).is_empty());
}

/// A small xorshift generator, so a failure repeats.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

/// Property of the name check (input that Iris does not control): any name
/// with no at sign is never a spoof; a name that holds a foreign address,
/// with no `via`, always is; a name with the word `via` between any
/// separators never is; the `From` address itself never is.
#[test]
fn the_name_address_check_holds_for_any_name() {
    use iris_understand::name_address_spoof as spoof;
    let parts = [
        "Ana", "Lee", " ", "\"", "<", ">", "(", ")", ",", ";", ":", ".", "!", "-", "é", "\u{202E}",
        "\t", "x.y", "[", "]", "'", "`", "?",
    ];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..4000 {
        let n = (rng.next() % 12) as usize;
        let noise: String = (0..n).map(|_| rng.pick(&parts)).collect();
        let from = "x@evil.example.net";
        assert!(!spoof(&noise, from), "{noise:?}");
        assert!(
            !spoof(&format!("{noise} {from} {noise}"), from),
            "{noise:?}"
        );
        let sep = rng.pick(&[" ", "\"", "<", "(", ",", ";", ":"]);
        let name = format!("{noise}{sep}ceo@bank.example{sep}{noise}");
        let has_via = name
            .split_whitespace()
            .any(|w| w.eq_ignore_ascii_case("via"));
        assert_eq!(spoof(&name, from), !has_via, "{name:?}");
        let (a, b) = (
            rng.pick(&[" ", "(", "[", "\"", ","]),
            rng.pick(&[" ", ")", "]", ":", "."]),
        );
        let list = format!("{noise}ceo@bank.example {a}VIA{b} {noise}");
        assert!(!spoof(&list, from), "{list:?}");
    }
}
