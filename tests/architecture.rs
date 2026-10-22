#![cfg(not(miri))]
#![allow(missing_docs)]

use std::{fs, path::Path};

#[test]
fn core_ui_and_dsl_keep_dependencies_within_their_layer() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let imports = regex::Regex::new(r"\bcrate::([a-z_]+)").unwrap();
    // `row` is the core-owned macro exported at crate root; `tests` is test glue.
    for (module, allowed) in [
        ("core", "core row"),
        ("ui", "ui core config normalize command_words row tests"),
        ("dsl", "dsl core ui row"),
    ] {
        let mut paths = vec![src.join(module)];
        while let Some(path) = paths.pop() {
            if path.is_dir() {
                paths.extend(
                    fs::read_dir(&path)
                        .unwrap()
                        .map(|entry| entry.unwrap().path())
                        .filter(|path| path.file_name().unwrap() != "tests"),
                );
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") || path.ends_with("tests.rs") {
                continue;
            }
            let source = fs::read_to_string(&path).unwrap();
            for line in source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
            {
                for import in imports.captures_iter(line) {
                    let dependency = &import[1];
                    // Path prompts reuse the REPL's file-completing editor.
                    let exception = path == src.join("ui/prompt.rs") && dependency == "repl";
                    assert!(
                        allowed.split_whitespace().any(|name| name == dependency) || exception,
                        "{}: {module} must not depend on {dependency}",
                        path.display()
                    );
                }
            }
        }
    }
}
