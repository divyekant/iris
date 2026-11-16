//! The text of a PDF file. Bounded, and safe on bad input.
//!
//! The release build stops the program on a panic (`panic = "abort"`), and a
//! PDF attachment can come from an attacker. So this module does not use a
//! full PDF crate. It reads the PDF once from the start. On the way, it reads
//! the text operators `Tj`, `TJ`, `'` and `"` of the content streams (raw or
//! `FlateDecode`), and it keeps the small object dictionaries, the
//! dictionaries of object streams and the `ToUnicode` code maps. The fonts of
//! a page often come after its content, so the strings wait in one bounded
//! buffer. At the end, each string goes through the code map of its font. A
//! string of a font without a map reads as Windows-1252, the encoding of the
//! standard PDF fonts. A CID font (`/Type0`) without a map gives no text.

use std::collections::{BTreeMap, HashMap};

/// The most bytes of one decompressed stream.
const STREAM_LIMIT: usize = 4 * 1024 * 1024;
/// The most bytes of decompressed streams for one PDF.
const TOTAL_LIMIT: usize = 32 * 1024 * 1024;
/// The most bytes of one stream dictionary.
const DICT_LIMIT: usize = 4096;
/// The most bytes of one object that the reader keeps for its dictionary.
const OBJ_LIMIT: usize = 16 * 1024;
/// The most bytes of kept objects for one PDF, with 64 more for each object.
const OBJ_BUDGET: usize = 4 * 1024 * 1024;
/// The most code map entries for one PDF, with one more for each UTF-16 unit.
const MAP_BUDGET: usize = 1024 * 1024;
/// The most fonts (a content stream and a font name) for one PDF.
const FONT_LIMIT: usize = 4096;

// The tags of the string buffer. A byte from 1 to `CHUNK` is the count of
// string bytes that come after it.
const CHUNK: u8 = 0xFB;
/// The end of the strings of one operator.
const END: u8 = 0xFC;
/// A font change. Two bytes of font number (little-endian) come after it.
const FONT: u8 = 0xFD;
const SPACE: u8 = 0xFE;
const LINE: u8 = 0xFF;
/// The font number for a font that the reader cannot name.
const NO_FONT: u16 = u16::MAX;

/// True if the first 1 KiB holds the PDF marker `%PDF`.
pub fn is_pdf(start: &[u8]) -> bool {
    find(&start[..start.len().min(1024)], b"%PDF").is_some()
}

/// The text, at most `limit` bytes, and true if the reader stopped at a limit
/// before the end. `None` if the file has no text that this module can read.
/// Trimmed white space can make the text shorter than the limit.
pub fn text_cut(pdf: &[u8], limit: usize) -> (Option<String>, bool) {
    if !is_pdf(pdf) {
        return (None, false);
    }
    let mut doc = Doc::new(limit);
    let mut stopped = false;
    let mut total = 0;
    let mut at = 0;
    while let Some(item) = next_item(pdf, at) {
        let (kw, start, header) = match item {
            Item::Object { num, body, end } => {
                doc.keep(num, &pdf[body.0..body.1]);
                at = end;
                continue;
            }
            Item::Stream { kw, start, header } => (kw, start, header),
        };
        let num = header.and_then(|h| h.1);
        // The dictionary is the text from the object header to the keyword.
        // A longer dictionary than 4 KiB is not read.
        let dict = header
            .map(|h| h.0)
            .filter(|&h| kw - h <= DICT_LIMIT)
            .map_or(&b""[..], |h| &pdf[h..kw]);
        // A direct `/Length` gives the end, so `endstream` inside the data
        // does not cut the stream. Without it, search for the keyword.
        let len = match declared_len(dict).filter(|&n| ends_at(pdf, start, n)) {
            Some(n) => n,
            None => match find(&pdf[start..], b"endstream") {
                Some(n) => n,
                None => break,
            },
        };
        let data = &pdf[start..start + len];
        at = start + len;
        let objects = get(dict, b"/Type") == Some(Val::Name(b"/ObjStm"));
        if !objects && !is_content(dict) {
            continue;
        }
        // Each stream gets at most the rest of the budget of the PDF.
        let cap = STREAM_LIMIT.min(TOTAL_LIMIT - total);
        let mut more = false;
        let decoded: std::borrow::Cow<[u8]> = if only_flate(dict) {
            match miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, cap) {
                Ok(v) => v.into(),
                Err(e) => {
                    // More output than the limit: text after it is not read.
                    more = e.status == miniz_oxide::inflate::TINFLStatus::HasMoreOutput;
                    e.output.into()
                }
            }
        } else if direct_name(dict, b"/Filter", true).is_some() {
            continue;
        } else {
            // A raw stream is read in place, and only up to the stream limit.
            more = data.len() > cap;
            data[..data.len().min(cap)].into()
        };
        total += decoded.len();
        if objects {
            doc.object_stream(dict, &decoded);
        } else if find(&decoded, b"begincmap").is_some() {
            if let Some(n) = num {
                doc.add_map(n, &decoded);
            }
        } else {
            stopped |= more;
            if !doc.full && find(&decoded, b"BT").is_some() {
                // A form keeps its fonts in its own dictionary.
                if let Some(n) = num.filter(|_| get(dict, b"/Resources").is_some()) {
                    doc.keep(n, dict);
                }
                content_text(&decoded, num, &mut doc);
            }
        }
        if total >= TOTAL_LIMIT {
            stopped = true;
            break;
        }
    }
    let (out, full) = doc.text(limit);
    stopped |= doc.full || full;
    // Join the lines that are not empty, without a list of all lines.
    let mut text = String::with_capacity(out.len());
    for l in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(l);
    }
    drop(out);
    (readable(&text).then_some(text), stopped)
}

