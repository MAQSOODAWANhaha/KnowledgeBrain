//! Host release entry point. Docker output can contain credentials; never echo it.
use async_trait::async_trait;
use platform::{ReleaseDescriptorV1, SchemaRuntimeIdentity};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

type Result<T> = std::result::Result<T, String>;
const MOUNT: &str = "/run/knowledgebrain/release-descriptor.json";
const ATTEMPT_LABEL: &str = "io.knowledgebrain.release-attempt";

struct Input {
    descriptor_path: PathBuf,
    env_path: PathBuf,
    compose_path: PathBuf,
    descriptor_bytes: Vec<u8>,
    env_bytes: Vec<u8>,
    compose_bytes: Vec<u8>,
    schema_path: PathBuf,
    schema_bytes: Vec<u8>,
    descriptor: ReleaseDescriptorV1,
    sha: String,
    namespace: String,
    project: String,
    env: BTreeMap<String, String>,
    timeout: u64,
    check_only: bool,
}

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|_| "required release input is unreadable".into())
}

impl Input {
    fn load(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut values = BTreeMap::new();
        let mut check_only = false;
        let mut args = args;
        while let Some(key) = args.next() {
            if key == "--check-only" && !check_only {
                check_only = true;
                continue;
            }
            if !matches!(
                key.as_str(),
                "--descriptor"
                    | "--env-file"
                    | "--compose-file"
                    | "--project-name"
                    | "--timeout-seconds"
            ) || values.contains_key(&key)
            {
                return Err("unknown or repeated release option".into());
            }
            values.insert(key, args.next().ok_or("release option requires a value")?);
        }
        let path = |key: &str| -> Result<PathBuf> {
            std::fs::canonicalize(
                values
                    .get(key)
                    .ok_or("descriptor, env-file and project-name are required")?,
            )
            .map_err(|_| "required release input is missing".into())
        };
        let descriptor_path = path("--descriptor")?;
        let env_path = path("--env-file")?;
        let compose_path = if values.contains_key("--compose-file") {
            path("--compose-file")?
        } else {
            std::fs::canonicalize("deploy/docker-compose.yml")
                .map_err(|_| "base Compose file is missing")?
        };
        let project = values
            .get("--project-name")
            .ok_or("explicit project-name is required")?
            .clone();
        if project.is_empty()
            || !project.bytes().enumerate().all(|(i, b)| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || (i > 0 && matches!(b, b'-' | b'_'))
            })
        {
            return Err("invalid explicit Compose project name".into());
        }
        let timeout = values
            .get("--timeout-seconds")
            .map(|s| s.parse::<u64>())
            .transpose()
            .map_err(|_| "invalid timeout")?
            .unwrap_or(300);
        if timeout == 0 {
            return Err("timeout must be positive".into());
        }
        let descriptor_bytes = read(&descriptor_path)?;
        let descriptor: ReleaseDescriptorV1 =
            serde_json::from_slice(&descriptor_bytes).map_err(|_| "invalid release descriptor")?;
        let sha = descriptor.sha256().map_err(|e| e.to_string())?;
        // The typed runtime validator is shared, not a second release grammar.
        // Refuse a tool built against a different checked-in schema contract.
        let schema_path = compose_path
            .parent()
            .ok_or("Compose parent missing")?
            .join("release-descriptor-v1.schema.json");
        let schema_bytes = read(&schema_path)?;
        let checked: Value = serde_json::from_slice(&schema_bytes)
            .map_err(|_| "invalid checked-in release schema")?;
        let embedded: Value = serde_json::from_str(platform::RELEASE_DESCRIPTOR_SCHEMA)
            .map_err(|_| "invalid embedded release schema")?;
        if checked != embedded {
            return Err("checked-in release schema differs from this tool".into());
        }
        let env_bytes = read(&env_path)?;
        let mut env = BTreeMap::new();
        for pair in dotenvy::from_read_iter(env_bytes.as_slice()) {
            let (key, value) = pair.map_err(|_| "invalid release env file")?;
            if env.insert(key, value).is_some() {
                return Err("duplicate release env key".into());
            }
        }
        let namespace = env
            .get(platform::DEPLOYMENT_NAMESPACE_ID_ENV)
            .ok_or("env file must explicitly set KB_DEPLOYMENT_NAMESPACE_ID")?
            .clone();
        let identity = SchemaRuntimeIdentity::from_descriptor(
            descriptor_path.clone(),
            descriptor.clone(),
            &sha,
            "migrator",
            descriptor
                .component_digest(platform::SchemaComponentKind::Migrator)
                .map_err(|e| e.to_string())?,
            &namespace,
        )
        .map_err(|e| e.to_string())?;
        if identity.deployment_namespace_id.to_string() != namespace {
            return Err("noncanonical namespace".into());
        }
        env.insert(
            "KB_RELEASE_DESCRIPTOR_FILE".into(),
            descriptor_path
                .to_str()
                .ok_or("non-UTF8 descriptor path")?
                .into(),
        );
        Ok(Self {
            compose_bytes: read(&compose_path)?,
            schema_path,
            schema_bytes,
            descriptor_path,
            env_path,
            compose_path,
            descriptor_bytes,
            env_bytes,
            descriptor,
            sha,
            namespace,
            project,
            env,
            timeout,
            check_only,
        })
    }

    fn unchanged(&self) -> Result<()> {
        if read(&self.descriptor_path)? != self.descriptor_bytes
            || read(&self.env_path)? != self.env_bytes
            || read(&self.compose_path)? != self.compose_bytes
            || read(&self.schema_path)? != self.schema_bytes
        {
            return Err("release inputs changed during verification".into());
        }
        Ok(())
    }

    fn components(&self) -> [(&str, &str, &str); 5] {
        let i = &self.descriptor.images;
        [
            ("migrate", "migrator", &i.migrator),
            ("api", "api", &i.api),
            ("worker", "worker", &i.worker),
            ("retention", "retention", &i.retention),
            ("docreader", "docreader", &i.docreader),
        ]
    }
}

