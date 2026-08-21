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
use domain::{
    ApiKey, Document, ParseStatus, Product, ProductKind, ProductVersion, Role, Store,
    TYPE_DATATABLE, TYPE_DOCUMENT_PROCESS, TYPE_KB_DELETE, TYPE_LIST_DELETE, TYPE_LIST_REPARSE,
    TYPE_MANUAL_PROCESS, Tag, User, VersionStatus, Workspace, is_audio_type, is_image_type,
    is_valid_file_type, is_video,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

pub fn build(state: AppState) -> Router {
    let app = Router::<AppState>::new()
        .route("/health", get_s(health))
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
        .route("/api/v1/bids", get_s(list_bids).merge(post_s(create_bid)))
        .route("/api/v1/bids/{id}", get_s(get_bid).merge(post_s(end_bid)))
        .route(
            "/api/v1/bids/{id}/documents",
            get_s(list_bid_docs).merge(post_s(upload_bid_doc)),
        )
        .route(
            "/api/v1/bids/{id}/documents/{did}",
            delete_s(delete_bid_doc),
        )
        .route(
            "/api/v1/bids/{id}/documents/{did}/retry",
            post_s(retry_bid_doc),
        )
        .route("/api/v1/bids/{id}/extract", post_s(reextract_bid))
        .route(
            "/api/v1/bids/{id}/sections/{sid}/retry",
            post_s(retry_bid_section),
        )
        .route(
            "/api/v1/bids/{id}/sections/{sid}/merge",
            post_s(merge_bid_section),
        )
        .route("/api/v1/bids/{id}/units", get_s(list_bid_units))
        .route(
            "/api/v1/bids/{id}/clauses",
            get_s(list_bid_clauses).merge(post_s(add_bid_clause)),
        )
        .route("/api/v1/bids/{id}/clauses/{cid}", patch_s(patch_bid_clause))
        .route("/api/v1/bids/{id}/match", post_s(run_bid_match))
        .route(
            "/api/v1/bids/{id}/picks",
            get_s(list_bid_picks).merge(post_s(upsert_bid_pick)),
        )
        .route("/api/v1/bids/{id}/picks/{pid}", delete_s(delete_bid_pick))
        .route(
            "/api/v1/bids/{id}/shots",
            get_s(list_bid_shots).merge(post_s(upload_bid_shot)),
        )
        .route("/api/v1/bids/{id}/shots/{sid}", delete_s(delete_bid_shot))
        .route("/api/v1/bids/{id}/preview", get_s(bid_preview))
        .route("/api/v1/bids/{id}/booklet", get_s(list_bid_booklet))
        .route("/api/v1/bids/{id}/booklet/{key}", put_s(put_bid_booklet))
        .route(
            "/api/v1/bids/{id}/booklet/{key}/regenerate",
            post_s(regen_bid_booklet),
        )
        .route("/api/v1/bids/{id}/export", get_s(bid_export))
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
            domain::max_file_bytes() + 1024 * 1024,
        ));
    let app = if let Some((storage, catalog)) = runtime::dashboard_catalog() {
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
    require_admin(&state, &actor)?;
    Ok(next.run(request).await)
}

async fn health(State(_state): State<AppState>) -> Json<HealthBody> {
    Json(HealthBody {
        status: "ok",
        service: "api",
    })
}

type ApiErr = (StatusCode, Json<ErrorBody>);

fn parse_rank(st: ParseStatus) -> u8 {
    match st {
        ParseStatus::Pending => 0,
        ParseStatus::Processing => 1,
        ParseStatus::Finalizing => 2,
        ParseStatus::Completed
        | ParseStatus::Failed
        | ParseStatus::Cancelled
        | ParseStatus::Deleting => 3,
    }
}

fn merge_catalog(dst: &mut Store, src: Store) {
    // PG overwrites catalog. A memory document that is further along than PG
    // (in-process drain, worker not yet flushed) is kept.
    dst.workspaces.extend(src.workspaces);
    dst.members.extend(src.members);
    for (k, mut v) in src.products {
        if v.current_version_id.is_none()
            && let Some(old) = dst.products.get(&k)
        {
            v.current_version_id = old.current_version_id;
            if v.embedding_model_id.is_empty() {
                v.embedding_model_id = old.embedding_model_id.clone();
            }
        }
        dst.products.insert(k, v);
    }
    for (k, mut v) in src.versions {
        if v.embedding_model_id.is_empty()
            && let Some(old) = dst.versions.get(&k)
        {
            v.embedding_model_id = old.embedding_model_id.clone();
        }
        dst.versions.insert(k, v);
    }
    for (k, v) in src.documents {
        match dst.documents.get(&k) {
            Some(old) if parse_rank(old.parse_status) > parse_rank(v.parse_status) => {}
            _ => {
                dst.documents.insert(k, v);
            }
        }
    }
    dst.tags.extend(src.tags);
    dst.document_tags.extend(src.document_tags);
    for (k, v) in src.chunks {
        dst.chunks.entry(k).or_insert(v);
    }
    for (k, v) in src.embeddings {
        dst.embeddings.entry(k).or_insert(v);
    }
    dst.graph.extend(src.graph);
    dst.relations.extend(src.relations);
    dst.wiki.extend(src.wiki);
}

pub(crate) async fn ensure_workspace(state: &AppState, workspace_id: Uuid) {
    let Ok(pool) = storage::connect().await else {
        return;
    };
    let mut tmp = Store::default();
    if !storage::hydrate_workspace(&pool, &mut tmp, workspace_id)
        .await
        .unwrap_or(false)
    {
        return;
    }
    if let Ok(mut s) = lock(state) {
        merge_catalog(&mut s, tmp);
    }
}

async fn ensure_user_workspaces(state: &AppState, user_id: Uuid) {
    let Ok(pool) = storage::connect().await else {
        return;
    };
    let Ok(ids) = storage::workspaces_for_user(&pool, user_id).await else {
        return;
    };
    for id in ids {
        ensure_workspace(state, id).await;
    }
}

async fn ensure_product(state: &AppState, product_id: Uuid) {
    let Ok(pool) = storage::connect().await else {
        return;
    };
    if let Ok(Some(ws)) = storage::product_workspace_id(&pool, product_id).await {
        ensure_workspace(state, ws).await;
    }
}

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, Store>, ApiErr> {
    state.store.lock().map_err(|_| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "VALIDATION",
            "store lock",
        )
    })
}

