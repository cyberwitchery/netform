use std::fs;
use std::path::Path;

use netform_diff::{NormalizeOptions, diff_documents};
use netform_ir::parse_generic;

#[test]
fn diff_is_deterministic_for_embedded_corpus_pairs() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let testdata = repo_root.join("netform_ir").join("testdata");

    let mut samples = Vec::new();
    for entry in fs::read_dir(&testdata).expect("read testdata") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.is_file() {
            samples.push(fs::read_to_string(path).expect("read sample"));
        }
    }

    for (i, a_text) in samples.iter().enumerate() {
        for (j, b_text) in samples.iter().enumerate() {
            let a = parse_generic(a_text);
            let b = parse_generic(b_text);

            let one = diff_documents(&a, &b, NormalizeOptions::default());
            let two = diff_documents(&a, &b, NormalizeOptions::default());

            let one_json = serde_json::to_string_pretty(&one).expect("serialize first");
            let two_json = serde_json::to_string_pretty(&two).expect("serialize second");

            assert_eq!(
                one_json, two_json,
                "flapping output for corpus pair ({i}, {j})"
            );
        }
    }
}
