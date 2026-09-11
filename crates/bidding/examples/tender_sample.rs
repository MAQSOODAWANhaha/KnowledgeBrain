//! Local real-provider acceptance runner. No database/publication adapter.
//! Inputs are frozen outputs of the shared Python service; no uploaded-file parsing.
use async_trait::async_trait;
use bidding::{
    agent_error::AgentError,
    authoring_runtime::AuthoringRuntimeContractV1,
    docx_composition::agent as composition,
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
#[async_trait]
impl composition::Journal for Local {
    async fn load(&self) -> Result<Option<composition::Checkpoint>, AgentError> {
        let p = self.root.join("checkpoint.json");
        if p.exists() {
            Ok(Some(read(p)?))
        } else {
            Ok(None)
        }
    }
    async fn reserve(
        &self,
        state: &composition::Checkpoint,
        body: &[u8],
    ) -> Result<usize, AgentError> {
        let (total, _) = self.reserve(
            state.turn,
            if state.workspace.reviewing {
                "reviewer"
            } else {
                "main"
            },
            body,
        )?;
        save(self.root.join("checkpoint.json"), state)?;
        Ok(total)
    }
    async fn save(&self, s: &composition::Checkpoint) -> Result<(), AgentError> {
        save(self.root.join("checkpoint.json"), s)?;
        eprintln!(
            "{}",
            json!({"turn":s.turn,"sections":s.workspace.draft.sections.len(),"reviewing":s.workspace.reviewing,"done":s.workspace.done})
        );
        Ok(())
    }
}
struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
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
        return Err("usage: tender_sample <extract|review|compose> <input-directory> <explicit-limits.json> <run-directory>".into());
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
    let review_seed: Option<ta::Analysis> = if mode == "review" {
        let seed = read(input_dir.join("analysis-seed.json"))?;
        if !ta::tools::gaps(&input, &seed).is_empty() {
            return Err("review diagnostic seed has structural or primary reading gaps".into());
        }
        Some(seed)
    } else {
        None
    };
    let mut contract = json!({"mode":mode,"input_sha256":ta::digest(&input)?,
                          "provider":provider,"limits":limits});
    if let Some(seed) = &review_seed {
        contract["review_seed_sha256"] = json!(ta::digest(seed)?);
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
        "extract" | "review" => {
            let config = extraction::Config::with_provider(
                provider,
                serde_json::from_value(limits["extraction"].clone())?,
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
                    role: extraction::Role::Reviewer,
                    analysis: seed,
                    review: None,
                    review_draft: BTreeMap::new(),
                    source_review: Some(extraction::source_review::initialize(&input, &config)?),
                    reviewer_coverage: Default::default(),
                    pending_coverage: None,
                    transcript: vec![],
                    main_progress: Default::default(),
                    reviewer_progress: Default::default(),
                    main_work: None,
                    reviewer_work: None,
                    done: false,
                    source_views: BTreeMap::new(),
                };
                extraction::source_review::select_next(&input, &config, &mut state)?;
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
            let name = if mode == "review" {
                "review-diagnostic-result.json"
            } else {
                "analysis-result.json"
            };
            save(root.join(name), &result)?;
        }
        "compose" => {
            let result: ta::AnalysisResult = read(input_dir.join("analysis-result.json"))?;
            let config = composition::Config {
                provider,
                limits: serde_json::from_value(limits["composition"].clone())?,
            };
            save(root.join("runtime.json"), &config)?;
            let artifact = composition::run(
                &input,
                &result,
                &config,
                &journal,
                &composition::ConfiguredModel,
                &cancel,
            )
            .await?;
            use base64::Engine;
            fs::write(
                root.join("bid-template.docx"),
                base64::engine::general_purpose::STANDARD.decode(&artifact.docx_base64)?,
            )?;
            save(root.join("manifest.json"), &artifact.manifest)?;
            save(root.join("rendered.json"), &artifact.rendered)?;
        }
        _ => return Err("unknown mode".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
