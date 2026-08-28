use super::semantic_v2::{EmbeddingCredentialResolverV2, SemanticCandidateV2};
use crate::knowledge_retrieval::{KnowledgeRetrievalError, RerankRevisionV2, RetrievalPolicyV2};
use async_trait::async_trait;
use rust_decimal::{Decimal, RoundingStrategy, prelude::ToPrimitive};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{cmp::Ordering, str::FromStr, sync::Arc, time::Duration};

#[derive(Clone)]
pub(crate) struct StrictRerankClientV2 {
    transport: Arc<dyn RerankTransportV2>,
    credentials: Arc<dyn EmbeddingCredentialResolverV2>,
}

#[derive(Clone)]
struct RerankTransportRequestV2 {
    endpoint: String,
    timeout: Duration,
    bearer_token: String,
    model: String,
    query: String,
    documents: Vec<String>,
}

struct RerankTransportResponseV2 {
    status: reqwest::StatusCode,
    bytes: Vec<u8>,
}

#[async_trait]
trait RerankTransportV2: Send + Sync {
    async fn send(
        &self,
        request: RerankTransportRequestV2,
    ) -> Result<RerankTransportResponseV2, KnowledgeRetrievalError>;
}

struct ReqwestRerankTransportV2 {
    http: reqwest::Client,
}

#[async_trait]
impl RerankTransportV2 for ReqwestRerankTransportV2 {
    async fn send(
        &self,
        request: RerankTransportRequestV2,
    ) -> Result<RerankTransportResponseV2, KnowledgeRetrievalError> {
        let response = self
            .http
            .post(request.endpoint)
            .timeout(request.timeout)
            .bearer_auth(request.bearer_token)
            .json(&RerankRequestV2 {
                model: &request.model,
                query: &request.query,
                documents: request.documents.iter().map(String::as_str).collect(),
            })
            .send()
            .await
            .map_err(|error| unavailable(&format!("rerank provider request failed: {error}")))?;
        let status = response.status();
        if !status.is_success() {
            return Ok(RerankTransportResponseV2 {
                status,
                bytes: Vec::new(),
            });
        }
        let response = super::http_v2::read_bounded_response_body_v2(
            response,
            super::http_v2::STRICT_V2_MAX_RESPONSE_BYTES,
        )
        .await
        .map_err(|error| match error {
            super::http_v2::BoundedBodyErrorV2::TooLarge => {
                unavailable("rerank provider response exceeds byte limit")
            }
            super::http_v2::BoundedBodyErrorV2::Transport(error) => {
                unavailable(&format!("rerank provider response failed: {error}"))
            }
        })?;
        Ok(RerankTransportResponseV2 {
            status: response.status,
            bytes: response.bytes,
        })
    }
}

impl StrictRerankClientV2 {
    pub(crate) fn new(
        credentials: Arc<dyn EmbeddingCredentialResolverV2>,
    ) -> Result<Self, KnowledgeRetrievalError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| unavailable(&format!("failed to configure rerank client: {error}")))?;
        Ok(Self {
            transport: Arc::new(ReqwestRerankTransportV2 { http }),
            credentials,
        })
    }

    #[cfg(test)]
    fn with_transport(
        credentials: Arc<dyn EmbeddingCredentialResolverV2>,
        transport: Arc<dyn RerankTransportV2>,
    ) -> Self {
        Self {
            transport,
            credentials,
        }
    }

    pub(crate) async fn rerank(
        &self,
        requirement_text: &str,
        candidates: Vec<SemanticCandidateV2>,
        policy: &RetrievalPolicyV2,
        revision: &RerankRevisionV2,
        credential_ref: &str,
    ) -> Result<Vec<RerankedSemanticCandidateV2>, KnowledgeRetrievalError> {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        if !revision.endpoint_identity.starts_with("https://") {
            return Err(invalid("rerank endpoint must use https"));
        }
        if revision.sha256().ok().as_deref() != Some(policy.rerank.revision_sha256.as_str())
            || revision.provider_model_revision_sha256 != policy.rerank.model_revision_sha256
            || revision.config_revision_sha256 != policy.rerank.config_revision_sha256
        {
            return Err(invalid("rerank revision does not match policy"));
        }
        let credential = self.credentials.resolve(credential_ref).await?;
        if credential.is_empty() {
            return Err(invalid("rerank credential is empty"));
        }
        let response = self
            .transport
            .send(RerankTransportRequestV2 {
                endpoint: revision.endpoint_identity.clone(),
                timeout: Duration::from_millis(u64::from(policy.rerank.timeout_ms)),
                bearer_token: credential,
                model: revision.provider_model_identifier.clone(),
                query: requirement_text.to_owned(),
                documents: candidates
                    .iter()
                    .map(|candidate| candidate.chunk_utf8.clone())
                    .collect(),
            })
            .await?;
        if !response.status.is_success() {
            return Err(status_error(response.status));
        }
        apply_response(&response.bytes, candidates, policy, revision)
    }
}