#[derive(Clone)]
enum Actor {
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

async fn actor_from(headers: &HeaderMap, state: &AppState) -> Result<Actor, ApiErr> {
    if let Some(raw) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        if !state.bootstrap_key.is_empty() && raw == state.bootstrap_key {
            return Ok(Actor::Bootstrap);
        }
        let hash = auth::hash_password(raw);
        {
            let s = lock(state)?;
            if let Some(key) = s.api_keys.values().find(|k| k.key_hash == hash).cloned() {
                return Ok(Actor::Key(key));
            }
        }
        if let Ok(pool) = storage::connect().await
            && let Ok(Some(key)) = storage::find_api_key_by_hash(&pool, &hash).await
        {
            if let Ok(mut s) = lock(state) {
                s.api_keys.insert(key.id, key.clone());
            }
            return Ok(Actor::Key(key));
        }
        return Err(unauthorized());
    }
    let raw = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(unauthorized)?;
    let token = raw.strip_prefix("Bearer ").ok_or_else(unauthorized)?;
    let uid = auth::parse_jwt(token, &state.jwt_secret).map_err(|_| unauthorized())?;
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

fn require_ws(
    _store: &Store,
    _ws: Uuid,
    actor: &Actor,
    _write: bool,
    _admin: bool,
) -> Result<Role, ApiErr> {
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
    let hash = auth::hash_password(&body.password);
    let email = body.email.clone();
    let id = {
        let mut s = lock(&state)?;
        if s.users_by_email.contains_key(&body.email) {
            return Err(fail(StatusCode::CONFLICT, "CONFLICT", "email taken"));
        }
        let id = Uuid::new_v4();
        s.users.insert(
            id,
            User {
                id,
                email: email.clone(),
                password_hash: hash.clone(),
                ldap_dn: String::new(),
            },
        );
        s.users_by_email.insert(body.email, id);
        id
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::insert_user(&pool, id, &email, Some(&hash)).await;
    }
    let token = auth::issue_jwt(id, &state.jwt_secret).map_err(|e| validation(&e.to_string()))?;
    Ok(Json(TokenBody { token, user_id: id }))
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<AuthBody>,
) -> Result<Json<TokenBody>, ApiErr> {
    if auth::local_open() {
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
            auth::issue_jwt(id, &state.jwt_secret).map_err(|e| validation(&e.to_string()))?;
        return Ok(Json(TokenBody { token, user_id: id }));
    }
    if !auth::ldap_url().is_empty() {
        let dn = auth::ldap_bind(&body.email, &body.password).map_err(|_| unauthorized())?;
        let id = {
            let mut s = lock(&state)?;
            if let Some(id) = s.users_by_email.get(&body.email).copied() {
                if let Some(u) = s.users.get_mut(&id) {
                    u.ldap_dn = dn.clone();
                }
                id
            } else {
                let id = Uuid::new_v4();
                s.users.insert(
                    id,
                    User {
                        id,
                        email: body.email.clone(),
                        password_hash: String::new(),
                        ldap_dn: dn.clone(),
                    },
                );
                s.users_by_email.insert(body.email.clone(), id);
                id
            }
        };
        if let Ok(pool) = storage::connect().await {
            let _ = storage::insert_user(&pool, id, &body.email, None).await;
        }
        let token =
            auth::issue_jwt(id, &state.jwt_secret).map_err(|e| validation(&e.to_string()))?;
        return Ok(Json(TokenBody { token, user_id: id }));
    }
    let mem = {
        let s = lock(&state)?;
        s.users_by_email
            .get(&body.email)
            .and_then(|id| s.users.get(id).cloned())
    };
    let u = if let Some(u) = mem {
        u
    } else if let Ok(pool) = storage::connect().await {
        let row = storage::find_user_by_email(&pool, &body.email)
            .await
            .ok()
            .flatten()
            .ok_or_else(unauthorized)?;
        let u = User {
            id: row.0,
            email: row.1,
            password_hash: row.2,
            ldap_dn: String::new(),
        };
        if let Ok(mut s) = lock(&state) {
            s.users_by_email.insert(u.email.clone(), u.id);
            s.users.insert(u.id, u.clone());
        }
        u
    } else {
        return Err(unauthorized());
    };
    if !auth::verify_password(&body.password, &u.password_hash) {
        return Err(unauthorized());
    }
    let token = auth::issue_jwt(u.id, &state.jwt_secret).map_err(|e| validation(&e.to_string()))?;
    Ok(Json(TokenBody {
        token,
        user_id: u.id,
    }))
}

#[derive(Serialize)]
struct MeBody {
    id: Uuid,
    email: String,
}

async fn ensure_local_user(state: &AppState, email: &str) -> Result<Uuid, ApiErr> {
    {
        let s = lock(state)?;
        if let Some(id) = s.users_by_email.get(email).copied() {
            return Ok(id);
        }
    }
    if let Ok(pool) = storage::connect().await
        && let Ok(Some((id, db_email, hash))) = storage::find_user_by_email(&pool, email).await
    {
        if let Ok(mut s) = lock(state) {
            s.users.insert(
                id,
                User {
                    id,
                    email: db_email,
                    password_hash: hash,
                    ldap_dn: String::new(),
                },
            );
            s.users_by_email.insert(email.into(), id);
        }
        return Ok(id);
    }
    let id = Uuid::new_v4();
    {
        let mut s = lock(state)?;
        s.users.insert(
            id,
            User {
                id,
                email: email.into(),
                password_hash: String::new(),
                ldap_dn: String::new(),
            },
        );
        s.users_by_email.insert(email.into(), id);
    }
    if let Ok(pool) = storage::connect().await {
        let _ = storage::insert_user(&pool, id, email, None).await;
    }
    Ok(id)
}

async fn me(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<MeBody>, ApiErr> {
    let uid = user_from(&headers, &state).await?;
    {
        let s = lock(&state)?;
        if let Some(u) = s.users.get(&uid) {
            return Ok(Json(MeBody {
                id: u.id,
                email: u.email.clone(),
            }));
        }
    }
    if let Ok(pool) = storage::connect().await
        && let Ok(Some((id, email))) = storage::find_user_by_id(&pool, uid).await
    {
        if let Ok(mut s) = lock(&state) {
            s.users.insert(
                id,
                User {
                    id,
                    email: email.clone(),
                    password_hash: String::new(),
                    ldap_dn: String::new(),
                },
            );
            s.users_by_email.insert(email.clone(), id);
        }
        return Ok(Json(MeBody { id, email }));
    }
    Err(not_found("user"))
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
    let email = {
        let mut s = lock(&state)?;
        let u = s.users.get_mut(&uid).ok_or_else(|| not_found("user"))?;
        if let Some(e) = body.email {
            u.email = e;
        }
        u.email.clone()
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::update_user_email(&pool, uid, &email).await;
    }
    Ok(Json(MeBody { id: uid, email }))
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
        Some("company") => domain::WorkspaceKind::Company,
        _ => domain::WorkspaceKind::ProductLine,
    };
    let (view, ws_id, ws_name, ws_slug, kind_s) = {
        let mut s = lock(&state)?;
        if s.workspaces.values().any(|w| w.slug == body.slug) {
            return Err(fail(StatusCode::CONFLICT, "CONFLICT", "slug taken"));
        }
        if kind == domain::WorkspaceKind::Company
            && s.workspaces
                .values()
                .any(|w| w.kind == domain::WorkspaceKind::Company)
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
        s.members.insert((ws.id, uid), Role::Owner);
        let view = WorkspaceView::from(&ws);
        let ids = (
            view,
            ws.id,
            ws.name.clone(),
            ws.slug.clone(),
            ws.kind.as_str(),
        );
        s.workspaces.insert(ws.id, ws);
        ids
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::insert_workspace_kind(&pool, ws_id, &ws_name, &ws_slug, kind_s).await;
        let _ = storage::insert_member(&pool, ws_id, uid, "owner").await;
    }
    Ok((StatusCode::CREATED, Json(view)))
}

#[derive(Serialize)]
struct WorkspaceView {
    id: Uuid,
    name: String,
    slug: String,
    kind: domain::WorkspaceKind,
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
    if let Ok(pool) = storage::connect().await {
        let _ = storage::ensure_company_workspace(&pool).await;
        if let Ok(ids) = storage::list_workspace_ids(&pool).await {
            for id in ids {
                ensure_workspace(&state, id).await;
            }
        }
    } else if let Some(uid) = actor.user_id() {
        ensure_user_workspaces(&state, uid).await;
    }
    let s = lock(&state)?;
    let out = s
        .workspaces
        .values()
        .filter(|w| require_ws(&s, w.id, &actor, false, false).is_ok())
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
    ensure_workspace(&state, id).await;
    let s = lock(&state)?;
    require_ws(&s, id, &actor, false, false)?;
    let w = s
        .workspaces
        .get(&id)
        .ok_or_else(|| not_found("workspace"))?;
    Ok(Json(WorkspaceView::from(w)))
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
    ensure_workspace(&state, id).await;
    let view = {
        let mut s = lock(&state)?;
        require_ws(&s, id, &actor, false, true)?;
        let w = s
            .workspaces
            .get_mut(&id)
            .ok_or_else(|| not_found("workspace"))?;
        if let Some(n) = body.name {
            w.name = n;
        }
        WorkspaceView::from(w)
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::update_workspace_name(&pool, id, &view.name).await;
    }
    Ok(Json(view))
}

async fn delete_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_workspace(&state, id).await;
    let versions = {
        let mut s = lock(&state)?;
        require_ws(&s, id, &actor, false, true)?;
        let versions: Vec<_> = s
            .products
            .values()
            .filter(|p| p.workspace_id == id)
            .flat_map(|p| {
                s.versions
                    .values()
                    .filter(|v| v.product_id == p.id)
                    .map(|v| v.id)
            })
            .collect();
        for vid in &versions {
            s.enqueue(
                TYPE_KB_DELETE,
                domain::QUEUE_LOW,
                json!({ "product_version_id": vid }),
            );
        }
        for d in s.documents.values_mut() {
            if versions.contains(&d.product_version_id)
                && matches!(
                    d.parse_status,
                    ParseStatus::Pending | ParseStatus::Processing | ParseStatus::Finalizing
                )
            {
                d.parse_status = ParseStatus::Cancelled;
            }
        }
        s.members.retain(|(w, _), _| *w != id);
        s.workspaces.remove(&id);
        versions
    };
    if let Ok(pool) = storage::connect().await {
        let mut vids = versions.clone();
        if let Ok(pg) = storage::version_ids_for_workspace(&pool, id).await {
            for v in pg {
                if !vids.contains(&v) {
                    vids.push(v);
                }
            }
        }
        let _ = storage::cancel_active_docs_for_versions(&pool, &vids).await;
        for vid in vids {
            let _ = runtime::enqueue_kb_delete(vid).await;
        }
        let _ = storage::retire_workspace(&pool, id).await;
    }
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
    ensure_workspace(&state, id).await;
    let s = lock(&state)?;
    require_ws(&s, id, &actor, false, false)?;
    let out = s
        .members
        .iter()
        .filter(|((w, _), _)| *w == id)
        .map(|((_, u), r)| MemberView {
            user_id: *u,
            role: *r,
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
    ensure_workspace(&state, id).await;
    {
        let mut s = lock(&state)?;
        require_ws(&s, id, &actor, false, true)?;
        s.members.insert((id, body.user_id), body.role);
    }
    if let Ok(pool) = storage::connect().await {
        let _ = storage::insert_member(&pool, id, body.user_id, role_name(body.role)).await;
    }
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
    ensure_workspace(&state, id).await;
    {
        let mut s = lock(&state)?;
        require_ws(&s, id, &actor, false, true)?;
        s.members.insert((id, user_id), body.role);
    }
    if let Ok(pool) = storage::connect().await {
        let _ = storage::upsert_member(&pool, id, user_id, role_name(body.role)).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_workspace(&state, id).await;
    {
        let mut s = lock(&state)?;
        require_ws(&s, id, &actor, false, true)?;
        s.members.remove(&(id, user_id));
    }
    if let Ok(pool) = storage::connect().await {
        let _ = storage::delete_member(&pool, id, user_id).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn get_retrieval(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<domain::RetrievalConfig>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_workspace(&state, id).await;
    let s = lock(&state)?;
    require_ws(&s, id, &actor, false, false)?;
    let w = s
        .workspaces
        .get(&id)
        .ok_or_else(|| not_found("workspace"))?;
    Ok(Json(w.retrieval.clone()))
}

async fn patch_retrieval(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<domain::RetrievalConfig>,
) -> Result<Json<domain::RetrievalConfig>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_workspace(&state, id).await;
    let view = {
        let mut s = lock(&state)?;
        require_ws(&s, id, &actor, false, true)?;
        let w = s
            .workspaces
            .get_mut(&id)
            .ok_or_else(|| not_found("workspace"))?;
        w.retrieval = body;
        w.retrieval.clone()
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::set_retrieval_config(
            &pool,
            id,
            view.vector_threshold,
            view.keyword_threshold,
            view.embedding_top_k as i32,
        )
        .await;
    }
    Ok(Json(view))
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
    ensure_workspace(&state, id).await;
    let s = lock(&state)?;
    require_ws(&s, id, &actor, false, false)?;
    let out = s
        .products
        .values()
        .filter(|p| p.workspace_id == id)
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
    ensure_workspace(&state, id).await;
    let (view, pid, pname, pslug, pkind) = {
        let mut s = lock(&state)?;
        require_ws(&s, id, &actor, true, false)?;
        if s.products
            .values()
            .any(|p| p.workspace_id == id && p.slug == body.slug)
        {
            return Err(fail(StatusCode::CONFLICT, "CONFLICT", "slug taken"));
        }
        let ws_kind = s.workspaces.get(&id).map(|w| w.kind).unwrap_or_default();
        let kind = if ws_kind == domain::WorkspaceKind::Company {
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
        s.products.insert(p.id, p);
        (view, pid, pname, pslug, pkind)
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::insert_product(&pool, pid, id, pkind, &pname, &pslug, None).await;
    }
    Ok((StatusCode::CREATED, Json(view)))
}

async fn get_product(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<ProductView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
    let s = lock(&state)?;
    let p = s.products.get(&id).ok_or_else(|| not_found("product"))?;
    require_ws(&s, p.workspace_id, &actor, false, false)?;
    Ok(Json(ProductView::from(p)))
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
    ensure_product(&state, id).await;
    let view = {
        let mut s = lock(&state)?;
        let ws = s
            .products
            .get(&id)
            .ok_or_else(|| not_found("product"))?
            .workspace_id;
        require_ws(&s, ws, &actor, false, true)?;
        let p = s.products.get_mut(&id).unwrap();
        if let Some(n) = body.name {
            p.name = n;
        }
        ProductView::from(p)
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::update_product_name(&pool, id, &view.name).await;
    }
    Ok(Json(view))
}

async fn delete_product(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
    let vids = {
        let mut s = lock(&state)?;
        let p = s
            .products
            .get(&id)
            .ok_or_else(|| not_found("product"))?
            .clone();
        require_ws(&s, p.workspace_id, &actor, false, true)?;
        if p.kind == ProductKind::Library && p.slug == "library" {
            return Err(fail(
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
                "default library cannot be deleted",
            ));
        }
        let vids: Vec<Uuid> = s
            .versions
            .values()
            .filter(|v| v.product_id == id)
            .map(|v| v.id)
            .collect();
        for vid in &vids {
            s.enqueue(
                TYPE_KB_DELETE,
                domain::QUEUE_LOW,
                json!({ "product_version_id": vid }),
            );
        }
        for d in s.documents.values_mut() {
            if vids.contains(&d.product_version_id)
                && matches!(
                    d.parse_status,
                    ParseStatus::Pending | ParseStatus::Processing | ParseStatus::Finalizing
                )
            {
                d.parse_status = ParseStatus::Cancelled;
            }
        }
        vids
    };
    if let Ok(pool) = storage::connect().await {
        let mut all = vids.clone();
        if let Ok(pg) = storage::version_ids_for_product(&pool, id).await {
            for v in pg {
                if !all.contains(&v) {
                    all.push(v);
                }
            }
        }
        let _ = storage::cancel_active_docs_for_versions(&pool, &all).await;
        for vid in all {
            let _ = runtime::enqueue_kb_delete(vid).await;
        }
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

fn version_view(s: &Store, v: &ProductVersion) -> VersionView {
    let current = s
        .products
        .get(&v.product_id)
        .and_then(|p| p.current_version_id)
        == Some(v.id);
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
    ensure_product(&state, id).await;
    let s = lock(&state)?;
    let p = s.products.get(&id).ok_or_else(|| not_found("product"))?;
    require_ws(&s, p.workspace_id, &actor, false, false)?;
    let out = s
        .versions
        .values()
        .filter(|v| v.product_id == id)
        .map(|v| version_view(&s, v))
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
    ensure_product(&state, id).await;
    let clone_from = body.clone_from;
    let make_current = body.make_current;
    let diffs: Vec<serde_json::Value> = body
        .diffs
        .iter()
        .map(|d| json!({"op": d.op, "source_document_id": d.source_document_id}))
        .collect();
    let (view, view_id, label, made_current) = {
        let mut s = lock(&state)?;
        let p = s
            .products
            .get(&id)
            .ok_or_else(|| not_found("product"))?
            .clone();
        require_ws(&s, p.workspace_id, &actor, true, false)?;
        if s.versions.values().any(|v| {
            v.product_id == id && v.label == body.label && v.status != VersionStatus::Archived
        }) {
            return Err(fail(StatusCode::CONFLICT, "CONFLICT", "label taken"));
        }
        let mut v = ProductVersion::new(id, body.label);
        v.cloned_from = clone_from;
        if let Some(src) = clone_from {
            if let Some(src_v) = s.versions.get(&src).cloned() {
                v.vector_enabled = src_v.vector_enabled;
                v.keyword_enabled = src_v.keyword_enabled;
                v.wiki_enabled = src_v.wiki_enabled;
                v.graph_enabled = src_v.graph_enabled;
                v.extract_enabled = src_v.extract_enabled;
                v.extract_custom_instructions = src_v.extract_custom_instructions.clone();
                v.question_enabled = src_v.question_enabled;
                v.question_count = src_v.question_count;
                v.question_custom_instructions = src_v.question_custom_instructions.clone();
                v.table_metadata_instructions = src_v.table_metadata_instructions.clone();
                v.enable_multimodel = src_v.enable_multimodel;
                v.asr_enabled = src_v.asr_enabled;
                v.asr_model_id = src_v.asr_model_id;
                v.embedding_model_id = src_v.embedding_model_id;
                v.summary_model_id = src_v.summary_model_id;
                v.wiki_synthesis_model_id = src_v.wiki_synthesis_model_id;
                v.chunk_size = src_v.chunk_size;
                v.chunk_overlap = src_v.chunk_overlap;
                v.chunk_strategy = src_v.chunk_strategy;
                v.enable_parent_child = src_v.enable_parent_child;
                v.parent_chunk_size = src_v.parent_chunk_size;
                v.child_chunk_size = src_v.child_chunk_size;
                v.chunk_separators = src_v.chunk_separators;
                v.chunk_token_limit = src_v.chunk_token_limit;
                v.chunk_languages = src_v.chunk_languages;
                v.parser_engine_rules = src_v.parser_engine_rules;
            }
            v.status = VersionStatus::Cloning;
        }
        let view_id = v.id;
        let label = v.label.clone();
        s.versions.insert(v.id, v);
        let made_current = p.current_version_id.is_none();
        if made_current && let Some(prod) = s.products.get_mut(&id) {
            prod.current_version_id = Some(view_id);
        }
        let view = version_view(&s, s.versions.get(&view_id).unwrap());
        (view, view_id, label, made_current)
    };
    if let Ok(pool) = storage::connect().await {
        if let Some(src) = clone_from {
            let _ = storage::insert_version_cloning(&pool, view_id, id, &label, src).await;
        } else {
            let _ = storage::insert_version(&pool, view_id, id, &label, "active", None).await;
        }
        if made_current {
            let _ = storage::set_product_current(&pool, id, view_id).await;
        }
    }
    if let Some(src) = clone_from {
        let _ = runtime::enqueue_version_clone(src, view_id, json!(diffs), make_current).await;
    }
    Ok((StatusCode::CREATED, Json(view)))
}

async fn get_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
) -> Result<Json<VersionView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
    let s = lock(&state)?;
    let p = s.products.get(&id).ok_or_else(|| not_found("product"))?;
    require_ws(&s, p.workspace_id, &actor, false, false)?;
    let vid = resolve_write_version(&s, id, &version_id)?;
    Ok(Json(version_view(&s, s.versions.get(&vid).unwrap())))
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

async fn patch_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
    Json(body): Json<PatchVersion>,
) -> Result<Json<VersionView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
    let (view, vid, cfg) = {
        let mut s = lock(&state)?;
        let p = s.products.get(&id).ok_or_else(|| not_found("product"))?;
        require_ws(&s, p.workspace_id, &actor, false, true)?;
        let vid = resolve_write_version(&s, id, &version_id)?;
        let v = s.versions.get_mut(&vid).unwrap();
        if let Some(st) = body.status {
            v.status = st;
        }
        if let Some(n) = body.chunk_size.filter(|n| *n > 0) {
            v.chunk_size = n;
        }
        if let Some(n) = body.chunk_overlap.filter(|n| *n > 0) {
            v.chunk_overlap = n;
        }
        if let Some(st) = body.chunk_strategy.filter(|s| !s.is_empty()) {
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
        if let Some(s) = body.separators.filter(|s| !s.is_empty()) {
            v.chunk_separators = s;
        }
        if let Some(n) = body.token_limit.filter(|n| *n > 0) {
            v.chunk_token_limit = n;
        }
        if let Some(l) = body.languages.filter(|l| !l.is_empty()) {
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
        if let Some(s) = body.question_custom_instructions {
            v.question_custom_instructions = s;
        }
        if let Some(s) = body.table_metadata_instructions {
            v.table_metadata_instructions = s;
        }
        if let Some(b) = body.enable_multimodel {
            v.enable_multimodel = b;
        }
        if let Some(b) = body.asr_enabled {
            v.asr_enabled = b;
        }
        if let Some(m) = body.embedding_model_id.filter(|s| !s.is_empty()) {
            v.embedding_model_id = m;
        }
        if let Some(m) = body.summary_model_id.filter(|s| !s.is_empty()) {
            v.summary_model_id = m;
        }
        if let Some(m) = body.wiki_synthesis_model_id {
            v.wiki_synthesis_model_id = m;
        }
        if let Some(m) = body.asr_model_id {
            v.asr_model_id = m;
        }
        let view = version_view(&s, s.versions.get(&vid).unwrap());
        let v = s.versions.get(&vid).unwrap();
        let cfg = storage::VersionConfig {
            status: Some(version_status_str(view.status).into()),
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
        };
        (view, vid, cfg)
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::update_version_config(&pool, vid, cfg).await;
    }
    Ok(Json(view))
}

async fn delete_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
    let vid = {
        let mut s = lock(&state)?;
        let p = s.products.get(&id).ok_or_else(|| not_found("product"))?;
        require_ws(&s, p.workspace_id, &actor, false, true)?;
        let vid = resolve_write_version(&s, id, &version_id)?;
        for d in s.documents.values_mut() {
            if d.product_version_id == vid
                && matches!(
                    d.parse_status,
                    ParseStatus::Pending | ParseStatus::Processing | ParseStatus::Finalizing
                )
            {
                d.parse_status = ParseStatus::Cancelled;
            }
        }
        if let Some(v) = s.versions.get_mut(&vid) {
            v.status = VersionStatus::Archived;
        }
        if let Some(prod) = s.products.get_mut(&id)
            && prod.current_version_id == Some(vid)
        {
            prod.current_version_id = None;
        }
        s.enqueue(
            TYPE_KB_DELETE,
            domain::QUEUE_LOW,
            json!({ "product_version_id": vid }),
        );
        vid
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::cancel_active_docs_for_versions(&pool, &[vid]).await;
    }
    let _ = runtime::enqueue_kb_delete(vid).await;
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
    ensure_product(&state, id).await;
    let view = {
        let mut s = lock(&state)?;
        let p = s
            .products
            .get(&id)
            .ok_or_else(|| not_found("product"))?
            .clone();
        require_ws(&s, p.workspace_id, &actor, false, true)?;
        let v = s
            .versions
            .get(&body.version_id)
            .ok_or_else(|| not_found("version"))?
            .clone();
        if v.product_id != id {
            return Err(validation("version not on product"));
        }
        write_active(&s, body.version_id)?;
        if let Some(err) = embedding_mismatch(&s, p.workspace_id, &v.embedding_model_id) {
            return Err(fail(StatusCode::BAD_REQUEST, "EMBEDDING_MISMATCH", err));
        }
        s.products.get_mut(&id).unwrap().current_version_id = Some(body.version_id);
        s.products.get_mut(&id).unwrap().embedding_model_id = v.embedding_model_id.clone();
        ProductView::from(s.products.get(&id).unwrap())
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::set_product_current(&pool, id, body.version_id).await;
    }
    Ok(Json(view))
}

fn embedding_mismatch(store: &Store, workspace_id: Uuid, incoming: &str) -> Option<String> {
    for p in store
        .products
        .values()
        .filter(|p| p.workspace_id == workspace_id && p.kind == domain::ProductKind::Product)
    {
        if let Some(vid) = p.current_version_id
            && let Some(ver) = store.versions.get(&vid)
            && !ver.embedding_model_id.is_empty()
            && !incoming.is_empty()
            && ver.embedding_model_id != incoming
        {
            return Some(format!(
                "workspace products must share embedding_model_id (have {}, got {incoming})",
                ver.embedding_model_id
            ));
        }
    }
    None
}

#[derive(Serialize)]
struct DocView {
    id: Uuid,
    product_version_id: Uuid,
    title: String,
    file_name: String,
    object_key: String,
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
            object_key: d.object_key.clone(),
            parse_status: d.parse_status,
            enable_status: d.enable_status.clone(),
            index_ready: d.index_ready,
            pending_subtasks_count: d.pending_subtasks_count,
            error_message: d.error_message.clone(),
            description: d.description.clone(),
        }
    }
}

fn write_active(s: &Store, vid: Uuid) -> Result<(), ApiErr> {
    let v = s.versions.get(&vid).ok_or_else(|| not_found("version"))?;
    if v.status != VersionStatus::Active {
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
    ensure_product(&state, id).await;
    let s = lock(&state)?;
    let p = s.products.get(&id).ok_or_else(|| not_found("product"))?;
    require_ws(&s, p.workspace_id, &actor, false, false)?;
    let vid = resolve_write_version(&s, id, &version_id)?;
    let keyword = q
        .keyword
        .as_deref()
        .map(|k| k.to_ascii_lowercase())
        .filter(|k| !k.is_empty());
    let out = s
        .documents
        .values()
        .filter(|d| d.product_version_id == vid)
        .filter(|d| {
            q.parse_status
                .as_ref()
                .is_none_or(|st| d.parse_status.as_str() == st)
        })
        .filter(|d| {
            keyword.as_ref().is_none_or(|k| {
                d.title.to_ascii_lowercase().contains(k)
                    || d.file_name.to_ascii_lowercase().contains(k)
                    || d.description.to_ascii_lowercase().contains(k)
            })
        })
        .filter(|d| q.tag.is_none_or(|t| s.document_tags.contains(&(d.id, t))))
        .map(DocView::from)
        .collect();
    Ok(Json(out))
}

fn enqueue_process(s: &mut Store, doc: &Document, extra: serde_json::Value) -> Result<(), String> {
    let mut payload = extra;
    payload["document_id"] = json!(doc.id);
    payload["product_version_id"] = json!(doc.product_version_id);
    payload["attempt"] = json!(doc.attempt);
    s.try_enqueue(TYPE_DOCUMENT_PROCESS, domain::QUEUE_DEFAULT, payload)
        .map(|_| ())
}

struct PendingIngest<'a> {
    vid: Uuid,
    title: String,
    file_name: String,
    bytes: &'a [u8],
    tag_ids: &'a [Uuid],
    ws: Uuid,
    overrides: Option<domain::ProcessOverrides>,
}

fn insert_pending(s: &mut Store, req: PendingIngest<'_>) -> Result<Document, ApiErr> {
    let PendingIngest {
        vid,
        title,
        file_name,
        bytes,
        tag_ids,
        ws,
        overrides,
    } = req;
    if is_video(&file_name) {
        return Err(validation("video is not allowed"));
    }
    if !is_valid_file_type(&file_name) {
        return Err(validation("file type not allowed"));
    }
    if bytes.len() > domain::max_file_bytes() {
        return Err(validation("file too large"));
    }
    let version = s.versions.get(&vid).ok_or_else(|| not_found("version"))?;
    if let Some(p) = s.products.get(&version.product_id) {
        let frozen = p.kind == ProductKind::Library
            && p.slug == "library"
            && s.workspaces
                .get(&p.workspace_id)
                .is_some_and(|w| w.kind == domain::WorkspaceKind::ProductLine);
        if frozen {
            return Err(fail(
                StatusCode::CONFLICT,
                "CONFLICT",
                "product-line default library is frozen; upload to company workspace",
            ));
        }
    }
    let eff = domain::resolve_process_config(version, overrides.as_ref());
    if is_image_type(&file_name) && (!eff.enable_multimodel || !domain::vlm_configured()) {
        return Err(validation("image requires VLM configuration"));
    }
    if is_audio_type(&file_name) && !eff.asr_enabled {
        return Err(validation("audio requires ASR configuration"));
    }
    let hash = domain::sha256_hex(bytes);
    if let Some(existing) = s.find_duplicate(vid, &file_name, bytes.len() as i64, &hash) {
        return Err(fail(
            StatusCode::CONFLICT,
            "CONFLICT",
            format!("duplicate file {existing}"),
        ));
    }
    let (hash, key) = storage::put(s, bytes);
    for t in tag_ids {
        let tag = s.tags.get(t).ok_or_else(|| validation("unknown tag"))?;
        if tag.workspace_id != ws {
            return Err(validation("tag not in workspace"));
        }
    }
    let mut doc = Document::new(vid, title, file_name.clone(), bytes.len() as i64, hash, key);
    doc.process_overrides = overrides.filter(|o| !o.is_empty());
    if file_name.ends_with(".csv") || file_name.ends_with(".xlsx") || file_name.ends_with(".xls") {
        let _ = s.try_enqueue(
            TYPE_DATATABLE,
            domain::QUEUE_SUMMARY,
            json!({ "document_id": doc.id }),
        );
    }
    for t in tag_ids {
        s.document_tags.insert((doc.id, *t));
    }
    s.documents.insert(doc.id, doc.clone());
    if let Err(e) = enqueue_process(s, &doc, json!({})) {
        s.fail_document(doc.id, &e);
        doc = s.documents.get(&doc.id).cloned().unwrap_or(doc);
        return Ok(doc);
    }
    Ok(doc)
}

async fn persist_ingest_row(doc: &Document, tag_ids: &[Uuid]) -> Result<(), ApiErr> {
    let Ok(pool) = storage::connect().await else {
        return Ok(());
    };
    let version_in_pg = storage::version_exists(&pool, doc.product_version_id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e.to_string()))?;
    if !version_in_pg {
        return Ok(());
    }
    if let Ok(Some(existing)) = storage::find_duplicate_document(
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
    if let Err(e) = storage::insert_document(
        &pool,
        storage::NewDocument {
            id: doc.id,
            product_version_id: doc.product_version_id,
            title: &doc.title,
            file_name: &doc.file_name,
            file_size: doc.file_size,
            file_hash: &doc.file_hash,
            object_key: &doc.object_key,
        },
    )
    .await
    {
        if storage::is_unique_violation(&e) {
            let existing = storage::find_duplicate_document(
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
    let _ = storage::set_document_source(&pool, doc.id, &doc.doc_type, &doc.source_passages).await;
    let _ = storage::insert_document_tags(&pool, doc.id, tag_ids).await;
    let _ = storage::bump_object_ref(&pool, &doc.file_hash, doc.file_size).await;
    if let Some(o) = &doc.process_overrides {
        let _ = storage::set_process_overrides(&pool, doc.id, o).await;
    }
    let _ = storage::open_attempt(&pool, doc.id, doc.attempt).await;
    if doc.parse_status == ParseStatus::Failed {
        let _ = storage::set_parse_status(&pool, doc.id, "failed", &doc.error_message).await;
    }
    Ok(())
}

async fn persist_failed_row(doc: &Document) {
    if doc.parse_status != ParseStatus::Failed {
        return;
    }
    let Ok(pool) = storage::connect().await else {
        return;
    };
    if !storage::version_exists(&pool, doc.product_version_id)
        .await
        .unwrap_or(false)
    {
        return;
    }
    let _ = storage::set_parse_status(&pool, doc.id, "failed", &doc.error_message).await;
}

fn rollback_ingest(state: &AppState, doc: &Document) {
    if let Ok(mut s) = lock(state) {
        s.documents.remove(&doc.id);
        s.document_tags.retain(|(id, _)| *id != doc.id);
        s.queue.retain(|j| {
            j.payload.get("document_id").and_then(|v| v.as_str()) != Some(&doc.id.to_string())
        });
        storage::drop_ref(&mut s, &doc.file_hash);
    }
}

fn resolve_write_version(s: &Store, product_id: Uuid, version_id: &str) -> Result<Uuid, ApiErr> {
    if version_id == "current"
        && s.products
            .get(&product_id)
            .is_some_and(|p| p.current_version_id.is_none())
    {
        return Err(validation("no current version"));
    }
    s.resolve_version(product_id, version_id)
        .ok_or_else(|| not_found("version"))
}

async fn ensure_document(state: &AppState, document_id: Uuid) {
    if let Ok(pool) = storage::connect().await
        && let Ok(Some(ws)) = storage::document_workspace_id(&pool, document_id).await
    {
        ensure_workspace(state, ws).await;
    }
}

async fn push_document_job(s: &std::sync::Mutex<domain::Store>, doc: &mut Document) {
    if doc.parse_status == ParseStatus::Failed {
        return;
    }
    match runtime::enqueue_document_process(doc.id, doc.product_version_id, doc.attempt).await {
        Ok(_) => {}
        Err(e) => {
            if let Ok(mut store) = s.lock() {
                store.fail_document(doc.id, &e);
                if let Some(d) = store.documents.get(&doc.id) {
                    *doc = d.clone();
                }
            }
        }
    }
}

async fn ingest_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<DocView>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
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
            overrides = serde_json::from_str::<domain::ProcessOverrides>(&t).ok();
        }
    }
    if bytes.is_empty() {
        return Err(validation("empty file"));
    }
    let mut doc = {
        let mut s = lock(&state)?;
        let p = s
            .products
            .get(&id)
            .ok_or_else(|| not_found("product"))?
            .clone();
        require_ws(&s, p.workspace_id, &actor, true, false)?;
        let vid = resolve_write_version(&s, id, &version_id)?;
        write_active(&s, vid)?;
        insert_pending(
            &mut s,
            PendingIngest {
                vid,
                title: file_name.clone(),
                file_name,
                bytes: &bytes,
                tag_ids: &tag_ids,
                ws: p.workspace_id,
                overrides,
            },
        )?
    };
    if let Err(e) = persist_ingest_row(&doc, &tag_ids).await {
        rollback_ingest(&state, &doc);
        return Err(e);
    }
    if doc.file_name.ends_with(".csv")
        || doc.file_name.ends_with(".xlsx")
        || doc.file_name.ends_with(".xls")
    {
        let _ = runtime::enqueue_datatable(doc.id).await;
    }
    push_document_job(&state.store, &mut doc).await;
    persist_failed_row(&doc).await;
    Ok((ingest_status(&doc), Json(DocView::from(&doc))))
}

#[derive(Deserialize)]
struct UrlIn {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    process_config: Option<domain::ProcessOverrides>,
}

async fn ingest_url(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
    Json(body): Json<UrlIn>,
) -> Result<(StatusCode, Json<DocView>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
    if domain::url_blocked(&body.url) {
        return Err(validation("url failed SSRF check"));
    }
    let name = body.title.unwrap_or_else(|| "remote.md".into());
    let bytes = format!("url:{}", body.url).into_bytes();
    let mut doc = {
        let mut s = lock(&state)?;
        let p = s
            .products
            .get(&id)
            .ok_or_else(|| not_found("product"))?
            .clone();
        require_ws(&s, p.workspace_id, &actor, true, false)?;
        let vid = resolve_write_version(&s, id, &version_id)?;
        write_active(&s, vid)?;
        let mut doc = insert_pending(
            &mut s,
            PendingIngest {
                vid,
                title: name.clone(),
                file_name: name,
                bytes: &bytes,
                tag_ids: &[],
                ws: p.workspace_id,
                overrides: body.process_config,
            },
        )?;
        if let Some(d) = s.documents.get_mut(&doc.id) {
            d.doc_type = "url".into();
            doc = d.clone();
        }
        doc
    };
    if let Err(e) = persist_ingest_row(&doc, &[]).await {
        rollback_ingest(&state, &doc);
        return Err(e);
    }
    push_document_job(&state.store, &mut doc).await;
    persist_failed_row(&doc).await;
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
    process_config: Option<domain::ProcessOverrides>,
}

async fn ingest_passage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
    Json(body): Json<PassageIn>,
) -> Result<(StatusCode, Json<DocView>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
    let joined = body.passages.join("\n");
    let bytes = joined.as_bytes();
    let mut doc = {
        let mut s = lock(&state)?;
        let p = s
            .products
            .get(&id)
            .ok_or_else(|| not_found("product"))?
            .clone();
        require_ws(&s, p.workspace_id, &actor, true, false)?;
        let vid = resolve_write_version(&s, id, &version_id)?;
        write_active(&s, vid)?;
        let mut doc = insert_pending(
            &mut s,
            PendingIngest {
                vid,
                title: body.title.clone(),
                file_name: format!("{}.txt", body.title),
                bytes,
                tag_ids: &body.tag_ids,
                ws: p.workspace_id,
                overrides: body.process_config,
            },
        )?;
        if let Some(j) = s.queue.back_mut() {
            j.payload["passages"] = json!(body.passages);
        }
        if let Some(d) = s.documents.get_mut(&doc.id) {
            d.title = body.title;
            d.doc_type = "passage".into();
            d.source_passages = body.passages.clone();
            doc = d.clone();
        }
        doc
    };
    if let Err(e) = persist_ingest_row(&doc, &body.tag_ids).await {
        rollback_ingest(&state, &doc);
        return Err(e);
    }
    match runtime::enqueue_document_process_with(
        doc.id,
        doc.product_version_id,
        doc.attempt,
        body.passages,
    )
    .await
    {
        Ok(_) => {}
        Err(e) => {
            if let Ok(mut store) = lock(&state) {
                store.fail_document(doc.id, &e);
                if let Some(d) = store.documents.get(&doc.id) {
                    doc = d.clone();
                }
            }
        }
    }
    persist_failed_row(&doc).await;
    Ok((ingest_status(&doc), Json(DocView::from(&doc))))
}

#[derive(Deserialize)]
struct ManualIn {
    title: String,
    content: String,
    #[serde(default)]
    tag_ids: Vec<Uuid>,
    #[serde(default)]
    process_config: Option<domain::ProcessOverrides>,
}

async fn ingest_manual(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(Uuid, String)>,
    Json(body): Json<ManualIn>,
) -> Result<(StatusCode, Json<DocView>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
    if body.content.trim().is_empty() {
        return Err(validation("content required"));
    }
    let bytes = body.content.as_bytes();
    let file_name = if body.title.to_ascii_lowercase().ends_with(".md") {
        body.title.clone()
    } else {
        format!("{}.md", body.title)
    };
    let mut doc = {
        let mut s = lock(&state)?;
        let p = s
            .products
            .get(&id)
            .ok_or_else(|| not_found("product"))?
            .clone();
        require_ws(&s, p.workspace_id, &actor, true, false)?;
        let vid = resolve_write_version(&s, id, &version_id)?;
        write_active(&s, vid)?;
        let mut doc = insert_pending(
            &mut s,
            PendingIngest {
                vid,
                title: body.title.clone(),
                file_name,
                bytes,
                tag_ids: &body.tag_ids,
                ws: p.workspace_id,
                overrides: body.process_config,
            },
        )?;
        if let Some(j) = s.queue.back_mut() {
            j.task_type = TYPE_MANUAL_PROCESS.to_string();
            j.payload["manual"] = json!(true);
        }
        if let Some(d) = s.documents.get_mut(&doc.id) {
            d.title = body.title;
            d.doc_type = "manual".into();
            doc = d.clone();
        }
        doc
    };
    if let Err(e) = persist_ingest_row(&doc, &body.tag_ids).await {
        rollback_ingest(&state, &doc);
        return Err(e);
    }
    match runtime::enqueue_manual_process(doc.id, doc.product_version_id, doc.attempt).await {
        Ok(_) => {}
        Err(e) => {
            if let Ok(mut store) = lock(&state) {
                store.fail_document(doc.id, &e);
                if let Some(d) = store.documents.get(&doc.id) {
                    doc = d.clone();
                }
            }
        }
    }
    persist_failed_row(&doc).await;
    Ok((ingest_status(&doc), Json(DocView::from(&doc))))
}

async fn get_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<DocView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    if let Ok(pool) = storage::connect().await
        && let Ok(Some(ws)) = storage::document_workspace_id(&pool, id).await
    {
        ensure_workspace(&state, ws).await;
    }
    let s = lock(&state)?;
    let d = s.documents.get(&id).ok_or_else(|| not_found("document"))?;
    let vid = d.product_version_id;
    let pid = s
        .versions
        .get(&vid)
        .ok_or_else(|| not_found("version"))?
        .product_id;
    let ws = s
        .products
        .get(&pid)
        .ok_or_else(|| not_found("product"))?
        .workspace_id;
    require_ws(&s, ws, &actor, false, false)?;
    Ok(Json(DocView::from(d)))
}

async fn delete_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_document(&state, id).await;
    {
        let mut s = lock(&state)?;
        let d = s
            .documents
            .get(&id)
            .ok_or_else(|| not_found("document"))?
            .clone();
        let pid = s.versions.get(&d.product_version_id).unwrap().product_id;
        let ws = s.products.get(&pid).unwrap().workspace_id;
        require_ws(&s, ws, &actor, true, false)?;
        if let Some(doc) = s.documents.get_mut(&id) {
            doc.parse_status = ParseStatus::Deleting;
        }
        s.enqueue(
            TYPE_LIST_DELETE,
            domain::QUEUE_LOW,
            json!({ "document_ids": [id] }),
        );
    }
    if let Ok(pool) = storage::connect().await {
        let _ = storage::set_parse_status(&pool, id, "deleting", "").await;
    }
    let _ = runtime::enqueue_list_delete(id).await;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize, Default)]
