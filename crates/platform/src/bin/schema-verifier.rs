#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    platform::init_tracing();
    let component = std::env::var(platform::COMPONENT_KIND_ENV)
        .ok()
        .and_then(|value| value.parse::<platform::SchemaComponentKind>().ok())
        .filter(|component| *component != platform::SchemaComponentKind::Migrator)
        .unwrap_or_else(|| {
            panic!("runtime schema verification requires api, worker, or retention component kind")
        });
    platform::connect_runtime_verified(component)
        .await
        .unwrap_or_else(|error| panic!("runtime schema verification failed: {error}"));
    tracing::info!(
        component = component.as_str(),
        "runtime schema verification complete"
    );
}
