//! ①–⑤ Word export with embedded technical BidShots and qualification images.

use std::io::Cursor;

use chrono::{DateTime, Utc};
use docx_rs::{Docx, Paragraph, Pic, Run, Table, TableCell, TableRow};
use image::GenericImageView;
use sqlx::Row;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Docx,
    Pdf,
}

#[derive(Clone)]
pub struct ExportImage {
    pub bytes: Vec<u8>,
}

pub struct ExportTechRow {
    pub text: String,
    pub response: String,
    pub product: String,
    pub images: Vec<ExportImage>,
}

pub struct ExportQualRow {
    pub text: String,
    pub file_name: String,
    pub images: Vec<ExportImage>,
}

pub struct ExportDoc {
    pub title: String,
    pub owner_name: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub products: Vec<String>,
    pub tech: Vec<ExportTechRow>,
    pub deviate: Vec<(String, String)>,
    pub quals: Vec<ExportQualRow>,
    pub missing: Vec<String>,
}

fn p(text: &str) -> Paragraph {
    Paragraph::new().add_run(Run::new().add_text(text))
}

fn h(text: &str, size: usize) -> Paragraph {
    Paragraph::new().add_run(Run::new().add_text(text).bold().size(size))
}

fn cell(text: &str) -> TableCell {
    TableCell::new().add_paragraph(p(text))
}

fn pic_from_bytes(bytes: &[u8]) -> Option<Pic> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let mut png = Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).ok()?;
    let max_w = 480u32;
    let (dw, dh) = if w > max_w {
        let nh = (u64::from(h) * u64::from(max_w) / u64::from(w)) as u32;
        (max_w, nh.max(1))
    } else {
        (w, h)
    };
    Some(Pic::new_with_dimensions(png.into_inner(), dw, dh))
}

fn add_images(docx: Docx, images: &[ExportImage]) -> Docx {
    let mut docx = docx;
    let mut any = false;
    for img in images {
        let Some(pic) = pic_from_bytes(&img.bytes) else {
            continue;
        };
        any = true;
        docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_image(pic)));
    }
    if !any && !images.is_empty() {
        docx = docx.add_paragraph(p("（材料不是可嵌入的图片，已保留文件名）"));
    }
    docx
}

