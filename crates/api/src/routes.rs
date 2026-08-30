use crate::err::{ErrorBody, fail, forbidden, not_found, unauthorized, validation};
use crate::{AppState, HealthBody};
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State};
use axum::handler::Handler;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{MethodRouter, delete, get, patch, post, put};
use axum::{Json, Router};

fn get_s<H, T>(h: H) -> MethodRouter<AppState>
where
    H: Handler<T, AppState>,
    T: 'static,
{
    get::<H, T, AppState>(h)
}

fn post_s<H, T>(h: H) -> MethodRouter<AppState>
where
    H: Handler<T, AppState>,
    T: 'static,
{
    post::<H, T, AppState>(h)
}

fn patch_s<H, T>(h: H) -> MethodRouter<AppState>
where
    H: Handler<T, AppState>,
    T: 'static,
{
    patch::<H, T, AppState>(h)
}

fn put_s<H, T>(h: H) -> MethodRouter<AppState>
where
    H: Handler<T, AppState>,
    T: 'static,
{
    put::<H, T, AppState>(h)
}

fn delete_s<H, T>(h: H) -> MethodRouter<AppState>
where
    H: Handler<T, AppState>,
    T: 'static,
{
    delete::<H, T, AppState>(h)
}
use knowledge::{
    ApiKey, Document, ParseStatus, Product, ProductKind, ProductVersion, Role, Tag, VersionStatus,
    Workspace, is_audio_type, is_image_type, is_valid_file_type, is_video,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

pub fn build(state: AppState) -> Router {
    let app = Router::<AppState>::new()
        .merge(crate::bid_v2_routes::router())
        .route("/health", get_s(health))
        .route("/live", get_s(live))
        .route("/ready", get_s(ready))
        .route("/api/v1/auth/register", post_s(register))
        .route("/api/v1/auth/login", post_s(login))
        .route("/api/v1/me", get_s(me).merge(patch_s(patch_me)))
        .route(
            "/api/v1/workspaces",
            get_s(list_workspaces).merge(post_s(create_workspace)),
        )
        .route(
            "/api/v1/workspaces/{id}",
            get_s(get_workspace)
                .merge(patch_s(patch_workspace))
                .merge(delete_s(delete_workspace)),
        )
        .route(
            "/api/v1/workspaces/{id}/members",
            get_s(list_members).merge(post_s(add_member)),
        )
        .route(
            "/api/v1/workspaces/{id}/members/{user_id}",
            patch_s(patch_member).merge(delete_s(remove_member)),
        )
        .route(
            "/api/v1/workspaces/{id}/retrieval-config",
            get_s(get_retrieval).merge(patch_s(patch_retrieval)),
        )
        .route(
            "/api/v1/workspaces/{id}/api-keys",
            get_s(list_api_keys).merge(post_s(create_api_key)),
        )
        .route(
            "/api/v1/workspaces/{id}/api-keys/{key_id}",
            delete_s(delete_api_key),
        )
        .route(
            "/api/v1/workspaces/{id}/products",
            get_s(list_products).merge(post_s(create_product)),
        )
        .route(
            "/api/v1/products/{id}",
            get_s(get_product)
                .merge(patch_s(patch_product))
                .merge(delete_s(delete_product)),
        )
        .route(
            "/api/v1/products/{id}/versions",
            get_s(list_versions).merge(post_s(create_version)),
        )
        .route(
            "/api/v1/products/{id}/versions/{version_id}",
            get_s(get_version)
                .merge(patch_s(patch_version))
                .merge(delete_s(delete_version)),
        )
        .route("/api/v1/products/{id}/current-version", post_s(set_current))
        .route(
            "/api/v1/products/{id}/versions/{version_id}/documents",
            get_s(list_documents),
        )
        .route(
            "/api/v1/products/{id}/versions/{version_id}/documents/file",
            post_s(ingest_file),
        )
        .route(
            "/api/v1/products/{id}/versions/{version_id}/documents/url",
            post_s(ingest_url),
        )
        .route(
            "/api/v1/products/{id}/versions/{version_id}/documents/passage",
            post_s(ingest_passage),
        )
        .route(
            "/api/v1/products/{id}/versions/{version_id}/documents/manual",
            post_s(ingest_manual),
        )
        .route(
            "/api/v1/documents/{id}",
            get_s(get_document).merge(delete_s(delete_document)),
        )
        .route("/api/v1/documents/{id}/reparse", post_s(reparse_document))
        .route("/api/v1/documents/{id}/cancel", post_s(cancel_document))
        .route("/api/v1/documents/{id}/timeline", get_s(timeline))
        .route("/api/v1/documents/{id}/content", get_s(document_content))
        .route(
            "/api/v1/workspaces/{id}/tags",
            get_s(list_tags).merge(post_s(create_tag)),
        )
        .route(
            "/api/v1/workspaces/{id}/tags/{tag_id}",
            delete_s(delete_tag),
        )
        .route("/api/v1/documents/{id}/tags", put_s(put_tags))
        .route(
            "/api/v1/products/{id}/versions/{vid}/wiki/pages",
            get_s(wiki_pages),
        )
        .route(
            "/api/v1/products/{id}/versions/{vid}/wiki/pages/{slug}",
            get_s(wiki_page),
        )
        .route("/api/v1/search", post_s(do_search))
        .route("/api/v1/match", post_s(do_match))
        .route("/api/v1/answer", post_s(do_answer))
        .route(
            "/api/v1/products/{id}/versions/{vid}/wiki/folders",
            get_s(wiki_folders),
        )
        .route(
            "/api/v1/products/{id}/versions/{vid}/files",
            get_s(version_file),
        )
        .route("/api/v1/files", get_s(global_file))
        .route("/api/v1/system/parser-engines", get_s(list_parser_engines))
        .route(
            "/api/v1/models",
            get_s(list_models).merge(post_s(create_model)),
        )
        .route("/api/v1/models/{id}", patch_s(patch_model))
        .route("/api/v1/ops/dead-letters", get_s(dead_letters))
        .route("/api/v1/ops/queues", get_s(list_queues))
        .route("/api/v1/ops/oxana", get_s(ops_oxana))
        .route("/metrics", get_s(metrics))
        .layer(DefaultBodyLimit::max(
            platform::max_file_bytes() + 1024 * 1024,
        ));
    let app = if let Some((storage, catalog)) = platform::dashboard_catalog() {
        let ui = oxana_web::router(oxana_web::OxanaWebState::new(
            storage,
            catalog,
            "/api/v1/ops/oxana/web".into(),
        ));
        app.nest_service(
            "/api/v1/ops/oxana/web",
            ui.layer(middleware::from_fn_with_state(
                state.clone(),
                oxana_admin_gate,
            )),
        )
    } else {
        app
    };
    let app = app.layer(tower_http::cors::CorsLayer::permissive());
    let app = if let Ok(dir) = std::env::var("KNOWLEDGEBRAIN_WEB_ROOT") {
        let index = std::path::Path::new(&dir).join("index.html");
        app.fallback_service(
            tower_http::services::ServeDir::new(dir)
                .append_index_html_on_directories(true)
                .not_found_service(tower_http::services::ServeFile::new(index)),
        )
    } else {
        app
    };
    app.with_state(state)
}

async fn oxana_admin_gate(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor).await?;
    Ok(next.run(request).await)
}

async fn health(State(_state): State<AppState>) -> Json<HealthBody> {
    Json(HealthBody {
        status: "ok",
        service: "api",
    })
}

async fn live() -> Json<knowledge::LiveBody> {
    Json(platform::live_body("api"))
}

async fn ready() -> (StatusCode, Json<knowledge::ReadyBody>) {
    let check = knowledge::check_readiness().await;
    let status = if check.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(platform::ready_body("api", &check)))
}

pub(crate) type ApiErr = (StatusCode, Json<ErrorBody>);

async fn pg() -> Result<sqlx::PgPool, ApiErr> {
    platform::connect()
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))
}

fn pg_err(e: impl ToString) -> ApiErr {
    fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string())
}

async fn resolve_vid(
    pool: &sqlx::PgPool,
    product_id: Uuid,
    version_id: &str,
) -> Result<Uuid, ApiErr> {
    knowledge::resolve_product_version_id(pool, product_id, version_id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| {
            if version_id == "current" {
                validation("no current version")
            } else {
                not_found("version")
            }
        })
}

#[derive(Clone)]
pub(crate) enum Actor {
    User(Uuid),
    Key(ApiKey),
    Bootstrap,
}

impl Actor {
    fn user_id(&self) -> Option<Uuid> {
        match self {
            Self::User(id) => Some(*id),
            _ => None,
        }
    }
}

