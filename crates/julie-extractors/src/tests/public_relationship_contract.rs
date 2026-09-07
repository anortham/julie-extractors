use julie_extractors::{NormalizedSpan, PendingSpan, UnresolvedTarget};

#[test]
fn relationship_components_are_nameable_by_external_consumers() {
    let target = UnresolvedTarget::simple("worker.run");
    let span: PendingSpan = NormalizedSpan {
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 10,
        start_byte: 0,
        end_byte: 10,
    };
    assert_eq!(target.display_name, "worker.run");
    assert_eq!(span.end_byte, 10);
}
