//! Independent final-file evidence for export review (§13.2).
//! Scan the actual OOXML parts. Generation bookmarks are matching aids only.
pub mod agent;
pub mod postgres;
pub mod runtime;
use crate::tender_analysis::digest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrozenContext {
    pub analysis_identity: Option<Value>,
    pub execution_contract: Option<Value>,
    pub layout_result: Option<Value>,
}

impl FrozenContext {
    pub fn allows_semantic_export_review(&self) -> bool {
        self.analysis_identity.as_ref().is_some_and(|identity| {
            identity.get("schema_version") == Some(&json!(2))
        })
    }
}
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputCoverage {
    pub units: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputUnit {
    pub id: String,
    pub file_sha256: String,
    pub part: String,
    pub ordinal: usize,
    pub kind: String,
    pub content_sha256: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub docx_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_sha256: Option<String>,
    pub units: Vec<OutputUnit>,
}

pub fn schemas() -> Vec<Value> {
    serde_json::from_str(include_str!("../../schemas/export-review-tools-v1.schema.json"))
        .expect("checked export-review tool schemas")
}

pub fn inventory_from_docx(bytes: &[u8], pdf_sha256: Option<String>) -> Result<Inventory, String> {
    let docx_sha256 = hex::encode(Sha256::digest(bytes));
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).map(|f| f.name().to_owned()))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    names.sort();
    let mut units = Vec::new();
    for name in names {
        if !is_content_part(&name) {
            continue;
        }
        let mut xml = String::new();
        zip.by_name(&name)
            .map_err(|e| e.to_string())?
            .read_to_string(&mut xml)
            .map_err(|e| e.to_string())?;
        let doc = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
        for node in doc.descendants().filter(|n| n.is_element()) {
            let Some(kind) = unit_kind(node) else {
                continue;
            };
            if nested_in_cell_or_textbox(node) && kind != "not_checked" {
                continue;
            }
            let text = node_text(node);
            let bookmark = bookmark_name(node);
            let ordinal = units.len();
            let content_sha256 = digest(&json!({
                "part":name,"kind":kind,"ordinal":ordinal,"text":text,"bookmark":bookmark
            }))?;
            units.push(OutputUnit {
                id: format!("docx:{docx_sha256}:{ordinal}"),
                file_sha256: docx_sha256.clone(),
                part: name.clone(),
                ordinal,
                kind: kind.into(),
                content_sha256,
                text,
                bookmark,
            });
        }
    }
    Ok(Inventory {
        docx_sha256,
        pdf_sha256,
        units,
    })
}

pub fn attach_pdf_pages(inventory: &mut Inventory, pdf: &[u8]) -> Result<(), String> {
    let pdf_sha256 = hex::encode(Sha256::digest(pdf));
    if inventory
        .pdf_sha256
        .as_ref()
        .is_some_and(|expected| expected != &pdf_sha256)
    {
        return Err("pdf digest does not match inventory".into());
    }
    inventory.pdf_sha256 = Some(pdf_sha256.clone());
    let document = lopdf::Document::load_mem(pdf).map_err(|e| e.to_string())?;
    let pages = document.get_pages();
    if pages.is_empty() {
        return Err("pdf has no pages".into());
    }
    for (number, _) in pages {
        let text = document.extract_text(&[number]).unwrap_or_default();
        let ordinal = inventory.units.len();
        let part = format!("pdf:page:{number}");
        let content_sha256 = digest(&json!({
            "part":part,"kind":"pdf_page","ordinal":ordinal,"text":text
        }))?;
        inventory.units.push(OutputUnit {
            id: format!("pdf:{pdf_sha256}:{number}"),
            file_sha256: pdf_sha256.clone(),
            part,
            ordinal,
            kind: "pdf_page".into(),
            content_sha256,
            text,
            bookmark: None,
        });
    }
    Ok(())
}

pub fn extra_units_not_covered_by_bookmarks<'a>(
    inventory: &'a Inventory,
    bookmarks: &[String],
) -> Vec<&'a OutputUnit> {
    inventory
        .units
        .iter()
        .filter(|unit| {
            matches!(unit.kind.as_str(), "paragraphs" | "table")
                && !unit
                    .bookmark
                    .as_ref()
                    .is_some_and(|bookmark| bookmarks.iter().any(|known| known == bookmark))
        })
        .collect()
}

pub fn inventory_from_files(docx: &[u8], pdf: Option<&[u8]>) -> Result<Inventory, String> {
    let pdf_sha256 = pdf.map(|bytes| hex::encode(Sha256::digest(bytes)));
    let mut inventory = inventory_from_docx(docx, pdf_sha256)?;
    if let Some(pdf) = pdf {
        attach_pdf_pages(&mut inventory, pdf)?;
    }
    Ok(inventory)
}

pub fn read_output_evidence(
    inventory: &Inventory,
    coverage: &mut OutputCoverage,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    let offset = args["offset"].as_u64().ok_or("offset required")? as usize;
    let limit = args["limit"].as_u64().ok_or("limit required")? as usize;
    if limit == 0 {
        return Err("limit must be positive".into());
    }
    let id = args["id"].as_str();
    let items: Vec<&OutputUnit> = if let Some(id) = id {
        inventory
            .units
            .iter()
            .filter(|unit| unit.id == id)
            .collect()
    } else {
        inventory.units.iter().skip(offset).take(limit).collect()
    };
    if id.is_some() && items.is_empty() {
        return Err("unknown output evidence id".into());
    }
    let page = json!({
        "docx_sha256":inventory.docx_sha256,
        "pdf_sha256":inventory.pdf_sha256,
        "total":inventory.units.len(),
        "next":offset.saturating_add(items.len()),
        "items":items,
    });
    if serde_json::to_vec(&page).map_err(|e| e.to_string())?.len() > max_bytes {
        return Err("output evidence page exceeds budget".into());
    }
    for unit in items {
        coverage
            .units
            .insert(unit.id.clone(), unit.content_sha256.clone());
    }
    Ok(page)
}

