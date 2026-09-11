//! Render an explicit source-backed reference fixture for comparison with Agent
//! output. This never marks a tender analysis or composition independently reviewed.
use bidding::{
    docx_composition::document,
    docx_template::{TemplatePlan, compile_template},
};
use serde_json::Value;
use std::{env, fs};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 4 {
        return Err(
            "usage: docx_template_fixture <frozen-input.json> <plan.json> <output-directory>"
                .into(),
        );
    }
    let input: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    let plan: TemplatePlan = serde_json::from_slice(&fs::read(&args[2])?)?;
    let bytes = compile_template(&input, &plan)?;
    let rendered = document::verify(&bytes, &input, &plan)?;
    let out = std::path::Path::new(&args[3]);
    fs::create_dir_all(out)?;
    fs::write(out.join("reference-template.docx"), bytes)?;
    fs::write(
        out.join("rendered.json"),
        serde_json::to_vec_pretty(&rendered)?,
    )?;
    Ok(())
}
