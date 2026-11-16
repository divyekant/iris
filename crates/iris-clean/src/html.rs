//! HTML to Markdown. A small tokenizer with an element stack, not a full HTML
//! parser. Scripts, styles, hidden elements and tracking images go out.
//!
//! The rule for hidden content: a person does not see it, so the clean text
//! does not hold it. Hidden text is a common place for prompt injection.

use std::borrow::Cow;

/// The deepest element stack that the converter keeps.
const MAX_STACK: usize = 512;
/// The most bytes of Markdown output. The quote and signature steps read at
/// most this much, so more output has no use.
pub const OUT_LIMIT: usize = 512 * 1024;
/// The deepest quote level that the output shows with `> ` marks.
const MAX_QUOTE: usize = 8;

const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];
/// Elements whose content is not shown as text. The converter skips to the end tag.
const RAW: [&str; 9] = [
    "script", "style", "title", "template", "textarea", "xmp", "noembed", "noframes", "iframe",
];
/// Elements with a blank line before and after them.
const PARA: [&str; 13] = [
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "table",
    "ul",
    "ol",
    "pre",
    "dl",
];
/// Elements with a line end before and after them.
const LINE: [&str; 20] = [
    "div", "tr", "li", "dt", "dd", "section", "article", "header", "footer", "main", "nav",
    "aside", "address", "center", "form", "fieldset", "caption", "details", "summary", "figure",
];

/// The attributes that the converter reads.
const USED_ATTRS: [&str; 9] = [
    "href", "style", "class", "id", "hidden", "type", "alt", "width", "height",
];

struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

struct Open {
    name: String,
    hidden: bool,
    /// The element and its content are visible (`visibility`, inherited).
    visible: bool,
    /// The element opened while the converter wrote output. Only such an
    /// element has an effect when it closes.
    shown: bool,
}

#[derive(Default)]
struct Writer {
    out: String,
    /// Line ends to write before the next character (at most 2).
    breaks: usize,
    space: bool,
    line_start: bool,
    quote: usize,
    pre: usize,
    lists: usize,
    /// The open link: its target and the output position of its text.
    link: Option<(String, usize)>,
    /// True when the output reached `OUT_LIMIT`.
    full: bool,
}

impl Writer {
    fn block(&mut self, n: usize) {
        self.breaks = self.breaks.max(n);
        self.space = false;
    }

    fn push(&mut self, c: char) {
        if self.out.len() >= OUT_LIMIT {
            self.full = true;
            return;
        }
        if self.breaks > 0 {
            if !self.out.is_empty() {
                for _ in 0..self.breaks {
                    self.out.push('\n');
                }
                self.line_start = true;
            }
            self.breaks = 0;
        }
        if self.out.is_empty() {
            self.line_start = true;
        }
        if self.line_start {
            for _ in 0..self.quote.min(MAX_QUOTE) {
                self.out.push_str("> ");
            }
            self.line_start = false;
        } else if self.space {
            self.out.push(' ');
        }
        self.space = false;
        self.out.push(c);
    }

    fn literal(&mut self, s: &str) {
        s.chars().for_each(|c| self.push(c));
    }

    fn text(&mut self, t: &str) {
        for c in t.chars() {
            if self.pre > 0 && c == '\n' {
                self.breaks += 1;
            } else if self.pre == 0 && c.is_whitespace() && c != '\u{a0}' {
                if self.breaks == 0 && !self.line_start && !self.out.is_empty() {
                    self.space = true;
                }
            } else {
                self.push(c);
            }
        }
    }

    fn close_link(&mut self) {
        let Some((href, start)) = self.link.take() else {
            return;
        };
        let start = start.min(self.out.len());
        let tail = &self.out[start..];
        let lead = tail.len() - tail.trim_start().len();
        let text = tail.trim().to_string();
        let plain = href.strip_prefix("mailto:").unwrap_or(&href);
        if text.is_empty() || text == plain {
            return;
        }
        self.out.truncate(start + lead);
        if text.contains('\n') {
            // A link text with lines is not a Markdown link text. The angle
            // brackets mark where the target starts and ends.
            self.out.push_str(&format!("{text} (<{href}>)"));
        } else {
            // Show the real target: a link text that is not its target is a phishing sign.
            self.out.push_str(&format!("[{text}]({href})"));
        }
        // A long target must not pass the output limit.
        if self.out.len() > OUT_LIMIT {
            self.out.truncate(floor(&self.out, OUT_LIMIT));
            self.full = true;
        }
    }
}