struct ReparseIn {
    #[serde(default)]
    process_config: Option<domain::ProcessOverrides>,
}

async fn reparse_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    body: Option<Json<ReparseIn>>,
) -> Result<Json<DocView>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_document(&state, id).await;
    let doc = {
        let mut s = lock(&state)?;
        let d = s
            .documents
            .get(&id)
            .ok_or_else(|| not_found("document"))?
            .clone();
        let pid = s.versions.get(&d.product_version_id).unwrap().product_id;
        let ws = s.products.get(&pid).unwrap().workspace_id;
        require_ws(&s, ws, &actor, true, false)?;
        write_active(&s, d.product_version_id)?;
        if let Some(doc) = s.documents.get_mut(&id) {
            doc.parse_status = ParseStatus::Pending;
            doc.enable_status = "disabled".into();
            if let Some(o) = body.as_ref().and_then(|b| b.process_config.clone()) {
                doc.process_overrides = Some(o).filter(|x| !x.is_empty());
            }
        }
        s.enqueue(
            TYPE_LIST_REPARSE,
            domain::QUEUE_LOW,
            json!({ "document_ids": [id] }),
        );
        s.documents.get(&id).unwrap().clone()
    };
    if let Ok(pool) = storage::connect().await {
        if let Some(o) = &doc.process_overrides {
            let _ = storage::set_process_overrides(&pool, id, o).await;
        }
        let _ = storage::mark_reparse_queued(&pool, id).await;
    }
    runtime::enqueue_list_reparse(id)
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
    ensure_document(&state, id).await;
    let view = {
        let mut s = lock(&state)?;
        let d = s
            .documents
            .get(&id)
            .ok_or_else(|| not_found("document"))?
            .clone();
        let pid = s.versions.get(&d.product_version_id).unwrap().product_id;
        let ws = s.products.get(&pid).unwrap().workspace_id;
        require_ws(&s, ws, &actor, true, false)?;
        if let Some(doc) = s.documents.get_mut(&id) {
            doc.parse_status = ParseStatus::Cancelled;
        }
        DocView::from(s.documents.get(&id).unwrap())
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::set_parse_status(&pool, id, "cancelled", "").await;
    }
    Ok(Json(view))
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
    if let Ok(pool) = storage::connect().await
        && let Ok(Some(ws)) = storage::document_workspace_id(&pool, id).await
    {
        ensure_workspace(&state, ws).await;
    }
    let (parse_status, mem_attempt, mem_spans, err_msg) = {
        let s = lock(&state)?;
        let d = s.documents.get(&id).ok_or_else(|| not_found("document"))?;
        let pid = s
            .versions
            .get(&d.product_version_id)
            .ok_or_else(|| not_found("version"))?
            .product_id;
        let ws = s
            .products
            .get(&pid)
            .ok_or_else(|| not_found("product"))?
            .workspace_id;
        require_ws(&s, ws, &actor, false, false)?;
        (
            d.parse_status.as_str().to_string(),
            d.attempt,
            s.spans
                .iter()
                .filter(|sp| sp.document_id == id)
                .cloned()
                .collect::<Vec<_>>(),
            d.error_message.clone(),
        )
    };
    let mut latest = mem_attempt;
    let mut rows = mem_spans;
    if let Ok(pool) = storage::connect().await {
        if let Ok(n) = storage::latest_span_attempt(&pool, id).await
            && n > 0
        {
            latest = n;
        }
        let want = q.attempt.filter(|n| *n > 0).unwrap_or(latest);
        if let Ok(pg) = storage::list_spans_attempt(&pool, id, want).await
            && !pg.is_empty()
        {
            rows = pg.into_iter().map(|r| r.into_span()).collect();
        }
    }
    let attempt = q.attempt.filter(|n| *n > 0).unwrap_or(latest);
    let (trace, current_stage, last_fail) = obs::build_trace(attempt, &parse_status, &rows);
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
                    "stage": obs::ROOT_NAME,
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
    ensure_workspace(&state, id).await;
    let s = lock(&state)?;
    require_ws(&s, id, &actor, false, false)?;
    Ok(Json(
        s.tags
            .values()
            .filter(|t| t.workspace_id == id)
            .cloned()
            .collect(),
    ))
}

