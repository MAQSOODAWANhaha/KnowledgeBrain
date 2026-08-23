//! Fenced extraction publication: hidden candidates are the write-ahead facts,
//! and only this store publishes current `bid_sections` / `bid_clauses`.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::bid::{ExtractionClauseRow, ExtractionSectionRow, PersistExtractionReport};

const POLICY_VERSION: &str = "cn-tender-v2";
const PROMPT_VERSION: &str = "clause-extractor-v2";
const SCHEMA_VERSION: &str = "1";
const PROVIDER_ID: &str = "heuristic";
const MODEL_ID: &str = "none";
const GENERATION_STALE: &str = "extract target generation stale";

pub struct ExtractionPublicationStore;

struct FencedTarget {
    id: Uuid,
    run_id: Uuid,
    project_id: Uuid,
    document_id: Uuid,
    extraction_generation: i64,
    expected_conversion_generation: i64,
    claim_token: Uuid,
}

impl ExtractionPublicationStore {
    pub async fn publish_document(
        pool: &PgPool,
        report: PersistExtractionReport<'_>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        let outcome = publish_document_tx(&mut tx, report).await;
        finish_publication_tx(tx, outcome).await
    }

    pub async fn publish_section(
        pool: &PgPool,
        retry_token: Uuid,
        project_id: Uuid,
        document_id: Uuid,
        section: &ExtractionSectionRow<'_>,
        clauses: &[ExtractionClauseRow<'_>],
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        let outcome = publish_section_tx(
            &mut tx,
            retry_token,
            project_id,
            document_id,
            section,
            clauses,
        )
        .await;
        finish_publication_tx(tx, outcome).await
    }

    pub async fn prune_unconfirmed_sections(
        pool: &PgPool,
        document_id: Uuid,
        keep_keys: &[String],
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        prune_unconfirmed_section_rows(&mut tx, document_id, keep_keys).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn sync_finished_extract_run(
        tx: &mut Transaction<'_, Postgres>,
        run_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        Self::terminalize_run_targets(tx, run_id).await?;
        refresh_extract_run_aggregates(tx, run_id).await
    }

    pub async fn terminalize_run_targets(
        tx: &mut Transaction<'_, Postgres>,
        run_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        let target_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM bid_extract_run_targets WHERE run_id = $1 FOR UPDATE",
        )
        .bind(run_id)
        .fetch_all(&mut **tx)
        .await?;
        for target_id in target_ids {
            terminalize_target(tx, target_id).await?;
        }
        Ok(())
    }

    pub async fn terminalize_runs(
        tx: &mut Transaction<'_, Postgres>,
        run_ids: &[Uuid],
    ) -> Result<(), sqlx::Error> {
        for run_id in run_ids {
            Self::terminalize_run_targets(tx, *run_id).await?;
        }
        Ok(())
    }

    pub async fn refresh_run_aggregates(
        tx: &mut Transaction<'_, Postgres>,
        run_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        refresh_extract_run_aggregates(tx, run_id).await
    }
}

struct RunningRun {
    id: Uuid,
    project_id: Uuid,
    document_id: Option<Uuid>,
    config_snapshot_id: Uuid,
    feature_snapshot_id: Uuid,
}

fn is_generation_stale(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Protocol(message) if message == GENERATION_STALE)
}

async fn finish_publication_tx(
    tx: Transaction<'_, Postgres>,
    outcome: Result<(), sqlx::Error>,
) -> Result<(), sqlx::Error> {
    match outcome {
        Ok(()) => tx.commit().await,
        Err(error) if is_generation_stale(&error) => {
            tx.commit().await?;
            Err(error)
        }
        Err(error) => Err(error),
    }
}

async fn publish_document_tx(
    tx: &mut Transaction<'_, Postgres>,
    report: PersistExtractionReport<'_>,
) -> Result<(), sqlx::Error> {
    let run = fence_running_run(tx, report.run_id, report.project_id, report.claim_token).await?;
    let target = ensure_document_target(
        tx,
        &run,
        report.document_id,
        report.claim_token,
        scoped_section_count(report.scoped_section_count, report.sections),
    )
    .await?;
    if report.replace_document {
        let keep: Vec<String> = report
            .sections
            .iter()
            .map(|section| section.section_key.to_string())
            .collect();
        prune_unconfirmed_section_rows(tx, report.document_id, &keep).await?;
    }
    persist_report_sections(tx, &target, report.sections, report.clauses).await
}

