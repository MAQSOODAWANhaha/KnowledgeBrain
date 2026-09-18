use super::*;
use docparser::{
    OutputInventoryEntry, OutputInventoryManifest, PdfTableCell, StructuredSourceLocator,
    StructuredSourceUnitKind, TableGrid,
};
use serde_json::json;

struct Unit {
    kind: &'static str,
    text: &'static str,
    bookmarks: Vec<&'static str>,
    level: Option<u32>,
    field: Option<&'static str>,
    grid: Option<TableGrid>,
    reason: Option<&'static str>,
}

fn paragraph(text: &'static str) -> Unit {
    Unit {
        kind: "paragraphs",
        text,
        bookmarks: vec![],
        level: None,
        field: None,
        grid: None,
        reason: None,
    }
}

fn heading(level: u32, text: &'static str, bookmarks: Vec<&'static str>) -> Unit {
    Unit {
        level: Some(level),
        bookmarks,
        ..paragraph(text)
    }
}

fn toc(text: &'static str) -> Unit {
    Unit {
        field: Some("toc"),
        ..paragraph(text)
    }
}

fn table(cells: &[(usize, usize, &str)]) -> Unit {
    Unit {
        kind: "table",
        text: "",
        bookmarks: vec![],
        level: None,
        field: None,
        grid: Some(TableGrid {
            row_count: 2,
            column_count: 2,
            widths_mm: None,
            cells: cells
                .iter()
                .map(|(row, column, text)| PdfTableCell {
                    row: *row as u32,
                    column: *column as u32,
                    row_span: 1,
                    col_span: 1,
                    text: (*text).into(),
                })
                .collect(),
        }),
        reason: None,
    }
}

fn image() -> Unit {
    Unit {
        kind: "image",
        reason: Some("image requires visual review"),
        ..paragraph("")
    }
}

fn inventory(units: Vec<Unit>) -> (OutputInventoryManifest, Vec<StructuredSourceUnit>) {
    let mut entries = Vec::new();
    let mut parsed = Vec::new();
    for (ordinal, unit) in units.into_iter().enumerate() {
        let key = format!("story:{ordinal}");
        entries.push(OutputInventoryEntry {
            unit_key: key.clone(),
            part: "/word/document.xml".into(),
            ordinal,
            kind: unit.kind.into(),
            bookmarks: unit.bookmarks.iter().map(|s| (*s).to_string()).collect(),
            fields: vec![],
            heading_level: unit.level,
            field_region: unit.field.map(str::to_string),
            status: if unit.reason.is_some() {
                "not_checked".into()
            } else {
                "extracted".into()
            },
            reason: unit.reason.map(str::to_string),
        });
        parsed.push(StructuredSourceUnit {
            key,
            ordinal: ordinal as u32,
            kind: if unit.grid.is_some() {
                StructuredSourceUnitKind::TableRegion
            } else {
                StructuredSourceUnitKind::Section
            },
            text: unit.text.into(),
            locator: StructuredSourceLocator::Document {
                section_ordinal: ordinal as u32,
                table_ordinal: None,
                row_ordinal: None,
                form_ordinal: None,
                heading_path: String::new(),
            },
            grid: unit.grid,
        });
    }
    let manifest = OutputInventoryManifest {
        schema_version: 1,
        profile: docparser::OUTPUT_INVENTORY_PROFILE.into(),
        file_sha256: "a".repeat(64),
        parser: "docreader-output-inventory-v1/test".into(),
        config: json!({"profile":docparser::OUTPUT_INVENTORY_PROFILE}),
        page_count: None,
        image_sha256: Default::default(),
        units: entries,
    };
    (manifest, parsed)
}

fn sections() -> Vec<String> {
    vec!["vol-biz".into(), "ch-letter".into(), "ch-appendix".into()]
}

fn prior_titles() -> Vec<(String, String)> {
    vec![
        ("vol-biz".into(), "第一册 商务文件".into()),
        ("ch-letter".into(), "一、投标函".into()),
        ("ch-appendix".into(), "1. 投标函附录".into()),
    ]
}

fn read(units: Vec<Unit>) -> ReadbackDocument {
    let (manifest, parsed) = inventory(units);
    read_chapters(&manifest, &parsed, &sections(), &prior_titles()).unwrap()
}

/// 首次填充读的就是没进过编辑器的编译产物：块级书签必须能认领章。
#[test]
fn compiled_skeleton_claims_every_chapter_by_bookmark() {
    let document = read(vec![
        paragraph("投标文件（草稿骨架）"),
        paragraph("目录"),
        toc("第一册 商务文件"),
        toc("一、投标函"),
        heading(1, "第一册 商务文件", vec!["kb_s0"]),
        paragraph(""),
        heading(2, "一、投标函", vec!["kb_s1"]),
        paragraph(""),
    ]);
    let ids: Vec<_> = document
        .chapters
        .iter()
        .map(|chapter| (chapter.id.as_str(), chapter.claim, chapter.has_body()))
        .collect();
    assert_eq!(
        ids,
        vec![
            ("vol-biz", Claim::Bookmark, false),
            ("ch-letter", Claim::Bookmark, false),
        ]
    );
    assert_eq!(document.chapters[1].parent.as_deref(), Some("vol-biz"));
    assert_eq!(pending_chapters(&document).len(), 2, "空章全部待填");
}

