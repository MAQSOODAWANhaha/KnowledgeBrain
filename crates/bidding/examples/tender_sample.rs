//! Local real-provider acceptance runner. No database/publication adapter.
//! Inputs are frozen outputs of the shared Python service; no uploaded-file parsing.
use async_trait::async_trait;
use bidding::{
    agent_error::AgentError,
    analysis::{self as ta, agent as extraction},
    authoring_runtime::AuthoringRuntimeContractV1,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

fn err(e: impl std::fmt::Display) -> AgentError {
    AgentError::new("INTERNAL", e.to_string())
}
fn read<T: DeserializeOwned>(p: impl AsRef<Path>) -> Result<T, AgentError> {
    serde_json::from_slice(&fs::read(p).map_err(err)?).map_err(err)
}
fn save(p: impl AsRef<Path>, value: &impl Serialize) -> Result<(), AgentError> {
    let p = p.as_ref();
    let tmp = p.with_extension("tmp");
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)
        .map_err(err)?;
    f.write_all(&serde_json::to_vec(value).map_err(err)?)
        .map_err(err)?;
    f.sync_all().map_err(err)?;
    fs::rename(tmp, p).map_err(err)?;
    fs::File::open(p.parent().ok_or_else(|| err("output parent"))?)
        .map_err(err)?
        .sync_all()
        .map_err(err)
}
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Calls {
    total: usize,
    attempts: BTreeMap<String, usize>,
}
// Sole cumulative spending guard, private acceptance only. Production limits
// do not contain these quotas. Include the pending request in every admission.
fn acceptance_admission(
    a: &bidding::agent_runtime::budget::ModelAccounting,
    now: u64,
) -> Result<(), AgentError> {
    if a.physical_calls > 71
        || a.reserved_input_tokens > 7_166_928
        || a.reserved_output_tokens > 581_632
        || a.started_unix_seconds
            .is_none_or(|start| now.saturating_sub(start) >= 5438)
    {
        return Err(AgentError::new(
            "PRIVATE_ACCEPTANCE_BUDGET_EXHAUSTED",
            "private acceptance spending guard stopped before send",
        ));
    }
    Ok(())
}

