//! Private local transport simulation, never a semantic result or publication.
use super::*;
use std::sync::Mutex;

struct SimulationJournal {
    state: Mutex<Checkpoint>,
}
#[async_trait]
impl Journal for SimulationJournal {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(Some(self.state.lock().unwrap().clone()))
    }
    async fn reserve(&self, state: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
        *self.state.lock().unwrap() = state.clone();
        Ok(Some(1))
    }
    async fn save(&self, state: &Checkpoint, _: &Value) -> Result<(), AgentError> {
        *self.state.lock().unwrap() = state.clone();
        if state
            .outline_run
            .reading_packs
            .as_ref()
            .is_some_and(|work| work.complete())
        {
            return Err(error(
                "LOCAL_SIMULATION_COMPLETE",
                "local transport simulation stopped before Organize",
            ));
        }
        Ok(())
    }
    async fn publish_outline(
        &self,
        _: &crate::outline::OutlineArtifact,
        _: &[crate::outline::chapters::AttachmentBinding],
    ) -> Result<(), AgentError> {
        Err(error(
            "LOCAL_SIMULATION_PUBLICATION_FORBIDDEN",
            "simulation cannot publish",
        ))
    }
}

struct SimulationModel {
    requests: Mutex<Vec<Value>>,
    response_text: String,
}
#[async_trait]
impl Model for SimulationModel {
    async fn turn(&self, config: &Config, bytes: &[u8]) -> Result<ChatTurn, AgentError> {
        let body: Value = serde_json::from_slice(bytes).map_err(invalid)?;
        let accounting = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
            &body,
            &config.limits.tokenizer,
            config.limits.image_token_reserve,
            config.limits.token_safety_margin,
            config.provider.output_token_reserve as usize,
        )?;
        if accounting.total_context_tokens > config.limits.max_context_tokens {
            return Err(error(
                "LOCAL_SIMULATION_CONTEXT_OVERFLOW",
                "actual request exceeds token limit",
            ));
        }
        let messages = body["messages"]
            .as_array()
            .ok_or_else(|| invalid("request messages absent"))?;
        let brief = messages
            .iter()
            .filter_map(|message| message["content"].as_str())
            .filter_map(|text| serde_json::from_str::<Value>(text).ok())
            .find(|value| value["reading_packs"].is_array())
            .ok_or_else(|| invalid("reading brief absent"))?;
        let packs = brief["reading_packs"].as_array().unwrap();
        if packs.is_empty() {
            return Err(error(
                "LOCAL_SIMULATION_ORGANIZE_REACHED",
                "simulation must stop before Organize",
            ));
        }
        let mut requests = self.requests.lock().unwrap();
        let turn = requests.len();
        if turn > 1000 {
            return Err(error(
                "LOCAL_SIMULATION_STALLED",
                "simulation exceeded bounded progress checks",
            ));
        }
        let calls=packs.iter().map(|session| {
            let pack=&session["pack"];
            let id=pack["id"].as_str().unwrap();
            knowledge::models::ChatToolCall {
                id:format!("simulation-{turn}-{id}"), name:"submit_pack".into(),
                arguments:json!({"wire_scope":serde_json::from_str::<Value>(messages.last().unwrap()["content"].as_str().unwrap()).unwrap()["wire_scope"],"pack_id":id,"call_id":session["submission_operation_id"],"claim_token":pack["claim_token"],"pack_revision":pack["pack_revision"],"repair":false,"requirements":[],"no_requirement_reason":"LOCAL TRANSPORT SIMULATION ONLY. No semantic extraction was performed; this is not a business finding.","inspected_atom_ids":pack["atoms"].as_array().unwrap().iter().filter(|atom|atom["context_only"]!=true).map(|atom|json!({"atom_key":atom["atom_key"]})).collect::<Vec<_>>()} ).to_string(),
            }
        }).collect::<Vec<_>>();
        let simulated_reply_tokens = config.limits.tokenizer.count_text_tokens(
            &serde_json::to_string(&json!({"content":self.response_text,"tool_calls":calls}))
                .map_err(invalid)?,
        )?;
        requests.push(json!({"request":turn+1,"pack_ids":packs.iter().map(|session|session["pack"]["id"].clone()).collect::<Vec<_>>(),"serialized_bytes":bytes.len(),"history_messages":messages.len(),"token_accounting":accounting,"simulated_reply_tokens":simulated_reply_tokens}));
        if simulated_reply_tokens > config.provider.output_token_reserve as usize {
            return Err(error(
                "LOCAL_SIMULATION_REPLY_EXCEEDS_OUTPUT_BUDGET",
                "mock submission exceeds configured output reservation",
            ));
        }
        Ok(ChatTurn {
            content: self.response_text.clone(),
            tool_calls: calls,
            finish_reason: "tool_calls".into(),
            ..Default::default()
        })
    }
}

