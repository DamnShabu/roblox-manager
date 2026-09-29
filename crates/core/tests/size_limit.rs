//! No Rust source file in the workspace may grow past 600 lines: a file that
//! big is doing too much, and gets split along a real seam instead.

use std::fs;
use std::path::{Path, PathBuf};

const LIMIT: usize = 600;

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name.to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_source_file_exceeds_the_line_limit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    assert!(!files.is_empty(), "found no .rs files under {}", root.display());
    let over: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let lines = fs::read_to_string(f).ok()?.lines().count();
            (lines > LIMIT).then(|| format!("{} ({lines} lines)", f.display()))
        })
        .collect();
    assert!(over.is_empty(), "over {LIMIT} lines: {over:#?}");
}