struct Local {
    root: PathBuf,
    views: PathBuf,
    input: ta::FrozenInput,
    count: Mutex<Calls>,
}
impl Local {
    fn reserve(&self, turn: usize, role: &str, body: &[u8]) -> Result<(usize, usize), AgentError> {
        let mut count = self.count.lock().map_err(err)?;
        let boundary = format!("turn-{turn}-{role}");
        let attempt = count.attempts.get(&boundary).copied().unwrap_or(0);
        if attempt >= 3 {
            return Err(AgentError::new(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "same-boundary transport attempts exhausted",
            ));
        }
        let identity = self.root.join(format!("{boundary}.json"));
        if identity.exists() {
            if fs::read(&identity).map_err(err)? != body {
                return Err(err("same-turn request changed"));
            }
        } else {
            let mut f = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&identity)
                .map_err(err)?;
            f.write_all(body).map_err(err)?;
            f.sync_all().map_err(err)?;
        }
        count.total += 1;
        count.attempts.insert(boundary, attempt + 1);
        save(self.root.join("calls.json"), &*count)?;
        eprintln!(
            "{}",
            json!({"event":"provider_reserved","role":role,"turn":turn,"physical_calls":count.total,"attempt":attempt + 1})
        );
        Ok((count.total, attempt + 1))
    }
}
#[async_trait]
impl extraction::Journal for Local {
    async fn load(&self) -> Result<Option<extraction::Checkpoint>, AgentError> {
        let p = self.root.join("checkpoint.json");
        if p.exists() {
            Ok(Some(read(p)?))
        } else {
            Ok(None)
        }
    }
    async fn admit(&self, state: &extraction::Checkpoint, _body: &[u8]) -> Result<(), AgentError> {
        acceptance_admission(
            &state.journal.accounting,
            bidding::agent_runtime::budget::unix_seconds(),
        )
    }
    async fn reserve(
        &self,
        state: &extraction::Checkpoint,
        body: &[u8],
    ) -> Result<Option<usize>, AgentError> {
        let (_, attempt) = self.reserve(
            state.turn,
            if state.role == extraction::Role::Main {
                "main"
            } else {
                "reviewer"
            },
            body,
        )?;
        save(self.root.join("checkpoint.json"), state)?;
        Ok(Some(attempt))
    }
    async fn save(&self, s: &extraction::Checkpoint, p: &Value) -> Result<(), AgentError> {
        save(self.root.join("checkpoint.json"), s)?;
        save(self.root.join("progress.json"), p)?;
        eprintln!("{p}");
        Ok(())
    }
    async fn source_view(
        &self,
        id: &str,
        limits: &extraction::Limits,
        cancel: &CancellationToken,
    ) -> Result<ta::views::SourceView, AgentError> {
        if cancel.is_cancelled() {
            return Err(err("canceled"));
        }
        let source = self
            .input
            .source_units
            .iter()
            .find(|s| s.source_unit_revision_id == id)
            .ok_or_else(|| err("unknown source"))?;
        let page = source.locator["page_ordinal"]
            .as_u64()
            .ok_or_else(|| err("source has no physical page"))?;
        let document = self
            .input
            .documents
            .iter()
            .find(|d| d.document_id == source.document_id)
            .ok_or_else(|| err("source document missing"))?;
        let revision = document.document_revision.as_str();
        if revision.len() != 64 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(err("source document has no strict revision digest"));
        }
        let mut view: ta::views::SourceView =
            read(self.views.join(revision).join(format!("page-{page}.json")))?;
        if view.identity.page_ordinal as u64 != page || revision != view.identity.original_sha256 {
            return Err(err(
                "source view does not belong to the frozen document/page",
            ));
        }
        view.identity.source_id = id.into();
        view.validate(
            id,
            limits.max_source_view_edge,
            limits.max_source_view_bytes,
        )
        .map_err(err)?;
        Ok(view)
    }
}
struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
// Seed import creates fresh execution ledgers. It must not erase spent repair
// allowances or bypass an unfinished response from an earlier attempt.

// Import only archived model claims validated by the same production gates.
// This does not carry the old review's reading or completion receipts into a
// new contract. The immutable old checkpoint remains the audit of that attempt.