/// The position after the literal string that starts at `i` (a `(`), with
/// nested parentheses and `\` escapes. The end of the input if it does not close.
fn skip_string(b: &[u8], mut i: usize) -> usize {
    let mut open = 0usize;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'(' => open += 1,
            b')' => {
                open -= 1;
                if open == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    b.len()
}

/// The position of the line end after the comment that starts at `i` (a `%`).
fn skip_comment(b: &[u8], i: usize) -> usize {
    b[i..]
        .iter()
        .position(|&c| matches!(c, b'\r' | b'\n'))
        .map_or(b.len(), |n| i + n)
}

/// One item of the forward pass.
enum Item {
    /// An object without a stream: its number, the span of its body, and
    /// the position after `endobj`.
    Object {
        num: u32,
        body: (usize, usize),
        end: usize,
    },
    /// A `stream` keyword: its position, the start of its data, and the end
    /// of the last object header before it, with the object number.
    Stream {
        kw: usize,
        start: usize,
        header: Option<(usize, Option<u32>)>,
    },
}

/// The next object or `stream` keyword outside strings and comments, from
/// `at`. One forward pass: each byte is read once.
fn next_item(pdf: &[u8], mut at: usize) -> Option<Item> {
    let delimiter = |b: u8| b.is_ascii_whitespace() || b"()<>[]{}/%".contains(&b);
    let mut header = None;
    while at < pdf.len() {
        match pdf[at] {
            b'(' => at = skip_string(pdf, at),
            b'%' => at = skip_comment(pdf, at),
            b'<' if pdf.get(at + 1) != Some(&b'<') => {
                // A hex string.
                at = pdf[at..]
                    .iter()
                    .position(|&c| c == b'>')
                    .map_or(pdf.len(), |n| at + n + 1);
            }
            c if !delimiter(c) => {
                let end = pdf[at..]
                    .iter()
                    .position(|&c| delimiter(c))
                    .map_or(pdf.len(), |n| at + n);
                let word = &pdf[at..end];
                if word == b"obj" {
                    if let Some(num) = object_number(&pdf[..at]) {
                        header = Some((end, num));
                    }
                } else if word == b"endobj" {
                    if let Some((h, Some(num))) = header.take() {
                        return Some(Item::Object {
                            num,
                            body: (h, at),
                            end,
                        });
                    }
                } else if word == b"stream" {
                    // The keyword `stream` ends with a line end.
                    match &pdf[end..] {
                        [b'\r', b'\n', ..] => {
                            return Some(Item::Stream {
                                kw: at,
                                start: end + 2,
                                header,
                            })
                        }
                        [b'\n', ..] => {
                            return Some(Item::Stream {
                                kw: at,
                                start: end + 1,
                                header,
                            })
                        }
                        _ => {}
                    }
                }
                at = end;
            }
            _ => at += 1,
        }
    }
    None
}

/// The position of a name (for example `/Length`) in the outer dictionary.
/// A name inside a string, a comment or an inner dictionary does not count.
/// With `exact`, a longer name does not match: `/Length1` is not `/Length`.
fn direct_name(dict: &[u8], name: &[u8], exact: bool) -> Option<usize> {
    let key_end = |b: &u8| b.is_ascii_whitespace() || b"()<>[]{}/%".contains(b);
    let (mut i, mut depth) = (0, 0usize);
    while i < dict.len() {
        match dict[i] {
            b'(' => i = skip_string(dict, i) - 1,
            b'%' => i = skip_comment(dict, i) - 1,
            b'<' if dict.get(i + 1) == Some(&b'<') => {
                depth += 1;
                i += 1;
            }
            b'>' if dict.get(i + 1) == Some(&b'>') => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'/' if depth <= 1
                && dict[i..].starts_with(name)
                && (!exact || dict.get(i + name.len()).is_none_or(key_end)) =>
            {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The value of a direct `/Length` entry. `None` for a reference (`5 0 R`).
fn declared_len(dict: &[u8]) -> Option<usize> {
    let p = direct_name(dict, b"/Length", true)?;
    let rest = &dict[p + 7..];
    let digits = rest.iter().skip_while(|b| b.is_ascii_whitespace());
    let num: Vec<u8> = digits
        .clone()
        .take_while(|b| b.is_ascii_digit())
        .copied()
        .collect();
    let after: Vec<u8> = digits.skip(num.len()).take(8).copied().collect();
    let is_ref = after
        .split(|b| b.is_ascii_whitespace())
        .filter(|w| !w.is_empty())
        .nth(1)
        .is_some_and(|w| w.starts_with(b"R"));
    if num.is_empty() || num.len() > 12 || is_ref {
        return None;
    }
    std::str::from_utf8(&num).ok()?.parse().ok()
}

/// True if the stream data of `len` bytes from `start` ends before `endstream`.
fn ends_at(pdf: &[u8], start: usize, len: usize) -> bool {
    start
        .checked_add(len)
        .and_then(|end| pdf.get(end..))
        .is_some_and(|tail| {
            let tail = &tail[..tail.len().min(16)];
            let p = tail
                .iter()
                .position(|b| !b.is_ascii_whitespace())
                .unwrap_or(tail.len());
            tail[p..].starts_with(b"endstream")
        })
}

/// `Some` if `before` ends with `<number> <number> ` (the start of an object
/// header), with the object number if it fits in a `u32`.
fn object_number(before: &[u8]) -> Option<Option<u32>> {
    let mut rest = before;
    let mut first = &b""[..];
    for _ in 0..2 {
        let ws = rest
            .iter()
            .rev()
            .take_while(|b| b.is_ascii_whitespace())
            .count();
        if ws == 0 {
            return None;
        }
        rest = &rest[..rest.len() - ws];
        let digits = rest.iter().rev().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        first = &rest[rest.len() - digits..];
        rest = &rest[..rest.len() - digits];
    }
    rest.last()
        .is_none_or(|b| b.is_ascii_whitespace())
        .then(|| std::str::from_utf8(first).ok()?.parse().ok())
}

/// False for streams that are not page content: images, fonts, metadata,
/// cross-reference streams and object streams.
fn is_content(dict: &[u8]) -> bool {
    ![
        &b"/Image"[..],
        b"/Length1",
        b"/Length2",
        b"/Metadata",
        b"/XRef",
        b"/ObjStm",
        b"/FontFile",
        b"/EmbeddedFile",
    ]
    .iter()
    .any(|k| direct_name(dict, k, false).is_some())
}

/// True if the `/Filter` of the dictionary is `FlateDecode` and nothing
/// else: `/FlateDecode` or `[/FlateDecode]`.
fn only_flate(dict: &[u8]) -> bool {
    let Some(p) = direct_name(dict, b"/Filter", true) else {
        return false;
    };
    let rest = &dict[p + 7..];
    let rest = &rest[rest.iter().take_while(|b| b.is_ascii_whitespace()).count()..];
    let value = match rest.first() {
        Some(b'[') => &rest[1..rest.iter().position(|&b| b == b']').unwrap_or(rest.len())],
        _ => {
            let end = rest
                .iter()
                .skip(1)
                .position(|&b| b.is_ascii_whitespace() || b"/<>[]()".contains(&b))
                .map_or(rest.len(), |n| n + 1);
            &rest[..end]
        }
    };
    let names: Vec<&[u8]> = value
        .split(|&b| b.is_ascii_whitespace() || b == b'/')
        .filter(|w| !w.is_empty())
        .collect();
    names == [b"FlateDecode"]
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Read the text operators of one content stream (object `num`) into the
/// string buffer of `doc`.
fn content_text(c: &[u8], num: Option<u32>, doc: &mut Doc) {
    // The string operands since the last operator, in one buffer. The string
    // buffer can hold at most `doc.cap` bytes, so more bytes have no use.
    let mut strings: Vec<u8> = Vec::new();
    let mut in_array = false;
    let cap = doc.cap;
    let add = |strings: &mut Vec<u8>, s: &[u8]| {
        let room = cap.saturating_sub(strings.len());
        strings.extend_from_slice(&s[..s.len().min(room)]);
    };
    let mut last_num = 0.0f64;
    let mut last_name: Option<&[u8]> = None;
    // The contents of one page can be an array of streams: the font of the
    // stream before stays.
    if let Some(name) = doc.name.take() {
        doc.set_font(num, &name);
        doc.name = Some(name);
    }
    // A viewer shows text only inside a text object (`BT` to `ET`).
    let mut in_text = false;
    // Text rendering modes 3 and 7 draw nothing a person can see.
    let mut invisible = false;
    let mut i = 0;
    while i < c.len() && !doc.full {
        match c[i] {
            b'(' => {
                let (s, n) = literal(&c[i..]);
                i += n;
                add(&mut strings, &s);
            }
            b'<' if c.get(i + 1) == Some(&b'<') => i += 2,
            b'<' => {
                let end = c[i..]
                    .iter()
                    .position(|&b| b == b'>')
                    .map_or(c.len(), |n| i + n);
                let s = hex(&c[i + 1..end]);
                i = (end + 1).min(c.len());
                add(&mut strings, &s);
            }
            b'[' => {
                in_array = true;
                i += 1;
            }
            b']' => {
                in_array = false;
                i += 1;
            }
            b'%' => {
                i = c[i..]
                    .iter()
                    .position(|&b| b == b'\n' || b == b'\r')
                    .map_or(c.len(), |n| i + n);
            }
            b'/' => {
                let end = c[i + 1..]
                    .iter()
                    .position(|&b| is_delimiter(b))
                    .map_or(c.len(), |n| i + 1 + n);
                // A PDF name has at most 127 bytes.
                last_name = Some(&c[i..end]).filter(|n| n.len() <= 128);
                i = end;
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => {
                let s = i;
                i += 1;
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == b'.') {
                    i += 1;
                }
                let n: f64 = std::str::from_utf8(&c[s..i])
                    .ok()
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(0.0);
                // In a `TJ` array, a large negative number moves to the next word.
                if in_array && n < -200.0 {
                    add(&mut strings, b" ");
                } else {
                    last_num = n;
                }
            }
            b if b.is_ascii_alphabetic() || b == b'\'' || b == b'"' || b == b'*' => {
                let s = i;
                while i < c.len()
                    && (c[i].is_ascii_alphanumeric() || matches!(c[i], b'\'' | b'"' | b'*'))
                {
                    i += 1;
                }
                match &c[s..i] {
                    b"BT" => in_text = true,
                    b"Tf" => {
                        doc.name = last_name.map(<[u8]>::to_vec);
                        doc.set_font(num, last_name.unwrap_or_default());
                    }
                    b"Tr" => invisible = last_num == 3.0 || last_num == 7.0,
                    b"Tj" | b"TJ" if in_text && !invisible => doc.strings(&strings),
                    b"'" | b"\"" if in_text && !invisible => {
                        doc.line();
                        doc.strings(&strings);
                    }
                    b"Td" | b"TD" => {
                        if last_num != 0.0 {
                            doc.line();
                        } else {
                            doc.space();
                        }
                    }
                    b"ET" => {
                        in_text = false;
                        doc.line();
                    }
                    b"T*" | b"Tm" => doc.line(),
                    _ => {}
                }
                strings.clear();
            }
            _ => i += 1,
        }
    }
}

fn is_delimiter(b: u8) -> bool {
    b.is_ascii_whitespace() || b"()<>[]{}/%".contains(&b)
}

/// What the forward pass keeps: the small objects, the code maps, the fonts
/// and the strings of the content streams.
struct Doc {
    objs: BTreeMap<u32, Vec<u8>>,
    obj_bytes: usize,
    maps: BTreeMap<u32, CMap>,
    map_left: usize,
    /// The font number of each content stream and font name.
    font_ids: HashMap<(Option<u32>, Vec<u8>), u16>,
    /// The font name of the last `Tf`.
    name: Option<Vec<u8>>,
    /// The tagged strings: see `CHUNK`.
    buf: Vec<u8>,
    cap: usize,
    /// True if a string did not fit in the buffer.
    full: bool,
    last: u8,
    font: u16,
}

impl Doc {
    fn new(limit: usize) -> Doc {
        Doc {
            objs: BTreeMap::new(),
            obj_bytes: 0,
            maps: BTreeMap::new(),
            map_left: MAP_BUDGET,
            font_ids: HashMap::new(),
            name: None,
            buf: Vec::new(),
            // Two bytes of codes give at least one byte of text in most
            // fonts, so this holds the text of the limit.
            cap: limit.saturating_mul(2).saturating_add(64),
            full: false,
            last: LINE,
            font: NO_FONT,
        }
    }

    /// Keep the body of object `num`. A later object of the same number
    /// replaces it, as in an incremental update.
    fn keep(&mut self, num: u32, body: &[u8]) {
        let cost = body.len() + 64;
        if body.len() > OBJ_LIMIT || self.obj_bytes + cost > OBJ_BUDGET {
            self.objs.remove(&num);
            return;
        }
        self.obj_bytes += cost;
        self.objs.insert(num, body.to_vec());
    }

    /// Keep the objects of one decoded object stream.
    fn object_stream(&mut self, dict: &[u8], data: &[u8]) {
        let (Some(Val::Int(n)), Some(Val::Int(first))) = (get(dict, b"/N"), get(dict, b"/First"))
        else {
            return;
        };
        let Some(first) = usize::try_from(first).ok().filter(|&f| f <= data.len()) else {
            return;
        };
        let mut lx = Lexer::new(&data[..first]);
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for _ in 0..n.clamp(0, 100_000) {
            let (Some(Tok::Word(a)), Some(Tok::Word(b))) = (lx.next(), lx.next()) else {
                break;
            };
            let (Some(a), Some(b)) = (int(a), int(b)) else {
                break;
            };
            let (Ok(a), Ok(b)) = (u32::try_from(a), usize::try_from(b)) else {
                break;
            };
            offsets.push((a, first.saturating_add(b)));
        }
        for (k, &(num, s)) in offsets.iter().enumerate() {
            let e = offsets
                .get(k + 1)
                .map_or(data.len(), |o| o.1)
                .min(data.len());
            if s < e {
                self.keep(num, &data[s..e]);
            }
        }
    }

    fn add_map(&mut self, num: u32, data: &[u8]) {
        let map = CMap::parse(data, &mut self.map_left);
        self.maps.insert(num, map);
    }

    /// Add `b` to the string buffer, or mark the buffer full.
    fn push(&mut self, b: &[u8]) -> bool {
        if self.full || self.buf.len() + b.len() > self.cap {
            self.full = true;
            return false;
        }
        self.buf.extend_from_slice(b);
        true
    }

    /// Add the strings of one text operator.
    fn strings(&mut self, s: &[u8]) {
        if s.is_empty() {
            return;
        }
        for c in s.chunks(CHUNK as usize) {
            if self.buf.len() + 1 + c.len() > self.cap {
                self.full = true;
                return;
            }
            self.buf.push(c.len() as u8);
            self.buf.extend_from_slice(c);
        }
        if self.push(&[END]) {
            self.last = END;
        }
    }

    fn line(&mut self) {
        if self.last != LINE && self.push(&[LINE]) {
            self.last = LINE;
        }
    }

    fn space(&mut self) {
        if self.last != SPACE && self.push(&[SPACE]) {
            self.last = SPACE;
        }
    }

    fn set_font(&mut self, num: Option<u32>, name: &[u8]) {
        let key = (num, name.to_vec());
        let id = match self.font_ids.get(&key) {
            Some(&id) => id,
            None if self.font_ids.len() < FONT_LIMIT => {
                let id = self.font_ids.len() as u16;
                self.font_ids.insert(key, id);
                id
            }
            None => NO_FONT,
        };
        let [lo, hi] = id.to_le_bytes();
        if id != self.font && self.push(&[FONT, lo, hi]) {
            self.font = id;
            self.last = FONT;
        }
    }

    /// The text of the string buffer, at most about `limit` bytes, and true
    /// if the text reached the limit.
    fn text(&self, limit: usize) -> (String, bool) {
        let pages = self.pages();
        let mut fonts = vec![(None, &b""[..]); self.font_ids.len()];
        for ((num, name), &id) in &self.font_ids {
            fonts[id as usize] = (*num, name.as_slice());
        }
        let mut memo = HashMap::new();
        let decs: Vec<Dec> = fonts
            .iter()
            .map(|&(num, name)| self.decoder(&pages, &mut memo, num, name))
            .collect();
        let mut out = String::new();
        let mut dec = &Dec::Latin;
        let mut codes = Vec::new();
        let b = &self.buf;
        let mut i = 0;
        while i < b.len() && out.len() < limit {
            let tag = b[i];
            i += 1;
            match tag {
                LINE => {
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                SPACE => {
                    if !out.ends_with(char::is_whitespace) {
                        out.push(' ');
                    }
                }
                END => {
                    decode(dec, &codes, &mut out, limit);
                    codes.clear();
                }
                FONT => {
                    let id = b
                        .get(i..i + 2)
                        .map_or(NO_FONT, |x| u16::from_le_bytes([x[0], x[1]]));
                    dec = decs.get(id as usize).unwrap_or(&Dec::Latin);
                    i += 2;
                }
                n => {
                    let e = (i + n as usize).min(b.len());
                    codes.extend_from_slice(&b[i..e]);
                    i = e;
                }
            }
        }
        // The buffer can end inside the strings of one operator.
        decode(dec, &codes, &mut out, limit);
        let full = out.len() >= limit;
        (out, full)
    }

    /// The page of each content stream.
    fn pages(&self) -> BTreeMap<u32, u32> {
        let mut of = BTreeMap::new();
        for (&page, body) in &self.objs {
            if get(body, b"/Type") != Some(Val::Name(b"/Page")) {
                continue;
            }
            let mut add = |n: u32| {
                of.entry(n).or_insert(page);
            };
            match get(body, b"/Contents") {
                Some(Val::Ref(n)) => {
                    add(n);
                    // The reference can point to an array of streams.
                    if let Some(a) = self.objs.get(&n) {
                        if Lexer::new(a).next() == Some(Tok::ArrOpen) {
                            array_refs(a).for_each(&mut add);
                        }
                    }
                }
                Some(Val::Array(a)) => array_refs(a).for_each(add),
                _ => {}
            }
        }
        of
    }

    /// The dictionary of a direct dictionary or of a reference.
    fn deref<'a>(&'a self, v: Val<'a>) -> Option<&'a [u8]> {
        match v {
            Val::Dict(d) => Some(d),
            Val::Ref(n) => self.objs.get(&n).map(Vec::as_slice),
            _ => None,
        }
    }

    /// The resources of a content stream: its own, or the resources of its
    /// page or of a parent of the page. `memo` keeps the result of each page
    /// and parent, so each one is read once.
    fn resources<'a>(
        &'a self,
        pages: &BTreeMap<u32, u32>,
        memo: &mut HashMap<u32, Option<&'a [u8]>>,
        content: u32,
    ) -> Option<&'a [u8]> {
        if let Some(r) = self.objs.get(&content).and_then(|d| get(d, b"/Resources")) {
            return self.deref(r);
        }
        let mut node = *pages.get(&content)?;
        let mut path = Vec::new();
        let mut found = None;
        // A loop of parents ends after 32 steps.
        for _ in 0..32 {
            if let Some(&r) = memo.get(&node) {
                found = r;
                break;
            }
            path.push(node);
            let Some(d) = self.objs.get(&node) else {
                break;
            };
            if let Some(r) = get(d, b"/Resources") {
                found = self.deref(r);
                break;
            }
            match get(d, b"/Parent") {
                Some(Val::Ref(p)) => node = p,
                _ => break,
            }
        }
        for n in path {
            memo.insert(n, found);
        }
        found
    }

    fn decoder<'a>(
        &'a self,
        pages: &BTreeMap<u32, u32>,
        memo: &mut HashMap<u32, Option<&'a [u8]>>,
        num: Option<u32>,
        name: &[u8],
    ) -> Dec<'a> {
        let font = num.and_then(|n| {
            let res = self.resources(pages, memo, n)?;
            let fonts = self.deref(get(res, b"/Font")?)?;
            self.deref(get(fonts, name)?)
        });
        let Some(font) = font else {
            return Dec::Latin;
        };
        let two = get(font, b"/Subtype") == Some(Val::Name(b"/Type0"));
        let map = match get(font, b"/ToUnicode") {
            Some(Val::Ref(n)) => self.maps.get(&n),
            _ => None,
        };
        match map {
            Some(m) => Dec::Map(m, two),
            None if two => Dec::Skip,
            None => Dec::Latin,
        }
    }
}

/// How the strings of one font become text.
enum Dec<'a> {
    /// Windows-1252.
    Latin,
    /// A CID font without a code map: its codes are glyph numbers.
    Skip,
    /// The code map, and true for a CID font (two-byte codes by default).
    Map(&'a CMap, bool),
}

fn latin(b: &[u8]) -> std::borrow::Cow<'_, str> {
    encoding_rs::WINDOWS_1252.decode_without_bom_handling(b).0
}

/// Add the text of `codes` to `out`, which never grows past `limit` bytes.
fn decode(dec: &Dec, codes: &[u8], out: &mut String, limit: usize) {
    let room = limit.saturating_sub(out.len());
    let t = match dec {
        Dec::Latin => latin(codes),
        Dec::Skip => return,
        Dec::Map(m, two) => {
            let mut t = String::new();
            let mut i = 0;
            while i < codes.len() && t.len() <= room {
                let e = (i + m.code_len(&codes[i..], if *two { 2 } else { 1 })).min(codes.len());
                // A simple font reads a code without an entry as Windows-1252.
                if !m.lookup(e - i, be(&codes[i..e]), &mut t) && !*two {
                    t.push_str(&latin(&codes[i..e]));
                }
                i = e;
            }
            t.into()
        }
    };
    out.push_str(&t[..crate::mime::floor_char(&t, room)]);
}

/// A `ToUnicode` code map.
#[derive(Default)]
struct CMap {
    /// The code space ranges: the code length in bytes, the lowest and the
    /// highest code.
    spaces: Vec<(usize, u32, u32)>,
    /// Sorted by code length, then by the lowest code.
    entries: Vec<Entry>,
}

struct Entry {
    len: usize,
    lo: u32,
    hi: u32,
    dst: Dst,
}

enum Dst {
    /// The UTF-16 text of `lo`. Each next code adds one to the last unit.
    Next(Vec<u16>),
    /// The UTF-16 text of each code from `lo`.
    List(Vec<Vec<u16>>),
}

/// The length and the value of a code of 1 to 4 bytes in hex.
fn code(h: &[u8]) -> Option<(usize, u32)> {
    let b = hex(h);
    (1..=4).contains(&b.len()).then(|| (b.len(), be(&b)))
}

/// The big-endian value of at most 4 bytes.
fn be(b: &[u8]) -> u32 {
    b.iter().take(4).fold(0, |v, &x| (v << 8) | u32::from(x))
}

/// The UTF-16 units of a hex string, at most 64, and at most `left`.
fn units(h: &[u8], left: &mut usize) -> Vec<u16> {
    let b = hex(&h[..h.len().min(256)]);
    let u: Vec<u16> = b
        .chunks(2)
        .take(*left)
        .map(|p| u16::from_be_bytes([p[0], p.get(1).copied().unwrap_or(0)]))
        .collect();
    *left -= u.len();
    u
}

fn push_units(u: impl Iterator<Item = u16>, out: &mut String) {
    out.extend(
        char::decode_utf16(u)
            .filter_map(Result::ok)
            .filter(|&c| c != '\0'),
    );
}

impl CMap {
    /// Read the code space and the `bfchar` and `bfrange` entries. Each entry
    /// and each UTF-16 unit takes one from `left`.
    fn parse(data: &[u8], left: &mut usize) -> CMap {
        enum Op<'a> {
            Hex(&'a [u8]),
            List(Vec<&'a [u8]>),
        }
        let mut m = CMap::default();
        let mut ops: Vec<Op> = Vec::new();
        let mut mode = &b""[..];
        let mut lx = Lexer::new(data);
        while let Some(t) = lx.next() {
            if *left == 0 {
                break;
            }
            match t {
                Tok::Hex(h) => ops.push(Op::Hex(h)),
                Tok::ArrOpen => {
                    let mut l = Vec::new();
                    for t in lx.by_ref() {
                        match t {
                            Tok::Hex(h) if l.len() < 256 => l.push(h),
                            Tok::ArrClose => break,
                            _ => {}
                        }
                    }
                    ops.push(Op::List(l));
                }
                Tok::Word(w) => {
                    mode = w;
                    ops.clear();
                    continue;
                }
                _ => ops.clear(),
            }
            match (mode, ops.as_slice()) {
                (b"begincodespacerange", [Op::Hex(a), Op::Hex(b)]) => {
                    if let (Some((n, lo)), Some((k, hi))) = (code(a), code(b)) {
                        if n == k && lo <= hi && m.spaces.len() < 16 {
                            m.spaces.push((n, lo, hi));
                        }
                    }
                }
                (b"beginbfchar", [Op::Hex(a), Op::Hex(d)]) => {
                    if let Some((len, v)) = code(a) {
                        *left -= 1;
                        let dst = Dst::Next(units(d, left));
                        m.entries.push(Entry {
                            len,
                            lo: v,
                            hi: v,
                            dst,
                        });
                    }
                }
                (b"beginbfrange", [Op::Hex(a), Op::Hex(b), d]) => {
                    if let (Some((len, lo)), Some((k, hi))) = (code(a), code(b)) {
                        if len == k && lo <= hi {
                            *left -= 1;
                            let dst = match d {
                                Op::Hex(h) => Dst::Next(units(h, left)),
                                Op::List(l) => {
                                    Dst::List(l.iter().map(|h| units(h, left)).collect())
                                }
                            };
                            m.entries.push(Entry { len, lo, hi, dst });
                        }
                    }
                }
                (_, [_, _, _]) => {}
                _ => continue,
            }
            ops.clear();
        }
        m.entries.sort_by_key(|e| (e.len, e.lo));
        m
    }

    /// The length of the code at the start of `b`, at least 1.
    fn code_len(&self, b: &[u8], default: usize) -> usize {
        if self.spaces.is_empty() {
            // Without a code space, the shortest code with an entry.
            return (1..=b.len().min(4))
                .find(|&n| self.find(n, be(&b[..n])).is_some())
                .unwrap_or(default);
        }
        for n in 1..=b.len().min(4) {
            let v = be(&b[..n]);
            if self
                .spaces
                .iter()
                .any(|&(k, lo, hi)| k == n && lo <= v && v <= hi)
            {
                return n;
            }
        }
        self.spaces.iter().map(|s| s.0).min().unwrap_or(1)
    }

    /// The entry of a code.
    fn find(&self, len: usize, v: u32) -> Option<&Entry> {
        let i = self.entries.partition_point(|e| (e.len, e.lo) <= (len, v));
        let e = &self.entries[i.checked_sub(1)?];
        (e.len == len && v <= e.hi).then_some(e)
    }

    /// Add the text of one code to `out`. False if the map has no entry.
    fn lookup(&self, len: usize, v: u32, out: &mut String) -> bool {
        let Some(e) = self.find(len, v) else {
            return false;
        };
        let off = v - e.lo;
        match &e.dst {
            Dst::Next(u) => {
                if let Some((&last, head)) = u.split_last() {
                    let last = last.wrapping_add(off as u16);
                    push_units(head.iter().copied().chain(std::iter::once(last)), out);
                }
            }
            Dst::List(l) => match l.get(off as usize) {
                Some(u) => push_units(u.iter().copied(), out),
                None => return false,
            },
        }
        true
    }
}

/// One token of a PDF object.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok<'a> {
    /// `<<`
    Open,
    /// `>>`
    Close,
    ArrOpen,
    ArrClose,
    /// A name, with its `/`.
    Name(&'a [u8]),
    /// The digits of a hex string.
    Hex(&'a [u8]),
    /// A literal string.
    Str,
    /// A number, a keyword or `R`.
    Word(&'a [u8]),
}

/// The tokens of a PDF object. Each step reads at least one byte.
#[derive(Clone, Copy)]
struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn new(b: &'a [u8]) -> Lexer<'a> {
        Lexer { b, i: 0 }
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Tok<'a>;

    fn next(&mut self) -> Option<Tok<'a>> {
        let b = self.b;
        loop {
            let s = self.i;
            let c = *b.get(s)?;
            let word_end = |from: usize| {
                b[from..]
                    .iter()
                    .position(|&x| is_delimiter(x))
                    .map_or(b.len(), |n| from + n)
            };
            let (tok, end) = match c {
                b'%' => {
                    self.i = skip_comment(b, s);
                    continue;
                }
                b'(' => (Tok::Str, skip_string(b, s)),
                b'<' if b.get(s + 1) == Some(&b'<') => (Tok::Open, s + 2),
                b'<' => {
                    let e = b[s..]
                        .iter()
                        .position(|&x| x == b'>')
                        .map_or(b.len(), |n| s + n);
                    (Tok::Hex(&b[s + 1..e]), (e + 1).min(b.len()))
                }
                b'>' if b.get(s + 1) == Some(&b'>') => (Tok::Close, s + 2),
                b'[' => (Tok::ArrOpen, s + 1),
                b']' => (Tok::ArrClose, s + 1),
                b'/' => {
                    let e = word_end(s + 1);
                    (Tok::Name(&b[s..e]), e)
                }
                c if is_delimiter(c) => {
                    self.i = s + 1;
                    continue;
                }
                _ => {
                    let e = word_end(s);
                    (Tok::Word(&b[s..e]), e)
                }
            };
            self.i = end;
            return Some(tok);
        }
    }
}

/// A value of a dictionary entry. A dictionary or an array is its text.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Val<'a> {
    Ref(u32),
    Int(i64),
    Name(&'a [u8]),
    Dict(&'a [u8]),
    Array(&'a [u8]),
    Other,
}

fn int(w: &[u8]) -> Option<i64> {
    if w.len() > 18 {
        return None;
    }
    std::str::from_utf8(w).ok()?.parse().ok()
}

/// The next value. `None` at the end.
fn value<'a>(lx: &mut Lexer<'a>) -> Option<Val<'a>> {
    let t = lx.next()?;
    Some(match t {
        Tok::Name(n) => Val::Name(n),
        Tok::Open | Tok::ArrOpen => {
            let open = lx.i - if t == Tok::Open { 2 } else { 1 };
            let mut depth = 1usize;
            while depth > 0 {
                match lx.next() {
                    Some(Tok::Open | Tok::ArrOpen) => depth += 1,
                    Some(Tok::Close | Tok::ArrClose) => depth -= 1,
                    Some(_) => {}
                    None => break,
                }
            }
            let text = &lx.b[open..lx.i];
            if t == Tok::Open {
                Val::Dict(text)
            } else {
                Val::Array(text)
            }
        }
        Tok::Word(w) => match int(w) {
            Some(n) => {
                // `<number> <number> R` is a reference.
                let mut p = *lx;
                match (p.next(), p.next()) {
                    (Some(Tok::Word(g)), Some(Tok::Word(r))) if r == b"R" && int(g).is_some() => {
                        *lx = p;
                        u32::try_from(n).map_or(Val::Other, Val::Ref)
                    }
                    _ => Val::Int(n),
                }
            }
            None => Val::Other,
        },
        _ => Val::Other,
    })
}

/// The entries of the dictionary that starts at the first `<<` of `d`.
fn entries(d: &[u8]) -> impl Iterator<Item = (&[u8], Val<'_>)> {
    let mut lx = Lexer::new(d);
    let mut started = false;
    std::iter::from_fn(move || {
        if !started {
            while lx.next()? != Tok::Open {}
            started = true;
        }
        loop {
            match lx.next()? {
                Tok::Close => return None,
                Tok::Name(k) => return Some((k, value(&mut lx)?)),
                _ => {}
            }
        }
    })
}

/// The value of `key` (with its `/`) in the dictionary `d`.
fn get<'a>(d: &'a [u8], key: &[u8]) -> Option<Val<'a>> {
    entries(d).find(|e| e.0 == key).map(|e| e.1)
}

/// The references of an array.
fn array_refs(a: &[u8]) -> impl Iterator<Item = u32> + '_ {
    let mut lx = Lexer::new(a);
    lx.next();
    std::iter::from_fn(move || loop {
        if let Val::Ref(n) = value(&mut lx)? {
            return Some(n);
        }
    })
}

/// A literal string `( ... )` with nested parentheses and escapes.
/// Return the bytes and the count of input bytes read.
fn literal(c: &[u8]) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < c.len() {
        let b = c[i];
        i += 1;
        match b {
            b'(' => {
                depth += 1;
                if depth > 1 {
                    out.push(b);
                }
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return (out, i);
                }
                out.push(b);
            }
            b'\\' => {
                let Some(&e) = c.get(i) else { break };
                i += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' | b'f' => {}
                    b'\r' | b'\n' => {
                        if e == b'\r' && c.get(i) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    b'0'..=b'7' => {
                        let mut v = (e - b'0') as u32;
                        for _ in 0..2 {
                            match c.get(i) {
                                Some(&d @ b'0'..=b'7') => {
                                    v = v * 8 + (d - b'0') as u32;
                                    i += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(v as u8);
                    }
                    _ => out.push(e),
                }
            }
            _ => out.push(b),
        }
    }
    (out, i)
}

fn hex(h: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = h
        .iter()
        .filter_map(|c| (*c as char).to_digit(16).map(|d| d as u8))
        .collect();
    digits
        .chunks(2)
        .map(|p| p[0] * 16 + p.get(1).copied().unwrap_or(0))
        .collect()
}

/// True if the text looks like words: at least 3 letters, and at least 85 %
/// of the characters are letters, digits, white space or common punctuation.
fn readable(t: &str) -> bool {
    let (mut good, mut letters, mut all) = (0usize, 0usize, 0usize);
    for c in t.chars() {
        all += 1;
        if c.is_alphabetic() {
            letters += 1;
        }
        if c.is_alphanumeric() || c.is_whitespace() || ".,;:!?'\"()-/%&@€$£#*+=".contains(c) {
            good += 1;
        }
    }
    letters >= 3 && good * 100 >= all * 85
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample(content: &[u8], flate: bool) -> Vec<u8> {
        let (filter, data) = if flate {
            (
                "/Filter /FlateDecode ",
                miniz_oxide::deflate::compress_to_vec_zlib(content, 6),
            )
        } else {
            ("", content.to_vec())
        };
        let mut pdf = format!(
            "%PDF-1.4\n1 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
             4 0 obj\n<< {filter}/Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        pdf.extend(data);
        pdf.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< >>\n%%EOF\n");
        pdf
    }

    const PAGE: &[u8] = b"BT /F1 12 Tf 72 720 Td (Invoice 2025-07 for Avel Torun) Tj 0 -14 Td \
        [(Total:) -250 (120.00 EUR)] TJ T* (Caf\\351 \\(Paris\\)) Tj ET";

    #[test]
    fn text_of_a_simple_pdf() {
        let want = "Invoice 2025-07 for Avel Torun\nTotal: 120.00 EUR\nCafé (Paris)";
        assert_eq!(text(&sample(PAGE, true), 1000).as_deref(), Some(want));
        assert_eq!(text(&sample(PAGE, false), 1000).as_deref(), Some(want));
    }

    #[test]
    fn codes_and_bad_input_give_no_text() {
        // Two-byte CID codes are not words.
        let cid = b"BT <0011002200330044005500660077008800990011002200330044005500660077> Tj ET";
        assert_eq!(text(&sample(cid, true), 1000), None);
        assert_eq!(text(b"not a pdf", 1000), None);
        let pdf = sample(PAGE, true);
        for cut in [10, 60, 120, pdf.len() - 20] {
            text(&pdf[..cut], 1000);
        }
        let mut bad = b"%PDF-1.4\nstream\n(((((\\".to_vec();
        bad.extend(std::iter::repeat_n(b'(', 10_000));
        text(&bad, 1000);
    }

    fn text(pdf: &[u8], limit: usize) -> Option<String> {
        text_cut(pdf, limit).0
    }

    #[test]
    fn a_limit_in_white_space_shows_a_cut() {
        let t = format!("BT (Hello{}) Tj (later) Tj ET", " ".repeat(200));
        let (got, cut) = text_cut(&sample(t.as_bytes(), false), 100);
        assert_eq!(got.as_deref(), Some("Hello"));
        assert!(cut);
        assert!(!text_cut(&sample(PAGE, false), 1000).1);
        // Text after the stream limit is not read: that is a cut too.
        let big = format!(
            "BT (Hello world) Tj ET\n{}BT (later) Tj ET",
            " ".repeat(STREAM_LIMIT)
        );
        for flate in [false, true] {
            let (got, cut) = text_cut(&sample(big.as_bytes(), flate), 1000);
            assert_eq!(got.as_deref(), Some("Hello world"));
            assert!(cut, "flate {flate}");
        }
    }

    #[test]
    fn no_text_past_the_inflate_limit_of_the_pdf() {
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut add = |data: &[u8], n: usize| {
            pdf.extend(format!("{n} 0 obj\n<< /Length {} >>\nstream\n", data.len()).bytes());
            pdf.extend_from_slice(data);
            pdf.extend_from_slice(b"\nendstream\nendobj\n");
        };
        let pad = vec![b' '; STREAM_LIMIT - 1];
        for n in 1..=8 {
            add(&pad, n);
        }
        let last = format!("{}BT (Hello world) Tj ET", " ".repeat(64));
        add(last.as_bytes(), 9);
        let (got, cut) = text_cut(&pdf, 1000);
        assert_eq!(got, None);
        assert!(cut);
    }

    #[test]
    fn names_inside_dictionary_strings_do_not_count() {
        for info in [
            "/Info (/Filter)",
            "/Info (/Image /XRef)",
            "% /Filter /Image\n",
        ] {
            let pdf = String::from_utf8(sample(PAGE, false))
                .unwrap()
                .replace("<< /Length", &format!("<< {info} /Length"));
            assert!(text(pdf.as_bytes(), 1000).is_some(), "{info}");
        }
        let flate = String::from_utf8_lossy(&sample(PAGE, true)).replace(
            "/Filter /FlateDecode",
            "/Info (/Filter /LZWDecode) /Filter /FlateDecode",
        );
        assert!(only_flate(flate.as_bytes()));
        assert!(!is_content(b"<< /Subtype /Image /Length 3 >>"));
        assert!(!is_content(b"<< /FontFile2 4 0 R >>"));
    }

    #[test]
    fn header_and_keyword_text_inside_strings_do_not_count() {
        let t = b"BT (the endstream report) Tj ET";
        for info in [
            "/Info ( 1 0 obj)",
            "/Info (stream\n)",
            "/Info <73747265616d>",
        ] {
            let pdf = String::from_utf8(sample(t, false))
                .unwrap()
                .replace(" >>\nstream", &format!(" {info} >>\nstream"));
            assert_eq!(
                text(pdf.as_bytes(), 100).as_deref(),
                Some("the endstream report"),
                "{info}"
            );
        }
    }

    #[test]
    fn many_short_streams_stay_fast() {
        let mut pdf = b"%PDF-1.4\n".to_vec();
        while pdf.len() < 9 * 1024 * 1024 {
            pdf.extend_from_slice(b"stream\nendstream\n");
        }
        let begin = std::time::Instant::now();
        assert_eq!(text(&pdf, 1000), None);
        let time = begin.elapsed();
        assert!(time < std::time::Duration::from_secs(3), "time {time:?}");
    }

    #[test]
    fn another_length_key_before_length() {
        let t = b"BT (the endstream report) Tj ET";
        let pdf = String::from_utf8(sample(t, false))
            .unwrap()
            .replace("<< /Length", "<< /LengthX 3 /Info (/Length 3) /Length");
        assert_eq!(
            text(pdf.as_bytes(), 100).as_deref(),
            Some("the endstream report")
        );
    }

    #[test]
    fn endstream_inside_a_string_does_not_end_the_stream() {
        let t = b"BT (the endstream report) Tj ET";
        assert_eq!(
            text(&sample(t, false), 100).as_deref(),
            Some("the endstream report")
        );
        assert_eq!(declared_len(b"<< /Length 5 0 R >>"), None);
        assert_eq!(declared_len(b"<< /Length1 9 /Length 31 >>"), Some(31));
        assert_eq!(declared_len(b"<< /LengthX 9 /Length 31 >>"), Some(31));
        assert_eq!(declared_len(b"<< /LengthX 9 >>"), None);
        // A key inside a string, a comment or an inner dictionary does not count.
        assert_eq!(
            declared_len(b"<< /Info (/Length 3) /Length 29 >>"),
            Some(29)
        );
        assert_eq!(
            declared_len(b"<< /Info (a \\\\ \\) (/Length 3)) /Length 29 >>"),
            Some(29)
        );
        assert_eq!(declared_len(b"<< % /Length 3\n/Length 29 >>"), Some(29));
        assert_eq!(
            declared_len(b"<< /P << /Length 3 >> /Length 29 >>"),
            Some(29)
        );
        assert_eq!(declared_len(b"<< /Length 31 >>"), Some(31));
    }

    #[test]
    fn text_outside_a_text_object_goes_out() {
        let t = b"(Hidden instructions) Tj BT (Visible words) Tj 3 Tr (Ignore all) Tj 0 Tr ( paid) Tj ET (After) Tj";
        assert_eq!(
            text(&sample(t, false), 100).as_deref(),
            Some("Visible words paid")
        );
    }

    #[test]
    fn only_a_single_flate_filter_is_read() {
        assert!(only_flate(b"<< /Filter /FlateDecode /Length 5 >>"));
        assert!(only_flate(b"<< /Filter [ /FlateDecode ] >>"));
        assert!(!only_flate(b"<< /Filter [/FlateDecode /Crypt] >>"));
        assert!(!only_flate(b"<< /Filter /FlateDecodeX >>"));
        assert!(!only_flate(b"<< /Length 5 >>"));
        let data = miniz_oxide::deflate::compress_to_vec_zlib(b"BT (Hidden words) Tj ET", 6);
        let mut pdf = format!(
            "%PDF-1.4\n4 0 obj\n<< /Filter [/FlateDecode /Crypt] /Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        pdf.extend(data);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        assert_eq!(text(&pdf, 100), None);
    }

    #[test]
    fn obj_in_a_value_is_not_an_object_header() {
        let content = b"BT (the endstream report) Tj ET";
        let mut pdf = format!(
            "%PDF-1.4\n4 0 obj\n<< /Length {} /Foo (obj) >>\nstream\n",
            content.len()
        )
        .into_bytes();
        pdf.extend_from_slice(content);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        assert_eq!(text(&pdf, 100).as_deref(), Some("the endstream report"));
        assert!(object_number(b"\n12 0 ").is_some());
        assert!(object_number(b"(1 0 ").is_none());
        assert!(object_number(b"/Foo (").is_none());
    }

    #[test]
    fn a_comment_in_a_content_stream_ends() {
        let t = b"BT %c\n(After the comment) Tj % last\nET";
        assert_eq!(
            text(&sample(t, false), 100).as_deref(),
            Some("After the comment")
        );
    }

    #[test]
    fn short_text_stays() {
        assert_eq!(
            text(&sample(b"BT (PAID) Tj ET", true), 100).as_deref(),
            Some("PAID")
        );
    }

    #[test]
    fn the_limit_holds() {
        let t = text(&sample(PAGE, true), 25).unwrap_or_default();
        assert!(t.len() < 60, "{t}");
    }

    /// A PDF of the objects `(number, dictionary, stream data)`. The data is
    /// compressed with `FlateDecode`.
    fn doc(objs: &[(u32, &str, Option<&[u8]>)]) -> Vec<u8> {
        build(objs, true)
    }

    fn build(objs: &[(u32, &str, Option<&[u8]>)], flate: bool) -> Vec<u8> {
        let mut pdf = b"%PDF-1.7\n".to_vec();
        for (n, dict, data) in objs {
            match data {
                Some(d) => {
                    let (filter, z) = if flate {
                        (
                            " /Filter /FlateDecode",
                            miniz_oxide::deflate::compress_to_vec_zlib(d, 6),
                        )
                    } else {
                        ("", d.to_vec())
                    };
                    let dict = dict.trim_end().strip_suffix(">>").unwrap_or(dict);
                    pdf.extend(
                        format!("{n} 0 obj\n{dict}{filter} /Length {} >>\nstream\n", z.len())
                            .bytes(),
                    );
                    pdf.extend(z);
                    pdf.extend_from_slice(b"\nendstream\nendobj\n");
                }
                None => pdf.extend(format!("{n} 0 obj\n{dict}\nendobj\n").bytes()),
            }
        }
        pdf.extend_from_slice(b"trailer\n<< /Root 1 0 R >>\n%%EOF\n");
        pdf
    }

    const CMAP: &[u8] = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
        /CMapName /Adobe-Identity-UCS def\n1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
        4 beginbfchar <0001> <0048> <0003> <006C> <0004> <0020> <0008> <0064> endbfchar\n\
        3 beginbfrange <0002> <0002> <0065> <0005> <0005> <006F> <0006> <0007> [<0057> <0072>] endbfrange\n\
        endcmap CMapName currentdict /CMap defineresource pop end end";
    /// "Hello World" in the two-byte codes of `CMAP`.
    const CODES: &[u8] =
        b"BT /F1 12 Tf 72 720 Td <00010002000300030005000400060005000700030008> Tj ET";
    const TYPE0: &str = "<< /Type /Font /Subtype /Type0 /BaseFont /AB+Arial /Encoding /Identity-H \
        /DescendantFonts [7 0 R] /ToUnicode 6 0 R >>";

    #[test]
    fn a_cid_font_with_a_to_unicode_map_gives_text() {
        // The content comes before the font and the map, as in many files.
        let pdf = doc(&[
            (1, "<< /Type /Pages /Kids [2 0 R] /Count 1 >>", None),
            (2, "<< /Type /Page /Parent 1 0 R /Contents 3 0 R /Resources << /Font << /F1 5 0 R >> >> >>", None),
            (3, "<< >>", Some(CODES)),
            (5, TYPE0, None),
            (6, "<< >>", Some(CMAP)),
        ]);
        assert_eq!(text(&pdf, 1000).as_deref(), Some("Hello World"));
    }

    #[test]
    fn resources_of_the_parent_and_of_a_reference() {
        let pdf = doc(&[
            (1, "<< /Type /Pages /Kids [2 0 R] /Resources 8 0 R >>", None),
            (2, "<< /Type /Page /Parent 1 0 R /Contents [3 0 R] >>", None),
            (3, "<< >>", Some(CODES)),
            (8, "<< /Font 9 0 R >>", None),
            (9, "<< /F1 5 0 R >>", None),
            (5, TYPE0, None),
            (6, "<< >>", Some(CMAP)),
        ]);
        assert_eq!(text(&pdf, 1000).as_deref(), Some("Hello World"));
    }

    #[test]
    fn dictionaries_in_an_object_stream() {
        let page = "<< /Type /Page /Contents 3 0 R /Resources << /Font << /F1 5 0 R >> >> >>";
        let head = format!("2 0 5 {} ", page.len() + 1);
        let body = format!("{head}{page}\n{TYPE0}");
        let dict = format!("<< /Type /ObjStm /N 2 /First {} >>", head.len());
        let pdf = doc(&[
            (3, "<< >>", Some(CODES)),
            (10, &dict, Some(body.as_bytes())),
            (6, "<< >>", Some(CMAP)),
        ]);
        assert_eq!(text(&pdf, 1000).as_deref(), Some("Hello World"));
    }

    #[test]
    fn a_form_with_its_own_resources() {
        let pdf = doc(&[
            (
                3,
                "<< /Type /XObject /Subtype /Form /Resources << /Font << /F1 5 0 R >> >> >>",
                Some(CODES),
            ),
            (5, TYPE0, None),
            (6, "<< >>", Some(CMAP)),
        ]);
        assert_eq!(text(&pdf, 1000).as_deref(), Some("Hello World"));
    }

    #[test]
    fn a_simple_font_with_a_one_byte_map() {
        let cmap = b"begincmap 1 begincodespacerange <00> <FF> endcodespacerange \
            2 beginbfchar <01> <0050> <02> <0061> endbfchar 1 beginbfrange <03> <04> <0069> endbfrange endcmap";
        // Codes 01 02 03 04: P a i j. A code with no entry reads as Windows-1252.
        let content = b"BT /F2 9 Tf <01020304> Tj (d) Tj ET";
        let pdf = doc(&[
            (
                2,
                "<< /Type /Page /Contents 3 0 R /Resources << /Font << /F2 5 0 R >> >> >>",
                None,
            ),
            (3, "<< >>", Some(content)),
            (
                5,
                "<< /Type /Font /Subtype /TrueType /ToUnicode 6 0 R >>",
                None,
            ),
            (6, "<< >>", Some(cmap)),
        ]);
        assert_eq!(text(&pdf, 1000).as_deref(), Some("Paijd"));
    }

    #[test]
    fn a_cid_font_without_a_map_gives_no_codes() {
        // The codes of the Type0 font are glyph numbers: they do not go into the text.
        let content = b"BT /F1 9 Tf <0041004200430044> Tj /F2 9 Tf (Plain words) Tj ET";
        let pdf = doc(&[
            (2, "<< /Type /Page /Contents 3 0 R /Resources << /Font << /F1 5 0 R /F2 7 0 R >> >> >>", None),
            (3, "<< >>", Some(content)),
            (5, "<< /Type /Font /Subtype /Type0 /Encoding /Identity-H >>", None),
            (7, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>", None),
        ]);
        assert_eq!(text(&pdf, 1000).as_deref(), Some("Plain words"));
    }

    /// A small random number generator with a fixed seed.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 16) as usize
        }

        fn below(&mut self, n: usize) -> usize {
            self.next() % n.max(1)
        }
    }

    fn corpus() -> Vec<Vec<u8>> {
        let cid = [
            (1, "<< /Type /Pages /Kids [2 0 R] /Resources 8 0 R >>", None),
            (2, "<< /Type /Page /Parent 1 0 R /Contents [3 0 R] >>", None),
            (3, "<< >>", Some(CODES)),
            (8, "<< /Font << /F1 5 0 R >> >>", None),
            (5, TYPE0, None),
            (6, "<< >>", Some(CMAP)),
        ];
        let page = "<< /Type /Page /Contents 3 0 R /Resources << /Font << /F1 5 0 R >> >> >>";
        let head = format!("2 0 5 {} ", page.len() + 1);
        let body = format!("{head}{page}\n{TYPE0}");
        let dict = format!("<< /Type /ObjStm /N 2 /First {} >>", head.len());
        let objstm = [
            (3, "<< >>", Some(CODES)),
            (10, dict.as_str(), Some(body.as_bytes())),
            (6, "<< >>", Some(CMAP)),
        ];
        vec![
            build(&cid, false),
            build(&objstm, false),
            build(&cid, true),
            build(&objstm, true),
            sample(PAGE, false),
        ]
    }

    #[test]
    fn hostile_pdfs_do_not_panic_and_stay_in_the_limit() {
        // Text that a mutation can add: tokens of each part of the reader.
        const TOKENS: &[&[u8]] = &[
            b"<<",
            b">>",
            b"[",
            b"]",
            b"(",
            b")",
            b"<",
            b">",
            b"\\",
            b"%",
            b"/",
            b" 0 R",
            b"9 0 obj",
            b"endobj",
            b"stream\n",
            b"endstream",
            b"/Type /Page",
            b"/Type /ObjStm",
            b"/N 99999",
            b"/First 4",
            b"/Parent 2 0 R",
            b"/Contents 2 0 R",
            b"/Resources 2 0 R",
            b"/ToUnicode 3 0 R",
            b"/Subtype /Type0",
            b"BT",
            b"ET",
            b"/F1 1 Tf",
            b"Tj",
            b"TJ",
            b"begincmap",
            b"begincodespacerange",
            b"<00> <FFFFFFFF>",
            b"beginbfchar",
            b"beginbfrange",
            b"<0000> <FFFF> <D800>",
            b"[<0041>",
            b"<FFFFFFFFFF>",
            b"4294967296 0 R",
            b"-1",
        ];
        let corpus = corpus();
        for c in &corpus {
            assert!(text(c, 1000).is_some());
        }
        let mut rng = Rng(0x1D15_C1EA_2025_0007);
        let begin = std::time::Instant::now();
        for _ in 0..3000 {
            let mut pdf = corpus[rng.below(corpus.len())].clone();
            for _ in 0..1 + rng.below(8) {
                let at = rng.below(pdf.len() + 1);
                match rng.below(5) {
                    0 => {
                        if at < pdf.len() {
                            pdf[at] = rng.next() as u8;
                        }
                    }
                    1 => {
                        let t = TOKENS[rng.below(TOKENS.len())];
                        pdf.splice(at..at, t.iter().copied());
                    }
                    2 => {
                        let end = (at + rng.below(64)).min(pdf.len());
                        pdf.drain(at..end);
                    }
                    3 => {
                        let end = (at + rng.below(256)).min(pdf.len());
                        let part = pdf[at..end].to_vec();
                        pdf.splice(at..at, part);
                    }
                    _ => pdf.truncate(at.max(8)),
                }
            }
            let limit = 1 + rng.below(2000);
            if let (Some(t), _) = text_cut(&pdf, limit) {
                assert!(t.len() <= limit + 2, "{} > {limit}", t.len());
            }
        }
        let time = begin.elapsed();
        assert!(time < std::time::Duration::from_secs(20), "time {time:?}");
    }

    #[test]
    fn hostile_structures_stay_bounded() {
        let begin = std::time::Instant::now();
        // A loop of parents, and contents that point to their own page.
        let pdf = doc(&[
            (1, "<< /Type /Page /Parent 2 0 R /Contents 1 0 R >>", None),
            (2, "<< /Type /Pages /Parent 1 0 R >>", None),
            (3, "<< >>", Some(b"BT /F1 1 Tf (Plain words) Tj ET")),
        ]);
        assert_eq!(text(&pdf, 100).as_deref(), Some("Plain words"));
        // More fonts than the font limit: the rest read as Windows-1252.
        let mut many = b"BT ".to_vec();
        for n in 0..FONT_LIMIT + 100 {
            many.extend(format!("/F{n} 1 Tf (w) Tj ").bytes());
        }
        many.extend_from_slice(b"ET");
        let pdf = doc(&[(3, "<< >>", Some(&many))]);
        assert_eq!(text(&pdf, 100_000).map(|t| t.len()), Some(FONT_LIMIT + 100));
        // One range for all four-byte codes, and a map past its budget.
        let mut cmap =
            b"begincmap 1 beginbfrange <00000000> <FFFFFFFF> <0041> endbfrange 1 beginbfchar"
                .to_vec();
        for _ in 0..MAP_BUDGET {
            cmap.extend_from_slice(b" <01> <0042>");
        }
        let pdf = doc(&[
            (
                2,
                "<< /Type /Page /Contents 3 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
                None,
            ),
            (
                3,
                "<< >>",
                Some(b"BT /F1 1 Tf <000000000000000100000002> Tj ET"),
            ),
            (5, "<< /Subtype /Type0 /ToUnicode 6 0 R >>", None),
            (6, "<< >>", Some(&cmap)),
        ]);
        assert_eq!(text(&pdf, 100).as_deref(), Some("ABC"));
        // An object stream that claims many objects, and deep arrays.
        let deep = "[".repeat(100_000);
        let pdf = doc(&[
            (
                9,
                "<< /Type /ObjStm /N 1000000000 /First 1 >>",
                Some(b"1 2 3 4 5"),
            ),
            (7, &format!("<< /Type /Page /Contents {deep} >>"), None),
            (3, "<< >>", Some(b"BT (Plain words) Tj ET")),
        ]);
        assert_eq!(text(&pdf, 100).as_deref(), Some("Plain words"));
        // Many pages under a long chain of big parents: each parent is read once.
        let pad = format!("/Pad ({})", "x".repeat(15_000));
        let mut objs: Vec<(u32, String, Option<&[u8]>)> = (0..32)
            .map(|n| {
                (
                    100 + n,
                    format!("<< /Type /Pages {pad} /Parent {} 0 R >>", 101 + n),
                    None,
                )
            })
            .collect();
        for n in 0..2000 {
            objs.push((
                1000 + n,
                format!(
                    "<< /Type /Page /Parent 100 0 R /Contents {} 0 R >>",
                    5000 + n
                ),
                None,
            ));
            objs.push((5000 + n, "<< >>".into(), Some(b"BT /F1 1 Tf (w) Tj ET")));
        }
        let objs: Vec<(u32, &str, Option<&[u8]>)> =
            objs.iter().map(|(n, d, b)| (*n, d.as_str(), *b)).collect();
        assert_eq!(
            text(&build(&objs, false), 100_000).map(|t| t.len()),
            Some(2000 * 2 - 1)
        );
        let time = begin.elapsed();
        assert!(time < std::time::Duration::from_secs(10), "time {time:?}");
    }

    #[test]
    fn a_replaced_object_near_the_budget_stays() {
        let mut d = Doc::new(100);
        let body = vec![b' '; OBJ_LIMIT];
        let mut n = 0;
        while d.obj_bytes + body.len() + 64 <= OBJ_BUDGET {
            d.keep(n, &body);
            n += 1;
        }
        d.keep(0, &body);
        assert!(d.objs.contains_key(&0));
    }

    #[test]
    fn a_character_past_the_limit_shows_a_cut() {
        // Byte 0x80 is the euro sign: 3 bytes of UTF-8 after 4,094 letters.
        let mut t = b"BT (".to_vec();
        t.extend(std::iter::repeat_n(b'a', 4094));
        t.extend_from_slice(b"\\200) Tj ET");
        let (got, cut) = text_cut(&sample(&t, false), 4096);
        assert_eq!(got.map(|t| t.len()), Some(4094));
        assert!(cut);
    }
}