async fn create_tag(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<NewTag>,
) -> Result<(StatusCode, Json<Tag>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_workspace(&state, id).await;
    let tag = {
        let mut s = lock(&state)?;
        require_ws(&s, id, &actor, true, false)?;
        let tag = Tag {
            id: Uuid::new_v4(),
            workspace_id: id,
            name: body.name,
            slug: body.slug,
        };
        s.tags.insert(tag.id, tag.clone());
        tag
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::insert_tag(&pool, tag.id, tag.workspace_id, &tag.name, &tag.slug).await;
    }
    Ok((StatusCode::CREATED, Json(tag)))
}

async fn delete_tag(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, tag_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_workspace(&state, id).await;
    {
        let mut s = lock(&state)?;
        require_ws(&s, id, &actor, false, true)?;
        let Some(tag) = s.tags.get(&tag_id).cloned() else {
            return Err(not_found("tag"));
        };
        if tag.workspace_id != id {
            return Err(not_found("tag"));
        }
        s.tags.remove(&tag_id);
        s.document_tags.retain(|(_, t)| *t != tag_id);
    }
    if let Ok(pool) = storage::connect().await {
        let _ = storage::delete_tag(&pool, tag_id).await;
    }
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
    ensure_document(&state, id).await;
    let kept = {
        let mut s = lock(&state)?;
        let d = s
            .documents
            .get(&id)
            .ok_or_else(|| not_found("document"))?
            .clone();
        let pid = s.versions.get(&d.product_version_id).unwrap().product_id;
        let ws = s.products.get(&pid).unwrap().workspace_id;
        require_ws(&s, ws, &actor, true, false)?;
        s.document_tags.retain(|(doc, _)| *doc != id);
        let mut kept = Vec::new();
        for t in body.tag_ids {
            if s.tags.get(&t).is_some_and(|tg| tg.workspace_id == ws) {
                s.document_tags.insert((id, t));
                kept.push(t);
            }
        }
        kept
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::replace_document_tags(&pool, id, &kept).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn wiki_pages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, vid)): Path<(Uuid, String)>,
) -> Result<Json<Vec<domain::WikiPage>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
    if let Ok(pool) = storage::connect().await
        && let Ok(Some(ws)) = storage::product_workspace_id(&pool, id).await
    {
        ensure_workspace(&state, ws).await;
    }
    let s = lock(&state)?;
    let p = s.products.get(&id).ok_or_else(|| not_found("product"))?;
    require_ws(&s, p.workspace_id, &actor, false, false)?;
    let version = resolve_write_version(&s, id, &vid)?;
    Ok(Json(
        s.wiki
            .values()
            .filter(|w| w.product_version_id == version)
            .cloned()
            .collect(),
    ))
}

