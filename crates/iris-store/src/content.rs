//! The content folder (specification 1, section 5.3): one raw message in each
//! file, at `content/<aa>/<sha256>.eml`.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::Result;

/// The path of a content file, or `None` if `sha256` is not 64 lowercase
/// hexadecimal characters. The check keeps a bad value out of the path.
pub(crate) fn path(root: &Path, sha256: &str) -> Option<PathBuf> {
    let ok = sha256.len() == 64
        && sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    ok.then(|| root.join(&sha256[..2]).join(format!("{sha256}.eml")))
}

/// Streams one raw message to a temporary file and hashes it on the way, so
/// the caller never holds the full message. [`ContentWriter::finish`] moves
/// the file to its final name. If the writer drops before the rename, it
/// deletes the temporary file.
pub struct ContentWriter {
    root: PathBuf,
    tmp: PathBuf,
    file: Option<File>,
    hash: Sha256,
}

impl ContentWriter {
    pub(crate) fn new(root: &Path) -> Result<ContentWriter> {
        let mut rand = [0u8; 8];
        getrandom::fill(&mut rand).map_err(|e| io::Error::other(e.to_string()))?;
        // The temporary file is in the content folder, on the same file
        // system as the final file, so the rename is atomic.
        let tmp = root.join(format!(".tmp-{}", hex(&rand)));
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        Ok(ContentWriter {
            root: root.to_path_buf(),
            tmp,
            file: Some(file),
            hash: Sha256::new(),
        })
    }

    /// Write the file to disk (`fsync`), rename it to its final name, and give
    /// its SHA-256. The rename replaces a file with the same name, so a
    /// fetch again repairs a damaged file (specification 1, section 4.8).
    pub fn finish(mut self) -> Result<String> {
        let file = self.file.take().expect("finish runs one time");
        file.sync_all()?;
        drop(file);
        let sha = hex(&std::mem::take(&mut self.hash).finalize());
        let dest = path(&self.root, &sha).expect("a SHA-256 in hexadecimal");
        let dir = dest.parent().expect("the path has a folder");
        let new_dir = !dir.exists();
        if new_dir {
            crate::make_private_dir(dir)?;
        }
        fs::rename(&self.tmp, &dest)?;
        // The rename is durable only after its folder is on disk too, and a
        // new folder only after the content folder is on disk.
        crate::sync_dir(dir)?;
        if new_dir {
            crate::sync_dir(&self.root)?;
        }
        Ok(sha)
    }
}

impl Write for ContentWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.file.as_mut().expect("not finished").write(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.as_mut().expect("not finished").flush()
    }
}

impl Drop for ContentWriter {
    fn drop(&mut self) {
        // After a rename, the temporary file is not there, and this does nothing.
        let _ = fs::remove_file(&self.tmp);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