/// Convert HTML to Markdown. If `drop_quotes` is true (the message is a
/// reply), quoted history of common mail clients goes out too.
/// Return the Markdown and true if the output reached [`OUT_LIMIT`].
pub fn to_markdown(html: &str, drop_quotes: bool) -> (String, bool) {
    let b = html.as_bytes();
    let mut w = Writer {
        line_start: true,
        ..Writer::default()
    };
    let mut stack: Vec<Open> = Vec::new();
    // A style rule applies to elements before it too, so read all rules first.
    let css = read_styles(html);
    let mut hidden = 0usize;
    // Elements past the stack limit. Their content goes out, so that a hidden
    // element deep in the tree cannot show its text.
    // The names are kept up to a limit. Past it, the content stays hidden to the end.
    let mut overflow: Vec<String> = Vec::new();
    let mut overflow_lost = false;
    let mut i = 0;
    while i < b.len() && !w.full {
        if b[i] != b'<' {
            let end = b[i..]
                .iter()
                .position(|&c| c == b'<')
                .map_or(b.len(), |n| i + n);
            if hidden == 0 && stack.last().is_none_or(|o| o.visible) {
                w.text(&decode_entities(&html[i..end]));
            }
            i = end;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = find(b, i + 4, b"-->").map_or(b.len(), |n| n + 3);
            continue;
        }
        if matches!(b.get(i + 1), Some(b'!' | b'?')) {
            i = find(b, i, b">").map_or(b.len(), |n| n + 1);
            continue;
        }
        let closing = b.get(i + 1) == Some(&b'/');
        let name_start = i + 1 + closing as usize;
        if !b.get(name_start).is_some_and(u8::is_ascii_alphabetic) {
            if hidden == 0 && stack.last().is_none_or(|o| o.visible) {
                w.text("<");
            }
            i += 1;
            continue;
        }
        let (tag, end) = parse_tag(html, name_start);
        i = end;
        let name = tag.name.as_str();
        if closing {
            if !overflow.is_empty() || overflow_lost {
                // Only the matching end tag closes an element past the stack
                // limit. Other end tags change nothing, so hidden text stays hidden.
                if !overflow_lost {
                    if let Some(at) = overflow.iter().rposition(|n| n == name) {
                        hidden -= overflow.len() - at;
                        overflow.truncate(at);
                    }
                }
                continue;
            }
            if let Some(at) = stack.iter().rposition(|o| o.name == name) {
                pop_to(&mut stack, at, &mut hidden, &mut w);
            }
            continue;
        }
        if RAW.contains(&name) {
            i = find_close(b, i, name);
            continue;
        }
        if drop_quotes && matches!(tag.attr("id"), Some("appendonsend" | "divRplyFwdMsg")) {
            // Outlook: the quoted history starts here and runs to the end.
            break;
        }
        if name == "body" {
            // An unclosed `head` ends at `body`.
            if let Some(at) = stack.iter().rposition(|o| o.name == "head") {
                pop_to(&mut stack, at, &mut hidden, &mut w);
            }
        }
        close_implied(name, &mut stack, &mut hidden, &mut w);
        let style = css.style_of(&tag);
        let own_hidden = hides(&tag, drop_quotes, style.as_deref());
        // `visibility` is inherited, and a child can set it again.
        let visible = style
            .as_deref()
            .and_then(css_visibility)
            .unwrap_or_else(|| stack.last().is_none_or(|o| o.visible));
        let shown = hidden == 0 && !own_hidden && visible;
        if shown {
            open_effect(&tag, &mut w);
        }
        if VOID.contains(&name) {
            continue;
        }
        if stack.len() >= MAX_STACK {
            if overflow.len() < MAX_STACK * 8 {
                overflow.push(tag.name.clone());
            } else {
                overflow_lost = true;
            }
            hidden += 1;
            continue;
        }
        stack.push(Open {
            name: tag.name.clone(),
            hidden: own_hidden,
            visible,
            shown,
        });
        hidden += own_hidden as usize;
    }
    w.close_link();
    (w.out, w.full)
}