async fn wiki_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, vid, slug)): Path<(Uuid, String, String)>,
) -> Result<Json<domain::WikiPage>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
    let s = lock(&state)?;
    let p = s.products.get(&id).ok_or_else(|| not_found("product"))?;
    require_ws(&s, p.workspace_id, &actor, false, false)?;
    let version = resolve_write_version(&s, id, &vid)?;
    s.wiki
        .get(&(version, slug))
        .cloned()
        .map(Json)
        .ok_or_else(|| not_found("page"))
}

async fn wiki_folders(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, vid)): Path<(Uuid, String)>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, id).await;
    let version = {
        let s = lock(&state)?;
        let p = s.products.get(&id).ok_or_else(|| not_found("product"))?;
        require_ws(&s, p.workspace_id, &actor, false, false)?;
        resolve_write_version(&s, id, &vid)?
    };
    let mut folders = {
        let s = lock(&state)?;
        s.wiki_folders
            .values()
            .filter(|f| f.product_version_id == version)
            .map(|f| {
                json!({
                    "id": f.id,
                    "name": f.name,
                    "path": f.path,
                    "depth": f.depth
                })
            })
            .collect::<Vec<_>>()
    };
    if folders.is_empty()
        && let Ok(pool) = storage::connect().await
        && let Ok(rows) = storage::list_wiki_folders(&pool, version).await
    {
        folders = rows
            .into_iter()
            .map(|(fid, name, path, depth)| {
                json!({"id": fid, "name": name, "path": path, "depth": depth})
            })
            .collect();
    }
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
    ensure_product(&state, id).await;
    let hash = q.key.trim_start_matches("objects/");
    if hash.is_empty() {
        return Err(not_found("file"));
    }
    let key = if q.key.starts_with("objects/") {
        q.key.clone()
    } else {
        format!("objects/{hash}")
    };
    let resolved = {
        let s = lock(&state)?;
        let p = s.products.get(&id).ok_or_else(|| not_found("product"))?;
        require_ws(&s, p.workspace_id, &actor, false, false)?;
        let version = resolve_write_version(&s, id, &vid)?;
        let mem = version_references_key(&s, version, &key, hash);
        (version, mem)
    };
    let mut allowed = resolved.1;
    if !allowed
        && let Ok(pool) = storage::connect().await
        && let Ok(hit) = storage::version_references_object(&pool, resolved.0, &key, hash).await
    {
        allowed = hit;
    }
    if !allowed {
        return Err(not_found("file"));
    }
    storage::read_blob(hash).map_err(|_| not_found("file"))
}

fn version_references_key(s: &Store, version_id: Uuid, key: &str, hash: &str) -> bool {
    s.documents.values().any(|d| {
        d.product_version_id == version_id
            && (d.object_key == key
                || d.object_key.ends_with(hash)
                || d.file_hash == hash
                || d.markdown.contains(key)
                || d.markdown.contains(hash))
    }) || s.chunks.values().any(|c| {
        c.product_version_id == version_id && (c.content.contains(key) || c.content.contains(hash))
    })
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
    require_admin(&state, &actor)?;
    let mut out = vec![
        json!({"id": "stub-emb", "kind": "embedding", "dimension": models::EMBEDDING_DIM}),
        json!({"id": "stub-chat", "kind": "chat", "dimension": 0}),
    ];
    if let Ok(pool) = storage::connect().await
        && let Ok(rows) = storage::list_models(&pool).await
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
        require_admin(&state, &actor)?;
    }
    let pool = storage::connect().await.ok();
    if let Some(pool) = pool {
        let _ = storage::upsert_model(&pool, &body.id, &body.kind, body.dimension).await;
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
        require_admin(&state, &actor)?;
    }
    let mut body = body;
    body.id = id;
    let pool = storage::connect().await.ok();
    if let Some(pool) = pool {
        let _ = storage::upsert_model(&pool, &body.id, &body.kind, body.dimension).await;
    }
    Ok(Json(body))
}

fn require_admin(state: &AppState, actor: &Actor) -> Result<(), ApiErr> {
    {
        let s = lock(state)?;
        let admin = match actor {
            Actor::Bootstrap => true,
            Actor::Key(k) => k.scopes.iter().any(|x| x == "admin"),
            Actor::User(uid) => s
                .members
                .iter()
                .any(|((_, u), r)| *u == *uid && r.can_admin()),
        };
        if !admin {
            return Err(forbidden());
        }
    }
    Ok(())
}

async fn ops_oxana(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor)?;
    let queues = runtime::queue_depths().await;
    let jobs = runtime::queue_job_previews().await;
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
    Json(mut req): Json<search::SearchRequest>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    if req.scope.is_some() {
        req.expand_wiki = false;
        req.expand_graph = false;
        req.include_library = false;
    }
    hydrate_search_workspace(&state, &actor, &req).await;
    if !matches!(req.mode.as_str(), "assembly" | "matching") {
        return Err(validation("mode must be assembly or matching"));
    }
    if req.mode == "matching" {
        let mem = {
            let s = lock(&state)?;
            if req.scope.is_none() {
                let ws = infer_workspace(&s, &actor, &req)?;
                req.workspace_id = Some(ws);
            }
            search::matching(&s, &req)
        };
        let out = match mem {
            Ok(r) if matching_has_hits(&r) => r,
            Ok(empty) => match storage::connect().await {
                Ok(pool) => search::matching_pg(&pool, &req).await.map_err(map_search)?,
                Err(_) => empty,
            },
            Err(e) if e.code == "NOT_FOUND" || e.code == "VALIDATION" => {
                let Ok(pool) = storage::connect().await else {
                    return Err(map_search(e));
                };
                match search::matching_pg(&pool, &req).await {
                    Ok(pg) => pg,
                    Err(pg_err) => return Err(map_search(pg_err)),
                }
            }
            Err(e) => return Err(map_search(e)),
        };
        let v = serde_json::to_value(&out).unwrap();
        assert!(v.get("best_product_id").is_none());
        return Ok(Json(v));
    }
    let mem = {
        let s = lock(&state)?;
        let ws = infer_workspace(&s, &actor, &req)?;
        req.workspace_id = Some(ws);
        search::assembly(&s, &req)
    };
    match mem {
        Ok(r) if !r.hits.is_empty() => Ok(Json(serde_json::to_value(&r).unwrap())),
        Ok(empty) => {
            if let Ok(pool) = storage::connect().await {
                match search::assembly_pg(&pool, &req).await {
                    Ok(pg) if !pg.hits.is_empty() => {
                        return Ok(Json(serde_json::to_value(&pg).unwrap()));
                    }
                    Ok(_) => {}
                    Err(e) if e.code == "UPSTREAM" => {
                        return Err(map_search(e));
                    }
                    Err(_) => {}
                }
            }
            Ok(Json(serde_json::to_value(&empty).unwrap()))
        }
        Err(e) => {
            if matches!(e.code, "NOT_FOUND" | "VALIDATION")
                && let Ok(pool) = storage::connect().await
            {
                match search::assembly_pg(&pool, &req).await {
                    Ok(pg) => return Ok(Json(serde_json::to_value(&pg).unwrap())),
                    Err(pg_err) => return Err(map_search(pg_err)),
                }
            }
            Err(map_search(e))
        }
    }
}

async fn do_match(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut req): Json<search::SearchRequest>,
) -> Result<Json<search::MatchingResponse>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    req.mode = "matching".into();
    if req.scope.is_some() {
        req.expand_wiki = false;
        req.expand_graph = false;
        req.include_library = false;
    }
    hydrate_search_workspace(&state, &actor, &req).await;
    let mem = {
        let s = lock(&state)?;
        if req.scope.is_none() {
            let ws = infer_workspace(&s, &actor, &req)?;
            req.workspace_id = Some(ws);
        }
        search::matching(&s, &req)
    };
    match mem {
        Ok(r) if matching_has_hits(&r) => {
            debug_assert!(
                serde_json::to_value(&r)
                    .unwrap()
                    .get("best_product_id")
                    .is_none()
            );
            Ok(Json(r))
        }
        Ok(empty) => {
            if let Ok(pool) = storage::connect().await {
                match search::matching_pg(&pool, &req).await {
                    Ok(pg) if matching_has_hits(&pg) => return Ok(Json(pg)),
                    Ok(_) if !empty.candidates.is_empty() => return Ok(Json(empty)),
                    Ok(pg) => return Ok(Json(pg)),
                    Err(e) if empty.candidates.is_empty() => return Err(map_search(e)),
                    Err(_) => return Ok(Json(empty)),
                }
            }
            Ok(Json(empty))
        }
        Err(e) if e.code == "NOT_FOUND" || e.code == "VALIDATION" => {
            if let Ok(pool) = storage::connect().await {
                return match search::matching_pg(&pool, &req).await {
                    Ok(pg) => Ok(Json(pg)),
                    Err(pg_err) => Err(map_search(pg_err)),
                };
            }
            Err(map_search(e))
        }
        Err(e) => Err(map_search(e)),
    }
}

async fn hydrate_search_workspace(state: &AppState, actor: &Actor, req: &search::SearchRequest) {
    if let Some(ws) = req.workspace_id {
        ensure_workspace(state, ws).await;
        return;
    }
    if let Some(pid) = req.product_id {
        ensure_product(state, pid).await;
        return;
    }
    match actor {
        Actor::User(uid) => ensure_user_workspaces(state, *uid).await,
        Actor::Key(k) if k.scope_type == "workspace" => ensure_workspace(state, k.scope_id).await,
        Actor::Key(k) if k.scope_type == "product" => ensure_product(state, k.scope_id).await,
        _ => {}
    }
}

fn matching_has_hits(r: &search::MatchingResponse) -> bool {
    r.candidates
        .iter()
        .any(|c| c.requirements.iter().any(|req| req.hit))
        || r.clauses.iter().any(|c| c.outcome == "hit")
}

fn infer_workspace(s: &Store, actor: &Actor, req: &search::SearchRequest) -> Result<Uuid, ApiErr> {
    if let Some(pid) = req.product_id {
        let p = s.products.get(&pid).ok_or_else(|| not_found("product"))?;
        require_ws(s, p.workspace_id, actor, false, false)?;
        return Ok(p.workspace_id);
    }
    if let Some(ws) = req.workspace_id {
        require_ws(s, ws, actor, false, false)?;
        return Ok(ws);
    }
    match actor {
        Actor::User(uid) => {
            let mut it = s.members.iter().filter(|((_, u), _)| *u == *uid);
            let ((ws, _), _) = it.next().ok_or_else(forbidden)?;
            if it.next().is_some() {
                return Err(validation("workspace_id required when member of multiple"));
            }
            Ok(*ws)
        }
        Actor::Key(k) if k.scope_type == "workspace" => {
            require_ws(s, k.scope_id, actor, false, false)?;
            Ok(k.scope_id)
        }
        Actor::Key(k) if k.scope_type == "product" => {
            let p = s.products.get(&k.scope_id).ok_or_else(forbidden)?;
            require_ws(s, p.workspace_id, actor, false, false)?;
            Ok(p.workspace_id)
        }
        Actor::Bootstrap => Err(validation("workspace_id required")),
        _ => Err(forbidden()),
    }
}

fn map_search(e: search::SearchError) -> ApiErr {
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
    hits: Vec<search::Hit>,
    citations: Vec<HashMap<String, serde_json::Value>>,
}

