//! Pixel delivery/identity contracts; scripted judgments do not establish image semantics.
use super::*;
use crate::export_review::OutputImage;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Pixels {
    bytes: Vec<u8>,
    reads: AtomicUsize,
}
#[async_trait]
impl visual::ImageReader for Pixels {
    async fn read(&self, _: &OutputImage, _: &CancellationToken) -> Result<Vec<u8>, AgentError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.bytes.clone())
    }
}

fn png() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn fixture() -> (FrozenInput, Checkpoint, Value, Pixels) {
    let (input, mut state, args) = evidence_fixture();
    let bytes = png();
    let sha = hex::encode(Sha256::digest(&bytes));
    state.inventory.images.insert(
        sha.clone(),
        OutputImage {
            sha256: sha.clone(),
            object_ref: format!("objects/{sha}"),
            media_type: "image/png".into(),
            byte_length: bytes.len() as u64,
            width: 2,
            height: 2,
        },
    );
    let unit = &mut state.inventory.units[0];
    unit.kind = "not_checked".into();
    unit.not_checked_reason = Some("visual review required".into());
    unit.image_sha256s = vec![sha];
    unit.source_unit = Some(json!({"kind":"image_region"}));
    unit.content_sha256 = digest(&unit.source_unit).unwrap();
    (
        input,
        state,
        args,
        Pixels {
            bytes,
            reads: AtomicUsize::new(0),
        },
    )
}

fn turn(calls: Vec<(&str, Value)>) -> ChatTurn {
    ChatTurn {
        usage: None,
        content: String::new(),
        finish_reason: "tool_calls".into(),
        tool_calls: calls
            .into_iter()
            .enumerate()
            .map(|(n, (name, args))| knowledge::models::ChatToolCall {
                id: format!("visual-{n}"),
                name: name.into(),
                arguments: args.to_string(),
            })
            .collect(),
    }
}

async fn view_batch(input: &FrozenInput, state: &mut Checkpoint, args: &Value) {
    let response = turn(vec![
        ("read_output_view", json!({"id":args["item_id"]})),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":100}),
        ),
        ("put_composition_review", args.clone()),
    ]);
    let results = execute_turn(
        input,
        &config(),
        state,
        response,
        BTreeMap::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(
        results[2]["content"]
            .as_str()
            .unwrap()
            .contains("\"ok\":false")
    );
    assert!(state.output_coverage.views.is_empty());
    assert!(state.reviews.is_empty());
}

