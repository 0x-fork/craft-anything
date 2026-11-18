//! Regression tests for case-insensitive path deduplication in `recover_modules`.

use craft_anything_js::Limits;
use craft_anything_js::model::BundleModule;
use craft_anything_js::recover::{RecoveredModule, RewriteLevel, recover_modules};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

/// Scoped temporary directory under `std::env::temp_dir()` cleaned up on drop.
struct TempDir(PathBuf);

impl TempDir {
    /// Create a fresh isolated directory for one test case.
    fn new(label: &str) -> Self {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("craft-js-recover-{}-{}-{label}", std::process::id(), id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp test directory");
        Self(dir)
    }

    /// Return the directory path.
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Normalize a path with per-character lowercasing matching case-insensitive filesystems.
fn fold_case(path: &str) -> String {
    path.chars().flat_map(char::to_lowercase).collect()
}

/// Build a synthetic `BundleModule` slice over `source` with the given id and sha256.
fn make_module(id: &str, sha256: &str, start: usize, end: usize) -> BundleModule {
    BundleModule {
        id: id.to_string(),
        format: "webpack".to_string(),
        chunk_ids: Vec::new(),
        start: start as u32,
        end: end as u32,
        body: Some((start as u32, end as u32)),
        line: 1,
        params: Vec::new(),
        deps: Vec::new(),
        sha256: sha256.to_string(),
        fingerprint: String::new(),
    }
}

/// Write every recovered module exclusively (`create_new(true)`) into `root`, mirroring `js_sources_restore`.
fn write_all_exclusively(root: &Path, recovered: &[RecoveredModule]) {
    for m in recovered {
        let mut path = root.to_path_buf();
        for comp in m.file_name.split('/') {
            path.push(comp);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent directory");
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap_or_else(|e| panic!("failed exclusive write for module {:?} at {}: {e}", m.id, m.file_name));
        f.write_all(m.text.as_bytes()).expect("write module text");
    }
}

/// Modules differing only in ASCII case receive distinct case-folded paths and write without `create_new` rejection.
#[test]
fn case_only_module_ids_keep_distinct_paths_and_write_exclusively() {
    let s0 = "globalThis.m0 = 10;\n";
    let s1 = "globalThis.m1 = 20;\n";
    let s2 = "globalThis.m2 = 30;\n";
    let s3 = "globalThis.m3 = 40;\n";
    let src = format!("{s0}{s1}{s2}{s3}");

    let o1 = s0.len();
    let o2 = o1 + s1.len();
    let o3 = o2 + s2.len();
    let o4 = o3 + s3.len();

    let modules = vec![
        make_module("X.CA978112.ts", "11111111aaaaaaaa", 0, o1),
        make_module("x.ca978112.ts", "22222222bbbbbbbb", o1, o2),
        make_module("src/App.js", "33333333cccccccc", o2, o3),
        make_module("src/app.js", "ca978112dddddddd", o3, o4),
    ];

    let recovered = recover_modules(&src, &modules, RewriteLevel::None, &Limits::default());
    assert_eq!(recovered.len(), 4);

    let dir = TempDir::new("ascii-case");
    write_all_exclusively(dir.path(), &recovered);

    assert_eq!(recovered[0].file_name, "X.CA978112.ts");
    assert_eq!(recovered[1].file_name, "x.ca978112.22222222.ts");
    assert_eq!(recovered[2].file_name, "src/App.js");
    assert_eq!(recovered[3].file_name, "src/app.ca978112.js");

    let folded: HashSet<String> = recovered.iter().map(|m| fold_case(&m.file_name)).collect();
    assert_eq!(folded.len(), 4, "case-folded paths must be unique: {:?}", recovered.iter().map(|m| &m.file_name).collect::<Vec<_>>());
}

/// Disambiguated `.hash` and `.hash.occurrence` names also check case-insensitive collisions against earlier and later modules.
#[test]
fn generated_hash_and_occurrence_suffixes_avoid_case_collisions() {
    let source = "globalThis.answer = 42;";
    let modules = vec![
        make_module("X.CA978112.js", "11111111aaaaaaaa", 0, source.len()),
        make_module("X.js", "22222222bbbbbbbb", 0, source.len()),
        make_module("x.js", "ca978112cccccccc", 0, source.len()),
        make_module("X.CA978112.2.js", "33333333dddddddd", 0, source.len()),
    ];

    let recovered = recover_modules(source, &modules, RewriteLevel::None, &Limits::default());
    assert_eq!(recovered.len(), 4);

    let dir = TempDir::new("suffix-case");
    write_all_exclusively(dir.path(), &recovered);

    assert_eq!(recovered[0].file_name, "X.CA978112.js");
    assert_eq!(recovered[1].file_name, "X.js");
    assert_eq!(recovered[2].file_name, "x.ca978112.2.js");
    assert_eq!(recovered[3].file_name, "X.CA978112.2.33333333.js");

    let folded: HashSet<String> = recovered.iter().map(|m| fold_case(&m.file_name)).collect();
    assert_eq!(folded.len(), 4);
}

/// Non-ASCII case pairs (including word-final Greek sigma) collide on NTFS/APFS and must be disambiguated.
#[test]
fn unicode_case_variants_including_final_sigma_get_distinct_paths() {
    let source = "globalThis.ok = 1;";
    let modules = vec![
        make_module("src/Über.js", "aaaa000011112222", 0, source.len()),
        make_module("src/über.js", "bbbb000011112222", 0, source.len()),
        make_module("src/ΟΣ.js", "cccc000011112222", 0, source.len()),
        make_module("src/οσ.js", "dddd000011112222", 0, source.len()),
    ];

    let recovered = recover_modules(source, &modules, RewriteLevel::None, &Limits::default());
    assert_eq!(recovered.len(), 4);

    let dir = TempDir::new("unicode-case");
    write_all_exclusively(dir.path(), &recovered);

    assert_eq!(recovered[0].file_name, "src/Über.js");
    assert_eq!(recovered[1].file_name, "src/über.bbbb0000.js");
    assert_eq!(recovered[2].file_name, "src/ΟΣ.js");
    assert_eq!(recovered[3].file_name, "src/οσ.dddd0000.js");

    let folded: HashSet<String> = recovered.iter().map(|m| fold_case(&m.file_name)).collect();
    assert_eq!(folded.len(), 4);
}

/// Non-colliding modules preserve their original mixed-case spelling untouched.
#[test]
fn distinct_modules_keep_original_casing_control() {
    let source = "globalThis.keep = 1;";
    let modules = vec![
        make_module("src/Alpha.js", "1111222233334444", 0, source.len()),
        make_module("src/Beta.ts", "5555666677778888", 0, source.len()),
        make_module("src/Gamma.mjs", "9999aaaabbbbcccc", 0, source.len()),
    ];

    let recovered = recover_modules(source, &modules, RewriteLevel::None, &Limits::default());
    let names: Vec<&str> = recovered.iter().map(|m| m.file_name.as_str()).collect();
    assert_eq!(names, vec!["src/Alpha.js", "src/Beta.ts", "src/Gamma.mjs"]);
}
