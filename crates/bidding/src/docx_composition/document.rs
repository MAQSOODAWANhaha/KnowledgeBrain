//! Verify generated OOXML, not a second uploaded-document parser. Original
//! tender parsing remains exclusively in Python docreader.
use crate::docx_template::{TemplatePlan, bookmark_name};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CellView {
    pub row: usize,
    pub column: usize,
    pub rowspan: usize,
    pub colspan: usize,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableView {
    pub rows: usize,
    pub grid_twips: Vec<usize>,
    pub headers: Vec<usize>,
    pub cells: Vec<CellView>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenderedBlock {
    pub bookmark: String,
    pub paragraphs: Vec<String>,
    pub paragraph_styles: Vec<Option<String>>,
    pub table: Option<TableView>,
}

fn text(node: roxmltree::Node<'_, '_>) -> String {
    node.descendants()
        .filter(|n| n.is_element())
        .filter_map(|n| match n.tag_name().name() {
            "t" if n.tag_name().namespace() == Some(W) => Some(n.text().unwrap_or("").to_owned()),
            "br" | "cr" if n.tag_name().namespace() == Some(W) => Some("\n".into()),
            "tab" if n.tag_name().namespace() == Some(W) => Some("\t".into()),
            _ => None,
        })
        .collect()
}
fn value(node: roxmltree::Node<'_, '_>, attribute: &str) -> Result<usize, String> {
    node.attribute((W, attribute))
        .ok_or("OOXML numeric attribute missing")?
        .parse()
        .map_err(|_| "invalid OOXML numeric value".into())
}
fn table(node: roxmltree::Node<'_, '_>) -> Result<TableView, String> {
    let mut out = TableView {
        rows: 0,
        grid_twips: vec![],
        headers: vec![],
        cells: vec![],
    };
    for c in node
        .children()
        .filter(|n| n.has_tag_name((W, "tblGrid")))
        .flat_map(|n| n.children().filter(|n| n.has_tag_name((W, "gridCol"))))
    {
        out.grid_twips.push(value(c, "w")?);
    }
    let mut prior: BTreeMap<usize, usize> = BTreeMap::new();
    for (row, tr) in node
        .children()
        .filter(|n| n.has_tag_name((W, "tr")))
        .enumerate()
    {
        out.rows += 1;
        if tr
            .children()
            .filter(|n| n.has_tag_name((W, "trPr")))
            .any(|p| p.children().any(|n| n.has_tag_name((W, "tblHeader"))))
        {
            out.headers.push(row);
        }
        let mut next = BTreeMap::new();
        let mut column = 0;
        for tc in tr.children().filter(|n| n.has_tag_name((W, "tc"))) {
            let props = tc
                .children()
                .find(|n| n.has_tag_name((W, "tcPr")))
                .ok_or("cell properties missing")?;
            let colspan = props
                .children()
                .find(|n| n.has_tag_name((W, "gridSpan")))
                .map(|n| value(n, "val"))
                .transpose()?
                .unwrap_or(1);
            let content = tc
                .children()
                .filter(|n| n.has_tag_name((W, "p")))
                .map(text)
                .collect::<Vec<_>>()
                .join("\n");
            let merge = props.children().find(|n| n.has_tag_name((W, "vMerge")));
            if let Some(merge) = merge
                && merge.attribute((W, "val")) != Some("restart")
            {
                let index = *prior
                    .get(&column)
                    .ok_or("orphan vertical merge continuation")?;
                if out.cells[index].colspan != colspan || !content.is_empty() {
                    return Err("vertical merge changed geometry or content".into());
                }
                out.cells[index].rowspan += 1;
                next.insert(column, index);
            } else {
                if merge.is_some() {
                    next.insert(column, out.cells.len());
                }
                out.cells.push(CellView {
                    row,
                    column,
                    rowspan: 1,
                    colspan,
                    text: content,
                });
            }
            column += colspan;
        }
        if column != out.grid_twips.len() {
            return Err("rendered row does not cover table grid".into());
        }
        prior = next;
    }
    Ok(out)
}

fn read_range(root: roxmltree::Node<'_, '_>, name: &str) -> Result<RenderedBlock, String> {
    let starts: Vec<_> = root
        .descendants()
        .filter(|n| n.has_tag_name((W, "bookmarkStart")) && n.attribute((W, "name")) == Some(name))
        .collect();
    if starts.len() != 1 {
        return Err("document location missing or duplicated".into());
    }
    let start = starts[0];
    let id = start.attribute((W, "id")).ok_or("bookmark id missing")?;
    let mut out = RenderedBlock {
        bookmark: name.into(),
        paragraphs: vec![],
        paragraph_styles: vec![],
        table: None,
    };
    let mut found = false;
    for node in start.next_siblings().skip(1).filter(|n| n.is_element()) {
        if node.has_tag_name((W, "bookmarkEnd")) && node.attribute((W, "id")) == Some(id) {
            found = true;
            break;
        }
        if node.has_tag_name((W, "p")) {
            out.paragraphs.push(text(node));
            out.paragraph_styles.push(
                node.children()
                    .find(|n| n.has_tag_name((W, "pPr")))
                    .and_then(|n| n.children().find(|n| n.has_tag_name((W, "pStyle"))))
                    .and_then(|n| n.attribute((W, "val")))
                    .map(str::to_owned),
            );
        } else if node.has_tag_name((W, "tbl")) && out.table.is_none() {
            out.table = Some(table(node)?);
        } else {
            return Err("unexpected content inside generated document location".into());
        }
    }
    if !found {
        return Err("unclosed generated document location".into());
    }
    Ok(out)
}
fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn read_text_regions(
    root: roxmltree::Node<'_, '_>,
    section: usize,
    block: usize,
    fragments: &[String],
) -> Result<Vec<RenderedBlock>, String> {
    let name = bookmark_name(section, Some(block));
    let start = root
        .descendants()
        .find(|n| {
            n.has_tag_name((W, "bookmarkStart")) && n.attribute((W, "name")) == Some(name.as_str())
        })
        .ok_or("text block missing")?;
    let paragraph = start
        .next_siblings()
        .skip(1)
        .find(|n| n.is_element())
        .filter(|n| n.has_tag_name((W, "p")))
        .ok_or("text block paragraph missing")?;
    let nodes: Vec<_> = paragraph.children().filter(|n| n.is_element()).collect();
    if nodes.len() != fragments.len() * 3 {
        return Err("text region boundaries changed".into());
    }
    let mut out = vec![];
    for (index, (expected, nodes)) in fragments.iter().zip(nodes.chunks_exact(3)).enumerate() {
        let name = crate::docx_template::region_bookmark_name(section, block, index);
        let [start, run, end] = nodes else {
            unreachable!()
        };
        let id = start
            .attribute((W, "id"))
            .ok_or("text region identity missing")?;
        if !start.has_tag_name((W, "bookmarkStart"))
            || start.attribute((W, "name")) != Some(name.as_str())
            || !run.has_tag_name((W, "r"))
            || !end.has_tag_name((W, "bookmarkEnd"))
            || end.attribute((W, "id")) != Some(id)
            || text(*run) != *expected
            || root
                .descendants()
                .filter(|n| {
                    n.has_tag_name((W, "bookmarkStart"))
                        && (n.attribute((W, "name")) == Some(name.as_str())
                            || n.attribute((W, "id")) == Some(id))
                })
                .count()
                != 1
            || root
                .descendants()
                .filter(|n| {
                    n.has_tag_name((W, "bookmarkEnd")) && n.attribute((W, "id")) == Some(id)
                })
                .count()
                != 1
        {
            return Err("text region wording, order or independent location changed".into());
        }
        out.push(RenderedBlock {
            bookmark: name,
            paragraphs: vec![text(*run)],
            paragraph_styles: vec![None],
            table: None,
        });
    }
    Ok(out)
}
fn twips(mm: f64) -> usize {
    (mm * 1440.0 / 25.4).round() as usize
}

fn consume_ranges(
    children: &[roxmltree::Node<'_, '_>],
    cursor: &mut usize,
    ranges: &[RenderedBlock],
) -> Result<(), String> {
    for range in ranges {
        let start = children
            .get(*cursor)
            .ok_or("missing ordered document range")?;
        if !start.has_tag_name((W, "bookmarkStart"))
            || start.attribute((W, "name")) != Some(range.bookmark.as_str())
        {
            return Err("rendered sections or blocks are out of order".into());
        }
        *cursor += 2 + range.paragraphs.len() + usize::from(range.table.is_some());
    }
    Ok(())
}

fn page_break(node: roxmltree::Node<'_, '_>) -> bool {
    node.has_tag_name((W, "p"))
        && text(node) == "\n"
        && node
            .descendants()
            .filter(|n| n.has_tag_name((W, "br")) && n.attribute((W, "type")) == Some("page"))
            .count()
            == 1
}

fn preserved<'a>(
    input: &'a Value,
    block: &crate::docx_template::TemplateBlock,
) -> Result<&'a Value, String> {
    let key = block
        .preserved_key
        .as_deref()
        .ok_or("preserved block has no receipt key")?;
    input["preserved_units"]
        .get(key)
        .ok_or_else(|| "preserved content is not in the readback receipt".to_string())
}

