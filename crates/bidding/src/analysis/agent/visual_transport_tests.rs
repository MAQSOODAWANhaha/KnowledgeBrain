//! Real request construction and journalled model consumption, without network.
use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct MemoryJournal {
    state: Mutex<Checkpoint>,
}
#[async_trait]
impl Journal for MemoryJournal {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(Some(self.state.lock().unwrap().clone()))
    }
    async fn reserve(&self, state: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
        *self.state.lock().unwrap() = state.clone();
        Ok(Some(1))
    }
    async fn save(&self, state: &Checkpoint, _: &Value) -> Result<(), AgentError> {
        *self.state.lock().unwrap() = state.clone();
        Ok(())
    }
}
struct PixelCheckingModel {
    calls: AtomicUsize,
    expected_urls: BTreeSet<String>,
}
#[async_trait]
impl Model for PixelCheckingModel {
    async fn turn(&self, config: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
            return Err(error(
                "AGENT_TRANSPORT_INTERRUPTED",
                "test stops after the image-consuming response",
            ));
        }
        assert_eq!(config.provider.model_id, "test-frozen-model");
        let body: Value = serde_json::from_slice(body).unwrap();
        let messages = body["messages"].as_array().unwrap();
        let urls = messages
            .iter()
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .filter(|part| part["type"] == "image_url")
            .map(|part| part["image_url"]["url"].as_str().unwrap().to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            urls, self.expected_urls,
            "the actual provider request must contain original image pixels, not image_ref metadata"
        );
        let packet = messages
            .iter()
            .filter_map(|message| message["content"].as_str())
            .filter_map(|text| serde_json::from_str::<Value>(text).ok())
            .find(|packet| packet["reading_packs"].is_array())
            .unwrap();
        let calls=packet["reading_packs"].as_array().unwrap().iter().map(|session|{
            let pack=&session["pack"];
            knowledge::models::ChatToolCall{id:format!("scan-{}",pack["id"].as_str().unwrap()),name:"submit_pack".into(),arguments:json!({
                "wire_scope":serde_json::from_str::<Value>(messages.last().unwrap()["content"].as_str().unwrap()).unwrap()["wire_scope"],"pack_id":pack["id"],"call_id":session["submission_operation_id"],"claim_token":pack["claim_token"],"pack_revision":pack["pack_revision"],"repair":false,"requirements":[],"no_requirement_reason":"Visual test fixture reviewed; no bidder response obligation.","inspected_atom_ids":pack["atoms"].as_array().unwrap().iter().map(|atom|json!({"atom_key":atom["atom_key"]})).collect::<Vec<_>>()
            }).to_string()}
        }).collect();
        Ok(ChatTurn {
            tool_calls: calls,
            finish_reason: "tool_calls".into(),
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn original_pixels_reach_configured_model_before_discovery_can_commit() {
    let input = crate::outline::frozen::tests::python_fixture_input(0);
    let base = crate::analysis::tests::config();
    let limits = base.limits;
    let config = Config::with_provider(base.provider, limits).unwrap();
    let mut state = super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    let rows: Value = serde_json::from_str(include_str!(
        "../../../../docparser/tests/fixtures/python-source-contract-v2.json"
    ))
    .unwrap();
    let mut view_ids = Vec::new();
    let mut expected_urls = BTreeSet::new();
    for source in input
        .source_units
        .iter()
        .filter(|source| source.locator["image_available"] == true)
    {
        let image = rows[0]["images"]
            .as_array()
            .unwrap()
            .iter()
            .find(|image| image["original_ref"] == source.locator["original_ref"])
            .unwrap();
        let bytes = hex::decode(image["hex"].as_str().unwrap()).unwrap();
        let original = source.locator["image_ref"]
            .as_str()
            .unwrap()
            .strip_prefix("objects/")
            .unwrap();
        let view = crate::outline::visual::render_frozen_image_view(
            &source.source_unit_revision_id,
            original,
            source.locator["page_ordinal"].as_u64().unwrap() as u32,
            &bytes,
            128,
            8192,
        )
        .unwrap();
        expected_urls.insert(format!("data:image/jpeg;base64,{}", view.jpeg_base64));
        let id = view.id().unwrap();
        view_ids.push(id.clone());
        state.source_views.insert(id, view);
    }
    // A recovered cache and pending transport reference have no visual receipt.
    state
        .transcript
        .push(json!({"role":"user","source_view_refs":view_ids}));
    let journal = MemoryJournal {
        state: Mutex::new(state),
    };
    let model = PixelCheckingModel {
        calls: AtomicUsize::new(0),
        expected_urls,
    };
    let stopped = run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        stopped.code, "AGENT_TRANSPORT_INTERRUPTED",
        "{}",
        stopped.message
    );
    let saved = journal.state.lock().unwrap();
    let work = saved.outline_run.reading_packs.as_ref().unwrap();
    assert!(work.complete());
    assert_eq!(work.requirement_count(), 0);
    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
}
