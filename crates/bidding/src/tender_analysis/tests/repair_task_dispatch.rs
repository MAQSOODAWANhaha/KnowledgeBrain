use super::*;

struct FixtureScript(Script);

#[async_trait]
impl Model for FixtureScript {
    async fn turn(&self, config: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        let mut response = self.0.turn(config, body).await?;
        for call in &mut response.tool_calls {
            if call.name == "put_source_review" {
                // Follow the request's complete required-ID checklist; earlier
                // finding-save acknowledgments may already be compacted away.
                let body: Value = serde_json::from_slice(body).unwrap();
                let packet: Value = serde_json::from_str(
                    body["messages"].as_array().unwrap().last().unwrap()["content"]
                        .as_str()
                        .unwrap(),
                )
                .unwrap();
                let required =
                    &packet["source_review"]["current"]["completion"]["required_finding_ids"];
                assert_eq!(required["total"], required["next"]);
                let mut args: Value = serde_json::from_str(&call.arguments).unwrap();
                args["finding_ids"] = required["items"].clone();
                args["status"] = json!("findings");
                call.arguments = args.to_string();
            }
        }
        Ok(response)
    }
}

async fn steps(config: &Config, journal: &MemoryJournal, calls: Vec<(&str, Value)>) -> Checkpoint {
    let start = journal.load().await.unwrap().map_or(0, |s| s.turn);
    *journal.interrupt_after.lock().unwrap() = Some(start + calls.len());
    let error = agent::run(
        &input(),
        config,
        journal,
        &FixtureScript(work_script(calls)),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    journal.load().await.unwrap().unwrap()
}

async fn fixture() -> (MemoryJournal, Config) {
    let mut config = config();
    config.limits.max_turns = 100;
    config.limits.max_tool_calls = 300;
    config.limits.max_focus_turns = 4;
    config.limits.max_focus_replans = 2;
    let journal = MemoryJournal::default();
    let finding = |code| {
        json!({"id":null,"finding":{"code":code,"message":"Inspect this independent synthetic question.",
        "correction":"Compare the source before repairing or disputing this question.","affected":[],"sources":[span()]}})
    };
    let state = steps(&config, &journal, vec![
        ("set_work_note", active_work("source")),
        ("read_source", json!({"source_id":"source","start":0,"max_bytes":1024})),
        ("put_record", json!({"id":null,"sources":[span()],"data":{"kind":"fact","name":"fixture heading","value":"original","scope":"source"}})),
        ("set_disposition", json!({"source_id":"source","state":"non_requirement","reason":"synthetic fixture carrier"})),
        ("fixture_global_checks", json!({})),
        ("request_review", json!({})),
        ("set_work_note", active_work("source")),
        ("read_source", json!({"source_id":"source","start":0,"max_bytes":1024})),
        ("inspect_analysis", json!({"kind":"all","view":"detail","offset":0,"limit":10})),
        ("inspect_analysis", json!({"kind":"disposition","view":"detail","offset":0,"limit":10})),
        ("put_review_finding", finding("question-one")),
        ("put_review_finding", finding("question-two")),
        ("fixture_global_checks", json!({})),
        ("put_source_review", json!({"fixture_status":"findings"})),
    ]).await;
    assert_eq!(state.role, Role::Main, "{:#?}", state.transcript);
    assert_eq!(state.repair.tasks.entries.len(), 2);
    assert!(
        state
            .repair
            .tasks
            .entries
            .values()
            .all(|e| e.committed_turns == 0),
        "the completing Reviewer batch is not charged to a newly selected Main task"
    );
    assert!(state.main_progress.blockers.is_empty());
    (journal, config)
}

#[tokio::test]
async fn same_source_exhausted_finding_does_not_spend_its_independent_sibling_allowance() {
    let (journal, config) = fixture().await;
    let initial = journal.load().await.unwrap().unwrap();
    let first = initial.repair.tasks.active.unwrap();
    let cap = agent::repair::tasks::limit(&config.limits).unwrap();
    let state = steps(
        &config,
        &journal,
        (0..cap)
            .map(|_| ("check_gaps", json!({"scope":"work","offset":0,"limit":10})))
            .collect(),
    )
    .await;
    let second = state.repair.tasks.active.clone().unwrap();
    assert_ne!(first, second);
    assert_eq!(state.repair.tasks.entries[&first].committed_turns, cap);
    assert_eq!(
        state.repair.tasks.entries[&first].watch.recovery,
        crate::agent_runtime::progress::Recovery::Blocked
    );
    assert_eq!(state.repair.tasks.entries[&second].committed_turns, 0);
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["source"]
    );
    assert!(
        state.main_progress.blockers.is_empty(),
        "task exhaustion does not poison the whole source"
    );
    let first_spent = json!(state.repair.tasks.entries[&first]);
    *journal.state.lock().unwrap() = Some(serde_json::from_value(json!(state)).unwrap());
    let resumed = steps(
        &config,
        &journal,
        vec![("check_gaps", json!({"scope":"work","offset":0,"limit":10}))],
    )
    .await;
    assert_eq!(json!(resumed.repair.tasks.entries[&first]), first_spent);
    assert_eq!(resumed.repair.tasks.entries[&second].committed_turns, 1);
    let stopped = steps(
        &config,
        &journal,
        (1..cap)
            .map(|_| ("check_gaps", json!({"scope":"work","offset":0,"limit":10})))
            .collect(),
    )
    .await;
    assert!(stopped.repair.tasks.active.is_none());
    let before = digest(&stopped).unwrap();
    let unused_model = work_script(vec![]);
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &unused_model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "AGENT_TURN_BUDGET_EXCEEDED");
    assert!(unused_model.bodies.lock().unwrap().is_empty());
    assert_eq!(
        digest(&journal.load().await.unwrap().unwrap()).unwrap(),
        before
    );
}

