//! Reviewer-only final-file loop. Reuses drive()/Journal; does not reuse
//! composition workspace.done or edit FrozenSource.
use super::{Inventory, OutputCoverage, OutputUnit, read_output_evidence};
use crate::agent_error::AgentError;
use crate::agent_runtime::{Driver, Status, drive};
use crate::agent_runtime::progress::Progress;
use crate::authoring_runtime::AuthoringRuntimeContractV1;
use crate::tender_analysis::{
    Analysis, AnalysisResult, Coverage, FrozenInput, digest, tools,
};
use async_trait::async_trait;
use knowledge::models::ChatTurn;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

const REVIEWER: &str = include_str!("../../prompts/export-review-v1.txt");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default = "crate::agent_runtime::progress::default_no_progress_turns")]
    pub max_no_progress_turns: usize,
    #[serde(default = "crate::agent_runtime::progress::default_focus_turns")]
    pub max_focus_turns: usize,
    #[serde(default = "crate::agent_runtime::progress::default_focus_replans")]
    pub max_focus_replans: usize,
    pub max_turns: usize,
    pub max_tool_calls: usize,
    pub max_physical_calls: usize,
    pub max_read_bytes: usize,
    pub max_context_bytes: usize,
    #[serde(default = "crate::agent_runtime::chat::default_context_tokens")]
    pub max_context_tokens: usize,
    #[serde(default = "crate::agent_runtime::chat::default_image_token_reserve")]
    pub image_token_reserve: usize,
    #[serde(default = "crate::agent_runtime::chat::default_token_safety_margin")]
    pub token_safety_margin: usize,
    pub max_tool_result_bytes: usize,
}

