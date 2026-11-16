//! Memory and time bounds for one big message. This file is its own test
//! program, so the counting allocator sees only this test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let n = NOW.fetch_add(l.size(), Relaxed) + l.size();
        PEAK.fetch_max(n, Relaxed);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        NOW.fetch_sub(l.size(), Relaxed);
        System.dealloc(p, l)
    }
}

#[global_allocator]
static A: Counting = Counting;

/// A message with a big HTML body and a big attachment.
fn message(html_bytes: usize, attachment_bytes: usize) -> Vec<u8> {
    let mut m = b"Subject: Big\r\nContent-Type: multipart/mixed; boundary=\"m\"\r\n\r\n\
                  --m\r\nContent-Type: text/html\r\n\r\n"
        .to_vec();
    let row = b"<tr><td style=\"padding:4px\">Item <a href=\"https://s.example.com/?utm_a=1\">link</a></td></tr>\r\n";
    while m.len() < html_bytes {
        m.extend_from_slice(row);
    }
    m.extend_from_slice(b"--m\r\nContent-Type: application/pdf; name=\"a.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\n");
    let line = [b'Q'; 76];
    for _ in 0..attachment_bytes / 57 {
        m.extend_from_slice(&line);
        m.extend_from_slice(b"\r\n");
    }
    m.extend_from_slice(b"--m--\r\n");
    m
}

/// The peak of new heap memory during one call, in bytes.
fn peak_of(raw: &[u8]) -> (usize, Duration, iris_clean::Clean) {
    let base = NOW.load(Relaxed);
    PEAK.store(base, Relaxed);
    let start = Instant::now();
    let c = iris_clean::clean(raw);
    (PEAK.load(Relaxed) - base, start.elapsed(), c)
}