async fn do_answer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AnswerIn>,
) -> Result<Json<AnswerOut>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    ensure_product(&state, body.product_id).await;
    {
        let s = lock(&state)?;
        let p = s
            .products
            .get(&body.product_id)
            .ok_or_else(|| not_found("product"))?;
        require_ws(&s, p.workspace_id, &actor, false, false)?;
        if p.current_version_id.is_none() {
            return Err(validation("product has no current version"));
        }
    }
    let req = search::AnswerRequest {
        query: body.query,
        product_id: body.product_id,
        version_id: body.version_id,
        include_library: body.include_library,
        tag_ids: body.tag_ids,
        context: body.context,
    };
    let store = state.store.clone();
    let req2 = req.clone();
    let mut res = tokio::task::spawn_blocking(move || {
        let s = store.lock().map_err(|_| search::SearchError {
            code: "INTERNAL",
            message: "store lock".into(),
        })?;
        search::answer(&s, &req)
    })
    .await
    .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e.to_string()))?
    .map_err(map_search)?;
    if res.hits.is_empty()
        && let Ok(pool) = storage::connect().await
    {
        let sreq = search::SearchRequest {
            mode: "assembly".into(),
            query: Some(req2.query.clone()),
            product_id: Some(req2.product_id),
            version_id: req2.version_id.clone(),
            include_library: req2.include_library,
            tag_ids: req2.tag_ids.clone(),
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
        if let Ok(pg) = search::assembly_pg(&pool, &sreq).await
            && !pg.hits.is_empty()
        {
            let model = storage::current_summary_model(&pool, req2.product_id)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "stub-chat".into());
            res = search::answer_from_hits(&req2.query, &req2.context, pg.hits, &model);
        }
    }
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
) -> Result<Json<Vec<domain::DeadLetter>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let mut out = {
        let s = lock(&state)?;
        let admin = match &actor {
            Actor::Bootstrap => true,
            Actor::Key(k) => k.scopes.iter().any(|x| x == "admin"),
            Actor::User(uid) => s
                .members
                .iter()
                .any(|((_, u), r)| *u == *uid && r.can_admin()),
        };
        if !admin {
            return Err(forbidden());
        }
        s.dead_letters.clone()
    };
    if let Ok(pool) = storage::connect().await
        && let Ok(pg) = storage::list_dead_letters(&pool).await
    {
        out.extend(pg);
    }
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
    let (memory, dead_letters) = {
        let s = lock(&state)?;
        let admin = match &actor {
            Actor::Bootstrap => true,
            Actor::Key(k) => k.scopes.iter().any(|x| x == "admin"),
            Actor::User(uid) => s
                .members
                .iter()
                .any(|((_, u), r)| *u == *uid && r.can_admin()),
        };
        if !admin {
            return Err(forbidden());
        }
        let mut memory = HashMap::new();
        for j in &s.queue {
            *memory.entry(j.queue.clone()).or_insert(0) += 1;
        }
        (memory, s.dead_letters.len())
    };
    let mut pending_ops = HashMap::new();
    let mut dead_letters = dead_letters;
    if let Ok(pool) = storage::connect().await {
        if let Ok(rows) = storage::pending_op_counts(&pool).await {
            pending_ops = rows;
        }
        if let Ok(n) = storage::count_dead_letters(&pool).await {
            dead_letters += n as usize;
        }
    }
    Ok(Json(QueueView {
        memory,
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
    let (view, persist) = {
        let mut s = lock(&state)?;
        require_ws(&s, id, &actor, false, true)?;
        let scope_id = if body.scope_type == "workspace" {
            id
        } else {
            let pid = body
                .scope_id
                .ok_or_else(|| validation("scope_id required"))?;
            let p = s.products.get(&pid).ok_or_else(|| not_found("product"))?;
            if p.workspace_id != id {
                return Err(forbidden());
            }
            pid
        };
        let raw = format!("kb_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let key = ApiKey {
            id: Uuid::new_v4(),
            name: body.name,
            key_hash: auth::hash_password(&raw),
            prefix: raw.chars().take(10).collect(),
            scope_type: body.scope_type,
            scope_id,
            scopes: body.scopes,
        };
        let view = ApiKeyView {
            id: key.id,
            name: key.name.clone(),
            prefix: key.prefix.clone(),
            scope_type: key.scope_type.clone(),
            scope_id: key.scope_id,
            scopes: key.scopes.clone(),
            token: Some(raw),
        };
        let persist = (
            key.id,
            key.name.clone(),
            key.key_hash.clone(),
            key.prefix.clone(),
            key.scope_type.clone(),
            key.scope_id,
            key.scopes.clone(),
        );
        s.api_keys.insert(key.id, key);
        (view, persist)
    };
    if let Ok(pool) = storage::connect().await {
        let _ = storage::insert_api_key(
            &pool,
            storage::NewApiKey {
                id: persist.0,
                name: &persist.1,
                key_hash: &persist.2,
                prefix: &persist.3,
                scope_type: &persist.4,
                scope_id: persist.5,
                scopes: &persist.6,
            },
        )
        .await;
    }
    Ok((StatusCode::CREATED, Json(view)))
}

async fn list_api_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ApiKeyView>>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let s = lock(&state)?;
    require_ws(&s, id, &actor, false, true)?;
    let out = s
        .api_keys
        .values()
        .filter(|k| {
            k.scope_type == "workspace" && k.scope_id == id
                || k.scope_type == "product"
                    && s.products
                        .get(&k.scope_id)
                        .is_some_and(|p| p.workspace_id == id)
        })
        .map(|k| ApiKeyView {
            id: k.id,
            name: k.name.clone(),
            prefix: k.prefix.clone(),
            scope_type: k.scope_type.clone(),
            scope_id: k.scope_id,
            scopes: k.scopes.clone(),
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
    {
        let mut s = lock(&state)?;
        require_ws(&s, id, &actor, false, true)?;
        let k = s
            .api_keys
            .get(&key_id)
            .ok_or_else(|| not_found("api_key"))?;
        let owned = k.scope_type == "workspace" && k.scope_id == id
            || k.scope_type == "product"
                && s.products
                    .get(&k.scope_id)
                    .is_some_and(|p| p.workspace_id == id);
        if !owned {
            return Err(not_found("api_key"));
        }
        s.api_keys.remove(&key_id);
    }
    if let Ok(pool) = storage::connect().await {
        let _ = storage::delete_api_key(&pool, key_id).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn global_file(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FileQuery>,
) -> Result<Vec<u8>, ApiErr> {
    let _actor = actor_from(&headers, &_state).await?;
    let hash = q.key.trim_start_matches("objects/");
    if hash.is_empty() {
        return Err(not_found("file"));
    }
    storage::read_blob(hash).map_err(|_| not_found("file"))
}

fn bid_project_from_row(r: &sqlx::postgres::PgRow) -> bid::ProjectView {
    use sqlx::Row;
    bid::ProjectView {
        id: r.get("id"),
        title: r.get("title"),
        owner_name: r.get("owner_name"),
        expires_at: r.get("expires_at"),
        status: r.get("status"),
        ended_at: r.get("ended_at"),
    }
}

async fn require_bid_pool() -> Result<sqlx::PgPool, ApiErr> {
    storage::connect().await.map_err(|error| {
        eprintln!("bid database unavailable: {error}");
        fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "DATABASE_UNAVAILABLE",
            "bid database unavailable",
        )
    })
}

fn bid_query_failed(operation: &str, error: impl std::fmt::Display) -> ApiErr {
    eprintln!("bid {operation} failed: {error}");
    fail(
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL",
        format!("bid {operation} failed"),
    )
}

async fn require_open_project(pool: &sqlx::PgPool, id: Uuid) -> Result<(), ApiErr> {
    use sqlx::Row;
    let row = storage::bid::get_project(pool, id)
        .await
        .map_err(|error| {
            eprintln!("bid project lookup failed: {error}");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
                "bid project lookup failed",
            )
        })?
        .ok_or_else(|| not_found("bid"))?;
    if row.get::<String, _>("status") == "ended" {
        return Err(fail(StatusCode::CONFLICT, "ENDED", "project ended"));
    }
    Ok(())
}

async fn list_bids(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<bid::ProjectView>>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let rows = storage::bid::list_projects(&pool)
        .await
        .map_err(|error| bid_query_failed("project list", error))?;
    Ok(Json(rows.iter().map(bid_project_from_row).collect()))
}

#[derive(Deserialize)]
struct NewBid {
    title: String,
    #[serde(default)]
    owner_name: String,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn create_bid(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NewBid>,
) -> Result<(StatusCode, Json<bid::ProjectView>), ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let id = Uuid::new_v4();
    let view = bid::ProjectView {
        id,
        title: body.title.clone(),
        owner_name: body.owner_name.clone(),
        expires_at: body.expires_at,
        status: "open".into(),
        ended_at: None,
    };
    let pool = require_bid_pool().await?;
    storage::bid::insert_project(&pool, id, &body.title, &body.owner_name, body.expires_at)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e.to_string()))?;
    Ok((StatusCode::CREATED, Json(view)))
}

async fn get_bid(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    use sqlx::Row;

    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let row = storage::bid::get_project(&pool, id)
        .await
        .map_err(|error| bid_query_failed("project lookup", error))?
        .ok_or_else(|| not_found("bid"))?;
    let p = bid_project_from_row(&row);
    let (files, ready, drafts, picks, pending_files, extract_running) =
        storage::bid::project_file_stats(&pool, id)
            .await
            .map_err(|error| bid_query_failed("project statistics", error))?;
    let match_running = storage::bid::any_match_running(&pool, id)
        .await
        .map_err(|error| bid_query_failed("match status", error))?;
    let match_jobs = storage::bid::current_match_jobs(&pool, id)
        .await
        .map_err(|error| bid_query_failed("match jobs", error))?
        .iter()
        .map(|row| {
            json!({
                "id": row.get::<Uuid, _>("id"),
                "job_kind": row.get::<String, _>("job_kind"),
                "unit_id": row.get::<Option<Uuid>, _>("unit_id"),
                "status": row.get::<String, _>("status"),
                "tech_status": row.get::<String, _>("tech_status"),
                "commercial_status": row.get::<String, _>("commercial_status"),
                "tech_candidates": row.get::<serde_json::Value, _>("tech_candidates"),
                "error_message": row.get::<String, _>("error_message")
            })
        })
        .collect::<Vec<_>>();
    let latest_extract = storage::bid::latest_extract(&pool, id)
        .await
        .map_err(|error| bid_query_failed("latest extraction", error))?
        .map(|row| {
            let diagnostics = row.get::<serde_json::Value, _>("diagnostics");
            let document_diagnostics: Vec<_> = diagnostics
                .get("documents")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|document| document.get("diagnostics"))
                .collect();
            let fallback_reasons: Vec<_> = document_diagnostics
                .iter()
                .filter_map(|item| item.get("fallback_reasons")?.as_array())
                .flatten()
                .cloned()
                .collect();
            let uncovered_spans: Vec<_> = document_diagnostics
                .iter()
                .filter_map(|item| item.get("coverage")?.get("uncovered_spans")?.as_array())
                .flatten()
                .cloned()
                .collect();
            let coverage_sum = |field: &str| {
                document_diagnostics
                    .iter()
                    .filter_map(|item| item.get("coverage")?.get(field)?.as_u64())
                    .sum::<u64>()
            };
            let failed_documents = diagnostics
                .get("failed_documents")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let error_message = row.get::<String, _>("error_message");
            let partial_failure = failed_documents > 0
                || diagnostics
                    .get("partial_failure")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                || error_message.contains("partial_failure");
            json!({
                "id": row.get::<Uuid, _>("id"),
                "status": row.get::<String, _>("status"),
                "extractor_mode": row.get::<String, _>("extractor_mode"),
                "failed_documents": failed_documents,
                "partial_failure": partial_failure,
                "diagnostics": {
                    "coverage": {
                        "candidate_spans": coverage_sum("candidate_spans"),
                        "covered_spans": coverage_sum("covered_spans"),
                        "uncovered_spans": uncovered_spans,
                        "ambiguous_clauses": coverage_sum("ambiguous_clauses")
                    },
                    "fallback_reasons": fallback_reasons
                },
                "error_message": error_message
            })
        });
    Ok(Json(json!({
        "project": p,
        "derived": bid::derived_status(
            files, ready, drafts, picks, pending_files, extract_running, match_running
        ),
        "latest_extract": latest_extract,
        "match_jobs": match_jobs
    })))
}

async fn end_bid(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    let ended = storage::bid::end_project(&pool, id)
        .await
        .map_err(|_| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", "end failed"))?;
    if !ended {
        return Err(fail(
            StatusCode::CONFLICT,
            "EXTRACT_RUNNING",
            "project extraction is running",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn list_bid_docs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let rows = storage::bid::list_documents(&pool, id)
        .await
        .map_err(|error| bid_query_failed("document list", error))?;
    use sqlx::Row;
    let docs: Vec<_> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "file_name": r.get::<String, _>("file_name"),
                "parse_status": r.get::<String, _>("parse_status"),
                "multimodal_status": r.get::<String, _>("multimodal_status"),
                "multimodal_error": r.get::<String, _>("multimodal_error"),
                "error_message": r.get::<String, _>("error_message"),
                "extract_status": r.get::<Option<String>, _>("extract_status"),
                "extract_error": r.get::<Option<String>, _>("extract_error"),
                "clause_count": r.get::<i64, _>("clause_count"),
                "object_key": r.get::<String, _>("object_key"),
            })
        })
        .collect();
    Ok(Json(json!({ "documents": docs })))
}

async fn upload_bid_doc(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    let mut file_name = String::from("tender.pdf");
    let mut bytes = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            if let Some(n) = field.file_name() {
                file_name = n.to_string();
            }
            bytes = field
                .bytes()
                .await
                .map_err(|e| validation(&e.to_string()))?
                .to_vec();
        }
    }
    if bytes.is_empty() {
        return Err(validation("file required"));
    }
    let hash = domain::sha256_hex(&bytes);
    let key = storage::object_key(&hash);
    storage::write_blob_async(&hash, &bytes)
        .await
        .map_err(|_| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
                "file write failed",
            )
        })?;
    let did = Uuid::new_v4();
    storage::bid::insert_document(&pool, did, id, &file_name, &hash, bytes.len() as i64, &key)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e.to_string()))?;
    // The pending row is durable; housekeeping re-enqueues it if Redis is temporarily unavailable.
    let _ = runtime::enqueue_bid_convert(did).await;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": did, "file_name": file_name, "parse_status": "pending" })),
    ))
}

