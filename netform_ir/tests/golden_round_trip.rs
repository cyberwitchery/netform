use std::fs;
use std::path::Path;

use netform_ir::parse_generic;

#[test]
fn round_trip_all_testdata_files() {
    let dir = Path::new("testdata");
    for entry in fs::read_dir(dir).expect("read testdata") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_file() {
            let input = fs::read_to_string(&path).expect("read sample");
            let doc = parse_generic(&input);
            assert_eq!(
                doc.render(),
                input,
                "round-trip mismatch for {}",
                path.display()
            );
        }
    }
}
