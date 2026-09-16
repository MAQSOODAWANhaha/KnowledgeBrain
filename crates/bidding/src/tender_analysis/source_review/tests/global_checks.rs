use super::*;

#[test]
fn put_analysis_check_stores_role_local_pass() {
    let (input, config, mut state) = fixture();
    state.role = Role::Reviewer;
    let err = apply(
        &input,
        &config,
        &mut state,
        "put_analysis_check",
        &json!({"key":"nope","conclusion":"pass","grounds":[]}),
    )
    .unwrap_err();
    assert!(err.contains("unknown global check"), "{err}");
    apply(
        &input,
        &config,
        &mut state,
        "put_analysis_check",
        &json!({"key":"source_coverage","conclusion":"pass","grounds":[citation(&input)]}),
    )
    .unwrap();
    let check = &state.analysis.review_global_checks["source_coverage"];
    assert_eq!(check.conclusion, GlobalCheckConclusion::Pass);
    assert_eq!(
        check.scope_sha256,
        crate::tender_analysis::rule_contract::scope_sha256(&input, &state.analysis).unwrap()
    );
    assert!(!state.analysis.main_global_checks.contains_key("source_coverage"));
}