impl Limits {
    fn progress(&self) -> crate::agent_runtime::progress::ProgressLimits {
        crate::agent_runtime::progress::ProgressLimits {
            max_no_progress_turns: self.max_no_progress_turns,
            max_focus_turns: self.max_focus_turns,
            max_focus_replans: self.max_focus_replans,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub provider: AuthoringRuntimeContractV1,
    pub limits: Limits,
}

pub struct ConfiguredModel;

#[async_trait]
impl Model for ConfiguredModel {
    async fn turn(&self, config: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        crate::agent_runtime::chat::provider_turn(&config.provider, body).await
    }
}

impl Config {
    pub fn from_environment() -> Result<Self, AgentError> {
        let raw = std::env::var("KB_EXPORT_REVIEW_LIMITS").map_err(|_| {
            AgentError::new(
                "AGENT_PROVIDER_UNAVAILABLE",
                "KB_EXPORT_REVIEW_LIMITS is required",
            )
        })?;
        let config = Self {
            provider: AuthoringRuntimeContractV1::resolve_tools_from_environment()
                .map_err(|e| AgentError::new("AGENT_PROVIDER_UNAVAILABLE", e))?,
            limits: serde_json::from_str(&raw).map_err(invalid)?,
        };
        config.contract_sha256()?;
        Ok(config)
    }

    pub fn contract_sha256(&self) -> Result<String, AgentError> {
        self.provider.validate().map_err(invalid)?;
        let l = &self.limits;
        if !l.progress().validate()
            || self.provider.response_mode != "tool_calls"
            || [
                l.max_turns,
                l.max_tool_calls,
                l.max_physical_calls,
                l.max_read_bytes,
                l.max_context_bytes,
                l.max_context_tokens,
                l.image_token_reserve,
                l.token_safety_margin,
                l.max_tool_result_bytes,
            ]
            .contains(&0)
            || l.max_context_bytes <= l.max_tool_result_bytes
        {
            return Err(invalid("invalid export-review limits"));
        }
        digest(&json!({
            "checkpoint_contract_version":crate::agent_runtime::CHECKPOINT_CONTRACT_VERSION,
            "runtime_adapter":crate::agent_runtime::RUNTIME_ADAPTER_VERSION,
            "config":self,
            "reviewer":crate::agent_runtime::chat::system_content(REVIEWER),
            "tools":schemas(),
        }))
        .map_err(invalid)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemConclusion {
    Pass,
    Findings,
    SourceLimited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemReview {
    pub item_id: String,
    pub conclusion: ItemConclusion,
    #[serde(default)]
    pub finding_ids: Vec<String>,
    pub grounds: Vec<crate::tender_analysis::Span>,
    pub artifact_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub journal: crate::agent_runtime::TurnJournal,
    pub contract_sha256: String,
    pub inventory: Inventory,
    pub analysis: Analysis,
    pub tender_coverage: Coverage,
    pub output_coverage: OutputCoverage,
    pub reviews: BTreeMap<String, ItemReview>,
    pub turn: usize,
    pub tool_calls: usize,
    pub read_bytes: usize,
    pub transcript: Vec<Value>,
    pub progress: Progress,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub docx_sha256: String,
    pub pdf_sha256: Option<String>,
    pub inventory_sha256: String,
    pub status: String,
    pub reviews: BTreeMap<String, ItemReview>,
}

#[async_trait]
pub trait Journal: Send + Sync {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError>;
    async fn reserve(&self, state: &Checkpoint, body: &[u8]) -> Result<usize, AgentError>;
    async fn save(&self, state: &Checkpoint) -> Result<(), AgentError>;
}

#[async_trait]
pub trait Model: Send + Sync {
    async fn turn(&self, config: &Config, body: &[u8]) -> Result<ChatTurn, AgentError>;
}

pub fn schemas() -> Vec<Value> {
    let mut tools: Vec<_> = tools::schemas(true)
        .into_iter()
        .filter(|v| {
            matches!(
                v["function"]["name"].as_str(),
                Some(
                    "collection_index"
                        | "source_index"
                        | "read_source"
                        | "read_form"
                        | "search_sources"
                        | "inspect_analysis"
                        | "read_source_view"
                )
            )
        })
        .collect();
    tools.extend(super::schemas());
    tools
}

struct RunDriver<'a, J, M> {
    input: &'a FrozenInput,
    config: &'a Config,
    state: &'a mut Checkpoint,
    journal: &'a J,
    model: &'a M,
}

#[async_trait]
impl<J: Journal, M: Model> Driver for RunDriver<'_, J, M> {
    fn status(&self) -> Status<'_> {
        let limits = &self.config.limits;
        Status {
            journal: &self.state.journal,
            max_context_bytes: limits.max_context_bytes,
            turn: self.state.turn,
            role: "reviewer",
            done: self.state.done,
            execution_blocked: self.state.progress.handoff_exhausted(&limits.progress()),
            budget_exhausted: self.state.turn >= limits.max_turns
                || self.state.tool_calls >= limits.max_tool_calls
                || self.state.read_bytes >= limits.max_read_bytes,
        }
    }
    fn journal_mut(&mut self) -> &mut crate::agent_runtime::TurnJournal {
        &mut self.state.journal
    }
    async fn prepare_request(&mut self) -> Result<Vec<u8>, AgentError> {
        let body = request(self.state, self.config).await?;
        self.state.journal.prepare_session(
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::COMPOSITION_SESSION_SUFFIX,
            self.config.limits.max_turns - self.state.turn,
            self.config.limits.max_context_bytes,
        )?;
        Ok(body)
    }
    async fn reserve(&self, body: &[u8], local_attempt: usize) -> Result<usize, AgentError> {
        let count = self.journal.reserve(self.state, body).await?;
        if count > self.config.limits.max_physical_calls {
            return Err(AgentError::new(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "physical boundary budget exhausted",
            ));
        }
        Ok(local_attempt)
    }
    async fn call_model(&self, body: &[u8]) -> Result<ChatTurn, AgentError> {
        self.model.turn(self.config, body).await
    }
    async fn execute(
        &mut self,
        response: ChatTurn,
        suppressed: BTreeMap<String, String>,
        cancel: &CancellationToken,
    ) -> Result<Vec<Value>, AgentError> {
        execute_turn(
            self.input,
            self.config,
            self.state,
            response,
            suppressed,
            cancel,
        )
        .await
    }
    async fn save(&self) -> Result<(), AgentError> {
        self.journal.save(self.state).await
    }
}

pub struct FrozenFiles<'a> {
    pub docx: &'a [u8],
    pub pdf: Option<&'a [u8]>,
}

pub async fn run<J: Journal, M: Model>(
    input: &FrozenInput,
    result: &AnalysisResult,
    files: FrozenFiles<'_>,
    config: &Config,
    journal: &J,
    model: &M,
    cancel: &CancellationToken,
) -> Result<Report, AgentError> {
    let contract = config.contract_sha256()?;
    let inventory = crate::export_review::inventory_from_files(files.docx, files.pdf).map_err(invalid)?;
    let mut state = match journal.load().await? {
        Some(state) => state,
        None => Checkpoint {
            journal: Default::default(),
            contract_sha256: contract.clone(),
            inventory: inventory.clone(),
            analysis: result.analysis.clone(),
            tender_coverage: Coverage::default(),
            output_coverage: OutputCoverage::default(),
            reviews: BTreeMap::new(),
            turn: 0,
            tool_calls: 0,
            read_bytes: 0,
            transcript: vec![],
            progress: Progress::default(),
            done: false,
        },
    };
    if state.contract_sha256 != contract
        || state.inventory.docx_sha256 != inventory.docx_sha256
        || state.inventory.pdf_sha256 != inventory.pdf_sha256
    {
        return Err(AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "export-review inventory or runtime changed",
        ));
    }
    state.inventory = inventory;
    state.journal.validate(state.turn, "reviewer")?;
    drive(
        &mut RunDriver {
            input,
            config,
            state: &mut state,
            journal,
            model,
        },
        cancel,
    )
    .await?;
    report(&state)
}

