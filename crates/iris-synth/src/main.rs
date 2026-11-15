//! Dev tool: write a synthetic mailbox to a folder.
//!
//! `iris-synth [--seed N] [--count N] <out-dir>` writes `mailbox.mbox` (mboxrd)
//! and `truth.jsonl` (one ground-truth line for each message, same order).

use std::io::{BufWriter, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut seed = 1u64;
    let mut count = iris_synth::DEFAULT_COUNT;
    let mut out = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let num = |v: Option<String>| v.and_then(|v| v.parse::<u64>().ok());
        match a.as_str() {
            "--seed" => match num(args.next()) {
                Some(v) => seed = v,
                None => return usage(),
            },
            "--count" => match num(args.next()) {
                Some(v) => count = v as usize,
                None => return usage(),
            },
            _ if out.is_none() && !a.starts_with('-') => out = Some(std::path::PathBuf::from(a)),
            _ => return usage(),
        }
    }
    let Some(dir) = out else { return usage() };
    match write(&dir, seed, count) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("iris-synth: cannot write to {}: {e}", dir.display());
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: iris-synth [--seed N] [--count N] <out-dir>");
    ExitCode::from(2)
}

fn write(dir: &std::path::Path, seed: u64, count: usize) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mb = iris_synth::Mailbox::new(seed, count);
    let mut mbox = BufWriter::new(std::fs::File::create(dir.join("mailbox.mbox"))?);
    let mut truth = BufWriter::new(std::fs::File::create(dir.join("truth.jsonl"))?);
    for i in 0..mb.len() {
        writeln!(mbox, "From synth@example.com Thu Jan  1 00:00:00 1970")?;
        for line in mb.raw(i).split_inclusive(|b| *b == b'\n') {
            // mboxrd: quote each line that starts with ">*From ".
            if line
                .iter()
                .skip_while(|b| **b == b'>')
                .take(5)
                .eq(b"From ".iter())
            {
                mbox.write_all(b">")?;
            }
            mbox.write_all(line)?;
        }
        mbox.write_all(b"\n")?;
        writeln!(truth, "{}", mb.truth(i).json_line())?;
    }
    mbox.flush()?;
    truth.flush()
}
