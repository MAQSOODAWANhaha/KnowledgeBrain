//! Local real-provider acceptance runner. No database/publication adapter.
//! Inputs are frozen outputs of the shared Python service; no uploaded-file parsing.
use async_trait::async_trait;
use bidding::{
    agent_error::AgentError,
    authoring_runtime::AuthoringRuntimeContractV1,
    tender_analysis::{self as ta, agent as extraction},
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
struct Local {
    root: PathBuf,
    views: PathBuf,
    input: ta::FrozenInput,
    count: Mutex<Calls>,
    max_calls: usize,
}
impl Local {
    fn reserve(&self, turn: usize, role: &str, body: &[u8]) -> Result<(usize, usize), AgentError> {
        let mut count = self.count.lock().map_err(err)?;
        let boundary = format!("turn-{turn}-{role}");
        let attempt = count.attempts.get(&boundary).copied().unwrap_or(0);
        if count.total >= self.max_calls || attempt >= 3 {
            return Err(AgentError::new(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "physical call or boundary retry budget exhausted",
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
        let mut view: ta::views::SourceView = read(self.views.join(format!("page-{page}.json")))?;
        let document = self
            .input
            .documents
            .iter()
            .find(|d| d["document_id"] == source.document_id)
            .ok_or_else(|| err("source document missing"))?;
        if view.identity.page_ordinal as u64 != page
            || document["sha256"] != view.identity.original_sha256
        {
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
fn validate_repair_seed_budget(seed: &extraction::Checkpoint) -> Result<(), AgentError> {
    let empty_watch =
        serde_json::to_value(bidding::agent_runtime::progress::ProgressWatch::default())
            .map_err(err)?;
    if seed.journal.pending.is_some()
        || !seed.main_progress.blockers.is_empty()
        || !seed.reviewer_progress.blockers.is_empty()
        || serde_json::to_value(&seed.main_progress.watch).map_err(err)? != empty_watch
        || serde_json::to_value(&seed.reviewer_progress.watch).map_err(err)? != empty_watch
        || serde_json::to_value(&seed.repair).map_err(err)?
            != serde_json::to_value(extraction::repair::State::default()).map_err(err)?
    {
        return Err(err(
            "repair seed import cannot reset prior execution or repair allowances; preserve the original checkpoint and use a supported recovery contract",
        ));
    }
    Ok(())
}

// Import only archived model claims validated by the same production gates.
// This does not carry the old review's reading or completion receipts into a
// new contract. The immutable old checkpoint remains the audit of that attempt.
fn validate_repair_seed(
    input: &ta::FrozenInput,
    seed: &extraction::Checkpoint,
) -> Result<(), AgentError> {
    validate_repair_seed_budget(seed)?;
    if seed.input_sha256 != ta::digest(input).map_err(err)?
        || !ta::tools::gaps(input, &seed.analysis).is_empty()
        || seed.review_draft.is_empty()
    {
        return Err(err(
            "repair seed needs the same frozen input, a structurally complete candidate and archived model findings",
        ));
    }
    for finding in seed.review_draft.values() {
        extraction::validate_finding(input, seed, finding).map_err(err)?;
    }
    Ok(())
}

// Preserve later candidate edits separately from the archived feedback evidence.
// Neither input imports an independent approval into the new runtime contract.
fn read_candidate_seed(
    input: &ta::FrozenInput,
    mode: &str,
    input_dir: &Path,
    repair_seed: Option<&extraction::Checkpoint>,
) -> Result<Option<ta::Analysis>, AgentError> {
    Ok(
        if mode == "review"
            || (repair_seed.is_some() && input_dir.join("analysis-seed.json").exists())
        {
            let seed = read(input_dir.join("analysis-seed.json"))?;
            if !ta::tools::gaps(input, &seed).is_empty() {
                return Err(err(
                    "review diagnostic seed has structural or primary reading gaps",
                ));
            }
            Some(seed)
        } else {
            repair_seed.map(|seed| seed.analysis.clone())
        },
    )
}

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
        let basis_error = bidding::docx_composition::validate_basis(&input, &result).err();
        let accepted = basis_error.is_none();
        fs::create_dir_all(output.parent().ok_or("audit output parent required")?)?;
        save(
            &output,
            &json!({"schema_version":1,"frozen_input_sha256":ta::digest(&input)?,
            "analysis_result_sha256":ta::digest(&result)?,"input_error":input_error,
            "structural_gaps":gaps,"review_gaps":review_gaps,"composition_basis_error":basis_error,
            "composition_basis_valid":accepted,"semantic_acceptance":"not_assessed_by_structural_validator"}),
        )?;
        if !accepted {
            return Err("source analysis failed offline structural audit; see report".into());
        }
        return Ok(());
    }
    if args.len() != 5 {
        return Err("usage: tender_sample <extract|review|repair> <input-directory> <explicit-limits.json> <run-directory>".into());
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
    let mut limits: Value = read(&args[3])?;
    let operator_turns = limits["extraction"]["max_turns"].as_u64().unwrap_or(0) as usize;
    let operator_physical = limits["max_physical_calls"]
        .as_u64()
        .ok_or("explicit physical call limit required")? as usize;
    let pack_max_units = limits["extraction"]["pack_max_units"].as_u64().unwrap_or(1) as usize;
    let pack_max_chars = limits["extraction"]["pack_max_chars"].as_u64().unwrap_or(0) as usize;
    if limits["extraction"]["draft_path"]
        .as_bool()
        .unwrap_or(false)
    {
        let turns = if operator_turns == 0 {
            ta::draft::OUTLINE_MAX_TURNS
        } else {
            operator_turns.min(ta::draft::OUTLINE_MAX_TURNS)
        };
        limits["extraction"]["max_turns"] = json!(turns);
        limits["extraction"]["reviewer_reserve"] = json!(0);
        // 阶段一记的是**绝对值目标**与兜底上限，不是「不达标就算失败」的门：先要
        // 成功出骨架，然后把耗时往下压。旧的 extract-6 416 回合来自非 draft 的按
        // source 抽取路径，与骨架不可比，不再作为基线出现在产物里。
        limits["budget_estimate"] = json!({
            "kind":"draft_path",
            "applied_turns":turns,
            "applied_physical":operator_physical,
            "reviewer_reserve":0,
            "outline_turn_target":ta::draft::OUTLINE_TURN_TARGET,
            "outline_seconds_target":ta::draft::OUTLINE_DEADLINE_TARGET_SECS,
            "outline_turn_backstop":ta::draft::OUTLINE_MAX_TURNS,
            "outline_deadline_backstop_secs":ta::draft::DRAFT_DEADLINE_SECS
        });
    } else {
        match ta::budget::apply_with_pack(
            &input,
            operator_turns,
            operator_physical,
            pack_max_units,
            pack_max_chars,
        ) {
            Ok(budget) => {
                limits["extraction"]["max_turns"] = json!(budget.applied_turns);
                limits["extraction"]["reviewer_reserve"] = json!(budget.reviewer_reserve);
                limits["max_physical_calls"] = json!(budget.applied_physical);
                limits["budget_estimate"] = json!(budget);
            }
            Err(refused) => {
                return Err(format!(
                    "extraction turn estimate {} exceeds ceiling {}; refuse to start without an explicit operator override",
                    refused.estimated_turns, refused.ceiling
                )
                .into());
            }
        }
    }
    let provider = AuthoringRuntimeContractV1::resolve_tools_from_environment()?;
    let repair_seed: Option<extraction::Checkpoint> = if mode == "repair" {
        let seed = read(input_dir.join("repair-seed.json"))?;
        validate_repair_seed(&input, &seed)?;
        Some(seed)
    } else {
        None
    };
    let review_seed = read_candidate_seed(&input, mode, &input_dir, repair_seed.as_ref())?;
    let mut contract = json!({"mode":mode,"input_sha256":ta::digest(&input)?,
                          "provider":provider,"limits":limits});
    if let Some(seed) = &review_seed {
        contract["review_seed_sha256"] = json!(ta::digest(seed)?);
    }
    if let Some(seed) = &repair_seed {
        contract["repair_seed_sha256"] = json!(ta::digest(seed)?);
        contract["repair_seed_status"] =
            json!("archived model claims only; new independent review required");
    }
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
        max_calls: limits["max_physical_calls"]
            .as_u64()
            .ok_or("explicit physical call limit required")? as usize,
    };
    match mode.as_str() {
        "extract" | "review" | "repair" => {
            let config = extraction::Config::with_provider_for(
                provider,
                serde_json::from_value(limits["extraction"].clone())?,
                Some(&input),
            )?;
            save(root.join("runtime.json"), &config)?;
            if let Some(seed) = review_seed
                && !root.join("checkpoint.json").exists()
            {
                // A new isolated review of archived claims, never a rewritten
                // recovery checkpoint or inherited independent reading ledger.
                let mut state = extraction::Checkpoint {
                    journal: Default::default(),
                    input_sha256: ta::digest(&input)?,
                    config_sha256: ta::digest(&config)?,
                    turn: 0,
                    tool_calls: 0,
                    read_bytes: 0,
                    review_rounds: 0,
                    role: if repair_seed.is_some() {
                        extraction::Role::Main
                    } else {
                        extraction::Role::Reviewer
                    },
                    analysis: seed,
                    review: None,
                    review_draft: repair_seed
                        .as_ref()
                        .map(|prior| prior.review_draft.clone())
                        .unwrap_or_default(),
                    source_review: Some(ta::source_review::initialize(&input, &config)?),
                    repair: Default::default(),
                    dispatch: Default::default(),
                    reviewer_coverage: Default::default(),
                    pending_coverage: None,
                    transcript: vec![],
                    main_progress: Default::default(),
                    reviewer_progress: Default::default(),
                    main_work: None,
                    reviewer_work: None,
                    done: false,
                    source_views: BTreeMap::new(),
                    draft_stage: Default::default(),
                    draft_active_id: None,
                    draft_outline_gaps: None,
                    draft_outline_stalls: 0,
                    draft_outline_window: 0,
                    draft_degraded: Vec::new(),
                    draft_stopped: false,
                    draft_compile_object_id: None,
                    draft_docx_base64: None,
                    outline_config_sha256: None,
                    fill_config_sha256: None,
                    outline_run: Default::default(),
                };
                if state.role == extraction::Role::Reviewer {
                    ta::source_review::select_next(&input, &config, &mut state)?;
                }
                save(root.join("checkpoint.json"), &state)?;
            }
            let result = extraction::run(
                &input,
                &config,
                &journal,
                &extraction::ConfiguredModel,
                &cancel,
            )
            .await?;
            let name = if matches!(mode.as_str(), "review" | "repair") {
                "review-diagnostic-result.json"
            } else {
                "analysis-result.json"
            };
            save(root.join(name), &result)?;
        }
        _ => return Err("unknown mode".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_seed_import_rejects_spent_execution_and_repair_ledgers() {
        let clean: extraction::Checkpoint = serde_json::from_value(json!({
            "journal":bidding::agent_runtime::TurnJournal::default(),
            "input_sha256":"synthetic-input", "config_sha256":"synthetic-config",
            "turn":0, "tool_calls":0, "read_bytes":0, "review_rounds":0,
            "role":"main", "analysis":ta::Analysis::default(), "review_draft":{},
            "reviewer_coverage":ta::Coverage::default(), "transcript":[],
            "done":false, "source_views":{}
        }))
        .unwrap();
        validate_repair_seed_budget(&clean).unwrap();
        let original = json!(clean);
        let cases = [
            ("/main_progress/watch/replans", json!(1)),
            ("/reviewer_progress/watch/no_progress_turns", json!(1)),
            ("/repair/feedback_sha256", json!("prior-repair-generation")),
            ("/repair/tasks/last_committed_turn", json!(1)),
            (
                "/main_progress/blockers",
                json!([{
                    "scope":["source"], "dependencies_sha256":"unchanged",
                    "watch":{"no_progress_turns":6,"focus_turns":6,"replans":2,"recovery":"blocked"}
                }]),
            ),
            (
                "/reviewer_progress/blockers",
                json!([{
                    "scope":["source"], "dependencies_sha256":"unchanged",
                    "watch":{"no_progress_turns":6,"focus_turns":6,"replans":2,"recovery":"blocked"}
                }]),
            ),
        ];
        for (path, value) in cases {
            let mut candidate = original.clone();
            *candidate.pointer_mut(path).unwrap() = value;
            let seed: extraction::Checkpoint = serde_json::from_value(candidate).unwrap();
            let before = json!(seed);
            let error = validate_repair_seed_budget(&seed).unwrap_err();
            assert!(
                error
                    .message
                    .contains("cannot reset prior execution or repair allowances"),
                "{path}: {error:?}"
            );
            assert_eq!(json!(seed), before, "rejection must preserve {path}");
        }
        assert_eq!(json!(clean), original);
    }

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
            max_calls: contract["limits"]["max_physical_calls"].as_u64().unwrap() as usize,
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
                "review",
                "review_draft",
                "source_review",
                "repair",
                "reviewer_coverage",
                "main_progress",
                "reviewer_progress",
                "main_work",
                "reviewer_work",
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
            "physical_call_cap":journal.max_calls,"captured_requests":received.len(),
            "captured_request_file":captured_request,
            "body_bytes":reserved.len(),"body_sha256":hash(&reserved),
            "captured_equals_pending_equals_reserved":true,
            "config_sha256":before.config_sha256,"runtime_schema_prompt_digests_match":true,
            "contract_digests":{
                "main_tools":config.tools_sha256,"review_tools":config.review_tools_sha256,
                "main_prompt":config.main_prompt_sha256,"review_prompt":config.review_prompt_sha256
            },
            "records":before.analysis.records.len(),"relations":before.analysis.relations.len(),
            "repair_receipts":before.repair.results.len(),"review_rounds":before.review_rounds,
            "main_blockers":before.main_progress.blockers.len(),
            "reviewer_blockers":before.reviewer_progress.blockers.len(),
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
    #[ignore = "requires KB_TENDER_REPAIR_SEED_DIR; validates archived model claims offline"]
    fn repair_seed_requires_matching_sources_and_original_reviewer_receipts() {
        let root = PathBuf::from(std::env::var("KB_TENDER_REPAIR_SEED_DIR").unwrap());
        let original = fs::read(root.join("repair-seed.json")).unwrap();
        let input: ta::FrozenInput = read(root.join("frozen-input.json")).unwrap();
        let seed: extraction::Checkpoint = serde_json::from_slice(&original).unwrap();
        validate_repair_seed(&input, &seed).unwrap();
        let mut changed = seed.clone();
        changed.input_sha256 = "different-frozen-input".into();
        assert!(validate_repair_seed(&input, &changed).is_err());
        changed = seed.clone();
        changed.review_draft.clear();
        assert!(validate_repair_seed(&input, &changed).is_err());
        changed = seed;
        changed.reviewer_coverage = Default::default();
        assert!(validate_repair_seed(&input, &changed).is_err());
        assert_eq!(fs::read(root.join("repair-seed.json")).unwrap(), original);
    }

    #[test]
    #[ignore = "requires KB_TENDER_REPAIR_SEED_DIR and KB_TENDER_REPAIR_CANDIDATE_CHECKPOINT; archived provenance validation only"]
    fn later_model_candidates_keep_original_feedback_provenance_without_transplanted_receipts() {
        let root = PathBuf::from(std::env::var("KB_TENDER_REPAIR_SEED_DIR").unwrap());
        let input: ta::FrozenInput = read(root.join("frozen-input.json")).unwrap();
        let original = fs::read(root.join("repair-seed.json")).unwrap();
        let feedback: extraction::Checkpoint = serde_json::from_slice(&original).unwrap();
        let candidate_bytes =
            fs::read(std::env::var("KB_TENDER_REPAIR_CANDIDATE_CHECKPOINT").unwrap()).unwrap();
        let current: extraction::Checkpoint = serde_json::from_slice(&candidate_bytes).unwrap();
        assert_eq!(current.input_sha256, ta::digest(&input).unwrap());
        validate_repair_seed(&input, &feedback).unwrap();
        assert_ne!(
            ta::digest(&current.analysis).unwrap(),
            ta::digest(&feedback.analysis).unwrap()
        );
        let mut transplanted = feedback.clone();
        transplanted.analysis = current.analysis.clone();
        assert!(
            validate_repair_seed(&input, &transplanted).is_err(),
            "old reviewer receipts must not approve later changed candidate fields"
        );
        let directory = std::env::temp_dir().join(format!("tender-seed-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        save(directory.join("analysis-seed.json"), &current.analysis).unwrap();
        let selected = read_candidate_seed(&input, "repair", &directory, Some(&feedback))
            .unwrap()
            .unwrap();
        assert_eq!(
            ta::digest(&selected).unwrap(),
            ta::digest(&current.analysis).unwrap()
        );
        let mut invalid = current.analysis.clone();
        invalid.coverage = Default::default();
        save(directory.join("analysis-seed.json"), &invalid).unwrap();
        assert!(read_candidate_seed(&input, "repair", &directory, Some(&feedback)).is_err());
        fs::remove_file(directory.join("analysis-seed.json")).unwrap();
        let selected = read_candidate_seed(&input, "repair", &directory, Some(&feedback))
            .unwrap()
            .unwrap();
        assert_eq!(
            ta::digest(&selected).unwrap(),
            ta::digest(&feedback.analysis).unwrap()
        );
        fs::remove_dir(directory).unwrap();
        assert_eq!(fs::read(root.join("repair-seed.json")).unwrap(), original);
    }

    #[test]
    fn journal_keeps_boundary_retries_and_total_budget_across_restart() {
        let directory =
            std::env::temp_dir().join(format!("tender-journal-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let mut journal = Local {
            root: directory.clone(),
            views: directory.clone(),
            input: ta::FrozenInput {
                schema_version: 1,
                project_id: String::new(),
                document_set_id: String::new(),
                documents: vec![],
                document_relations: vec![],
                source_units: vec![],
                structured_forms: vec![],
                decisions: vec![],
            },
            count: Mutex::new(Calls::default()),
            max_calls: 7,
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
        assert!(journal.reserve(4, "main", b"request").is_err());
        let saved: Calls = read(journal.root.join("calls.json")).unwrap();
        assert_eq!(saved.total, 7);
        assert_eq!(saved.attempts["turn-2-main"], 3);
        fs::remove_dir_all(directory).unwrap();
    }
}