impl Checkpoint {
    pub fn completed_report(&self) -> Option<Report> {
        report(self).ok()
    }
}

fn report(state: &Checkpoint) -> Result<Report, AgentError> {
    if !state.done {
        return Err(invalid("export review is incomplete"));
    }
    let findings = state
        .reviews
        .values()
        .any(|r| r.conclusion == ItemConclusion::Findings);
    Ok(Report {
        docx_sha256: state.inventory.docx_sha256.clone(),
        pdf_sha256: state.inventory.pdf_sha256.clone(),
        inventory_sha256: digest(&state.inventory).map_err(invalid)?,
        status: if findings {
            "reviewed_with_findings"
        } else {
            "reviewed"
        }
        .into(),
        reviews: state.reviews.clone(),
    })
}

async fn request(state: &Checkpoint, config: &Config) -> Result<Vec<u8>, AgentError> {
    let next = state
        .inventory
        .units
        .iter()
        .find(|unit| !state.reviews.contains_key(&unit.id))
        .map(OutputUnit::id_json);
    let messages = vec![
        json!({"role":"system","content":REVIEWER}),
        json!({"role":"user","content":json!({
            "docx_sha256":state.inventory.docx_sha256,
            "pdf_sha256":state.inventory.pdf_sha256,
            "inventory_total":state.inventory.units.len(),
            "reviewed":state.reviews.len(),
            "next_item":next,
            "output_coverage_units":state.output_coverage.units.len(),
            "tender_coverage_sources":state.tender_coverage.text.len(),
            "turn":state.turn,
        }).to_string()}),
    ];
    let mut messages = messages;
    messages.extend(state.transcript.clone());
    crate::agent_runtime::chat::prepare(&config.provider, messages, schemas()).await
}

impl OutputUnit {
    fn id_json(&self) -> Value {
        json!({"id":self.id,"kind":self.kind,"part":self.part,"ordinal":self.ordinal})
    }
}