fn pop_to(stack: &mut Vec<Open>, at: usize, hidden: &mut usize, w: &mut Writer) {
    while stack.len() > at {
        let o = stack.pop().expect("stack is longer than at");
        *hidden -= o.hidden as usize;
        if o.shown {
            close_effect(&o.name, w);
        }
    }
}

/// The end tags that HTML implies: a block closes an open `p`, an `li` closes
/// the `li` before it, and a table cell or row closes the one before it.
fn close_implied(name: &str, stack: &mut Vec<Open>, hidden: &mut usize, w: &mut Writer) {
    let (targets, fence): (&[&str], &[&str]) = if name == "li" {
        (&["li"], &["ul", "ol"])
    } else if name == "td" || name == "th" {
        (&["td", "th"], &["tr", "table"])
    } else if name == "tr" {
        (&["tr"], &["table"])
    } else if PARA.contains(&name) || LINE.contains(&name) || name == "hr" {
        (&["p"], &[])
    } else {
        return;
    };
    for at in (0..stack.len()).rev() {
        let n = stack[at].name.as_str();
        if targets.contains(&n) {
            pop_to(stack, at, hidden, w);
            return;
        }
        // A `p` closes only through inline elements.
        if fence.contains(&n) || (fence.is_empty() && (PARA.contains(&n) || LINE.contains(&n))) {
            return;
        }
    }
}

fn open_effect(tag: &Tag, w: &mut Writer) {
    let name = tag.name.as_str();
    match name {
        "br" => w.breaks = (w.breaks + 1).min(2),
        "hr" => {
            w.block(2);
            w.literal("---");
            w.block(2);
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            w.block(2);
            let level = (name.as_bytes()[1] - b'0') as usize;
            w.literal(&format!("{} ", "#".repeat(level)));
        }
        "li" => {
            w.block(1);
            w.literal(&format!("{}- ", "  ".repeat(w.lists.saturating_sub(1))));
        }
        "ul" | "ol" => {
            w.block(if w.lists == 0 { 2 } else { 1 });
            w.lists += 1;
        }
        "blockquote" => {
            w.block(2);
            w.quote += 1;
        }
        "pre" => {
            w.block(2);
            w.pre += 1;
        }
        "td" | "th" => {
            if !w.line_start && w.breaks == 0 {
                w.space = true;
            }
        }
        "a" => {
            w.close_link();
            let href = tag.attr("href").unwrap_or("").trim();
            let lower = href.to_ascii_lowercase();
            if ["http://", "https://", "mailto:"]
                .iter()
                .any(|p| lower.starts_with(p))
            {
                w.link = Some((href.to_string(), w.out.len()));
            }
        }
        "img" => {
            // The image itself goes out: a remote image can be a tracking pixel.
            // The alt text stays, because a person can see it.
            let tiny = ["width", "height"].iter().any(|a| {
                tag.attr(a)
                    .is_some_and(|v| v.trim() == "0" || v.trim() == "1")
            });
            let alt = tag.attr("alt").unwrap_or("").trim();
            if !tiny && !alt.is_empty() {
                w.text(&format!(" {alt} "));
            }
        }
        _ if PARA.contains(&name) => w.block(2),
        _ if LINE.contains(&name) => w.block(1),
        _ => {}
    }
}

fn close_effect(name: &str, w: &mut Writer) {
    match name {
        "a" => w.close_link(),
        "ul" | "ol" => {
            w.lists = w.lists.saturating_sub(1);
            w.block(if w.lists == 0 { 2 } else { 1 });
        }
        "blockquote" => {
            w.block(2);
            w.quote = w.quote.saturating_sub(1);
        }
        "pre" => {
            w.block(2);
            w.pre = w.pre.saturating_sub(1);
        }
        _ if PARA.contains(&name) => w.block(2),
        _ if LINE.contains(&name) => w.block(1),
        _ => {}
    }
}

