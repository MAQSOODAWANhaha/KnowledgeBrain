use bid::matching::run_match_route_v1;
use runtime::{BidMatchRouteV1Job, BidMatchRouteV1Snapshots};
use serde_json::json;
use sqlx::Row;
use storage::bid_matching::{PickSelectionV1, ReplaceRoutePickSetV1, ScheduleEnvironment};
use uuid::Uuid;

#[tokio::test]
async fn matching_publication_freezes_evidence_and_builds_unsectioned_pick_sets() {
    let Ok(pool) = storage::connect().await else {
        return;
    };
    let final_schema: bool = sqlx::query_scalar(
        "SELECT to_regclass('public.bid_matching_frozen_retrieved_hits') IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);
    if !final_schema {
        return;
    }

    let user_id = Uuid::new_v4();
    let workspace_id = Uuid::new_v4();
    let product_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    let chunk_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let clause_id = Uuid::new_v4();
    let actor = format!("user:{user_id}");
    let file_bytes = b"manual";
    let digest = domain::sha256_hex(file_bytes);
    let object_ref = format!("objects/{digest}");
    let requirement = "支持国密算法";
    let chunk = "产品完整支持国密算法，并提供配置说明。";

    let mut seed = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO users(id,email) VALUES($1,$2)")
        .bind(user_id)
        .bind(format!("{user_id}@matching.invalid"))
        .execute(&mut *seed)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workspaces(id,name,slug,kind) VALUES($1,'产品线',$2,'product_line')")
        .bind(workspace_id)
        .bind(format!("matching-{workspace_id}"))
        .execute(&mut *seed)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','防火墙',$3)",
    )
    .bind(product_id)
    .bind(workspace_id)
    .bind(format!("firewall-{product_id}"))
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status) VALUES($1,$2,'v1','active')",
    )
    .bind(version_id)
    .bind(product_id)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query("UPDATE products SET current_version_id=$2 WHERE id=$1")
        .bind(product_id)
        .bind(version_id)
        .execute(&mut *seed)
        .await
        .unwrap();
    sqlx::query("SELECT kb_object_reference_add($1,$2,'application/pdf',$3,'knowledge_document',$4,'original',$5)")
        .bind(&object_ref)
        .bind(&digest)
        .bind(file_bytes.len() as i64)
        .bind(document_id)
        .bind(&actor)
        .execute(&mut *seed)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO documents
         (id,product_version_id,title,parse_status,enable_status,index_ready,file_name,file_size,file_hash,object_ref)
         VALUES($1,$2,'产品手册','completed','enabled',true,'国密手册.pdf',$3,$4,$5)",
    )
    .bind(document_id)
    .bind(version_id)
    .bind(file_bytes.len() as i64)
    .bind(&digest)
    .bind(&object_ref)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content,start_at,end_at)
         VALUES($1,$2,$3,'text',$4,0,$5)",
    )
    .bind(chunk_id)
    .bind(version_id)
    .bind(document_id)
    .bind(chunk)
    .bind(chunk.len() as i32)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_projects
         (id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,
          matching_mutation_watermark,created_by)
         VALUES($1,'国密投标',$2,clock_timestamp()+interval '30 days',repeat('0',64),repeat('1',64),1,$3)",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(&actor)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_clauses
         (id,project_id,provenance,status,kind,text,must,revision,created_by)
         VALUES($1,$2,'manual','confirmed','technical',$3,true,1,$4)",
    )
    .bind(clause_id)
    .bind(project_id)
    .bind(requirement)
    .bind(&actor)
    .execute(&mut *seed)
    .await
    .unwrap();
    seed.commit().await.unwrap();

    let scheduled = storage::bid_matching::schedule_dirty_project(
        &pool,
        project_id,
        ScheduleEnvironment {
            environment: "test".into(),
            max_attempts: 3,
        },
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(scheduled.jobs.len(), 2, "technical + commercial routes");
    for scheduled_job in scheduled.jobs {
        run_match_route_v1(
            &pool,
            BidMatchRouteV1Job::new(
                scheduled_job.id,
                BidMatchRouteV1Snapshots {
                    config_snapshot_id: scheduled_job.snapshots.config_snapshot_id,
                    feature_snapshot_id: scheduled_job.snapshots.feature_snapshot_id,
                    score_policy_snapshot_id: scheduled_job.snapshots.score_policy_snapshot_id,
                    verifier_policy_snapshot_id: scheduled_job
                        .snapshots
                        .verifier_policy_snapshot_id,
                },
                None,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    }

    let routes = storage::bid_matching::current_routes(&pool, project_id)
        .await
        .unwrap();
    let technical_route = routes
        .iter()
        .find(|row| row.get::<String, _>("route_kind") == "technical")
        .unwrap();
    assert_eq!(technical_route.get::<Uuid, _>("unit_id"), Uuid::nil());
    let route_id: Uuid = technical_route.get("route_id");
    let report = storage::bid_matching::current_route_report(&pool, project_id, route_id)
        .await
        .unwrap()
        .unwrap();
    let candidates =
        storage::bid_matching::current_route_supported_candidates(&pool, project_id, route_id)
            .await
            .unwrap();
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].get::<bool, _>("recommended"));

    let candidate_id: Uuid = candidates[0].get("candidate_artifact_id");
    let requirement_id: Uuid = candidates[0].get("requirement_artifact_id");
    let body = json!({
        "source_report_artifact_id": report.get::<Uuid, _>("report_id"),
        "report_sha256": report.get::<String, _>("content_sha256"),
        "expected_revision": 0,
        "items": [{"requirement_artifact_id": requirement_id, "candidate_artifact_id": candidate_id}],
    });
    let context =
        storage::bidding::MutationContext::new(actor, format!("pick-{project_id}"), &body).unwrap();
    let receipt = storage::bid_matching::replace_route_pick_set(
        &pool,
        ReplaceRoutePickSetV1 {
            project_id,
            route_id,
            source_report_artifact_id: report.get("report_id"),
            report_sha256: report.get("content_sha256"),
            expected_revision: 0,
            selections: vec![PickSelectionV1 {
                requirement_artifact_id: requirement_id,
                candidate_artifact_id: candidate_id,
            }],
        },
        &context,
    )
    .await
    .unwrap();
    assert_eq!(receipt.route_revision, 1);
    let project_units: Vec<Option<Uuid>> = sqlx::query_scalar(
        "SELECT item.unit_id FROM bid_current_project_pick_sets current_value
         JOIN bid_project_pick_set_items item ON item.project_pick_set_id=current_value.pick_set_id
         WHERE current_value.project_id=$1",
    )
    .bind(project_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(project_units, vec![Some(Uuid::nil())]);

    sqlx::query("UPDATE documents SET file_name='已改名.pdf' WHERE id=$1")
        .bind(document_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("SELECT kb_release_knowledge_document_object($1,$2,$3,$4)")
        .bind(document_id)
        .bind(format!("user:{user_id}"))
        .bind(format!("release-{document_id}"))
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM chunks WHERE id=$1")
        .bind(chunk_id)
        .execute(&pool)
        .await
        .unwrap();
    let frozen_name: String = sqlx::query_scalar(
        "SELECT source.frozen_document_display_name FROM bid_matching_source_artifacts source
         JOIN bid_matching_reports report ON report.id=source.report_id
         WHERE report.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(frozen_name, "国密手册.pdf");
}