async fn execute_turn(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    response: ChatTurn,
    suppressed: BTreeMap<String, String>,
    cancel: &CancellationToken,
) -> Result<Vec<Value>, AgentError> {
    let l = &config.limits;
    state.transcript.push(json!({"role":"assistant","content":response.content,"tool_calls":response.tool_calls.iter().map(|c|json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments}})).collect::<Vec<_>>()}));
    let mut tool_results = Vec::new();
    for (index, call) in response.tool_calls.into_iter().enumerate() {
        if index > 0 {
            crate::agent_runtime::check_cancel(cancel)?;
        }
        let result_value = if state.tool_calls >= l.max_tool_calls
            || state.read_bytes >= l.max_read_bytes
            || state.done
        {
            Err("export-review budget exhausted or already complete".into())
        } else {
            state.tool_calls += 1;
            match serde_json::from_str::<Value>(&call.arguments).map_err(|e| e.to_string()) {
                _ if suppressed.contains_key(&call.id) => Err("tool unavailable in this role".into()),
                Err(e) => Err(e),
                Ok(args) if call.name == "read_output_evidence" => {
                    read_output_evidence(&state.inventory, &mut state.output_coverage, &args, l.max_tool_result_bytes)
                }
                Ok(args) if call.name == "put_composition_review" => put_review(state, &args),
                Ok(args) => {
                    let mut coverage = state.tender_coverage.clone();
                    let out = tools::invoke(
                        input,
                        &mut state.analysis,
                        &mut coverage,
                        true,
                        &call.name,
                        &args,
                        l.max_tool_result_bytes,
                    );
                    if out.is_ok() {
                        state.tender_coverage = coverage;
                    }
                    out
                }
            }
        };
        let value = match result_value {
            Ok(v) => json!({"ok":true,"result":v}),
            Err(e) => json!({"ok":false,"error":e}),
        };
        let content = suppressed
            .get(&call.id)
            .cloned()
            .unwrap_or_else(|| value.to_string());
        let message = json!({"role":"tool","tool_call_id":call.id,"content":content});
        tool_results.push(message.clone());
        state.transcript.push(message);
        tokio::task::yield_now().await;
    }
    state.done = complete(state);
    state.turn += 1;
    Ok(tool_results)
}

fn put_review(state: &mut Checkpoint, args: &Value) -> Result<Value, String> {
    let item_id = args["item_id"].as_str().ok_or("item_id required")?.to_owned();
    let unit = state
        .inventory
        .units
        .iter()
        .find(|unit| unit.id == item_id)
        .ok_or("unknown output evidence id")?;
    let conclusion: ItemConclusion =
        serde_json::from_value(args["conclusion"].clone()).map_err(|e| e.to_string())?;
    if unit.kind == "not_checked" && conclusion == ItemConclusion::Pass {
        return Err("not_checked units cannot pass without page inspection".into());
    }
    let grounds: Vec<crate::tender_analysis::Span> =
        serde_json::from_value(args["grounds"].clone()).map_err(|e| e.to_string())?;
    if conclusion == ItemConclusion::SourceLimited && grounds.is_empty() {
        return Err("source_limited conclusion needs grounds".into());
    }
    let finding_ids: Vec<String> = args
        .get("finding_ids")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if conclusion == ItemConclusion::Findings && finding_ids.is_empty() {
        return Err("findings conclusion needs finding ids".into());
    }
    state.reviews.insert(
        item_id.clone(),
        ItemReview {
            item_id: item_id.clone(),
            conclusion,
            finding_ids,
            grounds,
            artifact_sha256: state.inventory.docx_sha256.clone(),
        },
    );
    Ok(json!({"saved":true,"item_id":item_id}))
}

fn complete(state: &Checkpoint) -> bool {
    state.inventory.units.iter().all(|unit| state.reviews.contains_key(&unit.id))
        && !state.inventory.units.is_empty()
}