/// The rules of each `<style>` element. The scan skips comments, other
/// declarations and raw-text elements in the same way as the main pass, so
/// a style tag that a browser ignores adds no rule.
fn read_styles(html: &str) -> Css {
    let b = html.as_bytes();
    let mut css = Css::default();
    let mut i = 0;
    while let Some(n) = b[i..].iter().position(|&c| c == b'<') {
        i += n;
        if html[i..].starts_with("<!--") {
            i = find(b, i + 4, b"-->").map_or(b.len(), |n| n + 3);
            continue;
        }
        if matches!(b.get(i + 1), Some(b'!' | b'?')) {
            i = find(b, i, b">").map_or(b.len(), |n| n + 1);
            continue;
        }
        if !b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            i += 1;
            continue;
        }
        let (tag, end) = parse_tag(html, i + 1);
        i = end;
        if RAW.contains(&tag.name.as_str()) {
            let close = find_close(b, i, &tag.name);
            if tag.name == "style" {
                css.read(&html[floor(html, i)..floor(html, close)]);
            }
            i = close.max(i);
        }
    }
    css
}

/// The rules of the `<style>` elements that the converter applies. Only a
/// top-level rule with a single class (`.name`) or a single ID (`#name`) as
/// its selector counts. Other rules are ignored, so their content shows.
#[derive(Default)]
struct Css {
    /// Each rule as its selector and its declarations, in the order of the sheet.
    rules: Vec<(String, String)>,
}

/// The most style rules that the converter keeps for one message.
const MAX_CSS_RULES: usize = 256;
/// The most bytes of declarations that the converter reads from one rule or
/// one `style` attribute.
const MAX_CSS_DECLS: usize = 4096;

impl Css {
    fn read(&mut self, style: &str) {
        // Only top-level rules count. A rule inside `@media` (a nested block)
        // applies only on some screens, so the whole block goes out.
        let (mut depth, mut start, mut open) = (0usize, 0, 0);
        for (i, c) in style.bytes().enumerate() {
            match c {
                b'{' => {
                    if depth == 0 {
                        open = i;
                    }
                    depth += 1;
                }
                b'}' if depth == 0 => start = i + 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let selectors = style[start..open].rsplit(';').next().unwrap_or("");
                        if !self.rule(selectors, &style[open + 1..i]) {
                            return;
                        }
                        start = i + 1;
                    }
                }
                _ => {}
            }
        }
    }

    /// Read one top-level rule. False when the rule limit is reached.
    fn rule(&mut self, selectors: &str, body: &str) -> bool {
        if selectors.contains('@') || body.contains('{') {
            return true;
        }
        let body = &body[..floor(body, MAX_CSS_DECLS)];
        for sel in selectors.split(',') {
            let sel = sel.trim();
            // Only a single class or ID: a compound selector (`.a.b`, `p.x`)
            // or a combinator matches fewer elements than its last name.
            let name = sel.get(1..).unwrap_or("");
            let simple = name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
            if !sel.starts_with(['.', '#']) || name.is_empty() || !simple {
                continue;
            }
            if self.rules.len() >= MAX_CSS_RULES {
                return false;
            }
            self.rules.push((sel.to_string(), body.to_string()));
        }
        true
    }

    /// The declarations that apply to an element, in cascade order: the
    /// `hidden` attribute, then the class rules, then the ID rules (each in
    /// sheet order), then the `style` attribute. In this order, the last
    /// declaration of a property wins, and an `!important` one wins over a
    /// normal one (see [`css_get`]). `None` if nothing applies.
    fn style_of(&self, tag: &Tag) -> Option<String> {
        let (id, class, inline) = (tag.attr("id"), tag.attr("class"), tag.attr("style"));
        let hidden = tag.attr("hidden").is_some();
        if !hidden && id.is_none() && class.is_none() && inline.is_none() {
            return None;
        }
        // The `hidden` attribute is a rule of the browser: an author rule wins over it.
        let mut s = String::from(if hidden { "display:none" } else { "" });
        let mut add = |body: &str| {
            s.push(';');
            s.push_str(body);
        };
        if let Some(class) = class {
            for (sel, body) in &self.rules {
                let hit = sel
                    .strip_prefix('.')
                    .is_some_and(|n| class.split_whitespace().any(|c| c == n));
                if hit {
                    add(body);
                }
            }
        }
        if let Some(id) = id {
            for (sel, body) in &self.rules {
                if sel.strip_prefix('#') == Some(id.trim()) {
                    add(body);
                }
            }
        }
        if let Some(inline) = inline {
            add(&inline[..floor(inline, MAX_CSS_DECLS)]);
        }
        Some(s)
    }
}