pub fn build_export_docx(doc: &ExportDoc) -> Result<Vec<u8>, String> {
    let expires = doc
        .expires_at
        .map(|t| t.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "未填".into());
    let mut docx = Docx::new()
        .add_paragraph(h("投标预览（①～⑤）", 36))
        .add_paragraph(h("① 项目扉页", 28))
        .add_paragraph(p(&format!("项目：{}", doc.title)))
        .add_paragraph(p(&format!("负责人：{}", doc.owner_name)))
        .add_paragraph(p(&format!("招标结束：{expires}")))
        .add_paragraph(p(&format!(
            "已选产品：{}",
            if doc.products.is_empty() {
                "无".into()
            } else {
                doc.products.join("、")
            }
        )))
        .add_paragraph(h("② 技术点对点", 28));
    if doc.tech.is_empty() {
        docx = docx.add_paragraph(p("暂无已确认技术条款。"));
    }
    for (i, row) in doc.tech.iter().enumerate() {
        docx = docx
            .add_paragraph(h(&format!("{}. {}", i + 1, row.text), 22))
            .add_paragraph(p(&format!("应答：{}", row.response)))
            .add_paragraph(p(&format!("产品：{}", row.product)));
        docx = add_images(docx, &row.images);
    }
    docx = docx.add_paragraph(h("③ 技术偏离表", 28));
    if doc.deviate.is_empty() {
        docx = docx.add_paragraph(p("无偏离 / 无 must 未覆盖。"));
    } else {
        let mut rows = vec![TableRow::new(vec![cell("条款"), cell("类型")])];
        for (text, kind) in &doc.deviate {
            rows.push(TableRow::new(vec![cell(text), cell(kind)]));
        }
        docx = docx.add_table(Table::new(rows));
    }
    docx = docx.add_paragraph(h("④ 资格 / 商务材料", 28));
    if doc.quals.is_empty() {
        docx = docx.add_paragraph(p("暂无已命中的公司资料。"));
    }
    for (i, row) in doc.quals.iter().enumerate() {
        docx = docx
            .add_paragraph(h(&format!("{}. {}", i + 1, row.text), 22))
            .add_paragraph(p(&format!("材料：{}", row.file_name)));
        docx = add_images(docx, &row.images);
    }
    docx = docx.add_paragraph(h("⑤ 商务缺件", 28));
    if doc.missing.is_empty() {
        docx = docx.add_paragraph(p("无 must 缺件。"));
    } else {
        let mut rows = vec![TableRow::new(vec![cell("条款")])];
        for text in &doc.missing {
            rows.push(TableRow::new(vec![cell(text)]));
        }
        docx = docx.add_table(Table::new(rows));
    }
    let mut buf = Cursor::new(Vec::new());
    docx.pack(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}

fn cjk_font_bytes() -> Result<Vec<u8>, String> {
    const CANDIDATES: &[&str] = &[
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/opentype/unifont/unifont.otf",
    ];
    for p in CANDIDATES {
        if let Ok(b) = std::fs::read(p) {
            return Ok(b);
        }
    }
    Err("no CJK font on this host (need WenQuanYi or Unifont)".into())
}

pub fn build_export_pdf(doc: &ExportDoc) -> Result<Vec<u8>, String> {
    use printpdf::{
        FontId, Mm, Op, ParsedFont, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt,
        RawImage, TextItem, XObjectId, XObjectTransform,
    };

    let font_bytes = cjk_font_bytes()?;
    let mut font_warnings = Vec::new();
    let parsed = ParsedFont::from_bytes(&font_bytes, 0, &mut font_warnings)
        .ok_or_else(|| "parse CJK font".to_string())?;
    let mut pdf = PdfDocument::new(&doc.title);
    let font = pdf.add_font(&parsed);

    const PAGE_W: f32 = 210.0;
    const PAGE_H: f32 = 297.0;
    const MARGIN: f32 = 16.0;
    let mut pages: Vec<Vec<Op>> = Vec::new();
    let mut ops: Vec<Op> = Vec::new();
    let mut y = PAGE_H - MARGIN;

    let flush = |pages: &mut Vec<Vec<Op>>, ops: &mut Vec<Op>, y: &mut f32| {
        if !ops.is_empty() {
            pages.push(std::mem::take(ops));
        }
        *y = PAGE_H - MARGIN;
    };

    let line_h = |size: f32| size * 0.45;
    let wrap_w = PAGE_W - MARGIN * 2.0;

    let write_line = |ops: &mut Vec<Op>,
                      pages: &mut Vec<Vec<Op>>,
                      y: &mut f32,
                      font: &FontId,
                      text: &str,
                      size: f32| {
        if *y < MARGIN + line_h(size) {
            flush(pages, ops, y);
        }
        ops.extend([
            Op::StartTextSection,
            Op::SetTextCursor {
                pos: Point {
                    x: Mm(MARGIN).into(),
                    y: Mm(*y).into(),
                },
            },
            Op::SetFont {
                font: PdfFontHandle::External(font.clone()),
                size: Pt(size),
            },
            Op::SetLineHeight { lh: Pt(size + 2.0) },
            Op::ShowText {
                items: vec![TextItem::Text(text.to_string())],
            },
            Op::EndTextSection,
        ]);
        *y -= line_h(size);
    };

    let write_text = |ops: &mut Vec<Op>,
                      pages: &mut Vec<Vec<Op>>,
                      y: &mut f32,
                      font: &FontId,
                      text: &str,
                      size: f32| {
        let max_chars = ((wrap_w / (size * 0.38)).max(8.0)) as usize;
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            write_line(ops, pages, y, font, " ", size);
            return;
        }
        for chunk in chars.chunks(max_chars) {
            write_line(ops, pages, y, font, &chunk.iter().collect::<String>(), size);
        }
    };

    let write_image = |ops: &mut Vec<Op>,
                       pages: &mut Vec<Vec<Op>>,
                       y: &mut f32,
                       pdf: &mut PdfDocument,
                       bytes: &[u8]| {
        let mut w = Vec::new();
        let Ok(img) = RawImage::decode_from_bytes(bytes, &mut w) else {
            return;
        };
        let id: XObjectId = pdf.add_image(&img);
        let max_w_mm = 140.0_f32;
        let nat_w = img.width.max(1) as f32 * 25.4 / 150.0;
        let nat_h = img.height.max(1) as f32 * 25.4 / 150.0;
        let scale = (max_w_mm / nat_w).min(1.0);
        let draw_h = nat_h * scale;
        if *y - draw_h < MARGIN {
            flush(pages, ops, y);
        }
        *y -= draw_h;
        ops.push(Op::UseXobject {
            id,
            transform: XObjectTransform {
                translate_x: Some(Mm(MARGIN).into()),
                translate_y: Some(Mm(*y).into()),
                scale_x: Some(scale),
                scale_y: Some(scale),
                dpi: Some(150.0),
                ..Default::default()
            },
        });
        *y -= 3.0;
    };

    write_text(
        &mut ops,
        &mut pages,
        &mut y,
        &font,
        "投标预览（定稿 PDF）",
        18.0,
    );
    write_text(&mut ops, &mut pages, &mut y, &font, "① 项目扉页", 14.0);
    write_text(
        &mut ops,
        &mut pages,
        &mut y,
        &font,
        &format!("项目：{}", doc.title),
        11.0,
    );
    write_text(
        &mut ops,
        &mut pages,
        &mut y,
        &font,
        &format!("负责人：{}", doc.owner_name),
        11.0,
    );
    let expires = doc
        .expires_at
        .map(|t| t.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "未填".into());
    write_text(
        &mut ops,
        &mut pages,
        &mut y,
        &font,
        &format!("招标结束：{expires}"),
        11.0,
    );
    let products = if doc.products.is_empty() {
        "无".into()
    } else {
        doc.products.join("、")
    };
    write_text(
        &mut ops,
        &mut pages,
        &mut y,
        &font,
        &format!("已选产品：{products}"),
        11.0,
    );

    write_text(&mut ops, &mut pages, &mut y, &font, "② 技术点对点", 14.0);
    if doc.tech.is_empty() {
        write_text(
            &mut ops,
            &mut pages,
            &mut y,
            &font,
            "暂无已确认技术条款。",
            11.0,
        );
    }
    for (i, row) in doc.tech.iter().enumerate() {
        write_text(
            &mut ops,
            &mut pages,
            &mut y,
            &font,
            &format!("{}. {}", i + 1, row.text),
            12.0,
        );
        if !row.response.is_empty() {
            write_text(
                &mut ops,
                &mut pages,
                &mut y,
                &font,
                &format!("应答：{}", row.response),
                11.0,
            );
        }
        if !row.product.is_empty() {
            write_text(
                &mut ops,
                &mut pages,
                &mut y,
                &font,
                &format!("产品：{}", row.product),
                11.0,
            );
        }
        for img in &row.images {
            write_image(&mut ops, &mut pages, &mut y, &mut pdf, &img.bytes);
        }
    }

    write_text(&mut ops, &mut pages, &mut y, &font, "③ 技术偏离表", 14.0);
    if doc.deviate.is_empty() {
        write_text(
            &mut ops,
            &mut pages,
            &mut y,
            &font,
            "无偏离 / 无 must 未覆盖。",
            11.0,
        );
    } else {
        for (text, kind) in &doc.deviate {
            write_text(
                &mut ops,
                &mut pages,
                &mut y,
                &font,
                &format!("{text}  [{kind}]"),
                11.0,
            );
        }
    }

    write_text(
        &mut ops,
        &mut pages,
        &mut y,
        &font,
        "④ 资格 / 商务材料",
        14.0,
    );
    if doc.quals.is_empty() {
        write_text(
            &mut ops,
            &mut pages,
            &mut y,
            &font,
            "暂无已命中的公司资料。",
            11.0,
        );
    }
    for (i, row) in doc.quals.iter().enumerate() {
        write_text(
            &mut ops,
            &mut pages,
            &mut y,
            &font,
            &format!("{}. {}", i + 1, row.text),
            12.0,
        );
        write_text(
            &mut ops,
            &mut pages,
            &mut y,
            &font,
            &format!("材料：{}", row.file_name),
            11.0,
        );
        if row.images.is_empty() {
            write_text(
                &mut ops,
                &mut pages,
                &mut y,
                &font,
                "（原件无法嵌图，仅保留文件名）",
                10.0,
            );
        }
        for img in &row.images {
            write_image(&mut ops, &mut pages, &mut y, &mut pdf, &img.bytes);
        }
    }

    write_text(&mut ops, &mut pages, &mut y, &font, "⑤ 商务缺件", 14.0);
    if doc.missing.is_empty() {
        write_text(&mut ops, &mut pages, &mut y, &font, "无 must 缺件。", 11.0);
    } else {
        for text in &doc.missing {
            write_text(&mut ops, &mut pages, &mut y, &font, text, 11.0);
        }
    }
    if !ops.is_empty() {
        pages.push(ops);
    }
    let pdf_pages: Vec<PdfPage> = pages
        .into_iter()
        .map(|ops| PdfPage::new(Mm(PAGE_W), Mm(PAGE_H), ops))
        .collect();
    let mut save_warnings = Vec::new();
    Ok(pdf
        .with_pages(pdf_pages)
        .save(&PdfSaveOptions::default(), &mut save_warnings))
}

