use super::support::{extract, names};

const ROW_COUNT: usize = 4000;

fn dense_document() -> String {
    let mut document = String::from("<catalog name=\"parts\">\n  <rows>\n");
    for index in 0..ROW_COUNT {
        document.push_str(&format!("    <row><cell>{index}</cell></row>\n"));
    }
    document.push_str("  </rows>\n  <part name=\"bolt\"/>\n</catalog>\n");
    document
}

#[test]
fn thousands_of_anonymous_elements_yield_only_the_named_handful() {
    let document = dense_document();
    assert!(document.len() < 1_000_000, "fixture must stay under 1MB");

    let symbols = extract(&document);

    assert_eq!(names(&symbols), vec!["parts", "bolt"]);
}