async fn delete_bid_doc(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, did)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    let deleted = storage::bid::delete_document_for_project(&pool, id, did)
        .await
        .map_err(|error| {
            if error.to_string().contains("project extraction is running") {
                fail(
                    StatusCode::CONFLICT,
                    "EXTRACT_RUNNING",
                    "project extraction is running",
                )
            } else {
                fail(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL",
                    "delete failed",
                )
            }
        })?;
    if !deleted {
        return Err(not_found("document"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn retry_bid_doc(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, did)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    let reset = storage::bid::reset_document_for_retry(&pool, id, did)
        .await
        .map_err(|error| {
            if error.to_string().contains("project extraction is running") {
                fail(
                    StatusCode::CONFLICT,
                    "EXTRACT_RUNNING",
                    "project extraction is running",
                )
            } else {
                fail(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL",
                    "retry failed",
                )
            }
        })?;
    if !reset {
        return Err(not_found("document"));
    }
    // The pending row is durable; housekeeping re-enqueues it if Redis is temporarily unavailable.
    let _ = runtime::enqueue_bid_convert(did).await;
    Ok(StatusCode::ACCEPTED)
}

async fn retry_bid_section(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, sid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    let job_id = storage::bid::enqueue_section_retry(&pool, id, sid)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => not_found("section"),
            _ => bid_query_failed("section retry scheduling", error),
        })?;
    // The retry row is durable; housekeeping recovers a transient Redis enqueue failure.
    let _ = runtime::enqueue_bid_section_retry(job_id, id, sid).await;
    Ok(StatusCode::ACCEPTED)
}

async fn reextract_bid(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    let run_id = Uuid::new_v4();
    storage::bid::insert_extract_run(&pool, run_id, id, None, "manual")
        .await
        .map_err(|_| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "EXTRACTION_CREATE_FAILED",
                "could not create extraction run",
            )
        })?;
    // The pending run is durable; housekeeping re-enqueues it if Redis is temporarily unavailable.
    let _ = runtime::enqueue_bid_extract(run_id, id, None).await;
    Ok(StatusCode::ACCEPTED)
}

fn clause_from_row(
    r: &sqlx::postgres::PgRow,
    merge: &std::collections::HashMap<uuid::Uuid, Option<uuid::Uuid>>,
) -> bid::ClauseView {
    bid::clause_from_row(r, merge)
}

#[derive(Deserialize)]
struct ClauseListQ {
    #[serde(default)]
    include_superseded: bool,
}

async fn list_bid_clauses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<ClauseListQ>,
) -> Result<Json<Vec<bid::ClauseView>>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let merge = bid::section_merge_map(&pool, id)
        .await
        .map_err(|error| bid_query_failed("section merge lookup", error))?;
    let rows = storage::bid::list_clauses(&pool, id, q.include_superseded)
        .await
        .map_err(|error| bid_query_failed("clause list", error))?;
    let mut clauses: Vec<_> = rows.iter().map(|r| clause_from_row(r, &merge)).collect();
    bid::decorate_clauses(&pool, id, &mut clauses)
        .await
        .map_err(|error| bid_query_failed("clause decoration", error))?;
    Ok(Json(clauses))
}

#[derive(Deserialize)]
struct ClausePatch {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    must: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    deviate: Option<bool>,
    #[serde(default)]
    deviate_note: Option<String>,
    #[serde(default)]
    assessment: Option<String>,
}

async fn patch_bid_clause(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, cid)): Path<(Uuid, Uuid)>,
    Json(body): Json<ClausePatch>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    let rows = storage::bid::list_clauses(&pool, id, true)
        .await
        .map_err(|error| bid_query_failed("clause lookup", error))?;
    let Some(cur) = rows.iter().find(|r| {
        use sqlx::Row;
        r.get::<Uuid, _>("id") == cid
    }) else {
        return Err(not_found("clause"));
    };
    let merge = bid::section_merge_map(&pool, id)
        .await
        .map_err(|error| bid_query_failed("section merge lookup", error))?;
    let cur = clause_from_row(cur, &merge);
    let status = body.status.clone().unwrap_or_else(|| cur.status.clone());
    let text = body.text.as_deref().unwrap_or(&cur.text);
    let family = body.family.as_deref().unwrap_or(&cur.family);
    let must = body.must.unwrap_or(cur.must);
    if !matches!(family, "technical" | "commercial") {
        return Err(validation("family must be technical or commercial"));
    }
    if !matches!(status.as_str(), "draft" | "confirmed" | "rejected") {
        return Err(validation("status must be draft, confirmed, or rejected"));
    }
    let mut assessment = body
        .assessment
        .clone()
        .unwrap_or_else(|| cur.assessment.clone());
    if assessment.is_empty() {
        assessment = "unset".into();
    }
    if !matches!(
        assessment.as_str(),
        "unset" | "meet" | "partial" | "deviate" | "fail"
    ) {
        return Err(validation("invalid assessment"));
    }
    if assessment == "meet" {
        let mut all: Vec<_> = rows.iter().map(|r| clause_from_row(r, &merge)).collect();
        if let Some(c) = all.iter_mut().find(|c| c.id == cid) {
            c.status = status.clone();
            c.text = text.to_string();
            c.must = must;
            c.family = family.to_string();
        }
        bid::decorate_clauses(&pool, id, &mut all)
            .await
            .map_err(|error| bid_query_failed("clause assessment decoration", error))?;
        if all
            .iter()
            .any(|c| c.id == cid && bid::meet_blocked_by_suggestion(&c.suggestion))
        {
            return Err(fail(
                StatusCode::CONFLICT,
                "MEET_UNMET",
                "建议为未覆盖，不能评满足",
            ));
        }
    }
    let deviate = body
        .deviate
        .unwrap_or(assessment == "deviate" || cur.deviate);
    let updated = storage::bid::update_clause(
        &pool,
        storage::bid::ClausePatch {
            id: cid,
            project_id: id,
            expected_status: &cur.status,
            text: body.text.as_deref(),
            family: body.family.as_deref(),
            must: body.must,
            status: body.status.as_deref(),
            deviate: if body.deviate.is_some() || body.assessment.is_some() {
                Some(deviate)
            } else {
                None
            },
            deviate_note: body.deviate_note.as_deref(),
            assessment: body.assessment.as_ref().map(|_| assessment.as_str()),
        },
    )
    .await
    .map_err(|e| bid_query_failed("clause update", e))?;
    let Some(updated) = updated else {
        return Err(fail(
            StatusCode::CONFLICT,
            "STALE_CLAUSE",
            "clause changed or was superseded; reload before editing",
        ));
    };
    if updated.match_changed {
        let part = bid::booklet_key_for_unit(cur.unit_id);
        storage::bid::mark_booklet_stale(&pool, id, &["1", part.as_str()])
            .await
            .map_err(|error| bid_query_failed("booklet stale update", error))?;
        enqueue_bid_match(&pool, id).await?;
    }
    if body.assessment.is_some() || body.deviate.is_some() {
        let part = if cur.unit_id == bid::unsectioned_unit() {
            "2:unsectioned".to_string()
        } else {
            format!("2:{}", cur.unit_id)
        };
        storage::bid::mark_booklet_stale(&pool, id, &[part.as_str(), "3"])
            .await
            .map_err(|error| bid_query_failed("booklet assessment stale update", error))?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct NewClause {
    text: String,
    #[serde(default)]
    raw_text: String,
    #[serde(default = "tech_fam")]
    family: String,
    #[serde(default)]
    must: bool,
    #[serde(default)]
    section_id: Option<Uuid>,
}

fn tech_fam() -> String {
    "technical".into()
}

async fn add_bid_clause(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<NewClause>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    if !matches!(body.family.as_str(), "technical" | "commercial") {
        return Err(validation("family must be technical or commercial"));
    }
    let cid = Uuid::new_v4();
    let section_id = if body.family == "commercial" {
        None
    } else {
        body.section_id.filter(|u| !u.is_nil())
    };
    let merge = bid::section_merge_map(&pool, id)
        .await
        .map_err(|error| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", error))?;
    if section_id.is_some_and(|section_id| !merge.contains_key(&section_id)) {
        return Err(not_found("section"));
    }
    storage::bid::insert_clause(
        &pool,
        storage::bid::NewClause {
            id: cid,
            project_id: id,
            extract_run_id: None,
            section_id,
            source_document_id: None,
            source_span: None,
            family_conflict: false,
            extraction_meta: None,
            raw_text: &body.raw_text,
            text: &body.text,
            family: &body.family,
            must: body.must,
            status: "confirmed",
        },
    )
    .await
    .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e.to_string()))?;
    let unit = bid::resolve_unit(section_id, &merge);
    let part = bid::booklet_key_for_unit(unit);
    let _ = storage::bid::mark_booklet_stale(&pool, id, &["1", part.as_str()]).await;
    enqueue_bid_match(&pool, id).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": cid }))))
}

async fn enqueue_bid_match(pool: &sqlx::PgPool, project_id: Uuid) -> Result<(), ApiErr> {
    bid::schedule_match(pool, project_id)
        .await
        .map(|_| ())
        .map_err(|error| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "MATCH_SCHEDULE_FAILED",
                error,
            )
        })
}

async fn run_bid_match(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    enqueue_bid_match(&pool, id).await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
struct PickQ {
    #[serde(default)]
    unit_id: Option<Uuid>,
}

async fn list_bid_picks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<PickQ>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let Some(unit) = q.unit_id else {
        return Err(validation("unit_id required"));
    };
    let pool = require_bid_pool().await?;
    let picks = storage::bid::list_picks_for_unit(&pool, id, Some(unit))
        .await
        .map_err(|error| bid_query_failed("pick list", error))?;
    use sqlx::Row;
    let picks: Vec<_> = picks
        .iter()
        .map(|r| {
            json!({
                "product_id": r.get::<Uuid, _>("product_id"),
                "unit_id": r.try_get::<Uuid, _>("unit_id").unwrap_or(uuid::Uuid::nil()),
                "version_id": r.get::<Uuid, _>("version_id"),
                "score": r.get::<f64, _>("score"),
                "coverage": r.get::<f64, _>("coverage"),
                "clauses": r.get::<serde_json::Value, _>("clauses"),
            })
        })
        .collect();
    let job = storage::bid::latest_match_job_for_unit(&pool, id, Some(unit))
        .await
        .map_err(|error| bid_query_failed("latest unit match", error))?;
    let candidates = job
        .as_ref()
        .and_then(|j| j.try_get("tech_candidates").ok())
        .unwrap_or(json!([]));
    Ok(Json(json!({ "picks": picks, "candidates": candidates })))
}

#[derive(Deserialize)]
struct PickBody {
    product_id: Uuid,
    #[serde(default)]
    unit_id: Option<Uuid>,
}

async fn upsert_bid_pick(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<PickBody>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    let Some(unit) = body.unit_id else {
        return Err(validation("unit_id required"));
    };
    let job = storage::bid::latest_match_job_for_unit(&pool, id, Some(unit))
        .await
        .map_err(|error| bid_query_failed("latest unit match", error))?;
    use sqlx::Row;
    let candidates: Vec<search::Candidate> = job
        .as_ref()
        .and_then(|j| j.try_get::<serde_json::Value, _>("tech_candidates").ok())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let Some(c) = candidates
        .into_iter()
        .find(|c| c.product_id == body.product_id)
    else {
        return Err(validation(
            "product not in current candidates; rematch then pick",
        ));
    };
    let clauses = {
        let tech = storage::bid::confirmed_clauses(&pool, id, "technical")
            .await
            .map_err(|error| bid_query_failed("confirmed technical clauses", error))?;
        use sqlx::Row;
        json!(
            c.requirements
                .iter()
                .map(|r| {
                    let row = tech
                        .iter()
                        .find(|x| x.get::<Uuid, _>("id").to_string() == r.id);
                    json!({
                        "clause_id": r.id,
                        "text": row.map(|x| x.get::<String, _>("text")).unwrap_or_default(),
                        "must": row.map(|x| x.get::<bool, _>("must")).unwrap_or(false),
                        "hit": r.hit,
                        "hits": r.hits
                    })
                })
                .collect::<Vec<_>>()
        )
    };
    storage::bid::upsert_pick(
        &pool,
        id,
        unit,
        c.product_id,
        c.matched_version_id,
        c.score,
        c.coverage,
        &clauses,
    )
    .await
    .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e.to_string()))?;
    let part = if unit == bid::unsectioned_unit() {
        "2:unsectioned".to_string()
    } else {
        format!("2:{unit}")
    };
    let _ = storage::bid::mark_booklet_stale(&pool, id, &["1", part.as_str(), "3"]).await;
    for r in &c.requirements {
        let Ok(cid) = Uuid::parse_str(&r.id) else {
            continue;
        };
        for h in &r.hits {
            let Some(key) = h.image_object_key.as_deref() else {
                continue;
            };
            let _ = storage::bid::insert_shot(
                &pool,
                storage::bid::NewShot {
                    id: Uuid::new_v4(),
                    project_id: id,
                    clause_id: cid,
                    product_id: c.product_id,
                    version_id: h.version_id,
                    source: "matched",
                    object_key: key,
                    kb_document_id: Some(h.document_id),
                    kb_image_ref: Some(key),
                },
            )
            .await;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_bid_pick(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, pid)): Path<(Uuid, Uuid)>,
    Query(q): Query<PickQ>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    let Some(unit) = q.unit_id else {
        return Err(validation("unit_id required"));
    };
    let deleted = storage::bid::delete_pick(&pool, id, unit, pid)
        .await
        .map_err(|error| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
                error.to_string(),
            )
        })?;
    if !deleted {
        return Err(not_found("pick"));
    }
    let part = bid::booklet_key_for_unit(unit);
    let _ = storage::bid::mark_booklet_stale(&pool, id, &["1", part.as_str(), "3"]).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_bid_units(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let units = bid::list_match_units(&pool, id)
        .await
        .map_err(|error| bid_query_failed("unit list", error))?;
    Ok(Json(json!({ "units": units })))
}

