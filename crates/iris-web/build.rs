//! Embed the build of the web client (`client/dist`) in the program.
//!
//! If `client/dist/index.html` does not exist (for example, on a machine with
//! no Node), embed no files. The router then serves a fallback page.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    let dist = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../client/dist");
    // Cargo scans the folder for changes. If the folder is missing, Cargo runs
    // this script again on each build, so a new client build is never missed.
    println!("cargo:rerun-if-changed={}", dist.display());

    let mut files = Vec::new();
    if dist.join("index.html").is_file() {
        collect(&dist, &mut files);
        files.sort();
    }
    let mut out = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for file in &files {
        let rel = file.strip_prefix(&dist).unwrap().to_str().unwrap();
        let abs = fs::canonicalize(file).unwrap();
        writeln!(
            out,
            "    ({:?}, include_bytes!({:?})),",
            format!("/{rel}"),
            abs
        )
        .unwrap();
    }
    out.push_str("];\n");
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("files.rs"),
        out,
    )
    .unwrap();
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, files);
        } else {
            files.push(path);
        }
    }
}