// Preserve later candidate edits separately from the archived feedback evidence.
// Neither input imports an independent approval into the new runtime contract.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    platform::init_tracing();
    let args: Vec<_> = std::env::args().collect();
    // Offline diagnostics use exactly the production validators. No environment
    // provider resolution, model call, checkpoint repair or uploaded-file parser.
    if args.get(1).map(String::as_str) == Some("audit") {
        if args.len() != 4 {
            return Err("usage: tender_sample audit <input-directory> <new-report.json>".into());
        }
        let input_dir = PathBuf::from(&args[2]);
        let output = PathBuf::from(&args[3]);
        if output.exists() {
            return Err(
                "audit report must be a new file; frozen evidence is never overwritten".into(),
            );
        }
        let input: ta::FrozenInput = read(input_dir.join("frozen-input.json"))?;
        let result: ta::AnalysisResult = read(input_dir.join("analysis-result.json"))?;
        let input_error = ta::tools::validate_input(&input).err();
        let gaps = ta::tools::gaps(&input, &result.analysis);
        let review_gaps = ta::tools::review_gaps(&input, &result.analysis, &result.review.coverage);
        let accepted = input_error.is_none() && gaps.is_empty() && review_gaps.is_empty();
        fs::create_dir_all(output.parent().ok_or("audit output parent required")?)?;
        save(
            &output,
            &json!({"schema_version":1,"frozen_input_sha256":ta::digest(&input)?,
            "analysis_result_sha256":ta::digest(&result)?,"input_error":input_error,
            "structural_gaps":gaps,"review_gaps":review_gaps,
            "structural_valid":accepted,"semantic_acceptance":"not_assessed_by_structural_validator"}),
        )?;
        if !accepted {
            return Err("source analysis failed offline structural audit; see report".into());
        }
        return Ok(());
    }
    if args.len() != 5 {
        return Err(
            "usage: tender_sample extract <input-directory> <explicit-limits.json> <run-directory>"
                .into(),
        );
    }
    let mode = &args[1];
    let input_dir = PathBuf::from(&args[2]);
    let root = PathBuf::from(&args[4]);
    fs::create_dir_all(&root)?;
    let lock_path = root.join("running.lock");
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock_path)?;
    let _lock = Lock(lock_path);
    let input: ta::FrozenInput = read(input_dir.join("frozen-input.json"))?;
    let limits: Value = read(&args[3])?;
    let provider = AuthoringRuntimeContractV1::resolve_tools_from_environment()?;
    if mode != "extract" {
        return Err("retired reviewer seed entry: use extract for the formal Discover/Organize/Check runtime".into());
    }
    let contract =
        json!({"mode":mode,"input_sha256":ta::digest(&input)?,"provider":provider,"limits":limits});
    let contract_path = root.join("run-contract.json");
    if contract_path.exists() {
        let previous: Value = read(&contract_path)?;
        if previous != contract {
            return Err(
                "frozen input, env provider or budgets changed; use a new run directory".into(),
            );
        }
    } else {
        save(&contract_path, &contract)?;
    }
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            c.cancel();
        }
    });
    let journal = Local {
        root: root.clone(),
        views: input_dir.join("source-views"),
        input: input.clone(),
        count: Mutex::new(if root.join("calls.json").exists() {
            read(root.join("calls.json"))?
        } else {
            Calls::default()
        }),
    };
    match mode.as_str() {
        "extract" => {
            let config = extraction::Config::with_provider_for(
                provider,
                serde_json::from_value(limits["extraction"].clone())?,
                Some(&input),
            )?;
            save(root.join("runtime.json"), &config)?;
            let result = extraction::run(
                &input,
                &config,
                &journal,
                &extraction::ConfiguredModel,
                &cancel,
            )
            .await?;
            let name = "analysis-result.json";
            save(root.join(name), &result)?;
        }
        _ => return Err("unknown mode".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires KB_TENDER_PAUSED_RUN_DIR, KB_TENDER_PAUSE_ARCHIVE_DIR and KB_TENDER_RESUME_REPORT; offline real journal recovery only"]
    async fn paused_prepared_turn_reserves_original_bytes_without_executing_tools() {
        capture_paused_turn(true).await;
    }

    #[tokio::test]
    #[ignore = "requires KB_TENDER_PAUSED_RUN_DIR, KB_TENDER_PAUSE_ARCHIVE_DIR and KB_TENDER_RESUME_REPORT; offline committed-boundary recovery only"]
    async fn paused_committed_turn_prepares_next_request_without_resetting_business_state() {
        capture_paused_turn(false).await;
    }

    async fn capture_paused_turn(prepared: bool) {
        use sha2::{Digest, Sha256};

        struct Capture {
            received: Mutex<Vec<Vec<u8>>>,
            config_sha256: String,
            cancel: CancellationToken,
        }
        #[async_trait]
        impl extraction::Model for Capture {
            async fn turn(
                &self,
                config: &extraction::Config,
                body: &[u8],
            ) -> Result<knowledge::models::ChatTurn, AgentError> {
                assert_eq!(ta::digest(config).unwrap(), self.config_sha256);
                self.received.lock().unwrap().push(body.to_vec());
                // Stop during the production retry wait: no second reservation,
                // provider response, tool application or candidate generation.
                self.cancel.cancel();
                Err(AgentError::new(
                    "OFFLINE_CAPTURE_COMPLETE",
                    "request captured",
                ))
            }
        }

        let run = PathBuf::from(std::env::var("KB_TENDER_PAUSED_RUN_DIR").unwrap());
        let archive = PathBuf::from(std::env::var("KB_TENDER_PAUSE_ARCHIVE_DIR").unwrap());
        let report = PathBuf::from(std::env::var("KB_TENDER_RESUME_REPORT").unwrap());
        assert!(
            !report.exists(),
            "use a new report, never overwrite evidence"
        );
        let temporary =
            std::env::temp_dir().join(format!("tender-resume-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&temporary).unwrap();
        let root = temporary.join("extraction");
        fs::create_dir(&root).unwrap();
        let source = temporary.join("source");
        fs::create_dir(&source).unwrap();
        let mut originals = BTreeMap::new();
        let hash = |bytes: &[u8]| hex::encode(Sha256::digest(bytes));
        for (from, to) in [
            (
                archive.join("paused-checkpoint.json"),
                root.join("checkpoint.json"),
            ),
            (archive.join("paused-calls.json"), root.join("calls.json")),
            (
                archive.join("paused-runtime.json"),
                root.join("runtime.json"),
            ),
            (
                archive.join("paused-run-contract.json"),
                root.join("run-contract.json"),
            ),
            (
                run.join("source/frozen-input.json"),
                source.join("frozen-input.json"),
            ),
        ] {
            let bytes = fs::read(&from).unwrap();
            originals.insert(from, hash(&bytes));
            fs::write(to, bytes).unwrap();
        }
        // Keep every prior reservation identity, not just the resumed boundary.
        for entry in fs::read_dir(run.join("extraction")).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if name.to_str().unwrap().starts_with("turn-") {
                let bytes = fs::read(entry.path()).unwrap();
                originals.insert(entry.path(), hash(&bytes));
                fs::write(root.join(name), bytes).unwrap();
            }
        }
        let input: ta::FrozenInput = read(source.join("frozen-input.json")).unwrap();
        let config: extraction::Config = read(root.join("runtime.json")).unwrap();
        let contract: Value = read(root.join("run-contract.json")).unwrap();
        let before: extraction::Checkpoint = read(root.join("checkpoint.json")).unwrap();
        let before_calls: Calls = read(root.join("calls.json")).unwrap();
        config.validate().unwrap();
        let rebuilt =
            extraction::Config::with_provider(config.provider.clone(), config.limits.clone())
                .unwrap();
        assert_eq!(ta::digest(&rebuilt).unwrap(), before.config_sha256);
        assert_eq!(ta::digest(&input).unwrap(), before.input_sha256);
        assert_eq!(contract["input_sha256"], before.input_sha256);
        assert_eq!(
            contract["provider"],
            serde_json::to_value(&config.provider).unwrap()
        );
        assert_eq!(
            serde_json::to_value(
                serde_json::from_value::<extraction::Limits>(
                    contract["limits"]["extraction"].clone(),
                )
                .unwrap(),
            )
            .unwrap(),
            serde_json::to_value(&config.limits).unwrap()
        );
        let identity = format!("turn-{}-main", before.turn);
        let original_reserved = if prepared {
            let pending = before.journal.pending.as_ref().unwrap();
            assert!(pending.response.is_none());
            assert_eq!(pending.turn, before.turn);
            assert_eq!(pending.role, "main");
            let reserved = fs::read(root.join(format!("{identity}.json"))).unwrap();
            assert_eq!(reserved, pending.body.as_bytes());
            Some(reserved)
        } else {
            assert!(before.journal.pending.is_none());
            assert!(!before_calls.attempts.contains_key(&identity));
            assert!(!root.join(format!("{identity}.json")).exists());
            None
        };
        let journal = Local {
            root: root.clone(),
            views: source.join("source-views"),
            input: input.clone(),
            count: Mutex::new(read(root.join("calls.json")).unwrap()),
        };
        let cancel = CancellationToken::new();
        let model = Capture {
            received: Mutex::new(vec![]),
            config_sha256: before.config_sha256.clone(),
            cancel: cancel.clone(),
        };
        let error = extraction::run(&input, &config, &journal, &model, &cancel)
            .await
            .unwrap_err();
        assert_eq!(error.code, "INTERNAL");
        assert_eq!(error.message, "Agent run cancelled");
        let received = model.received.lock().unwrap();
        assert_eq!(received.len(), 1);
        let reserved = fs::read(root.join(format!("{identity}.json"))).unwrap();
        assert_eq!(received[0], reserved);
        let after: extraction::Checkpoint = read(root.join("checkpoint.json")).unwrap();
        let pending = after.journal.pending.as_ref().unwrap();
        assert_eq!(pending.body.as_bytes(), reserved);
        assert_eq!(pending.turn, before.turn);
        assert_eq!(pending.role, "main");
        assert!(pending.response.is_none());
        let before_value = serde_json::to_value(&before).unwrap();
        let after_value = serde_json::to_value(&after).unwrap();
        if let Some(original_reserved) = &original_reserved {
            assert_eq!(reserved, *original_reserved);
            assert_eq!(after_value, before_value);
        } else {
            assert_eq!(after.journal.sequence, before.journal.sequence + 1);
            // Preparing a new request may compact retained messages, stage
            // reading receipts and rebuild the SDK session. It cannot execute
            // a repair, grant recovery or reset any role's progress ledger.
            for field in [
                "input_sha256",
                "config_sha256",
                "turn",
                "tool_calls",
                "read_bytes",
                "review_rounds",
                "role",
                "analysis",
                "main_progress",
                "main_work",
                "done",
            ] {
                assert_eq!(after_value[field], before_value[field], "changed {field}");
            }
        }
        assert_eq!(
            after.main_progress.seen, before.main_progress.seen,
            "known and consumed recovery history must not reset or advance"
        );
        let after_calls: Calls = read(root.join("calls.json")).unwrap();
        let mut expected_attempts = before_calls.attempts.clone();
        *expected_attempts.entry(identity.clone()).or_default() += 1;
        assert_eq!(after_calls.total, before_calls.total + 1);
        assert_eq!(after_calls.attempts, expected_attempts);
        assert!(!root.join("progress.json").exists());
        for (path, expected) in &originals {
            assert_eq!(
                hash(&fs::read(path).unwrap()),
                *expected,
                "original changed: {}",
                path.display()
            );
            if path.parent() == Some(run.join("extraction").as_path()) {
                assert_eq!(
                    hash(&fs::read(root.join(path.file_name().unwrap())).unwrap()),
                    *expected
                );
            }
        }
        fs::create_dir_all(report.parent().unwrap()).unwrap();
        let captured_request = if prepared {
            None
        } else {
            let path = report.with_extension("request.json");
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .unwrap();
            file.write_all(&received[0]).unwrap();
            file.sync_all().unwrap();
            Some(path)
        };
        save(&report, &json!({
            "test":if prepared {"paused_prepared_turn_reserves_original_bytes_without_executing_tools"}
                else {"paused_committed_turn_prepares_next_request_without_resetting_business_state"},
            "starting_boundary":if prepared {"prepared"} else {"committed"},
            "run":run,"archive":archive,"temporary_owned_directory":temporary,
            "turn":before.turn,"calls_before":before_calls.total,"calls_after":after_calls.total,
            "attempt_before":before_calls.attempts.get(&identity).copied().unwrap_or(0),"attempt_after":after_calls.attempts[&identity],
            "captured_requests":received.len(),
            "captured_request_file":captured_request,
            "body_bytes":reserved.len(),"body_sha256":hash(&reserved),
            "captured_equals_pending_equals_reserved":true,
            "config_sha256":before.config_sha256,"runtime_schema_prompt_digests_match":true,
            "contract_digests":{
                "main_tools":config.tools_sha256,
                "main_prompt":config.main_prompt_sha256
            },
            "records":before.analysis.records.len(),"relations":before.analysis.relations.len(),
            "repair_events":before.outline_run.repair_events.len(),
            "main_blockers":before.main_progress.blockers.len(),
            "journal_sequence_before":before.journal.sequence,"journal_sequence_after":after.journal.sequence,
            "recovery_known_markers":before.main_progress.seen.iter().filter(|s| s.starts_with("main-repair-history-recovery-v1:known:")).count(),
            "recovery_consumed_markers":before.main_progress.seen.iter().filter(|s| s.starts_with("main-repair-history-recovery-v1:consumed:")).count(),
            "business_state_and_recovery_ledgers_unchanged":true,
            "entire_checkpoint_semantically_unchanged":prepared,"prior_call_records_preserved":true,
            "original_files_unchanged":originals,"provider_calls":0,"generated_candidates":0,
            "termination":error.message,"full_acceptance":false
        })).unwrap();
        fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn journal_preserves_attempt_receipts_without_a_second_cumulative_limit() {
        let directory =
            std::env::temp_dir().join(format!("tender-journal-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let mut journal = Local {
            root: directory.clone(),
            views: directory.clone(),
            input: ta::FrozenInput {
                schema_version: bidding::outline::frozen::FROZEN_SCHEMA_VERSION,
                project_id: String::new(),
                document_set_id: String::new(),
                documents: vec![],
                document_relations: vec![],
                source_units: vec![],
                structured_forms: vec![],
                decisions: vec![],
            },
            count: Mutex::new(Calls::default()),
        };
        for turn in 0..3 {
            assert_eq!(
                journal.reserve(turn, "main", b"request").unwrap(),
                (turn + 1, 1)
            );
        }
        assert_eq!(journal.reserve(2, "main", b"request").unwrap(), (4, 2));
        // Reload persisted reservations as a new process would. A changed
        // request cannot consume a slot or reset its existing attempts.
        journal.count = Mutex::new(read(journal.root.join("calls.json")).unwrap());
        assert!(journal.reserve(2, "main", b"changed").is_err());
        assert_eq!(journal.reserve(2, "main", b"request").unwrap(), (5, 3));
        journal.count = Mutex::new(read(journal.root.join("calls.json")).unwrap());
        assert!(journal.reserve(2, "main", b"request").is_err());
        // Composition requires the global count, extraction the boundary slot.
        assert_eq!(journal.reserve(2, "reviewer", b"review").unwrap(), (6, 1));
        assert_eq!(journal.reserve(3, "main", b"request").unwrap(), (7, 1));
        assert_eq!(journal.reserve(4, "main", b"request").unwrap(), (8, 1));
        let saved: Calls = read(journal.root.join("calls.json")).unwrap();
        assert_eq!(saved.total, 8);
        assert_eq!(saved.attempts["turn-2-main"], 3);
        fs::remove_dir_all(directory).unwrap();
    }
}

#[cfg(test)]
mod acceptance_guard_tests {
    use super::*;
    #[test]
    fn each_private_spending_boundary_stops_before_send() {
        use bidding::agent_runtime::budget::ModelAccounting;
        let allowed = ModelAccounting {
            physical_calls: 71,
            reserved_input_tokens: 7_166_928,
            reserved_output_tokens: 581_632,
            started_unix_seconds: Some(100),
        };
        assert!(acceptance_admission(&allowed, 5537).is_ok());
        assert!(acceptance_admission(&allowed, 5538).is_err());
        let mut calls = allowed.clone();
        calls.physical_calls += 1;
        assert!(acceptance_admission(&calls, 100).is_err());
        let mut input = allowed.clone();
        input.reserved_input_tokens += 1;
        assert!(acceptance_admission(&input, 100).is_err());
        let mut output = allowed;
        output.reserved_output_tokens += 1;
        assert!(acceptance_admission(&output, 100).is_err());
    }
}
