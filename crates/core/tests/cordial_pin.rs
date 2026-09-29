//! The Nix build and the Flatpak build must run the same Cordial. The Nix
//! side reads cordial/source.json; the Flatpak manifest cannot, so this
//! checks that its cordial module names the same repository, commit and
//! patches.

use std::fs;
use std::path::PathBuf;

fn repo_file(path: &str) -> String {
    let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(path);
    fs::read_to_string(&full).unwrap_or_else(|e| panic!("{}: {e}", full.display()))
}

/// The manifest's `- name: cordial` module, up to the next module; empty
/// when there is none.
fn cordial_module(manifest: &str) -> Vec<&str> {
    let mut lines = manifest.lines().skip_while(|l| l.trim() != "- name: cordial");
    let Some(first) = lines.next() else { return Vec::new() };
    std::iter::once(first).chain(lines.take_while(|l| !l.starts_with("  - name:"))).collect()
}

/// The value of the first `key:` line at or after `from`, and its index.
fn value_after<'a>(lines: &[&'a str], from: usize, key: &str) -> Option<(usize, &'a str)> {
    lines.iter().enumerate().skip(from).find_map(|(i, l)| {
        let rest = l.trim().trim_start_matches("- ").strip_prefix(key)?.strip_prefix(':')?;
        Some((i, rest.trim()))
    })
}

#[test]
fn the_flatpak_builds_the_cordial_that_source_json_pins() {
    let source: serde_json::Value =
        serde_json::from_str(&repo_file("cordial/source.json")).expect("source.json parses");
    let field = |k: &str| source[k].as_str().unwrap_or_else(|| panic!("source.json lacks {k}"));
    let (owner, repo, rev) = (field("owner"), field("repo"), field("rev"));
    let patches: Vec<String> = source["patches"]
        .as_array()
        .expect("source.json lists its patches")
        .iter()
        .map(|p| format!("../../cordial/{}", p.as_str().expect("a patch is a file name")))
        .collect();

    let manifest = repo_file("packaging/flatpak/io.github.mujo.RobloxManager.yml");
    let module = cordial_module(&manifest);
    assert!(!module.is_empty(), "the manifest has a cordial module");

    let (at, url) = value_after(&module, 0, "url").expect("the cordial module has a url");
    assert_eq!(url, format!("https://github.com/{owner}/{repo}"), "the Flatpak's Cordial source");
    let (_, commit) = value_after(&module, at, "commit").expect("the source pins a commit");
    assert_eq!(commit, rev, "the Flatpak's Cordial commit");

    let (_, sha) = value_after(&module, 0, "CORDIAL_GIT_SHA").expect("CORDIAL_GIT_SHA is set");
    assert_eq!(sha, format!("{}-mujo", &rev[..7]), "the Flatpak's CORDIAL_GIT_SHA");

    let applied: Vec<&str> = module
        .iter()
        .filter_map(|l| l.trim().strip_prefix("path: ../../cordial/").map(|_| l.trim()))
        .map(|l| l.trim_start_matches("path: "))
        .collect();
    assert_eq!(applied, patches, "the Flatpak's Cordial patches, in order");
}