#[async_trait]
trait Engine {
    async fn run(&mut self, args: &[String]) -> Result<String>;
}

struct Docker {
    env: BTreeMap<String, String>,
    timeout: Duration,
}
#[async_trait]
impl Engine for Docker {
    async fn run(&mut self, args: &[String]) -> Result<String> {
        let mut command = tokio::process::Command::new("docker");
        // Preserve host Engine access, but do not let stale shell application
        // settings override the explicit release env file in Compose.
        command.env_clear();
        for key in [
            "PATH",
            "HOME",
            "DOCKER_HOST",
            "DOCKER_CONTEXT",
            "DOCKER_CONFIG",
            "DOCKER_CERT_PATH",
            "DOCKER_TLS_VERIFY",
            "SSH_AUTH_SOCK",
            "XDG_RUNTIME_DIR",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "no_proxy",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command.args(args).envs(&self.env).kill_on_drop(true);
        let output = tokio::time::timeout(self.timeout, command.output())
            .await
            .map_err(|_| "Docker command timed out")?
            .map_err(|_| "Docker command could not start")?;
        if !output.status.success() {
            return Err(
                "Docker command failed; inspect local engine logs without publishing credentials"
                    .into(),
            );
        }
        String::from_utf8(output.stdout).map_err(|_| "Docker output is not UTF8".into())
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).into()).collect()
}
fn parsed(raw: &str) -> Result<Value> {
    serde_json::from_str(raw).map_err(|_| "invalid Docker JSON".into())
}
fn one(value: &Value) -> Result<&Value> {
    let array = value
        .as_array()
        .ok_or("expected one Engine inspect result")?;
    if array.len() != 1 {
        return Err("expected exactly one Engine inspect result".into());
    }
    Ok(&array[0])
}

fn image_id(value: &Value, expected: &str) -> Result<String> {
    let image = one(value)?;
    if !image["RepoDigests"]
        .as_array()
        .is_some_and(|ds| ds.iter().any(|d| d.as_str() == Some(expected)))
    {
        return Err("actual full RepoDigest does not match descriptor".into());
    }
    let id = image["Id"].as_str().ok_or("image config ID missing")?;
    if !id.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }) {
        return Err("invalid image config ID".into());
    }
    Ok(id.into())
}

fn verify_process(image: &Value, container: &Value) -> Result<()> {
    let image = &one(image)?["Config"];
    let container = &one(container)?["Config"];
    if container["Cmd"] != image["Cmd"] || container["Entrypoint"] != image["Entrypoint"] {
        return Err("component command differs from verified image".into());
    }
    Ok(())
}

