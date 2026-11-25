//! `index_tokens` splits text as the full-text index does (issue #756).

use iris_store::{index_tokens, Store};

/// The tokens of `text` in an FTS5 table with the tokenizer of
/// `message_fts`, read back through `fts5vocab` in position order.
fn vocab(c: &rusqlite::Connection, text: &str) -> Vec<String> {
    c.execute("DELETE FROM temp.t", []).unwrap();
    c.execute("INSERT INTO temp.t(x) VALUES (?1)", [text])
        .unwrap();
    let mut st = c
        .prepare("SELECT term FROM temp.v ORDER BY offset")
        .unwrap();
    st.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn index_tokens_agree_with_the_index() {
    let dir = std::env::temp_dir().join(format!("iris-store-tokens-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    store
        .read(|c| {
            c.execute_batch(
                "CREATE VIRTUAL TABLE temp.t USING fts5(x, tokenize = 'unicode61 remove_diacritics 2');
                 CREATE VIRTUAL TABLE temp.v USING fts5vocab(temp, t, instance);",
            )?;
            // The cases of the issue, then random text from a pool of hard
            // characters: combining marks, marks that Rust calls letters,
            // other scripts, digits, punctuation and spaces.
            let fixed = [
                "a\u{301}b",
                "a\u{345}b",
                "a\u{903}b",
                "Ça va? Ünïcödé — 1,250.50 USD",
                "",
                "日本語のテキスト",
            ];
            let pool: Vec<char> = "aZ09 \t\n.,;\"'-_é\u{301}\u{345}\u{903}\u{300}ßİıΣσς日本😀\u{200d}\u{fe0f}١٢\u{e000}"
                .chars()
                .collect();
            let mut x: u64 = 0x2545_f491_4f6c_dd1d;
            let mut texts: Vec<String> = fixed.iter().map(|s| s.to_string()).collect();
            for _ in 0..2_000 {
                let mut s = String::new();
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                for k in 0..(x % 24) {
                    let y = x.rotate_left(k as u32 * 5);
                    s.push(pool[(y % pool.len() as u64) as usize]);
                }
                texts.push(s);
            }
            for text in &texts {
                let got = index_tokens(c, text)?;
                let words: Vec<String> = got.iter().map(|t| t.word.clone()).collect();
                assert_eq!(words, vocab(c, text), "{text:?}");
                for t in &got {
                    assert!(text.get(t.range.clone()).is_some(), "{text:?} {t:?}");
                }
            }
            Ok(())
        })
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