/// 目录条目与章标题逐字相同：认成章就会每章读两遍。
#[test]
fn table_of_contents_entries_are_not_chapters() {
    let document = read(vec![
        toc("第一册 商务文件"),
        toc("一、投标函"),
        heading(1, "第一册 商务文件", vec!["kb_s0"]),
    ]);
    assert_eq!(document.chapters.len(), 1);
}

/// 整行重写标题会删掉该章 `kb_sN`：改名章只能靠标题匹配兜底，认不到才算新增。
#[test]
fn renamed_chapter_falls_back_to_title_match_and_new_chapter_is_new() {
    let document = read(vec![
        heading(1, "第一册 商务文件", vec!["kb_s0"]),
        heading(2, "一、投标函（用户改名）", vec![]),
        heading(2, "三、用户新增章", vec![]),
    ]);
    let claims: Vec<_> = document
        .chapters
        .iter()
        .map(|chapter| (chapter.id.clone(), chapter.claim))
        .collect();
    assert_eq!(claims[0], ("vol-biz".to_string(), Claim::Bookmark));
    assert_eq!(claims[1], ("ch-letter".to_string(), Claim::Title));
    assert_eq!(claims[2].1, Claim::New);
    assert!(claims[2].0.starts_with("readback-"), "{claims:?}");
}

/// 删章会留下孤儿块书签：章的存在性只由 heading 段落判定，孤儿不得复活已删章。
#[test]
fn orphan_block_bookmarks_do_not_resurrect_a_deleted_chapter() {
    let document = read(vec![
        heading(1, "第一册 商务文件", vec!["kb_s0"]),
        Unit {
            bookmarks: vec!["kb_s2_b0"],
            ..paragraph("")
        },
        heading(2, "一、投标函", vec!["kb_s1"]),
    ]);
    let ids: Vec<_> = document
        .chapters
        .iter()
        .map(|chapter| chapter.id.clone())
        .collect();
    assert_eq!(ids, vec!["vol-biz", "ch-letter"], "已删的附录章不得回来");
}

/// 已有正文的章不重填，无论正文来自用户手写还是上一轮 AI 填充；表格原样保留。
#[test]
fn chapters_with_body_are_preserved_and_tables_keep_their_grid() {
    let document = read(vec![
        heading(1, "第一册 商务文件", vec!["kb_s0"]),
        paragraph("我方郑重承诺如下。"),
        paragraph("第二段。"),
        table(&[
            (0, 0, "项目"),
            (0, 1, "报价"),
            (1, 0, "总价"),
            (1, 1, "壹万元"),
        ]),
        heading(2, "一、投标函", vec!["kb_s1"]),
        paragraph(""),
    ]);
    let volume = &document.chapters[0];
    assert!(volume.has_body());
    assert_eq!(
        volume.body[0],
        Preserved::Paragraphs {
            unit_keys: vec!["story:1".into(), "story:2".into()],
            paragraphs: vec!["我方郑重承诺如下。".into(), "第二段。".into()],
        }
    );
    match &volume.body[1] {
        Preserved::Table {
            row_count,
            column_count,
            cells,
            ..
        } => {
            assert_eq!((*row_count, *column_count), (2, 2));
            assert_eq!(cells[3].text, "壹万元", "用户手填的报价必须原样回去");
        }
        other => panic!("{other:?}"),
    }
    let pending: Vec<_> = pending_chapters(&document)
        .iter()
        .map(|chapter| chapter.id.clone())
        .collect();
    assert_eq!(pending, vec!["ch-letter"], "只有空章进待填");
}

/// 图片与域这类看不懂的载体列进报告，不假装没有，也不当成正文。
#[test]
fn not_checked_carriers_are_reported_and_never_become_body() {
    let document = read(vec![heading(1, "第一册 商务文件", vec!["kb_s0"]), image()]);
    assert!(!document.chapters[0].has_body());
    assert_eq!(document.not_checked, vec!["image requires visual review"]);
}

#[test]
fn readback_rejects_a_foreign_receipt_or_a_document_without_headings() {
    let (mut manifest, parsed) = inventory(vec![paragraph("只有正文")]);
    let err = read_chapters(&manifest, &parsed, &sections(), &prior_titles()).unwrap_err();
    assert!(err.contains("no heading paragraph"), "{err}");
    manifest.units.clear();
    let err = read_chapters(&manifest, &parsed, &sections(), &prior_titles()).unwrap_err();
    assert!(err.contains("cover every parsed unit"), "{err}");
    let (mut manifest, parsed) = inventory(vec![heading(1, "第一册 商务文件", vec![])]);
    manifest.profile = "other".into();
    let err = read_chapters(&manifest, &parsed, &sections(), &prior_titles()).unwrap_err();
    assert!(err.contains("output inventory profile"), "{err}");
}

/// 编译收据的 key 与块引用一致：`preserved_key` 取段落组首个单元或表格单元。
#[test]
fn preserved_units_receipt_keys_match_the_block_references() {
    let document = read(vec![
        heading(1, "第一册 商务文件", vec!["kb_s0"]),
        paragraph("我方郑重承诺如下。"),
        table(&[
            (0, 0, "项目"),
            (0, 1, "报价"),
            (1, 0, "总价"),
            (1, 1, "壹万元"),
        ]),
    ]);
    let units = preserved_units(document.chapters.iter().flat_map(|chapter| &chapter.body));
    assert_eq!(units["story:1"]["kind"], "paragraphs");
    assert_eq!(units["story:1"]["paragraphs"][0], "我方郑重承诺如下。");
    assert_eq!(units["story:2"]["kind"], "table");
    assert_eq!(units["story:2"]["cells"][3]["text"], "壹万元");
}
