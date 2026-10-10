//! Internal producer: DocReader -> literal OCR -> frozen input -> owned publication.
//! Usage: DATABASE_URL=... prepare-tender-input manifest.json
use bidding::outline::frozen::{
    ConfiguredTenderImageProcessor, PrepareTenderInput, RawTenderDocument, prepare_tender_input,
};
use bidding::outline::store::PreparedInputStore;
use serde::Deserialize;
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    project_id: Uuid,
    document_set_id: String,
    documents: Vec<Document>,
    document_relations: Vec<bidding::analysis::DocumentRelation>,
    decisions: Vec<Value>,
    parser_contract_version: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    role: bidding::analysis::DocumentRole,
    document_id: String,
    file_path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(
        args.next()
            .ok_or("usage: prepare-tender-input manifest.json")?,
    );
    if args.next().is_some() {
        return Err("usage: prepare-tender-input manifest.json".into());
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(&path)?)?;
    let base = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let max_bytes = std::env::var("KB_TENDER_PREPARE_MAX_BYTES")
        .ok()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(256 * 1024 * 1024);
    let mut used = 0u64;
    let mut documents = Vec::new();
    for document in manifest.documents {
        let file = base.join(&document.file_path);
        let remaining = max_bytes.saturating_sub(used);
        let mut bytes = Vec::new();
        std::fs::File::open(&file)?
            .take(remaining.saturating_add(1))
            .read_to_end(&mut bytes)?;
        used = used
            .checked_add(bytes.len() as u64)
            .ok_or("raw input size overflow")?;
        if used > max_bytes {
            return Err(
                "raw input budget exceeded; raise KB_TENDER_PREPARE_MAX_BYTES deliberately".into(),
            );
        }
        let file_name = file
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("source filename must be UTF-8")?
            .to_string();
        documents.push(RawTenderDocument {
            role: document.role,
            document_id: document.document_id,
            file_name,
            bytes,
        });
    }
    let database_url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .map_err(|_| "database connection failed")?;
    let store = PreparedInputStore::new(pool, manifest.project_id);
    let processor = ConfiguredTenderImageProcessor {
        image_store: &store,
    };
    let request = PrepareTenderInput {
        project_id: manifest.project_id.to_string(),
        document_set_id: manifest.document_set_id,
        documents,
        document_relations: manifest.document_relations,
        decisions: manifest.decisions,
        parser_contract_version: manifest.parser_contract_version,
    };
    let result = async {
        for document in &request.documents {
            store
                .stage_source_document(&document.document_id, &document.file_name, &document.bytes)
                .await?;
        }
        let input = prepare_tender_input(request, &processor, &CancellationToken::new()).await?;
        store.publish_prepared_input(&input).await
    }
    .await;
    match result {
        Ok(digest) => {
            println!("{digest}");
            Ok(())
        }
        Err(error) => {
            let _ = store.abandon().await;
            Err(error.into())
        }
    }
}
