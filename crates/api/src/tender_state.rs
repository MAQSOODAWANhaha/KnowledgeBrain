use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub(crate) enum ProjectStatus {
    Open,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub(crate) enum EditorMode {
    View,
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub(crate) enum SaveState {
    Pending,
    Saved,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tender_states_preserve_wire_values_and_reject_unknown_values() {
        for (status, wire) in [
            (ProjectStatus::Open, "open"),
            (ProjectStatus::Ended, "ended"),
        ] {
            assert_eq!(serde_json::to_value(status).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<ProjectStatus>(json!(wire)).unwrap(),
                status
            );
        }
        for (mode, wire) in [(EditorMode::View, "view"), (EditorMode::Edit, "edit")] {
            assert_eq!(serde_json::to_value(mode).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<EditorMode>(json!(wire)).unwrap(),
                mode
            );
        }
        for (state, wire) in [
            (SaveState::Pending, "pending"),
            (SaveState::Saved, "saved"),
            (SaveState::Failed, "failed"),
        ] {
            assert_eq!(serde_json::to_value(state).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<SaveState>(json!(wire)).unwrap(),
                state
            );
        }
        assert!(serde_json::from_value::<ProjectStatus>(json!("pending")).is_err());
        assert!(serde_json::from_value::<EditorMode>(json!("admin")).is_err());
        assert!(serde_json::from_value::<SaveState>(json!("completed")).is_err());
    }
}