async fn publish_section_tx(
    tx: &mut Transaction<'_, Postgres>,
    retry_token: Uuid,
    project_id: Uuid,
    document_id: Uuid,
    section: &ExtractionSectionRow<'_>,
    clauses: &[ExtractionClauseRow<'_>],
) -> Result<(), sqlx::Error> {
    let target =
        ensure_section_retry_target(tx, retry_token, project_id, document_id, section).await?;
    let outcome = async {
        persist_report_sections(tx, &target, std::slice::from_ref(section), clauses).await?;
        terminalize_target(tx, target.id).await
    }
    .await;
    // The synthetic parent run is never a claimable extract intent. Finish it in this
    // transaction, including generation-stale commits, so housekeeping cannot enqueue it.
    let finished = finish_section_retry_run(tx, target.run_id).await;
    match outcome {
        Ok(()) => finished,
        Err(error) => Err(error),
    }
}

async fn prune_unconfirmed_section_rows(
    tx: &mut Transaction<'_, Postgres>,
    document_id: Uuid,
    keep_keys: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE bid_section_publication_state publication
         SET stale = true, updated_at = now()
         WHERE publication.document_id = $1
           AND NOT (publication.section_key = ANY($2))
           AND publication.current_target_id IS NULL",
    )
    .bind(document_id)
    .bind(keep_keys)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "DELETE FROM bid_sections s
         WHERE s.document_id = $1
           AND NOT (s.section_key = ANY($2))
           AND NOT EXISTS (
               SELECT 1 FROM bid_clauses c
               WHERE c.section_id = s.id AND c.status IN ('confirmed', 'rejected')
           )
           AND NOT EXISTS (
               SELECT 1 FROM bid_section_publication_state publication
               WHERE publication.document_id = s.document_id
                 AND publication.section_key = s.section_key
                 AND publication.current_section_id IS NOT NULL
                 AND NOT publication.removed
           )",
    )
    .bind(document_id)
    .bind(keep_keys)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn fence_running_run(
    tx: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
    project_id: Uuid,
    claim_token: Uuid,
) -> Result<RunningRun, sqlx::Error> {
    let run = sqlx::query_as::<_, (Uuid, Uuid, Option<Uuid>, Uuid, Uuid)>(
        "SELECT id, project_id, document_id, config_snapshot_id, feature_snapshot_id
         FROM bid_extract_runs
         WHERE id = $1 AND project_id = $2 AND claim_token = $3 AND status = 'running'
         FOR UPDATE",
    )
    .bind(run_id)
    .bind(project_id)
    .bind(claim_token)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((id, project_id, document_id, config_snapshot_id, feature_snapshot_id)) = run else {
        return Err(sqlx::Error::Protocol("extract run lease lost".into()));
    };
    sqlx::query("UPDATE bid_extract_runs SET heartbeat_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(RunningRun {
        id,
        project_id,
        document_id,
        config_snapshot_id,
        feature_snapshot_id,
    })
}

fn scoped_section_count(
    explicit: Option<i32>,
    sections: &[ExtractionSectionRow<'_>],
) -> Option<i32> {
    explicit.filter(|count| *count > 0).or_else(|| {
        let count = sections.len() as i32;
        (count > 0).then_some(count)
    })
}

async fn load_document_generation(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    document_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT conversion_generation FROM bid_documents
         WHERE id = $1 AND project_id = $2
         FOR UPDATE",
    )
    .bind(document_id)
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| sqlx::Error::Protocol("extract document missing".into()))
}

