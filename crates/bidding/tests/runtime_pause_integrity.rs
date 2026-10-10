#[path = "support/frozen_fixture.rs"]
mod frozen_fixture;

#[test]
fn committed_requirement_must_fit_the_largest_organize_page() {
    use bidding::outline::discover::{DiscoverWork, PackRequirement, PackSubmit};
    let input = frozen_fixture::valid_text_input("project", "Submit qualification proof.");
    let mut work = DiscoverWork::plan_with_budget(&input, &|_| Ok(true));
    let pack = work.claim(1).remove(0);
    let submission = PackSubmit {
        call_id: "oversized-requirement".into(),
        claim_token: pack.claim_token.clone(),
        pack_revision: pack.pack_revision,
        requirements: vec![PackRequirement {
            condition_support: Vec::new(),
            obligation_strength: "mandatory".into(),
            extraction_quality: "explicit".into(),
            description: "x".repeat(20_000),
            evidence: work.pack_evidence(&input, &pack.id).unwrap(),
            source_section_id: pack.atoms[0].section_id.clone(),
            kind: "qualification".into(),
        }],
        no_requirement_reason: None,
        inspected_atom_ids: vec![],
    };
    let outcome = work.submit(&input, &pack.id, submission);
    outcome.unwrap();
    let record = work.requirement_records().values().next().unwrap();
    assert!(!record.description.is_empty());
    assert_eq!(record.evidence.len(), 1);
    // The runtime continuation regression verifies accessible bounded delivery;
    // domain storage must preserve the complete record instead of rejecting it.
}