fn private_input() -> FrozenInput {
    use crate::outline::frozen::{
        FrozenBuildInput, PARSER_CONTRACT_VERSION, ParsedDocument, build_frozen_input,
    };
    use sha2::{Digest, Sha256};
    let path = std::env::var("KB_FROZEN_VALIDATION_RECORDS")
        .expect("private parser records path required");
    let bytes = std::fs::read(path).unwrap_or_else(|_| panic!("private input read failed"));
    let rows: Vec<Value> =
        serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("invalid private input JSON"));
    let documents = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            assert!(
                row["images"].as_array().unwrap().is_empty(),
                "this local simulation has no OCR or image authorization"
            );
            let parsed = docparser::ReadResult {
                markdown: row["markdown"].as_str().unwrap().into(),
                metadata: serde_json::from_value(row["metadata"].clone())
                    .unwrap_or_else(|_| panic!("invalid private metadata")),
                structured_source_units: serde_json::from_value(
                    row["structured_source_units"].clone(),
                )
                .unwrap_or_else(|_| panic!("invalid private units")),
                ..Default::default()
            };
            let contract = docparser::parse_source_contract(&parsed)
                .unwrap_or_else(|_| panic!("invalid private contract"))
                .unwrap();
            let source = hex::decode(row["source_hex"].as_str().unwrap())
                .unwrap_or_else(|_| panic!("invalid private source bytes"));
            assert!(contract.document_revision == hex::encode(Sha256::digest(source)));
            ParsedDocument {
                document_id: format!("document-{index}"),
                parsed,
            }
        })
        .collect();
    build_frozen_input(
        FrozenBuildInput {
            project_id: "local-transport-simulation".into(),
            document_set_id: "local-transport-simulation".into(),
            documents,
            document_relations: vec![],
            decisions: vec![],
        },
        vec![],
        PARSER_CONTRACT_VERSION,
    )
    .unwrap_or_else(|_| panic!("private freeze failed"))
}

#[tokio::test]
#[ignore = "requires private local records; local mock responses are not semantic findings"]
async fn all_discover_requests_fit_with_real_evolving_history_without_external_calls() {
    let input = private_input();
    let reference = crate::analysis::tests::config();
    let mut provider = reference.provider;
    provider.model_id = "gpt-4.1".into();
    let mut limits = reference.limits;
    limits.max_context_tokens = 131_072;

    limits.token_safety_margin = 4096;
    limits.image_token_reserve = 16_384;
    limits.tokenizer = crate::agent_runtime::TokenizerProfile {
        model_id: provider.model_id.clone(),
        encoding: crate::agent_runtime::TokenEncoding::O200kBase,
        calibration: None,
    };
    let config = Config::with_provider(provider, limits).unwrap();
    let mut state = super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    let journal = SimulationJournal {
        state: Mutex::new(state),
    };
    let model = SimulationModel {
        requests: Mutex::new(Vec::new()),
        response_text: String::new(),
    };
    let started = Instant::now();
    let outcome = run(&input, &config, &journal, &model, &CancellationToken::new()).await;
    let state = journal.state.lock().unwrap();
    let requests = model.requests.lock().unwrap();
    let counts = state
        .outline_run
        .reading_packs
        .as_ref()
        .map(|work| work.pack_counts());
    let code = outcome
        .as_ref()
        .err()
        .map(|error| error.code.as_str())
        .unwrap_or("UNEXPECTED_SUCCESS");
    let complete = code == "LOCAL_SIMULATION_COMPLETE"
        && state
            .outline_run
            .reading_packs
            .as_ref()
            .is_some_and(|work| work.complete());
    let summary = json!({"simulation_only":true,"semantic_findings":false,"external_model_calls":0,"published":false,"stopped_before_organize":complete,"reference_model":"gpt-4.1","tokenizer":"o200k_base","is_user_deployed_model":false,"max_context_tokens":131072,"result_code":code,"total_packs":counts.map(|counts|counts.total),"committed_simulation_packs":counts.map(|counts|counts.committed),"request_count":requests.len(),"requests":*requests,"elapsed_seconds":started.elapsed().as_secs_f64()});
    let path = std::env::var("KB_FROZEN_TRANSPORT_SUMMARY")
        .expect("private simulation summary path required");
    std::fs::write(path, serde_json::to_vec_pretty(&summary).unwrap())
        .unwrap_or_else(|_| panic!("private summary write failed"));
    println!("{summary}");
    assert!(complete, "local transport simulation stopped with {code}");
}

