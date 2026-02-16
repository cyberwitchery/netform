use netform_diff::{NormalizeOptions, diff_documents};
use netform_ir::parse_generic;
use proptest::prelude::*;

fn text_strategy() -> impl Strategy<Value = String> {
    let line = prop::string::string_regex("[ -~]{0,40}").expect("valid regex");
    prop::collection::vec(line, 0..40).prop_map(|lines| {
        if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n")
        }
    })
}

proptest! {
    #[test]
    fn diff_is_deterministic(a in text_strategy(), b in text_strategy()) {
        let doc_a = parse_generic(&a);
        let doc_b = parse_generic(&b);

        let one = diff_documents(&doc_a, &doc_b, NormalizeOptions::default());
        let two = diff_documents(&doc_a, &doc_b, NormalizeOptions::default());

        prop_assert_eq!(one, two);
    }

    #[test]
    fn roundtrip_survives_random_inputs(input in text_strategy()) {
        let doc = parse_generic(&input);
        prop_assert_eq!(doc.render(), input);
    }
}