pub(crate) async fn actor_from(headers: &HeaderMap, state: &AppState) -> Result<Actor, ApiErr> {
    if let Some(raw) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        if !state.bootstrap_key.is_empty() && raw == state.bootstrap_key {
            return Ok(Actor::Bootstrap);
        }
        let hash = platform::hash_password(raw);
        let Ok(pool) = platform::connect().await else {
            return Err(unauthorized());
        };
        let key = knowledge::find_api_key_by_hash(&pool, &hash)
            .await
            .map_err(pg_err)?
            .ok_or_else(unauthorized)?;
        return Ok(Actor::Key(key));
    }
    let raw = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(unauthorized)?;
    let token = raw.strip_prefix("Bearer ").ok_or_else(unauthorized)?;
    let uid = platform::parse_jwt(token, &state.jwt_secret).map_err(|_| unauthorized())?;
    Ok(Actor::User(uid))
}

async fn user_from(headers: &HeaderMap, state: &AppState) -> Result<Uuid, ApiErr> {
    actor_from(headers, state)
        .await?
        .user_id()
        .ok_or_else(unauthorized)
}

#[allow(dead_code)]
fn key_role(scopes: &[String]) -> Role {
    if scopes.iter().any(|s| s == "admin") {
        Role::Admin
    } else if scopes.iter().any(|s| s == "ingest") {
        Role::Contributor
    } else {
        Role::Viewer
    }
}

fn require_ws(_ws: Uuid, actor: &Actor, _write: bool, _admin: bool) -> Result<Role, ApiErr> {
    match actor {
        Actor::User(_) | Actor::Bootstrap | Actor::Key(_) => Ok(Role::Owner),
    }
}

#[derive(Deserialize)]
struct AuthBody {
    #[serde(default)]
    email: String,
    #[serde(default)]
    password: String,
}

#[derive(Serialize)]
struct TokenBody {
    token: String,
    user_id: Uuid,
}

async fn register(
    State(_state): State<AppState>,
    Json(_body): Json<AuthBody>,
) -> Result<Json<TokenBody>, ApiErr> {
    Err(fail(
        StatusCode::GONE,
        "GONE",
        "registration is disabled; use LDAP login",
    ))
}

#[allow(dead_code)]
async fn register_local(
    State(state): State<AppState>,
    Json(body): Json<AuthBody>,
) -> Result<Json<TokenBody>, ApiErr> {
    let pool = pg().await?;
    if knowledge::find_user_by_email(&pool, &body.email)
        .await
        .map_err(pg_err)?
        .is_some()
    {
        return Err(fail(StatusCode::CONFLICT, "CONFLICT", "email taken"));
    }
    let hash = platform::hash_password(&body.password);
    let id = Uuid::new_v4();
    knowledge::insert_user(&pool, id, &body.email, Some(&hash))
        .await
        .map_err(pg_err)?;
    let token =
        platform::issue_jwt(id, &state.jwt_secret).map_err(|e| validation(&e.to_string()))?;
    Ok(Json(TokenBody { token, user_id: id }))
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<AuthBody>,
) -> Result<Json<TokenBody>, ApiErr> {
    if platform::local_open() {
        let email = {
            let t = body.email.trim();
            if t.is_empty() {
                "dev@local".into()
            } else {
                t.to_string()
            }
        };
        let id = ensure_local_user(&state, &email).await?;
        let token =
            platform::issue_jwt(id, &state.jwt_secret).map_err(|e| validation(&e.to_string()))?;
        return Ok(Json(TokenBody { token, user_id: id }));
    }
    if !platform::ldap_url().is_empty() {
        let _dn = platform::ldap_bind(&body.email, &body.password).map_err(|_| unauthorized())?;
        let pool = pg().await?;
        let id = if let Some((id, _, _)) = knowledge::find_user_by_email(&pool, &body.email)
            .await
            .map_err(pg_err)?
        {
            id
        } else {
            let id = Uuid::new_v4();
            knowledge::insert_user(&pool, id, &body.email, None)
                .await
                .map_err(pg_err)?;
            id
        };
        let token =
            platform::issue_jwt(id, &state.jwt_secret).map_err(|e| validation(&e.to_string()))?;
        return Ok(Json(TokenBody { token, user_id: id }));
    }
    let pool = pg().await?;
    let (id, _email, password_hash) = knowledge::find_user_by_email(&pool, &body.email)
        .await
        .map_err(pg_err)?
        .ok_or_else(unauthorized)?;
    if !platform::verify_password(&body.password, &password_hash) {
        return Err(unauthorized());
    }
    let token =
        platform::issue_jwt(id, &state.jwt_secret).map_err(|e| validation(&e.to_string()))?;
    Ok(Json(TokenBody { token, user_id: id }))
}

#[derive(Serialize)]
struct MeBody {
    id: Uuid,
    email: String,
}

async fn ensure_local_user(_state: &AppState, email: &str) -> Result<Uuid, ApiErr> {
    let pool = pg().await?;
    if let Some((id, _, _)) = knowledge::find_user_by_email(&pool, email)
        .await
        .map_err(pg_err)?
    {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    knowledge::insert_user(&pool, id, email, None)
        .await
        .map_err(pg_err)?;
    Ok(id)
}

async fn me(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<MeBody>, ApiErr> {
    let uid = user_from(&headers, &state).await?;
    let pool = pg().await?;
    let (id, email) = knowledge::find_user_by_id(&pool, uid)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("user"))?;
    Ok(Json(MeBody { id, email }))
}

#[derive(Deserialize)]
struct PatchMe {
    email: Option<String>,
}

async fn patch_me(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PatchMe>,
) -> Result<Json<MeBody>, ApiErr> {
    let uid = user_from(&headers, &state).await?;
    let pool = pg().await?;
    let (id, mut email) = knowledge::find_user_by_id(&pool, uid)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("user"))?;
    if let Some(e) = body.email {
        email = e;
        knowledge::update_user_email(&pool, id, &email)
            .await
            .map_err(pg_err)?;
    }
    Ok(Json(MeBody { id, email }))
}

#[derive(Deserialize)]
struct NewWorkspace {
    name: String,
    slug: String,
    #[serde(default)]
    kind: Option<String>,
}

async fn create_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NewWorkspace>,
) -> Result<(StatusCode, Json<WorkspaceView>), ApiErr> {
    let uid = user_from(&headers, &state).await?;
    let kind = match body.kind.as_deref() {
        Some("company") => knowledge::WorkspaceKind::Company,
        _ => knowledge::WorkspaceKind::ProductLine,
    };
    let pool = pg().await?;
    if knowledge::workspace_slug_taken(&pool, &body.slug)
        .await
        .map_err(pg_err)?
    {
        return Err(fail(StatusCode::CONFLICT, "CONFLICT", "slug taken"));
    }
    if kind == knowledge::WorkspaceKind::Company
        && knowledge::company_workspace_id(&pool)
            .await
            .map_err(pg_err)?
            .is_some()
    {
        return Err(fail(
            StatusCode::CONFLICT,
            "CONFLICT",
            "company workspace already exists",
        ));
    }
    let ws = Workspace {
        id: Uuid::new_v4(),
        name: body.name,
        slug: body.slug,
        kind,
        retrieval: Default::default(),
    };
    knowledge::insert_workspace_kind(&pool, ws.id, &ws.name, &ws.slug, ws.kind.as_str())
        .await
        .map_err(pg_err)?;
    knowledge::insert_member(&pool, ws.id, uid, "owner")
        .await
        .map_err(pg_err)?;
    Ok((StatusCode::CREATED, Json(WorkspaceView::from(&ws))))
}

#[derive(Serialize)]
struct WorkspaceView {
    id: Uuid,
    name: String,
    slug: String,
    kind: knowledge::WorkspaceKind,
}

impl WorkspaceView {
    fn from(w: &Workspace) -> Self {
        Self {
            id: w.id,
            name: w.name.clone(),
            slug: w.slug.clone(),
            kind: w.kind,
        }
    }
}

async fn list_workspaces(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<WorkspaceView>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let _ = knowledge::ensure_company_workspace(&pool).await;
    let rows = knowledge::list_workspaces(&pool)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    let out = rows
        .iter()
        .filter(|w| require_ws(w.id, &actor, false, false).is_ok())
        .map(WorkspaceView::from)
        .collect();
    Ok(Json(out))
}

async fn get_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<WorkspaceView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    require_ws(id, &actor, false, false)?;
    let w = knowledge::load_workspace(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("workspace"))?;
    Ok(Json(WorkspaceView::from(&w)))
}

#[derive(Deserialize)]
struct PatchWs {
    name: Option<String>,
}

async fn patch_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchWs>,
) -> Result<Json<WorkspaceView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, true)?;
    let pool = pg().await?;
    let mut w = knowledge::load_workspace(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("workspace"))?;
    if let Some(name) = body.name {
        w.name = name;
        knowledge::update_workspace_name(&pool, id, &w.name)
            .await
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    }
    Ok(Json(WorkspaceView::from(&w)))
}