fn sizeable_synthetic_input() -> FrozenInput {
    synthetic_input(1500, 2)
}
pub(super) fn synthetic_input(repeats: usize, documents_count: usize) -> FrozenInput {
    use crate::outline::frozen::{FrozenBuildInput, ParsedDocument, build_frozen_input};
    use sha2::{Digest, Sha256};
    let text = "Synthetic source sentence for transport admission testing.\n".repeat(repeats);
    let locator = docparser::StructuredSourceLocator::Document {
        section_ordinal: 0,
        table_ordinal: None,
        row_ordinal: None,
        form_ordinal: None,
        heading_path: "Synthetic section".into(),
    };
    let hash = hex::encode(Sha256::digest(text.as_bytes()));
    let contract = json!({"schema_version":2,"document_revision":hash,
        "parser_version":"synthetic-test-v2","markdown_sha256":hash,"page_manifest":[],
        "units":[{"unit_id":"section","ordinal":0,"kind":"section","text_sha256":hash,
        "grid_sha256":null,"section_id":"section","parent_section_id":null,"heading_level":1,
        "heading_path":"Synthetic section","physical_locator":locator,"physical_path":"body/p:0",
        "physical_locator_unavailable_reason":null,"rendered_spans":[{"start_byte":0,"end_byte":text.len()}],
        "completeness":"complete","reasons":[],"table_id":null,"header_cells":[]}]});
    let documents = (0..documents_count)
        .map(|index| ParsedDocument {
            document_id: format!("synthetic-{index}"),
            parsed: docparser::ReadResult {
                markdown: text.clone(),
                metadata: std::collections::HashMap::from([(
                    "source_contract".into(),
                    contract.to_string(),
                )]),
                structured_source_units: vec![docparser::StructuredSourceUnit {
                    key: "section".into(),
                    ordinal: 0,
                    kind: docparser::StructuredSourceUnitKind::Section,
                    text: text.clone(),
                    locator: locator.clone(),
                    grid: None,
                }],
                ..Default::default()
            },
        })
        .collect();
    build_frozen_input(
        FrozenBuildInput {
            project_id: "synthetic-transport".into(),
            document_set_id: "synthetic-transport".into(),
            documents,
            document_relations: vec![],
            decisions: vec![],
        },
        vec![],
        crate::outline::frozen::PARSER_CONTRACT_VERSION,
    )
    .unwrap()
}

#[tokio::test]
async fn near_window_submit_retires_sources_before_fitting_its_response_history() {
    let input = sizeable_synthetic_input();
    let reference = crate::analysis::tests::config();
    let mut provider = reference.provider;
    provider.model_id = "gpt-4.1".into();
    let mut limits = reference.limits;
    limits.max_context_tokens = 131_072;

    limits.token_safety_margin = 4096;
    limits.tokenizer = crate::agent_runtime::TokenizerProfile {
        model_id: provider.model_id.clone(),
        encoding: crate::agent_runtime::TokenEncoding::O200kBase,
        calibration: None,
    };
    let config = Config::with_provider(provider.clone(), limits.clone()).unwrap();
    let mut probe = super::retirement::checkpoint(&input);
    probe.turn = 0;
    let body = request(&input, &config, &mut probe).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let used = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
        &body,
        &limits.tokenizer,
        limits.image_token_reserve,
        limits.token_safety_margin,
        provider.output_token_reserve as usize,
    )
    .unwrap()
    .total_context_tokens;
    // The source request fits, but its sizeable valid response cannot coexist
    // with that retired source in the next window. No test-side eviction occurs.
    limits.max_context_tokens = used + 64;
    let config = Config::with_provider(provider, limits).unwrap();
    let mut state = super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    let journal = SimulationJournal {
        state: Mutex::new(state),
    };
    let model = SimulationModel {
        requests: Mutex::new(Vec::new()),
        response_text: "TRANSPORT SIMULATION; not a semantic finding.\n".repeat(250),
    };
    let outcome = run(&input, &config, &journal, &model, &CancellationToken::new()).await;
    let error = outcome.unwrap_err();
    assert_eq!(
        error.code,
        "LOCAL_SIMULATION_COMPLETE",
        "{}; requests={:?}; packs={:?}",
        error.message,
        model.requests.lock().unwrap(),
        journal
            .state
            .lock()
            .unwrap()
            .outline_run
            .reading_packs
            .as_ref()
            .map(|work| work.pack_counts())
    );
    let saved = journal.state.lock().unwrap();
    assert!(saved.outline_run.reading_packs.as_ref().unwrap().complete());
    let requests = model.requests.lock().unwrap();
    // The lifecycle reservation admits one document at a time. The second
    // genuine request must progress using the first completed response history.
    assert!(
        requests.len() >= 2,
        "must advance through multiple real request envelopes"
    );
    assert!(requests.iter().all(|request| {
        request["token_accounting"]["total_context_tokens"]
            .as_u64()
            .unwrap()
            <= config.limits.max_context_tokens as u64
    }));
    assert!(requests[0]["simulated_reply_tokens"].as_u64().unwrap() > 64);
}