async fn lock_head(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    document_id: Uuid,
) -> Result<(i64, Option<Uuid>), sqlx::Error> {
    sqlx::query(
        "INSERT INTO bid_document_extraction_heads
            (document_id, project_id, current_extraction_generation)
         VALUES ($1, $2, 0)
         ON CONFLICT (document_id) DO NOTHING",
    )
    .bind(document_id)
    .bind(project_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query_as(
        "SELECT current_extraction_generation, active_target_id
         FROM bid_document_extraction_heads
         WHERE document_id = $1
         FOR UPDATE",
    )
    .bind(document_id)
    .fetch_one(&mut **tx)
    .await
}

async fn ensure_document_target(
    tx: &mut Transaction<'_, Postgres>,
    run: &RunningRun,
    document_id: Uuid,
    claim_token: Uuid,
    scoped_section_count: Option<i32>,
) -> Result<FencedTarget, sqlx::Error> {
    if run
        .document_id
        .is_some_and(|run_document| run_document != document_id)
    {
        return Err(sqlx::Error::Protocol("extract document mismatch".into()));
    }
    let conversion_generation = load_document_generation(tx, run.project_id, document_id).await?;
    let (head_generation, _active_target) = lock_head(tx, run.project_id, document_id).await?;
    if let Some(existing) = load_run_document_target(tx, run.id, document_id).await? {
        return adopt_existing_target(
            tx,
            existing,
            claim_token,
            head_generation,
            conversion_generation,
        )
        .await;
    }
    let target_kind = if run.document_id.is_some() {
        "document"
    } else {
        "full"
    };
    insert_running_target(
        tx,
        run,
        document_id,
        target_kind,
        None,
        None,
        claim_token,
        conversion_generation,
        head_generation,
        scoped_section_count,
        false,
    )
    .await
}

async fn ensure_section_retry_target(
    tx: &mut Transaction<'_, Postgres>,
    retry_token: Uuid,
    project_id: Uuid,
    document_id: Uuid,
    section: &ExtractionSectionRow<'_>,
) -> Result<FencedTarget, sqlx::Error> {
    let conversion_generation = load_document_generation(tx, project_id, document_id).await?;
    let owned: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, section_key FROM bid_sections
         WHERE id = $1 AND project_id = $2 AND document_id = $3
         FOR UPDATE",
    )
    .bind(section.id)
    .bind(project_id)
    .bind(document_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((source_section_id, section_key)) = owned else {
        return Err(sqlx::Error::Protocol("section retry lease lost".into()));
    };
    if section_key != section.section_key {
        return Err(sqlx::Error::Protocol("section retry key mismatch".into()));
    }
    let (head_generation, _) = lock_head(tx, project_id, document_id).await?;
    if let Some(existing) = load_section_retry_target(tx, document_id, retry_token).await? {
        return adopt_existing_target(
            tx,
            existing,
            retry_token,
            head_generation,
            conversion_generation,
        )
        .await;
    }
    let run_id = Uuid::new_v4();
    // Targets require a parent run, but this row is only a publication parent.
    // Insert it already terminal so it is never selected by next_pending_extract
    // or the housekeeping pending-run query, even if a later statement fails.
    sqlx::query(
        "INSERT INTO bid_extract_runs
            (id, project_id, document_id, status, triggered_by, started_at, finished_at)
         VALUES ($1, $2, $3, 'failed', 'manual', now(), now())",
    )
    .bind(run_id)
    .bind(project_id)
    .bind(document_id)
    .execute(&mut **tx)
    .await?;
    let (config_snapshot_id, feature_snapshot_id) = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT config_snapshot_id, feature_snapshot_id FROM bid_extract_runs WHERE id = $1",
    )
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await?;
    let run = RunningRun {
        id: run_id,
        project_id,
        document_id: Some(document_id),
        config_snapshot_id,
        feature_snapshot_id,
    };
    insert_running_target(
        tx,
        &run,
        document_id,
        "section_retry",
        Some(section.section_key),
        Some(source_section_id),
        retry_token,
        conversion_generation,
        head_generation,
        Some(1),
        true,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn insert_running_target(
    tx: &mut Transaction<'_, Postgres>,
    run: &RunningRun,
    document_id: Uuid,
    target_kind: &str,
    section_key: Option<&str>,
    source_section_id: Option<Uuid>,
    claim_token: Uuid,
    conversion_generation: i64,
    head_generation: i64,
    scoped_section_count: Option<i32>,
    outline_complete: bool,
) -> Result<FencedTarget, sqlx::Error> {
    let foreign: Option<Uuid> = sqlx::query_scalar(
        "SELECT run_id FROM bid_extract_run_targets
         WHERE document_id = $1 AND status IN ('running', 'publishing')
         FOR UPDATE",
    )
    .bind(document_id)
    .fetch_optional(&mut **tx)
    .await?;
    if foreign.is_some_and(|owner| owner != run.id) {
        return Err(sqlx::Error::Protocol(
            "extract document publisher already active".into(),
        ));
    }
    sqlx::query(
        "UPDATE bid_document_extraction_heads
         SET active_target_id = NULL, updated_at = now()
         WHERE document_id = $1",
    )
    .bind(document_id)
    .execute(&mut **tx)
    .await?;
    let extraction_generation = head_generation + 1;
    let target_id = Uuid::new_v4();
    let frozen = outline_complete || scoped_section_count.is_some();
    let scoped = if frozen {
        Some(scoped_section_count.unwrap_or(1).max(1))
    } else {
        None
    };
    sqlx::query(
        "INSERT INTO bid_extract_run_targets
            (id, run_id, project_id, document_id, target_kind, section_key, source_section_id,
             expected_conversion_generation, extraction_generation, status,
             claim_token, heartbeat_at, claimed_at, attempt, max_attempts,
             config_snapshot_id, feature_snapshot_id,
             outline_complete, scoped_section_count,
             policy_version, prompt_version, schema_version)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'running',$10,now(),now(),1,3,$11,$12,$13,$14,$15,$16,$17)",
    )
    .bind(target_id)
    .bind(run.id)
    .bind(run.project_id)
    .bind(document_id)
    .bind(target_kind)
    .bind(section_key)
    .bind(source_section_id)
    .bind(conversion_generation)
    .bind(extraction_generation)
    .bind(claim_token)
    .bind(run.config_snapshot_id)
    .bind(run.feature_snapshot_id)
    .bind(frozen)
    .bind(scoped)
    .bind(POLICY_VERSION)
    .bind(PROMPT_VERSION)
    .bind(SCHEMA_VERSION)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE bid_document_extraction_heads
         SET current_extraction_generation = $2, active_target_id = $3, updated_at = now()
         WHERE document_id = $1",
    )
    .bind(document_id)
    .bind(extraction_generation)
    .bind(target_id)
    .execute(&mut **tx)
    .await?;
    Ok(FencedTarget {
        id: target_id,
        run_id: run.id,
        project_id: run.project_id,
        document_id,
        extraction_generation,
        expected_conversion_generation: conversion_generation,
        claim_token,
    })
}