async fn delete_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, true)?;
    let pool = pg().await?;
    let vids = knowledge::version_ids_for_workspace(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    let _ = knowledge::cancel_active_docs_for_versions(&pool, &vids).await;
    for vid in vids {
        let _ = platform::enqueue_kb_delete(vid).await;
    }
    knowledge::retire_workspace(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Serialize)]
struct MemberView {
    user_id: Uuid,
    role: Role,
}

async fn list_members(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<MemberView>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, false)?;
    let pool = pg().await?;
    let rows = knowledge::list_members_for_workspace(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    let out = rows
        .into_iter()
        .map(|(user_id, role)| MemberView {
            user_id,
            role: knowledge::Role::parse(&role),
        })
        .collect();
    Ok(Json(out))
}

#[derive(Deserialize)]
struct AddMember {
    user_id: Uuid,
    role: Role,
}

async fn add_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<AddMember>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, true)?;
    let pool = pg().await?;
    knowledge::load_workspace(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("workspace"))?;
    knowledge::insert_member(&pool, id, body.user_id, role_name(body.role))
        .await
        .map_err(pg_err)?;
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize)]
struct PatchMember {
    role: Role,
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Admin => "admin",
        Role::Contributor => "contributor",
        Role::Viewer => "viewer",
    }
}

async fn patch_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchMember>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, true)?;
    let pool = pg().await?;
    knowledge::load_workspace(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("workspace"))?;
    knowledge::upsert_member(&pool, id, user_id, role_name(body.role))
        .await
        .map_err(pg_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, true)?;
    let pool = pg().await?;
    knowledge::load_workspace(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("workspace"))?;
    knowledge::delete_member(&pool, id, user_id)
        .await
        .map_err(pg_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_retrieval(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<knowledge::RetrievalConfig>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, false)?;
    let pool = pg().await?;
    let w = knowledge::load_workspace(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("workspace"))?;
    Ok(Json(w.retrieval))
}

async fn patch_retrieval(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<knowledge::RetrievalConfig>,
) -> Result<Json<knowledge::RetrievalConfig>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, true)?;
    let pool = pg().await?;
    knowledge::load_workspace(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("workspace"))?;
    knowledge::set_retrieval_config(
        &pool,
        id,
        body.vector_threshold,
        body.keyword_threshold,
        body.embedding_top_k as i32,
    )
    .await
    .map_err(pg_err)?;
    Ok(Json(body))
}

#[derive(Deserialize)]
struct KindQ {
    kind: Option<String>,
}

#[derive(Deserialize)]
struct NewProduct {
    name: String,
    slug: String,
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Serialize)]
struct ProductView {
    id: Uuid,
    workspace_id: Uuid,
    kind: ProductKind,
    name: String,
    slug: String,
    current_version_id: Option<Uuid>,
}

impl ProductView {
    fn from(p: &Product) -> Self {
        Self {
            id: p.id,
            workspace_id: p.workspace_id,
            kind: p.kind,
            name: p.name.clone(),
            slug: p.slug.clone(),
            current_version_id: p.current_version_id,
        }
    }
}

async fn list_products(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<KindQ>,
) -> Result<Json<Vec<ProductView>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, false)?;
    let pool = pg().await?;
    let rows = knowledge::list_products_in_workspace(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    let out = rows
        .iter()
        .filter(|p| match q.kind.as_deref() {
            Some("library") => p.kind == ProductKind::Library,
            Some("product") => p.kind == ProductKind::Product,
            _ => true,
        })
        .map(ProductView::from)
        .collect();
    Ok(Json(out))
}

async fn create_product(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<NewProduct>,
) -> Result<(StatusCode, Json<ProductView>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, true, false)?;
    let pool = pg().await?;
    let (view, pid, pname, pslug, pkind) = {
        if knowledge::product_slug_taken(&pool, id, &body.slug)
            .await
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        {
            return Err(fail(StatusCode::CONFLICT, "CONFLICT", "slug taken"));
        }
        let ws_kind = knowledge::load_workspace(&pool, id)
            .await
            .ok()
            .flatten()
            .map(|w| w.kind)
            .unwrap_or_default();
        let kind = if ws_kind == knowledge::WorkspaceKind::Company {
            ProductKind::Library
        } else {
            match body.kind.as_deref() {
                Some("library") => ProductKind::Library,
                _ => ProductKind::Product,
            }
        };
        let p = Product {
            id: Uuid::new_v4(),
            workspace_id: id,
            kind,
            name: body.name,
            slug: body.slug,
            current_version_id: None,
            embedding_model_id: "stub-emb".into(),
        };
        let view = ProductView::from(&p);
        let pid = p.id;
        let pname = p.name.clone();
        let pslug = p.slug.clone();
        let pkind = match p.kind {
            ProductKind::Library => "library",
            ProductKind::Product => "product",
        };
        (view, pid, pname, pslug, pkind)
    };
    knowledge::insert_product(&pool, pid, id, pkind, &pname, &pslug, None)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    Ok((StatusCode::CREATED, Json(view)))
}

async fn get_product(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<ProductView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, false)?;
    Ok(Json(ProductView::from(&p)))
}

#[derive(Deserialize)]
struct PatchProduct {
    name: Option<String>,
}

async fn patch_product(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchProduct>,
) -> Result<Json<ProductView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let mut p = knowledge::load_product(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, true)?;
    if let Some(name) = body.name {
        p.name = name;
        knowledge::update_product_name(&pool, id, &p.name)
            .await
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    }
    Ok(Json(ProductView::from(&p)))
}

async fn delete_product(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, true)?;
    if p.kind == ProductKind::Library && p.slug == "library" {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "default library cannot be deleted",
        ));
    }
    let vids = knowledge::version_ids_for_product(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    let _ = knowledge::cancel_active_docs_for_versions(&pool, &vids).await;
    for vid in vids {
        let _ = platform::enqueue_kb_delete(vid).await;
    }
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
struct NewVersion {
    label: String,
    clone_from: Option<Uuid>,
    #[serde(default)]
    diffs: Vec<clone_diff::DiffIn>,
    #[serde(default)]
    make_current: bool,
}

mod clone_diff {
    use serde::Deserialize;
    use uuid::Uuid;
    #[derive(Deserialize)]
    pub struct DiffIn {
        pub op: String,
        pub source_document_id: Option<Uuid>,
    }
}

#[derive(Serialize)]
struct VersionView {
    id: Uuid,
    product_id: Uuid,
    label: String,
    status: VersionStatus,
    current: bool,
    chunk_size: usize,
    chunk_overlap: usize,
    chunk_strategy: String,
    enable_parent_child: bool,
    parent_chunk_size: usize,
    child_chunk_size: usize,
    vector_enabled: bool,
    keyword_enabled: bool,
    wiki_enabled: bool,
    graph_enabled: bool,
    extract_enabled: bool,
    question_enabled: bool,
    enable_multimodel: bool,
    asr_enabled: bool,
    embedding_model_id: String,
    summary_model_id: String,
    wiki_synthesis_model_id: String,
    asr_model_id: String,
    question_count: usize,
    question_custom_instructions: String,
    table_metadata_instructions: String,
}

fn version_view_for(current_version_id: Option<Uuid>, v: &ProductVersion) -> VersionView {
    let current = current_version_id == Some(v.id);
    VersionView {
        id: v.id,
        product_id: v.product_id,
        label: v.label.clone(),
        status: v.status,
        current,
        chunk_size: v.chunk_size,
        chunk_overlap: v.chunk_overlap,
        chunk_strategy: v.chunk_strategy.clone(),
        enable_parent_child: v.enable_parent_child,
        parent_chunk_size: v.parent_chunk_size(),
        child_chunk_size: v.child_chunk_size(),
        vector_enabled: v.vector_enabled,
        keyword_enabled: v.keyword_enabled,
        wiki_enabled: v.wiki_enabled,
        graph_enabled: v.graph_enabled,
        extract_enabled: v.extract_enabled,
        question_enabled: v.question_enabled,
        enable_multimodel: v.enable_multimodel,
        asr_enabled: v.asr_enabled,
        embedding_model_id: v.embedding_model_id.clone(),
        summary_model_id: v.summary_model_id.clone(),
        wiki_synthesis_model_id: v.wiki_synthesis_model_id.clone(),
        asr_model_id: v.asr_model_id.clone(),
        question_count: v.question_count(),
        question_custom_instructions: v.question_custom_instructions.clone(),
        table_metadata_instructions: v.table_metadata_instructions.clone(),
    }
}

async fn list_versions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<VersionView>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, false)?;
    let versions = knowledge::list_versions_for_product(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    let out = versions
        .iter()
        .map(|v| version_view_for(p.current_version_id, v))
        .collect();
    Ok(Json(out))
}

