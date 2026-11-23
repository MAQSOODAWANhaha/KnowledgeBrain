use super::*;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use jsonwebtoken::{EncodingKey, Header, encode};
use tower::ServiceExt;

fn sign(value: &Value, secret: &str) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        value,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap()
}

fn worker() -> Value {
    json!({"purpose":WORKER_PURPOSE,"execution_id":"fixture-execution",
        "docx_sha256":"a".repeat(64),"epoch":2,"exp":now()+60})
}

#[tokio::test]
async fn response_quota_follows_the_last_network_bytes_after_the_body_is_dropped() {
    use http_body_util::BodyExt;
    static QUOTA: Semaphore = Semaphore::const_new(1);
    let permit = std::sync::Arc::new(QUOTA.try_acquire().unwrap());
    let bytes = retain_quota(Bytes::from(vec![b'x'; 128 * 1024]), permit.clone());
    let mut body = timed_body(bytes, tokio::time::Instant::now() + Duration::from_secs(1));
    drop(permit);
    let data = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert_eq!(data.len(), 64 * 1024);
    drop(body);
    assert_eq!(QUOTA.available_permits(), 0);
    drop(data);
    assert_eq!(QUOTA.available_permits(), 1);
}

#[tokio::test]
async fn expired_response_stops_sending_and_releases_its_remaining_quota() {
    use http_body_util::BodyExt;
    static QUOTA: Semaphore = Semaphore::const_new(1);
    let permit = std::sync::Arc::new(QUOTA.try_acquire().unwrap());
    let bytes = retain_quota(Bytes::from_static(b"private candidate"), permit);
    let mut body = timed_body(
        bytes,
        tokio::time::Instant::now() - Duration::from_millis(1),
    );
    assert!(body.frame().await.unwrap().is_err());
    assert_eq!(QUOTA.available_permits(), 1);
    assert!(body.frame().await.is_none());
}

fn grant() -> SourceClaims {
    SourceClaims {
        aud: SOURCE_PURPOSE.into(),
        execution_id: "fixture-execution".into(),
        docx_sha256: "a".repeat(64),
        epoch: 2,
        project: Uuid::new_v4(),
        owner: Uuid::new_v4(),
        nonce: Uuid::new_v4(),
        exp: now() + 120,
    }
}

