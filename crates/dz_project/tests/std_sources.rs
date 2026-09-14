//! The standard library is compiled into `dz` from a list written by hand (spec §14.4). A
//! file added under `src/std` and not listed would silently not exist, so the list is held to
//! the directory.

use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "dz") {
            out.push(path);
        }
    }
}

#[test]
fn every_std_file_is_listed_and_every_listed_file_exists() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/std");
    let mut on_disk: Vec<String> = Vec::new();
    let mut files = Vec::new();
    walk(&root, &mut files);
    for f in files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        on_disk.push(format!("<std>/{rel}"));
    }
    on_disk.sort();
    let mut listed: Vec<String> =
        dz_project::STD_SOURCES.iter().map(|(p, _)| (*p).to_owned()).collect();
    listed.sort();
    assert_eq!(listed, on_disk, "STD_SOURCES and src/std disagree");
}

#[test]
fn each_listed_file_holds_its_own_text() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/std");
    for (pseudo, text) in dz_project::STD_SOURCES {
        let rel = pseudo.strip_prefix("<std>/").unwrap();
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap().replace("\r\n", "\n");
        assert_eq!(
            text.replace("\r\n", "\n"),
            on_disk,
            "{pseudo} is listed with another file's text"
        );
    }
}