/// True if a person does not see the element and all its content, or if the
/// content is a signature or (with `drop_quotes`) quoted history. `style` is
/// the result of [`Css::style_of`].
fn hides(tag: &Tag, drop_quotes: bool, style: Option<&str>) -> bool {
    if matches!(tag.name.as_str(), "svg" | "math" | "head") || style.is_some_and(css_hidden) {
        return true;
    }
    let class = tag.attr("class").unwrap_or("");
    let has = |names: &[&str]| class.split_whitespace().any(|c| names.contains(&c));
    if has(&["gmail_signature", "moz-signature"]) {
        return true;
    }
    drop_quotes
        && (has(&["gmail_quote", "yahoo_quoted", "moz-cite-prefix"])
            || (tag.name == "blockquote" && tag.attr("type") == Some("cite")))
}

/// The largest char boundary at or below `i`.
fn floor(s: &str, i: usize) -> usize {
    (0..=i.min(s.len()))
        .rev()
        .find(|&k| s.is_char_boundary(k))
        .unwrap_or(0)
}

/// The value of each property in a declaration list, as the cascade picks
/// it: the last `!important` declaration, else the last declaration.
fn css_get(decls: &[(&str, &str)], p: &str) -> Option<String> {
    let all = decls.iter().rev().filter(|(k, _)| *k == p);
    let important = all.clone().find(|(_, v)| v.ends_with("!important"));
    important
        .or_else(|| all.clone().next())
        .map(|(_, v)| v.trim_end_matches("!important").to_string())
}

fn css_decls(style: &str) -> String {
    style
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// `Some(false)` if the style sets `visibility: hidden` (or `collapse`),
/// `Some(true)` if it sets `visibility: visible`. A child can show itself
/// again inside a parent with `visibility: hidden`.
fn css_visibility(style: &str) -> Option<bool> {
    let s = css_decls(style);
    let decls: Vec<(&str, &str)> = s.split(';').filter_map(|d| d.split_once(':')).collect();
    match css_get(&decls, "visibility")?.as_str() {
        "hidden" | "collapse" => Some(false),
        "visible" => Some(true),
        _ => None,
    }
}

/// True if the style hides the element and all its content.
fn css_hidden(style: &str) -> bool {
    let s = css_decls(style);
    let decls: Vec<(&str, &str)> = s.split(';').filter_map(|d| d.split_once(':')).collect();
    let get = |p: &str| css_get(&decls, p);
    let zero = |v: &str| {
        let n: String = v
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        n.parse::<f64>().is_ok_and(|x| x == 0.0)
    };
    let clipped = get("overflow").as_deref() == Some("hidden")
        && ["height", "max-height", "width", "max-width"]
            .iter()
            .any(|p| get(p).is_some_and(|v| zero(&v)));
    get("display").as_deref() == Some("none")
        || get("font-size").is_some_and(|v| zero(&v))
        || get("opacity").is_some_and(|v| zero(&v))
        || clipped
}

/// Parse a start or end tag from its name. Return the tag and the index after `>`.
fn parse_tag(s: &str, start: usize) -> (Tag, usize) {
    let b = s.as_bytes();
    let mut i = start;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b':') {
        i += 1;
    }
    let name = s[start..i].to_ascii_lowercase();
    let mut attrs = Vec::new();
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        if b[i] == b'>' {
            i += 1;
            break;
        }
        let ks = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        if i == ks {
            // A lone `=`.
            i += 1;
            continue;
        }
        let key = s[ks..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = "";
        if b.get(i) == Some(&b'=') {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            match b.get(i) {
                Some(&q @ (b'"' | b'\'')) => {
                    let vs = i + 1;
                    let ve = b[vs..]
                        .iter()
                        .position(|&c| c == q)
                        .map_or(b.len(), |n| vs + n);
                    value = &s[vs..ve];
                    i = (ve + 1).min(b.len());
                }
                _ => {
                    let vs = i;
                    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                        i += 1;
                    }
                    value = &s[vs..i];
                }
            }
        }
        // Keep only the attributes that the converter reads, so that a tag
        // with very many attributes costs no memory.
        if USED_ATTRS.contains(&key.as_str()) && attrs.len() < USED_ATTRS.len() * 2 {
            attrs.push((key, decode_entities(value).into_owned()));
        }
    }
    (Tag { name, attrs }, i)
}