async fn create_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<NewVersion>,
) -> Result<(StatusCode, Json<VersionView>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, true, false)?;
    if knowledge::version_label_taken(&pool, id, &body.label)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
    {
        return Err(fail(StatusCode::CONFLICT, "CONFLICT", "label taken"));
    }
    let clone_from = body.clone_from;
    let make_current = body.make_current;
    let diffs: Vec<serde_json::Value> = body
        .diffs
        .iter()
        .map(|d| json!({"op": d.op, "source_document_id": d.source_document_id}))
        .collect();
    let mut v = ProductVersion::new(id, body.label);
    v.cloned_from = clone_from;
    if clone_from.is_some() {
        v.status = VersionStatus::Cloning;
    }
    let view_id = v.id;
    let label = v.label.clone();
    if let Some(src) = clone_from {
        knowledge::insert_version_cloning(&pool, view_id, id, &label, src)
            .await
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    } else {
        knowledge::insert_version(&pool, view_id, id, &label, "active", None)
            .await
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    }
    let mut current = p.current_version_id;
    if current.is_none() {
        knowledge::set_product_current(&pool, id, view_id)
            .await
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
        current = Some(view_id);
    }
    if let Some(src) = clone_from {
        let _ = platform::enqueue_version_clone(src, view_id, json!(diffs), make_current).await;
    }
    if let Some(loaded) = knowledge::load_version(&pool, view_id).await.ok().flatten() {
        v = loaded;
    }
    Ok((StatusCode::CREATED, Json(version_view_for(current, &v))))
}

async fn get_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
) -> Result<Json<VersionView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, false)?;
    let vid = knowledge::resolve_product_version_id(&pool, id, &version_id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| {
            if version_id == "current" {
                validation("no current version")
            } else {
                not_found("version")
            }
        })?;
    let v = knowledge::load_version(&pool, vid)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("version"))?;
    Ok(Json(version_view_for(p.current_version_id, &v)))
}

#[derive(Deserialize, Default)]
struct PatchVersion {
    status: Option<VersionStatus>,
    chunk_size: Option<usize>,
    chunk_overlap: Option<usize>,
    chunk_strategy: Option<String>,
    enable_parent_child: Option<bool>,
    parent_chunk_size: Option<usize>,
    child_chunk_size: Option<usize>,
    separators: Option<Vec<String>>,
    token_limit: Option<usize>,
    languages: Option<Vec<String>>,
    vector_enabled: Option<bool>,
    keyword_enabled: Option<bool>,
    wiki_enabled: Option<bool>,
    graph_enabled: Option<bool>,
    extract_enabled: Option<bool>,
    question_enabled: Option<bool>,
    enable_multimodel: Option<bool>,
    asr_enabled: Option<bool>,
    embedding_model_id: Option<String>,
    summary_model_id: Option<String>,
    wiki_synthesis_model_id: Option<String>,
    asr_model_id: Option<String>,
    question_count: Option<usize>,
    question_custom_instructions: Option<String>,
    table_metadata_instructions: Option<String>,
}

fn version_status_str(st: VersionStatus) -> &'static str {
    match st {
        VersionStatus::Cloning => "cloning",
        VersionStatus::Active => "active",
        VersionStatus::Archived => "archived",
        VersionStatus::Failed => "failed",
    }
}

fn apply_patch_version(v: &mut ProductVersion, body: &PatchVersion) {
    if let Some(st) = body.status {
        v.status = st;
    }
    if let Some(n) = body.chunk_size.filter(|n| *n > 0) {
        v.chunk_size = n;
    }
    if let Some(n) = body.chunk_overlap.filter(|n| *n > 0) {
        v.chunk_overlap = n;
    }
    if let Some(st) = body.chunk_strategy.clone().filter(|s| !s.is_empty()) {
        v.chunk_strategy = st;
    }
    if let Some(b) = body.enable_parent_child {
        v.enable_parent_child = b;
    }
    if let Some(n) = body.parent_chunk_size.filter(|n| *n > 0) {
        v.parent_chunk_size = n;
    }
    if let Some(n) = body.child_chunk_size.filter(|n| *n > 0) {
        v.child_chunk_size = n;
    }
    if let Some(s) = body.separators.clone().filter(|s| !s.is_empty()) {
        v.chunk_separators = s;
    }
    if let Some(n) = body.token_limit.filter(|n| *n > 0) {
        v.chunk_token_limit = n;
    }
    if let Some(l) = body.languages.clone().filter(|l| !l.is_empty()) {
        v.chunk_languages = l;
    }
    if let Some(b) = body.vector_enabled {
        v.vector_enabled = b;
    }
    if let Some(b) = body.keyword_enabled {
        v.keyword_enabled = b;
    }
    if let Some(b) = body.wiki_enabled {
        v.wiki_enabled = b;
    }
    if let Some(b) = body.graph_enabled {
        v.graph_enabled = b;
    }
    if let Some(b) = body.extract_enabled {
        v.extract_enabled = b;
    }
    if let Some(b) = body.question_enabled {
        v.question_enabled = b;
    }
    if let Some(n) = body.question_count {
        v.question_count = if n == 0 { 3 } else { n.min(10) };
    }
    if let Some(s) = body.question_custom_instructions.clone() {
        v.question_custom_instructions = s;
    }
    if let Some(s) = body.table_metadata_instructions.clone() {
        v.table_metadata_instructions = s;
    }
    if let Some(b) = body.enable_multimodel {
        v.enable_multimodel = b;
    }
    if let Some(b) = body.asr_enabled {
        v.asr_enabled = b;
    }
    if let Some(m) = body.embedding_model_id.clone().filter(|s| !s.is_empty()) {
        v.embedding_model_id = m;
    }
    if let Some(m) = body.summary_model_id.clone().filter(|s| !s.is_empty()) {
        v.summary_model_id = m;
    }
    if let Some(m) = body.wiki_synthesis_model_id.clone() {
        v.wiki_synthesis_model_id = m;
    }
    if let Some(m) = body.asr_model_id.clone() {
        v.asr_model_id = m;
    }
}

fn version_config_of(v: &ProductVersion) -> knowledge::VersionConfig {
    knowledge::VersionConfig {
        status: Some(version_status_str(v.status).into()),
        chunking: Some(json!({
            "chunk_size": v.chunk_size,
            "chunk_overlap": v.chunk_overlap,
            "strategy": v.chunk_strategy,
            "enable_parent_child": v.enable_parent_child,
            "parent_chunk_size": v.parent_chunk_size(),
            "child_chunk_size": v.child_chunk_size(),
            "separators": v.chunk_separators,
            "token_limit": v.chunk_token_limit,
            "languages": v.chunk_languages,
            "parser_engine_rules": v.parser_engine_rules,
            "table_metadata_instructions": v.table_metadata_instructions,
        })),
        indexing: Some(json!({
            "vector": v.vector_enabled,
            "keyword": v.keyword_enabled,
            "wiki": v.wiki_enabled,
            "graph": v.graph_enabled,
        })),
        image_processing: Some(json!({"enable_multimodel": v.enable_multimodel})),
        embedding_model_id: Some(v.embedding_model_id.clone()),
        summary_model_id: Some(v.summary_model_id.clone()),
        asr_model_id: Some(v.asr_model_id.clone()),
        asr_config: Some(json!({"enabled": v.asr_enabled})),
        extract_config: Some(json!({"enabled": v.extract_enabled})),
        wiki_config: Some(json!({
            "synthesis_model_id": v.wiki_synthesis_model_id,
        })),
        question_generation_config: Some(json!({
            "enabled": v.question_enabled,
            "question_count": v.question_count(),
            "custom_instructions": v.question_custom_instructions,
        })),
    }
}

async fn patch_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
    Json(body): Json<PatchVersion>,
) -> Result<Json<VersionView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, true)?;
    let vid = resolve_vid(&pool, id, &version_id).await?;
    let mut v = knowledge::load_version(&pool, vid)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("version"))?;
    apply_patch_version(&mut v, &body);
    knowledge::update_version_config(&pool, vid, version_config_of(&v))
        .await
        .map_err(pg_err)?;
    Ok(Json(version_view_for(p.current_version_id, &v)))
}

async fn delete_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, true)?;
    let vid = knowledge::resolve_product_version_id(&pool, id, &version_id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("version"))?;
    let _ = knowledge::cancel_active_docs_for_versions(&pool, &[vid]).await;
    let _ = knowledge::set_version_status(&pool, vid, "archived").await;
    let _ = knowledge::clear_product_current_if(&pool, id, vid).await;
    let _ = platform::enqueue_kb_delete(vid).await;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
struct SetCurrent {
    version_id: Uuid,
}

