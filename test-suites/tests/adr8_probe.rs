#[test]
fn exact_generated_prefix() {
    test_suites::adr8_properties::replay_generated(&[
        (14, 3, 2, 4, 4),
        (0, 1, 0, 3, 6),
        (15, 0, 1, 4, 1),
    ]);
}
#[test]
fn final_fill_lawful_rejection_precedence() {
    // Self-fill 1211 precedes any auction/maturity/emissions reasoning;
    // a spurious BadRequest or accepted fill fails here.
    test_suites::adr8_properties::replay_generated(&[(15, 1, 1, 4, 1)]);
}
