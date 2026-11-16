# Spec: Clean text of one message

- Date: 2026-11-16
- Card: R1-08
- Status: Built

## Problem

Specification 1, section 7.2, says what the clean text of a message holds. It
does not say how exact each rule is. HTML, CSS and PDF are large formats. A
rule such as "hidden elements go out" can mean the full browser behavior or a
small, fixed subset. Without a closed list, each review finds one more case
where the clean text and a mail client do not agree.

This spec gives the closed list. A case that the list does not name is a
known limit, not a defect.

## Goal

`iris_clean::clean(raw)` gives the clean text of one raw RFC 5322 message.
The result follows the rules below exactly. Memory and time stay bounded for
each input.

## Not in scope

- The full CSS cascade, CSS layout and CSS selectors other than the subset in
  [Hidden HTML](#hidden-html).
- PDF fonts with their own code map (CID fonts, `ToUnicode`). Card R1-25.
- The model and the fixed rules (specification 1, sections 7.3 and 7.4).
- The detection of hostile mail (specification 1, section 6.7).

## Design

### API

```rust
pub fn clean(raw: &[u8]) -> Clean;
pub struct Clean { pub subject: String, pub text: String, pub cut: bool, pub attachments: Vec<Attachment> }
pub struct Attachment { pub name: String, pub mime_type: String, pub size: u64, pub text: Option<String> }
pub fn remove_repeated_end(text: &str, earlier: &[&str]) -> String;
pub const TEXT_LIMIT: usize = 32_000;           // characters of `text`
pub const ATTACHMENT_TEXT_LIMIT: usize = 1_000_000; // bytes of all attachment text
pub const CUT_MARK: &str = "\n\n[Iris cut the text here.]";
```

The functions are pure. They use no network, no file and no model. Mail text
is data: the code never runs or follows text from a message.

### Two steps

1. Step 1 walks the MIME tree. It makes the clean text of each body part, in
   the order of the message, until the text has `TEXT_LIMIT` characters. It
   lists each attachment without a decode.
2. Step 2 reads the attachments in the order of the message. All attachment
   text uses one budget of `ATTACHMENT_TEXT_LIMIT` bytes, in this order. An
   attached message (`message/rfc822`) gets step 1 for its own text. Its text
   uses the budget first, then its own attachments follow it in the list.
   The budget counts bytes of decoded text. For a UTF-16 text attachment,
   the reader reads two source bytes for each byte of budget.

Each budget only goes down. No step gives budget back.

### MIME

- A part is a body part if its type is `text/plain` or `text/html`, it has no
  `Content-Disposition: attachment`, and it has no file name (`filename` or
  `name`). A part with no type is `text/plain`.
- Each other leaf part is an attachment. A multipart part with a file name or
  with `Content-Disposition: attachment` is one attachment too.
- `multipart/alternative`: the walker reads the versions from the last one.
  It uses the first version with clean text that is not empty. A version that
  it does not use adds no text, no attachment and no cut.
- Each other multipart: each part, in order.
- Transfer encodings: `base64` (characters outside the alphabet are skipped),
  `quoted-printable` (an `=` without a valid escape stays), and all others as
  they are.
- Charsets: the labels of the WHATWG Encoding Standard, through `encoding_rs`.
  Text that is valid UTF-8 is read as UTF-8 if it has no label, an unknown
  label, or a label that `encoding_rs` maps to Windows-1252 (for example
  `us-ascii` and `iso-8859-1`). Other text with no label or an unknown label
  is read as Windows-1252.
- Headers: RFC 2047 encoded words, and RFC 2231 parameters.

### A reply

The message is a reply if it has `In-Reply-To` or `References`, or if its
decoded subject starts with one of `Re`, `Aw`, `Sv`, `Vs`, `Antw` or `Réf`
(any case) before its first `:`. An attached message has its own reply state.

### The clean text of one body part

The steps, in order:

1. Decode at most `PART_LIMIT` (2 MiB) bytes. If bytes stay, the text is cut.
2. HTML to Markdown (see [HTML](#html)). The output has at most 512 KiB. Output
   that fits in 512 KiB stays whole. If more output does not fit, the text
   is cut.
3. Keep at most `MAX_LINES` lines. If lines stay, the text is cut. Then
   remove quoted history (see [Quoted history](#quoted-history)).
4. Remove the signature (see [Signatures](#signatures)).
5. Remove tracking parameters from URLs (see [URLs](#urls)).
6. Tidy: remove the characters U+200B, U+200C, U+200D, U+2060, U+FEFF,
   U+034F, U+00AD and U+180E. Change U+00A0 to a space. Remove the spaces at
   line ends. Keep at most one empty line between two lines of text.

The texts of the body parts join with one empty line. When the text has
`TEXT_LIMIT` characters, it stops there, and a later body part with clean
text makes the text cut. A later part with no clean text (for example, only
hidden HTML) makes no cut.

### HTML

- The converter is a tokenizer with an element stack of at most 512 levels.
  The content of an element deeper than 512 levels goes out. The converter
  keeps the names of at most 4,096 elements past that depth. Such an
  element closes at the end tag with its name, as on the stack. Another end
  tag changes nothing. Past those 4,096 names, the converter only counts
  the start tags, and each end tag closes one of them. After the deep
  elements close, the text shows again.
- Implied end tags: a block closes an open `p`, an `li` closes the `li`
  before it, a `td` or `th` closes the cell before it, a `tr` closes the row
  before it, and `body` closes an open `head`.
- If an attribute occurs more than once in a tag, the first one counts, as
  in a browser.
- Comments, `<!...>` and `<?...>` go out.
- These elements go out with their content, as raw text: `script`, `style`,
  `title`, `template`, `textarea`, `xmp`, `noembed`, `noframes`, `iframe`.
  These go out with their content: `head`, `svg`, `math`.
- Links: `[text](href)` for an `http`, `https` or `mailto` target. If the
  text is the target, only the text shows. If the text has more than one
  line, the form is `text (<href>)`. In the target, a space, `(`, `)`, `<`
  and `>` become percent codes (`%20`, `%28`, `%29`, `%3C`, `%3E`). Other
  targets (for example `javascript:`) show the text only.
- Images go out. The `alt` text stays, unless `width` or `height` is `0` or
  `1` (a tracking pixel).
- Headings, lists, block quotes, `pre`, `hr` and line breaks become Markdown.
  At most 8 quote marks show.
- Entities: the numeric forms and the named entities of the table in
  `html.rs`. An unknown name stays as it is.

### Hidden HTML

An element and all its content go out if its declarations give one of these:

- `display: none`
- `font-size: 0` (any unit)
- `opacity: 0`
- `overflow: hidden` together with a `height`, `max-height`, `width` or
  `max-width` of 0

`visibility: hidden` or `collapse` hides the text of the element and of its
children. A child with `visibility: visible` shows again.

The declarations of an element, in this order:

1. `display: none` if the element has the `hidden` attribute.
2. The rules of the `<style>` elements whose selector is one class
   (`.name`) that the element has, in the order of the sheet.
3. The rules whose selector is the ID (`#name`) of the element, in order.
4. The `style` attribute.

For each property, the last `!important` declaration wins. If there is none,
the last declaration wins.

The style rules that count: a top-level rule with a selector list. Each
selector of the list that is one class or one ID counts. A rule inside an
`@` block (for example `@media`) does not count. A selector with more than
one part (`.a.b`, `p.x`, `div .x`, `.x:hover`, `[a]`) does not count. A
`<style>` element inside a comment or a raw-text element does not count. At
most 256 rules count, and at most 4,096 selectors over all rules. A rule
counts once, for all of its selectors. At most 4,096 bytes of the
declarations of each rule count. The selector list has no byte limit of its
own: the 4,096 selectors and the 2 MiB part limit bound it. Comments and
the content of strings in CSS have no effect on rules or declarations.

These go out also: `class` `gmail_signature` and `moz-signature`. In a
reply: `class` `gmail_quote`, `yahoo_quoted` and `moz-cite-prefix`, a
`blockquote` with `type="cite"`, and all content from an element with `id`
`appendonsend` or `divRplyFwdMsg`.

### Quoted history

The search reads the lines in order and stops at the first of these:

- A forward marker: a line that starts with `---` and holds
  "forwarded message", or the line "Begin forwarded message:". The text from
  this line stays.
- In a reply: an "-----Original Message-----" line (also "-----Ursprüngliche
  Nachricht-----"), or an Outlook header block (`From:`, then `Sent:` or
  `Date:` in the 4 lines after `From:`, then `Subject:` or `To:` in the 5
  lines after that line). The history starts at this line.
- An attribution line: a line, or two lines that start with `On `, that end
  with `wrote:` or the same word in French, German, Spanish, Italian, Danish
  or Norwegian or Swedish, Dutch or Polish. It is history only if the next
  line that is not empty starts with `>`, or if the message is a reply and
  the line starts with one of `On `, `Le `, `Am `, `El `, `Il `, `Op `,
  `Den `, `W dniu `, `Em `.

Before the history (or a forward marker):

- In a reply, each line that starts with `>` goes out.
- Not in a reply, only the block of `>` lines at the end goes out.

Rule lines (`_`, `-`, `=` and `*` only) and empty lines just before the
history go out.

### Signatures

- The text after the last `-- ` line (RFC 3676) goes out.
- The text after the last `--` line goes out, if at most 10 lines follow it.
- Last lines that start with "Sent from my ", "Get Outlook for ",
  "Sent from Mail for Windows" or "Sent from Yahoo Mail" go out.
- `remove_repeated_end(text, earlier)` removes the end block that `text`
  shares with one of the earlier clean texts of the same sender in the
  thread. The block has 2 or more lines that are not empty, and some text
  stays. The caller gives `earlier`.

### URLs

- A URL starts at `http://` or `https://`, in any case.
- In prose, a URL ends at white space or at one of ``)<>"'[]{}|\^` ``. A `)`
  that closes a `(` of the URL stays. The last characters `.,;:!?` are prose,
  not URL.
- In a link target (after `](` or `(<`), each character up to the end of the
  target is URL.
- The query parameters that go out: each name that starts with `utm_`, and
  each name of the `TRACKING` list in `url.rs`. Names have no case. The order
  of the other parameters stays, empty entries too. The fragment stays.

### Attachments

- `name`: the file name, or for an attached message with no file name, its
  subject. Control characters go out. At most 255 characters.
- `size`: the decoded size, counted without a decode.
- `text`:
  - `text/plain`: the decoded text.
  - `message/rfc822`: the clean text of the attached message, from at most
    `INNER_LIMIT` (2 MiB) decoded bytes. If bytes stay, the text gets
    `CUT_MARK`, and the attachments of the attached message are not listed.
  - `application/pdf`: see [PDF](#pdf).
  - Each other type: `None`.
  - `None` also when the budget is spent before the attachment.
    The attachments of an attached message are listed also then.
- When a text reaches the budget, it stops there and ends with `CUT_MARK`.
  The mark is not part of the budget.

### PDF

A PDF gives text only if its decoded size is at most `PDF_LIMIT` (10 MiB) and
its first 1 KiB holds `%PDF`.

- Streams: the reader finds each `stream` keyword. Its dictionary is the
  text after the last object header (`<number> <number> obj`) before it, at
  most 4 KiB back and not before the data of the stream before. The data
  ends at the
  direct `/Length` key (not `/Length1` or another key that starts with
  `/Length`) of the stream dictionary, outside its strings, comments and
  inner dictionaries, if `endstream` follows that
  length. If not, the data ends at the next `endstream`.
- A stream with no filter is read in place. A stream whose `/Filter` is
  `/FlateDecode` or `[/FlateDecode]` is inflated, at most 4 MiB for each stream and 32 MiB for each PDF. A
  stream with another filter is skipped.
- Streams with `/Image`, `/Length1`, `/Length2`, `/Metadata`, `/XRef`,
  `/ObjStm`, `/FontFile` or `/EmbeddedFile` are skipped.
- The reader reads the names of a stream dictionary (`/Length`, `/Filter`
  and the names above) only outside its strings, comments and inner
  dictionaries.
- Text: the strings of `Tj`, `TJ`, `'` and `"`, only between `BT` and `ET`,
  as Windows-1252. A text rendering mode (`Tr`) of 3 or 7 hides the strings
  until the next `Tr`. `T*`, `Tm`, `ET`, `'`, `"`, and `Td` or `TD` with a
  vertical move start a new line. A gap of more than 200 units in a `TJ`
  array is a space.
- The text must have 3 or more letters, and 85% or more of its characters
  must be letters, digits, white space or one of
  `.,;:!?'"()-/%&@€$£#*+=`. If not, `text` is `None`.
- The check reads at least 4,096 bytes of text, also when less budget is
  left. Then the text is clipped to the budget. If the reader stops at a
  limit before the end of the PDF (the text budget, the stream limit or
  the 32 MiB inflate limit of each PDF), the text gets `CUT_MARK`, also when
  trimmed white space makes the text shorter than the budget.

### Known limits

These cases give a clean text that differs from what a person sees in a mail
client. They are not defects of this design:

- HTML text that hides by other means stays: the same color as the
  background, a position off the screen, `clip` or `clip-path`, a transform,
  `text-indent`, a small font size that is not 0, `line-height: 0`, a width
  or height of 0 without `overflow: hidden`, CSS generated content, and
  external style sheets.
- A style rule with a selector that is not one class or one ID does not
  apply. So both of these can occur: hidden text stays, and text that such a
  rule shows again stays hidden.
- Rules inside `@media` do not apply, also for the screen of the person.
- A reply in a language or a client that the lists above do not name keeps
  its quoted history. A message that is not a reply keeps the quotes between
  its own lines.
- A signature without a `--` line or a phone line stays, until
  `remove_repeated_end` finds it in a thread.
- PDF: text hidden by a white fill, by clipping, by a position off the page,
  by a small font or by an object on top of it stays. A content stream that
  no page uses gives its text too. Text in a font with its own code map
  fails the quality check (card R1-25). An encrypted PDF gives no text.
- The model reads clean text as data, never as instructions (specification
  1). So hidden text that stays is a quality limit, not a security hole.

### Limits and the cut

| Limit | Value | When it acts |
|---|---|---|
| `TEXT_LIMIT` | 32,000 characters | The body text stops, with `CUT_MARK` if clean text goes out. |
| `PART_LIMIT` | 2 MiB | One body part decodes no more, with `CUT_MARK`. |
| HTML output | 512 KiB | The Markdown of one part stops, with `CUT_MARK`. |
| `MAX_LINES` | 100,000 lines | The quote and signature steps read no more lines of one part, with `CUT_MARK`. |
| `MAX_PARTS` | 500 parts | Later parts are not read, with `CUT_MARK`. |
| `MAX_DEPTH` | 8 levels | Deeper parts are not read, with `CUT_MARK`. |
| `ATTACHMENT_TEXT_LIMIT` | 1,000,000 bytes | Attachment text stops, with `CUT_MARK` on that text. |
| `INNER_LIMIT` | 2 MiB | An attached message is read no more, with `CUT_MARK` on its text. |
| `PDF_LIMIT` | 10 MiB | A larger PDF gives no text. |
| Headers | 256 KiB for each block, 16 KiB for each value | The rest of the block or of the value is not read. |

`Clean.cut` is true exactly when the crate adds `CUT_MARK` to the end of
`Clean.text`.

## Options not chosen

- A full HTML parser and CSS engine (`html5ever`, a browser engine): many
  dependencies and much more memory for each message. Memory has priority.
- A PDF crate (`lopdf`, `pdf-extract`): they can panic on hostile input, and
  the release build stops the program on a panic.
- One shared body budget that each part takes from, with give-back for
  versions that are not used: earlier builds did this, and each review found
  a new order in which one part took the budget of another.

## Impact

- New crate `crates/iris-clean/`. One row in the folder table of `AGENTS.md`.
- No caller yet. The engine (card R1-10) and the conversations (W15) call it.
- Dependencies: `encoding_rs` 0.8 and `miniz_oxide` 0.9.

## Assumptions

- The model treats mail text as data (specification 1). Evidence:
  `docs/spec/01-mirror-store-forms.md`, section 7.4.
- Mail clients use the last `multipart/alternative` version that they can
  show (RFC 2046, section 5.1.4).
- Gmail limits a message to 50 MB, so a linear time in the message size is
  enough. Evidence: pending. The design tolerates a larger message: the
  memory stays bounded, only the time grows.

## External writes

None.

## Open questions

None.

## Acceptance criteria

- The golden files of `tests/golden/` match for the 15 synthetic samples.
  A change in a rule above makes one of them go red.
- The tests of `tests/messages.rs` and the unit tests cover each row of the
  limit table, the cascade order, the reply rules, the link targets and the
  attachment budget order. A change of one of these rules makes a test go red.
- `tests/bounds.rs` measures the heap peak: below 8 MiB for a 36 MB message
  with HTML and an attachment, below 16 MiB for a PDF of 9 MiB. A copy of a
  whole part or of an attachment makes it go red.

## Verify

```bash
cargo test -p iris-clean
cargo clippy -p iris-clean --all-targets -- -D warnings
```

## Rollout

None.