pub fn verify(
    bytes: &[u8],
    input: &Value,
    plan: &TemplatePlan,
) -> Result<Vec<RenderedBlock>, String> {
    crate::tender_upload::validate_docx_document(bytes).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .map_err(|e| e.to_string())?
        .read_to_string(&mut xml)
        .map_err(|e| e.to_string())?;
    let doc = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
    let mut notice_ranges = Vec::new();
    for notice in &plan.notices {
        let rendered = read_range(doc.root(), &crate::docx_template::notice_bookmark(notice))?;
        if rendered.paragraphs != [normalized(notice)] || rendered.table.is_some() {
            return Err("outline warning text changed during compilation".into());
        }
        notice_ranges.push(rendered);
    }
    let mut out = vec![];
    let mut inline = vec![];
    for (s, section) in plan.sections.iter().enumerate() {
        let heading = read_range(doc.root(), &bookmark_name(s, None))?;
        if heading.paragraphs != [normalized(&section.title)] || heading.table.is_some() {
            return Err("rendered chapter title disagrees".into());
        }
        let style = if section.placement.is_body() {
            format!("Heading{}", section.depth + 1)
        } else {
            "Title".into()
        };
        if heading.paragraph_styles != [Some(style)] {
            return Err("rendered heading hierarchy changed".into());
        }
        out.push(heading);
        for (b, block) in section.blocks.iter().enumerate() {
            let rendered = read_range(doc.root(), &bookmark_name(s, Some(b)))?;
            match block.kind.as_str() {
                "text_regions" => {
                    let fragments = crate::docx_template::resolve_text_regions(input, block)
                        .map_err(|e| e.to_string())?;
                    if rendered.paragraphs != [fragments.concat()] || rendered.table.is_some() {
                        return Err("text region paragraph or source line breaks changed".into());
                    }
                    inline.extend(read_text_regions(doc.root(), s, b, &fragments)?);
                }
                "source_excerpt" => {
                    let expected = normalized(
                        &crate::docx_template::resolve_source_parts(input, &block.source_parts)
                            .map_err(|e| e.to_string())?,
                    );
                    if rendered.paragraphs != [expected] || rendered.table.is_some() {
                        return Err("source excerpt did not survive DOCX rendering".into());
                    }
                }
                "condition" => {
                    let expected = normalized(&format!(
                        "适用条件：{}",
                        block
                            .condition
                            .as_deref()
                            .ok_or("template condition missing")?
                    ));
                    if rendered.paragraphs != [expected] || rendered.table.is_some() {
                        return Err(
                            "template applicability condition did not survive DOCX rendering"
                                .into(),
                        );
                    }
                }
                "quote" => {
                    let expected = normalized(block.quote.as_deref().ok_or("quote missing")?)
                        .split('\n')
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    if rendered.paragraphs != expected || rendered.table.is_some() {
                        return Err("fixed text did not survive DOCX rendering".into());
                    }
                }
                "blank" => {
                    if rendered.paragraphs != [String::new()] || rendered.table.is_some() {
                        return Err("pending bidder slot contains generated content".into());
                    }
                }
                "table" | "response_table" => {
                    if !rendered.paragraphs.is_empty() {
                        return Err("paragraph unexpectedly replaces a required table".into());
                    }
                    let table = rendered
                        .table
                        .as_ref()
                        .ok_or("required table is absent from DOCX")?;
                    if block.kind == "table" {
                        let form = input["structured_forms"]
                            .as_array()
                            .ok_or("forms missing")?
                            .iter()
                            .find(|f| {
                                f["form_definition_revision_id"].as_str()
                                    == block.form_id.as_deref()
                            })
                            .ok_or("form missing")?;
                        let def = &form["definition"];
                        let widths = def["widths_mm"]
                            .as_array()
                            .ok_or("widths missing")?
                            .iter()
                            .map(|v| v.as_f64().map(twips).ok_or("width missing"))
                            .collect::<Result<Vec<_>, _>>()?;
                        if Some(table.rows as u64) != def["row_count"].as_u64()
                            || table.grid_twips != widths
                            || table.headers != (0..block.header_rows).collect::<Vec<_>>()
                        {
                            return Err("rendered table geometry or headers changed".into());
                        }
                        let raw = def["cells"].as_array().ok_or("cells missing")?;
                        for cell in &table.cells {
                            let source = raw
                                .iter()
                                .find(|c| c["row"] == cell.row && c["column"] == cell.column)
                                .ok_or("unexpected rendered cell")?;
                            let blank = block
                                .blank_cells
                                .iter()
                                .any(|c| c.row == cell.row && c.column == cell.column);
                            let expected = if blank {
                                String::new()
                            } else {
                                normalized(&crate::template_grid::blank_cell_text(
                                    source["text"].as_str().ok_or("cell text missing")?,
                                    cell.row,
                                    cell.column,
                                    &block.blank_ranges,
                                )?)
                            };
                            if cell.text != expected
                                || source["row_span"].as_u64() != Some(cell.rowspan as u64)
                                || source["col_span"].as_u64() != Some(cell.colspan as u64)
                            {
                                return Err(
                                    "rendered cell wording, blank policy or merge changed".into()
                                );
                            }
                        }
                    } else {
                        if table.rows != block.blank_rows + 1
                            || table.grid_twips.len() != block.columns.len()
                            || table.headers != [0]
                        {
                            return Err("proposed response table shape changed".into());
                        }
                        for cell in &table.cells {
                            let expected = if cell.row == 0 {
                                normalized(&block.columns[cell.column])
                            } else {
                                String::new()
                            };
                            if cell.text != expected || cell.rowspan != 1 || cell.colspan != 1 {
                                return Err(
                                    "proposed response table contains unexpected content".into()
                                );
                            }
                        }
                    }
                }
                // 保留内容按收据逐字校验：用户写的正文既不能被改写，也不能被
                // 悄悄丢掉。收据在 input 里，块只带 key。
                "preserved_paragraphs" => {
                    let unit = preserved(input, block)?;
                    let expected = unit["paragraphs"]
                        .as_array()
                        .ok_or("preserved paragraphs missing")?
                        .iter()
                        .map(|item| {
                            item.as_str()
                                .map(normalized)
                                .ok_or("preserved paragraph is not text")
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    if rendered.paragraphs != expected || rendered.table.is_some() {
                        return Err("preserved paragraphs did not survive DOCX rendering".into());
                    }
                }
                "preserved_table" => {
                    let unit = preserved(input, block)?;
                    if !rendered.paragraphs.is_empty() {
                        return Err("paragraph unexpectedly replaces a preserved table".into());
                    }
                    let table = rendered
                        .table
                        .as_ref()
                        .ok_or("preserved table is absent from DOCX")?;
                    if Some(table.rows as u64) != unit["row_count"].as_u64()
                        || table.grid_twips.len() as u64
                            != unit["column_count"].as_u64().unwrap_or(0)
                        || serde_json::to_value(&table.grid_twips).map_err(|e| e.to_string())?
                            != unit["widths_twips"]
                        || table.headers
                            != (0..unit["header_rows"]
                                .as_u64()
                                .ok_or("preserved headers missing")?
                                as usize)
                                .collect::<Vec<_>>()
                    {
                        return Err("preserved table geometry changed".into());
                    }
                    let raw = unit["cells"].as_array().ok_or("preserved cells missing")?;
                    for cell in &table.cells {
                        let source = raw
                            .iter()
                            .find(|c| c["row"] == cell.row && c["column"] == cell.column)
                            .ok_or("unexpected preserved cell")?;
                        if cell.text
                            != normalized(source["text"].as_str().ok_or("preserved cell text")?)
                            || source["row_span"].as_u64() != Some(cell.rowspan as u64)
                            || source["col_span"].as_u64() != Some(cell.colspan as u64)
                        {
                            return Err("preserved cell wording or merge changed".into());
                        }
                    }
                }
                _ => return Err("unknown generated block".into()),
            }
            out.push(rendered);
        }
    }
    let body = doc
        .descendants()
        .find(|n| n.has_tag_name((W, "body")))
        .ok_or("document body missing")?;
    for text in [&plan.title, &plan.toc_title] {
        let heading = read_range(doc.root(), &crate::docx_template::notice_bookmark(text))?;
        if heading.paragraphs != [normalized(text)] || heading.table.is_some() {
            return Err("system title bookmark does not match its text".into());
        }
    }
    let children: Vec<_> = body
        .children()
        .filter(|n| n.is_element())
        .filter(|n| {
            !((n.has_tag_name((W, "bookmarkStart")) || n.has_tag_name((W, "bookmarkEnd")))
                && matches!(n.attribute((W, "id")), Some("0" | "1")))
        })
        .collect();
    let body_sections: Vec<_> = plan
        .sections
        .iter()
        .filter(|s| s.placement.is_body())
        .collect();
    let front_sections = plan
        .sections
        .iter()
        .take_while(|s| !s.placement.is_body())
        .count();
    let prefix = body_sections.len() + 5;
    let expected_count = prefix
        + notice_ranges.len() * 3
        + out
            .iter()
            .map(|b| 2 + b.paragraphs.len() + usize::from(b.table.is_some()))
            .sum::<usize>()
        + 1;
    if children.len() != expected_count
        || !children[0].has_tag_name((W, "p"))
        || text(children[0]) != normalized(&plan.title)
    {
        return Err("DOCX contains missing or unmapped top-level content".into());
    }
    let front_ranges = plan.sections[..front_sections]
        .iter()
        .map(|s| 1 + s.blocks.len())
        .sum::<usize>();
    let mut cursor = 1;
    consume_ranges(&children, &mut cursor, &out[..front_ranges])?;
    if !page_break(children[cursor]) {
        return Err("cover and front matter must end before the TOC page".into());
    }
    cursor += 1;
    let toc_nodes = &children[cursor..cursor + body_sections.len() + 3];
    if !toc_nodes.iter().all(|n| n.has_tag_name((W, "p")))
        || text(toc_nodes[0]) != normalized(&plan.toc_title)
    {
        return Err("TOC must follow front matter and precede body chapters".into());
    }
    let field_kinds = |node: roxmltree::Node<'_, '_>| {
        node.descendants()
            .filter(|n| n.has_tag_name((W, "fldChar")))
            .map(|n| {
                n.attribute((W, "fldCharType"))
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    let instruction: String = toc_nodes[1]
        .descendants()
        .filter(|n| n.has_tag_name((W, "instrText")))
        .map(|n| n.text().unwrap_or_default())
        .collect();
    if field_kinds(toc_nodes[1]) != ["begin", "separate"]
        || instruction != " TOC \\o \"1-9\" \\h \\z \\u "
        || field_kinds(*toc_nodes.last().unwrap()) != ["end"]
        || !page_break(*toc_nodes.last().unwrap())
    {
        return Err("native TOC field or body page break changed".into());
    }
    for (i, section) in body_sections.iter().enumerate() {
        if text(toc_nodes[2 + i]) != normalized(&section.title) {
            return Err("cached TOC does not match chapters".into());
        }
    }
    cursor += toc_nodes.len();
    consume_ranges(&children, &mut cursor, &out[front_ranges..])?;
    consume_ranges(&children, &mut cursor, &notice_ranges)?;
    if cursor + 1 != children.len() {
        return Err("unmapped content after body chapters".into());
    }
    let section = children.last().ok_or("page properties missing")?;
    if !section.has_tag_name((W, "sectPr")) {
        return Err("final page properties missing".into());
    }
    let size = section
        .children()
        .find(|n| n.has_tag_name((W, "pgSz")))
        .ok_or("page size missing")?;
    if size.attribute((W, "orient"))
        != Some(if plan.style.width_mm > plan.style.height_mm {
            "landscape"
        } else {
            "portrait"
        })
    {
        return Err("rendered page orientation changed".into());
    }
    let margin = section
        .children()
        .find(|n| n.has_tag_name((W, "pgMar")))
        .ok_or("margins missing")?;
    if value(size, "w")? != twips(plan.style.width_mm)
        || value(size, "h")? != twips(plan.style.height_mm)
    {
        return Err("rendered page dimensions changed".into());
    }
    for (name, mm) in [
        ("top", plan.style.top_mm),
        ("right", plan.style.right_mm),
        ("bottom", plan.style.bottom_mm),
        ("left", plan.style.left_mm),
    ] {
        if value(margin, name)? != twips(mm) {
            return Err("rendered margins changed".into());
        }
    }
    // Nested region locations are queryable, but are not additional body blocks.
    out.extend(inline);
    Ok(out)
}