pub async fn export_project(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    kind: ExportKind,
) -> Result<(String, Vec<u8>), String> {
    export_project_opts(pool, project_id, kind, false).await
}

pub async fn export_project_opts(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    kind: ExportKind,
    regenerate_stale: bool,
) -> Result<(String, Vec<u8>), String> {
    let row = storage::bid::get_project(pool, project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "bid missing".to_string())?;
    let ended = row.get::<String, _>("status") == "ended";
    if ended && regenerate_stale {
        return Err("project ended".into());
    }
    let parts = crate::booklet::ensure_all_parts(pool, project_id, regenerate_stale).await?;
    let must = crate::booklet::must_ids(pool, project_id).await?;
    let missing = crate::booklet::missing_must_in_parts(&must, &parts);
    if !missing.is_empty() {
        return Err(format!(
            "成稿②缺少 {} 条必须条款锚，请重生成或补回 <!-- clause:id -->",
            missing.len()
        ));
    }
    let title: String = row.get("title");
    let md = parts
        .into_iter()
        .map(|p| p.markdown)
        .collect::<Vec<_>>()
        .join("\n\n");
    let clause_imgs = load_clause_images(pool, project_id).await;
    let object_imgs = load_object_images_in(&md);
    let bytes = match kind {
        ExportKind::Docx => md_to_docx_rich(&title, &md, &clause_imgs, &object_imgs)?,
        ExportKind::Pdf => md_to_pdf_rich(&title, &md, &clause_imgs, &object_imgs)?,
    };
    Ok((title, bytes))
}

