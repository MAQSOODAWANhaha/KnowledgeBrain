//! Text storage and external JSON share the domain's exact status vocabulary.
use knowledge::*;

#[tokio::test]
async fn status_values_round_trip_through_text_without_unknown_fallbacks() {
    let Ok(url) = std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL") else {
        assert!(std::env::var_os("KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS").is_none());
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    macro_rules! check {
        ($ty:ty, [$($value:expr),+]) => {{
            for value in [$($value),+] {
                let wire = value.as_str();
                assert_eq!(serde_json::to_value(value).unwrap(), serde_json::json!(wire));
                assert_eq!(serde_json::from_value::<$ty>(serde_json::json!(wire)).unwrap(), value);
                let decoded = sqlx::query_scalar::<_, $ty>("SELECT $1::text")
                    .bind(value).fetch_one(&pool).await.unwrap();
                assert_eq!(decoded, value);
            }
            assert!(serde_json::from_value::<$ty>(serde_json::json!("unknown")).is_err());
            let decoded = sqlx::query_scalar::<_, $ty>("SELECT $1::text")
                .bind("unknown").fetch_one(&pool).await;
            assert!(matches!(decoded, Err(sqlx::Error::ColumnDecode { .. })));
        }};
    }
    check!(
        ParseStatus,
        [
            ParseStatus::Pending,
            ParseStatus::Processing,
            ParseStatus::Finalizing,
            ParseStatus::Completed,
            ParseStatus::Failed,
            ParseStatus::Cancelled,
            ParseStatus::Deleting,
            ParseStatus::Deleted
        ]
    );
    check!(
        SummaryStatus,
        [
            SummaryStatus::None,
            SummaryStatus::Pending,
            SummaryStatus::Processing,
            SummaryStatus::Completed,
            SummaryStatus::Failed
        ]
    );
    check!(
        VersionStatus,
        [
            VersionStatus::Cloning,
            VersionStatus::Active,
            VersionStatus::Archived,
            VersionStatus::Failed
        ]
    );
    check!(
        EnableStatus,
        [EnableStatus::Disabled, EnableStatus::Enabled]
    );
    check!(
        WikiStatus,
        [
            WikiStatus::Draft,
            WikiStatus::Published,
            WikiStatus::Archived
        ]
    );
    check!(
        OutboxState,
        [
            OutboxState::Pending,
            OutboxState::Published,
            OutboxState::Complete,
            OutboxState::Failed
        ]
    );
    check!(
        SemanticIndexIntentStatus,
        [
            SemanticIndexIntentStatus::Pending,
            SemanticIndexIntentStatus::Completed,
            SemanticIndexIntentStatus::Terminal,
            SemanticIndexIntentStatus::Superseded
        ]
    );
    check!(
        SupportState,
        [SupportState::Supported, SupportState::Revoked]
    );
    check!(
        SpanStatus,
        [
            SpanStatus::Pending,
            SpanStatus::Running,
            SpanStatus::Done,
            SpanStatus::Failed,
            SpanStatus::Skipped,
            SpanStatus::Cancelled
        ]
    );
}
