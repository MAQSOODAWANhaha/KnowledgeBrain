//! Closed, bounded Agent tools. The serializable workspace is the checkpoint;
//! model messages receive metadata/inspection pages, never a whole DOCX/base64.
use super::*;
use super::{
    compiler::{self, Manifest, compile},
    document::RenderedBlock,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy)]
pub struct ToolLimits {
    pub max_result_bytes: usize,
    pub max_docx_bytes: usize,
    pub max_review_rounds: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub docx_base64: String,
    pub manifest: Manifest,
    pub rendered: Vec<RenderedBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionFinding {
    pub message: String,
    pub section_ids: Vec<String>,
    pub record_ids: Vec<String>,
    pub sources: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanReviewConclusion {
    Pass,
    Findings,
    SourceLimited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanReview {
    pub item_id: String,
    pub conclusion: PlanReviewConclusion,
    #[serde(default)]
    pub finding_ids: Vec<String>,
    pub grounds: Vec<Span>,
    pub artifact_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub draft: Draft,
    pub artifact: Option<Artifact>,
    pub reviewing: bool,
    pub done: bool,
    pub findings: Vec<CompositionFinding>,
    pub review_rounds: usize,
    pub source_coverage: Coverage,
    pub review_coverage: Coverage,
    pub inspected: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plan_reviews: BTreeMap<String, PlanReview>,
}

impl Workspace {
    pub fn new(input: &FrozenInput, result: &AnalysisResult) -> Result<Self, String> {
        Ok(Self {
            draft: Draft::new(input, result)?,
            artifact: None,
            reviewing: false,
            done: false,
            findings: vec![],
            review_rounds: 0,
            source_coverage: Coverage::default(),
            review_coverage: Coverage::default(),
            inspected: BTreeMap::new(),
            plan_reviews: BTreeMap::new(),
        })
    }
    /// Apply on a clone and commit only after the bounded response fits. No
    /// unseen page, partial mutation or failed render can become a checkpoint.
    pub fn invoke(
        &mut self,
        input: &FrozenInput,
        result: &AnalysisResult,
        name: &str,
        args: &Value,
        limits: ToolLimits,
    ) -> Result<Value, String> {
        self.draft.validate_basis(input, result)?;
        if self.done {
            return Err("composition is already independently reviewed".into());
        }
        let args = crate::tender_analysis::evidence_refs::expand(input, args)?;
        let mut next = self.clone();
        let out = next.execute(input, result, name, &args, limits)?;
        if serde_json::to_vec(&out).map_err(|e| e.to_string())?.len() > limits.max_result_bytes {
            return Err("composition tool result exceeds budget; request a smaller page".into());
        }
        *self = next;
        Ok(out)
    }

    fn execute(
        &mut self,
        input: &FrozenInput,
        result: &AnalysisResult,
        name: &str,
        args: &Value,
        limits: ToolLimits,
    ) -> Result<Value, String> {
        let ToolLimits {
            max_result_bytes: max_bytes,
            max_docx_bytes,
            max_review_rounds,
        } = limits;
        // Reuse existing source tools, preserving their coverage and byte-range
        // safeguards. Both roles have immutable access to the frozen analysis.
        if matches!(
            name,
            "collection_index"
                | "source_index"
                | "read_source"
                | "read_form"
                | "search_sources"
                | "inspect_analysis"
        ) {
            let mut analysis = result.analysis.clone();
            let coverage = if self.reviewing {
                &mut self.review_coverage
            } else {
                &mut self.source_coverage
            };
            return crate::tender_analysis::tools::invoke(
                input,
                &mut analysis,
                coverage,
                true,
                name,
                args,
                max_bytes,
            );
        }
        match name {
            "read_source_view" => {
                if args.as_object().is_none_or(|o| o.len() != 1) {
                    return Err("only source_id is accepted".into());
                }
                let source = args["source_id"].as_str().ok_or("source_id required")?;
                let (id,view)=result.source_views.iter().find(|(_,v)|v.identity.source_id==source).ok_or("no frozen original view; revise the source analysis before using new visual evidence")?;
                let coverage = if self.reviewing {
                    &mut self.review_coverage
                } else {
                    &mut self.source_coverage
                };
                coverage.views.insert(id.clone(), view.identity.clone());
                Ok(json!({"source_view_id":id,"identity":view.identity}))
            }
            "set_presentation"
            | "put_composition_plan_item"
            | "put_section"
            | "delete_section"
            | "put_omission"
            | "delete_omission"
            | "omit_template_relation"
            | "delete_relation_omission" => {
                if self.reviewing {
                    return Err("independent reviewer cannot mutate composition".into());
                }
                let expected = args["expected_draft_sha256"]
                    .as_str()
                    .ok_or("expected draft digest required")?;
                if expected != digest(&self.draft)? {
                    return Err(
                        "composition changed; inspect the current draft before editing".into(),
                    );
                }
                let mut payload = args.clone();
                payload
                    .as_object_mut()
                    .ok_or("object required")?
                    .remove("expected_draft_sha256");
                let mut id = None;
                match name {
                    "put_composition_plan_item" => {
                        let mut item: PlanItem =
                            serde_json::from_value(payload).map_err(|e| e.to_string())?;
                        if item.id.trim().is_empty() {
                            item.id = uuid::Uuid::new_v4().to_string();
                        }
                        validate_grounds(input, &result.analysis, &item.grounds)?;
                        if item.title.trim().is_empty() {
                            return Err("plan item title required".into());
                        }
                        if item
                            .parent
                            .as_ref()
                            .is_some_and(|parent| {
                                parent == &item.id || !self.draft.plan.contains_key(parent)
                            })
                        {
                            return Err("invalid plan parent".into());
                        }
                        let key = item.id.clone();
                        self.draft.plan.insert(key.clone(), item);
                        id = Some(key);
                    }
                    "set_presentation" => {
                        let p: Presentation =
                            serde_json::from_value(payload).map_err(|e| e.to_string())?;
                        validate_grounds(input, &result.analysis, &p.grounds)?;
                        if p.explanation.trim().is_empty() {
                            return Err("presentation explanation required".into());
                        }
                        self.draft.presentation = Some(p);
                    }
                    "put_section" => {
                        let key = match payload["id"].as_str() {
                            Some(key) if self.draft.sections.contains_key(key) => key.to_owned(),
                            Some(key)
                                if self.draft.plan.get(key).is_some_and(|item| {
                                    item.kind == PlanItemKind::Section
                                }) =>
                            {
                                key.to_owned()
                            }
                            Some(_) => {
                                return Err("unknown section; use null id for creation".into());
                            }
                            None if payload["id"].is_null() && self.draft.plan.is_empty() => {
                                uuid::Uuid::new_v4().to_string()
                            }
                            _ => return Err("invalid section id".into()),
                        };
                        payload["id"] = json!(key);
                        if !self.draft.plan.is_empty()
                            && self.draft.plan.get(&key).is_none_or(|item| {
                                item.kind != PlanItemKind::Section
                            })
                        {
                            return Err(
                                "section id must be an existing chapter plan item".into(),
                            );
                        }
                        let section: Section =
                            serde_json::from_value(payload).map_err(|e| e.to_string())?;
                        validate_grounds(input, &result.analysis, &section.grounds)?;
                        if section.title.trim().is_empty()
                            || section
                                .parent
                                .as_ref()
                                .is_some_and(|p| p == &key || !self.draft.sections.contains_key(p))
                        {
                            return Err("invalid chapter title or parent".into());
                        }
                        validate_section_content(input, result, &section)?;
                        self.draft.sections.insert(key.clone(), section);
                        id = Some(key);
                    }
                    "delete_section" => {
                        let key = only_id(&payload)?;
                        if self
                            .draft
                            .sections
                            .values()
                            .any(|s| s.parent.as_deref() == Some(key))
                        {
                            return Err("move or delete child chapters first".into());
                        }
                        if self.draft.sections.remove(key).is_none() {
                            return Err("unknown chapter".into());
                        }
                    }
                    "put_omission" => {
                        let omission: Omission =
                            serde_json::from_value(payload).map_err(|e| e.to_string())?;
                        validate_reference(input, &result.analysis, &omission.reference)?;
                        validate_grounds(input, &result.analysis, &omission.grounds)?;
                        if omission.reason.trim().is_empty() {
                            return Err("omission requires tender-grounded explanation".into());
                        }
                        let key = reference_key(&omission.reference)?;
                        self.draft.omissions.insert(key.clone(), omission);
                        id = Some(key);
                    }
                    "delete_omission" => {
                        if self.draft.omissions.remove(only_id(&payload)?).is_none() {
                            return Err("unknown omission".into());
                        }
                    }
                    "omit_template_relation" => {
                        let omission: RelationOmission =
                            serde_json::from_value(payload).map_err(|e| e.to_string())?;
                        if omission.reason.trim().is_empty()
                            || result
                                .analysis
                                .relations
                                .get(&omission.relation_id)
                                .is_none_or(|r| r.kind != RelationKind::RequiresTemplate)
                        {
                            return Err(
                                "known prescribed-format relation and explanation required".into(),
                            );
                        }
                        validate_grounds(input, &result.analysis, &omission.grounds)?;
                        id = Some(omission.relation_id.clone());
                        self.draft
                            .relation_omissions
                            .insert(omission.relation_id.clone(), omission);
                    }
                    "delete_relation_omission" => {
                        if self
                            .draft
                            .relation_omissions
                            .remove(only_id(&payload)?)
                            .is_none()
                        {
                            return Err("unknown relation omission".into());
                        }
                    }
                    _ => unreachable!(),
                }
                self.artifact = None;
                // Keep role-local version receipts; review_gaps rejects changed versions.
                Ok(json!({"id":id,"draft_sha256":digest(&self.draft)?,"render_invalidated":true}))
            }
            "inspect_composition" | "inspect_rendered" | "inspect_findings" => {
                let (offset, limit) = page(args)?;
                let rows = match name {
                    "inspect_composition" => self.draft_rows(),
                    "inspect_rendered" => {
                        let a = self
                            .artifact
                            .as_ref()
                            .ok_or("compile the current draft before inspecting DOCX")?;
                        a.rendered
                            .iter()
                            .map(|v| (format!("rendered:{}", v.bookmark), rendered_metadata(v)))
                            .collect()
                    }
                    _ => self
                        .findings
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (format!("finding:{i}"), json!(v)))
                        .collect(),
                };
                if offset > rows.len() {
                    return Err("inspection offset outside collection".into());
                }
                let items: Vec<_> = rows
                    .iter()
                    .skip(offset)
                    .take(limit)
                    .map(|(key, value)| {
                        if self.reviewing {
                            self.inspected.insert(
                                key.clone(),
                                digest(value).expect("serializable inspection"),
                            );
                        }
                        json!({"key":key,"value":value})
                    })
                    .collect();
                Ok(
                    json!({"draft_sha256":digest(&self.draft)?,"total":rows.len(),"next":offset+items.len(),"items":items}),
                )
            }
            "composition_coverage" => {
                let (offset, limit) = page(args)?;
                let required = super::compiler::required_references(result);
                if offset > required.len() {
                    return Err("coverage offset outside collection".into());
                }
                let items:Vec<_>=required.iter().skip(offset).take(limit).map(|r|{
                    let sections:Vec<_>=self.draft.sections.values().filter(|s|s.content.iter().any(|c|match c {
                        Content::Template{record_id,bindings,..} => r.record_id==*record_id && r.target==RelationTarget::Record || bindings.iter().any(|b|b.need==*r),
                        Content::Placeholder{needs}|Content::ResponseTable{needs,..} => needs.contains(r),
                        Content::SourceResponse{need,..} => need == r,
                    })).map(|s|&s.id).collect();
                    json!({"reference":r,"planned_sections":sections,"omission":self.draft.omissions.get(&reference_key(r).expect("serializable reference"))})
                }).collect();
                Ok(json!({"total":required.len(),"next":offset+items.len(),"items":items}))
            }
            "inspect_rendered_cells" => {
                if args.as_object().is_none_or(|o| o.len() != 3) {
                    return Err("only bookmark, offset and limit are accepted".into());
                }
                let bookmark = args["bookmark"].as_str().ok_or("bookmark required")?;
                let (offset, limit) =
                    page(&json!({"offset":args["offset"],"limit":args["limit"]}))?;
                let table = self
                    .artifact
                    .as_ref()
                    .ok_or("compile the draft first")?
                    .rendered
                    .iter()
                    .find(|b| b.bookmark == bookmark)
                    .and_then(|b| b.table.as_ref())
                    .ok_or("unknown rendered table location")?;
                if offset > table.cells.len() {
                    return Err("cell offset outside rendered table".into());
                }
                let cells: Vec<_> = table
                    .cells
                    .iter()
                    .enumerate()
                    .skip(offset)
                    .take(limit)
                    .map(|(i, c)| {
                        if self.reviewing {
                            self.inspected.insert(
                                format!("rendered_cell:{bookmark}:{i}"),
                                digest(c).expect("serializable cell"),
                            );
                        }
                        c
                    })
                    .collect();
                Ok(
                    json!({"bookmark":bookmark,"total":table.cells.len(),"next":offset+cells.len(),"cells":cells}),
                )
            }
            "compile_docx" => {
                if self.reviewing {
                    return Err("reviewer inspects the frozen render, not a replacement".into());
                }
                empty(args)?;
                let compiled = compile(input, result, &self.draft, max_docx_bytes)?;
                let out = json!({"draft_sha256":compiled.manifest.draft_sha256,"docx_sha256":compiled.manifest.docx_sha256,"bytes":compiled.docx.len(),"sections":compiled.manifest.sections.len(),"placements":compiled.manifest.placements.len(),"source_quality":compiled.manifest.source_quality,"source_open_items":compiled.manifest.source_open_items.len(),"status":"needs_review"});
                self.artifact = Some(Artifact {
                    docx_base64: STANDARD.encode(compiled.docx),
                    manifest: compiled.manifest,
                    rendered: compiled.rendered,
                });
                // Keep role-local version receipts; review_gaps rejects changed versions.
                Ok(out)
            }
            "inspect_placements" => {
                let (offset, limit) = page(args)?;
                let a = self
                    .artifact
                    .as_ref()
                    .ok_or("compile the current draft first")?;
                if offset > a.manifest.placements.len() {
                    return Err("placement offset outside collection".into());
                }
                let items: Vec<_> = a
                    .manifest
                    .placements
                    .iter()
                    .enumerate()
                    .skip(offset)
                    .take(limit)
                    .map(|(i, p)| {
                        if self.reviewing {
                            self.inspected.insert(
                                format!("placement:{i}"),
                                digest(p).expect("serializable placement"),
                            );
                        }
                        p
                    })
                    .collect();
                Ok(
                    json!({"total":a.manifest.placements.len(),"next":offset+items.len(),"items":items}),
                )
            }
            "put_composition_review" => {
                if !self.reviewing {
                    return Err("independent review is not active".into());
                }
                let item_id = args["item_id"].as_str().ok_or("plan item id required")?.to_owned();
                if self.draft.plan.is_empty() {
                    if !self.draft.sections.contains_key(&item_id) {
                        return Err("unknown chapter for composition review".into());
                    }
                } else if !self.draft.plan.contains_key(&item_id) {
                    return Err("unknown plan item for composition review".into());
                }
                let conclusion: PlanReviewConclusion =
                    serde_json::from_value(args["conclusion"].clone()).map_err(|e| e.to_string())?;
                let grounds: Vec<Span> =
                    serde_json::from_value(args["grounds"].clone()).map_err(|e| e.to_string())?;
                let finding_ids: Vec<String> = args
                    .get("finding_ids")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| e.to_string())?
                    .unwrap_or_default();
                if matches!(conclusion, PlanReviewConclusion::Findings) && finding_ids.is_empty() {
                    return Err("findings conclusion needs finding ids".into());
                }
                if matches!(conclusion, PlanReviewConclusion::SourceLimited) && grounds.is_empty() {
                    return Err("source_limited conclusion needs grounds".into());
                }
                let artifact_sha256 = self
                    .artifact
                    .as_ref()
                    .ok_or("compile the draft before independent review")?
                    .manifest
                    .docx_sha256
                    .clone();
                self.plan_reviews.insert(
                    item_id.clone(),
                    PlanReview {
                        item_id: item_id.clone(),
                        conclusion,
                        finding_ids,
                        grounds,
                        artifact_sha256,
                    },
                );
                Ok(json!({"saved":true,"item_id":item_id}))
            }
            "request_composition_review" => {
                empty(args)?;
                if self.reviewing || self.review_rounds >= max_review_rounds {
                    return Err("composition review unavailable or budget exhausted".into());
                }
                let a = self
                    .artifact
                    .as_ref()
                    .ok_or("compile the draft before independent review")?;
                if a.manifest.draft_sha256 != digest(&self.draft)? {
                    return Err("render is stale".into());
                }
                self.reviewing = true;
                self.review_rounds += 1;
                // Keep role-local version receipts; review_gaps rejects changed versions.
                Ok(json!({"reviewing_docx_sha256":a.manifest.docx_sha256}))
            }
            "submit_composition_review" => {
                if !self.reviewing {
                    return Err("independent review is not active".into());
                }
                if args
                    .as_object()
                    .is_none_or(|o| o.len() != 1 || !o.contains_key("findings"))
                {
                    return Err("only findings are accepted".into());
                }
                let gaps = self.review_gaps(input, result)?;
                if !gaps.is_empty() {
                    return Err(format!(
                        "{} source/analysis/document inspection gaps remain",
                        gaps.len()
                    ));
                }
                if !self.draft.plan.is_empty() {
                    let missing: Vec<_> = self
                        .draft
                        .plan
                        .keys()
                        .filter(|id| !self.plan_reviews.contains_key(*id))
                        .cloned()
                        .collect();
                    if !missing.is_empty() {
                        return Err(format!(
                            "{} plan items lack composition review", 
                            missing.len()
                        ));
                    }
                }
                let findings: Vec<CompositionFinding> =
                    serde_json::from_value(args["findings"].clone()).map_err(|e| e.to_string())?;
                for f in &findings {
                    if f.message.trim().is_empty()
                        || f.section_ids.is_empty()
                            && f.record_ids.is_empty()
                            && f.sources.is_empty()
                        || f.section_ids
                            .iter()
                            .any(|id| !self.draft.sections.contains_key(id))
                        || f.record_ids
                            .iter()
                            .any(|id| !result.analysis.records.contains_key(id))
                    {
                        return Err(
                            "review finding needs valid affected objects and explanation".into(),
                        );
                    }
                    for span in &f.sources {
                        crate::tender_analysis::tools::validate_span(
                            input,
                            &self.review_coverage,
                            span,
                        )?;
                    }
                }
                self.done = findings.is_empty();
                self.findings = findings;
                self.reviewing = false;
                if self.done {
                    let manifest = &mut self
                        .artifact
                        .as_mut()
                        .ok_or("reviewed artifact missing")?
                        .manifest;
                    manifest.status = manifest.reviewed_status().into();
                }
                Ok(json!({"done":self.done,"findings":self.findings.len()}))
            }
            "check_composition_review" => {
                let (offset, limit) = page(args)?;
                let gaps = self.review_gaps(input, result)?;
                Ok(
                    json!({"total":gaps.len(),"items":gaps.iter().skip(offset).take(limit).collect::<Vec<_>>()}),
                )
            }
            _ => Err("unknown or unavailable composition tool".into()),
        }
    }
    fn draft_rows(&self) -> Vec<(String, Value)> {
        let mut rows = vec![("presentation".into(), json!(self.draft.presentation))];
        rows.extend(
            self.draft
                .relation_omissions
                .iter()
                .map(|(id, v)| (format!("relation_omission:{id}"), json!(v))),
        );
        rows.extend(
            self.draft
                .sections
                .iter()
                .map(|(id, s)| (format!("section:{id}"), json!(s))),
        );
        rows.extend(
            self.draft
                .omissions
                .iter()
                .map(|(id, s)| (format!("omission:{id}"), json!(s))),
        );
        if let Some(artifact) = &self.artifact {
            rows.extend(
                artifact
                    .manifest
                    .source_excerpts
                    .iter()
                    .enumerate()
                    .map(|(i, excerpt)| (format!("source_excerpt:{i}"), json!(excerpt))),
            );
            rows.push((
                "source_report:quality".into(),
                json!(artifact.manifest.source_quality),
            ));
            rows.extend(
                artifact
                    .manifest
                    .source_open_items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| (format!("source_report:item:{i}"), json!(item))),
            );
        }
        rows
    }
    pub fn review_gaps(
        &self,
        input: &FrozenInput,
        result: &AnalysisResult,
    ) -> Result<Vec<Value>, String> {
        let mut gaps = crate::tender_analysis::tools::review_gaps(
            input,
            &result.analysis,
            &self.review_coverage,
        );
        // Originals already reviewed during extraction still require independent
        // composition inspection. Visual replay uses the exact frozen images.
        let mut rows = self.draft_rows();
        if let Some(a) = &self.artifact {
            if a.manifest.draft_sha256 != digest(&self.draft)? {
                gaps.push(json!({"kind":"stale_render"}));
            }
            rows.extend(
                a.rendered
                    .iter()
                    .map(|v| (format!("rendered:{}", v.bookmark), rendered_metadata(v))),
            );
            rows.extend(a.rendered.iter().flat_map(|b| {
                b.table.iter().flat_map(move |t| {
                    t.cells
                        .iter()
                        .enumerate()
                        .map(move |(i, c)| (format!("rendered_cell:{}:{i}", b.bookmark), json!(c)))
                })
            }));
            rows.extend(
                a.manifest
                    .placements
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (format!("placement:{i}"), json!(v))),
            );
        } else {
            gaps.push(json!({"kind":"missing_render"}));
        }
        for (key, value) in rows {
            if self.inspected.get(&key) != Some(&digest(&value)?) {
                gaps.push(json!({"kind":"unreviewed_composition","key":key}));
            }
        }
        Ok(gaps)
    }
}
fn rendered_metadata(block: &RenderedBlock) -> Value {
    let mut value = json!(block);
    if let Some(table) = value["table"].as_object_mut() {
        let count = table
            .remove("cells")
            .and_then(|v| v.as_array().map(Vec::len))
            .unwrap_or(0);
        table.insert("cell_count".into(), json!(count));
    }
    value
}
fn empty(args: &Value) -> Result<(), String> {
    if args.as_object().is_some_and(|o| o.is_empty()) {
        Ok(())
    } else {
        Err("tool takes no arguments".into())
    }
}
fn validate_section_content(
    input: &FrozenInput,
    result: &AnalysisResult,
    section: &Section,
) -> Result<(), String> {
    for content in &section.content {
        match content {
            Content::Template {
                record_id,
                bindings,
                ..
            } => {
                let record = result
                    .analysis
                    .records
                    .get(record_id)
                    .ok_or("unknown template record")?;
                if !matches!(record.data, RecordData::Template { .. }) {
                    return Err("template content requires a template record".into());
                }
                for binding in bindings {
                    compiler::need(input, &result.analysis, &binding.need)?;
                    crate::tender_analysis::relations::validate_target(
                        input,
                        record,
                        &binding.field,
                    )?;
                }
            }
            Content::Placeholder { needs } | Content::ResponseTable { needs, .. } => {
                if needs.is_empty() {
                    return Err("empty bidder work placeholder".into());
                }
                for reference in needs {
                    compiler::need(input, &result.analysis, reference)?;
                }
            }
            Content::SourceResponse { .. } => {}
        }
    }
    Ok(())
}
fn only_id(args: &Value) -> Result<&str, String> {
    if args.as_object().is_none_or(|o| o.len() != 1) {
        return Err("only id is accepted".into());
    }
    args["id"].as_str().ok_or_else(|| "id required".into())
}
fn page(args: &Value) -> Result<(usize, usize), String> {
    if args.as_object().is_none_or(|o| o.len() != 2) {
        return Err("only offset and limit are accepted".into());
    }
    let offset = args["offset"]
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or("offset required")?;
    let limit = args["limit"]
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .filter(|v| *v > 0)
        .ok_or("positive limit required")?;
    Ok((offset, limit))
}