async fn set_current(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<SetCurrent>,
) -> Result<Json<ProductView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let mut p = knowledge::load_product(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, true)?;
    let v = knowledge::load_version(&pool, body.version_id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("version"))?;
    if v.product_id != id {
        return Err(validation("version not on product"));
    }
    if v.status != VersionStatus::Active {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "VERSION_NOT_ACTIVE",
            "version is not active",
        ));
    }
    if let Some(err) =
        knowledge::workspace_embedding_conflict(&pool, p.workspace_id, &v.embedding_model_id)
            .await
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
    {
        return Err(fail(StatusCode::BAD_REQUEST, "EMBEDDING_MISMATCH", err));
    }
    knowledge::set_product_current(&pool, id, body.version_id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    p.current_version_id = Some(body.version_id);
    p.embedding_model_id = v.embedding_model_id;
    Ok(Json(ProductView::from(&p)))
}

#[derive(Serialize)]
struct DocView {
    id: Uuid,
    product_version_id: Uuid,
    title: String,
    file_name: String,
    object_ref: String,
    parse_status: ParseStatus,
    enable_status: String,
    index_ready: bool,
    pending_subtasks_count: i32,
    error_message: String,
    description: String,
}

impl DocView {
    fn from(d: &Document) -> Self {
        Self {
            id: d.id,
            product_version_id: d.product_version_id,
            title: d.title.clone(),
            file_name: d.file_name.clone(),
            object_ref: d.object_ref.clone(),
            parse_status: d.parse_status,
            enable_status: d.enable_status.clone(),
            index_ready: d.index_ready,
            pending_subtasks_count: d.pending_subtasks_count,
            error_message: d.error_message.clone(),
            description: d.description.clone(),
        }
    }
}

fn write_active_status(status: VersionStatus) -> Result<(), ApiErr> {
    if status != VersionStatus::Active {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "VERSION_NOT_ACTIVE",
            "version is not active",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
struct DocListQ {
    parse_status: Option<String>,
    tag: Option<Uuid>,
    keyword: Option<String>,
}

async fn list_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
    Query(q): Query<DocListQ>,
) -> Result<Json<Vec<DocView>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, false)?;
    let vid = if version_id == "current" {
        p.current_version_id.ok_or_else(|| not_found("version"))?
    } else {
        Uuid::parse_str(&version_id).map_err(|_| validation("version_id"))?
    };
    let keyword = q
        .keyword
        .as_deref()
        .map(|k| k.trim())
        .filter(|k| !k.is_empty());
    let rows =
        knowledge::list_documents_in_version(&pool, vid, q.parse_status.as_deref(), keyword, q.tag)
            .await
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    let out = rows.iter().map(DocView::from).collect();
    Ok(Json(out))
}

struct PreparedIngest<'a> {
    title: String,
    file_name: String,
    bytes: &'a [u8],
    tag_ids: &'a [Uuid],
    overrides: Option<knowledge::ProcessOverrides>,
    doc_type: &'a str,
    passages: Vec<String>,
}

async fn ingest_prepared(
    actor: &Actor,
    product_id: Uuid,
    version_id: &str,
    input: PreparedIngest<'_>,
) -> Result<Document, ApiErr> {
    let PreparedIngest {
        title,
        file_name,
        bytes,
        tag_ids,
        overrides,
        doc_type,
        passages,
    } = input;
    if is_video(&file_name) {
        return Err(validation("video is not allowed"));
    }
    if !is_valid_file_type(&file_name) {
        return Err(validation("file type not allowed"));
    }
    if bytes.len() > platform::max_file_bytes() {
        return Err(validation("file too large"));
    }
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, product_id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, actor, true, false)?;
    let vid = resolve_vid(&pool, product_id, version_id).await?;
    let version = knowledge::load_version(&pool, vid)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("version"))?;
    write_active_status(version.status)?;
    if knowledge::is_frozen_default_library(&pool, product_id)
        .await
        .map_err(pg_err)?
    {
        return Err(fail(
            StatusCode::CONFLICT,
            "CONFLICT",
            "product-line default library is frozen; upload to company workspace",
        ));
    }
    let eff = knowledge::resolve_process_config(&version, overrides.as_ref());
    if is_image_type(&file_name) && (!eff.enable_multimodel || !platform::vlm_configured()) {
        return Err(validation("image requires VLM configuration"));
    }
    if is_audio_type(&file_name) && !eff.asr_enabled {
        return Err(validation("audio requires ASR configuration"));
    }
    let hash = platform::sha256_hex(bytes);
    if let Some(existing) =
        knowledge::find_duplicate_document(&pool, vid, &file_name, bytes.len() as i64, &hash)
            .await
            .map_err(pg_err)?
    {
        return Err(fail(
            StatusCode::CONFLICT,
            "CONFLICT",
            format!("duplicate file {existing}"),
        ));
    }
    if !knowledge::tags_belong_to_workspace(&pool, p.workspace_id, tag_ids)
        .await
        .map_err(pg_err)?
    {
        return Err(validation("unknown tag"));
    }
    let (hash, key) = knowledge::put_bytes(bytes);
    let mut doc = Document::new(vid, title, file_name.clone(), bytes.len() as i64, hash, key);
    doc.process_overrides = overrides.filter(|o| !o.is_empty());
    if !doc_type.is_empty() {
        doc.doc_type = doc_type.to_string();
    }
    if !passages.is_empty() {
        doc.source_passages = passages.clone();
    }
    persist_ingest_row(&doc, tag_ids).await?;
    let enqueue_err = if doc_type == "manual" {
        platform::enqueue_manual_process(doc.id, doc.product_version_id, doc.attempt)
            .await
            .err()
    } else if doc_type == "passage" {
        platform::enqueue_document_process_with(
            doc.id,
            doc.product_version_id,
            doc.attempt,
            passages,
        )
        .await
        .err()
    } else {
        if file_name.ends_with(".csv")
            || file_name.ends_with(".xlsx")
            || file_name.ends_with(".xls")
        {
            let _ = platform::enqueue_datatable(doc.id).await;
        }
        platform::enqueue_document_process(doc.id, doc.product_version_id, doc.attempt)
            .await
            .err()
    };
    if let Some(e) = enqueue_err {
        doc.parse_status = ParseStatus::Failed;
        doc.error_message = e;
        persist_failed_row(&doc).await;
    }
    Ok(doc)
}

async fn persist_ingest_row(doc: &Document, tag_ids: &[Uuid]) -> Result<(), ApiErr> {
    let Ok(pool) = platform::connect().await else {
        return Ok(());
    };
    let version_in_pg = knowledge::version_exists(&pool, doc.product_version_id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e.to_string()))?;
    if !version_in_pg {
        return Ok(());
    }
    if let Ok(Some(existing)) = knowledge::find_duplicate_document(
        &pool,
        doc.product_version_id,
        &doc.file_name,
        doc.file_size,
        &doc.file_hash,
    )
    .await
        && existing != doc.id
    {
        return Err(fail(
            StatusCode::CONFLICT,
            "CONFLICT",
            format!("duplicate file {existing}"),
        ));
    }
    if let Err(e) = knowledge::insert_document(
        &pool,
        knowledge::NewDocument {
            id: doc.id,
            product_version_id: doc.product_version_id,
            title: &doc.title,
            file_name: &doc.file_name,
            file_size: doc.file_size,
            file_hash: &doc.file_hash,
            object_ref: &doc.object_ref,
        },
    )
    .await
    {
        if knowledge::is_unique_violation(&e) {
            let existing = knowledge::find_duplicate_document(
                &pool,
                doc.product_version_id,
                &doc.file_name,
                doc.file_size,
                &doc.file_hash,
            )
            .await
            .ok()
            .flatten()
            .unwrap_or(doc.id);
            return Err(fail(
                StatusCode::CONFLICT,
                "CONFLICT",
                format!("duplicate file {existing}"),
            ));
        }
        return Err(fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            e.to_string(),
        ));
    }
    let _ =
        knowledge::set_document_source(&pool, doc.id, &doc.doc_type, &doc.source_passages).await;
    let _ = knowledge::insert_document_tags(&pool, doc.id, tag_ids).await;
    if let Some(o) = &doc.process_overrides {
        let _ = knowledge::set_process_overrides(&pool, doc.id, o).await;
    }
    let _ = knowledge::open_attempt(&pool, doc.id, doc.attempt).await;
    if doc.parse_status == ParseStatus::Failed {
        let _ = knowledge::set_parse_status(&pool, doc.id, "failed", &doc.error_message).await;
    }
    Ok(())
}

async fn persist_failed_row(doc: &Document) {
    if doc.parse_status != ParseStatus::Failed {
        return;
    }
    let Ok(pool) = platform::connect().await else {
        return;
    };
    if !knowledge::version_exists(&pool, doc.product_version_id)
        .await
        .unwrap_or(false)
    {
        return;
    }
    let _ = knowledge::set_parse_status(&pool, doc.id, "failed", &doc.error_message).await;
}

