//! Opt-in profiling of the real request planner; never invokes an HTTP model.
use crate::agent_error::AgentError;
use crate::analysis::{FrozenInput, agent};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;

struct OfflineJournal {
    first_request: Mutex<Option<Value>>,
    first_checkpoint: Mutex<Option<agent::Checkpoint>>,
}
#[async_trait]
impl agent::Journal for OfflineJournal {
    async fn load(&self) -> Result<Option<agent::Checkpoint>, AgentError> {
        Ok(None)
    }
    async fn save(&self, _: &agent::Checkpoint, _: &Value) -> Result<(), AgentError> {
        Ok(())
    }
    async fn reserve(
        &self,
        state: &agent::Checkpoint,
        body: &[u8],
    ) -> Result<Option<usize>, AgentError> {
        self.stop_before_transport(state, body);
        Err(AgentError::new(
            "PRIVATE_PROFILE_COMPLETE",
            "offline reservation stopped",
        ))
    }
    async fn reserve_discover_worker(
        &self,
        state: &agent::Checkpoint,
        _: &str,
        body: &[u8],
    ) -> Result<(), AgentError> {
        self.stop_before_transport(state, body);
        Err(AgentError::new(
            "PRIVATE_PROFILE_COMPLETE",
            "offline reservation stopped",
        ))
    }
}
impl OfflineJournal {
    fn stop_before_transport(&self, state: &agent::Checkpoint, body: &[u8]) {
        let mut captured = self.first_request.lock().unwrap();
        if captured.is_none() {
            *self.first_checkpoint.lock().unwrap() = Some(state.clone());
            *captured = Some(json!({
                "request_bytes":body.len(),
                "planned_packs":state.outline_run.reading_packs.as_ref().map(|work|work.packs.len()),
            }));
        }
    }
}
struct NoModel(AtomicUsize);
#[async_trait]
impl agent::Model for NoModel {
    async fn turn(
        &self,
        _: &agent::Config,
        _: &[u8],
    ) -> Result<knowledge::models::ChatTurn, AgentError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(AgentError::new(
            "PRIVATE_PROFILE_MODEL_FORBIDDEN",
            "network model is not installed",
        ))
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires private frozen input/limits/output paths; no HTTP model exists"]
async fn private_full_request_planner_profile_without_model() {
    let total_started = std::time::Instant::now();
    // Output must already name a private directory outside this checkout.
    let output = std::path::PathBuf::from(
        std::env::var("KB_PRIVATE_PROFILE_OUTPUT").expect("private output path required"),
    );
    let directory = output
        .parent()
        .expect("output directory required")
        .canonicalize()
        .expect("output directory unavailable");
    assert!(
        !directory.starts_with(std::env::current_dir().unwrap().canonicalize().unwrap()),
        "profile output must be outside checkout"
    );
    let stage_started = std::time::Instant::now();
    let input_bytes = std::fs::read(
        std::env::var("KB_PRIVATE_PROFILE_INPUT").expect("private input path required"),
    )
    .expect("private input unreadable");
    let input_read_micros = stage_started.elapsed().as_micros();
    let stage_started = std::time::Instant::now();
    let input: FrozenInput = serde_json::from_slice(&input_bytes).expect("invalid frozen input");
    let input_deserialize_micros = stage_started.elapsed().as_micros();
    let stage_started = std::time::Instant::now();
    let limits_bytes = std::fs::read(
        std::env::var("KB_PRIVATE_PROFILE_LIMITS").expect("private limits path required"),
    )
    .expect("private limits unreadable");
    let limits_read_micros = stage_started.elapsed().as_micros();
    let stage_started = std::time::Instant::now();
    let limits_json: Value = serde_json::from_slice(&limits_bytes).expect("invalid limits JSON");
    let limits: agent::Limits = serde_json::from_value(limits_json["extraction"].clone())
        .expect("invalid extraction limits");
    let limits_deserialize_micros = stage_started.elapsed().as_micros();
    assert_eq!(limits.max_context_tokens, 131_072);
    assert_eq!(limits.tokenizer.model_id, "gpt-6-luna");
    assert_eq!(
        limits
            .tokenizer
            .calibration
            .as_ref()
            .map(|c| c.multiplier_bps),
        Some(11_500)
    );
    assert_eq!(limits.token_safety_margin, 4096);
    let stage_started = std::time::Instant::now();
    // Private file contains only the provider contract and env credential reference.
    // No credential value is read or copied by this offline test.
    let provider_json: Value = serde_json::from_slice(
        &std::fs::read(
            std::env::var("KB_PRIVATE_PROFILE_PROVIDER").expect("provider contract path required"),
        )
        .expect("provider contract unreadable"),
    )
    .expect("provider contract invalid JSON");
    assert_eq!(provider_json["model_id"], "gpt-6-luna");
    assert_eq!(provider_json["output_token_reserve"], 16384);
    assert_eq!(
        provider_json["credential_ref"],
        "env:KNOWLEDGEBRAIN_CHAT_API_KEY"
    );
    let provider = serde_json::from_value(provider_json).expect("provider contract invalid");
    let config = agent::Config::with_provider_for(provider, limits, Some(&input))
        .expect("offline config invalid");
    let config_build_micros = stage_started.elapsed().as_micros();
    let journal = OfflineJournal {
        first_request: Mutex::new(None),
        first_checkpoint: Mutex::new(None),
    };
    let model = NoModel(AtomicUsize::new(0));
    agent::profile::reset();
    let started = std::time::Instant::now();
    let result = agent::run(&input, &config, &journal, &model, &CancellationToken::new()).await;
    let counters = agent::profile::finish_counters();
    let metrics = agent::profile::finish();
    let first_request_wall_micros = started.elapsed().as_micros();
    let planned = journal.first_checkpoint.lock().unwrap().take();
    agent::profile::reset();
    let projection_started = std::time::Instant::now();
    let all_packs = match planned.as_ref() {
        Some(state) => {
            agent::profile::all_pack_requests(&input, &config, state, &journal, &directory).await
        }
        None => Err(AgentError::new(
            "PRIVATE_PROFILE_NO_PLAN",
            "no planned checkpoint",
        )),
    };
    let projection_wall_micros = projection_started.elapsed().as_micros();
    let projection_counters = agent::profile::finish_counters();
    let projection_metrics = agent::profile::finish();
    let calls = model.0.load(Ordering::SeqCst);
    let summary = json!({
        "total_until_summary_micros":total_started.elapsed().as_micros(),
        "setup_micros": {
            "input_read":input_read_micros,"input_deserialize":input_deserialize_micros,
            "limits_read":limits_read_micros,"limits_deserialize":limits_deserialize_micros,
            "runtime_and_config_build":config_build_micros
        },
        "wall_micros":first_request_wall_micros,
        "all_pack_projection_wall_micros":projection_wall_micros,
        "all_pack_projection_inclusive_count_micros":projection_metrics,
        "all_pack_requests":all_packs.as_ref().ok(),
        "all_pack_projection_error_code":all_packs.as_ref().err().map(|error|error.code.as_str()),
        "inclusive_count_micros":metrics,
        "quantities":counters,
        "all_pack_projection_quantities":projection_counters,
        "input_digest":crate::analysis::digest(&input).expect("input digest"),
        "limits_digest":crate::analysis::digest(&limits_json).expect("limits digest"),
        "first_request":journal.first_request.lock().unwrap().clone(),
        "model_turn_calls":calls,"external_api_calls":0,
        "result_code":result.as_ref().err().map(|error|error.code.as_str()),
        "timing_note":"Inclusive spans overlap; do not sum. Current-thread full run through first worker reservation; wall_micros excludes setup; setup_micros records single reads/deserialization/config separately. total_until_summary includes setup, first-request run and subsequent all-pack projection, excluding final report serialization/write."
    });
    std::fs::write(&output, serde_json::to_vec_pretty(&summary).unwrap())
        .expect("profile output failed");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o600))
            .expect("private profile permissions failed");
    }
    eprintln!("PRIVATE_PLANNER_PROFILE {}", summary);
    assert!(
        all_packs.is_ok(),
        "all-pack offline projection failed; inspect output code"
    );
    assert_eq!(calls, 0, "NoModel must never be dispatched");
    assert!(
        journal.first_request.lock().unwrap().is_some(),
        "planner did not reach first reservation; inspect result_code"
    );
    assert_eq!(result.unwrap_err().code, "PRIVATE_PROFILE_COMPLETE");
}