#[test]
fn memory_and_time_stay_bounded() {
    let small = message(200_000, 1_000_000);
    let big = message(6_000_000, 30_000_000);
    let (small_peak, _, _) = peak_of(&small);
    let (big_peak, time, c) = peak_of(&big);
    eprintln!("peak: {small_peak} bytes (small), {big_peak} bytes (big), time {time:?}");
    assert!(c.cut && c.text.ends_with(iris_clean::CUT_MARK));
    assert_eq!(c.attachments[0].size, 30_000_000 / 57 * 57);
    // One body part decodes at most 2 MiB. The steps hold a few copies of it at most. Measured: about 5 MiB.
    assert!(big_peak < 8 * 1024 * 1024, "peak {big_peak}");
    // The size of an attachment does not change the memory use.
    assert!(big_peak < small_peak + 12 * 1024 * 1024);
    // Debug builds are slow. The limit holds in a debug build on a small machine.
    assert!(time < Duration::from_secs(10), "time {time:?}");

    // A big attached message in base64: the parser decodes only its start.
    let inner = message(100_000, 20_000_000);
    let mut b64 = String::new();
    for chunk in inner.chunks(57) {
        b64.push_str(&b64_line(chunk));
        b64.push_str("\r\n");
    }
    let raw = format!(
        "Subject: Fwd\r\nContent-Type: multipart/mixed; boundary=o\r\n\r\n--o\r\n\
         Content-Type: message/rfc822\r\nContent-Transfer-Encoding: base64\r\n\r\n{b64}--o--\r\n"
    );
    drop(inner);
    let (peak, _, c) = peak_of(raw.as_bytes());
    eprintln!("peak: {peak} bytes (attached message)");
    assert_eq!(c.attachments[0].mime_type, "message/rfc822");
    assert!(peak < 8 * 1024 * 1024, "peak {peak}");

    // A PDF of 9 MiB is decoded for its text. One of 30 MB is not.
    let mut raw = message(1000, 0);
    raw.truncate(raw.len() - b"--m--\r\n".len());
    let pdf = format!("%PDF-1.4\n{}", "x".repeat(9 * 1024 * 1024));
    for chunk in pdf.as_bytes().chunks(57) {
        raw.extend_from_slice(b64_line(chunk).as_bytes());
        raw.extend_from_slice(b"\r\n");
    }
    raw.extend_from_slice(b"--m--\r\n");
    let (peak, _, c) = peak_of(&raw);
    eprintln!("peak: {peak} bytes (PDF of 9 MiB)");
    assert_eq!(c.attachments[0].text, None);
    assert!(peak < 16 * 1024 * 1024, "peak {peak}");

    // A PDF with one raw stream of 9 MiB: the stream is read in place.
    let mut raw = message(1000, 0);
    raw.truncate(raw.len() - b"--m--\r\n".len());
    let stream = format!("BT (Hello raw words) Tj ET{}", " ".repeat(9 * 1024 * 1024));
    let pdf = format!(
        "%PDF-1.4\n1 0 obj\n<< /Length {} >>\nstream\n{stream}\nendstream\nendobj\n",
        stream.len()
    );
    drop(stream);
    for chunk in pdf.as_bytes().chunks(57) {
        raw.extend_from_slice(b64_line(chunk).as_bytes());
        raw.extend_from_slice(b"\r\n");
    }
    drop(pdf);
    raw.extend_from_slice(b"--m--\r\n");
    let (peak, _, c) = peak_of(&raw);
    eprintln!("peak: {peak} bytes (PDF with a raw stream of 9 MiB)");
    // The reader stops at the stream limit, so the text shows a cut.
    let want = format!("Hello raw words{}", iris_clean::CUT_MARK);
    assert_eq!(c.attachments[0].text.as_deref(), Some(want.as_str()));
    assert!(peak < 12 * 1024 * 1024, "peak {peak}");

    // A PDF stream of 4 MiB with many short text lines.
    let mut raw = message(1000, 0);
    raw.truncate(raw.len() - b"--m--\r\n".len());
    let stream = "BT (ab) Tj T* (cd) Tj ET\n".repeat(4 * 1024 * 1024 / 25);
    let pdf = format!(
        "%PDF-1.4\n1 0 obj\n<< /Length {} >>\nstream\n{stream}\nendstream\nendobj\n",
        stream.len()
    );
    drop(stream);
    for chunk in pdf.as_bytes().chunks(57) {
        raw.extend_from_slice(b64_line(chunk).as_bytes());
        raw.extend_from_slice(b"\r\n");
    }
    drop(pdf);
    raw.extend_from_slice(b"--m--\r\n");
    let (peak, _, c) = peak_of(&raw);
    eprintln!("peak: {peak} bytes (PDF with many short lines)");
    let t = c.attachments[0].text.as_deref().unwrap_or_default();
    assert!(
        t.len() <= iris_clean::ATTACHMENT_TEXT_LIMIT + 64,
        "{}",
        t.len()
    );
    assert!(peak < 10 * 1024 * 1024, "peak {peak}");

    // A PDF stream of 4 MiB with many short strings before one operator.
    let mut raw = message(1000, 0);
    raw.truncate(raw.len() - b"--m--\r\n".len());
    let stream = format!("BT {} Tj ET", "(a) ".repeat(4 * 1024 * 1024 / 4 - 4));
    let pdf = format!(
        "%PDF-1.4\n1 0 obj\n<< /Length {} >>\nstream\n{stream}\nendstream\nendobj\n",
        stream.len()
    );
    drop(stream);
    for chunk in pdf.as_bytes().chunks(57) {
        raw.extend_from_slice(b64_line(chunk).as_bytes());
        raw.extend_from_slice(b"\r\n");
    }
    drop(pdf);
    raw.extend_from_slice(b"--m--\r\n");
    let (peak, _, _) = peak_of(&raw);
    eprintln!("peak: {peak} bytes (PDF with many strings before one operator)");
    assert!(peak < 10 * 1024 * 1024, "peak {peak}");

    // A PDF with a dense code map of 4 MiB: the map entries have a budget.
    let mut raw = message(1000, 0);
    raw.truncate(raw.len() - b"--m--\r\n".len());
    let mut cmap =
        b"begincmap 1 begincodespacerange <0000> <FFFF> endcodespacerange beginbfchar".to_vec();
    let mut n = 0u32;
    while cmap.len() < 4 * 1024 * 1024 - 64 {
        cmap.extend(format!(" <{:04X}> <0041>", n % 0x10000).bytes());
        n += 1;
    }
    cmap.extend_from_slice(b" endbfchar endcmap");
    let content = b"BT /F1 9 Tf <0001000200030004> Tj ET";
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&cmap, 6);
    let mut pdf = format!(
        "%PDF-1.4\n2 0 obj\n<< /Type /Page /Contents 3 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n\
         5 0 obj\n<< /Subtype /Type0 /ToUnicode 6 0 R >>\nendobj\n\
         3 0 obj\n<< /Length {} >>\nstream\n",
        content.len()
    )
    .into_bytes();
    pdf.extend_from_slice(content);
    pdf.extend(
        format!(
            "\nendstream\nendobj\n6 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
            z.len()
        )
        .bytes(),
    );
    pdf.extend(z);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    for chunk in pdf.chunks(57) {
        raw.extend_from_slice(b64_line(chunk).as_bytes());
        raw.extend_from_slice(b"\r\n");
    }
    drop(pdf);
    raw.extend_from_slice(b"--m--\r\n");
    let (peak, _, c) = peak_of(&raw);
    eprintln!("peak: {peak} bytes (PDF with a dense code map)");
    assert_eq!(c.attachments[0].text.as_deref(), Some("AAAA"));
    assert!(peak < 12 * 1024 * 1024, "peak {peak}");

    // A code map of 4 MiB of hex strings that make no entry.
    let mut raw = message(1000, 0);
    raw.truncate(raw.len() - b"--m--\r\n".len());
    let mut cmap = b"begincmap ".to_vec();
    while cmap.len() < 4 * 1024 * 1024 - 64 {
        cmap.extend_from_slice(b"<> ");
    }
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&cmap, 6);
    let mut pdf = format!(
        "%PDF-1.4\n6 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
        z.len()
    )
    .into_bytes();
    pdf.extend(z);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    for chunk in pdf.chunks(57) {
        raw.extend_from_slice(b64_line(chunk).as_bytes());
        raw.extend_from_slice(b"\r\n");
    }
    drop(pdf);
    raw.extend_from_slice(b"--m--\r\n");
    let (peak, _, _) = peak_of(&raw);
    eprintln!("peak: {peak} bytes (PDF with a code map of empty hex strings)");
    assert!(peak < 12 * 1024 * 1024, "peak {peak}");

    // Many tiny code maps with no entry: each map costs its budget.
    let mut raw = message(1000, 0);
    raw.truncate(raw.len() - b"--m--\r\n".len());
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut n = 1;
    while pdf.len() < 9 * 1024 * 1024 {
        pdf.extend(format!("{n} 0 obj<</Length 9>>stream\nbegincmap\nendstream endobj\n").bytes());
        n += 1;
    }
    for chunk in pdf.chunks(57) {
        raw.extend_from_slice(b64_line(chunk).as_bytes());
        raw.extend_from_slice(b"\r\n");
    }
    drop(pdf);
    raw.extend_from_slice(b"--m--\r\n");
    let (peak, _, _) = peak_of(&raw);
    eprintln!("peak: {peak} bytes (PDF with many empty code maps)");
    assert!(peak < 16 * 1024 * 1024, "peak {peak}");

    // A code map with one source code of 4 MiB.
    let mut raw = message(1000, 0);
    raw.truncate(raw.len() - b"--m--\r\n".len());
    let mut cmap = b"begincmap 1 beginbfchar <".to_vec();
    cmap.resize(4 * 1024 * 1024 - 64, b'A');
    cmap.extend_from_slice(b"> <0041> endbfchar endcmap");
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&cmap, 6);
    drop(cmap);
    let mut pdf = format!(
        "%PDF-1.4\n6 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
        z.len()
    )
    .into_bytes();
    pdf.extend(z);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    for chunk in pdf.chunks(57) {
        raw.extend_from_slice(b64_line(chunk).as_bytes());
        raw.extend_from_slice(b"\r\n");
    }
    drop(pdf);
    raw.extend_from_slice(b"--m--\r\n");
    let (peak, _, _) = peak_of(&raw);
    eprintln!("peak: {peak} bytes (PDF with one long source code)");
    assert!(peak < 6 * 1024 * 1024, "peak {peak}");

    // A plain body part of 2 MiB of empty lines and of short quote lines.
    for line in ["\n", "> a\n", "-- \n"] {
        let mut raw = b"Subject: Re: Lines\r\nIn-Reply-To: <a@b>\r\n\r\nHi\n".to_vec();
        raw.extend(line.repeat(2 * 1024 * 1024 / line.len()).bytes());
        let (peak, _, _) = peak_of(&raw);
        eprintln!("peak: {peak} bytes (2 MiB of {line:?} lines)");
        assert!(peak < 12 * 1024 * 1024, "peak {peak}");
    }

    // Base64 with a few characters and very much white space.
    let mut raw = message(1000, 0);
    raw.truncate(raw.len() - b"--m--\r\n".len());
    raw.extend_from_slice(b"JVBERi0x");
    raw.extend(std::iter::repeat_n(b' ', 20_000_000));
    raw.extend_from_slice(b"\r\n--m--\r\n");
    let (peak, _, _) = peak_of(&raw);
    eprintln!("peak: {peak} bytes (base64 with white space)");
    assert!(peak < 1024 * 1024, "peak {peak}");

    // A very large header block.
    let raw = format!(
        "Subject: {}\r\n{}Content-Type: text/plain\r\n\r\nbody",
        "s".repeat(5_000_000),
        " fold\r\n".repeat(500_000)
    );
    let (peak, _, c) = peak_of(raw.as_bytes());
    eprintln!("peak: {peak} bytes (large headers)");
    assert_eq!(c.text, "body");
    assert!(peak < 1024 * 1024, "peak {peak}");

    // One HTML tag with very many attributes.
    let raw = format!(
        "Subject: Attrs\r\nContent-Type: text/html\r\n\r\n<p {}>text</p>",
        "a=b ".repeat(400_000)
    );
    let (peak, _, c) = peak_of(raw.as_bytes());
    eprintln!("peak: {peak} bytes (many attributes)");
    assert_eq!(c.text, "text");
    assert!(peak < 4 * 1024 * 1024, "peak {peak}");

    // Many small parts: the split stops at the part limit.
    let mut raw =
        String::from("Subject: Parts\r\nContent-Type: multipart/mixed; boundary=p\r\n\r\n");
    for _ in 0..200_000 {
        raw.push_str("--p\r\n\r\nx\r\n");
    }
    let (peak, _, c) = peak_of(raw.as_bytes());
    eprintln!("peak: {peak} bytes (200,000 parts)");
    assert!(c.cut);
    assert!(peak < 1024 * 1024, "peak {peak}");
}

fn b64_line(chunk: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for g in chunk.chunks(3) {
        let n = (g[0] as u32) << 16
            | (*g.get(1).unwrap_or(&0) as u32) << 8
            | *g.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= g.len() {
                out.push(A[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