fn invalid(message: impl std::fmt::Display) -> AgentError {
    AgentError::new("AGENT_OUTPUT_INVALID", message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tender_analysis::{Source, Span};
    use serde_json::json;
    use std::{collections::VecDeque, io::Write, sync::Mutex};

    fn docx() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let document = format!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>投标函</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#
        );
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(document.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn input() -> FrozenInput {
        FrozenInput {
            schema_version: 1,
            project_id: "project".into(),
            document_set_id: "set".into(),
            documents: vec![],
            document_relations: vec![],
            decisions: vec![],
            structured_forms: vec![],
            source_units: vec![Source {
                source_unit_revision_id: "source".into(),
                document_id: "document".into(),
                text: "投标函".into(),
                locator: json!({"heading_path":"须知"}),
                ordinal: 0,
            }],
        }
    }

    fn analysis_result(input: &FrozenInput) -> AnalysisResult {
        let mut analysis = Analysis::default();
        analysis
            .coverage
            .text
            .insert("source".into(), vec![(0, input.source_units[0].text.len())]);
        AnalysisResult {
            schema_version: 1,
            frozen_input_sha256: digest(input).unwrap(),
            review: crate::tender_analysis::Review {
                analysis_sha256: digest(&analysis).unwrap(),
                coverage: analysis.coverage.clone(),
                findings: vec![],
                ..Default::default()
            },
            analysis,
            quality: "needs_review".into(),
            source_views: BTreeMap::new(),
        }
    }

    fn config() -> Config {
        let provider = serde_json::from_value(json!({"schema_version":1,"base_url":"https://example.invalid/v1","endpoint":"https://example.invalid/v1/chat/completions","protocol":"openai_chat_completions_sse","model_id":"scripted-fixture","credential_ref":"env:LLM_API_KEY","stream":true,"max_tokens":8192,"timeout_ms":180000,"response_mode":"tool_calls","transport_retries":0,"temperature":null,"reasoning_effort":null})).unwrap();
        Config {
            provider,
            limits: Limits {
                max_no_progress_turns: 6,
                max_focus_turns: 24,
                max_focus_replans: 2,
                max_turns: 8,
                max_tool_calls: 20,
                max_physical_calls: 20,
                max_read_bytes: 200_000,
                max_context_bytes: 200_000,
                max_context_tokens: 131072,
                image_token_reserve: 16384,
                token_safety_margin: 4096,
                max_tool_result_bytes: 20_000,
            },
        }
    }

    #[derive(Default)]
    struct Memory {
        state: Mutex<Option<Checkpoint>>,
        calls: Mutex<usize>,
    }
    #[async_trait::async_trait]
    impl Journal for Memory {
        async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
            Ok(self.state.lock().unwrap().clone())
        }
        async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<usize, AgentError> {
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            Ok(*calls)
        }
        async fn save(&self, state: &Checkpoint) -> Result<(), AgentError> {
            *self.state.lock().unwrap() = Some(state.clone());
            Ok(())
        }
    }

    struct Script {
        turns: Mutex<VecDeque<(&'static str, Value)>>,
    }
    #[async_trait::async_trait]
    impl Model for Script {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            let (name, args) = self.turns.lock().unwrap().pop_front().expect("turn");
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: vec![knowledge::models::ChatToolCall {
                    id: "c1".into(),
                    name: name.into(),
                    arguments: args.to_string(),
                }],
            })
        }
    }

    #[tokio::test]
    async fn host_review_loop_does_not_reuse_composition_done() {
        let input = input();
        let result = analysis_result(&input);
        let bytes = docx();
        let inventory = crate::export_review::inventory_from_docx(&bytes, None).unwrap();
        let id = inventory.units[0].id.clone();
        let model = Script {
            turns: Mutex::new(VecDeque::from([
                (
                    "read_output_evidence",
                    json!({"offset":0,"limit":8}),
                ),
                (
                    "put_composition_review",
                    json!({
                        "item_id":id,
                        "conclusion":"pass",
                        "grounds":[Span{
                            source_id:"source".into(),
                            start:0,
                            end:input.source_units[0].text.len(),
                            view_id:None,
                            grid_cell:None
                        }]
                    }),
                ),
            ])),
        };
        let journal = Memory::default();
        let report = run(
            &input,
            &result,
            FrozenFiles {
                docx: &bytes,
                pdf: None,
            },
            &config(),
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(report.status, "reviewed");
        assert_eq!(report.docx_sha256, inventory.docx_sha256);
        let saved = journal.state.lock().unwrap().clone().unwrap();
        assert!(saved.done);
        assert!(saved.output_coverage.units.contains_key(&id));
    }
}
