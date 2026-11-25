//! The words of a text as the full-text index splits them (issue #756).
//!
//! `message_fts` uses the FTS5 tokenizer `unicode61 remove_diacritics 2`
//! (migration 1). No Rust library has the same Unicode tables (checked for
//! #403: `unicode-segmentation`, `unicode-general-category`, `regex`,
//! `aho-corasick`). So [`index_tokens`] calls that tokenizer of SQLite
//! through the FTS5 C API, and gives the same tokens as the index.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::ops::Range;
use std::ptr;

use rusqlite::{ffi, Connection};

use crate::{Error, Result};

/// The arguments of the tokenizer of `message_fts` (migration 1).
const TOKENIZER: &CStr = c"unicode61";
const ARGS: [&CStr; 2] = [c"remove_diacritics", c"2"];

/// One token of [`index_tokens`]: the folded word of the index, and the
/// byte range of the text that gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub word: String,
    pub range: Range<usize>,
}

fn fail(what: &str, rc: c_int) -> Error {
    Error::Query(format!("fts5 tokenizer: {what} ({rc})"))
}

/// The tokens of `text`, in order, as `message_fts` indexes it.
pub fn index_tokens(c: &Connection, text: &str) -> Result<Vec<Token>> {
    let len = c_int::try_from(text.len()).map_err(|_| fail("text too long", 0))?;
    // SAFETY: each pointer below comes from SQLite for this connection and
    // is used only while `c` is borrowed. The tokenizer instance is deleted
    // before the return. The callback gets only `out` as its context.
    unsafe {
        let api = fts5_api(c)?;
        let mut user: *mut c_void = ptr::null_mut();
        let mut tok: ffi::fts5_tokenizer = std::mem::zeroed();
        let find = (*api)
            .xFindTokenizer
            .ok_or_else(|| fail("no xFindTokenizer", 0))?;
        let rc = find(api, TOKENIZER.as_ptr(), &mut user, &mut tok);
        if rc != ffi::SQLITE_OK {
            return Err(fail("unicode61 not found", rc));
        }
        let (create, delete, tokenize) = match (tok.xCreate, tok.xDelete, tok.xTokenize) {
            (Some(a), Some(b), Some(t)) => (a, b, t),
            _ => return Err(fail("incomplete tokenizer", 0)),
        };
        let mut args: [*const c_char; 2] = [ARGS[0].as_ptr(), ARGS[1].as_ptr()];
        let mut inst: *mut ffi::Fts5Tokenizer = ptr::null_mut();
        let rc = create(user, args.as_mut_ptr(), 2, &mut inst);
        if rc != ffi::SQLITE_OK || inst.is_null() {
            return Err(fail("create", rc));
        }
        let mut out: Vec<Token> = Vec::new();
        let rc = tokenize(
            inst,
            (&mut out as *mut Vec<Token>).cast(),
            ffi::FTS5_TOKENIZE_DOCUMENT,
            text.as_ptr().cast(),
            len,
            Some(on_token),
        );
        delete(inst);
        if rc != ffi::SQLITE_OK {
            return Err(fail("tokenize", rc));
        }
        Ok(out)
    }
}

unsafe extern "C" fn on_token(
    ctx: *mut c_void,
    _flags: c_int,
    token: *const c_char,
    n: c_int,
    start: c_int,
    end: c_int,
) -> c_int {
    let out = &mut *ctx.cast::<Vec<Token>>();
    let bytes = std::slice::from_raw_parts(token.cast::<u8>(), n.max(0) as usize);
    out.push(Token {
        word: String::from_utf8_lossy(bytes).into_owned(),
        range: start.max(0) as usize..end.max(0) as usize,
    });
    ffi::SQLITE_OK
}

/// The `fts5_api` of the connection: `SELECT fts5(?1)` with a pointer
/// bind of the type `fts5_api_ptr` (FTS5 documentation, section 7.1).
unsafe fn fts5_api(c: &Connection) -> Result<*mut ffi::fts5_api> {
    let db = c.handle();
    let mut stmt: *mut ffi::sqlite3_stmt = ptr::null_mut();
    let rc = ffi::sqlite3_prepare_v2(
        db,
        c"SELECT fts5(?1)".as_ptr(),
        -1,
        &mut stmt,
        ptr::null_mut(),
    );
    if rc != ffi::SQLITE_OK {
        return Err(fail("prepare", rc));
    }
    let mut api: *mut ffi::fts5_api = ptr::null_mut();
    let rc = ffi::sqlite3_bind_pointer(
        stmt,
        1,
        (&mut api as *mut *mut ffi::fts5_api).cast(),
        c"fts5_api_ptr".as_ptr(),
        None,
    );
    let step = if rc == ffi::SQLITE_OK {
        ffi::sqlite3_step(stmt)
    } else {
        rc
    };
    ffi::sqlite3_finalize(stmt);
    if step != ffi::SQLITE_ROW || api.is_null() {
        return Err(fail("no fts5_api", step));
    }
    Ok(api)
}