async fn ingest_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<DocView>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let mut file_name = String::from("upload.txt");
    let mut bytes = Vec::new();
    let mut tag_ids = Vec::new();
    let mut overrides = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| validation(&e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" || name == "content" {
            if let Some(fnm) = field.file_name().map(|x| x.to_string()) {
                file_name = fnm;
            }
            bytes = field
                .bytes()
                .await
                .map_err(|e| validation(&e.to_string()))?
                .to_vec();
        } else if name == "file_name" {
            file_name = field.text().await.map_err(|e| validation(&e.to_string()))?;
        } else if name == "tag_ids" {
            let t = field.text().await.map_err(|e| validation(&e.to_string()))?;
            if let Ok(v) = serde_json::from_str::<Vec<Uuid>>(&t) {
                tag_ids = v;
            }
        } else if name == "process_config" {
            let t = field.text().await.map_err(|e| validation(&e.to_string()))?;
            overrides = serde_json::from_str::<knowledge::ProcessOverrides>(&t).ok();
        }
    }
    if bytes.is_empty() {
        return Err(validation("empty file"));
    }
    let title = file_name.clone();
    let doc = ingest_prepared(
        &actor,
        id,
        &version_id,
        PreparedIngest {
            title,
            file_name,
            bytes: &bytes,
            tag_ids: &tag_ids,
            overrides,
            doc_type: "file",
            passages: Vec::new(),
        },
    )
    .await?;
    Ok((ingest_status(&doc), Json(DocView::from(&doc))))
}

#[derive(Deserialize)]
struct UrlIn {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    process_config: Option<knowledge::ProcessOverrides>,
}

async fn ingest_url(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
    Json(body): Json<UrlIn>,
) -> Result<(StatusCode, Json<DocView>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    if platform::url_blocked(&body.url) {
        return Err(validation("url failed SSRF check"));
    }
    let name = body.title.unwrap_or_else(|| "remote.md".into());
    let bytes = format!("url:{}", body.url).into_bytes();
    let doc = ingest_prepared(
        &actor,
        id,
        &version_id,
        PreparedIngest {
            title: name.clone(),
            file_name: name,
            bytes: &bytes,
            tag_ids: &[],
            overrides: body.process_config,
            doc_type: "url",
            passages: Vec::new(),
        },
    )
    .await?;
    Ok((ingest_status(&doc), Json(DocView::from(&doc))))
}

fn ingest_status(doc: &Document) -> StatusCode {
    if doc.parse_status == ParseStatus::Failed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    }
}

#[derive(Deserialize)]
struct PassageIn {
    title: String,
    passages: Vec<String>,
    #[serde(default)]
    tag_ids: Vec<Uuid>,
    #[serde(default)]
    process_config: Option<knowledge::ProcessOverrides>,
}

async fn ingest_passage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
    Json(body): Json<PassageIn>,
) -> Result<(StatusCode, Json<DocView>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let joined = body.passages.join("\n");
    let file_name = format!("{}.txt", body.title);
    let doc = ingest_prepared(
        &actor,
        id,
        &version_id,
        PreparedIngest {
            title: body.title,
            file_name,
            bytes: joined.as_bytes(),
            tag_ids: &body.tag_ids,
            overrides: body.process_config,
            doc_type: "passage",
            passages: body.passages,
        },
    )
    .await?;
    Ok((ingest_status(&doc), Json(DocView::from(&doc))))
}

#[derive(Deserialize)]
struct ManualIn {
    title: String,
    content: String,
    #[serde(default)]
    tag_ids: Vec<Uuid>,
    #[serde(default)]
    process_config: Option<knowledge::ProcessOverrides>,
}

async fn ingest_manual(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
    Json(body): Json<ManualIn>,
) -> Result<(StatusCode, Json<DocView>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    if body.content.trim().is_empty() {
        return Err(validation("content required"));
    }
    let file_name = if body.title.to_ascii_lowercase().ends_with(".md") {
        body.title.clone()
    } else {
        format!("{}.md", body.title)
    };
    let doc = ingest_prepared(
        &actor,
        id,
        &version_id,
        PreparedIngest {
            title: body.title,
            file_name,
            bytes: body.content.as_bytes(),
            tag_ids: &body.tag_ids,
            overrides: body.process_config,
            doc_type: "manual",
            passages: Vec::new(),
        },
    )
    .await?;
    Ok((ingest_status(&doc), Json(DocView::from(&doc))))
}

async fn get_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<DocView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let d = knowledge::load_document(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("document"))?;
    let ws = knowledge::document_workspace_id(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("document"))?;
    require_ws(ws, &actor, false, false)?;
    Ok(Json(DocView::from(&d)))
}

async fn document_content(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let meta = knowledge::load_document(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("document"))?;
    let ws = knowledge::document_workspace_id(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("document"))?;
    require_ws(ws, &actor, false, false)?;
    let mut chunks = knowledge::load_document_chunks(&pool, id)
        .await
        .map_err(pg_err)?;
    chunks.sort_by_key(|c| (c.start_at, c.id));
    let mut markdown = meta.markdown.clone();
    if markdown.is_empty()
        && !meta.file_hash.is_empty()
        && let Ok(bytes) = platform::read_blob(&format!("{}.md", meta.file_hash))
    {
        markdown = String::from_utf8_lossy(&bytes).into_owned();
    }
    Ok(Json(json!({
        "id": meta.id,
        "title": meta.title,
        "file_name": meta.file_name,
        "object_ref": meta.object_ref,
        "file_hash": meta.file_hash,
        "parse_status": meta.parse_status,
        "index_ready": meta.index_ready,
        "error_message": meta.error_message,
        "description": meta.description,
        "markdown": markdown,
        "chunks": chunks.iter().map(|c| json!({
            "id": c.id,
            "chunk_type": c.chunk_type,
            "content": c.content,
            "context_header": c.context_header,
            "start_at": c.start_at,
            "end_at": c.end_at,
            "generated_questions": c.generated_questions,
        })).collect::<Vec<_>>(),
    })))
}

async fn delete_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let d = knowledge::load_document(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("document"))?;
    let ws = knowledge::document_workspace_id(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("document"))?;
    require_ws(ws, &actor, true, false)?;
    let _ = d;
    knowledge::set_parse_status(&pool, id, "deleting", "")
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    platform::enqueue_list_delete(id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e))?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize, Default)]
struct ReparseIn {
    #[serde(default)]
    process_config: Option<knowledge::ProcessOverrides>,
}

async fn reparse_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    body: Option<Json<ReparseIn>>,
) -> Result<Json<DocView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let mut doc = knowledge::load_document(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("document"))?;
    let ws = knowledge::document_workspace_id(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("document"))?;
    require_ws(ws, &actor, true, false)?;
    let v = knowledge::load_version(&pool, doc.product_version_id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("version"))?;
    if v.status != VersionStatus::Active {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "VERSION_NOT_ACTIVE",
            "version is not active",
        ));
    }
    if let Some(o) = body.as_ref().and_then(|b| b.process_config.clone()) {
        doc.process_overrides = Some(o).filter(|x| !x.is_empty());
        if let Some(o) = &doc.process_overrides {
            knowledge::set_process_overrides(&pool, id, o)
                .await
                .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
        }
    }
    knowledge::mark_reparse_queued(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    doc.parse_status = ParseStatus::Pending;
    doc.enable_status = "disabled".into();
    platform::enqueue_list_reparse(id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e))?;
    Ok(Json(DocView::from(&doc)))
}

async fn cancel_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<DocView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let mut doc = knowledge::load_document(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("document"))?;
    let ws = knowledge::document_workspace_id(&pool, id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?
        .ok_or_else(|| not_found("document"))?;
    require_ws(ws, &actor, true, false)?;
    knowledge::set_parse_status(&pool, id, "cancelled", "")
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM", e.to_string()))?;
    doc.parse_status = ParseStatus::Cancelled;
    Ok(Json(DocView::from(&doc)))
}

#[derive(Deserialize)]
struct TimelineQuery {
    attempt: Option<i32>,
}