async fn load_run_document_target(
    tx: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
    document_id: Uuid,
) -> Result<Option<(Uuid, String, Option<Uuid>, i64, i64)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, status, claim_token, extraction_generation, expected_conversion_generation
         FROM bid_extract_run_targets
         WHERE run_id = $1 AND document_id = $2 AND target_kind IN ('full', 'document')
         FOR UPDATE",
    )
    .bind(run_id)
    .bind(document_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn load_section_retry_target(
    tx: &mut Transaction<'_, Postgres>,
    document_id: Uuid,
    claim_token: Uuid,
) -> Result<Option<(Uuid, String, Option<Uuid>, i64, i64)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, status, claim_token, extraction_generation, expected_conversion_generation
         FROM bid_extract_run_targets
         WHERE document_id = $1 AND target_kind = 'section_retry' AND claim_token = $2
           AND status IN ('running', 'publishing')
         FOR UPDATE",
    )
    .bind(document_id)
    .bind(claim_token)
    .fetch_optional(&mut **tx)
    .await
}

async fn adopt_existing_target(
    tx: &mut Transaction<'_, Postgres>,
    existing: (Uuid, String, Option<Uuid>, i64, i64),
    claim_token: Uuid,
    head_generation: i64,
    conversion_generation: i64,
) -> Result<FencedTarget, sqlx::Error> {
    let (target_id, status, target_token, extraction_generation, expected_conversion) = existing;
    let (run_id, project_id, document_id) = sqlx::query_as::<_, (Uuid, Uuid, Uuid)>(
        "SELECT run_id, project_id, document_id FROM bid_extract_run_targets WHERE id = $1",
    )
    .bind(target_id)
    .fetch_one(&mut **tx)
    .await?;
    if !matches!(status.as_str(), "running" | "publishing") {
        return Err(sqlx::Error::Protocol("extract target lease lost".into()));
    }
    if target_token != Some(claim_token) {
        return Err(sqlx::Error::Protocol("extract run lease lost".into()));
    }
    if extraction_generation != head_generation || expected_conversion != conversion_generation {
        stale_or_supersede_target(tx, target_id).await?;
        return Err(sqlx::Error::Protocol(GENERATION_STALE.into()));
    }
    sqlx::query(
        "UPDATE bid_extract_run_targets SET heartbeat_at = now(), updated_at = now()
         WHERE id = $1 AND claim_token = $2 AND status IN ('running', 'publishing')",
    )
    .bind(target_id)
    .bind(claim_token)
    .execute(&mut **tx)
    .await?;
    Ok(FencedTarget {
        id: target_id,
        run_id,
        project_id,
        document_id,
        extraction_generation,
        expected_conversion_generation: expected_conversion,
        claim_token,
    })
}

