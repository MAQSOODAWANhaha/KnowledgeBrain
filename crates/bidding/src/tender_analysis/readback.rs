//! 回读当前 Word：填充依据是**用户保存的结构**，不是阶段一的 `draft_plan`。
//!
//! 用户可能改了标题、加删了章、调了顺序。每次触发填充先把当前 DOCX 交给 Python
//! docreader（`output_inventory=v1`）解析，再在这里按「书签 → 标题 → 新章」逐章
//! 判定身份。Rust 里不写 DOCX 解析器（`docx_composition/document.rs` 的约定）。
use docparser::{OutputInventoryManifest, StructuredSourceUnit};
use serde::{Deserialize, Serialize};

/// 章的身份来源。同一份文档里三种章共存，不是「书签存活就全走书签」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Claim {
    /// 编译器埋的 `kb_s{N}` 还在，直接认领。
    Bookmark,
    /// 标题被整行重写会删掉 `kb_sN`，只能靠标题匹配兜底。
    Title,
    /// 用户新增章。
    New,
}

/// 用户已有正文，原样回去。图片与自定义样式不保留（plan 原语无法表达）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Preserved {
    Paragraphs {
        unit_keys: Vec<String>,
        paragraphs: Vec<String>,
    },
    Table {
        unit_key: String,
        row_count: usize,
        column_count: usize,
        cells: Vec<PreservedCell>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreservedCell {
    pub row: usize,
    pub column: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadChapter {
    /// 认领到的阶段一章 id，或新分配的 id。
    pub id: String,
    pub claim: Claim,
    pub parent: Option<String>,
    pub order: usize,
    pub level: u32,
    pub title: String,
    /// 该章已有正文。空表示待填。
    pub body: Vec<Preserved>,
}

impl ReadChapter {
    /// 已有正文的章不重填，无论正文来自用户手写还是上一轮 AI 填充。
    pub fn has_body(&self) -> bool {
        !self.body.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadbackDocument {
    pub file_sha256: String,
    pub chapters: Vec<ReadChapter>,
    /// 回读时看不懂的载体（图片、域、几何异常表）。列出来而不是假装没有。
    pub not_checked: Vec<String>,
}

fn folded(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn titles_match(left: &str, right: &str) -> bool {
    let left = folded(left);
    let right = folded(right);
    !left.is_empty() && !right.is_empty() && (left.contains(&right) || right.contains(&left))
}

/// `kb_s{N}` → 阶段一章序号。`kb_s{N}_b{M}` 是块书签，不代表章的存在。
fn section_bookmark(name: &str) -> Option<usize> {
    name.strip_prefix("kb_s")?
        .split('_')
        .next()
        .filter(|digits| !digits.is_empty())
        .and_then(|digits| digits.parse().ok())
        .filter(|_| !name.contains("_b") && !name.contains("_r"))
}

/// 以回读结构为准重建章节树。`sections` 是阶段一编译时的章序（序号 → 计划 id），
/// 用来把 `kb_sN` 映射回原章；`prior_titles` 用于标题匹配兜底。
pub fn read_chapters(
    manifest: &OutputInventoryManifest,
    units: &[StructuredSourceUnit],
    sections: &[String],
    prior_titles: &[(String, String)],
) -> Result<ReadbackDocument, String> {
    if manifest.profile != docparser::OUTPUT_INVENTORY_PROFILE || manifest.schema_version != 1 {
        return Err("readback requires the output inventory profile".into());
    }
    if manifest.units.len() != units.len() {
        return Err("readback receipt does not cover every parsed unit".into());
    }
    let mut chapters: Vec<ReadChapter> = Vec::new();
    let mut not_checked = Vec::new();
    // 层级栈：父子关系取回读的标题级别，不取阶段一的树。
    let mut open: Vec<(u32, String)> = Vec::new();
    let mut claimed: Vec<String> = Vec::new();
    let mut pending_paragraphs: Vec<(String, String)> = Vec::new();
    for (entry, unit) in manifest.units.iter().zip(units) {
        if entry.unit_key != unit.key {
            return Err("readback receipt unit key does not match the parsed unit".into());
        }
        if entry.status == "not_checked" {
            if let Some(reason) = &entry.reason {
                not_checked.push(reason.clone());
            }
            continue;
        }
        // 目录域条目与章标题逐字相同，认成章就会每章读两遍。
        if entry.field_region.as_deref() == Some("toc") {
            continue;
        }
        if entry.field_region.as_deref() == Some("field") {
            not_checked.push(format!("{}: ordinary fields cannot be preserved by template recompilation", entry.unit_key));
        }
        if !matches!(entry.kind.as_str(), "paragraphs" | "table") {
            not_checked.push(format!("{}: unsupported {} carrier", entry.unit_key, entry.kind));
        }
        let heading = entry
            .heading_level
            .filter(|_| entry.kind == "paragraphs" && !unit.text.trim().is_empty());
        if let Some(level) = heading {
            flush_paragraphs(&mut chapters, &mut pending_paragraphs);
            while open
                .last()
                .is_some_and(|(open_level, _)| *open_level >= level)
            {
                open.pop();
            }
            let title = unit.text.trim().to_string();
            let (id, claim) = claim_identity(entry, &title, sections, prior_titles, &claimed);
            claimed.push(id.clone());
            let parent = open.last().map(|(_, id)| id.clone());
            let order = chapters
                .iter()
                .filter(|chapter| chapter.parent == parent)
                .count();
            open.push((level, id.clone()));
            chapters.push(ReadChapter {
                id,
                claim,
                parent,
                order,
                level,
                title,
                body: Vec::new(),
            });
            continue;
        }
        if chapters.is_empty() {
            // 首个标题之前的内容是封面/目录标题，不属于任何章。
            continue;
        }
        match (entry.kind.as_str(), &unit.grid) {
            ("paragraphs", _) if !unit.text.trim().is_empty() => {
                pending_paragraphs.push((unit.key.clone(), unit.text.clone()));
            }
            ("table", Some(grid)) => {
                flush_paragraphs(&mut chapters, &mut pending_paragraphs);
                let chapter = chapters.last_mut().expect("open chapter");
                chapter.body.push(Preserved::Table {
                    unit_key: unit.key.clone(),
                    row_count: grid.row_count as usize,
                    column_count: grid.column_count as usize,
                    cells: grid
                        .cells
                        .iter()
                        .map(|cell| PreservedCell {
                            row: cell.row as usize,
                            column: cell.column as usize,
                            row_span: cell.row_span.max(1) as usize,
                            col_span: cell.col_span.max(1) as usize,
                            text: cell.text.clone(),
                        })
                        .collect(),
                });
            }
            // 空段落是骨架里的留白，不算正文；section 属性也不是内容。
            _ => {}
        }
    }
    flush_paragraphs(&mut chapters, &mut pending_paragraphs);
    if chapters.is_empty() {
        return Err(
            "readback found no heading paragraph; the saved document has no chapter".into(),
        );
    }
    Ok(ReadbackDocument {
        file_sha256: manifest.file_sha256.clone(),
        chapters,
        not_checked,
    })
}

fn flush_paragraphs(chapters: &mut [ReadChapter], pending: &mut Vec<(String, String)>) {
    if pending.is_empty() {
        return;
    }
    let taken = std::mem::take(pending);
    if let Some(chapter) = chapters.last_mut() {
        chapter.body.push(Preserved::Paragraphs {
            unit_keys: taken.iter().map(|(key, _)| key.clone()).collect(),
            paragraphs: taken.into_iter().map(|(_, text)| text).collect(),
        });
    }
}

fn claim_identity(
    entry: &docparser::OutputInventoryEntry,
    title: &str,
    sections: &[String],
    prior_titles: &[(String, String)],
    claimed: &[String],
) -> (String, Claim) {
    let bookmarked = entry
        .bookmarks
        .iter()
        .filter_map(|name| section_bookmark(name))
        .filter_map(|ordinal| sections.get(ordinal))
        .find(|id| !claimed.contains(id));
    if let Some(id) = bookmarked {
        return (id.clone(), Claim::Bookmark);
    }
    // 整行重写标题会删掉该章 kb_sN，改名章在字节上与新增章无法区分。
    let matched = prior_titles
        .iter()
        .find(|(id, prior)| !claimed.contains(id) && titles_match(prior, title));
    if let Some((id, _)) = matched {
        return (id.clone(), Claim::Title);
    }
    let mut ordinal = claimed.len();
    let mut id = format!("readback-{ordinal}");
    while claimed.contains(&id) || sections.contains(&id) {
        ordinal += 1;
        id = format!("readback-{ordinal}");
    }
    (id, Claim::New)
}

/// 取当前已保存 DOCX 的结构。收据绑 `file_sha256`，由 `read_output_inventory`
/// 对着刚发出的字节校验，sha 不符或单元 key 孤立一律拒绝。
pub async fn read_saved_docx(
    bytes: Vec<u8>,
    sections: &[String],
    prior_titles: &[(String, String)],
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<ReadbackDocument, String> {
    let read = docparser::read_output_inventory("output.docx", bytes, cancel)
        .await
        .map_err(|error| error.to_string())?;
    read_chapters(
        &read.manifest,
        &read.parsed.structured_source_units,
        sections,
        prior_titles,
    )
}

/// 编译时投给 `compile_template` 的保留内容收据：`preserved_key` → 单元内容。
pub fn preserved_units<'a>(blocks: impl IntoIterator<Item = &'a Preserved>) -> serde_json::Value {
    let mut units = serde_json::Map::new();
    for block in blocks {
        let key = match block {
            Preserved::Paragraphs { unit_keys, .. } => unit_keys.first().cloned(),
            Preserved::Table { unit_key, .. } => Some(unit_key.clone()),
        };
        if let Some(key) = key {
            units.insert(key, serde_json::json!(block));
        }
    }
    serde_json::Value::Object(units)
}

/// 回读结构 → 填充 run 的计划种子。**以用户保存的文档为准**：用户删掉的章不再
/// 出现，改过的标题按新标题走，新增章（标题样式）当待填章。
///
/// 已有正文的章标 `Filled` 并带上 `preserved`，编译时原样重排，模型看不到也改
/// 不动；正文为空的章标 `Pending` 交给填充回路。绑源不在这里做：`assign_next_chapter`
/// 每次派章前都会按标题重绑，新增章因此不需要种子带窗。
pub fn seed_plan(
    document: &ReadbackDocument,
    prior: &[crate::tender_analysis::draft::DraftPlanItem],
) -> Vec<crate::tender_analysis::draft::DraftPlanItem> {
    use crate::tender_analysis::draft::{BodyStatus, ChapterPurpose, DraftPlanItem, DraftStatus};
    document
        .chapters
        .iter()
        .map(|chapter| {
            let previous = prior.iter().find(|item| item.id == chapter.id && item.title == chapter.title && item.parent == chapter.parent);
            DraftPlanItem {
            grounds: previous.map(|item| item.grounds.clone()).unwrap_or_default(),
            requirement_ids: previous.map(|item| item.requirement_ids.clone()).unwrap_or_default(),
                id: chapter.id.clone(),
                parent: chapter.parent.clone(),
                order: chapter.order,
                title: chapter.title.clone(),
                prescribed: previous.is_some_and(|item| item.prescribed),
                source_ids: previous
                    .map(|item| item.source_ids.clone())
                    .unwrap_or_default(),
                windows: previous
                    .map(|item| item.windows.clone())
                    .unwrap_or_default(),
                window_index: 0,
                template_id: None,
                status: match chapter.has_body() {
                    true => DraftStatus::Filled,
                    false => DraftStatus::Pending,
                },
                purpose: ChapterPurpose::Response,
                format_refs: previous.map(|item| item.format_refs.clone()).unwrap_or_default(),
                body_status: if chapter.has_body() { BodyStatus::User } else { BodyStatus::Empty },
                omit_reason: None,
                preserved: chapter.body.clone(),
            }
        })
        .collect()
}

/// 阶段一编译时的章序（`kb_sN` 的 N → 计划 id）与标题表，喂给 `read_chapters`
/// 做身份认领。章序必须与编译器排章一致，否则书签会认到别的章上。
/// 排好序的章 id，加上「章 id → 标题」对照：书签认不出的章靠标题认。
pub type ClaimBasis = (Vec<String>, Vec<(String, String)>);

pub fn claim_basis(
    input: &crate::tender_analysis::FrozenInput,
    result: &crate::tender_analysis::AnalysisResult,
) -> Result<ClaimBasis, String> {
    let draft = crate::docx_composition::synthesize_draft_document(input, result)?;
    let sections = crate::docx_composition::compiler::ordered_section_ids(&draft)?;
    let titles = result
        .analysis
        .draft_plan
        .iter()
        .map(|item| (item.id.clone(), item.title.clone()))
        .collect();
    Ok((sections, titles))
}

/// 回读结构里还需要填的章：正文为空。绑源与派章由宿主在填充 run 里做。
pub fn pending_chapters(document: &ReadbackDocument) -> Vec<&ReadChapter> {
    document
        .chapters
        .iter()
        .filter(|chapter| !chapter.has_body())
        .collect()
}

#[cfg(test)]
mod tests;