fn blob_from_key(key: &str) -> Option<Vec<u8>> {
    let hash = key.rsplit('/').next().unwrap_or(key);
    storage::read_blob(hash).ok()
}

async fn load_clause_images(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> std::collections::HashMap<Uuid, Vec<ExportImage>> {
    let mut out: std::collections::HashMap<Uuid, Vec<ExportImage>> =
        std::collections::HashMap::new();
    let Ok(rows) = storage::bid::list_shots(pool, project_id).await else {
        return out;
    };
    for r in rows {
        let cid: Uuid = r.get("clause_id");
        let key: String = r.get("object_key");
        if let Some(bytes) = blob_from_key(&key) {
            out.entry(cid).or_default().push(ExportImage { bytes });
        }
    }
    out
}

fn load_object_images_in(md: &str) -> std::collections::HashMap<String, Vec<u8>> {
    let re = regex::Regex::new(r"objects/[a-fA-F0-9]{64}").expect("object key regex");
    let mut out = std::collections::HashMap::new();
    for m in re.find_iter(md) {
        let key = m.as_str();
        if out.contains_key(key) {
            continue;
        }
        if let Some(bytes) = blob_from_key(key) {
            out.insert(key.to_string(), bytes);
        }
    }
    out
}

fn prepare_export_md(md: &str) -> String {
    let re = regex::Regex::new(r"<!--\s*clause:([0-9a-fA-F-]{36})\s*-->").expect("anchor regex");
    re.replace_all(md, "[[clause:$1]]").into_owned()
}

fn take_clause_ids(text: &str) -> (String, Vec<Uuid>) {
    let re = regex::Regex::new(r"\[\[clause:([0-9a-fA-F-]{36})\]\]").expect("marker regex");
    let ids = re
        .captures_iter(text)
        .filter_map(|c| Uuid::parse_str(&c[1]).ok())
        .collect();
    (re.replace_all(text, "").into_owned(), ids)
}

fn md_blocks(md: &str) -> Vec<(u8, String, Vec<Uuid>, Vec<String>)> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let src = prepare_export_md(md);
    let parser = Parser::new_ext(&src, Options::ENABLE_TABLES);
    let mut out = Vec::new();
    let mut heading: u8 = 0;
    let mut buf = String::new();
    let flush =
        |out: &mut Vec<(u8, String, Vec<Uuid>, Vec<String>)>, heading: u8, buf: &mut String| {
            if buf.trim().is_empty() {
                buf.clear();
                return;
            }
            let objects = {
                let re = regex::Regex::new(r"objects/[a-fA-F0-9]{64}").expect("obj");
                re.find_iter(buf).map(|m| m.as_str().to_string()).collect()
            };
            let (text, ids) = take_clause_ids(buf);
            let text = text.trim().to_string();
            if !text.is_empty() || !ids.is_empty() {
                out.push((heading, text, ids, objects));
            }
            buf.clear();
        };
    for ev in parser {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                heading = match level {
                    pulldown_cmark::HeadingLevel::H1 => 1,
                    pulldown_cmark::HeadingLevel::H2 => 2,
                    _ => 3,
                };
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(&mut out, heading.max(1), &mut buf);
                heading = 0;
            }
            Event::End(TagEnd::Paragraph)
            | Event::End(TagEnd::Item)
            | Event::End(TagEnd::TableRow)
            | Event::End(TagEnd::TableCell) => {
                flush(&mut out, heading, &mut buf);
            }
            Event::Text(t) | Event::Code(t) => buf.push_str(&t),
            Event::SoftBreak | Event::HardBreak => buf.push(' '),
            Event::Start(Tag::Item) => {
                if !buf.is_empty() {
                    buf.push(' ');
                }
                buf.push_str("• ");
            }
            _ => {}
        }
    }
    flush(&mut out, heading, &mut buf);
    out
}