fn bookmark_name(node: roxmltree::Node<'_, '_>) -> Option<String> {
    node.descendants()
        .find(|child| child.has_tag_name((W, "bookmarkStart")))
        .and_then(|child| child.attribute((W, "name")).map(str::to_owned))
        .filter(|name| !name.is_empty() && !name.starts_with('_'))
}

fn is_content_part(name: &str) -> bool {
    let name = name.trim_start_matches('/');
    name.starts_with("word/")
        && name.ends_with(".xml")
        && !name.contains("_rels")
        && !matches!(
            name,
            "word/styles.xml"
                | "word/settings.xml"
                | "word/webSettings.xml"
                | "word/numbering.xml"
                | "word/fontTable.xml"
                | "word/comments.xml"
        )
        && !name.starts_with("word/theme/")
}

fn unit_kind(node: roxmltree::Node<'_, '_>) -> Option<&'static str> {
    if node.has_tag_name((W, "drawing")) || node.has_tag_name((W, "txbxContent")) {
        Some("not_checked")
    } else if node.has_tag_name((W, "tbl")) {
        Some("table")
    } else if node.has_tag_name((W, "p")) {
        Some("paragraphs")
    } else {
        None
    }
}

fn nested_in_cell_or_textbox(node: roxmltree::Node<'_, '_>) -> bool {
    node.ancestors().skip(1).any(|parent| {
        parent.has_tag_name((W, "tc"))
            || parent.has_tag_name((W, "txbxContent"))
            || parent.has_tag_name((W, "drawing"))
    })
}

fn node_text(node: roxmltree::Node<'_, '_>) -> String {
    node.descendants()
        .filter(|n| n.is_element() && n.has_tag_name((W, "t")))
        .filter_map(|n| n.text().map(str::to_owned))
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn paragraph(text: &str) -> String {
        format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
    }

    fn docx_with(body: &str, extra_parts: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let document = format!(
            r#"<w:document xmlns:w="{W}"><w:body>{body}<w:sectPr/></w:body></w:document>"#
        );
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(document.as_bytes()).unwrap();
        for (name, xml) in extra_parts {
            zip.start_file(*name, options).unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn inventory_includes_unbookmarked_paragraphs_and_header_parts() {
        let header = format!(r#"<w:hdr xmlns:w="{W}">{}</w:hdr>"#, paragraph("页眉"));
        let bytes = docx_with(
            &format!("{}{}", paragraph("投标函"), paragraph("无书签正文")),
            &[("word/header1.xml", &header)],
        );
        let inventory = inventory_from_docx(&bytes, None).unwrap();
        let texts: Vec<_> = inventory.units.iter().map(|u| u.text.as_str()).collect();
        assert!(texts.contains(&"投标函"), "{texts:?}");
        assert!(texts.contains(&"无书签正文"), "{texts:?}");
        assert!(texts.contains(&"页眉"), "{texts:?}");
        assert!(
            inventory
                .units
                .iter()
                .any(|u| u.part == "word/header1.xml" && u.kind == "paragraphs")
        );
        let extra = extra_units_not_covered_by_bookmarks(&inventory, &[]);
        assert!(
            extra.iter().any(|unit| unit.text == "无书签正文" && unit.bookmark.is_none()),
            "unbookmarked body text must remain in the file inventory"
        );
    }

    #[test]
    fn drawings_are_not_checked_and_do_not_grant_tender_coverage() {
        let drawing = format!(
            r#"<w:p xmlns:w="{W}"><w:r><w:drawing/></w:r></w:p>"#
        );
        let bytes = docx_with(&drawing, &[]);
        let inventory = inventory_from_docx(&bytes, None).unwrap();
        assert!(
            inventory.units.iter().any(|u| u.kind == "not_checked"),
            "{:?}",
            inventory.units
        );
        let mut coverage = OutputCoverage::default();
        let page = read_output_evidence(
            &inventory,
            &mut coverage,
            &json!({"offset":0,"limit":8}),
            16_000,
        )
        .unwrap();
        assert_eq!(page["total"], inventory.units.len());
        assert_eq!(coverage.units.len(), inventory.units.len());
        let err = read_output_evidence(
            &inventory,
            &mut coverage,
            &json!({"offset":0,"limit":8,"id":"missing"}),
            16_000,
        )
        .unwrap_err();
        assert!(err.contains("unknown output evidence id"), "{err}");
        assert!(
            schemas()
                .iter()
                .any(|t| t["function"]["name"] == "read_output_evidence")
        );
    }

    fn empty_pdf() -> Vec<u8> {
        use lopdf::{Document, Object, dictionary};
        let mut doc = Document::with_version("1.5");
        let pages = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![Object::from(0), Object::from(0), Object::from(100), Object::from(100)]
        });
        doc.objects.insert(
            pages,
            dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }.into(),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn pdf_inventory_keeps_empty_pages() {
        let docx = docx_with(&paragraph("投标函"), &[]);
        let pdf = empty_pdf();
        let inventory = inventory_from_files(&docx, Some(&pdf)).unwrap();
        assert!(inventory.pdf_sha256.is_some());
        assert!(
            inventory.units.iter().any(|unit| unit.kind == "pdf_page"),
            "{:?}",
            inventory.units.iter().map(|u| u.kind.as_str()).collect::<Vec<_>>()
        );
    }
}