#[tokio::test]
async fn output_pixels_require_exact_received_body_not_metadata_or_same_batch_read() {
    let (input, mut state, args, pixels) = fixture();
    deliver_output(&mut state);
    deliver_tender(&input, &mut state);
    assert!(
        put_review(&input, &mut state, &args).is_err(),
        "text extraction is not visual evidence"
    );
    view_batch(&input, &mut state, &args).await;
    let body = super::super::request(
        &mut state,
        &config(),
        &BTreeMap::new(),
        &pixels,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(pixels.reads.load(Ordering::SeqCst), 1);
    let request: Value = serde_json::from_slice(&body).unwrap();
    assert!(request["messages"].as_array().unwrap().iter().any(|m| {
        m["content"].as_array().is_some_and(|parts| {
            parts.iter().any(|p| {
                p["image_url"]["url"]
                    == format!("data:image/png;base64,{}", STANDARD.encode(&pixels.bytes))
            })
        })
    }));
    state
        .journal
        .prepare(state.turn, "reviewer", &body)
        .unwrap();
    assert!(delivery::accept_pending(&mut state, &BTreeMap::new()).is_err());
    state
        .journal
        .responded(turn(vec![("put_composition_review", args.clone())]))
        .unwrap();
    let original = state.journal.pending.as_ref().unwrap().body.clone();
    let mut changed = request.clone();
    changed["messages"]
        .as_array_mut()
        .unwrap()
        .retain(|m| !m["content"].is_array());
    state.journal.pending.as_mut().unwrap().body = changed.to_string();
    assert!(delivery::accept_pending(&mut state, &BTreeMap::new()).is_err());
    assert!(state.output_coverage.views.is_empty());
    state.journal.pending.as_mut().unwrap().body = original;
    delivery::accept_pending(&mut state, &BTreeMap::new()).unwrap();
    put_review(&input, &mut state, &args).unwrap();
    state.done = true;
    assert_eq!(state.completed_report(&input).unwrap().status, "reviewed");
    state.output_coverage.views.clear();
    assert!(
        state.completed_report(&input).is_none(),
        "cached done cannot bypass visual receipts"
    );
}

#[tokio::test]
async fn changed_object_bytes_or_dimensions_never_enter_a_model_request() {
    let (input, mut state, args, mut pixels) = fixture();
    view_batch(&input, &mut state, &args).await;
    pixels.bytes.push(0);
    let error = super::super::request(
        &mut state,
        &config(),
        &BTreeMap::new(),
        &pixels,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "FROZEN_INPUT_DIGEST_MISMATCH");
    assert!(state.output_coverage.views.is_empty());
    pixels.bytes.pop();
    state.inventory.images.values_mut().next().unwrap().width += 1;
    assert!(
        super::super::request(
            &mut state,
            &config(),
            &BTreeMap::new(),
            &pixels,
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn encoded_pixels_count_toward_read_budget_and_rejected_reads_grant_nothing() {
    let (input, mut state, args, _) = fixture();
    let (value, _, pixel_bytes) = visual::read_output(
        &state,
        &mut OutputCoverage::default(),
        &json!({"id":args["item_id"]}),
    )
    .unwrap();
    let mut limits = config();
    limits.limits.max_read_bytes = serde_json::to_vec(&value).unwrap().len() + pixel_bytes - 1;
    let results = execute_turn(
        &input,
        &limits,
        &mut state,
        turn(vec![("read_output_view", json!({"id":args["item_id"]}))]),
        BTreeMap::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(
        results[0]["content"]
            .as_str()
            .unwrap()
            .contains("remaining read budget")
    );
    assert_eq!(state.read_bytes, 0);
    assert!(state.pending_delivery.is_none());
    assert!(state.output_coverage.views.is_empty());
    assert!(
        state
            .transcript
            .iter()
            .all(|entry| entry.get("export_view_refs").is_none())
    );
}

#[test]
fn delivered_neighbor_pixels_cannot_approve_unsupported_field_or_textbox() {
    let (input, mut state, args, _) = fixture();
    deliver_tender(&input, &mut state);
    deliver_output(&mut state);
    for (sha, image) in &state.inventory.images {
        state
            .output_coverage
            .views
            .insert(sha.clone(), digest(image).unwrap());
    }
    state.inventory.units[0].source_unit =
        Some(json!({"kind":"attachment_region","locator":{"part_name":"field"}}));
    assert!(put_review(&input, &mut state, &args).is_err());
}

#[tokio::test]
async fn original_source_pixels_have_independent_received_receipts() {
    use crate::tender_analysis::views::{SourceView, ViewIdentity};
    let (input, mut state, mut args) = evidence_fixture();
    deliver_output(&mut state);
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut bytes, image::ImageFormat::Jpeg)
        .unwrap();
    let bytes = bytes.into_inner();
    let view = SourceView {
        identity: ViewIdentity {
            source_id: "source".into(),
            original_sha256: "e".repeat(64),
            image_sha256: hex::encode(Sha256::digest(&bytes)),
            page_ordinal: 0,
            width: 2,
            height: 2,
            renderer: "docreader-source-view-v1/test".into(),
        },
        jpeg_base64: STANDARD.encode(bytes),
    };
    let id = view.id().unwrap();
    let views = BTreeMap::from([(id.clone(), view)]);
    args["grounds"] = json!([{"source_id":"source","start":0,"end":0,"view_id":id}]);
    let response = turn(vec![
        ("read_source_view", json!({"source_id":"source"})),
        ("put_composition_review", args.clone()),
    ]);
    super::super::execute_turn(
        &input,
        &config(),
        &mut state,
        response,
        BTreeMap::new(),
        &views,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(state.reviews.is_empty());
    assert!(state.tender_coverage.views.is_empty());
    let body = super::super::request(
        &mut state,
        &config(),
        &views,
        &NoImages,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    state
        .journal
        .prepare(state.turn, "reviewer", &body)
        .unwrap();
    state
        .journal
        .responded(turn(vec![("put_composition_review", args.clone())]))
        .unwrap();
    delivery::accept_pending(&mut state, &views).unwrap();
    assert!(state.output_coverage.views.is_empty());
    put_review(&input, &mut state, &args).unwrap();
}

struct BatchScript {
    turns: Mutex<VecDeque<ChatTurn>>,
    calls: AtomicUsize,
}
#[async_trait]
impl Model for BatchScript {
    async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .turns
            .lock()
            .unwrap()
            .pop_front()
            .expect("script exhausted"))
    }
}
#[derive(Default)]
struct LostReceived {
    memory: Memory,
    lost: Mutex<bool>,
}
#[async_trait]
impl Journal for LostReceived {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        self.memory.load().await
    }
    async fn reserve(&self, state: &Checkpoint, body: &[u8]) -> Result<usize, AgentError> {
        self.memory.reserve(state, body).await
    }
    async fn save(&self, state: &Checkpoint) -> Result<(), AgentError> {
        self.memory.save(state).await?;
        let mut lost = self.lost.lock().unwrap();
        if state.turn == 1 && state.journal.response().is_some() && !*lost {
            *lost = true;
            return Err(AgentError::new("INTERNAL", "lost visual received ACK"));
        }
        Ok(())
    }
}

#[tokio::test]
async fn received_replay_reuses_exact_pixels_without_another_object_or_model_call() {
    let (input, state, args, pixels) = fixture();
    let result = analysis_result(&input);
    let model = BatchScript {
        calls: AtomicUsize::new(0),
        turns: Mutex::new(VecDeque::from([
            turn(vec![
                ("read_output_view", json!({"id":args["item_id"]})),
                (
                    "read_source",
                    json!({"source_id":"source","start":0,"max_bytes":100}),
                ),
            ]),
            turn(vec![("put_composition_review", args)]),
        ])),
    };
    let journal = LostReceived::default();
    let bytes = docx();
    let files = || FrozenFiles {
        docx: &bytes,
        pdf: None,
        inventory: &state.inventory,
        images: &pixels,
    };
    assert_eq!(
        run(
            &input,
            &result,
            files(),
            &config(),
            &journal,
            &model,
            &CancellationToken::new()
        )
        .await
        .unwrap_err()
        .code,
        "INTERNAL"
    );
    assert_eq!(pixels.reads.load(Ordering::SeqCst), 1);
    let saved = journal.load().await.unwrap().unwrap();
    assert!(saved.journal.response().is_some());
    assert!(saved.output_coverage.views.is_empty());
    let report = run(
        &input,
        &result,
        files(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(report.status, "reviewed");
    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
    assert_eq!(pixels.reads.load(Ordering::SeqCst), 1);
}
