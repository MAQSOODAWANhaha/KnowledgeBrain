//! One-shot: schedule wiki:ingest for a product version.

#[tokio::main]
async fn main() {
    let vid: uuid::Uuid = std::env::args()
        .nth(1)
        .expect("product_version_id")
        .parse()
        .expect("uuid");
    let delay: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    match runtime::enqueue_wiki_ingest_in(vid, delay).await {
        Ok(id) => println!("enqueued {id:?}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