struct Batch(Vec<(&'static str, Value)>);
#[async_trait]
impl Model for Batch {
    async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
        Ok(ChatTurn {
            finish_reason: "tool_calls".into(),
            tool_calls: self
                .0
                .iter()
                .enumerate()
                .map(|(i, (name, args))| ChatToolCall {
                    id: format!("batch-{i}"),
                    name: (*name).into(),
                    arguments: args.to_string(),
                })
                .collect(),
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn later_batch_write_invalidates_receipt_before_task_switch_and_handoff_does_not_rebill_it() {
    let (journal, config) = fixture().await;
    let ready = steps(
        &config,
        &journal,
        vec![
            ("inspect_review", json!({"offset":0,"limit":10})),
            (
                "inspect_analysis",
                json!({"kind":"all","view":"detail","offset":0,"limit":10}),
            ),
        ],
    )
    .await;
    let first = ready.repair.tasks.active.clone().unwrap();
    let first_sha = ready.repair.tasks.entries[&first].finding_sha256.clone();
    let record = ready.analysis.records.values().next().unwrap();
    let reference = format!("record:{}", record.id);
    let receipt = |sha| json!({"finding_sha256":sha,"conclusion":"disputed","summary":"The cited source supports this local outcome; request independent verification.","sources":[span()],"candidate_refs":[reference]});
    let mut changed = json!(record);
    changed["data"]["value"] = json!("source interpretation amended within the same response");
    let model = Batch(vec![
        ("put_repair_result", receipt(first_sha.clone())),
        ("put_record", changed),
    ]);
    *journal.interrupt_after.lock().unwrap() = Some(ready.turn + 1);
    agent::run(
        &input(),
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let stale = journal.load().await.unwrap().unwrap();
    assert_eq!(stale.turn, ready.turn + 1);
    assert_eq!(stale.tool_calls, ready.tool_calls + 2);
    assert_eq!(stale.repair.tasks.active.as_ref(), Some(&first));
    assert_eq!(
        stale.repair.tasks.entries[&first].committed_turns,
        ready.repair.tasks.entries[&first].committed_turns + 1
    );
    assert!(
        !agent::repair::tasks::current(&stale, &config.limits)
            .unwrap()
            .iter()
            .find(|t| t.id == first)
            .unwrap()
            .complete
    );
    let next = steps(
        &config,
        &journal,
        vec![
            (
                "inspect_analysis",
                json!({"kind":"all","view":"detail","offset":0,"limit":10}),
            ),
            ("put_repair_result", receipt(first_sha)),
        ],
    )
    .await;
    let second = next.repair.tasks.active.clone().unwrap();
    assert_ne!(first, second);
    assert_eq!(next.repair.tasks.entries[&second].committed_turns, 0);
    let model = Batch(vec![
        (
            "put_repair_result",
            receipt(next.repair.tasks.entries[&second].finding_sha256.clone()),
        ),
        ("request_review", json!({})),
    ]);
    *journal.interrupt_after.lock().unwrap() = Some(next.turn + 1);
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    let rejected = journal.load().await.unwrap().unwrap();
    assert_eq!(rejected.turn, next.turn + 1);
    assert_eq!(rejected.tool_calls, next.tool_calls + 2);
    assert_eq!(rejected.role, Role::Main);
    assert_eq!(rejected.repair.tasks.active.as_ref(), Some(&second));
    let results = rejected
        .transcript
        .iter()
        .rev()
        .take(2)
        .map(|message| serde_json::from_str::<Value>(message["content"].as_str().unwrap()).unwrap())
        .collect::<Vec<_>>();
    assert!(results.iter().all(|result| result["ok"] == false
        && result["error"] == "review transition must be the only tool call in its turn"));
    assert_eq!(json!(rejected.analysis), json!(next.analysis));
    assert_eq!(json!(rejected.repair.results), json!(next.repair.results));
    assert_eq!(
        rejected.repair.tasks.entries[&second].committed_turns,
        next.repair.tasks.entries[&second].committed_turns + 1,
        "an invalid transition batch still consumes exactly one original task turn"
    );
    assert_eq!(
        json!(rejected.repair.tasks.entries[&first]),
        json!(next.repair.tasks.entries[&first]),
        "the completed sibling is not charged by the other task's rejected handoff"
    );
    let completed = steps(
        &config,
        &journal,
        vec![(
            "put_repair_result",
            receipt(next.repair.tasks.entries[&second].finding_sha256.clone()),
        )],
    )
    .await;
    assert!(completed.repair.tasks.active.is_none());
    assert_eq!(
        completed.repair.tasks.entries[&second].committed_turns,
        rejected.repair.tasks.entries[&second].committed_turns + 1
    );
    let completed_tasks = json!(completed.repair.tasks);
    let reviewer = steps(
        &config,
        &journal,
        vec![
            ("fixture_global_checks", json!({})),
            ("request_review", json!({})),
        ],
    )
    .await;
    assert_eq!(reviewer.role, Role::Reviewer, "{:#?}", reviewer.transcript);
    assert!(reviewer.repair.tasks.active.is_none());
    assert_eq!(
        json!(reviewer.repair.tasks),
        completed_tasks,
        "the legal separate handoff cannot charge an already completed Main task"
    );
    let spent = json!(reviewer.repair.tasks);
    *journal.state.lock().unwrap() = Some(serde_json::from_value(json!(reviewer)).unwrap());
    let resumed = steps(
        &config,
        &journal,
        vec![("inspect_review", json!({"offset":0,"limit":10}))],
    )
    .await;
    assert_eq!(resumed.role, Role::Reviewer);
    assert_eq!(
        json!(resumed.repair.tasks),
        spent,
        "a resumed Reviewer query must not retain or charge the completed Main task"
    );
}