fn md_to_docx_rich(
    title: &str,
    md: &str,
    clause_imgs: &std::collections::HashMap<Uuid, Vec<ExportImage>>,
    object_imgs: &std::collections::HashMap<String, Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let mut docx = Docx::new().add_paragraph(h(title, 36));
    for (lvl, text, ids, objects) in md_blocks(md) {
        if !text.is_empty() {
            docx = match lvl {
                1 => docx.add_paragraph(h(&text, 32)),
                2 => docx.add_paragraph(h(&text, 28)),
                3 => docx.add_paragraph(h(&text, 22)),
                _ => docx.add_paragraph(p(&text)),
            };
        }
        for id in ids {
            if let Some(imgs) = clause_imgs.get(&id) {
                docx = add_images(docx, imgs);
            }
        }
        for key in objects {
            if let Some(bytes) = object_imgs.get(&key) {
                docx = add_images(
                    docx,
                    &[ExportImage {
                        bytes: bytes.clone(),
                    }],
                );
            }
        }
    }
    let mut buf = Cursor::new(Vec::new());
    docx.pack(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}

fn md_to_pdf_rich(
    title: &str,
    md: &str,
    clause_imgs: &std::collections::HashMap<Uuid, Vec<ExportImage>>,
    object_imgs: &std::collections::HashMap<String, Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let mut blocks = Vec::new();
    for (lvl, text, ids, objects) in md_blocks(md) {
        let mut row_imgs = Vec::new();
        for id in ids {
            if let Some(imgs) = clause_imgs.get(&id) {
                row_imgs.extend(imgs.iter().map(|i| ExportImage {
                    bytes: i.bytes.clone(),
                }));
            }
        }
        for key in objects {
            if let Some(bytes) = object_imgs.get(&key) {
                row_imgs.push(ExportImage {
                    bytes: bytes.clone(),
                });
            }
        }
        blocks.push((lvl, text, row_imgs));
    }
    pdf_from_blocks(title, &blocks)
}

fn pdf_from_blocks(
    title: &str,
    blocks: &[(u8, String, Vec<ExportImage>)],
) -> Result<Vec<u8>, String> {
    use printpdf::{
        FontId, Mm, Op, ParsedFont, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt,
        RawImage, TextItem, XObjectId, XObjectTransform,
    };
    let font_bytes = cjk_font_bytes()?;
    let mut font_warnings = Vec::new();
    let parsed = ParsedFont::from_bytes(&font_bytes, 0, &mut font_warnings)
        .ok_or_else(|| "parse CJK font".to_string())?;
    let mut pdf = PdfDocument::new(title);
    let font = pdf.add_font(&parsed);
    const PAGE_W: f32 = 210.0;
    const PAGE_H: f32 = 297.0;
    const MARGIN: f32 = 16.0;
    let mut pages: Vec<Vec<Op>> = Vec::new();
    let mut ops: Vec<Op> = Vec::new();
    let mut y = PAGE_H - MARGIN;
    let flush = |pages: &mut Vec<Vec<Op>>, ops: &mut Vec<Op>, y: &mut f32| {
        if !ops.is_empty() {
            pages.push(std::mem::take(ops));
        }
        *y = PAGE_H - MARGIN;
    };
    let line_h = |size: f32| size * 0.45;
    let wrap_w = PAGE_W - MARGIN * 2.0;
    let write_line = |ops: &mut Vec<Op>,
                      pages: &mut Vec<Vec<Op>>,
                      y: &mut f32,
                      font: &FontId,
                      text: &str,
                      size: f32| {
        if *y < MARGIN + line_h(size) {
            flush(pages, ops, y);
        }
        ops.extend([
            Op::StartTextSection,
            Op::SetTextCursor {
                pos: Point {
                    x: Mm(MARGIN).into(),
                    y: Mm(*y).into(),
                },
            },
            Op::SetFont {
                font: PdfFontHandle::External(font.clone()),
                size: Pt(size),
            },
            Op::SetLineHeight { lh: Pt(size + 2.0) },
            Op::ShowText {
                items: vec![TextItem::Text(text.to_string())],
            },
            Op::EndTextSection,
        ]);
        *y -= line_h(size);
    };
    let write_text = |ops: &mut Vec<Op>,
                      pages: &mut Vec<Vec<Op>>,
                      y: &mut f32,
                      font: &FontId,
                      text: &str,
                      size: f32| {
        let max_chars = ((wrap_w / (size * 0.38)).max(8.0)) as usize;
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            write_line(ops, pages, y, font, " ", size);
            return;
        }
        for chunk in chars.chunks(max_chars) {
            write_line(ops, pages, y, font, &chunk.iter().collect::<String>(), size);
        }
    };
    let write_image = |ops: &mut Vec<Op>,
                       pages: &mut Vec<Vec<Op>>,
                       y: &mut f32,
                       pdf: &mut PdfDocument,
                       bytes: &[u8]| {
        let mut w = Vec::new();
        let Ok(img) = RawImage::decode_from_bytes(bytes, &mut w) else {
            return;
        };
        let id: XObjectId = pdf.add_image(&img);
        let max_w_mm = 140.0_f32;
        let nat_w = img.width.max(1) as f32 * 25.4 / 150.0;
        let nat_h = img.height.max(1) as f32 * 25.4 / 150.0;
        let scale = (max_w_mm / nat_w).min(1.0);
        let draw_h = nat_h * scale;
        if *y - draw_h < MARGIN {
            flush(pages, ops, y);
        }
        *y -= draw_h;
        ops.push(Op::UseXobject {
            id,
            transform: XObjectTransform {
                translate_x: Some(Mm(MARGIN).into()),
                translate_y: Some(Mm(*y).into()),
                scale_x: Some(scale),
                scale_y: Some(scale),
                dpi: Some(150.0),
                ..Default::default()
            },
        });
        *y -= 3.0;
    };
    write_text(&mut ops, &mut pages, &mut y, &font, title, 18.0);
    for (lvl, text, imgs) in blocks {
        if !text.is_empty() {
            let size = match *lvl {
                1 => 16.0,
                2 => 14.0,
                3 => 12.5,
                _ => 11.0,
            };
            write_text(&mut ops, &mut pages, &mut y, &font, text, size);
        }
        for img in imgs {
            write_image(&mut ops, &mut pages, &mut y, &mut pdf, &img.bytes);
        }
    }
    if !ops.is_empty() {
        pages.push(ops);
    }
    let pdf_pages: Vec<PdfPage> = pages
        .into_iter()
        .map(|ops| PdfPage::new(Mm(PAGE_W), Mm(PAGE_H), ops))
        .collect();
    let mut save_warnings = Vec::new();
    Ok(pdf
        .with_pages(pdf_pages)
        .save(&PdfSaveOptions::default(), &mut save_warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn docx_embeds_tech_and_qual_images() {
        let bytes = build_export_docx(&ExportDoc {
            title: "示范标".into(),
            owner_name: "张三".into(),
            expires_at: None,
            products: vec!["交换机A".into()],
            tech: vec![ExportTechRow {
                text: "吞吐 40G".into(),
                response: "已覆盖".into(),
                product: "交换机A".into(),
                images: vec![ExportImage {
                    bytes: PNG.to_vec(),
                }],
            }],
            deviate: vec![("时延".into(), "偏离".into())],
            quals: vec![ExportQualRow {
                text: "ISO9001".into(),
                file_name: "iso.png".into(),
                images: vec![ExportImage {
                    bytes: PNG.to_vec(),
                }],
            }],
            missing: vec!["注册资本".into()],
        })
        .expect("docx");
        assert!(bytes.starts_with(b"PK"), "not zip");
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("word/media"), "{s:.200}");
        assert!(bytes.windows(4).any(|w| w == b"word"));
    }

    #[test]
    fn pdf_is_pdf_and_embeds_images() {
        let bytes = build_export_pdf(&ExportDoc {
            title: "示范标".into(),
            owner_name: "张三".into(),
            expires_at: None,
            products: vec!["交换机A".into()],
            tech: vec![ExportTechRow {
                text: "吞吐 40G".into(),
                response: "已覆盖".into(),
                product: "交换机A".into(),
                images: vec![ExportImage {
                    bytes: PNG.to_vec(),
                }],
            }],
            deviate: vec![],
            quals: vec![ExportQualRow {
                text: "ISO9001".into(),
                file_name: "iso.png".into(),
                images: vec![ExportImage {
                    bytes: PNG.to_vec(),
                }],
            }],
            missing: vec![],
        })
        .expect("pdf");
        assert!(bytes.starts_with(b"%PDF"), "not pdf");
        assert!(bytes.len() > 200);
    }

    #[test]
    fn manuscript_pdf_has_no_empty_booklet_chrome() {
        let md = "# ② 交换机\n\n已覆盖\n";
        let bytes =
            md_to_pdf_rich("示范标", md, &Default::default(), &Default::default()).expect("pdf");
        assert!(bytes.starts_with(b"%PDF"), "not pdf");
        let s = String::from_utf8_lossy(&bytes);
        assert!(!s.contains("暂无已命中的公司资料"));
        assert!(!s.contains("无 must 缺件"));
    }
}