fn verify_container(
    input: &Input,
    service: &str,
    kind: &str,
    image: &str,
    id: &str,
    value: &Value,
    expected: &Value,
) -> Result<()> {
    let container = one(value)?;
    if container["Image"] != id
        || container["Config"]["Image"] != image
        || container["Config"]["Labels"]["com.docker.compose.project"] != input.project
        || container["Config"]["Labels"]["com.docker.compose.service"] != service
    {
        return Err("container image or Compose scope mismatch".into());
    }
    if container["Name"] != format!("/{}-{service}", input.project) {
        return Err("container name is outside release project".into());
    }
    if kind == "migrator" {
        if container["State"]["Status"] != "exited" || container["State"]["ExitCode"] != 0 {
            return Err("migrator has not completed successfully".into());
        }
    } else if container["State"]["Running"] != true
        || container["State"]["Health"]["Status"] != "healthy"
    {
        return Err("runtime container is not healthy".into());
    }
    if kind != "migrator"
        && (expected["healthcheck"]["test"]
            .as_array()
            .is_none_or(|v| v.len() < 2)
            || container["Config"]["Healthcheck"]["Test"] != expected["healthcheck"]["test"])
    {
        return Err("container health probe differs from resolved Compose".into());
    }
    {
        let mut env = BTreeMap::new();
        for v in container["Config"]["Env"]
            .as_array()
            .ok_or("container environment missing")?
        {
            let (key, value) = v
                .as_str()
                .and_then(|s| s.split_once('='))
                .ok_or("invalid container environment")?;
            if env.insert(key, value).is_some() {
                return Err("duplicate container environment key".into());
            }
        }
        if let Some(expected_env) = expected.get("environment") {
            for (key, value) in expected_env
                .as_object()
                .ok_or("invalid resolved environment")?
            {
                if env.get(key.as_str()).copied() != value.as_str() || value.as_str().is_none() {
                    return Err("container environment differs from resolved Compose".into());
                }
            }
        }
        if matches!(kind, "migrator" | "api" | "worker" | "retention") {
            for (key, value) in [
                ("BIN", kind),
                (platform::RELEASE_DESCRIPTOR_PATH_ENV, MOUNT),
                (platform::RELEASE_DESCRIPTOR_SHA256_ENV, &input.sha),
                (platform::DEPLOYMENT_NAMESPACE_ID_ENV, &input.namespace),
                (platform::COMPONENT_KIND_ENV, kind),
                (
                    platform::COMPONENT_IMAGE_DIGEST_ENV,
                    image.split_once('@').ok_or("image digest missing")?.1,
                ),
            ] {
                if env.get(key) != Some(&value) {
                    return Err("container release environment mismatch".into());
                }
            }
            let mounts = container["Mounts"]
                .as_array()
                .ok_or("container mounts missing")?;
            let selected: Vec<_> = mounts
                .iter()
                .filter(|m| m["Destination"] == MOUNT)
                .collect();
            if selected.len() != 1
                || selected[0]["Type"] != "bind"
                || selected[0]["RW"] != false
                || selected[0]["Source"].as_str() != input.descriptor_path.to_str()
            {
                return Err("descriptor mount must be exact and read-only".into());
            }
        }
    }
    Ok(())
}

struct Override {
    directory: PathBuf,
    path: PathBuf,
}
impl Override {
    fn close(&self) -> Result<()> {
        std::fs::remove_file(&self.path).map_err(|_| "cannot remove private override")?;
        std::fs::remove_dir(&self.directory)
            .map_err(|_| "cannot remove private release directory")?;
        Ok(())
    }

    fn finish(self, outcome: Result<Value>) -> Result<Value> {
        self.close()?;
        outcome
    }