#[tokio::test]
async fn render_requires_worker_authentication_before_configuration_or_database() {
    let app = router().with_state(AppState {
        jwt_secret: "unused-fixture".into(),
        bootstrap_key: String::new(),
    });
    let response = app
        .oneshot(
            Request::post("/api/internal/tender-render/fixture-execution")
                .body(Body::from("untrusted candidate"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn saved_pdf_requires_user_authentication_before_configuration_or_storage() {
    let app = router().with_state(AppState {
        jwt_secret: "unused-fixture".into(),
        bootstrap_key: String::new(),
    });
    let response = app
        .oneshot(
            Request::post(format!(
                "/api/v3/tender-projects/{}/docx/versions/{}/pdf",
                Uuid::new_v4(),
                Uuid::new_v4(),
            ))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"docx_sha256":"a".repeat(64)}).to_string(),
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn worker_token_binds_purpose_execution_hash_epoch_expiry_and_exact_fields() {
    let value = worker();
    assert_eq!(
        worker_claims(
            &sign(&value, "worker-secret"),
            "worker-secret",
            "fixture-execution"
        )
        .unwrap()
        .epoch,
        2
    );
    for patch in [
        json!({"purpose":"tender_knowledge_v1"}),
        json!({"execution_id":"another-job"}),
        json!({"docx_sha256":"A".repeat(64)}),
        json!({"epoch":0}),
        json!({"epoch":-1}),
        json!({"exp":now()-1}),
        json!({"exp":now()+1000}),
        json!({"extra":"ignored"}),
    ] {
        let mut bad = value.clone();
        bad.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(
            worker_claims(
                &sign(&bad, "worker-secret"),
                "worker-secret",
                "fixture-execution"
            )
            .is_err()
        );
    }
    assert!(
        worker_claims(
            &sign(&value, "wrong-secret"),
            "worker-secret",
            "fixture-execution"
        )
        .is_err()
    );
    assert!(worker_claims(&sign(&value, ""), "", "fixture-execution").is_err());
}

#[test]
fn source_capability_is_short_lived_and_bound_to_all_candidate_fields() {
    let claims = grant();
    let token = sign(&serde_json::to_value(&claims).unwrap(), "source-secret");
    assert_eq!(
        source_claims(&token, "source-secret", &claims.execution_id, claims.nonce).unwrap(),
        claims
    );
    assert!(source_claims(&token, "source-secret", "another-job", claims.nonce).is_err());
    assert!(
        source_claims(
            &token,
            "source-secret",
            &claims.execution_id,
            Uuid::new_v4()
        )
        .is_err()
    );
    assert!(source_claims(&token, "other-secret", &claims.execution_id, claims.nonce).is_err());
    for patch in [
        json!({"aud":"tender-word-source"}),
        json!({"exp":now()-1}),
        json!({"exp":now()+1000}),
        json!({"epoch":0}),
        json!({"extra":true}),
    ] {
        let mut bad = serde_json::to_value(&claims).unwrap();
        bad.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(
            source_claims(
                &sign(&bad, "source-secret"),
                "source-secret",
                &claims.execution_id,
                claims.nonce
            )
            .is_err()
        );
    }
    let guard = install(Candidate {
        claims: ConversionSource::Render(claims.clone()),
        bytes: Bytes::from_static(b"fixture"),
    })
    .unwrap();
    assert_eq!(
        candidate_bytes(&ConversionSource::Render(claims.clone()))
            .unwrap()
            .as_ref(),
        b"fixture"
    );
    let mut other = claims.clone();
    other.project = Uuid::new_v4();
    assert!(candidate_bytes(&ConversionSource::Render(other)).is_err());
    other = claims.clone();
    other.owner = Uuid::new_v4();
    assert!(candidate_bytes(&ConversionSource::Render(other)).is_err());
    other = claims.clone();
    other.epoch += 1;
    assert!(candidate_bytes(&ConversionSource::Render(other)).is_err());
    other = claims.clone();
    other.docx_sha256 = "b".repeat(64);
    assert!(candidate_bytes(&ConversionSource::Render(other)).is_err());
    drop(guard);
    assert!(candidate_bytes(&ConversionSource::Render(claims)).is_err());
}

#[tokio::test]
async fn timeout_or_cancelled_conversion_revokes_its_source_bytes() {
    let claims = ConversionSource::Render(grant());
    let copy = claims.clone();
    let timed = tokio::time::timeout(Duration::from_millis(1), async move {
        let _candidate = install(Candidate {
            claims: copy,
            bytes: Bytes::from_static(b"fixture"),
        })
        .unwrap();
        std::future::pending::<()>().await;
    })
    .await;
    assert!(timed.is_err());
    assert!(candidate_bytes(&claims).is_err());
    let (send, receive) = tokio::sync::oneshot::channel();
    let copy = claims.clone();
    let task = tokio::spawn(async move {
        let _candidate = install(Candidate {
            claims: copy,
            bytes: Bytes::from_static(b"fixture"),
        })
        .unwrap();
        send.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    receive.await.unwrap();
    assert!(candidate_bytes(&claims).is_ok());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(candidate_bytes(&claims).is_err());
}

mod postgres {
    use super::*;
    use axum::Json;
    use axum::extract::State;
    use axum::routing::{get, post};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};

    #[derive(Clone)]
    struct Fixture {
        pool: PgPool,
        execution: String,
        origin: String,
        mode: Arc<AtomicU8>,
        source: Arc<Mutex<Option<String>>>,
    }

    async fn converter(State(fixture): State<Fixture>, Json(command): Json<Value>) -> Json<Value> {
        let mut rules = Validation::new(Algorithm::HS256);
        rules.required_spec_claims.remove("exp");
        rules.validate_exp = false;
        let signed = decode::<Value>(
            command["token"].as_str().unwrap(),
            &DecodingKey::from_secret(b"fixture-office-secret"),
            &rules,
        )
        .unwrap()
        .claims;
        let mut unsigned = command.clone();
        unsigned.as_object_mut().unwrap().remove("token");
        assert!(
            signed == unsigned,
            "conversion JWT must sign the exact request body"
        );
        let source = command["url"].as_str().unwrap().to_owned();
        *fixture.source.lock().unwrap() = Some(source.clone());
        let response = reqwest::get(&source)
            .await
            .unwrap_or_else(|_| panic!("fixture candidate read failed"));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.bytes().await.unwrap().as_ref(),
            b"PK\x03\x04fixture-docx"
        );
        match fixture.mode.load(Ordering::SeqCst) {
            6 => std::future::pending::<Json<Value>>().await,
            1 => Json(
                json!({"endConvert":true,"fileType":"pdf","fileUrl":"http://127.0.0.1:1/private.pdf"}),
            ),
            2 => Json(json!({"error":-3})),
            7 => {
                let mut transaction = fixture.pool.begin().await.unwrap();
                sqlx::query("LOCK TABLE tender_authoring_jobs IN ACCESS EXCLUSIVE MODE")
                    .execute(&mut *transaction)
                    .await
                    .unwrap();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(6)).await;
                    transaction.rollback().await.unwrap();
                });
                Json(
                    json!({"endConvert":true,"fileType":"pdf","fileUrl":format!("{}/result.pdf",fixture.origin)}),
                )
            }
            4 | 5 => {
                let update = if fixture.mode.load(Ordering::SeqCst) == 4 {
                    "UPDATE tender_authoring_executions SET cancel_requested=true WHERE execution_id=$1"
                } else {
                    "UPDATE tender_authoring_executions SET epoch=epoch+1 WHERE execution_id=$1"
                };
                sqlx::query(update)
                    .bind(&fixture.execution)
                    .execute(&fixture.pool)
                    .await
                    .unwrap();
                let denied = reqwest::get(&source)
                    .await
                    .unwrap_or_else(|_| panic!("fixture revoked read failed"));
                assert_eq!(denied.status(), StatusCode::NOT_FOUND);
                Json(
                    json!({"endConvert":true,"fileType":"pdf","fileUrl":format!("{}/result.pdf",fixture.origin)}),
                )
            }
            _ => Json(
                json!({"endConvert":true,"fileType":"pdf","fileUrl":format!("{}/result.pdf",fixture.origin)}),
            ),
        }
    }

    async fn pdf(State(fixture): State<Fixture>) -> Response {
        if fixture.mode.load(Ordering::SeqCst) == 3 {
            (
                StatusCode::FOUND,
                [("location", "http://127.0.0.1:1/private.pdf")],
            )
                .into_response()
        } else {
            (
                [("content-type", "application/pdf")],
                b"%PDF-1.7\nfixture-pdf\n".as_slice(),
            )
                .into_response()
        }
    }

    async fn job(pool: &PgPool) -> (String, Uuid, Uuid) {
        let project = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let execution = format!("renderer-{}", Uuid::new_v4().simple());
        let input = json!({"run_id":"fixture-run","source_version":1,"domain_revision":1});
        sqlx::query("INSERT INTO tender_projects(id,owner_user_id,status) VALUES($1,$2,'open')")
            .bind(project)
            .bind(owner)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tender_authoring_jobs(execution_id,project_id,run_id,payload,status) VALUES($1,$2,'fixture-run',$3,'published')")
            .bind(&execution).bind(project).bind(json!({"owner_user_id":owner,"operation":"document","identity":input}))
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO tender_authoring_executions(execution_id,authoring_input,status,epoch,cancel_requested,lease_until) VALUES($1,$2,'running',2,false,clock_timestamp()+interval '5 minutes')")
            .bind(&execution).bind(input).execute(pool).await.unwrap();
        (execution, project, owner)
    }

    async fn initialize(pool: &PgPool) {
        // The production columns used by the renderer, in a disposable DB.
        // This tests SQL fences and HTTP, not the full migration contract.
        sqlx::raw_sql("CREATE TABLE tender_projects(id uuid PRIMARY KEY,owner_user_id uuid NOT NULL,status text NOT NULL);
            CREATE TABLE tender_authoring_jobs(execution_id text PRIMARY KEY,project_id uuid NOT NULL,run_id text NOT NULL,payload jsonb NOT NULL,status text NOT NULL);
            CREATE TABLE tender_authoring_executions(execution_id text PRIMARY KEY,authoring_input jsonb NOT NULL,status text NOT NULL,epoch bigint NOT NULL,cancel_requested boolean NOT NULL,lease_until timestamptz);")
            .execute(pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires the task-owned disposable kb_toc_render_test PostgreSQL database"]
    async fn scoped_http_conversion_and_source_revocation_use_real_persisted_job_fences() {
        let dsn = std::env::var("DATABASE_URL").expect("explicit disposable database required");
        assert!(
            dsn.contains("kb_toc_render_test_"),
            "refusing a database not owned by this renderer test"
        );
        let pool = platform::connect().await.unwrap();
        initialize(&pool).await;
        assert!(active_scope(&pool, "absent-execution", 2).await.is_err());
        let (execution, project, owner) = job(&pool).await;
        assert_eq!(
            active_scope(&pool, &execution, 2).await.unwrap(),
            (project, owner)
        );
        assert!(active_scope(&pool, &execution, 1).await.is_err());
        for update in [
            "UPDATE tender_authoring_executions SET status='failed' WHERE execution_id=$1",
            "UPDATE tender_authoring_executions SET lease_until=clock_timestamp()-interval '1 second' WHERE execution_id=$1",
            "UPDATE tender_authoring_executions SET cancel_requested=true WHERE execution_id=$1",
            "UPDATE tender_authoring_jobs SET status='cancelled' WHERE execution_id=$1",
            "UPDATE tender_projects SET status='ended' WHERE id=(SELECT project_id FROM tender_authoring_jobs WHERE execution_id=$1)",
            "UPDATE tender_projects SET owner_user_id='00000000-0000-0000-0000-000000000001' WHERE id=(SELECT project_id FROM tender_authoring_jobs WHERE execution_id=$1)",
            "UPDATE tender_authoring_jobs SET payload=jsonb_set(payload,'{operation}','\"outline\"') WHERE execution_id=$1",
            "UPDATE tender_authoring_executions SET authoring_input=jsonb_set(authoring_input,'{source_version}','2') WHERE execution_id=$1",
            "UPDATE tender_authoring_jobs SET payload=jsonb_set(payload,'{identity,run_id}','\"another-run\"') WHERE execution_id=$1",
        ] {
            let (execution, _, _) = job(&pool).await;
            sqlx::query(update)
                .bind(&execution)
                .execute(&pool)
                .await
                .unwrap();
            assert!(
                active_scope(&pool, &execution, 2).await.is_err(),
                "a persisted fence was not enforced"
            );
        }
        let api = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let office = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_origin = format!("http://{}", api.local_addr().unwrap());
        let origin = format!("http://{}", office.local_addr().unwrap());
        // This ignored test runs alone, with --test-threads=1, in a dedicated
        // process/environment prepared by the external disposable-DB runner.
        unsafe {
            std::env::set_var("KB_ONLYOFFICE_API_ORIGIN", format!("{api_origin}/"));
            std::env::set_var("KB_ONLYOFFICE_COMMAND_ORIGIN", format!("{origin}/"));
            std::env::set_var("KB_ONLYOFFICE_SERVER_ORIGIN", format!("{origin}/"));
        }
        let fixture = Fixture {
            pool: pool.clone(),
            execution: execution.clone(),
            origin: origin.clone(),
            mode: Arc::new(AtomicU8::new(0)),
            source: Arc::new(Mutex::new(None)),
        };
        let office_app = Router::new()
            .route("/converter", post(converter))
            .route("/result.pdf", get(pdf))
            .with_state(fixture.clone());
        let office_task =
            tokio::spawn(async move { axum::serve(office, office_app).await.unwrap() });
        let app = router().with_state(AppState {
            jwt_secret: "unused-fixture".into(),
            bootstrap_key: String::new(),
        });
        let api_task = tokio::spawn(async move { axum::serve(api, app).await.unwrap() });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let bytes = b"PK\x03\x04fixture-docx";
        let hash = platform::sha256_hex(bytes);
        for (mode, status) in [
            (0, StatusCode::OK),
            (1, StatusCode::FORBIDDEN),
            (2, StatusCode::SERVICE_UNAVAILABLE),
            (3, StatusCode::SERVICE_UNAVAILABLE),
            (4, StatusCode::NOT_FOUND),
            (5, StatusCode::NOT_FOUND),
            (7, StatusCode::SERVICE_UNAVAILABLE),
        ] {
            fixture.mode.store(mode, Ordering::SeqCst);
            sqlx::query("UPDATE tender_authoring_executions SET epoch=2,cancel_requested=false WHERE execution_id=$1")
                .bind(&execution).execute(&pool).await.unwrap();
            let claims = json!({"purpose":WORKER_PURPOSE,"execution_id":execution,"docx_sha256":hash,"epoch":2,"exp":now()+60});
            let response = client
                .post(format!(
                    "{api_origin}/api/internal/tender-render/{execution}"
                ))
                .bearer_auth(sign(&claims, "fixture-worker-secret"))
                .header("content-type", DOCX)
                .body(bytes.to_vec())
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), status, "conversion boundary result");
            if status == StatusCode::OK {
                assert_eq!(
                    response.headers()["x-kb-render-engine"],
                    "onlyoffice-converter-v1"
                );
                assert_eq!(
                    response.headers()["x-kb-render-service-sha256"],
                    platform::sha256_hex(format!("{origin}/").as_bytes())
                );
                assert_eq!(response.headers()["x-kb-render-source-sha256"], hash);
                assert_eq!(
                    response.headers()["x-kb-render-pdf-sha256"],
                    platform::sha256_hex(b"%PDF-1.7\nfixture-pdf\n")
                );
                assert_eq!(
                    response.bytes().await.unwrap().as_ref(),
                    b"%PDF-1.7\nfixture-pdf\n"
                );
            }
            let source = fixture.source.lock().unwrap().clone().unwrap();
            let revoked = client
                .get(source)
                .send()
                .await
                .unwrap_or_else(|_| panic!("fixture revoked URL read failed"));
            assert_eq!(
                revoked.status(),
                StatusCode::NOT_FOUND,
                "source must be gone after success or failure"
            );
        }
        let claims = json!({"purpose":WORKER_PURPOSE,"execution_id":execution,"docx_sha256":"f".repeat(64),"epoch":2,"exp":now()+60});
        sqlx::query("UPDATE tender_authoring_executions SET epoch=2,cancel_requested=false WHERE execution_id=$1")
            .bind(&execution).execute(&pool).await.unwrap();
        let response = client
            .post(format!(
                "{api_origin}/api/internal/tender-render/{execution}"
            ))
            .bearer_auth(sign(&claims, "fixture-worker-secret"))
            .header("content-type", DOCX)
            .body(bytes.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "a signed incorrect body hash cannot render"
        );
        let mut oversized = vec![b'x'; 2 * 1024 * 1024];
        oversized[..4].copy_from_slice(b"PK\x03\x04");
        let claims = json!({"purpose":WORKER_PURPOSE,"execution_id":execution,
            "docx_sha256":platform::sha256_hex(&oversized),"epoch":2,"exp":now()+60});
        let response = client
            .post(format!(
                "{api_origin}/api/internal/tender-render/{execution}"
            ))
            .bearer_auth(sign(&claims, "fixture-worker-secret"))
            .header("content-type", DOCX)
            .body(oversized)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "the configured 1MiB upload limit must be enforced"
        );
        let mut locked = pool.begin().await.unwrap();
        sqlx::query("LOCK TABLE tender_authoring_jobs IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *locked)
            .await
            .unwrap();
        let token = sign(
            &json!({"purpose":WORKER_PURPOSE,"execution_id":execution,
            "docx_sha256":hash,"epoch":2,"exp":now()+60}),
            "fixture-worker-secret",
        );
        let waiting = tokio::time::timeout(
            Duration::from_secs(7),
            client
                .post(format!(
                    "{api_origin}/api/internal/tender-render/{execution}"
                ))
                .bearer_auth(token)
                .header("content-type", DOCX)
                .body(bytes.to_vec())
                .send(),
        )
        .await;
        locked.rollback().await.unwrap();
        let response = waiting
            .expect("database waits must share the configured five-second layout deadline")
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            candidates().lock().unwrap().is_empty(),
            "a database timeout must not expose candidate bytes"
        );
        let short = SourceClaims {
            aud: SOURCE_PURPOSE.into(),
            execution_id: execution.clone(),
            docx_sha256: hash.clone(),
            epoch: 2,
            project,
            owner,
            nonce: Uuid::new_v4(),
            exp: now() + 2,
        };
        let candidate = install(Candidate {
            claims: ConversionSource::Render(short.clone()),
            bytes: Bytes::from_static(b"PK\x03\x04fixture-docx"),
        })
        .unwrap();
        let mut locked = pool.begin().await.unwrap();
        sqlx::query("LOCK TABLE tender_authoring_jobs IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *locked)
            .await
            .unwrap();
        let token = sign(
            &serde_json::to_value(&short).unwrap(),
            "fixture-source-secret",
        );
        let mut url = reqwest::Url::parse(&format!(
            "{api_origin}/api/internal/tender-render/{execution}/{}/source",
            short.nonce
        ))
        .unwrap();
        url.query_pairs_mut().append_pair("token", &token);
        let reading = tokio::time::timeout(Duration::from_secs(3), client.get(url).send()).await;
        locked.rollback().await.unwrap();
        assert_eq!(
            reading
                .expect("source database reads must respect the capability expiry")
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        drop(candidate);
        fixture.mode.store(6, Ordering::SeqCst);
        let mut active = Vec::new();
        for _ in 0..4 {
            let client = client.clone();
            let endpoint = format!("{api_origin}/api/internal/tender-render/{execution}");
            let token = sign(
                &json!({"purpose":WORKER_PURPOSE,"execution_id":execution,
                "docx_sha256":hash,"epoch":2,"exp":now()+60}),
                "fixture-worker-secret",
            );
            active.push(tokio::spawn(async move {
                client
                    .post(endpoint)
                    .bearer_auth(token)
                    .header("content-type", DOCX)
                    .body(b"PK\x03\x04fixture-docx".to_vec())
                    .send()
                    .await
                    .unwrap()
            }));
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while candidates().lock().unwrap().len() < 4 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("four conversions must hold their own source grants");
        let token = sign(
            &json!({"purpose":WORKER_PURPOSE,"execution_id":execution,
            "docx_sha256":hash,"epoch":2,"exp":now()+60}),
            "fixture-worker-secret",
        );
        let refused = client
            .post(format!(
                "{api_origin}/api/internal/tender-render/{execution}"
            ))
            .bearer_auth(token)
            .header("content-type", DOCX)
            .body(bytes.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(
            refused.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "a fifth concurrent conversion must not allocate a candidate"
        );
        assert_eq!(candidates().lock().unwrap().len(), 4);
        for task in active {
            assert_eq!(
                task.await.unwrap().status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "a stalled converter must end at the shared layout deadline"
            );
        }
        assert!(candidates().lock().unwrap().is_empty());
        api_task.abort();
        office_task.abort();
        let _ = api_task.await;
        let _ = office_task.await;
    }

    #[tokio::test]
    #[ignore = "requires a disposable renderer DB and the explicitly selected isolated OnlyOffice"]
    async fn actual_office_converts_a_scoped_candidate_and_revokes_its_source() {
        use std::io::Write;
        let dsn = std::env::var("DATABASE_URL").expect("explicit disposable database required");
        assert!(dsn.contains("kb_toc_render_test_"));
        let evidence = std::path::Path::new("/opt/workspace/kb-tender-evidence");
        let input =
            std::path::PathBuf::from(std::env::var("KB_TENDER_RENDER_SMOKE_DOCX_PATH").unwrap())
                .canonicalize()
                .unwrap();
        let output =
            std::path::PathBuf::from(std::env::var("KB_TENDER_RENDER_SMOKE_PDF_PATH").unwrap());
        assert!(
            input.starts_with(evidence)
                && output
                    .parent()
                    .unwrap()
                    .canonicalize()
                    .unwrap()
                    .starts_with(evidence)
        );
        let bytes = std::fs::read(input).unwrap();
        let hash = platform::sha256_hex(&bytes);
        let pool = platform::connect().await.unwrap();
        initialize(&pool).await;
        let (execution, project, owner) = job(&pool).await;
        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let host: std::net::IpAddr = std::env::var("KB_TENDER_RENDER_SMOKE_API_HOST")
            .unwrap()
            .parse()
            .unwrap();
        assert!(matches!(host,std::net::IpAddr::V4(ip) if ip.is_private()));
        let origin = format!("http://127.0.0.1:{port}");
        // Only this single, explicitly selected ignored test runs in the process.
        unsafe {
            std::env::set_var("KB_ONLYOFFICE_API_ORIGIN", format!("http://{host}:{port}/"));
        }
        let app = router().with_state(AppState {
            jwt_secret: "unused-fixture".into(),
            bootstrap_key: String::new(),
        });
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap();
        let token = sign(
            &json!({"purpose":WORKER_PURPOSE,"execution_id":execution,
            "docx_sha256":hash,"epoch":2,"exp":now()+60}),
            "fixture-worker-secret",
        );
        let call = client
            .post(format!("{origin}/api/internal/tender-render/{execution}"))
            .bearer_auth(token)
            .header("content-type", DOCX)
            .body(bytes);
        let task = tokio::spawn(async move { call.send().await });
        let cap =
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let Some(claims) = candidates().lock().unwrap().values().find_map(|value| {
                        match &value.claims {
                            ConversionSource::Render(claims)
                                if claims.execution_id == execution =>
                            {
                                Some(claims.clone())
                            }
                            _ => None,
                        }
                    }) {
                        break claims;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await;
        let response = task
            .await
            .unwrap()
            .unwrap_or_else(|_| panic!("actual Office HTTP request failed"));
        let status = response.status();
        if status != StatusCode::OK {
            let error: Value = response.json().await.unwrap_or(Value::Null);
            println!(
                "actual Office renderer status={status}, error_code={}",
                error["error"]["code"]
            );
            panic!("actual Office conversion failed; retained renderer error code above");
        }
        let cap = cap.expect("the real converter must have an observable scoped source capability");
        assert_eq!(
            response.headers()["x-kb-render-engine"],
            "onlyoffice-converter-v1"
        );
        assert_eq!(response.headers()["x-kb-render-source-sha256"], hash);
        let expected_pdf = response.headers()["x-kb-render-pdf-sha256"]
            .to_str()
            .unwrap()
            .to_owned();
        let pdf = response.bytes().await.unwrap();
        assert!(pdf.starts_with(b"%PDF-") && pdf.len() > 1000);
        assert_eq!(platform::sha256_hex(&pdf), expected_pdf);
        assert_eq!(
            active_scope(&pool, &execution, 2).await.unwrap(),
            (project, owner)
        );
        assert!(
            cap.exp > now(),
            "revocation must be tested before the capability naturally expires"
        );
        let mut url = reqwest::Url::parse(&format!(
            "{origin}/api/internal/tender-render/{execution}/{}/source",
            cap.nonce
        ))
        .unwrap();
        url.query_pairs_mut().append_pair(
            "token",
            &sign(
                &serde_json::to_value(&cap).unwrap(),
                "fixture-source-secret",
            ),
        );
        assert_eq!(
            client
                .get(url)
                .send()
                .await
                .unwrap_or_else(|_| panic!("actual Office revoked source request failed"))
                .status(),
            StatusCode::NOT_FOUND
        );
        assert!(candidates().lock().unwrap().is_empty());
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)
            .unwrap()
            .write_all(&pdf)
            .unwrap();
        server.abort();
        let _ = server.await;
    }
}
