//! Reviewer-only final-file loop. Reuses drive()/Journal; does not reuse
//! composition workspace.done or edit FrozenSource.
use super::{Inventory, OutputCoverage, OutputUnit, read_output_evidence};
use crate::agent_error::AgentError;
use crate::agent_runtime::progress::Progress;
use crate::agent_runtime::{Driver, Status, drive};
use crate::authoring_runtime::AuthoringRuntimeContractV1;
use crate::tender_analysis::{Analysis, AnalysisResult, Coverage, FrozenInput, digest, tools};
use async_trait::async_trait;
use knowledge::models::ChatTurn;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

mod delivery;
pub mod visual;

const REVIEWER: &str = include_str!("../../prompts/export-review-v1.txt");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

impl Default for Limits {
    fn default() -> Self {
        // The extraction defaults also bound a full final-document review.
        // Optional JSON overrides are frozen together with the provider contract.
        Self {
            max_no_progress_turns: crate::agent_runtime::progress::default_no_progress_turns(),
            max_focus_turns: crate::agent_runtime::progress::default_focus_turns(),
            max_focus_replans: crate::agent_runtime::progress::default_focus_replans(),
            max_turns: 1200,
            max_tool_calls: 12000,
            max_physical_calls: 3600,
            max_read_bytes: 400_000_000,
            max_context_bytes: 2_000_000,
            max_context_tokens: crate::agent_runtime::chat::default_context_tokens(),
            image_token_reserve: crate::agent_runtime::chat::default_image_token_reserve(),
            token_safety_margin: crate::agent_runtime::chat::default_token_safety_margin(),
            max_tool_result_bytes: 48000,
        }
    }
}

