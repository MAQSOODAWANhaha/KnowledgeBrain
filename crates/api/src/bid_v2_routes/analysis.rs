//! Read the complete frozen analysis, independently of the legacy requirements projection.
use super::*;
use axum::extract::{Query, rejection::QueryRejection};

pub(super) fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v2/bid-projects/{project_id}/requirement-sets/{requirement_set_id}/analysis",
        get(read),
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalysisQuery {
    kind: String,
    offset: i32,
    limit: i32,
}

impl AnalysisQuery {
    fn validate(&self) -> Result<(), ApiErr> {
        if !matches!(
            self.kind.as_str(),
            "all"
                | "fact"
                | "rule"
                | "requirement"
                | "template"
                | "unresolved"
                | "relation"
                | "disposition"
                | "finding"
        ) || self.offset < 0
            || !(1..=100).contains(&self.limit)
            || self.offset.checked_add(self.limit).is_none()
        {
            return Err(validation("invalid analysis kind or page"));
        }
        Ok(())
    }
}

async fn read(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, requirement_set)): Path<(Uuid, Uuid)>,
    query: Result<Query<AnalysisQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let Query(query) = query.map_err(|error| validation(&error.to_string()))?;
    query.validate()?;
    let pool = require_bid_pool().await?;
    let page: Option<Value> = sqlx::query_scalar(
        "SELECT kb_bid_v2_get_tender_analysis($1,$2,$3::kb_actor_identity,$4,$5,$6)",
    )
    .bind(project)
    .bind(requirement_set)
    .bind(actor)
    .bind(query.kind)
    .bind(query.offset)
    .bind(query.limit)
    .fetch_one(&pool)
    .await
    .map_err(map_sql)?;
    page.map(Json)
        .ok_or_else(|| not_found("frozen tender analysis"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Uri;
    use tower::ServiceExt;

    fn parse(query: &str) -> Result<AnalysisQuery, ApiErr> {
        let uri: Uri = format!("/analysis?{query}").parse().unwrap();
        let Query(query) = Query::<AnalysisQuery>::try_from_uri(&uri)
            .map_err(|error| validation(&error.to_string()))?;
        query.validate()?;
        Ok(query)
    }

    #[test]
    fn explicit_pages_support_every_complete_analysis_collection() {
        for kind in [
            "all",
            "fact",
            "rule",
            "requirement",
            "template",
            "unresolved",
            "relation",
            "disposition",
            "finding",
        ] {
            for (offset, limit) in [(0, 1), (37, 100), (i32::MAX - 1, 1)] {
                let parsed = parse(&format!("kind={kind}&offset={offset}&limit={limit}"))
                    .unwrap_or_else(|_| panic!("existing SQL page contract"));
                assert_eq!(parsed.kind, kind);
                assert_eq!(parsed.offset, offset);
                assert_eq!(parsed.limit, limit);
            }
        }
    }

    #[test]
    fn malformed_or_ambiguous_analysis_queries_are_rejected() {
        for query in [
            "kind=all&offset=-1&limit=10",
            "kind=all&offset=0&limit=0",
            "kind=all&offset=0&limit=101",
            "kind=all&offset=2147483648&limit=1",
            "kind=all&offset=2147483647&limit=1",
            "kind=all&offset=0&limit=text",
            "kind=all&offset=0",
            "kind=all&offset=0&limit=1&kind=template",
            "kind=all&offset=0&limit=1&project_id=other",
            "kind=legacy&offset=0&limit=1",
        ] {
            let error = parse(query).expect_err(query);
            assert_eq!(error.0, StatusCode::BAD_REQUEST, "{query}");
        }
    }

    #[tokio::test]
    async fn complete_analysis_route_requires_authentication_before_database_access() {
        let app = router().with_state(AppState {
            jwt_secret: "analysis-route-test".into(),
            bootstrap_key: String::new(),
        });
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v2/bid-projects/{}/requirement-sets/{}/analysis?kind=all&offset=0&limit=10",
                        Uuid::new_v4(),
                        Uuid::new_v4()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