async fn timeline(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<TimelineQuery>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let d = knowledge::load_document(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("document"))?;
    let ws = knowledge::document_workspace_id(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("document"))?;
    require_ws(ws, &actor, false, false)?;
    let parse_status = d.parse_status.as_str().to_string();
    let err_msg = d.error_message.clone();
    let mut latest = knowledge::latest_span_attempt(&pool, id)
        .await
        .map_err(pg_err)?;
    if latest <= 0 {
        latest = d.attempt;
    }
    let attempt = q.attempt.filter(|n| *n > 0).unwrap_or(latest);
    let rows: Vec<_> = knowledge::list_spans_attempt(&pool, id, attempt)
        .await
        .map_err(pg_err)?
        .into_iter()
        .map(|r| r.into_span())
        .collect();
    let (trace, current_stage, last_fail) =
        knowledge::obs::build_trace(attempt, &parse_status, &rows);
    let last_error = last_fail
        .map(|f| {
            json!({
                "stage": f.name,
                "code": if f.error_message.is_empty() { "FAILED" } else { "STAGE_FAILED" },
                "message": f.error_message,
            })
        })
        .or_else(|| {
            if parse_status == "failed" && !err_msg.is_empty() {
                Some(json!({
                    "stage": knowledge::obs::ROOT_NAME,
                    "code": "PARSE_FAILED",
                    "message": err_msg,
                }))
            } else {
                None
            }
        });
    Ok(Json(json!({
        "document_id": id,
        "attempt": attempt,
        "latest_attempt": latest,
        "parse_status": parse_status,
        "current_stage": current_stage,
        "trace": trace,
        "last_error": last_error,
    })))
}

#[derive(Deserialize)]
struct NewTag {
    name: String,
    slug: String,
}

async fn list_tags(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Tag>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, false)?;
    let pool = pg().await?;
    knowledge::load_workspace(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("workspace"))?;
    let tags = knowledge::list_tags_for_workspace(&pool, id)
        .await
        .map_err(pg_err)?;
    Ok(Json(tags))
}

async fn create_tag(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<NewTag>,
) -> Result<(StatusCode, Json<Tag>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, true, false)?;
    let pool = pg().await?;
    knowledge::load_workspace(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("workspace"))?;
    let tag = Tag {
        id: Uuid::new_v4(),
        workspace_id: id,
        name: body.name,
        slug: body.slug,
    };
    knowledge::insert_tag(&pool, tag.id, tag.workspace_id, &tag.name, &tag.slug)
        .await
        .map_err(pg_err)?;
    Ok((StatusCode::CREATED, Json(tag)))
}

async fn delete_tag(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, tag_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, true)?;
    let pool = pg().await?;
    let tag = knowledge::load_tag(&pool, tag_id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("tag"))?;
    if tag.workspace_id != id {
        return Err(not_found("tag"));
    }
    knowledge::delete_tag(&pool, tag_id).await.map_err(pg_err)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PutTags {
    tag_ids: Vec<Uuid>,
}

async fn put_tags(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<PutTags>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    knowledge::load_document(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("document"))?;
    let ws = knowledge::document_workspace_id(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("document"))?;
    require_ws(ws, &actor, true, false)?;
    let kept = knowledge::tags_in_workspace(&pool, ws, &body.tag_ids)
        .await
        .map_err(pg_err)?;
    knowledge::replace_document_tags(&pool, id, &kept)
        .await
        .map_err(pg_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn wiki_pages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, vid)): Path<(Uuid, String)>,
) -> Result<Json<Vec<knowledge::WikiPage>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, false)?;
    let version = resolve_vid(&pool, id, &vid).await?;
    let pages = knowledge::list_wiki_pages(&pool, version)
        .await
        .map_err(pg_err)?;
    Ok(Json(pages))
}

async fn wiki_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, vid, slug)): Path<(Uuid, String, String)>,
) -> Result<Json<knowledge::WikiPage>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, false)?;
    let version = resolve_vid(&pool, id, &vid).await?;
    knowledge::load_wiki_page(&pool, version, &slug)
        .await
        .map_err(pg_err)?
        .map(Json)
        .ok_or_else(|| not_found("page"))
}

async fn wiki_folders(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, vid)): Path<(Uuid, String)>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, false)?;
    let version = resolve_vid(&pool, id, &vid).await?;
    let folders = knowledge::list_wiki_folders(&pool, version)
        .await
        .map_err(pg_err)?
        .into_iter()
        .map(|(fid, name, path, depth)| {
            json!({"id": fid, "name": name, "path": path, "depth": depth})
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "folders": folders })))
}

#[derive(Deserialize)]
struct FileQuery {
    key: String,
}

async fn version_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, vid)): Path<(Uuid, String)>,
    Query(q): Query<FileQuery>,
) -> Result<Vec<u8>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let hash = q.key.trim_start_matches("objects/");
    if hash.is_empty() {
        return Err(not_found("file"));
    }
    let key = if q.key.starts_with("objects/") {
        q.key.clone()
    } else {
        format!("objects/{hash}")
    };
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, false)?;
    let version = resolve_vid(&pool, id, &vid).await?;
    let allowed = knowledge::version_references_object(&pool, version, &key, hash)
        .await
        .map_err(pg_err)?;
    if !allowed {
        return Err(not_found("file"));
    }
    platform::read_blob(hash).map_err(|_| not_found("file"))
}

#[derive(Deserialize, Serialize)]
struct NewModel {
    id: String,
    kind: String,
    #[serde(default)]
    dimension: i32,
}

async fn list_parser_engines(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<docparser::EngineCatalog>, ApiErr> {
    let _actor = actor_from(&headers, &state).await?;
    Ok(Json(
        docparser::list_all_engines(&std::collections::HashMap::new()).await,
    ))
}

async fn list_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor).await?;
    let mut out = vec![
        json!({"id": "stub-emb", "kind": "embedding", "dimension": knowledge::models::EMBEDDING_DIM}),
        json!({"id": "stub-chat", "kind": "chat", "dimension": 0}),
    ];
    if let Ok(pool) = platform::connect().await
        && let Ok(rows) = knowledge::list_models(&pool).await
    {
        for (id, kind, dim) in rows {
            if !out.iter().any(|m| m["id"] == id) {
                out.push(json!({"id": id, "kind": kind, "dimension": dim}));
            }
        }
    }
    Ok(Json(out))
}

async fn create_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NewModel>,
) -> Result<(StatusCode, Json<NewModel>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    {
        require_admin(&state, &actor).await?;
    }
    let pool = platform::connect().await.ok();
    if let Some(pool) = pool {
        let _ = knowledge::upsert_model(&pool, &body.id, &body.kind, body.dimension).await;
    }
    Ok((StatusCode::CREATED, Json(body)))
}

async fn patch_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<NewModel>,
) -> Result<Json<NewModel>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    {
        require_admin(&state, &actor).await?;
    }
    let mut body = body;
    body.id = id;
    let pool = platform::connect().await.ok();
    if let Some(pool) = pool {
        let _ = knowledge::upsert_model(&pool, &body.id, &body.kind, body.dimension).await;
    }
    Ok(Json(body))
}

pub(crate) async fn require_admin(_state: &AppState, actor: &Actor) -> Result<(), ApiErr> {
    let admin = match actor {
        Actor::Bootstrap => true,
        Actor::Key(k) => k.scopes.iter().any(|x| x == "admin"),
        Actor::User(uid) => {
            if let Ok(pool) = platform::connect().await {
                sqlx::query_scalar(
                    "SELECT EXISTS(
                        SELECT 1 FROM workspace_members
                        WHERE user_id=$1 AND role IN ('owner','admin')
                     )",
                )
                .bind(uid)
                .fetch_one(&pool)
                .await
                .unwrap_or(false)
            } else {
                false
            }
        }
    };
    if !admin {
        return Err(forbidden());
    }
    Ok(())
}

async fn ops_oxana(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor).await?;
    let queues = platform::queue_depths().await;
    let jobs = platform::queue_job_previews().await;
    Ok(Json(json!({
        "queues": queues,
        "jobs": jobs,
        "dashboard": "/api/v1/ops/oxana/web"
    })))
}

async fn metrics() -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        "# HELP knowledgebrain_up 1 if the api is up\n# TYPE knowledgebrain_up gauge\nknowledgebrain_up 1\n"
            .into(),
    )
}

async fn do_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut req): Json<knowledge::search::SearchRequest>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let _actor = actor_from(&headers, &state).await?;
    if req.scope.is_some() {
        req.expand_wiki = false;
        req.expand_graph = false;
        req.include_library = false;
    }
    if !matches!(req.mode.as_str(), "assembly" | "matching") {
        return Err(validation("mode must be assembly or matching"));
    }
    let pool = pg().await?;
    if req.mode == "matching" {
        let out = knowledge::search::matching_pg(&pool, &req)
            .await
            .map_err(map_search)?;
        let v = serde_json::to_value(&out).unwrap();
        assert!(v.get("best_product_id").is_none());
        return Ok(Json(v));
    }
    let out = knowledge::search::assembly_pg(&pool, &req)
        .await
        .map_err(map_search)?;
    Ok(Json(serde_json::to_value(&out).unwrap()))
}