#[derive(Serialize)]
struct RerankRequestV2<'a> {
    model: &'a str,
    query: &'a str,
    documents: Vec<&'a str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RerankResponseV2 {
    model_revision_sha256: String,
    config_revision_sha256: String,
    results: Vec<RerankResultV2>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RerankResultV2 {
    index: usize,
    score: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RerankedSemanticCandidateV2 {
    pub(crate) candidate: SemanticCandidateV2,
    pub(crate) score_millionths: u32,
}

impl RerankedSemanticCandidateV2 {
    pub(crate) fn fixed_score(&self) -> String {
        format!(
            "{}.{:06}",
            self.score_millionths / 1_000_000,
            self.score_millionths % 1_000_000
        )
    }
}

fn apply_response(
    bytes: &[u8],
    candidates: Vec<SemanticCandidateV2>,
    policy: &RetrievalPolicyV2,
    revision: &RerankRevisionV2,
) -> Result<Vec<RerankedSemanticCandidateV2>, KnowledgeRetrievalError> {
    let response: RerankResponseV2 = serde_json::from_slice(bytes)
        .map_err(|error| unavailable(&format!("invalid rerank provider JSON: {error}")))?;
    if revision.sha256().ok().as_deref() != Some(policy.rerank.revision_sha256.as_str())
        || response.model_revision_sha256 != policy.rerank.model_revision_sha256
        || response.model_revision_sha256 != revision.provider_model_revision_sha256
        || response.config_revision_sha256 != policy.rerank.config_revision_sha256
        || response.config_revision_sha256 != revision.config_revision_sha256
    {
        return Err(invalid("rerank provider identity mismatch"));
    }
    if response.results.len() != candidates.len() {
        return Err(unavailable(
            "rerank response does not cover every input index",
        ));
    }
    let mut scores = vec![None; candidates.len()];
    for result in response.results {
        if result.index >= candidates.len() || scores[result.index].is_some() {
            return Err(unavailable(
                "rerank response index is duplicate or out of range",
            ));
        }
        scores[result.index] = Some(score_millionths(&result.score)?);
    }
    if scores.iter().any(Option::is_none) {
        return Err(unavailable("rerank response is missing an input index"));
    }
    let mut output = candidates
        .into_iter()
        .zip(scores)
        .map(|(candidate, score)| RerankedSemanticCandidateV2 {
            candidate,
            score_millionths: score.expect("complete index coverage checked"),
        })
        .collect::<Vec<_>>();
    output.sort_by(compare_reranked);
    Ok(output)
}

fn score_millionths(value: &Value) -> Result<u32, KnowledgeRetrievalError> {
    let number = value
        .as_number()
        .ok_or_else(|| unavailable("rerank score is not a JSON number"))?;
    let decimal = Decimal::from_str(&number.to_string())
        .map_err(|_| unavailable("rerank score is not finite decimal"))?;
    if decimal < Decimal::ZERO || decimal > Decimal::ONE {
        return Err(unavailable("rerank score is outside unit interval"));
    }
    let quantized = decimal.round_dp_with_strategy(6, RoundingStrategy::ToZero);
    (quantized * Decimal::from(1_000_000u32))
        .to_u32()
        .ok_or_else(|| unavailable("rerank score cannot be quantized"))
}

fn compare_reranked(
    left: &RerankedSemanticCandidateV2,
    right: &RerankedSemanticCandidateV2,
) -> Ordering {
    right
        .score_millionths
        .cmp(&left.score_millionths)
        .then(
            left.candidate
                .pre_rerank_rrf_rank
                .cmp(&right.candidate.pre_rerank_rrf_rank),
        )
        .then(left.candidate.product_id.cmp(&right.candidate.product_id))
        .then(
            left.candidate
                .product_version_id
                .cmp(&right.candidate.product_version_id),
        )
        .then(left.candidate.document_id.cmp(&right.candidate.document_id))
        .then(
            left.candidate
                .source_chunk_id
                .cmp(&right.candidate.source_chunk_id),
        )
}

fn status_error(status: reqwest::StatusCode) -> KnowledgeRetrievalError {
    let message = format!("rerank provider returned status {status}");
    if status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
    {
        unavailable(&message)
    } else {
        invalid(&message)
    }
}

fn invalid(message: &str) -> KnowledgeRetrievalError {
    KnowledgeRetrievalError::InvalidRequest(message.into())
}

fn unavailable(message: &str) -> KnowledgeRetrievalError {
    KnowledgeRetrievalError::Unavailable(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_retrieval::{
        KnowledgeSourceTypeV2, RERANK_REVISION_SCHEMA_V2, RETRIEVAL_RERANK_PROTOCOL_VERSION_V2,
        RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2,
    };
    use std::sync::{Mutex, atomic::AtomicUsize};
    use uuid::Uuid;

    struct TestCredentialResolver {
        calls: Arc<AtomicUsize>,
        mode: u8,
    }

    #[async_trait]
    impl EmbeddingCredentialResolverV2 for TestCredentialResolver {
        async fn resolve(&self, _credential_ref: &str) -> Result<String, KnowledgeRetrievalError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match self.mode {
                0 => Ok("secret".into()),
                1 => Err(invalid("credential configuration failed")),
                _ => Err(unavailable("credential provider unavailable")),
            }
        }
    }

    enum TestTransportOutcome {
        Response(reqwest::StatusCode, Vec<u8>),
        Failure,
    }

    struct TestTransport {
        calls: Arc<AtomicUsize>,
        outcome: Mutex<TestTransportOutcome>,
    }

    #[async_trait]
    impl RerankTransportV2 for TestTransport {
        async fn send(
            &self,
            _request: RerankTransportRequestV2,
        ) -> Result<RerankTransportResponseV2, KnowledgeRetrievalError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match &*self.outcome.lock().expect("test transport lock") {
                TestTransportOutcome::Response(status, bytes) => Ok(RerankTransportResponseV2 {
                    status: *status,
                    bytes: bytes.clone(),
                }),
                TestTransportOutcome::Failure => Err(unavailable("simulated transport timeout")),
            }
        }
    }

    fn test_client(
        credential_mode: u8,
        outcome: TestTransportOutcome,
    ) -> (StrictRerankClientV2, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let credential_calls = Arc::new(AtomicUsize::new(0));
        let transport_calls = Arc::new(AtomicUsize::new(0));
        let client = StrictRerankClientV2::with_transport(
            Arc::new(TestCredentialResolver {
                calls: credential_calls.clone(),
                mode: credential_mode,
            }),
            Arc::new(TestTransport {
                calls: transport_calls.clone(),
                outcome: Mutex::new(outcome),
            }),
        );
        (client, credential_calls, transport_calls)
    }

    fn candidate(id: u128, rank: u32) -> SemanticCandidateV2 {
        SemanticCandidateV2 {
            document_id: Uuid::from_u128(id + 100),
            source_chunk_id: Uuid::from_u128(id),
            product_id: Uuid::from_u128(1),
            product_version_id: Uuid::from_u128(2),
            frozen_document_display_name: "manual.pdf".into(),
            chunk_utf8: format!("document {id}"),
            chunk_sha256: "a".repeat(64),
            chunk_byte_length: 10,
            source_type: KnowledgeSourceTypeV2::Text,
            vector_rank: Some(rank),
            keyword_rank: None,
            exact_rrf_score: super::super::semantic_v2::ExactRationalV2 {
                numerator: 1,
                denominator: u128::from(rank + 60),
            },
            pre_rerank_rrf_rank: rank,
        }
    }

    fn revision(policy: &RetrievalPolicyV2) -> RerankRevisionV2 {
        RerankRevisionV2 {
            schema_version: RERANK_REVISION_SCHEMA_V2,
            provider_protocol_version: RETRIEVAL_RERANK_PROTOCOL_VERSION_V2.into(),
            provider_model_identifier: "cross-encoder@2025-01-15".into(),
            provider_model_revision_sha256: policy.rerank.model_revision_sha256.clone(),
            config_revision_sha256: policy.rerank.config_revision_sha256.clone(),
            endpoint_identity: "https://rerank.example.test/v1/rerank".into(),
            request_config_sha256: RerankRevisionV2::canonical_request_config_sha256(),
            score_normalization_version: RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2.into(),
        }
    }

    fn policy_and_revision() -> (RetrievalPolicyV2, RerankRevisionV2) {
        let mut policy = super::super::semantic_v2::tests::policy();
        let revision = revision(&policy);
        policy.rerank.revision_sha256 = revision.sha256().unwrap();
        (policy, revision)
    }

    fn response(policy: &RetrievalPolicyV2, results: Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "model_revision_sha256": policy.rerank.model_revision_sha256,
            "config_revision_sha256": policy.rerank.config_revision_sha256,
            "results": results
        }))
        .unwrap()
    }

    #[test]
    fn equal_scores_and_shuffled_rows_are_stable() {
        let (policy, revision) = policy_and_revision();
        let candidates = vec![candidate(10, 1), candidate(20, 2)];
        let ordered = apply_response(
            &response(
                &policy,
                serde_json::json!([{"index":0,"score":0.5},{"index":1,"score":0.5}]),
            ),
            candidates.clone(),
            &policy,
            &revision,
        )
        .unwrap();
        let shuffled = apply_response(
            &response(
                &policy,
                serde_json::json!([{"index":1,"score":0.5},{"index":0,"score":0.5}]),
            ),
            candidates,
            &policy,
            &revision,
        )
        .unwrap();
        assert_eq!(ordered, shuffled);
        assert_eq!(
            ordered
                .iter()
                .map(|value| value.candidate.source_chunk_id)
                .collect::<Vec<_>>(),
            vec![Uuid::from_u128(10), Uuid::from_u128(20)]
        );
        assert_eq!(ordered[0].fixed_score(), "0.500000");
    }

    #[tokio::test]
    async fn strict_transport_rejects_malformed_identity_status_and_failure_modes() {
        let (policy, revision) = policy_and_revision();
        let candidates = vec![candidate(10, 1)];
        for bytes in [b"{".to_vec(), b"{\"model_revision_sha256\":\"x\",\"config_revision_sha256\":\"y\",\"results\":[{\"index\":0,\"score\":1e9999}]}".to_vec()] {
            let (client, _, calls) = test_client(
                0,
                TestTransportOutcome::Response(reqwest::StatusCode::OK, bytes),
            );
            assert!(matches!(
                client
                    .rerank("query", candidates.clone(), &policy, &revision, "credential")
                    .await,
                Err(KnowledgeRetrievalError::Unavailable(_))
            ));
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        }

        for field in ["model_revision_sha256", "config_revision_sha256"] {
            let mut wrong = serde_json::from_slice::<Value>(&response(
                &policy,
                serde_json::json!([{"index":0,"score":0.5}]),
            ))
            .unwrap();
            wrong[field] = Value::String("f".repeat(64));
            let (client, _, _) = test_client(
                0,
                TestTransportOutcome::Response(
                    reqwest::StatusCode::OK,
                    serde_json::to_vec(&wrong).unwrap(),
                ),
            );
            assert!(matches!(
                client
                    .rerank(
                        "query",
                        candidates.clone(),
                        &policy,
                        &revision,
                        "credential"
                    )
                    .await,
                Err(KnowledgeRetrievalError::InvalidRequest(_))
            ));
        }

        for (status, retryable) in [
            (reqwest::StatusCode::REQUEST_TIMEOUT, true),
            (reqwest::StatusCode::TOO_MANY_REQUESTS, true),
            (reqwest::StatusCode::INTERNAL_SERVER_ERROR, true),
            (reqwest::StatusCode::BAD_REQUEST, false),
            (reqwest::StatusCode::UNAUTHORIZED, false),
            (reqwest::StatusCode::FOUND, false),
        ] {
            let (client, _, _) = test_client(0, TestTransportOutcome::Response(status, Vec::new()));
            let result = client
                .rerank(
                    "query",
                    candidates.clone(),
                    &policy,
                    &revision,
                    "credential",
                )
                .await;
            assert_eq!(
                matches!(result, Err(KnowledgeRetrievalError::Unavailable(_))),
                retryable,
                "unexpected taxonomy for {status}"
            );
        }

        let (timeout_client, _, _) = test_client(0, TestTransportOutcome::Failure);
        assert!(matches!(
            timeout_client
                .rerank(
                    "query",
                    candidates.clone(),
                    &policy,
                    &revision,
                    "credential"
                )
                .await,
            Err(KnowledgeRetrievalError::Unavailable(_))
        ));

        let ok_response = response(&policy, serde_json::json!([{"index":0,"score":0.5}]));
        let (empty_client, credential_calls, transport_calls) = test_client(
            0,
            TestTransportOutcome::Response(reqwest::StatusCode::OK, ok_response.clone()),
        );
        assert!(
            empty_client
                .rerank("query", Vec::new(), &policy, &revision, "credential")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            credential_calls.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        assert_eq!(transport_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        let (credential_failure, _, provider_calls) = test_client(
            1,
            TestTransportOutcome::Response(reqwest::StatusCode::OK, ok_response),
        );
        assert!(matches!(
            credential_failure
                .rerank(
                    "query",
                    candidates.clone(),
                    &policy,
                    &revision,
                    "credential"
                )
                .await,
            Err(KnowledgeRetrievalError::InvalidRequest(_))
        ));
        assert_eq!(provider_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        let (provider_failure, credential_calls, provider_calls) =
            test_client(0, TestTransportOutcome::Failure);
        assert!(matches!(
            provider_failure
                .rerank("query", candidates, &policy, &revision, "credential")
                .await,
            Err(KnowledgeRetrievalError::Unavailable(_))
        ));
        assert_eq!(
            credential_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(provider_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn response_rejects_duplicate_missing_range_identity_and_scores() {
        let (policy, revision) = policy_and_revision();
        let candidates = vec![candidate(10, 1), candidate(20, 2)];
        for results in [
            serde_json::json!([{"index":0,"score":0.5},{"index":0,"score":0.4}]),
            serde_json::json!([{"index":0,"score":0.5}]),
            serde_json::json!([{"index":0,"score":0.5},{"index":2,"score":0.4}]),
            serde_json::json!([{"index":0,"score":-0.1},{"index":1,"score":0.4}]),
            serde_json::json!([{"index":0,"score":1.1},{"index":1,"score":0.4}]),
            serde_json::json!([{"index":0,"score":"NaN"},{"index":1,"score":0.4}]),
        ] {
            assert!(
                apply_response(
                    &response(&policy, results),
                    candidates.clone(),
                    &policy,
                    &revision
                )
                .is_err()
            );
        }
        let mut wrong = serde_json::from_slice::<Value>(&response(
            &policy,
            serde_json::json!([{"index":0,"score":0.5},{"index":1,"score":0.4}]),
        ))
        .unwrap();
        wrong["config_revision_sha256"] = Value::String("f".repeat(64));
        assert!(matches!(
            apply_response(
                &serde_json::to_vec(&wrong).unwrap(),
                candidates,
                &policy,
                &revision
            ),
            Err(KnowledgeRetrievalError::InvalidRequest(_))
        ));
    }
}