fn find(b: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    b.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|n| from + n)
}

/// The index after the end tag `</name ...>`, or the end of the input.
fn find_close(b: &[u8], from: usize, name: &str) -> usize {
    let mut at = from;
    while let Some(n) = find(b, at, b"</") {
        let rest = &b[n + 2..];
        // `</scripture>` does not end `script`: a delimiter must follow the name.
        let ends = rest
            .get(name.len())
            .is_none_or(|c| c.is_ascii_whitespace() || matches!(c, b'>' | b'/'));
        if rest.len() >= name.len()
            && rest[..name.len()].eq_ignore_ascii_case(name.as_bytes())
            && ends
        {
            return find(b, n, b">").map_or(b.len(), |e| e + 1);
        }
        at = n + 2;
    }
    b.len()
}

/// Decode HTML character references (`&amp;`, `&#233;`, `&#xE9;`).
pub fn decode_entities(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        match entity(tail) {
            Some((c, len)) => {
                out.push(c);
                rest = &tail[len..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

fn entity(t: &str) -> Option<(char, usize)> {
    let b = t.as_bytes();
    if b.first() == Some(&b'#') {
        let hex = matches!(b.get(1), Some(b'x' | b'X'));
        let start = 1 + hex as usize;
        let digits = b[start..]
            .iter()
            .take_while(|c| {
                if hex {
                    c.is_ascii_hexdigit()
                } else {
                    c.is_ascii_digit()
                }
            })
            .count();
        if digits == 0 || digits > 8 {
            return None;
        }
        let n = u32::from_str_radix(&t[start..start + digits], if hex { 16 } else { 10 }).ok()?;
        let mut len = start + digits;
        if b.get(len) == Some(&b';') {
            len += 1;
        }
        let c = char::from_u32(n)
            .filter(|c| *c != '\0')
            .unwrap_or('\u{fffd}');
        return Some((c, len));
    }
    let n = b
        .iter()
        .take(12)
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    let name = &t[..n];
    let c = NAMED.iter().find(|(k, _)| *k == name).map(|(_, c)| *c)?;
    if b.get(n) == Some(&b';') {
        Some((c, n + 1))
    } else if ["amp", "lt", "gt", "quot", "nbsp", "copy", "reg"].contains(&name) {
        // Old HTML permits these names without `;`.
        Some((c, n))
    } else {
        None
    }
}

const NAMED: [(&str, char); 58] = [
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("nbsp", '\u{a0}'),
    ("ensp", ' '),
    ("emsp", ' '),
    ("thinsp", ' '),
    ("zwnj", '\u{200c}'),
    ("zwj", '\u{200d}'),
    ("shy", '\u{ad}'),
    ("copy", '©'),
    ("reg", '®'),
    ("trade", '™'),
    ("hellip", '…'),
    ("mdash", '—'),
    ("ndash", '–'),
    ("lsquo", '‘'),
    ("rsquo", '’'),
    ("ldquo", '“'),
    ("rdquo", '”'),
    ("sbquo", '‚'),
    ("bdquo", '„'),
    ("laquo", '«'),
    ("raquo", '»'),
    ("bull", '•'),
    ("middot", '·'),
    ("euro", '€'),
    ("pound", '£'),
    ("yen", '¥'),
    ("cent", '¢'),
    ("sect", '§'),
    ("deg", '°'),
    ("plusmn", '±'),
    ("times", '×'),
    ("divide", '÷'),
    ("para", '¶'),
    ("rarr", '→'),
    ("larr", '←'),
    ("agrave", 'à'),
    ("aacute", 'á'),
    ("acirc", 'â'),
    ("auml", 'ä'),
    ("ccedil", 'ç'),
    ("egrave", 'è'),
    ("eacute", 'é'),
    ("ecirc", 'ê'),
    ("iacute", 'í'),
    ("ntilde", 'ñ'),
    ("oacute", 'ó'),
    ("ouml", 'ö'),
    ("uacute", 'ú'),
    ("uuml", 'ü'),
    ("szlig", 'ß'),
    ("Auml", 'Ä'),
    ("Ouml", 'Ö'),
    ("Uuml", 'Ü'),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn md(h: &str) -> String {
        to_markdown(h, true).0
    }

    #[test]
    fn blocks_lists_and_links() {
        assert_eq!(
            md("<h2>Order</h2><p>Hi&nbsp;Ana,<br>your order   is <b>ready</b>.</p><ul><li>One<li>Two</ul><p>Bye</p>"),
            "## Order\n\nHi\u{a0}Ana,\nyour order is ready.\n\n- One\n- Two\n\nBye"
        );
        assert_eq!(
            md("<a href=\"https://evil.example.net/x\">https://bank.example.com</a>"),
            "[https://bank.example.com](https://evil.example.net/x)"
        );
        assert_eq!(
            md("<a href='https://a.example.com/'>https://a.example.com/</a>"),
            "https://a.example.com/"
        );
        assert_eq!(md("<a href=\"javascript:run()\">Click</a>"), "Click");
        assert_eq!(
            md("<a href=\"mailto:a@example.com\">a@example.com</a>"),
            "a@example.com"
        );
        assert_eq!(
            md("<table><tr><td>A</td><td>B</td></tr><tr><td>C</td></tr></table>"),
            "A B\nC"
        );
    }

    #[test]
    fn hidden_and_raw_content_goes_out() {
        let h = "<head><title>T</title><style>p{}</style></head><body>\
                 <script>alert(1)</script><div style=\"display: none !important\">secret</div>\
                 <span style='font-size:0px'>tiny</span><span style='font-size:0.9em'>small</span>\
                 <div hidden>h</div><svg><text>s</text></svg>\
                 <div style=\"max-height:0;overflow:hidden\">pre</div><!-- c -->ok</body>";
        assert_eq!(md(h), "smallok");
        assert_eq!(md("<html><head><style>x</style><body>shown"), "shown");
    }

    #[test]
    fn a_block_ends_a_hidden_paragraph() {
        // A browser closes the `p` at the `div`, so a person sees "shown".
        assert_eq!(
            md("<p style=display:none>hidden<span><div>shown</div>"),
            "shown"
        );
    }

    #[test]
    fn tracking_images_go_out_alt_text_stays() {
        assert_eq!(
            md("Hi<img src=\"https://t.example.com/o.gif\" width=1 height=1 alt=\"x\"><img src=\"logo.png\" alt=\"Shop logo\">"),
            "Hi Shop logo"
        );
    }

    #[test]
    fn quotes_and_signatures() {
        let h = "<div>Yes</div><div class=\"gmail_signature\">Ana<br>CEO</div>\
                 <div class=\"gmail_quote\"><div>On Mon wrote:</div><blockquote>old</blockquote></div>";
        assert_eq!(to_markdown(h, true).0, "Yes");
        assert_eq!(to_markdown(h, false).0, "Yes\nOn Mon wrote:\n\n> old");
        assert_eq!(
            md("<p>Top</p><div id=\"appendonsend\"></div><p>old</p>"),
            "Top"
        );
        assert_eq!(md("<blockquote type=\"cite\">old</blockquote>new"), "new");
    }

    #[test]
    fn hidden_text_past_the_stack_limit_stays_hidden() {
        let deep = "<span>".repeat(MAX_STACK);
        let h = format!(
            "{deep}<span style=\"display:none\">secret</span>{}",
            "</span>".repeat(MAX_STACK)
        );
        assert!(!md(&h).contains("secret"));
        assert_eq!(md(&format!("{h}after")), "after");
        // An end tag that matches nothing does not end the hidden element.
        let h = format!(
            "{deep}<div hidden>a</b>secret</div>{}after",
            "</span>".repeat(MAX_STACK)
        );
        assert_eq!(md(&h), "after");
    }

    #[test]
    fn output_stops_at_the_limit() {
        let h = format!("{}{}", "<blockquote>".repeat(500), "<br>x".repeat(300_000));
        let (out, full) = to_markdown(&h, false);
        assert!(full);
        assert!(out.len() <= OUT_LIMIT + 64, "{}", out.len());
        // A long link target does not pass the limit.
        let long = format!(
            "<a href=\"https://example.org/{}\">x</a>",
            "a".repeat(OUT_LIMIT)
        );
        let (link_out, link_full) = to_markdown(&long, false);
        assert!(
            link_full && link_out.len() <= OUT_LIMIT,
            "{}",
            link_out.len()
        );
        assert!(out
            .lines()
            .nth(1)
            .unwrap()
            .starts_with(&"> ".repeat(MAX_QUOTE)));
        assert!(!out
            .lines()
            .nth(1)
            .unwrap()
            .starts_with(&"> ".repeat(MAX_QUOTE + 1)));
    }

    #[test]
    fn a_longer_end_tag_does_not_end_a_script() {
        assert_eq!(
            md("<script>x</scripture>also</SCRIPT ><p>shown</p>"),
            "shown"
        );
    }

    #[test]
    fn style_rules_hide_classes_and_ids() {
        let h = "<style>.hidden{display:none} #pre, .x { font-size: 0 } @media (max-width:1px) { .m{display:none} }\
                 .shown{color:red}</style><div class=\"a hidden\">secret</div><span id=pre>pre</span>\
                 <p class=x>tiny</p><div class=m>m</div><div class=shown>ok</div>";
        assert_eq!(md(h), "m\nok");
        // A rule after the element hides it too.
        assert_eq!(
            md("<div class=h>secret</div>ok<style>.h{display:none}</style>"),
            "ok"
        );
        // An important declaration wins over a later normal one.
        assert_eq!(
            md(
                "<div style=\"display:none!important;display:block\">secret</div>\
                <style>.i{display:none !important}.i{display:block}</style><p class=i>also</p>ok"
            ),
            "ok"
        );
        // A child can show itself inside a parent with visibility: hidden.
        assert_eq!(
            md("<div style=\"visibility:hidden\">no <span style=\"visibility:visible\">Shown</span> no</div>\
                <style>.v{visibility:hidden}</style><p class=v>gone</p>"),
            "Shown"
        );
        // A compound selector does not hide an element with only one of its names.
        assert_eq!(
            md("<style>.a.b{display:none} p.c{display:none}</style><div class=b>Visible</div><p class=c>too</p>"),
            "Visible\n\ntoo"
        );
        // The cascade: the style attribute wins over a rule, an ID rule wins
        // over a class rule, an important rule wins over a normal style
        // attribute, and an author rule wins over the hidden attribute.
        assert_eq!(
            md("<style>.k{display:none} #i{display:block} .c{display:none} .imp{display:none!important}\
                .shown{display:block}</style>\
                <p class=k style=\"display:block\">one</p><p class=c id=i>two</p>\
                <p class=imp style=\"display:block\">gone</p><p hidden class=shown>three</p><p hidden>gone</p>"),
            "one\n\ntwo\n\nthree"
        );
        // A later rule for the same name wins.
        assert_eq!(
            md("<style>.x{display:none}.x{display:block}.y{display:block}</style>\
                <style>.y{display:none}.y{color:red}</style><p class=x>Visible</p><p class=y>Gone</p>"),
            "Visible"
        );
        // A browser ignores a style tag inside a comment or a script.
        assert_eq!(
            md(
                "<!-- <style>.x{display:none}</style> --><p class=x>Visible</p>\
                <script>'<style>.y{display:none}</style>'</script><p class=y>Too</p>"
            ),
            "Visible\n\nToo"
        );
        // Two rules inside a print block: neither one hides text on a screen.
        assert_eq!(
            md("<style>@media print { .a {display:none} .b {display:none} } .c{display:none}</style>\
                <p class=a>A</p><p class=b>B</p><p class=c>C</p>"),
            "A\n\nB"
        );
    }

    #[test]
    fn entities() {
        assert_eq!(
            decode_entities("a &amp b &lt;&#233;&#x41;&eacute;&nope; &"),
            "a & b <éAé&nope; &"
        );
    }

    #[test]
    fn broken_html_does_not_panic() {
        for h in [
            "<",
            "<a",
            "<a href=",
            "<a href=\"x",
            "</",
            "<!--",
            "<p <p>",
            "<script>",
            "é<é>",
            "<div =x>",
        ] {
            md(h);
        }
    }
}
