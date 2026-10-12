//! S1 structural-context regression, not a model semantic extraction assertion.
//! The fixture is a real synthetic DOCX parsed by the production Python parser.
use bidding::{
    analysis::DocumentRole,
    outline::{
        claim_review,
        discover::{DiscoverWork, PackCarrier, RequirementRecord},
        evidence::{self, EvidenceRef},
        frozen::{self, FrozenBuildInput, PARSER_CONTRACT_VERSION, ParsedDocument},
    },
};
use prost::Message;

const INTRO: &str = "Only if the optional maintenance service is selected, every row in the following schedule requires a signed response; otherwise these rows are not applicable.";

#[test]
fn synthetic_docx_preserves_condition_navigation_and_check_after_table_split() {
    let wire = include_bytes!("fixtures/synthetic-s1-docx-wire.pb");
    let parsed = docparser::ReadResult::try_from(
        docparser::proto::ReadResponse::decode(wire.as_slice()).unwrap(),
    )
    .unwrap();
    let input = frozen::build_frozen_input(
        FrozenBuildInput {
            project_id: "synthetic-s1-project".into(),
            document_set_id: "synthetic-s1-set".into(),
            documents: vec![ParsedDocument {
                role: DocumentRole::Primary,
                document_id: "synthetic-s1-docx".into(),
                parsed,
            }],
            document_relations: vec![],
            decisions: vec![],
        },
        vec![],
        PARSER_CONTRACT_VERSION,
    )
    .unwrap();
    frozen::validate_frozen_input_contract(&input).unwrap();
    let intro = input
        .source_units
        .iter()
        .find(|s| s.text.contains(INTRO))
        .unwrap();
    let form = &input.structured_forms[0];
    let table = form["form_definition_revision_id"].as_str().unwrap();
    let owner = input
        .source_units
        .iter()
        .find(|s| form["source_unit_revision_id"] == s.source_unit_revision_id)
        .unwrap();
    assert_ne!(intro.source_unit_revision_id, owner.source_unit_revision_id);
    assert_eq!(form["definition"]["row_count"], 65);
    assert!(owner.text.is_empty());
    assert!(
        owner.locator["heading_path"]
            .as_str()
            .unwrap()
            .contains("Conditional scope")
    );

    // Synthetic full-session size predicate forces a large table across packs.
    // This is a deterministic planner seam, not a provider/tokenizer measurement.
    let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(serde_json::to_vec(sessions).unwrap().len() <= 20_000)
    });
    assert!(
        work.planning_error().is_none(),
        "{:?}",
        work.planning_error()
    );
    let packs = work.claim(256);
    assert!(packs.len() > 1);
    let table_packs = packs
        .iter()
        .filter(|pack| {
            pack.atoms.iter().any(|atom| {
        matches!(&atom.carrier, PackCarrier::Grid { table_id, .. } if table_id == table)
    })
        })
        .count();
    assert!(
        table_packs > 1,
        "fixture must exercise an actual table split"
    );
    let target = packs.iter().find(|pack| pack.atoms.iter().any(|atom| {
        matches!(&atom.carrier, PackCarrier::Grid { table_id, cells, .. }
            if table_id == table && cells.iter().any(|cell| cell.anchor_row == 64 && cell.anchor_column == 1))
    })).unwrap();
    assert!(
        target
            .atoms
            .iter()
            .all(|atom| atom.source_unit_revision_id != intro.source_unit_revision_id)
    );
    let session = work.session(&input, &target.id).unwrap();
    assert_eq!(
        session["pack"]["condition_support_options"],
        serde_json::json!([])
    );
    assert_eq!(
        session["pack"]["read_dependencies"][0]["type"],
        "structural_context_candidate"
    );
    assert!(
        session["pack"]["read_dependencies"][0]["read"]["args"]["dependency_id"]
            .as_str()
            .unwrap()
            .starts_with("context_")
    );
    assert!(
        !session.to_string().contains(INTRO),
        "directory is bounded navigation, not original-body delivery"
    );

    let cell = form["definition"]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .find(|cell| cell["row"] == 64 && cell["column"] == 1)
        .unwrap();
    let digest = evidence::input_digest(&input).unwrap();
    let record = RequirementRecord {
        condition_support: vec![],
        obligation_strength: "mandatory".into(),
        extraction_quality: "explicit".into(),
        description: "Submit signed response for synthetic item 064.".into(),
        evidence: vec![EvidenceRef::GridCell {
            input_digest: digest.clone(),
            table_id: table.into(),
            anchor_row: 64,
            anchor_column: 1,
            start_byte: 0,
            end_byte: cell["text"].as_str().unwrap().len(),
        }],
        source_section_id: target.atoms[0].section_id.clone(),
        kind: "material".into(),
        dedup_group: "synthetic-row-64".into(),
    };
    let review = claim_review::build(&input, "synthetic-requirement", &record).unwrap();
    assert!(
        review
            .evidence_units
            .iter()
            .flat_map(|u| &u.excerpts)
            .any(|excerpt| {
                matches!(
                    &excerpt.evidence,
                    EvidenceRef::GridCell {
                        anchor_row: 64,
                        anchor_column: 0,
                        ..
                    }
                )
            }),
        "Check expands the cited cell to its full native row"
    );
    assert!(
        review
            .evidence_units
            .iter()
            .any(|unit| unit.role == "related_section_context"
                && unit
                    .excerpts
                    .iter()
                    .any(|excerpt| excerpt.quote.contains(INTRO))),
        "Check sees the independent condition as a candidate, not established applicability"
    );
    let mut with_condition = record;
    with_condition.evidence.push(EvidenceRef::Text {
        input_digest: digest,
        unit_id: intro.source_unit_revision_id.clone(),
        start_byte: 0,
        end_byte: intro.text.len(),
    });
    let explicit = claim_review::build(&input, "synthetic-requirement", &with_condition).unwrap();
    assert!(
        serde_json::to_string(&explicit).unwrap().contains(INTRO),
        "positive control: the frozen condition exists and is recoverable when explicitly referenced"
    );
}
