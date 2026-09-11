use super::*;
use std::os::unix::fs::PermissionsExt;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("kb-release-test-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("descriptor.json"),
            include_str!("../../../../../deploy/release-descriptor-v1.development.json"),
        )
        .unwrap();
        std::fs::write(
            path.join("release-descriptor-v1.schema.json"),
            platform::RELEASE_DESCRIPTOR_SCHEMA,
        )
        .unwrap();
        std::fs::write(path.join("compose.yml"), "services: {}\n").unwrap();
        std::fs::write(
            path.join("release.env"),
            "KB_DEPLOYMENT_NAMESPACE_ID=123e4567-e89b-12d3-a456-426614174000\nTOKEN=test-secret\n",
        )
        .unwrap();
        Self(path)
    }
    fn args(&self) -> Vec<String> {
        vec![
            "--descriptor".into(),
            self.0.join("descriptor.json").to_str().unwrap().into(),
            "--env-file".into(),
            self.0.join("release.env").to_str().unwrap().into(),
            "--compose-file".into(),
            self.0.join("compose.yml").to_str().unwrap().into(),
            "--project-name".into(),
            "isolated-release".into(),
        ]
    }
    fn input(&self) -> Input {
        Input::load(self.args().into_iter()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn config(input: &Input) -> Value {
    let mut config = json!({"services":{}});
    for (service, kind, image) in input.components() {
        let mut value = json!({"image":image,"environment":{},"container_name":format!("{}-{service}",input.project)});
        if kind != "migrator" {
            value["healthcheck"] = json!({"test":["CMD","health-probe",kind]});
        }
        config["services"][service] = value;
    }
    config["services"]["postgres"] = json!({"image":format!("registry.test/pg:v1@sha256:{}","a".repeat(64)),
        "container_name":format!("{}-postgres",input.project),"environment":{"POSTGRES_DB":"release-test"},
        "healthcheck":{"test":["CMD","pg_isready"]}});
    config
}

fn image_value(image: &str) -> Value {
    json!([{"Id":format!("sha256:{}","f".repeat(64)), "RepoDigests":[dependency_digest(image).unwrap()],
        "Config":{"Cmd":null,"Entrypoint":["/image-entrypoint"]}}])
}

fn container_value(input: &Input, service: &str, expected: &Value) -> Value {
    let env: Vec<_> = expected["environment"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| format!("{k}={}", v.as_str().unwrap()))
        .collect();
    json!([{"Id":service,"Name":format!("/{}-{service}",input.project),"Image":format!("sha256:{}","f".repeat(64)),
        "Config":{"Image":expected["image"],"Labels":{"com.docker.compose.project":input.project,"com.docker.compose.service":service},
            "Env":env,"Cmd":null,"Entrypoint":["/image-entrypoint"],"Healthcheck":{"Test":expected["healthcheck"]["test"]}},
        "Mounts":[{"Destination":MOUNT,"Source":input.descriptor_path,"Type":"bind","RW":false}],
        "State":{"Status":if service=="migrate" {"exited"} else {"running"},"Running":service!="migrate","ExitCode":0,"Health":{"Status":"healthy"}}}])
}

struct FakeEngine {
    input: Input,
    config: Value,
    containers: BTreeMap<String, Value>,
    calls: Vec<Vec<String>>,
    overrides: Vec<PathBuf>,
    created: Vec<String>,
    stopped: Vec<String>,
    fault: Option<&'static str>,
}
impl FakeEngine {
    fn new(input: Input) -> Self {
        let config = config(&input);
        Self {
            input,
            config,
            containers: BTreeMap::new(),
            calls: vec![],
            overrides: vec![],
            created: vec![],
            stopped: vec![],
            fault: None,
        }
    }
    fn mutations(&self) -> Vec<&Vec<String>> {
        self.calls
            .iter()
            .filter(|a| a[0] == "pull" || a[0] == "stop" || a.iter().any(|s| s == "up"))
            .collect()
    }
    fn add(&mut self, service: &str) {
        self.containers.insert(
            service.into(),
            container_value(&self.input, service, &self.config["services"][service]),
        );
    }
    fn load_override(&mut self, path: &Path) {
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        self.overrides.push(path.into());
        let value: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for (name, entry) in value["services"].as_object().unwrap() {
            let current = self.config["services"][name].as_object_mut().unwrap();
            current.extend(entry.as_object().unwrap().clone());
        }
    }
    fn assert_clean(&self) {
        assert!(
            self.overrides
                .iter()
                .all(|p| !p.exists() && !p.parent().unwrap().exists())
        );
        self.input.unchanged().unwrap();
    }
}

#[async_trait]
impl Engine for FakeEngine {
    async fn run(&mut self, args: &[String]) -> Result<String> {
        self.calls.push(args.to_vec());
        if args[0] == "compose" {
            let files: Vec<_> = args
                .windows(2)
                .filter(|a| a[0] == "-f")
                .map(|a| a[1].clone())
                .collect();
            self.load_override(Path::new(files.last().unwrap()));
            if args.iter().any(|s| s == "config") {
                return Ok(self.config.to_string());
            }
            if args.iter().any(|s| s == "ps") {
                let service = args.last().unwrap();
                return Ok(if self.containers.contains_key(service) {
                    service.clone()
                } else {
                    String::new()
                });
            }
            if args.iter().any(|s| s == "up") {
                assert!(args.iter().any(|s| s == "--no-recreate"));
                let names: Vec<_> = args
                    .iter()
                    .filter(|s| self.config["services"].get(s.as_str()).is_some())
                    .cloned()
                    .collect();
                if names.iter().any(|s| s == "api") {
                    assert_eq!(self.containers["migrate"][0]["State"]["ExitCode"], 0);
                    let checked = self
                        .calls
                        .iter()
                        .rposition(|a| a == &strings(&["container", "inspect", "migrate"]))
                        .unwrap();
                    assert!(checked < self.calls.len() - 1);
                }
                for name in names {
                    if !self.containers.contains_key(&name) {
                        self.add(&name);
                        self.created.push(name);
                    }
                }
                if self.fault == Some("migrator") && self.containers.contains_key("migrate") {
                    self.containers.get_mut("migrate").unwrap()[0]["State"]["ExitCode"] = json!(1);
                }
                if self.fault == Some("runtime") && self.containers.contains_key("api") {
                    return Err("simulated runtime failure".into());
                }
                return Ok(String::new());
            }
        }
        if args[0] == "pull" {
            if self.fault == Some("pull") {
                return Err("simulated pull failure".into());
            }
            if self.fault == Some("drift") {
                std::fs::write(&self.input.env_path, "changed").unwrap();
            }
            return Ok(String::new());
        }
        if args[0] == "image" {
            return Ok(image_value(&args[2]).to_string());
        }
        if args[0] == "container" {
            let mut value = self.containers[&args[2]].clone();
            if self.fault == Some("command") && args[2] == "migrate" {
                value[0]["Config"]["Cmd"] = json!(["true"]);
            }
            return Ok(value.to_string());
        }
        if args[0] == "exec" {
            if self.fault == Some("probe") {
                return Err("simulated probe failure".into());
            }
            return Ok(String::new());
        }
        if args[0] == "ps" {
            assert_eq!(args.iter().filter(|s| s.as_str() == "--filter").count(), 2);
            assert!(
                args.iter()
                    .any(|s| s.starts_with(&format!("label={ATTEMPT_LABEL}=")))
            );
            assert!(args.contains(&format!(
                "label=com.docker.compose.project={}",
                self.input.project
            )));
            return Ok(self.created.join("\n"));
        }
        if args[0] == "stop" {
            self.stopped.extend(args[1..].iter().cloned());
            return Ok(String::new());
        }
        panic!("unexpected scripted Engine command: {args:?}");
    }
}

#[tokio::test]
async fn check_only_is_offline_and_removes_private_override() {
    let f = Fixture::new();
    let mut input = f.input();
    input.check_only = true;
    let mut engine = FakeEngine::new(f.input());
    assert_eq!(
        release(&input, &mut engine).await.unwrap()["status"],
        "descriptor_validated"
    );
    assert!(engine.calls.is_empty());
    input.unchanged().unwrap();
}

#[test]
fn private_override_scopes_every_service_sets_role_and_is_removed() {
    let f = Fixture::new();
    let input = f.input();
    let extra = Override::create(&input, "test-attempt", &["postgres".into()]).unwrap();
    let path = extra.path.clone();
    let mut engine = FakeEngine::new(f.input());
    engine.load_override(&path);
    assert_eq!(
        engine.config["services"]["postgres"]["labels"][ATTEMPT_LABEL],
        "test-attempt"
    );
    assert_eq!(
        engine.config["services"]["retention"]["environment"]["BIN"],
        "retention"
    );
    drop(extra);
    engine.assert_clean();
}

#[tokio::test]
async fn fresh_start_verifies_migration_before_runtime_and_replay_is_read_only() {
    let f = Fixture::new();
    let input = f.input();
    let mut engine = FakeEngine::new(f.input());
    assert_eq!(
        release(&input, &mut engine).await.unwrap()["status"],
        "healthy"
    );
    assert_eq!(engine.created.len(), 6);
    assert_eq!(engine.calls.iter().filter(|a| a[0] == "pull").count(), 6);
    assert!(engine.stopped.is_empty());
    engine.assert_clean();
    engine.calls.clear();
    engine.created.clear();
    assert_eq!(
        release(&input, &mut engine).await.unwrap()["status"],
        "already_healthy"
    );
    assert!(engine.mutations().is_empty());
    assert_eq!(engine.calls.iter().filter(|a| a[0] == "exec").count(), 4);
    engine.assert_clean();
}

#[tokio::test]
async fn dependency_without_explicit_environment_starts_and_replays() {
    let f = Fixture::new();
    let input = f.input();
    let mut engine = FakeEngine::new(f.input());
    engine.config["services"]["postgres"]
        .as_object_mut()
        .unwrap()
        .remove("environment");
    assert_eq!(
        release(&input, &mut engine).await.unwrap()["status"],
        "healthy"
    );
    engine.calls.clear();
    assert_eq!(
        release(&input, &mut engine).await.unwrap()["status"],
        "already_healthy"
    );
    assert!(engine.mutations().is_empty());
    engine.assert_clean();
}

#[tokio::test]
async fn failures_stop_only_this_attempt_and_preserve_inputs() {
    for fault in ["pull", "migrator", "command", "runtime", "probe"] {
        let f = Fixture::new();
        let input = f.input();
        let mut engine = FakeEngine::new(f.input());
        engine.fault = Some(fault);
        // A pre-existing dependency must never be in this attempt's stop set.
        engine.add("postgres");
        assert!(release(&input, &mut engine).await.is_err(), "{fault}");
        assert_eq!(engine.created, engine.stopped, "{fault}");
        assert!(!engine.stopped.iter().any(|s| s == "postgres"));
        if matches!(fault, "migrator" | "command" | "pull") {
            assert!(!engine.created.iter().any(|s| s == "api"));
        }
        engine.assert_clean();
    }
}

#[tokio::test]
async fn stale_dependency_is_rejected_before_any_mutation() {
    let f = Fixture::new();
    let input = f.input();
    let mut engine = FakeEngine::new(f.input());
    engine.add("postgres");
    engine.containers.get_mut("postgres").unwrap()[0]["Config"]["Labels"]["com.docker.compose.project"] =
        json!("other");
    assert!(release(&input, &mut engine).await.is_err());
    assert!(engine.mutations().is_empty());
    engine.assert_clean();
}

#[tokio::test]
async fn stale_component_or_faked_probe_rejects_replay_without_mutations() {
    for (pointer, value) in [
        ("/0/Config/Healthcheck/Test", json!(["CMD", "true"])),
        ("/0/Config/Cmd", json!(["true"])),
        ("/0/Config/Env", json!(["BIN=worker"])),
        ("/0/Mounts/0/RW", json!(true)),
    ] {
        let f = Fixture::new();
        let input = f.input();
        let mut engine = FakeEngine::new(f.input());
        release(&input, &mut engine).await.unwrap();
        *engine
            .containers
            .get_mut("api")
            .unwrap()
            .pointer_mut(pointer)
            .unwrap() = value;
        engine.calls.clear();
        engine.created.clear();
        assert!(release(&input, &mut engine).await.is_err(), "{pointer}");
        assert!(engine.mutations().is_empty(), "{pointer}");
        engine.assert_clean();
    }
}

#[tokio::test]
async fn input_drift_stops_before_start() {
    let f = Fixture::new();
    let input = f.input();
    let mut engine = FakeEngine::new(f.input());
    engine.fault = Some("drift");
    assert_eq!(
        release(&input, &mut engine).await.unwrap_err(),
        "release inputs changed during verification"
    );
    assert!(engine.created.is_empty());
    assert!(engine.overrides.iter().all(|p| !p.exists()));
}

#[test]
fn full_repo_digest_is_required_and_config_id_is_distinct() {
    let image = format!("registry.test/api@sha256:{}", "a".repeat(64));
    let value = image_value(&image);
    assert_eq!(
        image_id(&value, &image).unwrap(),
        format!("sha256:{}", "f".repeat(64))
    );
    assert!(image_id(&value, &image.replace("registry.test", "foreign.test")).is_err());
    assert!(image_id(&json!([]), &image).is_err());
    assert!(image_id(&json!([value[0], value[0]]), &image).is_err());
}

#[test]
fn typed_descriptor_validator_matches_checked_schema_for_release_inputs() {
    let f = Fixture::new();
    let valid = serde_json::to_value(f.input().descriptor).unwrap();
    let schema: Value = serde_json::from_str(platform::RELEASE_DESCRIPTOR_SCHEMA).unwrap();
    let validator = jsonschema::JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .compile(&schema)
        .unwrap();
    let mut values = vec![valid.clone()];
    for field in [
        "release_revision",
        "platform_schema_revision",
        "git_sha",
        "schema_version",
        "images",
    ] {
        for value in [
            Value::Null,
            json!(true),
            json!(0),
            json!(""),
            json!([]),
            json!({}),
            json!("has space"),
            json!("A".repeat(40)),
            json!("x".repeat(129)),
        ] {
            let mut changed = valid.clone();
            changed[field] = value;
            values.push(changed);
        }
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(field);
        values.push(missing);
    }
    for repository in [
        "registry.test/path",
        "registry.test:5000/path",
        "registry.test:12:34/path",
        "registry.test:/path",
        "registry.test/path:tag",
        "registry.test//path",
        "Registry.test/path",
        "registry.test",
        "registry.test/path with space",
    ] {
        let mut changed = valid.clone();
        changed["images"]["api"] = json!(format!("{repository}@sha256:{}", "a".repeat(64)));
        values.push(changed);
    }
    for key in ["api", "worker", "retention", "migrator", "docreader"] {
        let mut missing = valid.clone();
        missing["images"].as_object_mut().unwrap().remove(key);
        values.push(missing);
    }
    for value in values {
        let typed = serde_json::from_value::<ReleaseDescriptorV1>(value.clone())
            .is_ok_and(|v| v.validate().is_ok());
        assert_eq!(typed, validator.is_valid(&value), "{value}");
    }
}

#[test]
fn cleanup_failure_cannot_be_reported_as_success() {
    let f = Fixture::new();
    let extra = Override::create(&f.input(), "attempt", &[]).unwrap();
    let dir = extra.directory.clone();
    std::fs::write(dir.join("unexpected"), "test").unwrap();
    assert!(extra.finish(Ok(json!({"status":"healthy"}))).is_err());
    std::fs::remove_file(dir.join("unexpected")).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn role_identity_mount_health_and_endpoint_mismatches_are_rejected() {
    let f = Fixture::new();
    let input = f.input();
    let extra = Override::create(&input, "attempt", &[]).unwrap();
    let mut engine = FakeEngine::new(f.input());
    engine.load_override(&extra.path);
    let expected = &engine.config["services"]["api"];
    let valid = container_value(&input, "api", expected);
    let image = input.descriptor.images.api.as_str();
    let id = format!("sha256:{}", "f".repeat(64));
    verify_container(&input, "api", "api", image, &id, &valid, expected).unwrap();
    for (pointer, value) in [
        ("/0/Image", json!("wrong")),
        ("/0/Config/Image", json!("tag:latest")),
        (
            "/0/Config/Labels/com.docker.compose.project",
            json!("foreign"),
        ),
        (
            "/0/Config/Labels/com.docker.compose.service",
            json!("worker"),
        ),
        ("/0/Name", json!("/foreign-api")),
        ("/0/Mounts/0/RW", json!(true)),
        ("/0/Mounts/0/Source", json!("/other/descriptor")),
        ("/0/Mounts", json!([])),
        ("/0/Config/Healthcheck/Test", json!(["CMD", "true"])),
        ("/0/State/Health/Status", json!("unhealthy")),
        ("/0/State/Running", json!(false)),
    ] {
        let mut changed = valid.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            verify_container(&input, "api", "api", image, &id, &changed, expected).is_err(),
            "{pointer}"
        );
    }
    for key in [
        "BIN",
        platform::COMPONENT_KIND_ENV,
        platform::COMPONENT_IMAGE_DIGEST_ENV,
        platform::RELEASE_DESCRIPTOR_SHA256_ENV,
        platform::DEPLOYMENT_NAMESPACE_ID_ENV,
        platform::RELEASE_DESCRIPTOR_PATH_ENV,
    ] {
        let mut changed = valid.clone();
        let env = changed[0]["Config"]["Env"].as_array_mut().unwrap();
        let value = env
            .iter_mut()
            .find(|v| v.as_str().unwrap().starts_with(&format!("{key}=")))
            .unwrap();
        *value = json!(format!("{key}=wrong"));
        assert!(
            verify_container(&input, "api", "api", image, &id, &changed, expected).is_err(),
            "{key}"
        );
    }
    let mut duplicate = valid.clone();
    let first = duplicate[0]["Config"]["Env"][0].clone();
    duplicate[0]["Config"]["Env"]
        .as_array_mut()
        .unwrap()
        .push(first);
    assert!(verify_container(&input, "api", "api", image, &id, &duplicate, expected).is_err());
}

#[test]
fn invalid_inputs_and_schema_drift_fail_without_engine_access() {
    for invalid in [
        "",
        "KB_DEPLOYMENT_NAMESPACE_ID=INVALID\n",
        "KB_DEPLOYMENT_NAMESPACE_ID=123E4567-E89B-12D3-A456-426614174000\n",
        "TOKEN=a\nTOKEN=b\n",
    ] {
        let f = Fixture::new();
        std::fs::write(f.0.join("release.env"), invalid).unwrap();
        assert!(Input::load(f.args().into_iter()).is_err());
    }
    let f = Fixture::new();
    std::fs::write(f.0.join("release-descriptor-v1.schema.json"), "{}").unwrap();
    assert!(Input::load(f.args().into_iter()).is_err());
    let f = Fixture::new();
    let valid = f.input().descriptor_bytes;
    let text = String::from_utf8(valid).unwrap();
    std::fs::write(
        f.0.join("descriptor.json"),
        text.replacen('{', "{\"schema_version\":1,", 1),
    )
    .unwrap();
    assert!(Input::load(f.args().into_iter()).is_err());
    for extra in [
        vec!["--check-only", "--check-only"],
        vec!["--timeout-seconds", "0"],
        vec!["--project-name", "other"],
        vec!["--unknown", "secret"],
    ] {
        let f = Fixture::new();
        let mut args = f.args();
        args.extend(strings(&extra));
        assert!(Input::load(args.into_iter()).is_err());
    }
}

#[test]
fn all_captured_input_files_are_checked_for_drift() {
    for name in [
        "descriptor.json",
        "release.env",
        "compose.yml",
        "release-descriptor-v1.schema.json",
    ] {
        let f = Fixture::new();
        let input = f.input();
        std::fs::write(f.0.join(name), "changed").unwrap();
        assert!(input.unchanged().is_err(), "{name}");
    }
}

#[tokio::test]
async fn docker_errors_never_echo_output_and_timeout_is_bounded() {
    let f = Fixture::new();
    let path = f.0.join("docker");
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf 'secret-stdout'\nprintf 'secret-stderr' >&2\nexit 9\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut docker = Docker {
        env: BTreeMap::from([("PATH".into(), f.0.to_str().unwrap().into())]),
        timeout: Duration::from_millis(100),
    };
    let error = docker.run(&[]).await.unwrap_err();
    assert!(!error.contains("secret"));
    std::fs::write(&path, "#!/bin/sh\nexec /bin/sleep 5\n").unwrap();
    assert_eq!(
        docker.run(&[]).await.unwrap_err(),
        "Docker command timed out"
    );
}