async fn do_match(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut req): Json<knowledge::search::SearchRequest>,
) -> Result<Json<knowledge::search::MatchingResponse>, ApiErr> {
    let _actor = actor_from(&headers, &state).await?;
    req.mode = "matching".into();
    if req.scope.is_some() {
        req.expand_wiki = false;
        req.expand_graph = false;
        req.include_library = false;
    }
    let pool = pg().await?;
    let out = knowledge::search::matching_pg(&pool, &req)
        .await
        .map_err(map_search)?;
    debug_assert!(
        serde_json::to_value(&out)
            .unwrap()
            .get("best_product_id")
            .is_none()
    );
    Ok(Json(out))
}

fn map_search(e: knowledge::search::SearchError) -> ApiErr {
    let status = match e.code {
        "NOT_FOUND" => StatusCode::NOT_FOUND,
        "EMBEDDING_MISMATCH" => StatusCode::BAD_REQUEST,
        "TOO_MANY_TARGETS" => StatusCode::BAD_REQUEST,
        "UPSTREAM" => StatusCode::BAD_GATEWAY,
        "INTERNAL" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    };
    fail(status, e.code, e.message)
}

#[derive(Deserialize)]
struct AnswerIn {
    query: String,
    product_id: Uuid,
    version_id: Option<String>,
    #[serde(default)]
    include_library: bool,
    #[serde(default)]
    tag_ids: Vec<Uuid>,
    #[serde(default)]
    context: Vec<String>,
}

#[derive(Serialize)]
struct AnswerOut {
    answer: String,
    hits: Vec<knowledge::search::Hit>,
    citations: Vec<HashMap<String, serde_json::Value>>,
}

async fn do_answer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AnswerIn>,
) -> Result<Json<AnswerOut>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = pg().await?;
    let p = knowledge::load_product(&pool, body.product_id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("product"))?;
    require_ws(p.workspace_id, &actor, false, false)?;
    if p.current_version_id.is_none() {
        return Err(validation("product has no current version"));
    }
    if body.query.trim().is_empty() {
        return Err(validation("query required"));
    }
    let sreq = knowledge::search::SearchRequest {
        mode: "assembly".into(),
        query: Some(body.query.clone()),
        product_id: Some(body.product_id),
        version_id: body.version_id.clone(),
        include_library: body.include_library,
        tag_ids: body.tag_ids.clone(),
        match_count: 8,
        expand_wiki: true,
        expand_graph: true,
        requirements: vec![],
        version_scope: "current".into(),
        product_ids: vec![],
        workspace_id: None,
        scope: None,
        group_by: "none".into(),
        tender_text: None,
    };
    let assembled = knowledge::search::assembly_pg(&pool, &sreq)
        .await
        .map_err(map_search)?;
    let model = knowledge::current_summary_model(&pool, body.product_id)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "stub-chat".into());
    let res =
        knowledge::search::answer_from_hits(&body.query, &body.context, assembled.hits, &model);
    let citations = res
        .citations
        .into_iter()
        .map(|c| {
            let mut m = HashMap::new();
            m.insert("document_id".into(), json!(c.document_id));
            m.insert("version_id".into(), json!(c.version_id));
            m.insert("start_at".into(), json!(c.start_at));
            m.insert("end_at".into(), json!(c.end_at));
            m
        })
        .collect();
    Ok(Json(AnswerOut {
        answer: res.answer,
        hits: res.hits,
        citations,
    }))
}

async fn dead_letters(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<knowledge::DeadLetter>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor).await?;
    let pool = pg().await?;
    let out = knowledge::list_dead_letters(&pool).await.map_err(pg_err)?;
    Ok(Json(out))
}

#[derive(Serialize)]
struct QueueView {
    memory: HashMap<String, usize>,
    pending_ops: HashMap<String, i64>,
    dead_letters: usize,
}

async fn list_queues(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<QueueView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor).await?;
    let pool = pg().await?;
    let pending_ops = knowledge::pending_op_counts(&pool).await.map_err(pg_err)?;
    let dead_letters = knowledge::count_dead_letters(&pool).await.map_err(pg_err)? as usize;
    Ok(Json(QueueView {
        memory: HashMap::new(),
        pending_ops,
        dead_letters,
    }))
}

#[derive(Deserialize)]
struct NewApiKey {
    name: String,
    #[serde(default = "default_scope_workspace")]
    scope_type: String,
    scope_id: Option<Uuid>,
    scopes: Vec<String>,
}

fn default_scope_workspace() -> String {
    "workspace".into()
}

#[derive(Serialize)]
struct ApiKeyView {
    id: Uuid,
    name: String,
    prefix: String,
    scope_type: String,
    scope_id: Uuid,
    scopes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

fn valid_scopes(scopes: &[String]) -> bool {
    !scopes.is_empty()
        && scopes
            .iter()
            .all(|s| matches!(s.as_str(), "ingest" | "search" | "admin"))
}

async fn create_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<NewApiKey>,
) -> Result<(StatusCode, Json<ApiKeyView>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    if !valid_scopes(&body.scopes) {
        return Err(validation("scopes must be ingest|search|admin"));
    }
    if body.scope_type != "workspace" && body.scope_type != "product" {
        return Err(validation("scope_type must be workspace or product"));
    }
    require_ws(id, &actor, false, true)?;
    let pool = pg().await?;
    knowledge::load_workspace(&pool, id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("workspace"))?;
    let scope_id = if body.scope_type == "workspace" {
        id
    } else {
        let pid = body
            .scope_id
            .ok_or_else(|| validation("scope_id required"))?;
        let p = knowledge::load_product(&pool, pid)
            .await
            .map_err(pg_err)?
            .ok_or_else(|| not_found("product"))?;
        if p.workspace_id != id {
            return Err(forbidden());
        }
        pid
    };
    let raw = format!("kb_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let key = ApiKey {
        id: Uuid::new_v4(),
        name: body.name,
        key_hash: platform::hash_password(&raw),
        prefix: raw.chars().take(10).collect(),
        scope_type: body.scope_type,
        scope_id,
        scopes: body.scopes,
    };
    knowledge::insert_api_key(
        &pool,
        knowledge::NewApiKey {
            id: key.id,
            name: &key.name,
            key_hash: &key.key_hash,
            prefix: &key.prefix,
            scope_type: &key.scope_type,
            scope_id: key.scope_id,
            scopes: &key.scopes,
        },
    )
    .await
    .map_err(pg_err)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiKeyView {
            id: key.id,
            name: key.name,
            prefix: key.prefix,
            scope_type: key.scope_type,
            scope_id: key.scope_id,
            scopes: key.scopes,
            token: Some(raw),
        }),
    ))
}

async fn list_api_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ApiKeyView>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, true)?;
    let pool = pg().await?;
    let out = knowledge::list_api_keys_for_workspace(&pool, id)
        .await
        .map_err(pg_err)?
        .into_iter()
        .map(|k| ApiKeyView {
            id: k.id,
            name: k.name,
            prefix: k.prefix,
            scope_type: k.scope_type,
            scope_id: k.scope_id,
            scopes: k.scopes,
            token: None,
        })
        .collect();
    Ok(Json(out))
}

async fn delete_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, key_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_ws(id, &actor, false, true)?;
    let pool = pg().await?;
    let k = knowledge::load_api_key(&pool, key_id)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| not_found("api_key"))?;
    let owned = k.scope_type == "workspace" && k.scope_id == id
        || k.scope_type == "product"
            && knowledge::load_product(&pool, k.scope_id)
                .await
                .ok()
                .flatten()
                .is_some_and(|p| p.workspace_id == id);
    if !owned {
        return Err(not_found("api_key"));
    }
    knowledge::delete_api_key(&pool, key_id)
        .await
        .map_err(pg_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn global_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FileQuery>,
) -> Result<Vec<u8>, ApiErr> {
    let _actor = actor_from(&headers, &state).await?;
    let hash = q.key.trim_start_matches("objects/");
    if hash.is_empty() {
        return Err(not_found("file"));
    }
    platform::read_blob(hash).map_err(|_| not_found("file"))
}

pub(crate) fn durable_human_actor(actor: &Actor) -> Result<String, ApiErr> {
    match actor {
        Actor::User(id) => Ok(format!("user:{id}")),
        Actor::Key(key) => Ok(format!("api_key:{}", key.id)),
        Actor::Bootstrap => Err(fail(
            StatusCode::FORBIDDEN,
            "HUMAN_ACTOR_REQUIRED",
            "Bootstrap cannot perform this bidding mutation",
        )),
    }
}

pub(crate) fn required_idempotency_key(headers: &HeaderMap) -> Result<String, ApiErr> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 200)
        .map(str::to_string)
        .ok_or_else(|| validation("Idempotency-Key header is required"))
}

pub(crate) async fn require_bid_pool() -> Result<sqlx::PgPool, ApiErr> {
    platform::connect().await.map_err(|error| {
        tracing::error!(error = %error, "bid database unavailable");
        fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "DATABASE_UNAVAILABLE",
            "bid database unavailable",
        )
    })
}