    fn create(input: &Input, attempt: &str, service_names: &[String]) -> Result<Self> {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let directory =
            std::env::temp_dir().join(format!("knowledgebrain-release-{}", Uuid::new_v4()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|_| "cannot create private release directory")?;
        let result = Self {
            path: directory.join("override.json"),
            directory,
        };
        let mut services = serde_json::Map::new();
        for service in service_names {
            services.insert(
                service.clone(),
                json!({"container_name":format!("{}-{service}", input.project),
                "labels":{ATTEMPT_LABEL:attempt}}),
            );
        }
        for (service, kind, image) in input.components() {
            let mut entry = json!({"image":image,"pull_policy":"never", "labels":{ATTEMPT_LABEL:attempt},
                "container_name":format!("{}-{service}", input.project)});
            if kind != "docreader" {
                entry["environment"] = json!({"BIN":kind,"KB_RELEASE_DESCRIPTOR_PATH":MOUNT,"KB_RELEASE_DESCRIPTOR_SHA256":input.sha,
                    "KB_DEPLOYMENT_NAMESPACE_ID":input.namespace,"KB_COMPONENT_KIND":kind,
                    "KB_COMPONENT_IMAGE_DIGEST":image.split_once('@').ok_or("image digest missing")?.1});
                entry["volumes"] = json!([{"type":"bind","source":input.descriptor_path,"target":MOUNT,"read_only":true}]);
            }
            services.insert(service.into(), entry);
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&result.path)
            .map_err(|_| "cannot create private override")?;
        file.write_all(
            &serde_json::to_vec(&json!({"services":services})).map_err(|_| "invalid override")?,
        )
        .map_err(|_| "cannot write override")?;
        file.sync_all().map_err(|_| "cannot sync override")?;
        Ok(result)
    }
}
impl Drop for Override {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

fn compose(input: &Input, extra: &Override, tail: &[&str]) -> Vec<String> {
    let mut args = vec![
        "compose".into(),
        "--project-name".into(),
        input.project.clone(),
        "--env-file".into(),
        input.env_path.to_string_lossy().into(),
        "-f".into(),
        input.compose_path.to_string_lossy().into(),
        "-f".into(),
        extra.path.to_string_lossy().into(),
        "--profile".into(),
        "runtime".into(),
    ];
    args.extend(strings(tail));
    args
}

async fn container_id(
    engine: &mut impl Engine,
    input: &Input,
    extra: &Override,
    service: &str,
) -> Result<Option<String>> {
    let raw = engine
        .run(&compose(input, extra, &["ps", "--all", "--quiet", service]))
        .await?;
    let ids: Vec<_> = raw.split_whitespace().collect();
    match ids.as_slice() {
        [] => Ok(None),
        [id] => Ok(Some((*id).into())),
        _ => Err("expected at most one container per component".into()),
    }
}

async fn inspect(
    engine: &mut impl Engine,
    input: &Input,
    extra: &Override,
    require_all: bool,
    config: &Value,
) -> Result<bool> {
    let mut complete = true;
    for (service, kind, image) in input.components() {
        let Some(id) = container_id(engine, input, extra, service).await? else {
            complete = false;
            continue;
        };
        let image_value = parsed(&engine.run(&strings(&["image", "inspect", image])).await?)?;
        let actual = image_id(&image_value, image)?;
        let container = parsed(&engine.run(&strings(&["container", "inspect", &id])).await?)?;
        verify_container(
            input,
            service,
            kind,
            image,
            &actual,
            &container,
            &config["services"][service],
        )?;
        verify_process(&image_value, &container)?;
        if kind != "migrator" {
            probe(
                engine,
                &id,
                &config["services"][service]["healthcheck"]["test"],
            )
            .await?;
        }
    }
    if require_all && !complete {
        return Err("release component is missing".into());
    }
    Ok(complete)
}

fn dependency_digest(image: &str) -> Result<String> {
    let (repository, digest) = image
        .split_once('@')
        .ok_or("dependency must be digest pinned")?;
    let (prefix, name) = repository.rsplit_once('/').unwrap_or(("", repository));
    let name = name.split(':').next().ok_or("invalid dependency image")?;
    let full = if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    };
    // Docker reports familiar official-image names exactly as in the pull ref.
    if digest.len() != 71
        || !digest.starts_with("sha256:")
        || !digest[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("dependency must be digest pinned".into());
    }
    Ok(format!("{full}@{digest}"))
}

fn probe_args(id: &str, test: &Value) -> Result<Vec<String>> {
    let test: Vec<&str> = test
        .as_array()
        .ok_or("health probe missing")?
        .iter()
        .map(|s| s.as_str().ok_or("invalid health probe"))
        .collect::<std::result::Result<_, _>>()?;
    let mut args = strings(&["exec", id]);
    match test.as_slice() {
        ["CMD", command, rest @ ..] if !command.is_empty() => {
            args.push((*command).into());
            args.extend(strings(rest));
        }
        ["CMD-SHELL", command] if !command.is_empty() => {
            args.extend(strings(&["/bin/sh", "-c", command]))
        }
        _ => return Err("unsupported health probe".into()),
    }
    Ok(args)
}

async fn probe(engine: &mut impl Engine, id: &str, test: &Value) -> Result<()> {
    engine.run(&probe_args(id, test)?).await?;
    Ok(())
}

async fn release(input: &Input, engine: &mut impl Engine) -> Result<Value> {
    let attempt = Uuid::new_v4().to_string();
    let extra = Override::create(input, &attempt, &[])?;
    input.unchanged()?;
    if input.check_only {
        return extra.finish(Ok(
            json!({"status":"descriptor_validated","release_descriptor_sha256":input.sha,
            "images_verified":false,"deployed":false}),
        ));
    }
    // Resolve first to discover dependency names, then scope all containers and
    // cleanup labels to this explicit project. Both config calls are read-only.
    let base = parsed(
        &engine
            .run(&compose(input, &extra, &["config", "--format", "json"]))
            .await?,
    )?;
    let names: Vec<_> = base["services"]
        .as_object()
        .ok_or("resolved Compose services missing")?
        .keys()
        .cloned()
        .collect();
    extra.close()?;
    let extra = Override::create(input, &attempt, &names)?;
    let config = parsed(
        &engine
            .run(&compose(input, &extra, &["config", "--format", "json"]))
            .await?,
    )?;
    let services = config["services"]
        .as_object()
        .ok_or("resolved Compose services missing")?;
    if names != services.keys().cloned().collect::<Vec<_>>() {
        return Err("Compose service set changed during verification".into());
    }
    for (service, kind, image) in input.components() {
        let expected = &config["services"][service];
        if expected["image"] != image
            || !expected["command"].is_null()
            || !expected["entrypoint"].is_null()
        {
            return Err("resolved component image or command mismatch".into());
        }
        if kind != "migrator" {
            probe_args("validation", &expected["healthcheck"]["test"])?;
        }
    }
    for (name, service) in services {
        if service["container_name"] != format!("{}-{name}", input.project)
            || service["labels"][ATTEMPT_LABEL] != attempt
        {
            return Err("resolved container scope mismatch".into());
        }
    }
    let dependencies: Vec<&str> = services
        .keys()
        .map(String::as_str)
        .filter(|s| !input.components().iter().any(|(name, _, _)| name == s))
        .collect();
    // Refuse stale dependency containers before pulls, migrations or starts.
    let mut dependencies_complete = true;
    for service in &dependencies {
        let expected = &services[*service];
        let image = expected["image"]
            .as_str()
            .ok_or("dependency image missing")?;
        dependency_digest(image)?;
        if let Some(id) = container_id(engine, input, &extra, service).await? {
            let actual = image_id(
                &parsed(&engine.run(&strings(&["image", "inspect", image])).await?)?,
                &dependency_digest(image)?,
            )?;
            verify_container(
                input,
                service,
                "dependency",
                image,
                &actual,
                &parsed(&engine.run(&strings(&["container", "inspect", &id])).await?)?,
                expected,
            )?;
        } else {
            dependencies_complete = false;
        }
    }
    // Existing components must already match. No implicit replacement of an
    // existing deployment, and an already healthy release is read-only.
    if inspect(engine, input, &extra, false, &config).await? && dependencies_complete {
        input.unchanged()?;
        return extra.finish(Ok(
            json!({"status":"already_healthy","release_descriptor_sha256":input.sha,"deployed":false}),
        ));
    }
    let result = async {
        for (_, _, image) in input.components() {
            input.unchanged()?;
            engine.run(&strings(&["pull", image])).await?;
            image_id(
                &parsed(&engine.run(&strings(&["image", "inspect", image])).await?)?,
                image,
            )?;
        }
        for service in &dependencies {
            input.unchanged()?;
            let image = services[*service]["image"]
                .as_str()
                .ok_or("dependency image missing")?;
            engine.run(&strings(&["pull", image])).await?;
            image_id(
                &parsed(&engine.run(&strings(&["image", "inspect", image])).await?)?,
                &dependency_digest(image)?,
            )?;
        }
        let mut dependencies = dependencies.clone();
        dependencies.push("docreader");
        let timeout = input.timeout.to_string();
        if !dependencies.is_empty() {
            let mut up = vec![
                "up",
                "-d",
                "--wait",
                "--wait-timeout",
                &timeout,
                "--no-build",
                "--pull",
                "never",
                "--no-recreate",
                "--no-deps",
            ];
            up.extend(dependencies.iter().copied());
            input.unchanged()?;
            engine.run(&compose(input, &extra, &up)).await?;
        }
        if container_id(engine, input, &extra, "migrate")
            .await?
            .is_none()
        {
            input.unchanged()?;
            engine
                .run(&compose(
                    input,
                    &extra,
                    &[
                        "up",
                        "--no-deps",
                        "--no-build",
                        "--pull",
                        "never",
                        "--no-recreate",
                        "--abort-on-container-exit",
                        "--exit-code-from",
                        "migrate",
                        "migrate",
                    ],
                ))
                .await?;
        }
        let migration = container_id(engine, input, &extra, "migrate")
            .await?
            .ok_or("migrator disappeared")?;
        let image = &input.descriptor.images.migrator;
        let migration_image = parsed(&engine.run(&strings(&["image", "inspect", image])).await?)?;
        let actual = image_id(&migration_image, image)?;
        let migration_container = parsed(
            &engine
                .run(&strings(&["container", "inspect", &migration]))
                .await?,
        )?;
        verify_container(
            input,
            "migrate",
            "migrator",
            image,
            &actual,
            &migration_container,
            &config["services"]["migrate"],
        )?;
        verify_process(&migration_image, &migration_container)?;
        input.unchanged()?;
        engine
            .run(&compose(
                input,
                &extra,
                &[
                    "up",
                    "-d",
                    "--wait",
                    "--wait-timeout",
                    &timeout,
                    "--no-build",
                    "--pull",
                    "never",
                    "--no-recreate",
                    "--no-deps",
                    "api",
                    "worker",
                    "retention",
                ],
            ))
            .await?;
        inspect(engine, input, &extra, true, &config).await?;
        for service in dependencies.iter().filter(|s| **s != "docreader") {
            let expected = &services[*service];
            let image = expected["image"]
                .as_str()
                .ok_or("dependency image missing")?;
            let id = container_id(engine, input, &extra, service)
                .await?
                .ok_or("dependency disappeared")?;
            let actual = image_id(
                &parsed(&engine.run(&strings(&["image", "inspect", image])).await?)?,
                &dependency_digest(image)?,
            )?;
            verify_container(
                input,
                service,
                "dependency",
                image,
                &actual,
                &parsed(&engine.run(&strings(&["container", "inspect", &id])).await?)?,
                expected,
            )?;
        }
        input.unchanged()?;
        Ok(json!({"status":"healthy","release_descriptor_sha256":input.sha,"deployed":true}))
    }
    .await;
    if result.is_err() {
        // Stop only containers labelled by this invocation. Preserve
        // dependency data and pre-existing components; never remove volumes.
        let filter = format!("label={ATTEMPT_LABEL}={attempt}");
        let cleanup = async {
            let ids = engine
                .run(&strings(&[
                    "ps",
                    "-aq",
                    "--filter",
                    &filter,
                    "--filter",
                    &format!("label=com.docker.compose.project={}", input.project),
                ]))
                .await?;
            if !ids.trim().is_empty() {
                let mut stop = strings(&["stop"]);
                stop.extend(ids.split_whitespace().map(str::to_owned));
                engine.run(&stop).await?;
            }
            Ok::<(), String>(())
        }
        .await;
        if cleanup.is_err() {
            return Err("release failed and stopping this invocation's components failed; inspect the labelled containers".into());
        }
    }
    extra.finish(result)
}

#[tokio::main]
async fn main() {
    let outcome = async {
        let input = Input::load(std::env::args().skip(1))?;
        let mut engine = Docker {
            env: input.env.clone(),
            timeout: Duration::from_secs(input.timeout),
        };
        release(&input, &mut engine).await
    }
    .await;
    match outcome {
        Ok(value) => println!("{value}"),
        Err(message) => {
            eprintln!("RELEASE_NOT_READY: {message}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
#[path = "knowledgebrain-release/tests.rs"]
mod tests;
