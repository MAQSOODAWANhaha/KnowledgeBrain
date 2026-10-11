use super::*;
use std::sync::Mutex;

struct RecoveryJournal {
    state: Mutex<Checkpoint>,
    saves: Mutex<usize>,
}
#[async_trait]
impl Journal for RecoveryJournal {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(Some(self.state.lock().unwrap().clone()))
    }
    async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
        panic!("recovery must persist before any reservation")
    }
    async fn save(&self, state: &Checkpoint, _: &Value) -> Result<(), AgentError> {
        state.journal.validate(state.turn, "main")?;
        *self.saves.lock().unwrap() += 1;
        *self.state.lock().unwrap() =
            serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        // An unavailable persistence boundary must prevent admission/dispatch.
        Err(error("TEST_RECOVERY_SAVE_STOP", "synthetic journal stop"))
    }
}
struct NeverDispatch;
#[async_trait]
impl Model for NeverDispatch {
    async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
        panic!("recovery must be durably saved before any model dispatch")
    }
}

#[tokio::test]
async fn recovery_saves_before_dispatch_and_never_rewrites_sent_checkpoint() {
    let input =
        token_transport_validation::synthetic_input_with_text("Synthetic declaration.".into(), 1);
    let config = crate::analysis::tests::config();
    for attempts in [0, 1] {
        let mut state = retirement::checkpoint(&input);
        state.turn = 0;
        state.config_sha256 = digest(&config).unwrap();
        state.journal = Default::default();
        let body = br#"{"messages":[{"content":"synthetic","role":"system"}]}"#;
        state.journal.prepare_session(body, 1, 0, 4096).unwrap();
        state.journal.prepare(0, "main", body).unwrap();
        state.journal.pending.as_mut().unwrap().body =
            r#"{ "messages": [{ "role": "system", "content": "synthetic" }] }"#.into();
        state.journal.pending.as_mut().unwrap().physical_attempts = attempts;
        let old_body = state.journal.pending.as_ref().unwrap().body.clone();
        let journal = RecoveryJournal {
            state: Mutex::new(state),
            saves: Mutex::new(0),
        };
        let failure = run(
            &input,
            &config,
            &journal,
            &NeverDispatch,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        if attempts == 0 {
            assert_eq!(failure.code, "TEST_RECOVERY_SAVE_STOP");
            assert_eq!(*journal.saves.lock().unwrap(), 1);
            assert_eq!(journal.state.lock().unwrap().journal.body().unwrap(), body);
        } else {
            assert_eq!(failure.code, "AGENT_CHECKPOINT_NONCANONICAL");
            assert_eq!(*journal.saves.lock().unwrap(), 0);
            assert_eq!(
                journal
                    .state
                    .lock()
                    .unwrap()
                    .journal
                    .pending
                    .as_ref()
                    .unwrap()
                    .body,
                old_body
            );
        }
    }
}