#[derive(Deserialize)]
struct MergeBody {
    into: Uuid,
}

async fn merge_bid_section(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, sid)): Path<(Uuid, Uuid)>,
    Json(body): Json<MergeBody>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    if sid == body.into {
        return Err(validation("cannot merge a section into itself"));
    }
    let merge = bid::section_merge_map(&pool, id)
        .await
        .map_err(|error| bid_query_failed("section merge lookup", error))?;
    if !merge.contains_key(&sid) || !merge.contains_key(&body.into) {
        return Err(not_found("section"));
    }
    if bid::resolve_unit(Some(body.into), &merge) == sid {
        return Err(validation("merge would cycle"));
    }
    let merged = storage::bid::set_section_merge(&pool, id, sid, Some(body.into))
        .await
        .map_err(|error| {
            if error.to_string().contains("cycle") || error.to_string().contains("itself") {
                validation("merge would cycle")
            } else {
                bid_query_failed("section merge", error)
            }
        })?;
    if !merged {
        return Err(not_found("section"));
    }
    let keys = [
        "1".to_string(),
        "3".to_string(),
        bid::booklet_key_for_unit(sid),
        bid::booklet_key_for_unit(body.into),
    ];
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let _ = storage::bid::mark_booklet_stale(&pool, id, &refs).await;
    enqueue_bid_match(&pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_bid_shots(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let rows = storage::bid::list_shots(&pool, id)
        .await
        .map_err(|error| bid_query_failed("shot list", error))?;
    use sqlx::Row;
    let shots: Vec<_> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "clause_id": r.get::<Uuid, _>("clause_id"),
                "product_id": r.get::<Uuid, _>("product_id"),
                "source": r.get::<String, _>("source"),
                "object_key": r.get::<String, _>("object_key"),
            })
        })
        .collect();
    Ok(Json(json!({ "shots": shots })))
}

async fn upload_bid_shot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    let mut clause_id = Uuid::nil();
    let mut product_id = Uuid::nil();
    let mut version_id = Uuid::nil();
    let mut bytes = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("clause_id") => {
                if let Ok(v) = field.text().await {
                    clause_id = Uuid::parse_str(v.trim()).unwrap_or(Uuid::nil());
                }
            }
            Some("product_id") => {
                if let Ok(v) = field.text().await {
                    product_id = Uuid::parse_str(v.trim()).unwrap_or(Uuid::nil());
                }
            }
            Some("version_id") => {
                if let Ok(v) = field.text().await {
                    version_id = Uuid::parse_str(v.trim()).unwrap_or(Uuid::nil());
                }
            }
            Some("file") => {
                bytes = field
                    .bytes()
                    .await
                    .map_err(|e| validation(&e.to_string()))?
                    .to_vec();
            }
            _ => {}
        }
    }
    if bytes.is_empty() || clause_id.is_nil() || product_id.is_nil() || version_id.is_nil() {
        return Err(validation(
            "file, clause_id, product_id, version_id required",
        ));
    }
    let hash = domain::sha256_hex(&bytes);
    let key = storage::object_key(&hash);
    storage::write_blob_async(&hash, &bytes)
        .await
        .map_err(|_| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
                "file write failed",
            )
        })?;
    let sid = Uuid::new_v4();
    let inserted = storage::bid::insert_shot(
        &pool,
        storage::bid::NewShot {
            id: sid,
            project_id: id,
            clause_id,
            product_id,
            version_id,
            source: "uploaded",
            object_key: &key,
            kb_document_id: None,
            kb_image_ref: None,
        },
    )
    .await
    .map_err(|error| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            error.to_string(),
        )
    })?;
    if !inserted {
        return Err(validation(
            "clause, product, or version does not belong to this selection",
        ));
    }
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": sid, "object_key": key })),
    ))
}

async fn delete_bid_shot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, sid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open_project(&pool, id).await?;
    let deleted = storage::bid::delete_shot(&pool, id, sid)
        .await
        .map_err(|error| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
                error.to_string(),
            )
        })?;
    if !deleted {
        return Err(not_found("shot"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn bid_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let row = storage::bid::get_project(&pool, id)
        .await
        .map_err(|error| bid_query_failed("preview project", error))?
        .ok_or_else(|| not_found("bid"))?;
    let project = bid_project_from_row(&row);
    let merge = bid::section_merge_map(&pool, id)
        .await
        .map_err(|error| bid_query_failed("preview section map", sqlx::Error::Protocol(error)))?;
    let clauses: Vec<_> = storage::bid::list_clauses(&pool, id, true)
        .await
        .map_err(|error| bid_query_failed("preview clauses", error))?
        .iter()
        .map(|r| clause_from_row(r, &merge))
        .collect();
    use sqlx::Row;
    let picks: Vec<serde_json::Value> = storage::bid::list_picks(&pool, id)
        .await
        .map_err(|error| bid_query_failed("preview picks", error))?
        .iter()
        .map(|r| {
            json!({
                "product_id": r.get::<Uuid, _>("product_id"),
                "unit_id": r.try_get::<Uuid, _>("unit_id").unwrap_or(uuid::Uuid::nil()),
                "clauses": r.get::<serde_json::Value, _>("clauses"),
            })
        })
        .collect();
    let commercial: Vec<serde_json::Value> = storage::bid::list_commercial_hits(&pool, id)
        .await
        .map_err(|error| bid_query_failed("preview commercial hits", error))?
        .iter()
        .map(|r| {
            json!({
                "clause_id": r.get::<Uuid, _>("clause_id").to_string(),
                "outcome": r.get::<String, _>("outcome"),
                "file_name": r.try_get::<Option<String>, _>("file_name").ok().flatten(),
            })
        })
        .collect();
    let cov = bid::coverage_for(&clauses, &picks);
    Ok(Json(bid::preview_json(
        &project,
        &clauses,
        &picks,
        &commercial,
        &cov,
    )))
}

fn export_filename(title: &str, ext: &str) -> String {
    let stem: String = title
        .chars()
        .map(|c| match c {
            '"' | '/' | '\\' | '\n' | '\r' => '_',
            _ => c,
        })
        .take(60)
        .collect();
    let stem = stem.trim().trim_matches('_');
    let stem = if stem.is_empty() { "投标" } else { stem };
    if ext == "pdf" {
        format!("{stem}-定稿.pdf")
    } else {
        format!("{stem}-应答卷.docx")
    }
}

async fn list_bid_booklet(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let parts = bid::ensure_all_parts(&pool, id, false)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e))?;
    Ok(Json(json!({ "parts": parts })))
}

#[derive(Deserialize)]
struct BookletBody {
    markdown: String,
}

async fn put_bid_booklet(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, key)): Path<(Uuid, String)>,
    Json(body): Json<BookletBody>,
) -> Result<StatusCode, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bid::save_part(&pool, id, &key, &body.markdown)
        .await
        .map_err(|e| {
            if e == "project ended" {
                fail(StatusCode::CONFLICT, "ENDED", e)
            } else {
                fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e)
            }
        })?;
    Ok(StatusCode::NO_CONTENT)
}

async fn regen_bid_booklet(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, key)): Path<(Uuid, String)>,
) -> Result<Json<bid::BookletPartView>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let part = bid::ensure_part(&pool, id, &key, true).await.map_err(|e| {
        if e == "project ended" {
            fail(StatusCode::CONFLICT, "ENDED", e)
        } else {
            fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e)
        }
    })?;
    Ok(Json(part))
}

#[derive(Deserialize)]
struct ExportQ {
    #[serde(default)]
    format: String,
    #[serde(default)]
    regenerate_stale: bool,
}

async fn bid_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<ExportQ>,
) -> Result<
    (
        StatusCode,
        [(axum::http::header::HeaderName, String); 2],
        Vec<u8>,
    ),
    ApiErr,
> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let format = q.format.to_ascii_lowercase();
    if !matches!(format.as_str(), "" | "docx" | "pdf") {
        return Err(validation("format must be docx or pdf"));
    }
    let pdf = format == "pdf";
    let kind = if pdf {
        bid::ExportKind::Pdf
    } else {
        bid::ExportKind::Docx
    };
    let (title, bytes) = bid::export_project_opts(&pool, id, kind, q.regenerate_stale)
        .await
        .map_err(|e| {
            if e.contains("必须条款锚") {
                fail(StatusCode::CONFLICT, "MISSING_MUST", e)
            } else if e == "project ended" {
                fail(StatusCode::CONFLICT, "ENDED", e)
            } else {
                fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", e)
            }
        })?;
    let name = export_filename(&title, if pdf { "pdf" } else { "docx" });
    Ok((
        StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE,
                if pdf {
                    "application/pdf".into()
                } else {
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document".into()
                },
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn ensure_workspace_merges_tags_graph_wiki_chunks_into_live_store() {
        let Ok(pool) = storage::connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        let owner = Uuid::new_v4();
        storage::insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let slug = format!("merge-{}", owner.simple());
        let seeded = storage::create_workspace_with_library(&pool, owner, "Merge", &slug)
            .await
            .unwrap();
        let tag_id = Uuid::new_v4();
        sqlx::query("INSERT INTO tags (id, workspace_id, name, slug) VALUES ($1,$2,'iso','iso')")
            .bind(tag_id)
            .bind(seeded.workspace_id)
            .execute(&pool)
            .await
            .unwrap();
        let did = Uuid::new_v4();
        let file_hash = format!("merge-{did}");
        storage::insert_document(
            &pool,
            storage::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "cert",
                file_name: "cert.txt",
                file_size: 4,
                file_hash: &file_hash,
                object_key: "objects/merge",
            },
        )
        .await
        .unwrap();
        sqlx::query("INSERT INTO document_tags (document_id, tag_id) VALUES ($1,$2)")
            .bind(did)
            .bind(tag_id)
            .execute(&pool)
            .await
            .unwrap();
        let cid = Uuid::new_v4();
        storage::replace_document_chunks(
            &pool,
            did,
            &[domain::Chunk {
                id: cid,
                document_id: did,
                product_version_id: seeded.library_version_id,
                chunk_type: "text".into(),
                content: "iso certified".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 13,
                parent_chunk_id: None,
                generated_questions: vec![],
            }],
            &[domain::ChunkEmbedding {
                chunk_id: cid,
                product_version_id: seeded.library_version_id,
                document_id: did,
                content: "iso certified".into(),
                vector: vec![0.3; models::EMBEDDING_DIM],
                tsv: String::new(),
            }],
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO graph_nodes (product_version_id, document_id, name, chunk_ids)
             VALUES ($1,$2,'Widget',$3)",
        )
        .bind(seeded.library_version_id)
        .bind(did)
        .bind(&[cid][..])
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO graph_relations
                (product_version_id, document_id, node1, node2, rel_type)
             VALUES ($1,$2,'Widget','Spec','mentions')",
        )
        .bind(seeded.library_version_id)
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO wiki_pages (id, product_version_id, slug, title, content, status)
             VALUES ($1,$2,'overview','Overview','wiki body','published')",
        )
        .bind(Uuid::new_v4())
        .bind(seeded.library_version_id)
        .execute(&pool)
        .await
        .unwrap();

        let state = AppState {
            store: Arc::new(Mutex::new(Store::default())),
            jwt_secret: "secret".into(),
            bootstrap_key: String::new(),
        };
        assert!(state.store.lock().unwrap().workspaces.is_empty());
        ensure_workspace(&state, seeded.workspace_id).await;
        {
            let s = state.store.lock().unwrap();
            assert!(s.workspaces.contains_key(&seeded.workspace_id));
            assert!(
                s.tags.contains_key(&tag_id),
                "tags must survive merge_catalog"
            );
            assert!(s.document_tags.contains(&(did, tag_id)));
            assert!(s.chunks.contains_key(&cid));
            assert!(s.embeddings.contains_key(&cid));
            assert!(
                s.graph
                    .contains_key(&(seeded.library_version_id, did, "Widget".into()))
            );
            assert!(s.relations.contains_key(&(
                seeded.library_version_id,
                did,
                "Widget".into(),
                "Spec".into(),
                "mentions".into()
            )));
            assert!(
                s.wiki
                    .contains_key(&(seeded.library_version_id, "overview".into()))
            );
        }

        let catalog_only = AppState {
            store: Arc::new(Mutex::new(Store::default())),
            jwt_secret: "secret".into(),
            bootstrap_key: String::new(),
        };
        {
            let mut s = catalog_only.store.lock().unwrap();
            s.workspaces.insert(
                seeded.workspace_id,
                domain::Workspace {
                    id: seeded.workspace_id,
                    name: "Merge".into(),
                    slug: slug.clone(),
                    kind: Default::default(),
                    retrieval: domain::RetrievalConfig::default(),
                },
            );
            assert!(s.tags.is_empty());
            assert!(s.chunks.is_empty());
        }
        ensure_workspace(&catalog_only, seeded.workspace_id).await;
        let s = catalog_only.store.lock().unwrap();
        assert!(
            s.tags.contains_key(&tag_id),
            "catalog already present must still merge tags"
        );
        assert!(s.chunks.contains_key(&cid));
        assert!(s.embeddings.contains_key(&cid));
    }
}
