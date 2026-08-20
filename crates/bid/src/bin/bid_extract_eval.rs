use std::path::PathBuf;

use bid::extraction::evaluation::{GoldenExpected, evaluate};
use bid::extraction::{ExtractionInput, TenderExtractionEngine};
use serde_json::json;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: bid_extract_eval <tender.md> [report.json|report.md] [expected.json]")?;
    let output = args.next().map(PathBuf::from);
    let expected_path = args.next().map(PathBuf::from);
    let markdown = std::fs::read_to_string(&input)?;
    let engine = TenderExtractionEngine::from_env()?;
    let report = match engine
        .extract(ExtractionInput::document(Uuid::new_v4(), markdown))
        .await
    {
        Ok(report) => report,
        Err(failure) => {
            let payload = json!({
                "input": input,
                "expected": expected_path,
                "generated_at": chrono::Utc::now(),
                "quality_gate": "FAIL",
                "error": failure.message,
                "diagnostics": failure.diagnostics,
                "report": serde_json::Value::Null
            });
            let rendered = serde_json::to_string_pretty(&payload)?;
            write_payload(output.as_ref(), &input, "FAIL", &rendered)?;
            return Err("extraction failed; diagnostic artifact written".into());
        }
    };
    let metrics = expected_path
        .as_ref()
        .map(|path| -> Result<_, Box<dyn std::error::Error>> {
            let expected: GoldenExpected = serde_json::from_str(&std::fs::read_to_string(path)?)?;
            Ok(evaluate(&report, &expected))
        })
        .transpose()?;
    let passed = metrics.as_ref().map(|metrics| metrics.passed);
    let gate_status = match passed {
        Some(true) => "PASS",
        Some(false) => "FAIL",
        None => "NOT_EVALUATED",
    };
    let payload = json!({
        "input": input,
        "expected": expected_path,
        "generated_at": chrono::Utc::now(),
        "quality_gate": gate_status,
        "metrics": metrics,
        "report": report
    });
    let rendered = serde_json::to_string_pretty(&payload)?;
    write_payload(output.as_ref(), &input, gate_status, &rendered)?;
    if passed == Some(false) {
        Err("extraction quality thresholds failed".into())
    } else {
        Ok(())
    }
}

fn write_payload(
    output: Option<&PathBuf>,
    input: &std::path::Path,
    gate_status: &str,
    rendered: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(output) = output {
        let body = if output.extension().and_then(|ext| ext.to_str()) == Some("md") {
            format!(
                "# Bid extraction evaluation\n\n- Input: `{}`\n- Quality gate: **{}**\n\n```json\n{}\n```\n",
                input.display(),
                gate_status,
                rendered
            )
        } else {
            format!("{rendered}\n")
        };
        std::fs::write(output, body)?;
        eprintln!("wrote {}", output.display());
    } else {
        println!("{rendered}");
    }
    Ok(())
}