async fn persist_report_sections(
    tx: &mut Transaction<'_, Postgres>,
    target: &FencedTarget,
    sections: &[ExtractionSectionRow<'_>],
    clauses: &[ExtractionClauseRow<'_>],
) -> Result<(), sqlx::Error> {
    for (ordinal, section) in sections.iter().enumerate() {
        let section_clauses: Vec<&ExtractionClauseRow<'_>> = clauses
            .iter()
            .filter(|clause| clause.section_key == section.section_key)
            .collect();
        persist_one_section(tx, target, section, &section_clauses, ordinal as i32).await?;
    }
    Ok(())
}

async fn persist_one_section(
    tx: &mut Transaction<'_, Postgres>,
    target: &FencedTarget,
    section: &ExtractionSectionRow<'_>,
    clauses: &[&ExtractionClauseRow<'_>],
    outline_ordinal: i32,
) -> Result<(), sqlx::Error> {
    let existing = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, status FROM bid_extract_section_candidates
         WHERE target_id = $1 AND section_key = $2
         FOR UPDATE",
    )
    .bind(target.id)
    .bind(section.section_key)
    .fetch_optional(&mut **tx)
    .await?;
    if existing
        .as_ref()
        .is_some_and(|(_, status)| status == "published")
    {
        return Ok(());
    }
    if existing
        .as_ref()
        .is_some_and(|(_, status)| !matches!(status.as_str(), "pending" | "running"))
    {
        return Ok(());
    }
    let candidate_id = match existing {
        Some((id, _)) => id,
        None => insert_pending_section_candidate(tx, target, section, outline_ordinal).await?,
    };
    sqlx::query(
        "UPDATE bid_extract_section_candidates
         SET heading_path = $2, body = $3, outline_ordinal = $4
         WHERE id = $1 AND status IN ('pending', 'running')",
    )
    .bind(candidate_id)
    .bind(candidate_heading_path(section.heading_path))
    .bind(section.body)
    .bind(outline_ordinal)
    .execute(&mut **tx)
    .await?;
    match section.extract_status {
        "done" | "succeeded" => {
            write_clause_candidates(tx, target, candidate_id, clauses).await?;
            sqlx::query(
                "UPDATE bid_extract_section_candidates
                 SET status = 'succeeded', finished_at = now(),
                     quality_status = 'review', reason_codes = '{}'
                 WHERE id = $1 AND status IN ('pending', 'running')",
            )
            .bind(candidate_id)
            .execute(&mut **tx)
            .await?;
            publish_section(tx, target, candidate_id, section, clauses).await?;
        }
        "failed" | "skipped" => {
            sqlx::query(
                "UPDATE bid_extract_section_candidates
                 SET status = 'failed', finished_at = now(),
                     quality_status = 'block', reason_codes = ARRAY['OTHER_BOUNDED']::text[]
                 WHERE id = $1 AND status IN ('pending', 'running')",
            )
            .bind(candidate_id)
            .execute(&mut **tx)
            .await?;
        }
        _ => {
            sqlx::query(
                "UPDATE bid_extract_section_candidates
                 SET status = CASE WHEN status = 'pending' THEN 'running' ELSE status END
                 WHERE id = $1 AND status = 'pending'",
            )
            .bind(candidate_id)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

async fn insert_pending_section_candidate(
    tx: &mut Transaction<'_, Postgres>,
    target: &FencedTarget,
    section: &ExtractionSectionRow<'_>,
    outline_ordinal: i32,
) -> Result<Uuid, sqlx::Error> {
    let candidate_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO bid_extract_section_candidates
            (id, target_id, run_id, project_id, document_id,
             expected_conversion_generation, extraction_generation,
             section_key, heading_path, outline_ordinal, body, status,
             provider_id, model_id, policy_version, prompt_version, schema_version,
             diagnostics, idempotency_key)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'pending',$12,$13,$14,$15,$16,'{}'::jsonb,$17)",
    )
    .bind(candidate_id)
    .bind(target.id)
    .bind(target.run_id)
    .bind(target.project_id)
    .bind(target.document_id)
    .bind(target.expected_conversion_generation)
    .bind(target.extraction_generation)
    .bind(section.section_key)
    .bind(candidate_heading_path(section.heading_path))
    .bind(outline_ordinal)
    .bind(section.body)
    .bind(PROVIDER_ID)
    .bind(MODEL_ID)
    .bind(POLICY_VERSION)
    .bind(PROMPT_VERSION)
    .bind(SCHEMA_VERSION)
    .bind(format!("{}:{}", target.id, section.section_key))
    .execute(&mut **tx)
    .await?;
    Ok(candidate_id)
}

async fn write_clause_candidates(
    tx: &mut Transaction<'_, Postgres>,
    target: &FencedTarget,
    section_candidate_id: Uuid,
    clauses: &[&ExtractionClauseRow<'_>],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM bid_extract_clause_candidates WHERE section_candidate_id = $1")
        .bind(section_candidate_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM bid_extract_span_candidates WHERE section_candidate_id = $1")
        .bind(section_candidate_id)
        .execute(&mut **tx)
        .await?;
    for (ordinal, clause) in clauses.iter().enumerate() {
        let span_id = Uuid::new_v4();
        let source_span = candidate_source_span(clause.source_span);
        let span_key = clause
            .source_span
            .get("span_id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| clause.id.to_string());
        sqlx::query(
            "INSERT INTO bid_extract_span_candidates
                (id, section_candidate_id, target_id, span_key, outline_ordinal,
                 source_span, disposition, status)
             VALUES ($1,$2,$3,$4,$5,$6,'clause','pending')",
        )
        .bind(span_id)
        .bind(section_candidate_id)
        .bind(target.id)
        .bind(span_key)
        .bind(ordinal as i32)
        .bind(&source_span)
        .execute(&mut **tx)
        .await?;
        sqlx::query("UPDATE bid_extract_span_candidates SET status = 'succeeded' WHERE id = $1")
            .bind(span_id)
            .execute(&mut **tx)
            .await?;
        sqlx::query(
            "INSERT INTO bid_extract_clause_candidates
                (id, section_candidate_id, span_candidate_id, raw_text, text, family, must,
                 source_span, extraction_meta, quality_status)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'{}'::jsonb,'review')",
        )
        .bind(clause.id)
        .bind(section_candidate_id)
        .bind(span_id)
        .bind(non_empty_text(clause.raw_text))
        .bind(non_empty_text(clause.text))
        .bind(clause.family)
        .bind(clause.must)
        .bind(&source_span)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn fence_publish_target(
    tx: &mut Transaction<'_, Postgres>,
    target: &FencedTarget,
) -> Result<bool, sqlx::Error> {
    // Schedule and publish share this lock order: head, then target.
    let (head_generation, _) = lock_head(tx, target.project_id, target.document_id).await?;
    let conversion_generation =
        load_document_generation(tx, target.project_id, target.document_id).await?;
    let fenced: Option<(i64, i64, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT extraction_generation, expected_conversion_generation, status, claim_token
         FROM bid_extract_run_targets
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(target.id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((extraction_generation, expected_conversion, status, claim_token)) = fenced else {
        return Ok(false);
    };
    if !matches!(status.as_str(), "running" | "publishing")
        || claim_token != Some(target.claim_token)
        || extraction_generation != head_generation
        || expected_conversion != conversion_generation
    {
        stale_or_supersede_target(tx, target.id).await?;
        return Ok(false);
    }
    Ok(true)
}

async fn publish_section(
    tx: &mut Transaction<'_, Postgres>,
    target: &FencedTarget,
    candidate_id: Uuid,
    section: &ExtractionSectionRow<'_>,
    clauses: &[&ExtractionClauseRow<'_>],
) -> Result<(), sqlx::Error> {
    if !fence_publish_target(tx, target).await? {
        return Err(sqlx::Error::Protocol(GENERATION_STALE.into()));
    }
    let section_id: Uuid = sqlx::query_scalar(
        "INSERT INTO bid_sections
            (id, project_id, document_id, section_key, heading_path, hint_family,
             body, extract_status, error_message)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
         ON CONFLICT (document_id, section_key) DO UPDATE SET
            heading_path = EXCLUDED.heading_path,
            hint_family = EXCLUDED.hint_family,
            body = EXCLUDED.body,
            extract_status = EXCLUDED.extract_status,
            error_message = EXCLUDED.error_message
         RETURNING id",
    )
    .bind(section.id)
    .bind(target.project_id)
    .bind(target.document_id)
    .bind(section.section_key)
    .bind(section.heading_path)
    .bind(section.hint_family)
    .bind(section.body)
    .bind(section.extract_status)
    .bind(section.error_message)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE bid_clauses SET status = 'superseded', superseded_by_run_id = $2
         WHERE section_id = $1 AND status IN ('draft', 'confirmed')",
    )
    .bind(section_id)
    .bind(target.run_id)
    .execute(&mut **tx)
    .await?;
    let matching_relevant = true;
    for clause in clauses {
        sqlx::query(
            "INSERT INTO bid_clauses
                (id, project_id, extract_run_id, section_id, source_document_id,
                 source_span, family_conflict, extraction_meta,
                 raw_text, text, family, must, status, confirmed_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'confirmed', now())",
        )
        .bind(clause.id)
        .bind(target.project_id)
        .bind(target.run_id)
        .bind(section_id)
        .bind(target.document_id)
        .bind(clause.source_span)
        .bind(clause.family_conflict)
        .bind(clause.extraction_meta)
        .bind(clause.raw_text)
        .bind(clause.text)
        .bind(clause.family)
        .bind(clause.must)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        "UPDATE bid_extract_section_candidates
         SET status = 'published', published_at = now()
         WHERE id = $1 AND status = 'succeeded'",
    )
    .bind(candidate_id)
    .execute(&mut **tx)
    .await?;
    let bumped = sqlx::query(
        "UPDATE bid_extract_run_targets
         SET published_section_count = published_section_count + 1,
             heartbeat_at = now(), updated_at = now()
         WHERE id = $1 AND claim_token = $2 AND status IN ('running', 'publishing')
           AND scoped_section_count IS NOT NULL
           AND published_section_count < scoped_section_count",
    )
    .bind(target.id)
    .bind(target.claim_token)
    .execute(&mut **tx)
    .await?;
    if bumped.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "extract target publication count cas lost".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO bid_section_publication_state
            (document_id, section_key, project_id, current_run_id, current_target_id,
             current_section_candidate_id, current_section_id, published_extraction_generation,
             last_attempt_run_id, last_attempt_target_id, quality_status, updated_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$4,$5,'review',now())
         ON CONFLICT (document_id, section_key) DO UPDATE SET
            current_run_id = EXCLUDED.current_run_id,
            current_target_id = EXCLUDED.current_target_id,
            current_section_candidate_id = EXCLUDED.current_section_candidate_id,
            current_section_id = EXCLUDED.current_section_id,
            published_extraction_generation = EXCLUDED.published_extraction_generation,
            last_attempt_run_id = EXCLUDED.last_attempt_run_id,
            last_attempt_target_id = EXCLUDED.last_attempt_target_id,
            quality_status = EXCLUDED.quality_status,
            stale = false,
            removed = false,
            updated_at = now()
         WHERE bid_section_publication_state.published_extraction_generation IS NULL
            OR bid_section_publication_state.published_extraction_generation
               < EXCLUDED.published_extraction_generation",
    )
    .bind(target.document_id)
    .bind(section.section_key)
    .bind(target.project_id)
    .bind(target.run_id)
    .bind(target.id)
    .bind(candidate_id)
    .bind(section_id)
    .bind(target.extraction_generation)
    .execute(&mut **tx)
    .await?;
    if matching_relevant {
        crate::bid_matching::mark_project_matching_mutation(tx, target.project_id).await?;
    }
    Ok(())
}

async fn stale_or_supersede_target(
    tx: &mut Transaction<'_, Postgres>,
    target_id: Uuid,
) -> Result<(), sqlx::Error> {
    close_open_candidates(tx, target_id, "stale").await?;
    let published: i32 = sqlx::query_scalar(
        "SELECT count(*)::int FROM bid_extract_section_candidates
         WHERE target_id = $1 AND status = 'published'",
    )
    .bind(target_id)
    .fetch_one(&mut **tx)
    .await?;
    let status = if published > 0 { "superseded" } else { "stale" };
    apply_terminal_target(tx, target_id, status, true).await
}

async fn terminalize_target(
    tx: &mut Transaction<'_, Postgres>,
    target_id: Uuid,
) -> Result<(), sqlx::Error> {
    let status: String =
        sqlx::query_scalar("SELECT status FROM bid_extract_run_targets WHERE id = $1 FOR UPDATE")
            .bind(target_id)
            .fetch_one(&mut **tx)
            .await?;
    if !matches!(status.as_str(), "pending" | "running" | "publishing") {
        return Ok(());
    }
    close_open_candidates(tx, target_id, "failed").await?;
    let (published, scoped, target_kind): (i32, Option<i32>, String) = sqlx::query_as(
        "SELECT
            (SELECT count(*)::int FROM bid_extract_section_candidates
              WHERE target_id = $1 AND status = 'published'),
            scoped_section_count,
            target_kind
         FROM bid_extract_run_targets WHERE id = $1",
    )
    .bind(target_id)
    .fetch_one(&mut **tx)
    .await?;
    let failed: i32 = sqlx::query_scalar(
        "SELECT count(*)::int FROM bid_extract_section_candidates
         WHERE target_id = $1 AND (status IN ('failed', 'cancelled') OR quality_status = 'block')",
    )
    .bind(target_id)
    .fetch_one(&mut **tx)
    .await?;
    let complete = scoped.is_some_and(|count| count > 0 && published == count && failed == 0);
    let terminal = if complete { "published" } else { "failed" };
    if complete && matches!(target_kind.as_str(), "full" | "document") {
        sqlx::query("UPDATE bid_extract_run_targets SET cleanup_completed = true WHERE id = $1")
            .bind(target_id)
            .execute(&mut **tx)
            .await?;
    }
    apply_terminal_target(tx, target_id, terminal, false).await
}

async fn close_open_candidates(
    tx: &mut Transaction<'_, Postgres>,
    target_id: Uuid,
    leftover: &str,
) -> Result<(), sqlx::Error> {
    let leftover_status = match leftover {
        "stale" => "stale",
        "cancelled" => "cancelled",
        _ => "failed",
    };
    sqlx::query(
        "UPDATE bid_extract_section_candidates
         SET status = $2, finished_at = now(),
             quality_status = CASE WHEN $2 = 'failed' THEN 'block' ELSE quality_status END,
             reason_codes = CASE WHEN $2 = 'failed' THEN ARRAY['OTHER_BOUNDED']::text[] ELSE reason_codes END
         WHERE target_id = $1 AND status IN ('pending', 'running')",
    )
    .bind(target_id)
    .bind(leftover_status)
    .execute(&mut **tx)
    .await?;
    let succeeded_leftover = if leftover_status == "failed" {
        "stale"
    } else {
        leftover_status
    };
    sqlx::query(
        "UPDATE bid_extract_section_candidates
         SET status = $2, finished_at = COALESCE(finished_at, now())
         WHERE target_id = $1 AND status = 'succeeded'",
    )
    .bind(target_id)
    .bind(succeeded_leftover)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn apply_terminal_target(
    tx: &mut Transaction<'_, Postgres>,
    target_id: Uuid,
    status: &str,
    preserve_published_count: bool,
) -> Result<(), sqlx::Error> {
    let (published, failed, degraded, quality, reasons): (i32, i32, bool, String, Vec<String>) =
        sqlx::query_as(
            "SELECT
                count(*) FILTER (WHERE status = 'published')::int,
                count(*) FILTER (
                    WHERE status IN ('failed', 'cancelled') OR quality_status = 'block'
                )::int,
                COALESCE(bool_or(degraded), false),
                CASE
                    WHEN bool_or(quality_status = 'block') THEN 'block'
                    WHEN bool_or(quality_status = 'review') OR count(*) = 0 THEN 'review'
                    ELSE 'pass'
                END,
                COALESCE((
                    SELECT array_agg(reason ORDER BY reason)
                    FROM (
                        SELECT DISTINCT unnest(reason_codes) AS reason
                        FROM bid_extract_section_candidates
                        WHERE target_id = $1
                    ) reasons
                ), '{}'::text[])
             FROM bid_extract_section_candidates
             WHERE target_id = $1",
        )
        .bind(target_id)
        .fetch_one(&mut **tx)
        .await?;
    sqlx::query(
        "UPDATE bid_extract_run_targets SET
            status = $2,
            claim_token = NULL,
            heartbeat_at = NULL,
            finished_at = now(),
            published_section_count = CASE WHEN $8 THEN published_section_count ELSE $3 END,
            partial_failure = $4,
            worst_quality_status = $5,
            aggregate_degraded = $6,
            aggregate_reason_codes = $7,
            updated_at = now()
         WHERE id = $1 AND status IN ('pending', 'running', 'publishing')",
    )
    .bind(target_id)
    .bind(status)
    .bind(published)
    .bind(failed > 0)
    .bind(quality)
    .bind(degraded)
    .bind(&reasons)
    .bind(preserve_published_count)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn finish_section_retry_run(
    tx: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE bid_extract_runs SET
            status = CASE WHEN status IN ('pending', 'running') THEN 'failed' ELSE status END,
            claim_token = NULL,
            heartbeat_at = NULL,
            finished_at = COALESCE(finished_at, now())
         WHERE id = $1",
    )
    .bind(run_id)
    .execute(&mut **tx)
    .await?;
    refresh_extract_run_aggregates(tx, run_id).await
}

async fn refresh_extract_run_aggregates(
    tx: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
) -> Result<(), sqlx::Error> {
    let (targets, published_targets, scoped, incomplete, failure, quality, degraded, reasons, published): (
        i64,
        i64,
        i64,
        bool,
        bool,
        String,
        bool,
        Vec<String>,
        i64,
    ) = sqlx::query_as(
        "SELECT
            count(*)::bigint,
            count(*) FILTER (
                WHERE EXISTS (
                    SELECT 1 FROM bid_extract_section_candidates candidate
                    WHERE candidate.target_id = target.id AND candidate.status = 'published'
                      AND EXISTS (
                          SELECT 1 FROM bid_section_publication_state publication
                          WHERE publication.document_id = candidate.document_id
                            AND publication.section_key = candidate.section_key
                            AND (publication.current_section_candidate_id = candidate.id
                              OR publication.published_extraction_generation > candidate.extraction_generation)
                      )
                )
            )::bigint,
            COALESCE(sum(scoped_section_count), 0)::bigint,
            COALESCE(bool_or(NOT outline_complete), false),
            COALESCE(bool_or(partial_failure OR status IN ('failed', 'stale', 'cancelled')), false),
            CASE
                WHEN bool_or(worst_quality_status = 'block') THEN 'block'
                WHEN bool_or(worst_quality_status = 'review') OR count(*) = 0 THEN 'review'
                ELSE 'pass'
            END,
            COALESCE(bool_or(aggregate_degraded), false),
            COALESCE((
                SELECT array_agg(reason ORDER BY reason)
                FROM (
                    SELECT DISTINCT unnest(aggregate_reason_codes) AS reason
                    FROM bid_extract_run_targets
                    WHERE run_id = $1
                ) reasons
            ), '{}'::text[]),
            (
                SELECT count(*)::bigint FROM bid_extract_section_candidates
                WHERE run_id = $1 AND status = 'published'
            )
         FROM bid_extract_run_targets target
         WHERE run_id = $1",
    )
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await?;
    let scoped_section_count = if incomplete { None } else { Some(scoped) };
    sqlx::query(
        "UPDATE bid_extract_runs SET
            status = CASE
                WHEN status IN ('done', 'failed') AND $2 > 0 THEN 'done'
                WHEN status IN ('done', 'failed') THEN 'failed'
                ELSE status
            END,
            target_count = $3,
            published_target_count = $4,
            scoped_section_count = $5,
            published_section_count = $6,
            partial_failure = $7,
            worst_quality_status = $8,
            degraded = $9,
            reason_codes = $10
         WHERE id = $1",
    )
    .bind(run_id)
    .bind(published)
    .bind(targets)
    .bind(published_targets)
    .bind(scoped_section_count)
    .bind(published)
    .bind(failure)
    .bind(quality)
    .bind(degraded)
    .bind(&reasons)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn candidate_heading_path(path: &str) -> Vec<String> {
    path.split(" / ")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .take(16)
        .map(|part| part.chars().take(256).collect())
        .collect()
}

fn candidate_source_span(value: &serde_json::Value) -> serde_json::Value {
    let start = value
        .get("start")
        .and_then(|item| item.as_i64())
        .filter(|item| *item >= 0)
        .unwrap_or(0);
    let end = value
        .get("end")
        .and_then(|item| item.as_i64())
        .filter(|item| *item > start)
        .unwrap_or(start + 1);
    serde_json::json!({ "start": start, "end": end })
}

fn non_empty_text(value: &str) -> &str {
    if value.is_empty() { " " } else { value }
}