impl Limits {
    fn validate(&self) -> Result<(), AgentError> {
        let l = self;
        if !l.progress().validate()
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
        Ok(())
    }

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
        let limits = match std::env::var("KB_EXPORT_REVIEW_LIMITS") {
            Ok(raw) => serde_json::from_str(&raw).map_err(invalid)?,
            Err(std::env::VarError::NotPresent) => Limits::default(),
            Err(error) => return Err(invalid(error)),
        };
        limits.validate()?;
        let config = Self {
            provider: AuthoringRuntimeContractV1::resolve_tools_from_environment()
                .map_err(|e| AgentError::new("AGENT_PROVIDER_UNAVAILABLE", e))?,
            limits,
        };
        config.contract_sha256()?;
        Ok(config)
    }

    pub fn contract_sha256(&self) -> Result<String, AgentError> {
        digest(&self.contract_definition()?).map_err(invalid)
    }

    pub fn contract_definition(&self) -> Result<Value, AgentError> {
        self.provider.validate().map_err(invalid)?;
        self.limits.validate()?;
        if self.provider.response_mode != "tool_calls" {
            return Err(invalid("export review requires tool calls"));
        }
        Ok(json!({
            "checkpoint_contract_version":crate::agent_runtime::CHECKPOINT_CONTRACT_VERSION,
            "runtime_adapter":crate::agent_runtime::RUNTIME_ADAPTER_VERSION,
            "review_evidence_version":3,
            "config":self,
            "reviewer":crate::agent_runtime::chat::system_content(REVIEWER),
            "tools":schemas(),
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemConclusion {
    Pass,
    Findings,
    SourceLimited,
    NotChecked,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemReview {
    pub item_id: String,
    pub conclusion: ItemConclusion,
    #[serde(default)]
    pub finding_ids: Vec<String>,
    pub findings: Vec<String>,
    pub output_ids: Vec<String>,
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
    pub obligations: BTreeMap<String, crate::docx_composition::Reference>,
    pub tender_coverage: Coverage,
    pub output_coverage: OutputCoverage,
    pub pending_delivery: Option<delivery::PendingDelivery>,
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
    pub obligations: BTreeMap<String, crate::docx_composition::Reference>,
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
    source_views: &'a BTreeMap<String, crate::tender_analysis::views::SourceView>,
    images: &'a dyn visual::ImageReader,
    cancel: &'a CancellationToken,
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
        let body = request(
            self.state,
            self.config,
            self.source_views,
            self.images,
            self.cancel,
        )
        .await?;
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
            self.source_views,
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
    /// Parsed once by the product path and restored from its frozen snapshot.
    pub inventory: &'a Inventory,
    pub images: &'a dyn visual::ImageReader,
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
    use sha2::{Digest, Sha256};
    let inventory = files.inventory;
    let obligations = obligation_inventory(&result.analysis).map_err(invalid)?;
    if result.frozen_input_sha256 != digest(input).map_err(invalid)?
        || inventory.docx_sha256 != hex::encode(Sha256::digest(files.docx))
        || inventory.pdf_sha256 != files.pdf.map(|pdf| hex::encode(Sha256::digest(pdf)))
    {
        return Err(AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "output inventory does not identify the actual frozen files",
        ));
    }
    let mut state = match journal.load().await? {
        Some(state) => state,
        None => Checkpoint {
            journal: Default::default(),
            contract_sha256: contract.clone(),
            inventory: inventory.clone(),
            analysis: result.analysis.clone(),
            obligations: obligations.clone(),
            tender_coverage: Coverage::default(),
            output_coverage: OutputCoverage::default(),
            pending_delivery: None,
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
        || digest(&state.inventory).map_err(invalid)? != digest(inventory).map_err(invalid)?
        || digest(&state.analysis).map_err(invalid)? != digest(&result.analysis).map_err(invalid)?
        || digest(&state.obligations).map_err(invalid)? != digest(&obligations).map_err(invalid)?
    {
        return Err(AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "export-review inventory or runtime changed",
        ));
    }
    state.journal.validate(state.turn, "reviewer")?;
    drive(
        &mut RunDriver {
            input,
            config,
            state: &mut state,
            journal,
            model,
            source_views: &result.source_views,
            images: files.images,
            cancel,
        },
        cancel,
    )
    .await?;
    report(input, &state)
}

impl Checkpoint {
    pub fn completed_report(&self, input: &FrozenInput) -> Option<Report> {
        report(input, self).ok()
    }
}

fn report(input: &FrozenInput, state: &Checkpoint) -> Result<Report, AgentError> {
    if !state.done || !complete(input, state) {
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
        } else if state
            .reviews
            .values()
            .any(|r| r.conclusion == ItemConclusion::NotChecked)
        {
            "not_checked"
        } else if state
            .reviews
            .values()
            .any(|r| r.conclusion == ItemConclusion::SourceLimited)
        {
            "reviewed_with_source_limitations"
        } else {
            "reviewed"
        }
        .into(),
        reviews: state.reviews.clone(),
        obligations: state.obligations.clone(),
    })
}

fn obligation_inventory(
    analysis: &Analysis,
) -> Result<BTreeMap<String, crate::docx_composition::Reference>, String> {
    crate::docx_composition::compiler::required_references_for_analysis(analysis)
        .into_iter()
        .map(|reference| Ok((format!("obligation:{}", digest(&reference)?), reference)))
        .collect()
}

async fn request(
    state: &mut Checkpoint,
    config: &Config,
    source_views: &BTreeMap<String, crate::tender_analysis::views::SourceView>,
    images: &dyn visual::ImageReader,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, AgentError> {
    let next =
        state
            .inventory
            .units
            .iter()
            .find(|unit| !state.reviews.contains_key(&unit.id))
            .map(OutputUnit::id_json)
            .or_else(|| {
                state.obligations.iter().find(|(id, _)| !state.reviews.contains_key(*id))
            .map(|(id, reference)| json!({"id":id,"kind":"obligation","reference":reference}))
            });
    let messages = vec![
        json!({"role":"system","content":REVIEWER}),
        json!({"role":"user","content":json!({
            "docx_sha256":state.inventory.docx_sha256,
            "pdf_sha256":state.inventory.pdf_sha256,
            "inventory_total":state.inventory.units.len(),
            "obligation_total":state.obligations.len(),
            "reviewed":state.reviews.len(),
            "next_item":next,
            "output_coverage_units":state.output_coverage.units.len(),
            "tender_coverage_sources":state.tender_coverage.text.len(),
            "turn":state.turn,
        }).to_string()}),
    ];
    let mut image_messages = BTreeMap::new();
    loop {
        let mut current = messages.clone();
        for entry in &state.transcript {
            if let Some(refs) = entry.get("export_view_refs") {
                let refs: Vec<visual::ViewRef> =
                    serde_json::from_value(refs.clone()).map_err(invalid)?;
                for view in refs {
                    if !image_messages.contains_key(&view) {
                        image_messages.insert(
                            view.clone(),
                            visual::message(
                                state,
                                source_views,
                                images,
                                &view,
                                config.limits.max_context_bytes,
                                cancel,
                            )
                            .await?,
                        );
                    }
                    current.push(image_messages[&view].clone());
                }
            } else {
                current.push(entry.clone());
            }
        }
        let body =
            crate::agent_runtime::chat::prepare(&config.provider, current, schemas()).await?;
        let tokens = crate::agent_runtime::chat::estimate_input_tokens(
            &serde_json::from_slice(&body).map_err(invalid)?,
            config.limits.image_token_reserve,
            config.limits.token_safety_margin,
        )?;
        if body.len() <= config.limits.max_context_bytes
            && tokens
                .checked_add(config.provider.max_tokens as usize)
                .is_some_and(|total| total <= config.limits.max_context_tokens)
        {
            return Ok(body);
        }
        if !delivery::evict_history(state) {
            return Err(AgentError::new(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "one export-review evidence group exceeds context budget",
            ));
        }
    }
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
    source_views: &BTreeMap<String, crate::tender_analysis::views::SourceView>,
    cancel: &CancellationToken,
) -> Result<Vec<Value>, AgentError> {
    let l = &config.limits;
    delivery::accept_pending(state, source_views).map_err(invalid)?;
    let previous_reviews = digest(&state.reviews).map_err(invalid)?;
    state.transcript.push(json!({"role":"assistant","content":response.content,"tool_calls":response.tool_calls.iter().map(|c|json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments}})).collect::<Vec<_>>()}));
    let mut tool_results = Vec::new();
    // Reads in this response have not yet been delivered to the reviewer.
    // Keep their receipts pending until the next frozen request is received.
    let mut output_coverage = state.output_coverage.clone();
    let mut tender_coverage = state.tender_coverage.clone();
    let mut delivery_messages = BTreeMap::new();
    let mut view_refs = std::collections::BTreeSet::new();
    for (index, call) in response.tool_calls.into_iter().enumerate() {
        if index > 0 {
            crate::agent_runtime::check_cancel(cancel)?;
        }
        let before_output = output_coverage.clone();
        let before_tender = tender_coverage.clone();
        let mut tool_views = Vec::new();
        let mut image_bytes = 0usize;
        let mut result_value = if state.tool_calls >= l.max_tool_calls
            || state.read_bytes >= l.max_read_bytes
            || state.done
        {
            Err("export-review budget exhausted or already complete".into())
        } else {
            state.tool_calls += 1;
            match serde_json::from_str::<Value>(&call.arguments).map_err(|e| e.to_string()) {
                _ if suppressed.contains_key(&call.id) => {
                    Err("tool unavailable in this role".into())
                }
                Err(e) => Err(e),
                Ok(args) if call.name == "read_output_evidence" => read_output_evidence(
                    &state.inventory,
                    &mut output_coverage,
                    &args,
                    l.max_tool_result_bytes,
                ),
                Ok(args) if call.name == "read_output_view" => {
                    visual::read_output(state, &mut output_coverage, &args).map(
                        |(value, refs, bytes)| {
                            tool_views = refs;
                            image_bytes = bytes;
                            value
                        },
                    )
                }
                Ok(args) if call.name == "read_source_view" => {
                    visual::read_source(input, source_views, &mut tender_coverage, &args).map(
                        |(value, refs, bytes)| {
                            tool_views = refs;
                            image_bytes = bytes;
                            value
                        },
                    )
                }
                Ok(args) if call.name == "put_composition_review" => {
                    put_review(input, state, &args)
                }
                Ok(args) if call.name == "read_review_obligations" => {
                    let offset = args["offset"].as_u64().ok_or("offset required");
                    let limit = args["limit"]
                        .as_u64()
                        .filter(|n| *n > 0)
                        .ok_or("positive limit required");
                    match (offset, limit) {
                        (Ok(offset), Ok(limit)) => {
                            let rows: Vec<_> = state
                                .obligations
                                .iter()
                                .skip(offset as usize)
                                .take(limit as usize)
                                .map(|(id, reference)| json!({"id":id,"reference":reference}))
                                .collect();
                            let page = json!({"total":state.obligations.len(),"next":offset as usize + rows.len(),"items":rows});
                            if page.to_string().len() > l.max_tool_result_bytes {
                                Err("obligation page exceeds budget".into())
                            } else {
                                Ok(page)
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => Err(e.into()),
                    }
                }
                Ok(args) => {
                    let mut coverage = tender_coverage.clone();
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
                        tender_coverage = coverage;
                    }
                    out
                }
            }
        };
        if call.name != "put_composition_review"
            && let Ok(value) = &result_value
        {
            let text_bytes = serde_json::to_vec(value).map_err(invalid)?.len();
            let bytes = text_bytes.saturating_add(image_bytes);
            if text_bytes > l.max_tool_result_bytes
                || bytes > l.max_read_bytes.saturating_sub(state.read_bytes)
            {
                output_coverage = before_output;
                tender_coverage = before_tender;
                result_value =
                    Err("tool/image delivery exceeds result or remaining read budget".into());
            } else {
                state.read_bytes += bytes;
                view_refs.extend(tool_views);
                delivery_messages.insert(
                    call.id.clone(),
                    digest(&json!({"ok":true,"result":value}).to_string()).map_err(invalid)?,
                );
            }
        }
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
    if !view_refs.is_empty() {
        state
            .transcript
            .push(json!({"role":"user","export_view_refs":view_refs}));
    }
    if digest(&output_coverage).map_err(invalid)?
        != digest(&state.output_coverage).map_err(invalid)?
        || digest(&tender_coverage).map_err(invalid)?
            != digest(&state.tender_coverage).map_err(invalid)?
    {
        state.pending_delivery = Some(delivery::PendingDelivery {
            output_coverage,
            tender_coverage,
            messages: delivery_messages,
            view_refs: view_refs.into_iter().collect(),
        });
    }
    let reviews = digest(&state.reviews).map_err(invalid)?;
    state.progress.observe(
        [
            digest(&state.output_coverage).map_err(invalid)?,
            digest(&state.tender_coverage).map_err(invalid)?,
        ],
        (reviews != previous_reviews).then_some(reviews),
        &l.progress(),
    );
    state.done = complete(input, state);
    state.turn += 1;
    Ok(tool_results)
}

fn put_review(input: &FrozenInput, state: &mut Checkpoint, args: &Value) -> Result<Value, String> {
    if args.as_object().is_none_or(|object| {
        object.keys().any(|key| {
            !["item_id", "conclusion", "grounds", "findings", "output_ids"].contains(&key.as_str())
        })
    }) {
        return Err("unknown review fields; issue IDs are assigned by the host".into());
    }
    let item_id = args["item_id"]
        .as_str()
        .ok_or("item_id required")?
        .to_owned();
    let unit = state.inventory.units.iter().find(|unit| unit.id == item_id);
    let artifact_sha256 = if let Some(unit) = unit {
        unit.file_sha256.clone()
    } else if state.obligations.contains_key(&item_id) {
        digest(&state.inventory)?
    } else {
        return Err("unknown output evidence or obligation id".into());
    };
    let conclusion =
        serde_json::from_value(args["conclusion"].clone()).map_err(|e| e.to_string())?;
    let grounds = serde_json::from_value(args["grounds"].clone()).map_err(|e| e.to_string())?;
    let findings = args
        .get("findings")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let output_ids = args
        .get("output_ids")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| unit.map(|u| vec![u.id.clone()]).unwrap_or_default());
    let mut review = ItemReview {
        item_id: item_id.clone(),
        conclusion,
        grounds,
        findings,
        output_ids,
        finding_ids: vec![],
        artifact_sha256,
    };
    review.finding_ids = finding_ids(&review)?;
    validate_review(input, state, &review)?;
    state.reviews.insert(item_id.clone(), review);
    Ok(json!({"saved":true,"item_id":item_id}))
}

fn finding_ids(review: &ItemReview) -> Result<Vec<String>, String> {
    review
        .findings
        .iter()
        .map(|message| {
            digest(&json!({"item_id":review.item_id,
        "artifact_sha256":review.artifact_sha256,"grounds":review.grounds,
        "output_ids":review.output_ids,"message":message}))
        })
        .collect()
}

fn validate_review(
    input: &FrozenInput,
    state: &Checkpoint,
    review: &ItemReview,
) -> Result<(), String> {
    if let Some(reference) = state.obligations.get(&review.item_id) {
        if review.artifact_sha256 != digest(&state.inventory)? {
            return Err("obligation review belongs to another output inventory".into());
        }
        let record = state
            .analysis
            .records
            .get(&reference.record_id)
            .ok_or("unknown obligation record")?;
        if state
            .tender_coverage
            .candidate
            .get(&format!("record:{}", reference.record_id))
            != Some(&digest(record)?)
        {
            return Err("independently inspect the current obligation record before reviewing its implementation".into());
        }
        if review.conclusion == ItemConclusion::Pass && review.output_ids.is_empty() {
            return Err("a passing obligation needs actual output locations".into());
        }
        if review.conclusion == ItemConclusion::NotApplicable {
            use crate::tender_analysis::{ApplicabilityState, RecordData};
            let applicability = match &record.data {
                RecordData::Template { applicability, .. }
                | RecordData::Requirement { applicability, .. }
                | RecordData::Rule { applicability, .. } => Some(applicability),
                _ => None,
            };
            if applicability.is_none_or(|a| a.state != ApplicabilityState::NotApplicable) {
                return Err("not_applicable requires the independently read frozen obligation to be inapplicable".into());
            }
        }
        if review.output_ids.is_empty()
            && review.conclusion != ItemConclusion::NotApplicable
            && state
                .inventory
                .units
                .iter()
                .any(|unit| state.output_coverage.units.get(&unit.id) != Some(&unit.content_sha256))
        {
            return Err(
                "inspect the complete output inventory before concluding an obligation is absent"
                    .into(),
            );
        }
    } else {
        if review.conclusion == ItemConclusion::NotApplicable {
            return Err(
                "actual output content still needs review even if an obligation is inapplicable"
                    .into(),
            );
        }
        let unit = state
            .inventory
            .units
            .iter()
            .find(|unit| unit.id == review.item_id)
            .ok_or("unknown output item")?;
        if review.artifact_sha256 != unit.file_sha256 || review.output_ids != [unit.id.clone()] {
            return Err("output review must identify this exact output unit".into());
        }
    }
    if review
        .output_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != review.output_ids.len()
    {
        return Err("output locations must be distinct".into());
    }
    for id in &review.output_ids {
        let unit = state
            .inventory
            .units
            .iter()
            .find(|unit| &unit.id == id)
            .ok_or("unknown output location")?;
        let expected_file = if unit.id.starts_with("docx:") {
            Some(&state.inventory.docx_sha256)
        } else if unit.id.starts_with("pdf:") {
            state.inventory.pdf_sha256.as_ref()
        } else {
            None
        };
        if expected_file != Some(&unit.file_sha256)
            || !unit
                .id
                .split(':')
                .nth(1)
                .is_some_and(|sha| sha == unit.file_sha256)
        {
            return Err("output unit belongs to another file".into());
        }
        if state.output_coverage.units.get(id) != Some(&unit.content_sha256) {
            return Err("read the current output evidence before reviewing it".into());
        }
        if unit.kind == "not_checked"
            && !visual::reviewed_visual(state, unit)
            && matches!(
                review.conclusion,
                ItemConclusion::Pass | ItemConclusion::SourceLimited
            )
        {
            return Err("unsupported output is not checked, not tender-source uncertainty".into());
        }
    }
    if review.grounds.is_empty() && review.conclusion != ItemConclusion::NotChecked {
        return Err("review conclusion needs tender grounds".into());
    }
    for ground in &review.grounds {
        tools::validate_span(input, &state.tender_coverage, ground)?;
    }
    if (review.conclusion == ItemConclusion::Pass) != review.findings.is_empty()
        || review.findings.iter().any(|text| text.trim().is_empty())
        || review.finding_ids != finding_ids(review)?
    {
        return Err("non-passing conclusions need actual issue or limitation text; saved issue IDs must match its content".into());
    }
    if review.conclusion == ItemConclusion::SourceLimited {
        use crate::tender_analysis::{GlobalCheck, GlobalCheckConclusion, rule_contract};
        let record_ids = state
            .analysis
            .records
            .values()
            .filter(|record| {
                record.sources.iter().any(|span| {
                    review
                        .grounds
                        .iter()
                        .any(|ground| ground.source_id == span.source_id)
                })
            })
            .map(|record| record.id.clone())
            .collect();
        rule_contract::validate_check(
            input,
            &state.analysis,
            &GlobalCheck {
                key: "cross_references".into(),
                scope_sha256: rule_contract::scope_sha256(input, &state.analysis)?,
                conclusion: GlobalCheckConclusion::SourceLimited,
                grounds: review.grounds.clone(),
                record_ids,
                finding_ids: vec![],
            },
            &[],
        )?;
    }
    Ok(())
}

fn complete(input: &FrozenInput, state: &Checkpoint) -> bool {
    state.pending_delivery.is_none()
        && !state.inventory.units.is_empty()
        && obligation_inventory(&state.analysis).ok().as_ref() == Some(&state.obligations)
        && state.reviews.len() == state.inventory.units.len() + state.obligations.len()
        && state.inventory.units.iter().all(|unit| {
            state.reviews.get(&unit.id).is_some_and(|review| {
                review.item_id == unit.id && validate_review(input, state, review).is_ok()
            })
        })
        && state.obligations.keys().all(|id| {
            state.reviews.get(id).is_some_and(|review| {
                &review.item_id == id && validate_review(input, state, review).is_ok()
            })
        })
}

fn invalid(message: impl std::fmt::Display) -> AgentError {
    AgentError::new("AGENT_OUTPUT_INVALID", message.to_string())
}

#[cfg(test)]
mod tests {
    mod obligations;
    mod visual_evidence;
    use super::*;
    use crate::tender_analysis::{Source, Span};
    use serde_json::json;
    use std::{collections::VecDeque, io::Write, sync::Mutex};

    struct NoImages;
    #[async_trait]
    impl visual::ImageReader for NoImages {
        async fn read(
            &self,
            _: &crate::export_review::OutputImage,
            _: &CancellationToken,
        ) -> Result<Vec<u8>, AgentError> {
            panic!("this text-only fixture must not read image objects")
        }
    }

    async fn request(state: &mut Checkpoint, config: &Config) -> Result<Vec<u8>, AgentError> {
        super::request(
            state,
            config,
            &BTreeMap::new(),
            &NoImages,
            &CancellationToken::new(),
        )
        .await
    }

    async fn execute_turn(
        input: &FrozenInput,
        config: &Config,
        state: &mut Checkpoint,
        response: ChatTurn,
        suppressed: BTreeMap<String, String>,
        cancel: &CancellationToken,
    ) -> Result<Vec<Value>, AgentError> {
        super::execute_turn(
            input,
            config,
            state,
            response,
            suppressed,
            &BTreeMap::new(),
            cancel,
        )
        .await
    }

    fn docx() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let document = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>投标函</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#;
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(document.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    /// Domain-loop fixture, not evidence of a real parser or final-file acceptance.
    fn inventory(bytes: &[u8]) -> Inventory {
        use sha2::{Digest, Sha256};
        let sha = hex::encode(Sha256::digest(bytes));
        serde_json::from_value(json!({"docx_sha256":sha,"pdf_sha256":null,"units":[{
            "id":format!("docx:{sha}:0"),"file_sha256":sha,"part":"word/document.xml",
            "ordinal":0,"kind":"paragraphs","content_sha256":digest(&"投标函").unwrap(),
            "text":"投标函","bookmark":null
        }]}))
        .unwrap()
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
        let inventory = inventory(&bytes);
        let id = inventory.units[0].id.clone();
        let model = Script {
            turns: Mutex::new(VecDeque::from([
                ("read_output_evidence", json!({"offset":0,"limit":8})),
                (
                    "read_source",
                    json!({"source_id":"source","start":0,"max_bytes":100}),
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
                images: &NoImages,
                docx: &bytes,
                pdf: None,
                inventory: &inventory,
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

    fn evidence_fixture() -> (FrozenInput, Checkpoint, Value) {
        let input = input();
        let inventory = inventory(&docx());
        let args = json!({"item_id":inventory.units[0].id,"conclusion":"pass",
            "grounds":[Span { source_id:"source".into(), start:0,
                end:input.source_units[0].text.len(), view_id:None, grid_cell:None }]});
        let state = Checkpoint {
            journal: Default::default(),
            contract_sha256: String::new(),
            inventory,
            analysis: analysis_result(&input).analysis,
            obligations: BTreeMap::new(),
            tender_coverage: Coverage::default(),
            output_coverage: OutputCoverage::default(),
            pending_delivery: None,
            reviews: BTreeMap::new(),
            turn: 0,
            tool_calls: 0,
            read_bytes: 0,
            transcript: vec![],
            progress: Progress::default(),
            done: false,
        };
        (input, state, args)
    }

    fn deliver_output(state: &mut Checkpoint) {
        for unit in &state.inventory.units {
            state
                .output_coverage
                .units
                .insert(unit.id.clone(), unit.content_sha256.clone());
        }
    }

    fn deliver_tender(input: &FrozenInput, state: &mut Checkpoint) {
        state
            .tender_coverage
            .text
            .insert("source".into(), vec![(0, input.source_units[0].text.len())]);
    }

    #[test]
    fn review_rejects_unread_output_or_tender_and_stale_output_digest() {
        let (input, mut state, args) = evidence_fixture();
        assert!(
            put_review(&input, &mut state, &args).is_err(),
            "unread output cannot pass"
        );
        deliver_output(&mut state);
        assert!(
            put_review(&input, &mut state, &args).is_err(),
            "unread tender cannot pass"
        );
        deliver_tender(&input, &mut state);
        state
            .output_coverage
            .units
            .insert(state.inventory.units[0].id.clone(), "stale".into());
        assert!(
            put_review(&input, &mut state, &args).is_err(),
            "stale output receipt cannot pass"
        );
    }

    #[test]
    fn review_requires_grounded_valid_source_identity() {
        let (input, mut state, args) = evidence_fixture();
        deliver_output(&mut state);
        deliver_tender(&input, &mut state);
        for grounds in [
            json!([]),
            json!([{"source_id":"unknown","start":0,"end":9}]),
            json!([{"source_id":"source","start":1,"end":9}]),
        ] {
            let mut changed = args.clone();
            changed["grounds"] = grounds;
            assert!(
                put_review(&input, &mut state, &changed).is_err(),
                "invalid grounds: {changed}"
            );
        }
    }

    #[test]
    fn completion_and_cached_report_reject_missing_or_changed_review_evidence() {
        let (input, mut state, args) = evidence_fixture();
        deliver_output(&mut state);
        deliver_tender(&input, &mut state);
        put_review(&input, &mut state, &args).unwrap();
        state.done = true;
        assert!(complete(&input, &state));
        assert!(state.completed_report(&input).is_some());
        let id = state.inventory.units[0].id.clone();
        let valid = state;
        for case in 0..6 {
            let mut state = valid.clone();
            match case {
                0 => state.output_coverage.units.clear(),
                1 => state.tender_coverage = Coverage::default(),
                2 => state.reviews.get_mut(&id).unwrap().artifact_sha256 = "stale".into(),
                3 => state.reviews.get_mut(&id).unwrap().grounds[0].source_id = "unknown".into(),
                4 => state.inventory.units[0].content_sha256 = "changed".into(),
                5 => state.inventory.units[0].file_sha256 = "foreign".into(),
                _ => unreachable!(),
            }
            assert!(
                !complete(&input, &state),
                "review IDs alone must not complete: {case}"
            );
            assert!(
                state.completed_report(&input).is_none(),
                "cached done cannot bypass receipts: {case}"
            );
        }
    }

    #[test]
    fn pdf_review_binds_the_actual_pdf_instead_of_the_docx() {
        let (input, mut state, mut args) = evidence_fixture();
        let pdf_sha = "b".repeat(64);
        let unit = &mut state.inventory.units[0];
        unit.id = format!("pdf:{pdf_sha}:1");
        unit.file_sha256 = pdf_sha.clone();
        unit.part = "pdf:page:1".into();
        unit.kind = "pdf_page".into();
        state.inventory.pdf_sha256 = Some(pdf_sha.clone());
        args["item_id"] = json!(unit.id);
        deliver_output(&mut state);
        deliver_tender(&input, &mut state);
        put_review(&input, &mut state, &args).unwrap();
        assert_eq!(
            state.reviews[args["item_id"].as_str().unwrap()].artifact_sha256,
            pdf_sha
        );
    }

    #[tokio::test]
    async fn resumed_done_checkpoint_cannot_bypass_evidence_validation() {
        let (input, mut state, args) = evidence_fixture();
        deliver_output(&mut state);
        deliver_tender(&input, &mut state);
        put_review(&input, &mut state, &args).unwrap();
        state.done = true;
        state.output_coverage.units.clear();
        let config = config();
        state.contract_sha256 = config.contract_sha256().unwrap();
        let journal = Memory {
            state: Mutex::new(Some(state)),
            calls: Mutex::new(0),
        };
        let model = Script {
            turns: Mutex::new(VecDeque::new()),
        };
        let error = run(
            &input,
            &analysis_result(&input),
            FrozenFiles {
                images: &NoImages,
                docx: &docx(),
                pdf: None,
                inventory: &inventory(&docx()),
            },
            &config,
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "AGENT_OUTPUT_INVALID");
        assert_eq!(*journal.calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn frozen_inventory_cannot_change_under_the_same_file_hashes() {
        let (input, mut state, _) = evidence_fixture();
        let config = config();
        state.contract_sha256 = config.contract_sha256().unwrap();
        let journal = Memory {
            state: Mutex::new(Some(state)),
            calls: Mutex::new(0),
        };
        let model = Script {
            turns: Mutex::new(VecDeque::new()),
        };
        let bytes = docx();
        let mut altered = inventory(&bytes);
        altered.units[0].text = "different parser result under the same file hash".into();
        let error = run(
            &input,
            &analysis_result(&input),
            FrozenFiles {
                images: &NoImages,
                docx: &bytes,
                pdf: None,
                inventory: &altered,
            },
            &config,
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "FROZEN_INPUT_DIGEST_MISMATCH");
        assert_eq!(*journal.calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn supplied_inventory_must_identify_the_actual_file_bytes() {
        let input = input();
        let bytes = docx();
        let mut wrong = inventory(&bytes);
        wrong.docx_sha256 = "a".repeat(64);
        let journal = Memory::default();
        let model = Script {
            turns: Mutex::new(VecDeque::new()),
        };
        let error = run(
            &input,
            &analysis_result(&input),
            FrozenFiles {
                images: &NoImages,
                docx: &bytes,
                pdf: None,
                inventory: &wrong,
            },
            &config(),
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "FROZEN_INPUT_DIGEST_MISMATCH");
        assert!(journal.state.lock().unwrap().is_none());
        assert_eq!(*journal.calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn same_batch_reads_do_not_authorize_unseen_review() {
        let (input, mut state, args) = evidence_fixture();
        let calls = [
            ("read_output_evidence", json!({"offset":0,"limit":8})),
            (
                "read_source",
                json!({"source_id":"source","start":0,"max_bytes":100}),
            ),
            ("put_composition_review", args.clone()),
        ]
        .into_iter()
        .enumerate()
        .map(|(n, (name, args))| knowledge::models::ChatToolCall {
            id: format!("c{n}"),
            name: name.into(),
            arguments: args.to_string(),
        })
        .collect();
        let output = execute_turn(
            &input,
            &config(),
            &mut state,
            ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: calls,
            },
            BTreeMap::new(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(
            output[2]["content"]
                .as_str()
                .unwrap()
                .contains("\"ok\":false")
        );
        assert!(state.reviews.is_empty());
        assert!(state.output_coverage.units.is_empty());
        assert!(state.tender_coverage.text.is_empty());
        assert!(state.pending_delivery.is_some());
        assert!(put_review(&input, &mut state, &args).is_err());
        let body = request(&mut state, &config()).await.unwrap();
        state
            .journal
            .prepare(state.turn, "reviewer", &body)
            .unwrap();
        assert!(delivery::accept_pending(&mut state, &BTreeMap::new()).is_err());
        state
            .journal
            .responded(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: vec![knowledge::models::ChatToolCall {
                    id: "judgment".into(),
                    name: "put_composition_review".into(),
                    arguments: args.to_string(),
                }],
            })
            .unwrap();
        let frozen_body = state.journal.pending.as_ref().unwrap().body.clone();
        let mut changed: Value = serde_json::from_str(&frozen_body).unwrap();
        for message in changed["messages"].as_array_mut().unwrap() {
            if message["role"] == "tool" {
                message["content"] = json!("unrelated result");
            }
        }
        state.journal.pending.as_mut().unwrap().body = changed.to_string();
        assert!(delivery::accept_pending(&mut state, &BTreeMap::new()).is_err());
        assert!(state.pending_delivery.is_some());
        assert!(state.output_coverage.units.is_empty());
        state.journal.pending.as_mut().unwrap().body = frozen_body;
        delivery::accept_pending(&mut state, &BTreeMap::new()).unwrap();
        put_review(&input, &mut state, &args).unwrap();
    }
}
